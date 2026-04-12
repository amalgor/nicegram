use anyhow::Result;
use serde::{Deserialize, Serialize};
use sled::Db;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const BYTES_PER_GB: u128 = 1_000_000_000;
const MICRO_UNITS_PER_USDC: i64 = 1_000_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountTier {
    Free,
    Credit,
    Paid,
    Provider,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreditAccount {
    pub anchor_id: String,
    pub usage_bytes: u64,
    pub usage_seconds: u64,
    pub debt_micro_usdc: i64,
    pub credit_limit_micro_usdc: i64,
    pub payment_count: u32,
    pub last_payment: u64,
    pub tier: AccountTier,
    pub trial_accepted: bool,
    pub advanced_unlocked: bool,
    #[serde(default)]
    pub last_nudge_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditStatus {
    pub anchor_id: String,
    pub usage_bytes: u64,
    pub usage_seconds: u64,
    pub debt_micro_usdc: i64,
    pub credit_limit_micro_usdc: i64,
    pub payment_count: u32,
    pub last_payment: u64,
    pub tier: AccountTier,
    pub trial_accepted: bool,
    pub advanced_unlocked: bool,
    pub utilization_pct: f64,
    pub premium_allowed: bool,
    pub throttle_factor: f64,
    pub fallback_to_free: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NudgeEvent {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct CreditPolicySettings {
    pub trial_credit_micro_usdc: i64,
    pub linked_credit_micro_usdc: i64,
    pub growth_factor: f64,
    pub soft_nudge_threshold: f64,
    pub soft_throttle_threshold: f64,
    pub fallback_threshold: f64,
    pub min_speed_pct: f64,
    pub nudge_interval_secs: u64,
    pub advanced_after_payments: u32,
}

impl Default for CreditPolicySettings {
    fn default() -> Self {
        Self {
            trial_credit_micro_usdc: 100_000,
            linked_credit_micro_usdc: 1_000_000,
            growth_factor: 2.0,
            soft_nudge_threshold: 0.5,
            soft_throttle_threshold: 0.8,
            fallback_threshold: 1.0,
            min_speed_pct: 0.25,
            nudge_interval_secs: 3600,
            advanced_after_payments: 3,
        }
    }
}

pub struct CreditLedger {
    db: Db,
    settings: CreditPolicySettings,
}

impl CreditLedger {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::with_settings(path, CreditPolicySettings::default())
    }

    pub fn with_settings<P: AsRef<Path>>(path: P, settings: CreditPolicySettings) -> Result<Self> {
        Ok(Self {
            db: sled::open(path)?,
            settings,
        })
    }

    pub fn settings(&self) -> &CreditPolicySettings {
        &self.settings
    }

    pub fn accept_trial(&self, anchor_id: &str, linked_anchor: bool) -> Result<CreditAccount> {
        let mut account = self.load_or_create(anchor_id, linked_anchor)?;
        account.trial_accepted = true;
        account.tier = if account.payment_count > 0 {
            AccountTier::Paid
        } else {
            AccountTier::Credit
        };
        self.save(&account)?;
        Ok(account)
    }

    pub fn record_usage(
        &self,
        anchor_id: &str,
        linked_anchor: bool,
        bytes: u64,
        seconds: u64,
        price_per_gb_micro_usdc: u64,
    ) -> Result<CreditAccount> {
        let mut account = self.load_or_create(anchor_id, linked_anchor)?;
        account.usage_bytes = account.usage_bytes.saturating_add(bytes);
        account.usage_seconds = account.usage_seconds.saturating_add(seconds);
        if price_per_gb_micro_usdc > 0 {
            let debt_increment =
                micro_usdc_for_usage(bytes, price_per_gb_micro_usdc).min(i64::MAX as u128) as i64;
            account.debt_micro_usdc = account.debt_micro_usdc.saturating_add(debt_increment);
            if account.trial_accepted {
                account.tier = if account.payment_count > 0 {
                    AccountTier::Paid
                } else {
                    AccountTier::Credit
                };
            }
        }
        self.ensure_limit(&mut account, linked_anchor);
        account.advanced_unlocked = account.payment_count >= self.settings.advanced_after_payments;
        self.save(&account)?;
        Ok(account)
    }

    pub fn record_payment(
        &self,
        anchor_id: &str,
        linked_anchor: bool,
        amount_micro_usdc: i64,
    ) -> Result<CreditAccount> {
        let mut account = self.load_or_create(anchor_id, linked_anchor)?;
        account.debt_micro_usdc = (account.debt_micro_usdc - amount_micro_usdc).max(0);
        account.payment_count = account.payment_count.saturating_add(1);
        account.last_payment = now_epoch_secs();
        let grown =
            (account.credit_limit_micro_usdc as f64 * self.settings.growth_factor).round() as i64;
        account.credit_limit_micro_usdc =
            grown.max(base_limit_micro_usdc(&self.settings, linked_anchor));
        account.tier = AccountTier::Paid;
        account.advanced_unlocked = account.payment_count >= self.settings.advanced_after_payments;
        self.save(&account)?;
        Ok(account)
    }

    pub fn get_account(&self, anchor_id: &str, linked_anchor: bool) -> Result<CreditAccount> {
        self.load_or_create(anchor_id, linked_anchor)
    }

    pub fn merge_accounts(
        &self,
        from_anchor_id: &str,
        to_anchor_id: &str,
        linked_anchor: bool,
    ) -> Result<CreditAccount> {
        if from_anchor_id == to_anchor_id {
            return self.get_account(to_anchor_id, linked_anchor);
        }

        let from_key = credit_key(from_anchor_id);
        let Some(from_bytes) = self.db.get(&from_key)? else {
            return self.get_account(to_anchor_id, linked_anchor);
        };
        let from_account: CreditAccount = serde_json::from_slice(&from_bytes)?;
        let mut to_account = self.load_or_create(to_anchor_id, linked_anchor)?;
        to_account.usage_bytes = to_account
            .usage_bytes
            .saturating_add(from_account.usage_bytes);
        to_account.usage_seconds = to_account
            .usage_seconds
            .saturating_add(from_account.usage_seconds);
        to_account.debt_micro_usdc = to_account
            .debt_micro_usdc
            .saturating_add(from_account.debt_micro_usdc);
        to_account.credit_limit_micro_usdc = to_account
            .credit_limit_micro_usdc
            .max(from_account.credit_limit_micro_usdc)
            .max(base_limit_micro_usdc(&self.settings, linked_anchor));
        to_account.payment_count = to_account.payment_count.max(from_account.payment_count);
        to_account.last_payment = to_account.last_payment.max(from_account.last_payment);
        to_account.trial_accepted |= from_account.trial_accepted;
        to_account.advanced_unlocked |= from_account.advanced_unlocked;
        if matches!(from_account.tier, AccountTier::Provider) {
            to_account.tier = AccountTier::Provider;
        } else if matches!(from_account.tier, AccountTier::Paid) {
            to_account.tier = AccountTier::Paid;
        } else if from_account.trial_accepted {
            to_account.tier = AccountTier::Credit;
        }

        self.save(&to_account)?;
        self.db.remove(from_key)?;
        self.db.flush()?;
        Ok(to_account)
    }

    pub fn check_credit(&self, anchor_id: &str, linked_anchor: bool) -> Result<CreditStatus> {
        let mut account = self.load_or_create(anchor_id, linked_anchor)?;
        self.ensure_limit(&mut account, linked_anchor);
        account.advanced_unlocked = account.payment_count >= self.settings.advanced_after_payments;
        self.save(&account)?;

        let limit = account.credit_limit_micro_usdc.max(1);
        let utilization_pct = (account.debt_micro_usdc.max(0) as f64) / (limit as f64);
        let fallback_to_free = utilization_pct >= self.settings.fallback_threshold;
        let throttle_factor = if utilization_pct < self.settings.soft_throttle_threshold {
            1.0
        } else if fallback_to_free {
            self.settings.min_speed_pct
        } else {
            let range = (self.settings.fallback_threshold - self.settings.soft_throttle_threshold)
                .max(f64::EPSILON);
            let progress =
                ((utilization_pct - self.settings.soft_throttle_threshold) / range).clamp(0.0, 1.0);
            1.0 - progress * (1.0 - self.settings.min_speed_pct)
        };

        Ok(CreditStatus {
            anchor_id: account.anchor_id,
            usage_bytes: account.usage_bytes,
            usage_seconds: account.usage_seconds,
            debt_micro_usdc: account.debt_micro_usdc,
            credit_limit_micro_usdc: account.credit_limit_micro_usdc,
            payment_count: account.payment_count,
            last_payment: account.last_payment,
            tier: account.tier,
            trial_accepted: account.trial_accepted,
            advanced_unlocked: account.advanced_unlocked,
            utilization_pct,
            premium_allowed: account.trial_accepted && !fallback_to_free,
            throttle_factor,
            fallback_to_free,
        })
    }

    pub fn should_nudge(&self, anchor_id: &str, linked_anchor: bool) -> Result<Option<NudgeEvent>> {
        let mut account = self.load_or_create(anchor_id, linked_anchor)?;
        self.ensure_limit(&mut account, linked_anchor);

        let limit = account.credit_limit_micro_usdc.max(1);
        let utilization_pct = (account.debt_micro_usdc.max(0) as f64) / (limit as f64);
        let now = now_epoch_secs();
        if now.saturating_sub(account.last_nudge_at) < self.settings.nudge_interval_secs {
            return Ok(None);
        }

        let nudge = if utilization_pct >= self.settings.fallback_threshold {
            Some(NudgeEvent {
                id: "credit:fallback".to_string(),
                kind: "throttle_active".to_string(),
                title: "Premium speed reduced".to_string(),
                message: "You reached your current balance limit. Hydra will keep free routes working until you top up.".to_string(),
            })
        } else if utilization_pct >= self.settings.soft_throttle_threshold {
            Some(NudgeEvent {
                id: "credit:urge-payment".to_string(),
                kind: "urge_payment".to_string(),
                title: "Balance running low".to_string(),
                message:
                    "Premium routes are close to the limit. Add balance soon to avoid slowing down."
                        .to_string(),
            })
        } else if utilization_pct >= self.settings.soft_nudge_threshold {
            Some(NudgeEvent {
                id: "credit:soft-reminder".to_string(),
                kind: "soft_reminder".to_string(),
                title: "Usage climbing".to_string(),
                message: "Hydra is still working at full speed, but your faster-route balance is half used.".to_string(),
            })
        } else {
            None
        };

        if nudge.is_some() {
            account.last_nudge_at = now;
            self.save(&account)?;
        }

        Ok(nudge)
    }

    fn load_or_create(&self, anchor_id: &str, linked_anchor: bool) -> Result<CreditAccount> {
        let key = credit_key(anchor_id);
        if let Some(bytes) = self.db.get(&key)? {
            let mut account: CreditAccount = serde_json::from_slice(&bytes)?;
            self.ensure_limit(&mut account, linked_anchor);
            Ok(account)
        } else {
            let account = CreditAccount {
                anchor_id: anchor_id.to_string(),
                usage_bytes: 0,
                usage_seconds: 0,
                debt_micro_usdc: 0,
                credit_limit_micro_usdc: base_limit_micro_usdc(&self.settings, linked_anchor),
                payment_count: 0,
                last_payment: 0,
                tier: AccountTier::Free,
                trial_accepted: false,
                advanced_unlocked: false,
                last_nudge_at: 0,
            };
            self.save(&account)?;
            Ok(account)
        }
    }

    fn ensure_limit(&self, account: &mut CreditAccount, linked_anchor: bool) {
        account.credit_limit_micro_usdc = account
            .credit_limit_micro_usdc
            .max(base_limit_micro_usdc(&self.settings, linked_anchor));
        if account.advanced_unlocked
            || account.payment_count >= self.settings.advanced_after_payments
        {
            account.advanced_unlocked = true;
        }
    }

    fn save(&self, account: &CreditAccount) -> Result<()> {
        self.db
            .insert(credit_key(&account.anchor_id), serde_json::to_vec(account)?)?;
        self.db.flush()?;
        Ok(())
    }
}

fn credit_key(anchor_id: &str) -> String {
    format!("credit:{anchor_id}")
}

fn base_limit_micro_usdc(settings: &CreditPolicySettings, linked_anchor: bool) -> i64 {
    if linked_anchor {
        settings.linked_credit_micro_usdc
    } else {
        settings.trial_credit_micro_usdc
    }
}

fn micro_usdc_for_usage(bytes: u64, price_per_gb_micro_usdc: u64) -> u128 {
    let numerator = (bytes as u128) * (price_per_gb_micro_usdc as u128);
    numerator.div_ceil(BYTES_PER_GB)
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn usdc_to_micro(value: f64) -> i64 {
    (value * MICRO_UNITS_PER_USDC as f64).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let suffix = now_epoch_secs();
        std::env::temp_dir().join(format!("hydra-credit-{name}-{suffix}"))
    }

    #[test]
    fn anonymous_account_starts_with_trial_limit() {
        let ledger = CreditLedger::new(temp_path("trial")).unwrap();
        let status = ledger.check_credit("anon", false).unwrap();
        assert_eq!(status.credit_limit_micro_usdc, 100_000);
        assert!(!status.premium_allowed);
        assert_eq!(status.tier, AccountTier::Free);
    }

    #[test]
    fn linked_account_uses_linked_limit() {
        let ledger = CreditLedger::new(temp_path("linked")).unwrap();
        let status = ledger.check_credit("tg", true).unwrap();
        assert_eq!(status.credit_limit_micro_usdc, 1_000_000);
    }

    #[test]
    fn accept_trial_enables_premium_until_limit_is_exhausted() {
        let ledger = CreditLedger::new(temp_path("accept")).unwrap();
        ledger.accept_trial("anon", false).unwrap();
        let before = ledger.check_credit("anon", false).unwrap();
        assert!(before.premium_allowed);

        ledger
            .record_usage("anon", false, 150_000_000, 30, 1_000_000)
            .unwrap();
        let after = ledger.check_credit("anon", false).unwrap();
        assert!(after.fallback_to_free);
        assert!(!after.premium_allowed);
    }

    #[test]
    fn payment_growth_unlocks_advanced_mode() {
        let ledger = CreditLedger::new(temp_path("payments")).unwrap();
        let mut account = ledger.accept_trial("tg", true).unwrap();
        for _ in 0..3 {
            account = ledger.record_payment("tg", true, 500_000).unwrap();
        }
        assert!(account.advanced_unlocked);
        assert_eq!(account.payment_count, 3);
        assert!(account.credit_limit_micro_usdc >= 4_000_000);
    }

    #[test]
    fn threshold_nudges_follow_utilization() {
        let ledger = CreditLedger::with_settings(
            temp_path("nudge"),
            CreditPolicySettings {
                nudge_interval_secs: 0,
                ..CreditPolicySettings::default()
            },
        )
        .unwrap();
        ledger.accept_trial("anon", false).unwrap();
        ledger
            .record_usage("anon", false, 60_000_000, 20, 1_000_000)
            .unwrap();
        let soft = ledger.should_nudge("anon", false).unwrap().unwrap();
        assert_eq!(soft.kind, "soft_reminder");

        ledger
            .record_usage("anon", false, 25_000_000, 20, 1_000_000)
            .unwrap();
        let urge = ledger.should_nudge("anon", false).unwrap().unwrap();
        assert_eq!(urge.kind, "urge_payment");
    }
}
