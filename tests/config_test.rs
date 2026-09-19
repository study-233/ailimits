// tests/config_test.rs — configuration tests.

use ailimits::config::schema::{Config, DetailLevel, IndicatorKind, Layout, Palette, PanelDisplay};

#[test]
fn default_config_has_expected_providers() {
    // The default config contains all four providers, enabled, no keys.
    let config = Config::default();
    assert_eq!(config.providers.len(), 4);
    for p in &config.providers {
        assert!(p.enabled, "{} should be enabled by default", p.id);
        assert!(p.credential_label.is_empty());
    }
    let ids: Vec<&str> = config.providers.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, vec!["claude", "codex", "copilot", "antigravity"]);
}

#[test]
fn empty_toml_parses_to_defaults() {
    // An empty TOML must not break parsing.
    let config: Config = toml::from_str("").expect("empty config should parse");
    assert_eq!(config.providers.len(), 4);
}

#[test]
fn quota_alert_thresholds_migrate_and_preserve_independent_values() {
    use ailimits::providers::MetricWindow;
    let old: Config =
        toml::from_str("[notifications]\nenabled = false\ncooldown_minutes = 20").unwrap();
    assert_eq!(
        old.notifications.codex_threshold(MetricWindow::Session, 85),
        85
    );
    assert_eq!(
        old.notifications.codex_threshold(MetricWindow::Long, 85),
        85
    );
    let config: Config = toml::from_str(
        "[notifications]\ncodex_session_threshold = 90\ncodex_weekly_threshold = 70",
    )
    .unwrap();
    let roundtrip: Config = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
    assert_eq!(
        roundtrip
            .notifications
            .codex_threshold(MetricWindow::Session, 80),
        90
    );
    assert_eq!(
        roundtrip
            .notifications
            .codex_threshold(MetricWindow::Long, 80),
        70
    );
    let malformed: Config = toml::from_str("[notifications]\nenabled = false\ncodex_session_threshold = 'bad'\ncodex_weekly_threshold = 200").unwrap();
    assert!(!malformed.notifications.enabled);
    assert_eq!(
        malformed
            .notifications
            .codex_threshold(MetricWindow::Session, 80),
        80
    );
    assert_eq!(
        malformed
            .notifications
            .codex_threshold(MetricWindow::Long, 80),
        100
    );
}

#[test]
fn manual_taskbar_position_roundtrips_and_old_configs_stay_automatic() {
    let old: Config = toml::from_str("[general]\nindicator = \"panel_rows\"").unwrap();
    assert_eq!(old.general.panel_position_x, None);
    let mut config = old;
    config.general.panel_position_x = Some(420);
    let parsed: Config = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
    assert_eq!(parsed.general.panel_position_x, Some(420));
}

#[test]
fn widget_visibility_persists_and_disabled_widget_keeps_an_entry() {
    let mut config: Config =
        toml::from_str("[general]\nindicator = \"off\"\nupdate_interval_secs = 900").unwrap();
    assert!(!config.general.show_widget);
    config.general.ensure_visible_entry();
    assert_eq!(config.general.indicator, IndicatorKind::PanelRows);
    assert_eq!(config.general.update_interval_secs, 900);
    for show in [true, false] {
        config.general.show_widget = show;
        let parsed: Config = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
        assert_eq!(parsed.general.show_widget, show);
    }
    config.general.show_widget = true;
    config.general.indicator = IndicatorKind::Off;
    config.general.ensure_visible_entry();
    assert_eq!(config.general.indicator, IndicatorKind::PanelRows);
}

#[test]
fn legacy_config_with_removed_fields_still_parses() {
    // A legacy config with removed fields must not break parsing —
    // unknown fields and providers are ignored.
    let toml_str = r#"
        [[providers]]
        id = "gemini"
        enabled = true
        manual_rpd_limit = 1000
        probe_interval_secs = 300
        github_username = "x"
    "#;
    let config: Config = toml::from_str(toml_str).expect("legacy config should parse");
    assert_eq!(config.providers.len(), 1);
}

#[test]
fn partial_toml_keeps_defaults_for_missing_fields() {
    // A partial config: only one (legacy) field is set.
    let toml_str = r#"
        [ui]
        theme = "light"
    "#;
    let config: Config = toml::from_str(toml_str).expect("partial config should parse");
    // The remaining fields fall back to the defaults.
    assert_eq!(config.general.update_interval_secs, 60);
}

#[test]
fn indicator_defaults_to_panel_and_parses_values() {
    // The default entry point is the weekly remaining taskbar panel.
    assert_eq!(
        Config::default().general.indicator,
        IndicatorKind::PanelRows
    );

    let config: Config = toml::from_str("[general]\nindicator = \"bars\"").expect("parse");
    assert_eq!(config.general.indicator, IndicatorKind::Bars);
    let config: Config = toml::from_str("[general]\nindicator = \"panel_rows\"").expect("parse");
    assert_eq!(config.general.indicator, IndicatorKind::PanelRows);
    let config: Config = toml::from_str("[general]\nindicator = \"panel_grid\"").expect("parse");
    assert_eq!(config.general.indicator, IndicatorKind::PanelGrid);
    let config: Config = toml::from_str("[general]\nindicator = \"off\"").expect("parse");
    assert_eq!(config.general.indicator, IndicatorKind::Off);
}

#[test]
fn legacy_show_tray_icon_field_is_ignored() {
    // Pre-0.3 configs carry `show_tray_icon`; the unknown field must be
    // ignored and the indicator falls back to the taskbar panel.
    let config: Config =
        toml::from_str("[general]\nshow_tray_icon = false").expect("legacy config should parse");
    assert_eq!(config.general.indicator, IndicatorKind::PanelRows);
}

#[test]
fn unknown_enum_value_falls_back_without_wiping_siblings() {
    // The core regression: a single invalid enum value must NOT fail the whole
    // document and reset every other setting. The bad enum becomes its default;
    // the valid sibling fields are preserved.
    let toml_str = r#"
        [general]
        update_interval_secs = 900
        indicator = "totally_bogus_indicator"

        [ui]
        palette = "chartreuse"
        layout = "diagonal"
        detail = "ultra"
        brightness = 42

        [window]
        pos_x = 1234
        pinned = true

        [[providers]]
        id = "claude"
        enabled = false
        auth_method = "telepathy"
    "#;
    let config: Config = toml::from_str(toml_str).expect("must not fail on bad enum values");

    // Bad enums degraded to their own defaults…
    assert_eq!(config.general.indicator, IndicatorKind::default());
    assert_eq!(config.ui.palette, Palette::default());
    assert_eq!(config.ui.layout, Layout::default());
    assert_eq!(config.ui.detail, DetailLevel::default());
    // …while every valid sibling value SURVIVED (the bug wiped these to 0/default).
    assert_eq!(config.general.update_interval_secs, 900);
    assert_eq!(config.ui.brightness, 42);
    assert_eq!(config.window.pos_x, 1234);
    assert!(config.window.pinned);
    let claude = config.providers.iter().find(|p| p.id == "claude").unwrap();
    assert!(
        !claude.enabled,
        "the valid enabled=false must survive a bad auth_method"
    );
}

#[test]
fn valid_enum_values_still_parse() {
    let toml_str = r#"
        [general]
        indicator = "panel_rows"
        [ui]
        palette = "ocean"
        layout = "horizontal"
        detail = "expanded"
    "#;
    let config: Config = toml::from_str(toml_str).expect("parse");
    assert_eq!(config.general.indicator, IndicatorKind::PanelRows);
    assert_eq!(config.ui.palette, Palette::Ocean);
    assert_eq!(config.ui.layout, Layout::Horizontal);
    assert_eq!(config.ui.detail, DetailLevel::Expanded);
}

#[test]
fn panel_display_parses_primary_and_data_carrying_secondary_and_falls_back_on_nonsense() {
    // `Primary` is a plain string variant. `PanelDisplay::Primary` is also
    // what `de_enum_or_default` falls back to on ANY parse failure, so
    // asserting the parse of `"primary"` equals `Primary` alone would pass
    // even if `#[serde(rename_all = "snake_case")]` were dropped from
    // `PanelDisplay` (the token would then be `"Primary"`, parsing would
    // fail, and fallback would coincidentally produce the same value).
    // Round-trip through serialization instead: this proves the lowercase
    // token is what the type actually produces and accepts, not just what
    // the fallback happens to produce.
    let mut config = Config::default();
    config.general.panel_display = PanelDisplay::Primary;
    let serialized = toml::to_string_pretty(&config).expect("serialize primary");
    assert!(
        serialized.contains("panel_display = \"primary\""),
        "Primary must serialize to the lowercase token, not a coincidental fallback:\n{serialized}"
    );
    let reparsed: Config = toml::from_str(&serialized).expect("reparse primary");
    assert_eq!(reparsed.general.panel_display, PanelDisplay::Primary);

    // `Secondary(u8)` is data-carrying — its TOML form is an inline table
    // with the variant name as the key.
    let config: Config =
        toml::from_str("[general]\npanel_display = { secondary = 0 }").expect("parse secondary");
    assert_eq!(config.general.panel_display, PanelDisplay::Secondary(0));

    // A nonsense value must fall back to the default (Primary) instead of
    // failing the whole document, exactly like every other enum field here.
    let config: Config =
        toml::from_str("[general]\npanel_display = \"banana\"").expect("bad value must not fail");
    assert_eq!(config.general.panel_display, PanelDisplay::Primary);
}

#[test]
fn config_roundtrip_serialization() {
    // Serialize → deserialize losslessly.
    let config = Config::default();
    let serialized = toml::to_string_pretty(&config).expect("should serialize");
    let parsed: Config = toml::from_str(&serialized).expect("should parse back");
    assert_eq!(parsed.providers.len(), config.providers.len());
}
#[test]
fn language_and_proxy_preferences_are_backward_compatible() {
    use ailimits::config::schema::{Config, Language, ProxyMode};
    let old: Config = toml::from_str("[general]\nupdate_interval_secs = 900").unwrap();
    assert_eq!(old.general.language, Language::Auto);
    assert_eq!(old.network.proxy_mode, ProxyMode::System);
    assert_eq!(old.general.update_interval_secs, 900);
    let explicit: Config =
        toml::from_str("[general]\nlanguage = 'zh-CN'\n[network]\nproxy_mode = 'direct'").unwrap();
    let restored: Config = toml::from_str(&toml::to_string(&explicit).unwrap()).unwrap();
    assert_eq!(restored.general.language, Language::Chinese);
    assert_eq!(restored.network.proxy_mode, ProxyMode::Direct);
    let invalid: Config = toml::from_str("[general]\nlanguage = 'unknown'\nupdate_interval_secs = 900\n[network]\nproxy_mode = 'unknown'").unwrap();
    assert_eq!(invalid.general.language, Language::Auto);
    assert_eq!(invalid.network.proxy_mode, ProxyMode::System);
    assert_eq!(invalid.general.update_interval_secs, 900);
}

#[test]
fn lock_migration_and_bad_appearance_preserve_siblings() {
    use ailimits::config::storage::migrate_taskbar_preferences;
    let legacy="[general]\nshow_widget=true\nindicator='off'\nupdate_interval_secs=900\n[window]\nlocked=true\n[appearance]\nring_color='bad'\nnumber_color='#123456'\nring_size='large'\nnumber_x=127\nnumber_weight='unknown'";
    let mut config: Config = toml::from_str(legacy).unwrap();
    migrate_taskbar_preferences(&mut config, legacy);
    assert!(config.general.panel_locked);
    assert!(!config.general.show_widget);
    assert_eq!(config.general.indicator, IndicatorKind::PanelRows);
    assert_eq!(config.general.update_interval_secs, 900);
    assert_eq!(config.appearance.ring_size, 28);
    assert_eq!(config.appearance.number_x, 127);
    assert_eq!(config.appearance.number_color, "#123456");
    let new = legacy.replace("[general]", "[general]\npanel_locked=false");
    let mut config: Config = toml::from_str(&new).unwrap();
    migrate_taskbar_preferences(&mut config, &new);
    assert!(!config.general.panel_locked);
    let serialized = toml::to_string(&config).unwrap();
    let mut restored: Config = toml::from_str(&serialized).unwrap();
    migrate_taskbar_preferences(&mut restored, &serialized);
    assert!(!restored.general.panel_locked);
    assert_eq!(restored.appearance, config.appearance);
}
