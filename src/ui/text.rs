//! Shared display-only status and duration formatting.
use crate::{
    i18n::t,
    providers::{ProviderData, ProviderId, ProviderStatus},
};
use std::collections::HashMap;
pub fn hover_reason(
    data: &ProviderData,
    errors: &HashMap<ProviderId, ProviderStatus>,
) -> Option<String> {
    let aged = data.stale_age_secs().is_some() || matches!(data.status, ProviderStatus::Estimated);
    if !aged {
        return None;
    }
    let Some(status) = errors.get(&data.id) else {
        return Some(t("no fresh data").to_string());
    };
    match status {
        ProviderStatus::AuthError(message) => {
            let action = match data.id {
                ProviderId::Claude => "run Claude Code",
                ProviderId::Codex => "run Codex CLI",
                ProviderId::Copilot => "check gh auth",
                ProviderId::Antigravity => "run Antigravity",
            };
            Some(format!("{} — {}", t(message), t(action)))
        }
        ProviderStatus::NetworkError(message) => Some(t(message).to_string()),
        _ => Some(t("no fresh data").to_string()),
    }
}

pub fn format_duration(secs: i64) -> String {
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3600;
    let m = (secs % 3600) / 60;
    if d > 0 {
        crate::tr!("{days}d {hours}h", days = d, hours = h)
    } else if h > 0 {
        crate::tr!("{hours}h {minutes}min", hours = h, minutes = m)
    } else if m > 0 {
        crate::tr!("{minutes}min", minutes = m)
    } else {
        crate::tr!("{seconds}s", seconds = secs)
    }
}
