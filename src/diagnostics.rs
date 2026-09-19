//! Shareable diagnostics are assembled from safe fields, never raw errors.
use crate::{
    config::schema::{Config, Language, ProxyMode},
    meter::extras::{Availability, Snapshot},
    providers::{ProviderData, ProviderId, ProviderStatus},
};

fn text<'a>(chinese: bool, en: &'a str, zh: &'a str) -> &'a str {
    if chinese {
        zh
    } else {
        en
    }
}

fn provider_advice(status: &ProviderStatus, chinese: bool) -> &'static str {
    match status {
        ProviderStatus::Ok => text(chinese, "Read succeeded.", "读取成功。"),
        ProviderStatus::Estimated => text(chinese, "Estimated data; refresh to confirm quota.", "当前为估算数据，请刷新确认额度。"),
        ProviderStatus::Loading => text(chinese, "Waiting for a result; try Refresh shortly.", "正在等待结果，请稍后刷新。"),
        ProviderStatus::NotConfigured => text(chinese, "Login not found; sign in with this provider's CLI or configure its credential in More > Providers.", "未找到登录信息；请登录对应 CLI，或在“更多 → 服务商”设置凭据。"),
        ProviderStatus::AuthError(_) => text(chinese, "Authentication failed; sign in again with this provider's CLI and refresh. Check any manually saved credential.", "认证失败；请重新登录对应 CLI 后刷新，并检查手动保存的凭据。"),
        ProviderStatus::NetworkError(error) => {
            let lower = error.to_ascii_lowercase();
            if lower.contains("429") {
                text(chinese, "Rate limited; wait and increase the update interval.", "请求受到限流；请稍后重试，并适当延长刷新间隔。")
            } else if lower.contains("401") || lower.contains("403") {
                text(chinese, "Access rejected; check your CLI login and account permissions.", "访问被拒绝；请检查 CLI 登录和账号权限。")
            } else if lower.contains("timeout") || lower.contains("timed out") {
                text(chinese, "Request timed out; check your connection and configured proxy mode.", "请求超时；请检查网络连接及所选代理模式。")
            } else if lower.contains("schema") || lower.contains("decod") || lower.contains("json") {
                text(chinese, "Response could not be read; refresh or check for an app update.", "无法解析返回数据；请刷新或检查应用更新。")
            } else {
                text(chinese, "Request failed; check your connection, proxy mode and service availability, then refresh.", "请求失败；请检查网络、代理模式及服务状态后刷新。")
            }
        }
    }
}

fn extension_advice<T>(state: &Availability<T>, chinese: bool) -> &'static str {
    match state {
        Availability::Ready(_) => text(chinese, "Read succeeded.", "读取成功。"),
        Availability::NotLoaded => text(chinese, "Not read yet; enable this module and open the quota panel.", "尚未读取；请启用此模块并打开配额面板。"),
        Availability::Loading => text(chinese, "Reading; the helper times out after 25 seconds.", "读取中；辅助进程将在 25 秒后超时退出。"),
        Availability::Unavailable(reason) => match *reason {
            "Codex is not installed" => text(chinese, "Codex CLI not found; install it and restart QuotaBar.", "未找到 Codex CLI；请安装后重启 QuotaBar。"),
            "Account mismatch" | "Account could not be verified" | "Account cannot be verified" | "Account cannot be matched" => text(chinese, "Account could not be matched; use the same Codex login for base quota and extensions.", "无法匹配账号；请确保基础额度与扩展使用同一 Codex 登录。"),
            "Codex extension timed out" | "Codex read timed out" => text(chinese, "Extension timed out; check the CLI and connection, then refresh.", "扩展读取超时；请检查 CLI 和网络后刷新。"),
            "Daily token data incomplete" => text(chinese, "Incomplete daily data; missing dates are not zero. Refresh later.", "每日数据不完整；缺失日期不代表零，请稍后刷新。"),
            _ => text(chinese, "Extension unavailable; check your Codex login and CLI version, then refresh.", "扩展不可用；请检查 Codex 登录及 CLI 版本后刷新。"),
        },
    }
}

pub fn report(config: &Config, latest: &[ProviderData], extras: &Snapshot) -> String {
    let chinese = match config.general.language {
        Language::Chinese => true,
        Language::English => false,
        Language::Auto => crate::i18n::is_chinese(),
    };
    let proxy = match config.network.proxy_mode {
        ProxyMode::System => text(chinese, "System", "跟随系统"),
        ProxyMode::Direct => text(chinese, "Direct", "直连"),
    };
    let mut lines = vec![
        format!("QuotaBar {}", env!("CARGO_PKG_VERSION")),
        format!(
            "{}: {proxy} · {}: {}s",
            text(chinese, "Proxy", "代理"),
            text(chinese, "Update interval", "刷新间隔"),
            config.general.update_interval_secs
        ),
        text(
            chinese,
            "Credentials, account identifiers and raw errors are omitted.",
            "已排除凭据、账号标识和原始错误。",
        )
        .to_string(),
        String::new(),
    ];
    // Iterate known IDs, never arbitrary config labels or provider strings.
    for id in [
        ProviderId::Codex,
        ProviderId::Claude,
        ProviderId::Copilot,
        ProviderId::Antigravity,
    ] {
        if !config
            .providers
            .iter()
            .any(|p| p.enabled && ProviderId::from_config_id(&p.id) == Some(id.clone()))
        {
            continue;
        }
        match latest.iter().find(|d| d.id == id) {
            Some(data) => {
                lines.push(format!(
                    "{} · {}: {}",
                    id.display_name(),
                    text(chinese, "Latest result", "最新结果"),
                    data.updated_at.format("%m-%d %H:%M UTC")
                ));
                lines.push(provider_advice(&data.status, chinese).to_string());
            }
            None => lines.push(format!(
                "{}: {}",
                id.display_name(),
                text(chinese, "No result yet; refresh.", "尚无结果，请刷新。")
            )),
        }
    }
    lines.push(String::new());
    lines.push(format!(
        "{}: {}",
        text(chinese, "Reset opportunities", "重置机会"),
        extension_advice(&extras.credits, chinese)
    ));
    lines.push(format!(
        "{}: {}",
        text(chinese, "Token activity", "Token 活动"),
        extension_advice(&extras.tokens, chinese)
    ));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reports_safe_categories_without_copying_sensitive_strings() {
        let mut config = Config::default();
        config.general.language = Language::English;
        config.providers[1].credential_label = "secret-label".into();
        let mut data = ProviderData {
            account_key: Some("secret-account".into()), plan_type: Some("secret-plan".into()),
            id: ProviderId::Codex, status: ProviderStatus::NetworkError("HTTP 429 https://user:secret-token@proxy.invalid mail@example.test C:\\secret-path".into()),
            metrics: vec![], updated_at: chrono::Utc::now(), received_at: Some(std::time::Instant::now()),
        };
        let extras = Snapshot {
            account: Some("secret-extension-account".into()),
            tokens: Availability::Unavailable("secret-extension-error"),
            ..Default::default()
        };
        let report = report(&config, &[data.clone()], &extras);
        assert!(report.contains("Rate limited"));
        assert!(report.contains("Extension unavailable"));
        for sensitive in [
            "secret",
            "proxy.invalid",
            "mail@example.test",
            "https://",
            "C:\\",
        ] {
            assert!(!report.contains(sensitive), "leaked {sensitive}");
        }
        data.status = ProviderStatus::AuthError("secret-token".into());
        config.general.language = Language::Chinese;
        let report = super::report(&config, &[data], &extras);
        assert!(report.contains("认证失败"));
        assert!(!report.contains("secret"));
    }
}
