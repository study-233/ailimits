// config/schema.rs — configuration structs.
// Full format documentation: docs/en/CONFIG.md.

use serde::{Deserialize, Serialize};

/// Deserialize an enum field tolerantly: an UNKNOWN value (a typo, or a value
/// renamed in a newer version) falls back to that field's own default instead
/// of failing the whole document. Without this, one bad enum string in a
/// hand-edited config.toml would make the ENTIRE config fail to parse and
/// silently reset every other setting (window position, palette, providers,
/// hooks…). This mirrors the tolerance already applied to unknown fields
/// (ignored) and unknown providers (skipped) — see docs: "old configs never
/// break parsing".
pub(super) fn de_enum_or_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    // Buffer the value, then try to interpret it as T; unknown → default.
    // Config is always TOML, so toml::Value is the right intermediate.
    let raw = toml::Value::deserialize(deserializer)?;
    Ok(T::deserialize(raw).unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub panel: super::panel::PanelConfig,
    #[serde(default)]
    pub appearance: super::appearance::Appearance,
    #[serde(default)]
    pub network: NetworkConfig,
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub window: WindowConfig,
    #[serde(default)]
    pub ui: UIConfig,
    // If config.toml has no [[providers]] sections, fall back to the defaults.
    #[serde(default = "default_providers")]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub notifications: NotificationConfig,
    #[serde(default)]
    pub hooks: HooksConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            panel: super::panel::PanelConfig::default(),
            appearance: super::appearance::Appearance::default(),
            network: NetworkConfig::default(),
            general: GeneralConfig::default(),
            window: WindowConfig::default(),
            ui: UIConfig::default(),
            // IMPORTANT: the default config MUST contain every provider,
            // otherwise the widget is empty on first launch.
            providers: default_providers(),
            notifications: NotificationConfig::default(),
            hooks: HooksConfig::default(),
        }
    }
}

/// User-configured shell commands run on usage events.
///
/// Empty strings disable the hook (the default). This is intentionally
/// config.toml-only — there is no menu/clipboard path, so a malicious
/// clipboard string can never become an auto-run command.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    /// Run when a provider crosses its alert threshold (shares the toast cooldown).
    #[serde(default)]
    pub on_threshold: String,
    /// Run when a provider's limit window resets (usage drops from near-limit).
    #[serde(default)]
    pub on_reset: String,
    /// Run once after the first successful fetch of any provider.
    #[serde(default)]
    pub on_startup: String,
}

/// Default provider set for the first launch.
fn default_providers() -> Vec<ProviderConfig> {
    // Base template to avoid field duplication.
    let base =
        |id: &str, enabled: bool, auth: AuthMethod, label: &str, threshold: u8| ProviderConfig {
            id: id.to_string(),
            enabled,
            auth_method: auth,
            credential_label: label.to_string(),
            alert_threshold: threshold,
        };

    vec![
        // Claude — subscription: the Claude Code OAuth token, no key needed.
        base("claude", true, AuthMethod::Subscription, "", 80),
        // Codex — ChatGPT subscription via the Codex CLI auth.json.
        base("codex", true, AuthMethod::Subscription, "", 80),
        // Copilot — subscription via the gh CLI token or a PAT.
        base("copilot", true, AuthMethod::Subscription, "", 85),
        // Antigravity (the Gemini successor) — keyring token first, then the
        // legacy Gemini CLI file.
        base("antigravity", true, AuthMethod::Subscription, "", 80),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default)]
    pub panel_locked: bool,
    /// Manual X in DIP relative to the selected taskbar; None means automatic.
    #[serde(default)]
    pub panel_position_x: Option<i32>,
    /// Desktop widget visibility. The taskbar is the default entry point.
    #[serde(default)]
    pub show_widget: bool,
    #[serde(default, deserialize_with = "de_enum_or_default")]
    pub language: Language,
    #[serde(default = "default_update_interval")]
    pub update_interval_secs: u64,
    /// Taskbar entry point; defaults to the Codex weekly remaining panel.
    /// The legacy `show_tray_icon` field is ignored.
    #[serde(default, deserialize_with = "de_enum_or_default")]
    pub indicator: IndicatorKind,
    /// Silently install newer releases in the background. On by default; the
    /// context menu exposes a toggle. Old configs without the field default to
    /// enabled, matching the shipped behaviour.
    #[serde(default = "default_auto_update")]
    pub auto_update: bool,
    /// Manual nudge for the taskbar panel, in pixels, applied after the
    /// computed position. Config-only: a repair tool for shell replacements
    /// and unusual taskbars, not a preference. Clamped to +/-200.
    #[serde(default)]
    pub panel_offset_x: i32,
    #[serde(default)]
    pub panel_offset_y: i32,
    /// Taskbar the panel indicator attaches to. Falls back to the primary
    /// when the chosen display is gone.
    #[serde(default, deserialize_with = "de_enum_or_default")]
    pub panel_display: PanelDisplay,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            panel_locked: false,
            panel_position_x: None,
            show_widget: false,
            language: Language::Auto,
            update_interval_secs: default_update_interval(),
            indicator: IndicatorKind::default(),
            auto_update: default_auto_update(),
            panel_offset_x: 0,
            panel_offset_y: 0,
            panel_display: PanelDisplay::Primary,
        }
    }
}

impl GeneralConfig {
    /// Keep a reachable menu when the desktop widget is disabled.
    pub fn ensure_visible_entry(&mut self) {
        self.show_widget = false;
        if self.indicator == IndicatorKind::Off {
            self.indicator = IndicatorKind::PanelRows;
        }
        if self.indicator == IndicatorKind::Bars {
            self.indicator = IndicatorKind::Tray;
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "zh-CN")]
    Chinese,
    #[serde(rename = "en")]
    English,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkConfig {
    #[serde(default, deserialize_with = "de_enum_or_default")]
    pub proxy_mode: ProxyMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    #[default]
    System,
    Direct,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    #[serde(default = "default_pos_x")]
    pub pos_x: i32,
    #[serde(default = "default_pos_y")]
    pub pos_y: i32,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// Always on top.
    #[serde(default)]
    pub pinned: bool,
    /// Locked position — dragging disabled; INDEPENDENT of `pinned`.
    #[serde(default)]
    pub locked: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            pos_x: default_pos_x(),
            pos_y: default_pos_y(),
            opacity: default_opacity(),
            pinned: false,
            locked: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIConfig {
    #[serde(default, deserialize_with = "de_enum_or_default")]
    pub palette: Palette,
    #[serde(default = "default_saturation")]
    pub saturation: u8,
    /// Text and bar brightness, 20–100%.
    #[serde(default = "default_brightness")]
    pub brightness: u8,
    #[serde(default)]
    pub monochrome: bool,
    #[serde(default, deserialize_with = "de_enum_or_default")]
    pub layout: Layout,
    #[serde(default, deserialize_with = "de_enum_or_default")]
    pub detail: DetailLevel,
    /// Width step for the rows layout; the columns layout ignores it.
    #[serde(default, deserialize_with = "de_enum_or_default")]
    pub width_scale: WidthScale,
    /// Column arrangement for the vertical-bars layout; the rows layout
    /// ignores it. The two settings are the same choice on opposite axes.
    #[serde(default, deserialize_with = "de_enum_or_default")]
    pub column_flow: ColumnFlow,
    /// Show a burn-rate forecast ("~Xh to limit") when usage is climbing.
    /// It never replaces the reset countdown — it only fills the slot when no
    /// future reset is known. Off by default: the reset time is the fact
    /// users plan around, the projection is optional extra.
    #[serde(default)]
    pub show_forecast: bool,
}

impl Default for UIConfig {
    fn default() -> Self {
        Self {
            palette: Palette::Default,
            saturation: default_saturation(),
            brightness: default_brightness(),
            monochrome: false,
            layout: Layout::Vertical,
            detail: DetailLevel::Compact,
            width_scale: WidthScale::Full,
            column_flow: ColumnFlow::Row,
            show_forecast: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, deserialize_with = "de_enum_or_default")]
    pub auth_method: AuthMethod,
    /// Credential Manager key label; empty for the subscription method.
    #[serde(default)]
    pub credential_label: String,
    /// Notification threshold, %.
    #[serde(default = "default_threshold")]
    pub alert_threshold: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Cooldown per provider and quota window, minutes.
    #[serde(default = "default_cooldown")]
    pub cooldown_minutes: u32,
    /// Used-percent thresholds. None inherits the Codex provider threshold.
    #[serde(default, deserialize_with = "de_enum_or_default")]
    pub codex_session_threshold: Option<u8>,
    #[serde(default, deserialize_with = "de_enum_or_default")]
    pub codex_weekly_threshold: Option<u8>,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cooldown_minutes: default_cooldown(),
            codex_session_threshold: None,
            codex_weekly_threshold: None,
        }
    }
}

impl NotificationConfig {
    pub fn codex_threshold(&self, window: crate::providers::MetricWindow, fallback: u8) -> u8 {
        match window {
            crate::providers::MetricWindow::Session => self.codex_session_threshold,
            crate::providers::MetricWindow::Long => self.codex_weekly_threshold,
        }
        .unwrap_or(fallback)
        .min(100)
    }
}

// ─── Enum types ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Palette {
    #[default]
    Default,
    Ocean,
    Sunset,
    Forest,
    Neon,
    Ice,
    Rose,
    Slate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    #[default]
    Vertical,
    Horizontal,
    /// Legacy value kept for old configs; renders as Vertical.
    Grid,
}

/// Widget width as a fraction of its natural width, for the rows layout.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WidthScale {
    #[default]
    Full,
    ThreeQuarters,
    Half,
}

/// How the provider columns are arranged when the bars are vertical.
/// Ignored by the rows layout, which has no columns to arrange.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ColumnFlow {
    /// Side by side: a wide, short widget.
    #[default]
    Row,
    /// Stacked downwards: a narrow, tall widget.
    Column,
}

impl WidthScale {
    /// The fraction of the natural width this step keeps.
    pub fn fraction(&self) -> f32 {
        match self {
            Self::Full => 1.0,
            Self::ThreeQuarters => 0.75,
            Self::Half => 0.5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DetailLevel {
    #[default]
    Compact,
    Medium,
    Expanded,
}

/// Taskbar indicator: the default panel shows Codex weekly remaining quota.
/// Tray and Bars retain legacy usage icons. PanelGrid aliases PanelRows.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum IndicatorKind {
    Tray,
    Bars,
    #[default]
    PanelRows,
    PanelGrid,
    Off,
}

/// Which taskbar the mini panel attaches to. Secondary bars are numbered
/// left to right, starting at 0, across the taskbars that actually exist.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PanelDisplay {
    #[default]
    Primary,
    Secondary(u8),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    #[default]
    Subscription,
    ApiKey,
}

// ─── Default values ───────────────────────────────────────────────
fn default_update_interval() -> u64 {
    60
}
fn default_auto_update() -> bool {
    true
}
fn default_opacity() -> f32 {
    0.45
}
fn default_pos_x() -> i32 {
    50
}
fn default_pos_y() -> i32 {
    50
}
fn default_brightness() -> u8 {
    100
}
fn default_saturation() -> u8 {
    55
}
fn default_threshold() -> u8 {
    80
}
fn default_cooldown() -> u32 {
    15
}
fn default_true() -> bool {
    true
}
