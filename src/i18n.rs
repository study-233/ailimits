//! UI-only translations. Stored metrics, provider identifiers and diagnostics
//! remain language independent. Unknown text (including server errors) is kept.
use crate::config::schema::Language;
use std::sync::atomic::{AtomicBool, Ordering};

static CHINESE: AtomicBool = AtomicBool::new(false);

pub fn set_language(language: Language) {
    CHINESE.store(
        resolve(language, system_is_chinese()) == Language::Chinese,
        Ordering::Relaxed,
    );
}

pub fn resolve(language: Language, chinese_system: bool) -> Language {
    match language {
        Language::Auto if chinese_system => Language::Chinese,
        Language::Auto => Language::English,
        explicit => explicit,
    }
}

#[cfg(windows)]
fn system_is_chinese() -> bool {
    // The primary language bits cover simplified and traditional Chinese UI.
    unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() & 0x3ff == 0x04 }
}

#[cfg(not(windows))]
fn system_is_chinese() -> bool {
    false
}

pub fn is_chinese() -> bool {
    CHINESE.load(Ordering::Relaxed)
}

pub fn t(english: &str) -> &str {
    translate(english, is_chinese())
}

pub fn translate(english: &str, chinese: bool) -> &str {
    if chinese {
        if let Some((_, translation)) = MESSAGES.iter().find(|(key, _)| *key == english) {
            return translation;
        }
    }
    english
}

/// Interpolate named values exactly once, so braces in an external error
/// cannot be interpreted as another placeholder. Translations share keys.
pub fn message(key: &str, values: &[(&str, String)]) -> String {
    interpolate(t(key), values)
}

fn interpolate(template: &str, values: &[(&str, String)]) -> String {
    let mut output = String::new();
    let mut tail = template;
    while let Some(start) = tail.find('{') {
        output.push_str(&tail[..start]);
        let Some(end) = tail[start..].find('}').map(|i| start + i) else {
            output.push_str(&tail[start..]);
            return output;
        };
        let name = &tail[start + 1..end];
        if let Some((_, value)) = values.iter().find(|(key, _)| *key == name) {
            output.push_str(value);
        } else {
            output.push_str(&tail[start..=end]);
        }
        tail = &tail[end + 1..];
    }
    output.push_str(tail);
    output
}

#[macro_export]
macro_rules! tr {
    ($key:expr $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::i18n::message($key, &[$((stringify!($name), format!("{}", $value))),*])
    };
}

pub const MESSAGES: &[(&str, &str)] = &[
    ("Limit", "额度"),
    ("Quota alerts", "额度提醒"),
    ("Enable notifications", "启用通知"),
    ("Use provider threshold", "使用服务商默认阈值"),
    ("Diagnostics…", "诊断信息…"),
    ("Diagnostics", "诊断信息"),
    ("Choose OK to copy this report.", "点击“确定”复制此报告，或点击“取消”关闭。"),
    ("Diagnostic report copied", "诊断报告已复制"),
    ("Could not copy diagnostic report", "无法复制诊断报告，请稍后重试"),
    ("Settings", "设置"),
    ("Last updated", "更新于"),
    ("5-hour quota", "5 小时额度"),
    ("Weekly quota", "每周额度"),
    ("Reset opportunities", "重置机会"),
    ("Quota history", "额度历史"),
    ("Token activity", "Token 活动"),
    ("Taskbar appearance", "任务栏外观"),
    ("Panel content", "面板内容"),
    ("Show and arrange modules", "选择显示模块并调整顺序"),
    ("All modules hidden", "已隐藏全部模块"),
    ("All modules are hidden. Change this in Settings.", "已隐藏全部模块，可在设置中调整。"),
    ("Drag the handle or use the arrows.", "拖动手柄或使用上下箭头调整顺序。"),
    ("Hidden modules keep their position.", "隐藏模块会保留原来的位置。"),
    ("History recording continues while hidden.", "隐藏模块不影响额度历史记录。"),
    ("Plan unavailable", "套餐信息不可用"),
    ("Not loaded", "尚未加载"),
    ("Loading", "正在加载"),
    ("Refresh", "刷新"),
    ("Quota remaining", "额度剩余"),
    ("Time remaining", "时间剩余"),
    ("On pace", "节奏正常"),
    ("Above pace", "使用偏快"),
    ("Pace unavailable", "节奏未知"),
    ("Resets in", "重置倒计时"),
    ("available", "次可用"),
    ("No reset opportunities available", "暂无可用重置机会"),
    ("Only the available count was returned", "接口仅返回可用次数，未提供有效期"),
    ("Expiry of each available opportunity", "各次机会的剩余有效期"),
    ("Expires", "到期时间"),
    ("Expiry unavailable", "有效期不可用"),
    ("Reset opportunities unavailable", "重置机会不可用"),
    ("Daily token data unavailable", "每日 Token 数据不可用"),
    ("Daily token data incomplete", "每日 Token 数据不完整"),
    ("Account cannot be matched", "账号无法匹配"),
    ("Codex is not installed", "未安装 Codex"),
    ("Codex read timed out", "Codex 读取超时"),
    ("Codex extension unavailable", "Codex 扩展接口不可用"),
    ("Current weekly cycle", "当前每周额度周期"),
    ("Current cycle", "当前周期"),
    ("7 days", "近 7 天"),
    ("30 days", "近 30 天"),
    ("30 days · reported total", "近 30 天 · 已返回数据合计"),
    ("Observed data · gaps are not interpolated", "真实观测 · 采样空白不连线"),
    ("Missing dates are shown as gaps", "缺失日期与零用量分别显示"),
    ("Point to a sample to inspect its date and value", "指向数据点查看日期和数值"),
    ("No observations in this period", "此期间尚无额度观测记录"),
    ("No token observations in this period", "此期间尚无 Token 记录"),
    ("Missing data", "数据缺失"),
    ("remaining", "剩余"),
    ("Not updated yet", "尚未更新"),
    ("Reported total", "已返回数据合计"),

    ("More", "更多"),
    ("View quota", "查看额度"),
    ("Remaining quota", "剩余额度"),
    ("Waiting for update", "等待更新"),
    ("Waiting for quota", "等待额度数据"),
    ("Resets in {duration}", "{duration} 后重置"),
    ("Updated at {time}", "更新于 {time}"),
    ("Appearance", "外观"),
    ("Customize your taskbar quota display.", "自定义任务栏额度的显示方式"),
    ("Display style", "显示样式"), ("Quota periods", "额度显示"),
    ("Rings", "圆环"), ("Preview", "预览"), ("Light", "浅色"), ("Dark", "深色"),
    ("Text weight", "字重"), ("Tray display", "托盘显示"),
    ("Follow style", "跟随显示样式"), ("Style + state", "图形 + 状态"),
    ("Auto", "自动"), ("Number", "数字"), ("Ring + state", "圆环 + 状态"),
    ("Reset in", "重置于"), ("Updated just now", "刚刚更新"), ("Updated", "距更新"),
    ("Make room for what matters.", "额度，一目了然。"),
    ("Activity rings", "活动圆环"),
    ("Progress bars", "横向进度条"),
    ("5 hours", "5 小时"),
    ("Both periods", "同时显示"),
    ("Weekly color", "每周颜色"),
    ("5-hour color", "5 小时颜色"),
    ("Size", "大小"),
    ("Standard", "标准"),
    ("Large", "较大"),
    ("Custom", "自定义"),
    ("Advanced settings", "高级设置"),
    ("Hide advanced settings", "收起高级设置"),
    ("Ring diameter", "圆环直径"),
    ("Ring thickness", "圆环线宽"),
    ("Bar width", "横条宽度"),
    ("Bar thickness", "横条厚度"),
    ("Graphic X", "图形水平位置"),
    ("Graphic Y", "图形垂直偏移"),
    ("Text size", "数字字号"),
    ("Text X", "数字水平位置"),
    ("Text Y", "数字垂直偏移"),
    ("Symbols %", "符号比例 %"),
    ("Number color", "数字颜色"),
    ("Use auto to follow the taskbar theme.", "填 auto 可跟随任务栏主题。"),
    ("Check colors and ranges.", "请检查颜色和数值范围。"),
    ("Quota unavailable", "暂无此周期额度"),
    ("Reset:", "重置："),
    ("Reset time unavailable", "暂无重置时间"),
    ("Codex 5-hour remaining", "Codex 5 小时剩余额度"),

    ("Check colors and ranges; thickness must be less than half the diameter.","请检查颜色和数值范围；线宽必须小于直径的一半。"),
    ("Save","保存"),
    ("Cancel","取消"),
    ("Restore defaults","恢复默认"),
    ("Live preview · light / dark","实时预览 · 浅色 / 深色"),
    ("Offsets and sizes use display-independent pixels. Vertical movement stays inside the taskbar.","位置和大小不受系统缩放影响。垂直偏移限制在任务栏内。"),
    ("Colors: #RRGGBB; numbers also accept auto.","颜色可填 #RRGGBB；数字填 auto 可跟随系统。"),
    ("Bold","粗体"),
    ("Semibold","半粗"),
    ("Regular","常规"),
    ("Weight","字重"),
    ("Symbols % (40–100)","符号比例 % (40–100)"),
    ("Size (10–40)","字号高度 (10–40)"),
    ("Vertical (−20–20)","垂直偏移 (−20–20)"),
    ("Horizontal (0–300)","水平位置 (0–300)"),
    ("Thickness (1–12)","线宽 (1–12)"),
    ("Diameter (12–44)","直径 (12–44)"),
    ("Choose…","选择…"),
    ("Color","颜色"),
    ("Numbers","数字"),
    ("Ring","圆环"),
    ("Appearance settings…","外观设置…"),
    ("Restore automatic position", "恢复自动位置"),
    ("Show desktop widget", "显示桌面悬浮窗"),
    ("Codex weekly remaining", "Codex 每周余量"),
    ("Weekly reset:", "每周重置："),
    ("Weekly reset time unavailable", "每周重置时间未知"),
    ("Loading quota", "正在加载额度"),
    ("Authentication required", "需要认证"),
    ("Network unavailable", "网络不可用"),
    ("Codex not enabled or configured", "Codex 未启用或未配置"),
    ("Weekly quota unavailable", "每周额度不可用"),
    ("Estimated quota", "估算额度"),
    ("Cached quota (outdated)", "缓存额度（已过期）"),
    ("Live quota", "最新额度"),
    ("Compact", "精简"),
    ("Medium", "标准"),
    ("Expanded", "详细"),
    ("Vertical", "垂直"),
    ("Horizontal", "水平"),
    ("Width", "宽度"),
    ("Arrangement", "排列"),
    ("In a row", "横向排列"),
    ("In a column", "纵向排列"),
    ("Lock position", "锁定位置"),
    ("Always on top", "置顶显示"),
    ("Indicator", "任务栏指示器"),
    ("Tray icon", "托盘图标"),
    ("Taskbar panel", "任务栏面板"),
    ("Off", "关闭"),
    ("Display", "显示器"),
    ("Primary", "主显示器"),
    ("Display {number}", "显示器 {number}"),
    ("Forecast", "用量预测"),
    ("Automatic updates", "自动更新"),
    ("Palette", "配色"),
    ("Monochrome", "单色"),
    ("Default", "默认"),
    ("Ocean", "海洋"),
    ("Sunset", "日落"),
    ("Forest", "森林"),
    ("Neon", "霓虹"),
    ("Ice", "冰蓝"),
    ("Rose", "玫瑰"),
    ("Slate", "岩灰"),
    ("Background opacity", "背景不透明度"),
    ("Brightness", "亮度"),
    ("Saturation", "饱和度"),
    ("Update interval", "刷新间隔"),
    ("{minutes} min", "{minutes} 分钟"),
    ("Providers", "服务商"),
    ("Enabled", "启用"),
    ("Claude Code subscription", "Claude Code 订阅"),
    ("API key", "API 密钥"),
    (
        "Paste PAT from clipboard (optional)",
        "从剪贴板粘贴 PAT（可选）",
    ),
    ("Paste API key from clipboard", "从剪贴板粘贴 API 密钥"),
    ("Remove key", "移除密钥"),
    ("Paste usage token from clipboard", "从剪贴板粘贴用量令牌"),
    ("Remove usage token", "移除用量令牌"),
    ("Quit", "退出"),
    ("Language", "语言"),
    ("Follow system", "跟随系统"),
    ("Network proxy", "网络代理"),
    ("Direct connection", "直连"),
    ("Right-click → set up providers", "右键 → 配置服务商"),
    ("session unavailable", "会话用量不可用"),
    ("OK", "正常"),
    ("check the key", "检查密钥"),
    ("⚠ offline", "⚠ 网络不可用"),
    ("not configured", "未配置"),
    ("no fresh data", "暂无最新数据"),
    ("Session", "会话"),
    ("Premium", "高级请求"),
    ("Chat", "聊天"),
    ("Completions", "代码补全"),
    ("Weekly", "每周"),
    ("Monthly", "每月"),
    ("Daily", "每日"),
    ("Requests", "请求"),
    ("Tokens", "令牌"),
    ("Premium requests", "高级请求"),
    ("est · {duration} ago", "估算 · {duration}前"),
    ("{duration} ago", "{duration}前"),
    ("{days}d {hours}h", "{days}天 {hours}小时"),
    ("{hours}h {minutes}min", "{hours}小时 {minutes}分"),
    ("{minutes}min", "{minutes}分"),
    ("{seconds}s", "{seconds}秒"),
    (
        "{percent}% used, resets in {duration}",
        "已用 {percent}%，{duration}后重置",
    ),
    ("{percent}% used", "已用 {percent}%"),
    ("{provider} limit", "{provider} 用量提醒"),
    ("Key for {provider} stored", "已保存 {provider} 的密钥"),
    ("Key for {provider} removed", "已移除 {provider} 的密钥"),
    (
        "Usage token for {provider} stored",
        "已保存 {provider} 的用量令牌",
    ),
    (
        "Usage token for {provider} removed",
        "已移除 {provider} 的用量令牌",
    ),
    ("Failed to store the key", "无法保存密钥"),
    ("Failed to remove the key", "无法移除密钥"),
    ("Failed to store the usage token", "无法保存用量令牌"),
    ("Failed to remove the usage token", "无法移除用量令牌"),
    ("clipboard unavailable: {error}", "剪贴板不可用：{error}"),
    (
        "clipboard is empty or holds no text",
        "剪贴板为空或没有文本",
    ),
    (
        "clipboard content does not look like a key",
        "剪贴板内容不是有效的密钥格式",
    ),
    ("run Claude Code", "运行 Claude Code"),
    ("run Codex CLI", "运行 Codex CLI"),
    ("check gh auth", "检查 gh 认证"),
    ("run Antigravity", "运行 Antigravity"),
    ("check the network or proxy", "检查网络或代理"),
    ("invalid API key", "API 密钥无效"),
    (
        "API key not found in Credential Manager",
        "凭据管理器中未找到 API 密钥",
    ),
    ("rate-limited, retrying", "请求过于频繁，稍后重试"),
    ("token expired", "令牌已过期"),
    ("no ratelimit headers in response", "响应中没有用量限制信息"),
    (
        "unrecognized wham/usage schema",
        "无法识别 wham/usage 响应格式",
    ),
    (
        "unrecognized copilot_internal schema",
        "无法识别 copilot_internal 响应格式",
    ),
    ("token has no Copilot access", "令牌没有 Copilot 访问权限"),
    (
        "no token: install gh CLI (gh auth login) or add a PAT",
        "没有令牌：安装 gh CLI 并运行 gh auth login，或添加 PAT",
    ),
    ("Antigravity token rejected", "Antigravity 令牌被拒绝"),
    (
        "no Code Assist project id — open Antigravity once",
        "没有 Code Assist 项目 ID — 请启动一次 Antigravity",
    ),
    ("Antigravity quota unavailable", "Antigravity 配额暂不可用"),
    (
        "ailimits-auth — provider authorization for the QuotaBar widget",
        "ailimits-auth — QuotaBar 服务商认证工具",
    ),
    ("Usage:", "用法："),
    (
        "  ailimits-auth status                    show auth status",
        "  ailimits-auth status                    查看认证状态",
    ),
    (
        "  ailimits-auth set <provider>            store a key (claude: API key, copilot: PAT)",
        "  ailimits-auth set <provider>            保存密钥（claude：API 密钥，copilot：PAT）",
    ),
    (
        "  ailimits-auth remove <provider>         remove the key",
        "  ailimits-auth remove <provider>         移除密钥",
    ),
    (
        "  ailimits-auth set-usage-token <p>       store a manual usage token (claude | codex)",
        "  ailimits-auth set-usage-token <p>       保存用量令牌（claude | codex）",
    ),
    (
        "  ailimits-auth remove-usage-token <p>    remove the usage token",
        "  ailimits-auth remove-usage-token <p>    移除用量令牌",
    ),
    (
        "By default everything works through subscriptions, no keys needed:",
        "默认通过订阅读取用量，无需另外配置密钥：",
    ),
    (
        "  Claude — Claude Code token, Codex — Codex CLI token, Copilot — gh CLI token.",
        "  Claude — Claude Code 令牌，Codex — Codex CLI 令牌，Copilot — gh CLI 令牌。",
    ),
    (
        "provider '{provider}' has no key (expected: claude or copilot)",
        "服务商 '{provider}' 不支持密钥（仅支持 claude 或 copilot）",
    ),
    (
        "usage token is supported for claude and codex, got '{provider}'",
        "用量令牌仅支持 claude 和 codex，收到 '{provider}'",
    ),
    (
        "── Auth sources ─────────────────────────────────",
        "── 认证来源 ─────────────────────────────────",
    ),
    ("no (expired)", "不可用（已过期）"),
    ("no", "未找到"),
    ("stored", "已保存"),
    (
        "Claude OAuth ({sub_type} subscription): {mark}",
        "Claude OAuth（{sub_type} 订阅）：{mark}",
    ),
    (
        "  token valid until {expiry} UTC",
        "  令牌有效期至 {expiry} UTC",
    ),
    (
        "  → run Claude Code, it refreshes the token automatically",
        "  → 运行 Claude Code，它会自动刷新令牌",
    ),
    (
        "Claude OAuth: no (.credentials.json has no valid token)",
        "Claude OAuth：不可用（.credentials.json 中没有有效令牌）",
    ),
    (
        "Claude OAuth: no (.credentials.json missing — run Claude Code once)",
        "Claude OAuth：不可用（缺少 .credentials.json — 请运行一次 Claude Code）",
    ),
    (
        "{id} usage token ({label}): {state}",
        "{id} 用量令牌（{label}）：{state}",
    ),
    ("no (run codex login)", "未找到（请运行 codex login）"),
    (
        "gh CLI token (Copilot): {state}",
        "gh CLI 令牌（Copilot）：{state}",
    ),
    (
        "no (gh auth login, or store a PAT)",
        "未找到（请运行 gh auth login，或保存 PAT）",
    ),
    (
        "Antigravity keyring (gemini:antigravity): {state}",
        "Antigravity 凭据（gemini:antigravity）：{state}",
    ),
    (
        "no (run Antigravity once)",
        "未找到（请运行一次 Antigravity）",
    ),
    (
        "legacy Gemini CLI oauth_creds.json: {state}",
        "旧版 Gemini CLI oauth_creds.json：{state}",
    ),
    (
        "── Keys (Credential Manager) ─────────────────────",
        "── 密钥（Windows 凭据管理器）─────────────────────",
    ),
    ("── Config ({path}) ──", "── 配置（{path}）──"),
    ("enabled", "已启用"),
    ("disabled", "已禁用"),
    ("subscription", "订阅"),
    (
        "{provider} {state}, method: {method}",
        "{provider} {state}，认证方式：{method}",
    ),
    (
        "specify a provider: ailimits-auth set <claude|copilot>",
        "请指定服务商：ailimits-auth set <claude|copilot>",
    ),
    (
        "specify a provider: ailimits-auth set-usage-token <claude|codex>",
        "请指定服务商：ailimits-auth set-usage-token <claude|codex>",
    ),
    (
        "specify a provider: ailimits-auth remove-usage-token <claude|codex>",
        "请指定服务商：ailimits-auth remove-usage-token <claude|codex>",
    ),
    (
        "specify a provider: ailimits-auth remove <claude|copilot>",
        "请指定服务商：ailimits-auth remove <claude|copilot>",
    ),
    (
        "Paste the key for {provider}: ",
        "请粘贴 {provider} 的密钥：",
    ),
    (
        "Paste the {provider} usage token: ",
        "请粘贴 {provider} 的用量令牌：",
    ),
    ("failed to read input", "无法读取输入"),
    ("empty key — nothing stored", "密钥为空 — 未保存"),
    ("empty token — nothing stored", "令牌为空 — 未保存"),
    ("keyring unavailable", "凭据管理器不可用"),
    (
        "failed to store the key in Credential Manager",
        "无法将密钥保存到凭据管理器",
    ),
    (
        "failed to store the usage token in Credential Manager",
        "无法将用量令牌保存到凭据管理器",
    ),
    (
        "Key for '{provider}' stored in Credential Manager (label: {label})",
        "已将 '{provider}' 的密钥保存到凭据管理器（标签：{label}）",
    ),
    ("→ Restart the widget to apply", "→ 重启小组件以应用更改"),
    (
        "{provider} usage token stored (label: {label})",
        "已保存 {provider} 的用量令牌（标签：{label}）",
    ),
    (
        "the usage endpoint rejected the token: HTTP {status}",
        "用量接口拒绝了令牌：HTTP {status}",
    ),
    (
        "{provider} usage token removed",
        "已移除 {provider} 的用量令牌",
    ),
    (
        "— no {provider} usage token stored",
        "— 没有保存过 {provider} 的用量令牌",
    ),
    (
        "failed to remove the usage token: {error}",
        "无法移除用量令牌：{error}",
    ),
    (
        "Key '{label}' removed from Credential Manager",
        "已从凭据管理器移除密钥 '{label}'",
    ),
    ("— no key '{label}' stored", "— 没有保存过密钥 '{label}'"),
    ("failed to remove the key: {error}", "无法移除密钥：{error}"),
    (
        "'{provider}' switched back to the subscription method",
        "'{provider}' 已恢复使用订阅认证",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn language_resolution_and_unknown_text() {
        assert_eq!(resolve(Language::Auto, true), Language::Chinese);
        assert_eq!(resolve(Language::Auto, false), Language::English);
        assert_eq!(resolve(Language::English, true), Language::English);
        assert_eq!(translate("Session", true), "会话");
        assert_eq!(translate("Session", false), "Session");
        assert_eq!(translate("raw server error", true), "raw server error");
    }
    #[test]
    fn catalog_has_unique_keys_and_matching_placeholders() {
        fn placeholders(text: &str) -> std::collections::BTreeSet<&str> {
            text.split('{')
                .skip(1)
                .filter_map(|s| s.split_once('}').map(|(key, _)| key))
                .collect()
        }
        let mut keys = std::collections::HashSet::new();
        for (en, zh) in MESSAGES {
            assert!(keys.insert(en), "duplicate: {en}");
            assert!(!zh.is_empty());
            assert_eq!(placeholders(en), placeholders(zh), "{en}");
        }
        assert_eq!(
            interpolate("{a} {b}", &[("a", "{b}".into()), ("b", "ok".into())]),
            "{b} ok"
        );
    }
}
