use crate::config::schema::{Language, ProxyMode};
use crate::i18n::t;
// ui/context_menu.rs — the right-click context menu.
//
// A NATIVE menu via muda — no custom popup window. Provider authorization
// lives in the "Providers" submenu: keys and tokens are pasted FROM THE
// CLIPBOARD (a native menu has no text input).

use crate::config::schema::{AuthMethod, Config, IndicatorKind, PanelDisplay};
use anyhow::Result;
use muda::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

/// Update interval steps, seconds: 1 / 5 / 15 / 30 min.
const INTERVAL_STEPS: &[u64] = &[60, 300, 900, 1800];

/// Menu actions.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuAction {
    SetLanguage(Language),
    SetProxyMode(ProxyMode),
    ToggleLock,
    ResetPanelPosition,
    OpenAppearance,
    SetIndicator(IndicatorKind),
    SetPanelDisplay(PanelDisplay),
    ToggleAutoUpdate,
    /// Update interval, seconds.
    SetUpdateInterval(u64),
    /// Enable/disable a provider.
    ToggleProvider(String),
    /// Provider auth method.
    SetAuthMethod(String, AuthMethod),
    /// Paste an API key / PAT from the clipboard.
    PasteKey(String),
    /// Remove the API key / PAT.
    RemoveKey(String),
    /// Paste a manual usage token from the clipboard (claude, codex).
    PasteUsageToken(String),
    /// Remove the manual usage token.
    RemoveUsageToken(String),
    Quit,
}

/// The interval step closest to a value.
fn closest_interval(value: u64) -> u64 {
    INTERVAL_STEPS
        .iter()
        .copied()
        .min_by_key(|s| s.abs_diff(value))
        .unwrap_or(value)
}

/// Whether a config indicator value selects the given menu item. PanelGrid
/// is a config-only alias of PanelRows (the overlay renders them the same),
/// so either checks the single "Taskbar panel" item.
fn indicator_matches(config_kind: IndicatorKind, item_kind: IndicatorKind) -> bool {
    let canon = |k: IndicatorKind| match k {
        IndicatorKind::PanelGrid => IndicatorKind::PanelRows,
        other => other,
    };
    canon(config_kind) == canon(item_kind)
}

/// The full context menu: view, behavior, appearance, providers, actions.
pub struct ContextMenu {
    #[cfg(windows)]
    _style: super::native_menu::MenuStyle,
    pub menu: Menu,
    language_items: Vec<(CheckMenuItem, Language)>,
    proxy_items: Vec<(CheckMenuItem, ProxyMode)>,
    reset_position_item: MenuItem,
    lock_item: CheckMenuItem,
    appearance_item: MenuItem,
    indicator_items: Vec<(CheckMenuItem, IndicatorKind)>,
    /// Which taskbar the mini panel attaches to. Empty on a single-display
    /// machine — see the "Only worth showing..." comment where it's built.
    display_items: Vec<(CheckMenuItem, PanelDisplay)>,
    auto_update_item: CheckMenuItem,
    interval_items: Vec<(CheckMenuItem, u64)>,
    /// Per-provider enable toggles.
    provider_toggles: Vec<(CheckMenuItem, String)>,
    /// Auth method choice: only Claude has options.
    auth_method_items: Vec<(CheckMenuItem, String, AuthMethod)>,
    paste_key_items: Vec<(MenuItem, String)>,
    remove_key_items: Vec<(MenuItem, String)>,
    paste_usage_items: Vec<(MenuItem, String)>,
    remove_usage_items: Vec<(MenuItem, String)>,
    quit_item: MenuItem,
}

impl ContextMenu {
    pub fn new(config: &Config) -> Result<Self> {
        let menu = Menu::new();
        let reset_position_item = MenuItem::new(t("Restore automatic position"), true, None);
        let lock_item =
            CheckMenuItem::new(t("Lock position"), true, config.general.panel_locked, None);
        let appearance_item = MenuItem::new(t("Appearance settings…"), true, None);
        // "Indicator" submenu — radio-style choice of the secondary indicator.
        let indicator_submenu = Submenu::new(t("Indicator"), true);
        // The legacy 16px "bars" tray icon stays config-only (too small to
        // read). A single "Taskbar panel" entry: the transparent overlay
        // renders identically for panel_rows and panel_grid (the grid layout
        // now shows the weekly remaining ring), so panel_grid in an old config
        // is just an alias that checks this same item.
        let indicator_items: Vec<(CheckMenuItem, IndicatorKind)> = [
            (t("Tray icon"), IndicatorKind::Tray),
            (t("Taskbar panel"), IndicatorKind::PanelRows),
        ]
        .into_iter()
        .map(|(label, k)| {
            (
                CheckMenuItem::new(
                    label,
                    true,
                    indicator_matches(config.general.indicator, k),
                    None,
                ),
                k,
            )
        })
        .collect();
        for (item, _) in &indicator_items {
            indicator_submenu.append(item)?;
        }

        // "Display" submenu — which taskbar the mini panel attaches to.
        // Only worth showing when there IS another taskbar: a one-entry
        // "Display" submenu on a single-monitor machine is pure noise.
        let ui_display = config.general.panel_display;
        #[cfg(target_os = "windows")]
        let bars = crate::platform::secondary_taskbars();
        // No secondary taskbars to offer on other platforms.
        #[cfg(not(target_os = "windows"))]
        let bars: Vec<isize> = Vec::new();
        let mut display_items: Vec<(CheckMenuItem, PanelDisplay)> = Vec::new();
        tracing::debug!(
            "menu: {} secondary taskbar(s) found, Display submenu {}",
            bars.len(),
            if bars.is_empty() { "OMITTED" } else { "shown" }
        );
        if !bars.is_empty() {
            let display_submenu = Submenu::new(t("Display"), true);
            let mut entries = vec![(t("Primary").to_string(), PanelDisplay::Primary)];
            for i in 0..bars.len() {
                entries.push((
                    crate::tr!("Display {number}", number = i + 2),
                    PanelDisplay::Secondary(i as u8),
                ));
            }
            for (label, target) in entries {
                let item = CheckMenuItem::new(label, true, ui_display == target, None);
                display_submenu.append(&item)?;
                display_items.push((item, target));
            }
            indicator_submenu.append(&display_submenu)?;
        }

        let auto_update_item = CheckMenuItem::new(
            t("Automatic updates"),
            true,
            config.general.auto_update,
            None,
        );

        // Update interval submenu: 1 / 5 / 15 / 30 min.
        let interval_submenu = Submenu::new(t("Update interval"), true);
        let interval_marked = closest_interval(config.general.update_interval_secs);
        let mut interval_items = Vec::new();
        for &secs in INTERVAL_STEPS {
            let item = CheckMenuItem::new(
                crate::tr!("{minutes} min", minutes = secs / 60),
                true,
                secs == interval_marked,
                None,
            );
            interval_submenu.append(&item)?;
            interval_items.push((item, secs));
        }

        // "Providers" submenu with authorization.
        let providers_submenu = Submenu::new(t("Providers"), true);
        let mut provider_toggles = Vec::new();
        let mut auth_method_items = Vec::new();
        let mut paste_key_items = Vec::new();
        let mut remove_key_items = Vec::new();
        let mut paste_usage_items = Vec::new();
        let mut remove_usage_items = Vec::new();

        for pc in &config.providers {
            let display = crate::providers::ProviderId::from_config_id(&pc.id)
                .map(|p| p.display_name())
                .unwrap_or("?");
            let sub = Submenu::new(display, true);

            let toggle = CheckMenuItem::new(t("Enabled"), true, pc.enabled, None);
            sub.append(&toggle)?;
            provider_toggles.push((toggle, pc.id.clone()));

            // Method choice — Claude only: subscription or API key.
            // Codex and Copilot are subscription-only.
            if pc.id == "claude" {
                sub.append(&PredefinedMenuItem::separator())?;
                let sub_item = CheckMenuItem::new(
                    t("Claude Code subscription"),
                    true,
                    pc.auth_method == AuthMethod::Subscription,
                    None,
                );
                let key_item = CheckMenuItem::new(
                    t("API key"),
                    true,
                    pc.auth_method == AuthMethod::ApiKey,
                    None,
                );
                sub.append(&sub_item)?;
                sub.append(&key_item)?;
                auth_method_items.push((sub_item, pc.id.clone(), AuthMethod::Subscription));
                auth_method_items.push((key_item, pc.id.clone(), AuthMethod::ApiKey));
            }

            // Keys — only where they exist: Claude (API key), Copilot (PAT).
            if crate::providers::key_label_for(&pc.id).is_some() {
                sub.append(&PredefinedMenuItem::separator())?;
                let paste_label = if pc.id == "copilot" {
                    // The PAT is optional: without it the gh CLI token is used.
                    "Paste PAT from clipboard (optional)"
                } else {
                    "Paste API key from clipboard"
                };
                let paste = MenuItem::new(t(paste_label), true, None);
                let remove = MenuItem::new(t("Remove key"), true, None);
                sub.append(&paste)?;
                sub.append(&remove)?;
                paste_key_items.push((paste, pc.id.clone()));
                remove_key_items.push((remove, pc.id.clone()));
            }

            // Manual usage tokens — Claude and Codex: an alternative to the
            // auto-detected CLI tokens.
            if crate::providers::usage_token_label_for(&pc.id).is_some() {
                sub.append(&PredefinedMenuItem::separator())?;
                let paste = MenuItem::new(t("Paste usage token from clipboard"), true, None);
                let remove = MenuItem::new(t("Remove usage token"), true, None);
                sub.append(&paste)?;
                sub.append(&remove)?;
                paste_usage_items.push((paste, pc.id.clone()));
                remove_usage_items.push((remove, pc.id.clone()));
            }

            providers_submenu.append(&sub)?;
        }

        let language_submenu = Submenu::new(t("Language"), true);
        let mut language_items = Vec::new();
        for (label, language) in [
            (t("Follow system"), Language::Auto),
            ("简体中文", Language::Chinese),
            ("English", Language::English),
        ] {
            let item = CheckMenuItem::new(label, true, config.general.language == language, None);
            language_submenu.append(&item)?;
            language_items.push((item, language));
        }
        let proxy_submenu = Submenu::new(t("Network proxy"), true);
        let mut proxy_items = Vec::new();
        for (label, mode) in [
            (t("Follow system"), ProxyMode::System),
            (t("Direct connection"), ProxyMode::Direct),
        ] {
            let item = CheckMenuItem::new(label, true, config.network.proxy_mode == mode, None);
            proxy_submenu.append(&item)?;
            proxy_items.push((item, mode));
        }

        let quit_item = MenuItem::new(t("Quit"), true, None);

        menu.append(&appearance_item)?;
        menu.append(&lock_item)?;
        menu.append(&indicator_submenu)?;
        menu.append(&reset_position_item)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&auto_update_item)?;
        menu.append(&interval_submenu)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&providers_submenu)?;
        menu.append(&language_submenu)?;
        menu.append(&proxy_submenu)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&quit_item)?;
        // Version — a disabled info line.
        menu.append(&MenuItem::new(
            format!("QuotaBar v{}", env!("CARGO_PKG_VERSION")),
            false,
            None,
        ))?;

        #[cfg(windows)]
        let style = {
            use muda::ContextMenu as _;
            super::native_menu::MenuStyle::new(menu.hpopupmenu())
        };
        Ok(Self {
            #[cfg(windows)]
            _style: style,
            menu,
            language_items,
            proxy_items,
            reset_position_item,
            lock_item,
            appearance_item,
            indicator_items,
            display_items,
            auto_update_item,
            interval_items,
            provider_toggles,
            auth_method_items,
            paste_key_items,
            remove_key_items,
            paste_usage_items,
            remove_usage_items,
            quit_item,
        })
    }

    /// Map a menu event id to an action.
    pub fn action_for(&self, event_id: &muda::MenuId) -> Option<MenuAction> {
        if *event_id == self.reset_position_item.id() {
            return Some(MenuAction::ResetPanelPosition);
        }
        if *event_id == self.appearance_item.id() {
            return Some(MenuAction::OpenAppearance);
        }
        for (item, language) in &self.language_items {
            if *event_id == item.id() {
                return Some(MenuAction::SetLanguage(*language));
            }
        }
        for (item, mode) in &self.proxy_items {
            if *event_id == item.id() {
                return Some(MenuAction::SetProxyMode(*mode));
            }
        }

        for (item, v) in &self.interval_items {
            if *event_id == item.id() {
                return Some(MenuAction::SetUpdateInterval(*v));
            }
        }
        if *event_id == self.lock_item.id() {
            return Some(MenuAction::ToggleLock);
        }
        for (item, k) in &self.indicator_items {
            if *event_id == item.id() {
                return Some(MenuAction::SetIndicator(*k));
            }
        }
        for (item, target) in &self.display_items {
            if *event_id == item.id() {
                return Some(MenuAction::SetPanelDisplay(*target));
            }
        }
        if *event_id == self.auto_update_item.id() {
            return Some(MenuAction::ToggleAutoUpdate);
        }
        for (item, id) in &self.provider_toggles {
            if *event_id == item.id() {
                return Some(MenuAction::ToggleProvider(id.clone()));
            }
        }
        for (item, id, method) in &self.auth_method_items {
            if *event_id == item.id() {
                return Some(MenuAction::SetAuthMethod(id.clone(), method.clone()));
            }
        }
        for (item, id) in &self.paste_key_items {
            if *event_id == item.id() {
                return Some(MenuAction::PasteKey(id.clone()));
            }
        }
        for (item, id) in &self.remove_key_items {
            if *event_id == item.id() {
                return Some(MenuAction::RemoveKey(id.clone()));
            }
        }
        for (item, id) in &self.paste_usage_items {
            if *event_id == item.id() {
                return Some(MenuAction::PasteUsageToken(id.clone()));
            }
        }
        for (item, id) in &self.remove_usage_items {
            if *event_id == item.id() {
                return Some(MenuAction::RemoveUsageToken(id.clone()));
            }
        }
        if *event_id == self.quit_item.id() {
            Some(MenuAction::Quit)
        } else {
            None
        }
    }

    /// Sync the checkmarks with the config.
    pub fn sync(&self, config: &Config) {
        for (item, language) in &self.language_items {
            item.set_checked(*language == config.general.language);
        }
        for (item, mode) in &self.proxy_items {
            item.set_checked(*mode == config.network.proxy_mode);
        }
        let interval_marked = closest_interval(config.general.update_interval_secs);
        for (item, v) in &self.interval_items {
            item.set_checked(*v == interval_marked);
        }
        self.lock_item.set_checked(config.general.panel_locked);
        for (item, k) in &self.indicator_items {
            item.set_checked(indicator_matches(config.general.indicator, *k));
        }
        for (item, target) in &self.display_items {
            item.set_checked(config.general.panel_display == *target);
        }
        self.auto_update_item
            .set_checked(config.general.auto_update);
        for (item, id) in &self.provider_toggles {
            let enabled = config
                .providers
                .iter()
                .find(|p| p.id == *id)
                .is_some_and(|p| p.enabled);
            item.set_checked(enabled);
        }
        for (item, id, method) in &self.auth_method_items {
            let active = config
                .providers
                .iter()
                .find(|p| p.id == *id)
                .is_some_and(|p| p.auth_method == *method);
            item.set_checked(active);
        }
    }
}
