// app.rs — main application state and the event loop.
//
// Threads:
//   Thread 1: the tao event loop — window events, rendering, AppCommand.
//   Thread 2+: the tokio runtime — the scheduler and provider fetches.

use crate::{
    config::{
        schema::{AuthMethod, Config},
        storage,
    },
    hooks,
    monitor::scheduler::{Scheduler, SharedInterval, SharedProviders},
    notifications::toast::ToastNotifier,
    providers::{
        antigravity::AntigravityProvider, claude::ClaudeProvider, codex::CodexProvider,
        copilot::CopilotProvider, key_label_for, usage_token_label_for, Metric, MetricWindow,
        Provider, ProviderData, ProviderId, ProviderStatus,
    },
    ui::{
        context_menu::{ContextMenu, MenuAction},
        taskbar_panel::TaskbarPanel,
        text::format_duration,
        theme::ComputedTheme,
        tray::Tray,
    },
    updater,
};
use anyhow::{Context as AnyhowContext, Result};
use chrono::{Duration, Utc};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use tao::{
    event::{ElementState, Event, MouseButton, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::WindowBuilder,
};
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Commands for the event loop.
#[derive(Debug)]
pub enum AppCommand {
    /// Update a single provider's data.
    UpdateProvider(ProviderData),
    /// Quit the application.
    Quit,
}

/// tao user events: commands + menu clicks + tray clicks + taskbar moves.
#[derive(Debug)]
pub enum UserEvent {
    AppearancePreview(crate::config::appearance::Appearance),
    AppearanceSave(crate::config::appearance::Appearance),
    AppearanceCancel,
    Command(AppCommand),
    Menu(muda::MenuId),
    Tray(TrayKind),
    /// The taskbar slid/moved (auto-hide, resolution change) or the
    /// notification area changed width (icon pinned/unpinned) — the embedded
    /// panel follows it. Sent by the SetWinEventHook watcher.
    TaskbarMoved,
    /// A window came to the foreground (e.g. the tray overflow flyout) and may
    /// have covered the panel overlay — re-assert its topmost z-order.
    PanelRaise,
    /// The shell re-stacked the taskbar's z-order (EVENT_OBJECT_REORDER) with no
    /// move/foreground event — e.g. the auto-hide bar peeking back fronts itself
    /// above the overlay. Re-check coverage and toggle the tray fallback.
    #[cfg(target_os = "windows")]
    PanelRecheck,
}

/// Tray interactions we care about.
#[derive(Debug)]
pub enum TrayKind {
    /// Left-click → toggle the overlay window's visibility.
    LeftClick,
}

/// Fullscreen visibility never changes the user's chosen indicator mode.
#[cfg(target_os = "windows")]
fn eval_panel_visibility(
    panel: &mut TaskbarPanel,
    providers: &[ProviderData],
    fullscreen_was_active: &mut bool,
    target: crate::config::schema::PanelDisplay,
) {
    let fullscreen = crate::platform::fullscreen_foreground_active(target);
    if fullscreen {
        panel.suppress_for_fullscreen();
    } else if *fullscreen_was_active {
        panel.restore_from_fullscreen(providers);
    } else if !crate::platform::foreground_scrim_active(target) {
        panel.raise();
    }
    *fullscreen_was_active = fullscreen;
}

/// Loading placeholder before the first fetch.
fn loading_data(id: ProviderId) -> ProviderData {
    ProviderData {
        plan_type: None,
        id,
        status: ProviderStatus::Loading,
        metrics: vec![],
        updated_at: Utc::now(),
        received_at: Some(std::time::Instant::now()),
    }
}

// ─── Provider cache ──────────────────────────────────────────────
// Metrics with a still-future reset survive widget restarts: after a
// relaunch the row shows the last real value (greyed, with its age) even if
// the source is temporarily unavailable, instead of an error text.

fn provider_cache_path() -> std::path::PathBuf {
    storage::config_path().with_file_name("provider-cache.json")
}

fn cache_metric_is_valid(metric: &Metric) -> bool {
    // Any metric whose window has not reset yet is still true and worth
    // keeping: after a widget restart the row then shows the last real value
    // (greyed, with its age) instead of an error text while the first cycles
    // fight the flaky endpoints. Metrics past their reset are dropped — the
    // window rolled over, the number is dead.
    metric
        .reset_at
        .is_some_and(|reset| reset + Duration::minutes(1) > Utc::now())
        && metric.percentage().is_some()
}

fn prune_provider_cache_entry(data: &ProviderData) -> Option<ProviderData> {
    if !matches!(data.status, ProviderStatus::Ok) {
        return None;
    }

    let metrics: Vec<Metric> = data
        .metrics
        .iter()
        .filter(|metric| cache_metric_is_valid(metric))
        .cloned()
        .collect();

    (!metrics.is_empty()).then(|| ProviderData {
        metrics,
        ..data.clone()
    })
}

fn cacheable_provider_data(data: &ProviderData) -> Option<ProviderData> {
    prune_provider_cache_entry(data)
}

/// Whether a live metric already occupies the slot a cached metric would fill.
///
/// The label is authoritative when both sides have one. Beyond that, only
/// session-window metrics may collapse onto a shared slot by unit: providers
/// rename those rows between fetches (Antigravity follows Google's model
/// rotation), whereas long windows are distinct named pools — Claude reports
/// Weekly, Opus and Sonnet at once — so two differently-labelled long metrics
/// are always different slots.
fn metrics_match_slot(a: &Metric, b: &Metric) -> bool {
    if a.label.eq_ignore_ascii_case(&b.label) {
        return true;
    }
    if matches!(a.window, MetricWindow::Long) || matches!(b.window, MetricWindow::Long) {
        return false;
    }
    std::mem::discriminant(&a.unit) == std::mem::discriminant(&b.unit)
}

fn merge_cached_metrics(mut data: ProviderData, cached: Option<&ProviderData>) -> ProviderData {
    let Some(cached) = cached else {
        return data;
    };
    if data.id != cached.id || !matches!(cached.status, ProviderStatus::Ok) {
        return data;
    }

    for cached_metric in &cached.metrics {
        if !data
            .metrics
            .iter()
            .any(|metric| metrics_match_slot(metric, cached_metric))
        {
            data.metrics.push(cached_metric.clone());
        }
    }
    data
}

fn prune_provider_cache_map(cache: &mut HashMap<ProviderId, ProviderData>) -> bool {
    let before: HashMap<ProviderId, usize> = cache
        .iter()
        .map(|(id, data)| (id.clone(), data.metrics.len()))
        .collect();
    let pruned: HashMap<ProviderId, ProviderData> = cache
        .values()
        .filter_map(prune_provider_cache_entry)
        .map(|data| (data.id.clone(), data))
        .collect();
    let changed = before.len() != pruned.len()
        || pruned.iter().any(|(id, data)| {
            before
                .get(id)
                .is_none_or(|metric_count| *metric_count != data.metrics.len())
        });
    *cache = pruned;
    changed
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CachedProvider {
    #[serde(flatten)]
    data: ProviderData,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    codex_window_schema: Option<u8>,
}

fn decode_provider_cache(content: &str) -> serde_json::Result<HashMap<ProviderId, ProviderData>> {
    Ok(serde_json::from_str::<Vec<CachedProvider>>(content)?
        .into_iter()
        // Older Codex caches classified primary_window as Session regardless
        // of duration. Only a fresh response can correct that classification.
        .filter(|item| item.data.id != ProviderId::Codex || item.codex_window_schema == Some(1))
        .filter_map(|item| prune_provider_cache_entry(&item.data))
        .map(|data| (data.id.clone(), data))
        .collect())
}

async fn load_provider_cache() -> HashMap<ProviderId, ProviderData> {
    let path = provider_cache_path();
    let content = match tokio::fs::read_to_string(&path).await {
        Ok(content) => content,
        Err(_) => return HashMap::new(),
    };

    match decode_provider_cache(&content) {
        Ok(items) => items,
        Err(e) => {
            warn!("provider cache parse failed: {e}");
            HashMap::new()
        }
    }
}

async fn save_provider_cache(cache: HashMap<ProviderId, ProviderData>) -> Result<()> {
    let path = provider_cache_path();
    let mut items: Vec<ProviderData> = cache
        .into_values()
        .filter_map(|data| prune_provider_cache_entry(&data))
        .collect();

    if items.is_empty() {
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    items.sort_by_key(|d| match d.id {
        ProviderId::Claude => 0,
        ProviderId::Codex => 1,
        ProviderId::Copilot => 2,
        ProviderId::Antigravity => 3,
    });
    let items: Vec<_> = items
        .into_iter()
        .map(|data| CachedProvider {
            codex_window_schema: (data.id == ProviderId::Codex).then_some(1),
            data,
        })
        .collect();
    let content = serde_json::to_string_pretty(&items)?;
    storage::atomic_write(&path, content.as_bytes()).await?;
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────

/// Build provider instances + alert thresholds from the config.
fn build_providers(config: &Config) -> (Vec<Arc<dyn Provider>>, HashMap<ProviderId, u8>) {
    let mut providers: Vec<Arc<dyn Provider>> = Vec::new();
    let mut thresholds: HashMap<ProviderId, u8> = HashMap::new();
    for pc in &config.providers {
        let (provider, id): (Arc<dyn Provider>, ProviderId) = match pc.id.as_str() {
            "claude" => (
                Arc::new(ClaudeProvider::new(pc.clone())),
                ProviderId::Claude,
            ),
            "codex" => (Arc::new(CodexProvider::new(pc.clone())), ProviderId::Codex),
            "copilot" => (
                Arc::new(CopilotProvider::new(pc.clone())),
                ProviderId::Copilot,
            ),
            // "gemini" is the pre-rename config id; storage::load migrates
            // it, this arm just covers a config edited back by hand.
            "antigravity" | "gemini" => (
                Arc::new(AntigravityProvider::new(pc.clone())),
                ProviderId::Antigravity,
            ),
            other => {
                warn!("unknown provider '{other}' in config, skipping");
                continue;
            }
        };
        thresholds.insert(id, pc.alert_threshold);
        providers.push(provider);
    }
    (providers, thresholds)
}

/// Read a key/token from the clipboard, with validation.
fn read_clipboard_key() -> Result<String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| anyhow::anyhow!(crate::tr!("clipboard unavailable: {error}", error = e)))?;
    let text = clipboard
        .get_text()
        .map_err(|_| anyhow::anyhow!(crate::i18n::t("clipboard is empty or holds no text")))?;
    let key = text.trim().to_string();
    // A rough sanity check that this looks like a key.
    if key.len() < 10 || key.len() > 500 || key.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        anyhow::bail!(crate::i18n::t("clipboard content does not look like a key"));
    }
    Ok(key)
}

/// Quiet toast feedback for menu actions.
fn feedback(title: &str, body: &str) {
    if let Err(e) = ToastNotifier::show(title, body) {
        warn!("feedback toast failed: {e}");
    }
}

/// Providers visible in the widget: enabled and connected. Disabled
/// (enabled=false) and not-connected (NotConfigured) ones are not rendered.
fn visible_data(config: &Config, display: &[ProviderData]) -> Vec<ProviderData> {
    display
        .iter()
        .filter(|d| {
            let enabled = config
                .providers
                .iter()
                .find(|p| ProviderId::from_config_id(&p.id) == Some(d.id.clone()))
                .is_some_and(|p| p.enabled);
            enabled && !matches!(d.status, ProviderStatus::NotConfigured)
        })
        // Stale data is extrapolated: windows whose reset has passed → ≈0%.
        .map(|d| d.aged_for_display())
        .collect()
}

/// Rebuild the providers after a config change — the list is shared
/// with the scheduler.
fn refresh_providers(
    config: &Config,
    providers: &SharedProviders,
    thresholds: &mut HashMap<ProviderId, u8>,
    display: &mut [ProviderData],
    changed_id: &str,
) {
    let (built, new_thresholds) = build_providers(config);
    *thresholds = new_thresholds;
    match providers.write() {
        Ok(mut guard) => *guard = built,
        Err(poisoned) => *poisoned.into_inner() = built,
    }
    // The changed provider's row → Loading until a fresh fetch lands.
    if let Some(pid) = ProviderId::from_config_id(changed_id) {
        if let Some(slot) = display.iter_mut().find(|d| d.id == pid) {
            *slot = loading_data(pid);
        }
    }
}

/// Out-of-band fetch of a single provider.
fn fetch_one(
    providers: &SharedProviders,
    cmd_tx: &mpsc::Sender<AppCommand>,
    runtime: &tokio::runtime::Runtime,
    changed_id: &str,
) {
    let target = ProviderId::from_config_id(changed_id);
    let list: Vec<Arc<dyn Provider>> = match providers.read() {
        Ok(g) => g.clone(),
        Err(p) => p.into_inner().clone(),
    };
    for provider in list {
        if Some(provider.id()) != target {
            continue;
        }
        let tx = cmd_tx.clone();
        runtime.spawn(async move {
            if let Ok(data) = provider.fetch().await {
                let _ = tx.send(AppCommand::UpdateProvider(data)).await;
            }
        });
    }
}

pub fn run() -> Result<()> {
    // 1. The tokio runtime + the config. 2 workers instead of one-per-core:
    // we make 3 light HTTP requests per cycle; the default 32 workers would
    // waste ~30 threads and RAM.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("failed to create tokio runtime")?;
    let mut config = runtime.block_on(storage::load_or_default())?;
    crate::i18n::set_language(config.general.language);
    crate::network::set_mode(config.network.proxy_mode);
    info!("Config loaded: {} providers", config.providers.len());

    let mut event_loop_builder = EventLoopBuilder::<UserEvent>::with_user_event();
    #[cfg(windows)]
    {
        use tao::platform::windows::EventLoopBuilderExtWindows;
        event_loop_builder.with_msg_hook(|message| unsafe {
            crate::ui::appearance_dialog::handle_message(message)
        });
    }
    let event_loop = event_loop_builder.build();
    let proxy = event_loop.create_proxy();

    // The scheduler → event loop channel: tokio mpsc → EventLoopProxy.
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<AppCommand>(32);
    {
        let proxy = proxy.clone();
        runtime.spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                if proxy.send_event(UserEvent::Command(cmd)).is_err() {
                    break; // The event loop is gone.
                }
            }
        });
    }

    // 3. Providers — ALL from the config, including disabled ones: a disabled
    // provider's fetch returns NotConfigured instantly, with no network call.
    // The list is shared with the scheduler and rebuilt live from the menu.
    let (built, mut thresholds) = build_providers(&config);
    let mut provider_cache = runtime.block_on(load_provider_cache());
    let mut display: Vec<ProviderData> = built
        .iter()
        .map(|p| {
            let id = p.id();
            provider_cache
                .get(&id)
                .cloned()
                .unwrap_or_else(|| loading_data(id))
        })
        .collect();
    let providers: SharedProviders = Arc::new(RwLock::new(built));
    if display.is_empty() {
        anyhow::bail!("no providers in config");
    }

    // 4. The background scheduler; the interval is shared — changed live
    // from the menu.
    let update_interval: SharedInterval =
        Arc::new(AtomicU64::new(config.general.update_interval_secs));
    let scheduler = Scheduler::new(providers.clone(), update_interval.clone(), cmd_tx.clone());
    runtime.spawn(scheduler.run());

    // 4b. Background auto-updater — polls GitHub Releases and silently installs
    // newer builds. The enabled flag is shared so the menu toggle takes effect
    // without a restart.
    let auto_update_enabled = Arc::new(AtomicBool::new(config.general.auto_update));
    runtime.spawn(updater::run(auto_update_enabled.clone()));

    let theme = ComputedTheme::compute(&config.ui);
    // Hidden event/menu owner only; there is no desktop widget or render surface.
    let mut builder = WindowBuilder::new()
        .with_title("QuotaBar")
        .with_visible(false);
    #[cfg(windows)]
    {
        use tao::platform::windows::WindowBuilderExtWindows;
        builder = builder.with_skip_taskbar(true);
    }
    let _window = builder
        .build(&event_loop)
        .context("control window creation failed")?;
    // Per-provider threshold-event cooldown (shared by toast and on_threshold).
    let mut last_toast: HashMap<ProviderId, std::time::Instant> = HashMap::new();
    // Previous primary % per provider, for reset-edge detection.
    let mut last_pct: HashMap<ProviderId, f32> = HashMap::new();
    // Whether the on_startup hook has fired (once after the first success).
    let mut startup_fired = false;
    // 9. The context menu: events → proxy.
    let mut menu = ContextMenu::new(&config)?;
    {
        let proxy = proxy.clone();
        muda::MenuEvent::set_event_handler(Some(move |e: muda::MenuEvent| {
            let _ = proxy.send_event(UserEvent::Menu(e.id().clone()));
        }));
    }

    // 9b. The tray icon: a left-click toggles the overlay; a right-click shows
    // the SAME context menu. Tray menu clicks reuse the MenuEvent handler above.
    let mut tray = Tray::new();
    tray.set_warning_threshold(
        config
            .providers
            .iter()
            .find(|p| p.id == "codex")
            .map(|p| p.alert_threshold)
            .unwrap_or(80),
    );
    tray.set_appearance(config.appearance.clone());
    {
        let proxy = proxy.clone();
        tray_icon::TrayIconEvent::set_event_handler(Some(move |e: tray_icon::TrayIconEvent| {
            if let tray_icon::TrayIconEvent::Click {
                button,
                button_state,
                ..
            } = e
            {
                if button == tray_icon::MouseButton::Left
                    && button_state == tray_icon::MouseButtonState::Up
                {
                    let _ = proxy.send_event(UserEvent::Tray(TrayKind::LeftClick));
                }
            }
        }));
    }
    if !tray.set_mode(
        config.general.indicator,
        &menu.menu,
        &visible_data(&config, &display),
        &theme,
    ) {
        config.general.indicator = crate::config::schema::IndicatorKind::PanelRows;
    }
    // 9c. The taskbar mini panel (the readable indicator) + the event-driven
    // watcher that keeps it glued to the (auto-hiding) taskbar.
    let mut panel = TaskbarPanel::new(&event_loop)?;
    panel.set_offset(config.general.panel_offset_x, config.general.panel_offset_y);
    panel.set_position(config.general.panel_position_x);
    panel.set_locked(config.general.panel_locked);
    panel.set_warning_threshold(
        config
            .providers
            .iter()
            .find(|p| p.id == "codex")
            .map(|p| p.alert_threshold)
            .unwrap_or(80),
    );
    panel.set_appearance(config.appearance.clone());
    panel.set_display(config.general.panel_display);
    #[cfg(target_os = "windows")]
    crate::platform::install_taskbar_watch(proxy.clone(), config.general.panel_display);
    panel.set_mode(config.general.indicator, &visible_data(&config, &display));
    #[cfg(windows)]
    let mut appearance_dialog = crate::ui::appearance_dialog::AppearanceDialog::new(proxy.clone());
    // Whether the last evaluation saw a fullscreen app: the panel is hidden
    // while true; the falling edge re-presents it (alt-tab back to desktop).
    #[cfg(target_os = "windows")]
    let mut fullscreen_was_active = false;
    #[cfg(target_os = "windows")]
    let mut recheck_at: Option<std::time::Instant> = Some(std::time::Instant::now());
    // The hover tooltip appears only after the cursor lingers for the system
    // mouse-hover time (the same delay tray-icon tooltips use), tracked by this
    // deadline; it is cancelled the moment the cursor leaves.
    #[cfg(target_os = "windows")]
    let mut tooltip_at: Option<std::time::Instant> = None;
    #[cfg(target_os = "windows")]
    let hover_delay = std::time::Duration::from_millis(crate::platform::mouse_hover_time_ms());

    let rt_handle = runtime.handle().clone();
    let cache_rt_handle = rt_handle.clone();
    let save_cached_providers = move |cache: HashMap<ProviderId, ProviderData>| {
        cache_rt_handle.spawn(async move {
            if let Err(e) = save_provider_cache(cache).await {
                warn!("provider cache save failed: {e}");
            }
        });
    };

    // Save the config in the background through a single serialised writer
    // (storage::spawn_saver): a burst coalesces to the latest config on a
    // one-slot channel, so the newest setting always wins and a slow disk
    // cannot grow the queue. The saver itself is kept for the shutdown
    // handshake so the final write cannot race an in-flight background one.
    let saver = storage::spawn_saver(&rt_handle, storage::config_path());
    let save_config = {
        let handle = saver.handle();
        move |cfg: Config| handle.save(cfg)
    };
    let mut saver_at_exit = Some(saver);

    info!("QuotaBar ready");

    // 11. The event loop — never returns.
    event_loop.run(move |event, _target, control_flow| {
        #[cfg(target_os = "windows")]
        {
            let next = [recheck_at, tooltip_at].into_iter().flatten().min();
            *control_flow = match next {
                Some(t) => ControlFlow::WaitUntil(t),
                None => ControlFlow::Wait,
            };
        }
        #[cfg(not(target_os = "windows"))]
        {
            *control_flow = ControlFlow::Wait;
        }

        // A locked auto-position may resolve only after Explorer becomes ready.
        if config.general.panel_locked
            && config.general.panel_position_x.is_none()
            && panel.position().is_some()
        {
            config.general.panel_position_x = panel.position();
            save_config(config.clone());
        }
        let _control_owner = _window.id();
        match event {
            // Scheduled deadlines came due: re-check overlay coverage (toggle the
            // tray fallback), and/or show the hover tooltip after its delay.
            // Idle returns to 0% afterwards (no pending timer).
            #[cfg(target_os = "windows")]
            Event::NewEvents(_) => {
                let now = std::time::Instant::now();
                if let Some(t) = recheck_at {
                    if now >= t {
                        recheck_at = None;
                        // **Re-place before re-judging.** The taskbar SLIDES in
                        // and out over ~200ms, and the move events arrive
                        // during that slide: acting on the first one pins the
                        // panel to a mid-animation position — measured at bar
                        // top 1410 and 1413 while the settled bar is at 1392 —
                        // which leaves it hanging below the bar, partly off the
                        // screen edge, looking like it never appeared. Nothing
                        // else re-runs placement afterwards: the fallback
                        // evaluation only redraws into the rectangle it is
                        // given, so the stale position survived until the next
                        // slide or the 60-second provider tick.
                        panel.on_taskbar_moved(&visible_data(&config, &display));
                        eval_panel_visibility(
                            &mut panel,
                            &visible_data(&config, &display),
                            &mut fullscreen_was_active,
                            config.general.panel_display,
                        );
                        if matches!(
                            config.general.indicator,
                            crate::config::schema::IndicatorKind::PanelRows
                                | crate::config::schema::IndicatorKind::PanelGrid
                        ) {
                            // A stalled UIA provider cannot leave old geometry
                            // trusted indefinitely. Keep the check off COM.
                            recheck_at = Some(now + std::time::Duration::from_secs(1));
                        }
                    }
                }
                if let Some(t) = tooltip_at {
                    if now >= t {
                        tooltip_at = None;
                        panel.show_tooltip(&visible_data(&config, &display));
                    }
                }
            }
            // The embedded panel's own events: a left-click toggles the
            // overlay (like the tray icon), a right-click opens the menu.
            Event::WindowEvent {
                window_id, event, ..
            } if window_id == panel.window_id() => {
                match event {
                    WindowEvent::MouseInput {
                        state: ElementState::Pressed,
                        button: MouseButton::Left,
                        ..
                    } => {
                        panel.set_interaction(true, true, &visible_data(&config, &display));
                        panel.begin_drag();
                        #[cfg(windows)]
                        {
                            tooltip_at = None;
                        }
                    }
                    WindowEvent::MouseInput {
                        state: ElementState::Released,
                        button: MouseButton::Left,
                        ..
                    } => {
                        panel.set_interaction(true, false, &visible_data(&config, &display));
                        if let Some(x) = panel.finish_drag() {
                            config.general.panel_position_x = Some(x);
                            save_config(config.clone());
                        }
                    }
                    WindowEvent::KeyboardInput { event, .. }
                        if event.logical_key == tao::keyboard::Key::Escape =>
                    {
                        panel.cancel_drag();
                        panel.update(&visible_data(&config, &display), true);
                    }
                    WindowEvent::CursorMoved { .. } if panel.is_dragging() => {
                        panel.move_drag(&visible_data(&config, &display));
                    }
                    WindowEvent::MouseInput {
                        state: ElementState::Pressed,
                        button: MouseButton::Right,
                        ..
                    } => {
                        #[cfg(target_os = "windows")]
                        {
                            use muda::ContextMenu as _;
                            unsafe {
                                menu.menu.show_context_menu_for_hwnd(panel.hwnd(), None);
                            }
                        }
                    }
                    // Our own hover tooltip: arm the system hover delay on
                    // enter/hover (show only if the cursor lingers, like a
                    // tray-icon tooltip), hide and cancel on leave.
                    WindowEvent::CursorEntered { .. } | WindowEvent::CursorMoved { .. } => {
                        panel.set_interaction(true, false, &visible_data(&config, &display));
                        #[cfg(target_os = "windows")]
                        if !panel.is_dragging() && !panel.tooltip_shown() && tooltip_at.is_none() {
                            tooltip_at = Some(std::time::Instant::now() + hover_delay);
                        }
                    }
                    WindowEvent::ThemeChanged(_) => {
                        panel.update(&visible_data(&config, &display), true);
                        tray.update(&visible_data(&config, &display), &theme, true);
                    }
                    WindowEvent::CursorLeft { .. } => {
                        panel.set_interaction(false, false, &visible_data(&config, &display));
                        panel.hide_tooltip();
                        #[cfg(target_os = "windows")]
                        {
                            tooltip_at = None;
                        }
                    }
                    _ => {}
                }
            }

            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                WindowEvent::ThemeChanged(_) => {
                    panel.update(&visible_data(&config, &display), true);
                    tray.update(&visible_data(&config, &display), &theme, true);
                }
                _ => {}
            },
            Event::RedrawRequested(id) if id == panel.window_id() => {
                panel.redraw(&visible_data(&config, &display));
            }
            Event::UserEvent(ue) => match ue {
                UserEvent::AppearancePreview(style) => {
                    panel.set_appearance(style.clone());
                    tray.set_appearance(style);
                    panel.update(&visible_data(&config, &display), true);
                    tray.update(&visible_data(&config, &display), &theme, true);
                }
                UserEvent::AppearanceSave(mut style) => {
                    style.normalize();
                    config.appearance = style.clone();
                    panel.set_appearance(style.clone());
                    tray.set_appearance(style);
                    panel.update(&visible_data(&config, &display), true);
                    tray.update(&visible_data(&config, &display), &theme, true);
                    save_config(config.clone());
                }
                UserEvent::AppearanceCancel => {
                    panel.set_appearance(config.appearance.clone());
                    tray.set_appearance(config.appearance.clone());
                    panel.update(&visible_data(&config, &display), true);
                    tray.update(&visible_data(&config, &display), &theme, true);
                }
                UserEvent::Command(cmd) => match cmd {
                    AppCommand::UpdateProvider(data) => {
                        // Threshold crossing drives BOTH the toast and the
                        // `on_threshold` hook, sharing one cooldown. The hook
                        // fires even if toasts are disabled (it's its own opt-in).
                        if let Some(pct) = data.primary_percentage() {
                            let threshold = thresholds.get(&data.id).copied().unwrap_or(80) as f32;
                            let cooldown = std::time::Duration::from_secs(
                                config.notifications.cooldown_minutes.max(1) as u64 * 60,
                            );
                            let cooled = last_toast
                                .get(&data.id)
                                .is_none_or(|t| t.elapsed() >= cooldown);
                            if pct >= threshold && cooled {
                                last_toast.insert(data.id.clone(), std::time::Instant::now());
                                if config.notifications.enabled {
                                    let body = match data.headline_reset() {
                                        Some(t) => {
                                            let secs = t
                                                .signed_duration_since(Utc::now())
                                                .num_seconds()
                                                .max(0);
                                            crate::tr!(
                                                "{percent}% used, resets in {duration}",
                                                percent = pct.round() as u32,
                                                duration = format_duration(secs)
                                            )
                                        }
                                        None => crate::tr!(
                                            "{percent}% used",
                                            percent = pct.round() as u32
                                        ),
                                    };
                                    let title = crate::tr!(
                                        "{provider} limit",
                                        provider = data.id.display_name()
                                    );
                                    if let Err(e) = ToastNotifier::show(&title, &body) {
                                        warn!("toast failed: {e}");
                                    }
                                }
                                hooks::run(
                                    &runtime,
                                    &config.hooks.on_threshold,
                                    hooks::HookEvent::Threshold,
                                    data.id.clone(),
                                    Some(pct),
                                    data.headline_reset(),
                                );
                            }

                            // Reset edge: a provider that was near its limit
                            // (>= threshold) dropping sharply means a window
                            // rolled over. Self-debouncing: after firing, the
                            // tracked % is low, so it won't refire until usage
                            // climbs back above the threshold and drops again.
                            if let Some(&prev) = last_pct.get(&data.id) {
                                if prev >= threshold && pct + 30.0 < prev {
                                    hooks::run(
                                        &runtime,
                                        &config.hooks.on_reset,
                                        hooks::HookEvent::Reset,
                                        data.id.clone(),
                                        Some(pct),
                                        data.headline_reset(),
                                    );
                                }
                            }
                            last_pct.insert(data.id.clone(), pct);
                        }

                        // Startup edge: once, after the first successful fetch.
                        if !startup_fired && matches!(data.status, ProviderStatus::Ok) {
                            startup_fired = true;
                            hooks::run(
                                &runtime,
                                &config.hooks.on_startup,
                                hooks::HookEvent::Startup,
                                data.id.clone(),
                                data.primary_percentage(),
                                data.headline_reset(),
                            );
                        }

                        // A transient error must NOT wipe the last real data —
                        // it stays on screen; the renderer greys it out with
                        // its age. Applies to every provider.
                        let is_error = matches!(
                            data.status,
                            ProviderStatus::NetworkError(_) | ProviderStatus::AuthError(_)
                        );
                        if prune_provider_cache_map(&mut provider_cache) {
                            save_cached_providers(provider_cache.clone());
                        }

                        let is_success = matches!(data.status, ProviderStatus::Ok);
                        let cached_before_update = provider_cache.get(&data.id).cloned();
                        let display_data = if is_success {
                            merge_cached_metrics(data.clone(), cached_before_update.as_ref())
                        } else {
                            data.clone()
                        };
                        if is_success {
                            provider_cache.remove(&data.id);
                        }
                        if let Some(cached_data) = cacheable_provider_data(&display_data) {
                            provider_cache.insert(cached_data.id.clone(), cached_data);
                        }
                        if is_success {
                            save_cached_providers(provider_cache.clone());
                        }
                        let had_real_data = display.iter().any(|d| {
                            d.id == data.id
                                && matches!(d.status, ProviderStatus::Ok)
                                && !d.metrics.is_empty()
                        }) || provider_cache.contains_key(&data.id);
                        if is_error && had_real_data {
                            if let Some(cached) = provider_cache.get(&data.id).cloned() {
                                if let Some(slot) = display.iter_mut().find(|d| d.id == data.id) {
                                    if !matches!(slot.status, ProviderStatus::Ok)
                                        || slot.metrics.is_empty()
                                    {
                                        *slot = cached;
                                    } else {
                                        *slot = merge_cached_metrics(slot.clone(), Some(&cached));
                                    }
                                }
                            }
                            warn!(
                                "provider {:?}: {:?} — keeping last data on screen",
                                data.id, data.status
                            );
                        } else if let Some(slot) = display.iter_mut().find(|d| d.id == data.id) {
                            *slot = display_data;
                        }
                        tray.update(&visible_data(&config, &display), &theme, false);
                        panel.update(&visible_data(&config, &display), false);
                    }
                    AppCommand::Quit => *control_flow = ControlFlow::Exit,
                },

                UserEvent::TaskbarMoved => {
                    // While fullscreen-suppressed this is a no-op (the panel
                    // gates every presentation path itself).
                    panel.on_taskbar_moved(&visible_data(&config, &display));
                    // The bar settled into a new position; re-check coverage a
                    // beat later (the "rude topmost" front lands after the slide).
                    #[cfg(target_os = "windows")]
                    {
                        recheck_at =
                            Some(std::time::Instant::now() + std::time::Duration::from_millis(150));
                    }
                }

                UserEvent::PanelRaise => {
                    // The foreground changed (tray overflow flyout, Start menu, a
                    // fullscreen game, an app). Evaluate the fallback FIRST: if a
                    // fullscreen app just took over, this hides the panel, and the
                    // raise below becomes a no-op (rect=None) — never re-asserting
                    // the overlay above a game. Arm a delayed re-check too — the
                    // auto-hide bar can peek back (or the shell can flip its
                    // fullscreen state) a moment later with no further event.
                    #[cfg(target_os = "windows")]
                    {
                        eval_panel_visibility(
                            &mut panel,
                            &visible_data(&config, &display),
                            &mut fullscreen_was_active,
                            config.general.panel_display,
                        );
                        recheck_at =
                            Some(std::time::Instant::now() + std::time::Duration::from_millis(150));
                    }
                    panel.raise();
                }

                // The shell re-stacked the taskbar (it may have fronted the bar
                // above our overlay with no move/foreground event). Arm the
                // coalesced re-check; a burst of reorders collapses to one.
                #[cfg(target_os = "windows")]
                UserEvent::PanelRecheck => {
                    recheck_at =
                        Some(std::time::Instant::now() + std::time::Duration::from_millis(150));
                }

                UserEvent::Tray(TrayKind::LeftClick) => {}

                UserEvent::Menu(id) => match menu.action_for(&id) {
                    Some(MenuAction::SetLanguage(language)) => {
                        let previous = config.general.language;
                        crate::i18n::set_language(language);
                        config.general.language = language;
                        match ContextMenu::new(&config) {
                            Ok(new_menu) => {
                                menu = new_menu;
                                menu.sync(&config);
                                tray.replace_menu(&menu.menu);
                                panel.hide_tooltip();
                                tray.update(&visible_data(&config, &display), &theme, true);
                                panel.update(&visible_data(&config, &display), true);
                                save_config(config.clone());
                            }
                            Err(e) => {
                                config.general.language = previous;
                                crate::i18n::set_language(previous);
                                menu.sync(&config);
                                warn!("could not rebuild language menu: {e}");
                            }
                        }
                    }
                    Some(MenuAction::SetProxyMode(mode)) => {
                        config.network.proxy_mode = mode;
                        crate::network::set_mode(mode);
                        menu.sync(&config);
                        save_config(config.clone());
                    }

                    Some(MenuAction::OpenAppearance) => {
                        #[cfg(windows)]
                        if let Err(e) = appearance_dialog.open(&config.appearance) {
                            warn!("appearance dialog could not open: {e}");
                        }
                    }
                    Some(MenuAction::Quit) => *control_flow = ControlFlow::Exit,

                    Some(MenuAction::ResetPanelPosition) => {
                        config.general.panel_position_x = None;
                        config.general.panel_locked = false;
                        panel.set_locked(false);
                        menu.sync(&config);
                        panel.set_position(None);
                        panel.update(&visible_data(&config, &display), true);
                        save_config(config.clone());
                    }
                    Some(MenuAction::SetIndicator(kind)) => {
                        let kind = if kind == crate::config::schema::IndicatorKind::Off {
                            crate::config::schema::IndicatorKind::PanelRows
                        } else {
                            kind
                        };
                        // Do not hide the current panel until the tray is ready.
                        if tray.set_mode(kind, &menu.menu, &visible_data(&config, &display), &theme)
                        {
                            config.general.indicator = kind;
                            panel.set_mode(kind, &visible_data(&config, &display));
                            #[cfg(target_os = "windows")]
                            {
                                recheck_at = Some(std::time::Instant::now());
                            }
                            menu.sync(&config);
                            save_config(config.clone());
                        } else {
                            warn!("could not switch indicator; keeping current panel");
                        }
                    }
                    Some(MenuAction::SetPanelDisplay(target)) => {
                        config.general.panel_display = target;
                        panel.set_display(target);
                        #[cfg(target_os = "windows")]
                        crate::platform::watch_taskbar(target);
                        // A full restart, not a reposition: `on_taskbar_moved`
                        // returns early while the panel is suppressed, so a
                        // panel parked by a fullscreen app could not be brought
                        // back by switching displays at all.
                        panel.restart(&visible_data(&config, &display));
                        // The new display may be owned by a fullscreen app; the
                        // reposition above cannot see that, so arm the same
                        // deferred re-check TaskbarMoved uses to catch it a beat
                        // later instead of painting straight over the game.
                        #[cfg(target_os = "windows")]
                        {
                            recheck_at = Some(
                                std::time::Instant::now() + std::time::Duration::from_millis(150),
                            );
                        }
                        menu.sync(&config);
                        save_config(config.clone());
                    }

                    Some(MenuAction::ToggleProvider(pid)) => {
                        let mut now_enabled = None;
                        if let Some(pc) = config.providers.iter_mut().find(|p| p.id == pid) {
                            pc.enabled = !pc.enabled;
                            now_enabled = Some(pc.enabled);
                        }
                        if now_enabled == Some(false) {
                            if let Some(provider_id) = ProviderId::from_config_id(&pid) {
                                provider_cache.remove(&provider_id);
                                save_cached_providers(provider_cache.clone());
                            }
                        }
                        refresh_providers(&config, &providers, &mut thresholds, &mut display, &pid);
                        tray.sync_providers(&menu.menu, &visible_data(&config, &display), &theme);
                        panel.update(&visible_data(&config, &display), true);
                        save_config(config.clone());
                        fetch_one(&providers, &cmd_tx, &runtime, &pid);
                    }
                    Some(MenuAction::SetAuthMethod(pid, method)) => {
                        if let Some(pc) = config.providers.iter_mut().find(|p| p.id == pid) {
                            pc.auth_method = method.clone();
                            match method {
                                // api_key needs a key label.
                                AuthMethod::ApiKey => {
                                    pc.credential_label =
                                        key_label_for(&pid).unwrap_or_default().to_string();
                                }
                                // The subscription reads local files, no key needed.
                                AuthMethod::Subscription => pc.credential_label.clear(),
                            }
                        }
                        refresh_providers(&config, &providers, &mut thresholds, &mut display, &pid);
                        menu.sync(&config);
                        save_config(config.clone());
                        fetch_one(&providers, &cmd_tx, &runtime, &pid);
                    }
                    Some(MenuAction::PasteKey(pid)) => match read_clipboard_key() {
                        Ok(key) => {
                            let label = key_label_for(&pid).unwrap_or_default();
                            let stored = keyring::Entry::new("ailimits", label)
                                .and_then(|e| e.set_password(&key));
                            match stored {
                                Ok(()) => {
                                    if let Some(pc) =
                                        config.providers.iter_mut().find(|p| p.id == pid)
                                    {
                                        pc.enabled = true;
                                        pc.credential_label = label.to_string();
                                        // Claude: a key implies the api_key method.
                                        // Copilot: the PAT is just a token source
                                        // for the subscription.
                                        if pid == "claude" {
                                            pc.auth_method = AuthMethod::ApiKey;
                                        }
                                    }
                                    refresh_providers(
                                        &config,
                                        &providers,
                                        &mut thresholds,
                                        &mut display,
                                        &pid,
                                    );
                                    menu.sync(&config);
                                    save_config(config.clone());
                                    fetch_one(&providers, &cmd_tx, &runtime, &pid);
                                    feedback(
                                        "QuotaBar",
                                        &crate::tr!("Key for {provider} stored", provider = pid),
                                    );
                                }
                                Err(e) => {
                                    warn!("keyring store failed: {e}");
                                    feedback("QuotaBar", crate::i18n::t("Failed to store the key"));
                                }
                            }
                        }
                        Err(e) => feedback("QuotaBar", &format!("{e}")),
                    },
                    Some(MenuAction::RemoveKey(pid)) => {
                        let label = key_label_for(&pid).unwrap_or_default();
                        match keyring::Entry::new("ailimits", label)
                            .and_then(|e| e.delete_credential())
                        {
                            Ok(()) | Err(keyring::Error::NoEntry) => {
                                if let Some(pc) = config.providers.iter_mut().find(|p| p.id == pid)
                                {
                                    // Without a key everything runs on the
                                    // subscription method (Claude Code files /
                                    // the gh CLI token).
                                    pc.credential_label.clear();
                                    pc.auth_method = AuthMethod::Subscription;
                                }
                                refresh_providers(
                                    &config,
                                    &providers,
                                    &mut thresholds,
                                    &mut display,
                                    &pid,
                                );
                                menu.sync(&config);
                                save_config(config.clone());
                                fetch_one(&providers, &cmd_tx, &runtime, &pid);
                                feedback(
                                    "QuotaBar",
                                    &crate::tr!("Key for {provider} removed", provider = pid),
                                );
                            }
                            Err(e) => {
                                warn!("keyring delete failed: {e}");
                                feedback("QuotaBar", crate::i18n::t("Failed to remove the key"));
                            }
                        }
                    }
                    Some(MenuAction::PasteUsageToken(pid)) => match read_clipboard_key() {
                        Ok(token) => {
                            let label = usage_token_label_for(&pid).unwrap_or_default();
                            match keyring::Entry::new("ailimits", label)
                                .and_then(|e| e.set_password(&token))
                            {
                                Ok(()) => {
                                    // No config change: the usage token is just an
                                    // extra source the provider tries first.
                                    fetch_one(&providers, &cmd_tx, &runtime, &pid);
                                    feedback(
                                        "QuotaBar",
                                        &crate::tr!(
                                            "Usage token for {provider} stored",
                                            provider = pid
                                        ),
                                    );
                                }
                                Err(e) => {
                                    warn!("keyring store failed: {e}");
                                    feedback(
                                        "QuotaBar",
                                        crate::i18n::t("Failed to store the usage token"),
                                    );
                                }
                            }
                        }
                        Err(e) => feedback("QuotaBar", &format!("{e}")),
                    },
                    Some(MenuAction::RemoveUsageToken(pid)) => {
                        let label = usage_token_label_for(&pid).unwrap_or_default();
                        match keyring::Entry::new("ailimits", label)
                            .and_then(|e| e.delete_credential())
                        {
                            Ok(()) | Err(keyring::Error::NoEntry) => {
                                fetch_one(&providers, &cmd_tx, &runtime, &pid);
                                feedback(
                                    "QuotaBar",
                                    &crate::tr!(
                                        "Usage token for {provider} removed",
                                        provider = pid
                                    ),
                                );
                            }
                            Err(e) => {
                                warn!("keyring delete failed: {e}");
                                feedback(
                                    "QuotaBar",
                                    crate::i18n::t("Failed to remove the usage token"),
                                );
                            }
                        }
                    }

                    Some(MenuAction::SetUpdateInterval(secs)) => {
                        config.general.update_interval_secs = secs;
                        // The scheduler picks the new value up on its next cycle.
                        update_interval.store(secs, Ordering::Relaxed);
                        menu.sync(&config);
                        save_config(config.clone());
                    }
                    Some(MenuAction::ToggleAutoUpdate) => {
                        config.general.auto_update = !config.general.auto_update;
                        // The updater task reads this before each check.
                        auto_update_enabled.store(config.general.auto_update, Ordering::Relaxed);
                        menu.sync(&config);
                        save_config(config.clone());
                    }
                    Some(MenuAction::ToggleLock) => {
                        config.general.panel_locked = !config.general.panel_locked;
                        panel.set_locked(config.general.panel_locked);
                        panel.update(&visible_data(&config, &display), true);
                        config.general.panel_position_x = panel.position();
                        menu.sync(&config);
                        save_config(config.clone());
                    }
                    None => {}
                },
            },

            // tao calls process::exit() right after this event, and that does
            // not wait for the background writer. Hand the writer the final
            // config and wait for it to finish, so a setting changed moments
            // before quitting survives — and no in-flight background write can
            // land after it. This one arm covers every exit path (window
            // close, tray Quit, menu Quit).
            Event::LoopDestroyed => {
                if let Some(saver) = saver_at_exit.take() {
                    saver.shutdown(config.clone(), &rt_handle);
                }
            }

            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::MetricUnit;

    #[test]
    fn fresh_subscription_metadata_replaces_cached_plan() {
        let mut cached = data(vec![]);
        cached.plan_type = Some("pro".into());
        for plan in [Some("plus"), Some("pro"), None] {
            let mut live = data(vec![]);
            live.plan_type = plan.map(str::to_owned);
            assert_eq!(
                merge_cached_metrics(live, Some(&cached))
                    .plan_type
                    .as_deref(),
                plan
            );
        }
    }

    #[test]
    fn old_codex_cache_is_discarded_but_other_providers_and_new_cache_survive() {
        let mut metric = pct("Session", 27, MetricWindow::Session);
        metric.reset_at = Some(Utc::now() + Duration::days(3));
        let claude = data(vec![metric.clone()]);
        let mut codex = data(vec![metric]);
        codex.id = ProviderId::Codex;
        let old = serde_json::to_string(&vec![claude.clone(), codex.clone()]).unwrap();
        let decoded = decode_provider_cache(&old).unwrap();
        assert!(decoded.contains_key(&ProviderId::Claude));
        assert!(!decoded.contains_key(&ProviderId::Codex));
        codex.metrics[0].window = MetricWindow::Long;
        codex.metrics[0].label = "Weekly".into();
        let new = serde_json::to_string(&vec![
            CachedProvider {
                data: claude,
                codex_window_schema: None,
            },
            CachedProvider {
                data: codex,
                codex_window_schema: Some(1),
            },
        ])
        .unwrap();
        let decoded = decode_provider_cache(&new).unwrap();
        assert_eq!(decoded.len(), 2);
        assert_eq!(
            decoded[&ProviderId::Codex].metrics[0].window,
            MetricWindow::Long
        );
    }

    fn pct(label: &str, used: u64, window: MetricWindow) -> Metric {
        Metric {
            label: label.to_string(),
            used,
            limit: Some(100),
            unit: MetricUnit::Percent,
            reset_at: None,
            window,
        }
    }

    fn data(metrics: Vec<Metric>) -> ProviderData {
        ProviderData {
            plan_type: None,
            id: ProviderId::Claude,
            status: ProviderStatus::Ok,
            metrics,
            updated_at: Utc::now(),
            received_at: Some(std::time::Instant::now()),
        }
    }

    #[test]
    fn a_session_metric_never_covers_a_long_windows_slot() {
        // The old label guess called neither of these "weekly" and then matched
        // them on their shared unit, so a cached Opus row was thrown away.
        let session = pct("Session", 20, MetricWindow::Session);
        let opus = pct("Opus", 100, MetricWindow::Long);
        assert!(!metrics_match_slot(&session, &opus));
    }

    #[test]
    fn two_different_long_pools_are_different_slots() {
        // Claude reports Weekly, Opus and Sonnet at the same time; one long
        // window no longer stands for all of them.
        let weekly = pct("Weekly", 50, MetricWindow::Long);
        let opus = pct("Opus", 100, MetricWindow::Long);
        assert!(!metrics_match_slot(&weekly, &opus));
    }

    #[test]
    fn the_same_label_is_always_the_same_slot() {
        let a = pct("Weekly", 50, MetricWindow::Long);
        let b = pct("Weekly", 90, MetricWindow::Long);
        assert!(metrics_match_slot(&a, &b));
        let c = pct("Session", 10, MetricWindow::Session);
        let d = pct("session", 80, MetricWindow::Session);
        assert!(metrics_match_slot(&c, &d));
    }

    #[test]
    fn session_metrics_with_different_labels_still_share_a_slot_by_unit() {
        // Antigravity renames its rows as Google rotates models, so two session
        // metrics of the same unit must keep collapsing onto one slot.
        let a = pct("2.5 Pro", 30, MetricWindow::Session);
        let b = pct("3.6 Flash", 40, MetricWindow::Session);
        assert!(metrics_match_slot(&a, &b));
    }

    #[test]
    fn merging_restores_long_pools_the_api_omitted_this_cycle() {
        // Claude answers "seven_day_opus": null on some cycles; the cached rows
        // must come back instead of being swallowed by the Session metric.
        let live = data(vec![
            pct("Session", 20, MetricWindow::Session),
            pct("Weekly", 60, MetricWindow::Long),
        ]);
        let cached = data(vec![
            pct("Session", 15, MetricWindow::Session),
            pct("Weekly", 55, MetricWindow::Long),
            pct("Opus", 100, MetricWindow::Long),
            pct("Sonnet", 40, MetricWindow::Long),
        ]);
        let merged = merge_cached_metrics(live, Some(&cached));
        let labels: Vec<&str> = merged.metrics.iter().map(|m| m.label.as_str()).collect();
        assert!(labels.contains(&"Opus"), "Opus was dropped: {labels:?}");
        assert!(labels.contains(&"Sonnet"), "Sonnet was dropped: {labels:?}");
        // The live values must win for the slots the API did return.
        let weekly = merged.metrics.iter().find(|m| m.label == "Weekly").unwrap();
        assert_eq!(weekly.used, 60);
        assert_eq!(merged.metrics.len(), 4, "no duplicate slots: {labels:?}");
    }
}
