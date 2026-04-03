use anyhow::Result;
#[flutter_rust_bridge::frb(sync)]
pub fn dismiss_nudge(nudge_id: String) -> Result<()> {
    crate::credit_runtime::dismiss_nudge(nudge_id)
}

pub async fn get_credit_status() -> Result<String> {
    crate::credit_runtime::get_credit_status().await
}

pub async fn get_nudge() -> Result<String> {
    crate::credit_runtime::get_nudge().await
}

pub async fn accept_trial_route() -> Result<String> {
    crate::credit_runtime::accept_trial_route().await
}

pub async fn get_telegram_anchor_info() -> Result<String> {
    crate::credit_runtime::get_telegram_anchor_info().await
}
