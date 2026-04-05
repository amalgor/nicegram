use alloy::primitives::keccak256;
use anyhow::Result;
use hydra_config::HydraConfig;
use hydra_core::{
    discovery::RouteDiscoveryService, transport::ConfiguredTransport, CreditController,
    CreditRuntimeStatus,
};
use hydra_econ::credit::{
    usdc_to_micro, CreditLedger, CreditPolicySettings, CreditStatus, NudgeEvent,
};
use hydra_econ::provider::ProviderMetricsLedger;
use hydra_exchange::ExchangeConfig;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

const CREDIT_STATE_FILE: &str = "credit_state.json";

lazy_static! {
    static ref SHARED_CREDIT_LEDGER: Mutex<Option<Arc<CreditLedger>>> = Mutex::new(None);
    static ref SHARED_CREDIT_CONTROLLER: Mutex<Option<Arc<MobileCreditController>>> =
        Mutex::new(None);
    static ref SHARED_DISCOVERY: Mutex<Option<Arc<RouteDiscoveryService>>> = Mutex::new(None);
    static ref SHARED_PROVIDER_METRICS: Mutex<Option<Arc<ProviderMetricsLedger>>> =
        Mutex::new(None);
}

#[derive(Debug, Clone)]
struct AnchorContext {
    anchor_id: String,
    linked_anchor: bool,
    authorized_telegram: bool,
    user_name: String,
    source: String,
}

#[derive(Clone)]
pub(crate) struct MobileCreditController {
    ledger: Arc<CreditLedger>,
    base_dir: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct CreditLocalState {
    installation_id: String,
    salt: String,
    #[serde(default)]
    dismissed_nudges: BTreeSet<String>,
    #[serde(default)]
    last_promoted_anchor: String,
}

#[derive(Debug, Serialize)]
struct CreditStatusPayload {
    anchor_id: String,
    anchor_source: String,
    authorized_telegram: bool,
    user_name: String,
    usage_bytes: u64,
    usage_seconds: u64,
    debt_micro_usdc: i64,
    debt_display: String,
    credit_limit_micro_usdc: i64,
    credit_limit_display: String,
    utilization_pct: f64,
    payment_count: u32,
    trial_accepted: bool,
    premium_allowed: bool,
    fallback_to_free: bool,
    throttle_factor: f64,
    advanced_unlocked: bool,
    tier: String,
    premium_routes_available: bool,
    premium_trial_available: bool,
    premium_materially_better: bool,
    route_state: String,
    route_message: String,
}

#[derive(Debug, Serialize)]
struct TelegramAnchorInfoPayload {
    authorized: bool,
    user_name: String,
    anchor_id: String,
}

impl MobileCreditController {
    fn new(ledger: Arc<CreditLedger>, base_dir: PathBuf) -> Self {
        Self { ledger, base_dir }
    }

    async fn anchor_context(&self) -> Result<AnchorContext> {
        resolve_anchor_context(&self.base_dir, &self.ledger).await
    }
}

pub(crate) fn dismiss_nudge(nudge_id: String) -> Result<()> {
    let base_dir = crate::api::shared_state::shared_base_dir()
        .ok_or_else(|| anyhow::anyhow!("Shared base dir not initialized"))?;
    let mut local = load_local_state(&base_dir)?;
    if nudge_id.starts_with("trial:") || nudge_id.starts_with("memory:") {
        local.dismissed_nudges.insert(nudge_id);
        save_local_state(&base_dir, &local)?;
    }
    Ok(())
}

pub(crate) async fn init_credit_services(
    base_dir: PathBuf,
    config: &HydraConfig,
) -> Result<(
    Option<Arc<RouteDiscoveryService>>,
    Option<Arc<dyn CreditController>>,
    Option<Arc<ProviderMetricsLedger>>,
)> {
    ensure_local_state(&base_dir)?;

    let settings = CreditPolicySettings {
        trial_credit_micro_usdc: usdc_to_micro(config.credit.trial_credit_usdc),
        linked_credit_micro_usdc: usdc_to_micro(config.credit.linked_credit_usdc),
        growth_factor: config.credit.growth_factor,
        soft_nudge_threshold: config.credit.soft_nudge_threshold,
        soft_throttle_threshold: config.credit.soft_throttle_threshold,
        fallback_threshold: config.credit.fallback_threshold,
        min_speed_pct: config.credit.min_speed_pct,
        nudge_interval_secs: config.credit.nudge_interval_secs,
        advanced_after_payments: config.credit.advanced_after_payments,
    };
    let ledger_path = config.econ.db_path.join("credit_ledger");
    let ledger = Arc::new(CreditLedger::with_settings(ledger_path, settings)?);
    let provider_metrics = Arc::new(ProviderMetricsLedger::new(
        config.econ.db_path.join("provider_metrics"),
    )?);
    let controller = Arc::new(MobileCreditController::new(
        ledger.clone(),
        base_dir.clone(),
    ));

    let discovery = if config.crypto.enabled {
        match ExchangeConfig::from_crypto_config(&config.crypto) {
            Ok(exchange) => Some(Arc::new(RouteDiscoveryService::new(
                exchange,
                config.discovery.clone(),
                Some(provider_metrics.clone()),
            ))),
            Err(error) => {
                tracing::warn!("Dynamic discovery disabled: {}", error);
                None
            }
        }
    } else {
        None
    };

    if let Some(service) = &discovery {
        service.start_polling();
    }

    {
        let mut guard = SHARED_CREDIT_LEDGER.lock().await;
        *guard = Some(ledger);
    }
    {
        let mut guard = SHARED_CREDIT_CONTROLLER.lock().await;
        *guard = Some(controller.clone());
    }
    {
        let mut guard = SHARED_DISCOVERY.lock().await;
        *guard = discovery.clone();
    }
    {
        let mut guard = SHARED_PROVIDER_METRICS.lock().await;
        *guard = Some(provider_metrics.clone());
    }

    Ok((
        discovery,
        Some(controller as Arc<dyn CreditController>),
        Some(provider_metrics),
    ))
}

pub(crate) async fn get_credit_status() -> Result<String> {
    let controller = shared_credit_controller().await?;
    let context = controller.anchor_context().await?;
    let status = controller
        .ledger
        .check_credit(&context.anchor_id, context.linked_anchor)?;
    let discovery = shared_discovery().await;
    let premium_routes_available = if let Some(service) = &discovery {
        service.premium_route_available().await
    } else {
        false
    };
    let premium_materially_better = if let Some(service) = &discovery {
        service.premium_is_materially_better().await
    } else {
        false
    };
    let payload = CreditStatusPayload {
        anchor_id: context.anchor_id,
        anchor_source: context.source,
        authorized_telegram: context.authorized_telegram,
        user_name: context.user_name,
        usage_bytes: status.usage_bytes,
        usage_seconds: status.usage_seconds,
        debt_micro_usdc: status.debt_micro_usdc,
        debt_display: format_micro_usdc(status.debt_micro_usdc),
        credit_limit_micro_usdc: status.credit_limit_micro_usdc,
        credit_limit_display: format_micro_usdc(status.credit_limit_micro_usdc),
        utilization_pct: status.utilization_pct,
        payment_count: status.payment_count,
        trial_accepted: status.trial_accepted,
        premium_allowed: status.premium_allowed,
        fallback_to_free: status.fallback_to_free,
        throttle_factor: status.throttle_factor,
        advanced_unlocked: status.advanced_unlocked,
        tier: format!("{:?}", status.tier).to_lowercase(),
        premium_routes_available,
        premium_trial_available: premium_routes_available
            && premium_materially_better
            && !status.trial_accepted,
        premium_materially_better,
        route_state: route_state(&status, premium_routes_available),
        route_message: route_message(&status, premium_routes_available),
    };
    Ok(serde_json::to_string(&payload)?)
}

pub(crate) async fn get_nudge() -> Result<String> {
    let controller = shared_credit_controller().await?;
    let context = controller.anchor_context().await?;
    let status = controller
        .ledger
        .check_credit(&context.anchor_id, context.linked_anchor)?;
    let base_dir = crate::api::shared_state::shared_base_dir()
        .ok_or_else(|| anyhow::anyhow!("Shared base dir not initialized"))?;
    let local = load_local_state(&base_dir)?;
    let discovery = shared_discovery().await;

    let trial_offer_id = "trial:better-route".to_string();
    if !status.trial_accepted && !local.dismissed_nudges.contains(&trial_offer_id) {
        if let Some(service) = &discovery {
            if service.premium_is_materially_better().await {
                let event = NudgeEvent {
                    id: trial_offer_id,
                    kind: "trial_offer".to_string(),
                    title: "Found a faster route".to_string(),
                    message: "Hydra found a better route for Telegram. Try it with a small starter balance.".to_string(),
                };
                return Ok(serde_json::to_string(&event)?);
            }
        }
    }

    if let Some(event) = controller
        .ledger
        .should_nudge(&context.anchor_id, context.linked_anchor)?
    {
        return Ok(serde_json::to_string(&event)?);
    }

    let memory_id = "memory:protect-local-state".to_string();
    if context.authorized_telegram && !local.dismissed_nudges.contains(&memory_id) {
        let event = NudgeEvent {
            id: memory_id,
            kind: "memory_guard".to_string(),
            title: "Protect your local assistant memory".to_string(),
            message: "Hydra keeps your assistant history on this device. Keep this installation safe so your setup and habits stay with you.".to_string(),
        };
        return Ok(serde_json::to_string(&event)?);
    }

    Ok("null".to_string())
}

pub(crate) async fn accept_trial_route() -> Result<String> {
    let controller = shared_credit_controller().await?;
    let context = controller.anchor_context().await?;
    let account = controller
        .ledger
        .accept_trial(&context.anchor_id, context.linked_anchor)?;
    dismiss_nudge("trial:better-route".to_string())?;
    Ok(serde_json::to_string(&serde_json::json!({
        "anchor_id": account.anchor_id,
        "trial_accepted": account.trial_accepted,
    }))?)
}

pub(crate) async fn get_telegram_anchor_info() -> Result<String> {
    let controller = shared_credit_controller().await?;
    let context = controller.anchor_context().await?;
    let payload = TelegramAnchorInfoPayload {
        authorized: context.authorized_telegram,
        user_name: context.user_name,
        anchor_id: if context.authorized_telegram {
            context.anchor_id
        } else {
            String::new()
        },
    };
    Ok(serde_json::to_string(&payload)?)
}

async fn shared_credit_controller() -> Result<Arc<MobileCreditController>> {
    let guard = SHARED_CREDIT_CONTROLLER.lock().await;
    guard
        .as_ref()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Credit runtime not initialized"))
}

async fn shared_discovery() -> Option<Arc<RouteDiscoveryService>> {
    SHARED_DISCOVERY.lock().await.clone()
}

pub(crate) async fn shared_provider_metrics() -> Option<Arc<ProviderMetricsLedger>> {
    SHARED_PROVIDER_METRICS.lock().await.clone()
}

async fn resolve_anchor_context(base_dir: &Path, ledger: &CreditLedger) -> Result<AnchorContext> {
    let mut local = load_local_state(base_dir)?;
    let install_anchor = hash_anchor(&local.salt, &format!("install:{}", local.installation_id));
    if let Some((user_id, user_name)) = crate::api::content::telegram_anchor_info().await? {
        let telegram_anchor = hash_anchor(&local.salt, &format!("telegram:{user_id}"));
        if local.last_promoted_anchor != telegram_anchor {
            let _ = ledger.merge_accounts(&install_anchor, &telegram_anchor, true);
            local.last_promoted_anchor = telegram_anchor.clone();
            save_local_state(base_dir, &local)?;
        }
        Ok(AnchorContext {
            anchor_id: telegram_anchor,
            linked_anchor: true,
            authorized_telegram: true,
            user_name,
            source: "telegram".to_string(),
        })
    } else {
        Ok(AnchorContext {
            anchor_id: install_anchor,
            linked_anchor: false,
            authorized_telegram: false,
            user_name: String::new(),
            source: "installation".to_string(),
        })
    }
}

fn ensure_local_state(base_dir: &Path) -> Result<CreditLocalState> {
    let local = load_local_state(base_dir).or_else(|_| -> Result<CreditLocalState> {
        let created = CreditLocalState {
            installation_id: random_token("install", base_dir),
            salt: random_token("salt", base_dir),
            dismissed_nudges: BTreeSet::new(),
            last_promoted_anchor: String::new(),
        };
        save_local_state(base_dir, &created)?;
        Ok(created)
    })?;
    if local.installation_id.is_empty() || local.salt.is_empty() {
        let created = CreditLocalState {
            installation_id: if local.installation_id.is_empty() {
                random_token("install", base_dir)
            } else {
                local.installation_id
            },
            salt: if local.salt.is_empty() {
                random_token("salt", base_dir)
            } else {
                local.salt
            },
            dismissed_nudges: local.dismissed_nudges,
            last_promoted_anchor: local.last_promoted_anchor,
        };
        save_local_state(base_dir, &created)?;
        Ok(created)
    } else {
        Ok(local)
    }
}

fn load_local_state(base_dir: &Path) -> Result<CreditLocalState> {
    let path = base_dir.join(CREDIT_STATE_FILE);
    if !path.exists() {
        anyhow::bail!("credit state not initialized")
    }
    let bytes = std::fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn save_local_state(base_dir: &Path, state: &CreditLocalState) -> Result<()> {
    let path = base_dir.join(CREDIT_STATE_FILE);
    std::fs::write(path, serde_json::to_vec_pretty(state)?)?;
    Ok(())
}

fn hash_anchor(salt: &str, value: &str) -> String {
    format!("{:x}", keccak256(format!("{salt}:{value}").as_bytes()))
}

fn random_token(label: &str, base_dir: &Path) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{:x}",
        keccak256(
            format!(
                "{label}:{}:{now}:{}",
                std::process::id(),
                base_dir.display()
            )
            .as_bytes()
        )
    )
}

fn format_micro_usdc(value: i64) -> String {
    let negative = value.is_negative();
    let raw = value.unsigned_abs().to_string();
    let padded = if raw.len() <= 6 {
        format!("{}{}", "0".repeat(7 - raw.len()), raw)
    } else {
        raw
    };
    let split = padded.len() - 6;
    let integer = &padded[..split];
    let fraction = padded[split..].trim_end_matches('0');
    let formatted = if fraction.is_empty() {
        integer.to_string()
    } else {
        format!("{integer}.{fraction}")
    };
    if negative {
        format!("-{formatted}")
    } else {
        formatted
    }
}

fn route_state(status: &CreditStatus, premium_available: bool) -> String {
    if status.fallback_to_free {
        "fallback".to_string()
    } else if status.premium_allowed && premium_available {
        "premium".to_string()
    } else if premium_available && !status.trial_accepted {
        "trial_available".to_string()
    } else {
        "free".to_string()
    }
}

fn route_message(status: &CreditStatus, premium_available: bool) -> String {
    if status.fallback_to_free {
        "Hydra is keeping free routes active while faster routes are paused.".to_string()
    } else if status.premium_allowed && premium_available {
        "Hydra can use faster routes when they help Telegram.".to_string()
    } else if premium_available && !status.trial_accepted {
        "Hydra found faster routes that can be tried with one tap.".to_string()
    } else {
        "Hydra is using free routes.".to_string()
    }
}

#[async_trait::async_trait]
impl CreditController for MobileCreditController {
    async fn current_status(&self) -> Result<CreditRuntimeStatus> {
        let context = self.anchor_context().await?;
        let status = self
            .ledger
            .check_credit(&context.anchor_id, context.linked_anchor)?;
        Ok(CreditRuntimeStatus {
            premium_allowed: status.premium_allowed,
            throttle_factor: status.throttle_factor,
            fallback_to_free: status.fallback_to_free,
        })
    }

    async fn record_usage(
        &self,
        transport: &ConfiguredTransport,
        bytes_total: u64,
        duration: Duration,
    ) -> Result<()> {
        if transport.metadata.price_per_gb_micro_usdc == 0 {
            return Ok(());
        }
        let context = self.anchor_context().await?;
        self.ledger.record_usage(
            &context.anchor_id,
            context.linked_anchor,
            bytes_total,
            duration.as_secs().max(1),
            transport.metadata.price_per_gb_micro_usdc,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hydra_econ::credit::AccountTier;

    #[test]
    fn format_micro_units_for_ui() {
        assert_eq!(format_micro_usdc(100_000), "0.1");
        assert_eq!(format_micro_usdc(1_000_000), "1");
        assert_eq!(format_micro_usdc(1_250_000), "1.25");
    }

    #[test]
    fn route_state_prefers_fallback_and_trial() {
        let status = CreditStatus {
            anchor_id: "x".to_string(),
            usage_bytes: 0,
            usage_seconds: 0,
            debt_micro_usdc: 0,
            credit_limit_micro_usdc: 100_000,
            payment_count: 0,
            last_payment: 0,
            tier: AccountTier::Free,
            trial_accepted: false,
            advanced_unlocked: false,
            utilization_pct: 0.0,
            premium_allowed: false,
            throttle_factor: 1.0,
            fallback_to_free: false,
        };
        assert_eq!(route_state(&status, true), "trial_available");
        assert_eq!(route_state(&status, false), "free");
    }
}
