use anyhow::Result;

pub async fn get_share_earn_status() -> Result<String> {
    crate::provider_runtime::get_share_earn_status().await
}

pub async fn set_share_earn_enabled(enabled: bool, mnemonic: Option<String>) -> Result<String> {
    crate::provider_runtime::set_share_earn_enabled(enabled, mnemonic).await
}

pub async fn get_provider_earnings() -> Result<String> {
    crate::provider_runtime::get_provider_earnings().await
}

pub async fn update_share_settings(
    price_override_raw: Option<String>,
    max_bandwidth_mbps: Option<u64>,
    wifi_only: bool,
    schedule_start_hour: Option<u8>,
    schedule_end_hour: Option<u8>,
) -> Result<String> {
    crate::provider_runtime::update_share_settings(
        price_override_raw,
        max_bandwidth_mbps,
        wifi_only,
        schedule_start_hour,
        schedule_end_hour,
    )
    .await
}
