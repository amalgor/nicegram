use anyhow::{Context, Result};
use grammers_client::{Client, SignInError};
use grammers_client::client::{LoginToken, PasswordToken};
use grammers_mtsender::SenderPool;
use grammers_session::updates::UpdatesLike;
use hydra_config::TelegramConfig;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

/// Authentication state machine for the Telegram login flow.
/// The mobile UI drives this flow by calling methods in sequence.
#[derive(Debug, Clone)]
pub enum AuthState {
    /// Not connected yet
    Disconnected,
    /// Need phone number from the user
    NeedPhone,
    /// Need login code (sent via Telegram/SMS)
    NeedCode,
    /// Need 2FA password
    NeedPassword { hint: String },
    /// Fully authorized and ready
    Authorized { user_name: String },
    /// Error during auth
    Error(String),
}

#[derive(Debug, Clone)]
pub struct TelegramUserIdentity {
    pub user_id: i64,
    pub user_name: String,
}

/// Wraps grammers Client with session management and auth state.
/// Owns the SenderPool runner lifecycle and provides the Client handle.
pub struct TelegramClient {
    client: Option<Client>,
    config: TelegramConfig,
    state: AuthState,
    login_token: Option<LoginToken>,
    password_token: Option<PasswordToken>,
    updates_rx: Option<mpsc::UnboundedReceiver<UpdatesLike>>,
}

impl TelegramClient {
    pub fn new(config: TelegramConfig) -> Self {
        Self {
            client: None,
            config,
            state: AuthState::Disconnected,
            login_token: None,
            password_token: None,
            updates_rx: None,
        }
    }

    pub fn state(&self) -> &AuthState {
        &self.state
    }

    pub fn client(&self) -> Option<&Client> {
        self.client.as_ref()
    }

    pub async fn current_user_identity(&self) -> Result<Option<TelegramUserIdentity>> {
        let Some(client) = &self.client else {
            return Ok(None);
        };
        if !client.is_authorized().await? {
            return Ok(None);
        }

        let me = client.get_me().await?;
        Ok(Some(TelegramUserIdentity {
            user_id: me.id().bare_id() as i64,
            user_name: me.first_name().unwrap_or("User").to_string(),
        }))
    }

    /// Connect to Telegram servers and spawn the sender pool runner.
    /// Does NOT log in — just establishes the MTProto connection.
    pub async fn connect(&mut self) -> Result<()> {
        if self.config.api_id == 0 || self.config.api_hash.is_empty() {
            self.state = AuthState::Error(
                "Telegram API credentials not configured. Set [telegram] api_id and api_hash in hydra.toml.".to_string()
            );
            return Err(anyhow::anyhow!(
                "Telegram API credentials not configured. Set [telegram] api_id and api_hash in hydra.toml."
            ));
        }

        info!("Connecting to Telegram (api_id={})...", self.config.api_id);

        let session = Arc::new(
            grammers_session::storages::SqliteSession::open(&self.config.session_path)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to open Telegram session file: {}", e))?
        );

        let pool = SenderPool::new(session, self.config.api_id);

        let client = Client::new(pool.handle.clone());

        // Spawn the sender pool runner to drive I/O in the background
        tokio::spawn(async move {
            pool.runner.run().await;
        });

        info!("Connected to Telegram.");

        if client.is_authorized().await? {
            let me = client.get_me().await?;
            let name = me.first_name().unwrap_or("User").to_string();
            info!("Already authorized as {}", name);
            self.state = AuthState::Authorized { user_name: name };
        } else {
            info!("Not authorized, login required.");
            self.state = AuthState::NeedPhone;
        }

        self.updates_rx = Some(pool.updates);
        self.client = Some(client);
        Ok(())
    }

    /// Step 1 of login: send phone number, request login code
    pub async fn send_phone(&mut self, phone: &str) -> Result<()> {
        let client = self.client.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not connected. Call connect() first."))?;

        info!("Requesting login code for {}...", phone);
        let token = client
            .request_login_code(phone, &self.config.api_hash)
            .await
            .context("Failed to request login code")?;

        self.login_token = Some(token);
        self.state = AuthState::NeedCode;
        info!("Login code sent. Waiting for user to enter it.");
        Ok(())
    }

    /// Step 2 of login: verify the code received via Telegram/SMS
    pub async fn send_code(&mut self, code: &str) -> Result<()> {
        let client = self.client.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not connected."))?;
        let token = self.login_token.take()
            .ok_or_else(|| anyhow::anyhow!("No login token. Call send_phone() first."))?;

        match client.sign_in(&token, code).await {
            Ok(user) => {
                let name = user.first_name().unwrap_or("User").to_string();
                info!("Signed in as {}", name);
                self.state = AuthState::Authorized { user_name: name };
                Ok(())
            }
            Err(SignInError::PasswordRequired(pw_token)) => {
                let hint = pw_token.hint().unwrap_or("").to_string();
                info!("2FA password required (hint: {})", hint);
                self.password_token = Some(pw_token);
                self.state = AuthState::NeedPassword { hint };
                Ok(())
            }
            Err(e) => {
                self.state = AuthState::Error(format!("Sign-in failed: {}", e));
                Err(anyhow::anyhow!("Sign-in failed: {}", e))
            }
        }
    }

    /// Step 3 (optional): provide 2FA password
    pub async fn send_password(&mut self, password: &str) -> Result<()> {
        let client = self.client.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not connected."))?;
        let pw_token = self.password_token.take()
            .ok_or_else(|| anyhow::anyhow!("No password token. 2FA was not requested."))?;

        match client.check_password(pw_token, password.as_bytes()).await {
            Ok(user) => {
                let name = user.first_name().unwrap_or("User").to_string();
                info!("Signed in with 2FA as {}", name);
                self.state = AuthState::Authorized { user_name: name };
                Ok(())
            }
            Err(e) => {
                self.state = AuthState::Error(format!("2FA failed: {}", e));
                Err(anyhow::anyhow!("2FA check failed: {}", e))
            }
        }
    }

    /// Take the updates receiver for stream_updates. Can only be called once.
    pub fn take_updates_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<UpdatesLike>> {
        self.updates_rx.take()
    }

    /// Disconnect from Telegram
    pub fn disconnect(&mut self) {
        if let Some(client) = &self.client {
            client.disconnect();
        }
        self.client = None;
        self.updates_rx = None;
        self.state = AuthState::Disconnected;
        info!("Disconnected from Telegram.");
    }
}
