//! Codex weekly remaining ring and percentage, painted over the taskbar.
//! Tracks shell placement, theme, auto-hide and fullscreen suppression.

use crate::config::appearance::Appearance;
use crate::config::schema::IndicatorKind;
use crate::providers::ProviderData;
use crate::ui::tray::color;
use crate::ui::weekly::{paint_styled_ring, WeeklyQuota};
use anyhow::{Context as AnyhowContext, Result};
use std::rc::Rc;
use tao::window::{Window, WindowId};
use tiny_skia::{Color, Pixmap};

/// Base layout metrics at 100% DPI; desired_size() scales them to the bar.
#[cfg(test)]
const PAD_X: f32 = 8.0;
#[cfg(test)]
const NUM_W: f32 = 100.0;
#[cfg(test)]
const NUM_GAP: f32 = 7.0;
#[cfg(test)]
const RING_SIZE: f32 = 28.0;
/// Gap between the overlay and the notification area.
const TRAY_MARGIN: i32 = 10;
/// Initial (hidden) window size; reposition() sizes it to the real taskbar.
const INIT_W: u32 = 151;
const INIT_H: u32 = 40;
/// Background alpha (out of 255) of the otherwise-transparent overlay: just
/// enough that every pixel catches the mouse (so the whole window is the
/// right-click / hover target, not only the painted glyphs), yet visually
/// imperceptible over the taskbar.
const HIT_ALPHA: u8 = 1;

/// Ink + track colors for the current system theme. Near-black on a light
/// taskbar, near-white on a dark one; the track is the ink at a low alpha so
/// it reads as a faint groove rather than a filled box.
///
/// The light ink is 36, not the 20 it used to be. Measured against the shell's
/// own clock on the same bar: our digits had a median ink luma of 110 where the
/// clock's was 119, over a background of 228 — we were ~8% darker, and read as
/// heavier and higher-contrast than everything else in the tray. Solving
/// `median = ink*cov + bg*(1-cov)` at the measured coverage of 0.567 gives 36.
///
/// Note this is a CONTRAST correction, not a weight one: our glyphs are not
/// thicker, they are inked harder. fontdue rasterises with crisper edges than
/// DirectWrite (over the same string the shell lights 163 columns to our 118,
/// at the same mean), so matching the shell's darkness is the closest we get
/// without swapping the rasteriser.
fn theme_ink(light: bool) -> (Color, Color) {
    if light {
        (color(36, 36, 36, 255), color(36, 36, 36, 64))
    } else {
        (color(236, 236, 236, 255), color(236, 236, 236, 66))
    }
}

/// Premultiplied RGBA (tiny-skia) → premultiplied BGRA (top-down) for
/// UpdateLayeredWindow.
#[cfg(target_os = "windows")]
fn pixmap_to_bgra(pm: &Pixmap) -> Vec<u8> {
    let data = pm.data();
    let mut bgra = vec![0u8; data.len()];
    for (s, d) in data.chunks_exact(4).zip(bgra.chunks_exact_mut(4)) {
        d[0] = s[2];
        d[1] = s[1];
        d[2] = s[0];
        d[3] = s[3];
    }
    bgra
}

pub struct TaskbarPanel {
    locked: bool,
    appearance: Appearance,
    content_width: f32,
    position_x: Option<i32>,
    last_position_x: Option<i32>,
    drag: Option<PanelDrag>,
    window: Rc<Window>,
    pixmap: Pixmap,
    mode: IndicatorKind,
    /// Include validity and staleness so unchanged percentages cannot mask errors.
    last: Option<WeeklyQuota>,
    size: (u32, u32),
    /// Current screen placement, or None while hidden (taskbar slid away).
    rect: Option<(i32, i32, u32, u32)>,
    /// True while a fullscreen app owns the screen: EVERY presentation path
    /// (update ticks, taskbar moves, raises) is gated on this, so nothing can
    /// resurrect the overlay over a game between fallback evaluations.
    suppressed: bool,
    /// The panel could not place itself: no taskbar resolved, or the bar is a
    /// shape we refuse to draw on. Distinct from "the bar is auto-hidden",
    /// which is normal and needs no substitute — the tray icon lives in that
    /// same bar and would be hidden with it.
    unavailable: bool,
    /// Our own hover-tooltip window (a raw layered top-level window we paint),
    /// and whether it is currently shown. Painted dark/rounded/borderless to
    /// match the shell's tooltips, which a native control cannot.
    #[cfg(target_os = "windows")]
    tip_hwnd: isize,
    #[cfg(target_os = "windows")]
    tip_shown: bool,
    /// Last UpdateLayeredWindow error code, so a failing present is logged
    /// once per distinct cause instead of on every provider tick. `None`
    /// means the last present succeeded (or none has run yet) — distinct
    /// from `Some(0)`, which is itself a legitimate cause (the early
    /// size/buffer guard in `present_layered` returns `Err(0)`).
    last_present_error: Option<u32>,
    /// Manual position nudge from the config, already clamped.
    offset: (i32, i32),
    /// Which taskbar the panel attaches to.
    display: crate::config::schema::PanelDisplay,
}

struct PanelDrag {
    pointer_x: i32,
    left: i32,
    original: Option<i32>,
    last_original: Option<i32>,
    moved: bool,
}

/// The tooltip window is ours, created with CreateWindowExW; nothing else owns
/// it. Harmless to leak while exactly one panel lives for the whole process,
/// but `restart()` already exists as the "rebuild the panel" path, and the day
/// that becomes "make a new TaskbarPanel" the old window would outlive it.
///
/// **This does not run on a normal exit.** `tao::EventLoop::run` is `-> !` and
/// terminates the process from inside, so the closure holding the panel is
/// never dropped. The window is reclaimed by the OS instead. This impl exists
/// for the case above — a panel dropped while the process keeps running — and
/// is deliberately a no-op today rather than a fix for a live leak.
impl Drop for TaskbarPanel {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        crate::platform::destroy_window(self.tip_hwnd);
    }
}

impl TaskbarPanel {
    /// Create the (hidden) overlay window up front; it is shown and embedded
    /// only when the indicator switches to a Panel mode.
    pub fn new(event_loop: &tao::event_loop::EventLoop<crate::app::UserEvent>) -> Result<Self> {
        let mut builder = tao::window::WindowBuilder::new()
            .with_title("AI Limits Panel")
            .with_decorations(false)
            .with_resizable(false)
            .with_visible(false)
            .with_inner_size(tao::dpi::PhysicalSize::new(INIT_W, INIT_H));
        #[cfg(target_os = "windows")]
        {
            use tao::platform::windows::WindowBuilderExtWindows;
            builder = builder.with_skip_taskbar(true);
        }
        let window = Rc::new(
            builder
                .build(event_loop)
                .context("panel window creation failed")?,
        );
        let pixmap = Pixmap::new(INIT_W, INIT_H).context("panel pixmap")?;
        Ok(Self {
            locked: false,
            appearance: Appearance::default(),
            content_width: appearance_width(&Appearance::default()),
            position_x: None,
            last_position_x: None,
            drag: None,
            window,
            pixmap,
            mode: IndicatorKind::Off,
            last: None,
            size: (INIT_W, INIT_H),
            rect: None,
            suppressed: false,
            unavailable: false,
            #[cfg(target_os = "windows")]
            // No DWM blur here, deliberately. It tints and blurs the whole
            // WINDOW rectangle, not the rounded box we paint inside it, so once
            // the pixmap grew a margin for the drop shadow the blur showed up
            // as a hard-edged grey rectangle around the tooltip. Measurement
            // says we do not want it anyway: the shell's tooltip is alpha ~244,
            // i.e. all but opaque, and what separates it from the background is
            // the shadow.
            tip_hwnd: crate::platform::create_tooltip_window(),
            #[cfg(target_os = "windows")]
            tip_shown: false,
            last_present_error: None,
            offset: (0, 0),
            display: crate::config::schema::PanelDisplay::Primary,
        })
    }

    /// Apply the configured manual offset. Clamped here, so no caller can
    /// push the panel off the desktop by editing config.toml.
    pub fn set_offset(&mut self, x: i32, y: i32) {
        use crate::platform::taskbar_geom::clamp_offset;
        self.offset = (clamp_offset(x), clamp_offset(y));
    }

    /// Point the panel at a taskbar. The caller re-positions afterwards.
    pub fn set_display(&mut self, target: crate::config::schema::PanelDisplay) {
        self.cancel_drag();
        self.last_position_x = None;
        self.display = target;
    }

    pub fn set_position(&mut self, x: Option<i32>) {
        self.cancel_drag();
        self.position_x = x;
        self.last_position_x = None;
        self.last = None;
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn set_appearance(&mut self, mut appearance: Appearance) {
        appearance.normalize();
        self.content_width = appearance_width(&appearance);
        self.appearance = appearance;
        self.last = None;
    }

    pub fn set_locked(&mut self, locked: bool) {
        self.cancel_drag();
        self.locked = locked;
        if locked && self.position_x.is_none() {
            self.position_x = self.last_position_x;
        }
    }

    pub fn position(&self) -> Option<i32> {
        self.position_x
    }

    pub fn begin_drag(&mut self) {
        self.cancel_drag();
        if self.locked {
            return;
        }
        #[cfg(windows)]
        if let Some((left, _, _, _)) = self.rect {
            use windows::Win32::{
                Foundation::{HWND, POINT},
                UI::{Input::KeyboardAndMouse::SetCapture, WindowsAndMessaging::GetCursorPos},
            };
            let mut point = POINT::default();
            unsafe {
                if GetCursorPos(&mut point).is_err() {
                    return;
                }
                SetCapture(HWND(self.hwnd() as _));
            }
            self.hide_tooltip();
            self.drag = Some(PanelDrag {
                pointer_x: point.x,
                left,
                original: self.position_x,
                last_original: self.last_position_x,
                moved: false,
            });
        }
    }

    pub fn move_drag(&mut self, providers: &[ProviderData]) {
        #[cfg(windows)]
        {
            use windows::Win32::{
                Foundation::POINT,
                UI::{Input::KeyboardAndMouse::GetCapture, WindowsAndMessaging::GetCursorPos},
            };
            if self.drag.is_none() {
                return;
            }
            if unsafe { GetCapture().0 as isize } != self.hwnd() {
                self.cancel_drag();
                return;
            }
            let Some(slot) = crate::platform::taskbar_slot(self.display) else {
                self.cancel_drag();
                return;
            };
            let scale = crate::platform::taskbar_geom::bar_scale(slot.height);
            let mut point = POINT::default();
            if unsafe { GetCursorPos(&mut point) }.is_err() {
                return;
            }
            let drag = self.drag.as_mut().unwrap();
            let dx = point.x - drag.pointer_x;
            if !drag.moved && (dx as f32).abs() < 4.0 * scale {
                return;
            }
            drag.moved = true;
            let x =
                (drag.left + dx).clamp(slot.left, (slot.right - self.size.0 as i32).max(slot.left));
            self.position_x = Some(((x - slot.left) as f32 / scale).round() as i32);
            self.update(providers, true);
        }
        #[cfg(not(windows))]
        let _ = providers;
    }

    /// Returns the new persistent X only for a completed drag, not a click.
    pub fn finish_drag(&mut self) -> Option<i32> {
        let drag = self.drag.take()?;
        self.release_capture();
        if drag.moved {
            self.position_x
        } else {
            None
        }
    }

    pub fn cancel_drag(&mut self) {
        if let Some(drag) = self.drag.take() {
            self.position_x = drag.original;
            self.last_position_x = drag.last_original;
            self.last = None;
            self.release_capture();
        }
    }

    fn release_capture(&self) {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, ReleaseCapture};
            if GetCapture().0 as isize == self.hwnd() {
                let _ = ReleaseCapture();
            }
        }
    }

    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    #[cfg(target_os = "windows")]
    pub fn hwnd(&self) -> isize {
        use tao::platform::windows::WindowExtWindows;
        self.window.hwnd()
    }

    fn is_panel_mode(mode: IndicatorKind) -> bool {
        matches!(mode, IndicatorKind::PanelRows | IndicatorKind::PanelGrid)
    }

    /// Apply an indicator mode change: embed + show, or hide.
    pub fn set_mode(&mut self, mode: IndicatorKind, providers: &[ProviderData]) {
        self.cancel_drag();
        self.mode = mode;
        // A mode change is an explicit user action — start unsuppressed; the
        // next fallback evaluation re-hides if a fullscreen app is still up.
        // The placement verdict is cleared too: it belongs to the mode that
        // just ended, and keeping it would hand the tray a stale reason to
        // stay up after the user turned the panel back on.
        self.suppressed = false;
        self.unavailable = false;
        if Self::is_panel_mode(mode) {
            self.last = None;
            self.reposition();
            self.update(providers, true);
        } else {
            #[cfg(target_os = "windows")]
            crate::platform::taskbar_space::deactivate();
            self.hide();
        }
    }

    /// Periodic upkeep + redraw-on-change. Call on every provider update.
    pub fn update(&mut self, providers: &[ProviderData], force: bool) {
        if !Self::is_panel_mode(self.mode) || self.suppressed {
            return;
        }
        self.reposition();
        let state = WeeklyQuota::from_providers(providers);
        if force || self.last.as_ref() != Some(&state) {
            self.last = Some(state);
            self.redraw(providers);
        }
        #[cfg(target_os = "windows")]
        if self.tip_shown {
            self.hide_tooltip();
            self.show_tooltip(providers);
        }
    }

    /// Show the hover tooltip — the provider summary in a dark, rounded,
    /// borderless pill centered above the overlay — painted by us so it matches
    /// the shell's own tooltips. Idempotent while shown; no-op if the panel is
    /// hidden (no rect). Called from the overlay's CursorEntered.
    pub fn show_tooltip(&mut self, providers: &[ProviderData]) {
        #[cfg(target_os = "windows")]
        {
            if self.tip_shown || self.tip_hwnd == 0 || self.drag.is_some() {
                return;
            }
            let Some((px, py, pw, _ph)) = self.rect else {
                return;
            };
            let text = WeeklyQuota::from_providers(providers).tooltip();
            let light = crate::platform::system_uses_light_theme();
            let pm = crate::ui::tray::render_tooltip(&text, self.size.1 as f32, light);
            let (w, h) = (pm.width() as i32, pm.height() as i32);
            // The pixmap carries the drop shadow around the box, so the box's
            // own bottom edge sits `shadow` above the pixmap's — add it back or
            // the gap to the bar comes out short by that much.
            let shadow = crate::ui::tray::tip_shadow_inset(self.size.1 as f32);
            let mut tx = px + (pw as i32 - w) / 2;
            if let Some((left, _, right, _)) = crate::platform::work_area_at(px, py) {
                tx = tx.clamp(left, (right - w).max(left));
            }
            let ty = py - crate::ui::tray::TIP_GAP - h + shadow;
            let bgra = pixmap_to_bgra(&pm);
            let _ = crate::platform::present_layered(self.tip_hwnd, &bgra, tx, ty, w, h);
            self.tip_shown = true;
        }
        #[cfg(not(target_os = "windows"))]
        let _ = providers;
    }

    /// Hide the hover tooltip (cursor left the overlay, or the panel is hiding).
    pub fn hide_tooltip(&mut self) {
        #[cfg(target_os = "windows")]
        if self.tip_shown {
            crate::platform::hide_window(self.tip_hwnd);
            self.tip_shown = false;
        }
    }

    /// Whether the hover tooltip is currently shown.
    #[cfg(target_os = "windows")]
    pub fn tooltip_shown(&self) -> bool {
        self.tip_shown
    }

    /// Re-assert the overlay's topmost z-order after another window covered it
    /// (the tray overflow flyout, Start menu, …). Cheap: a single SetWindowPos,
    /// no reposition or repaint. No-op when hidden or not in a panel mode.
    pub fn raise(&self) {
        if !Self::is_panel_mode(self.mode) || self.rect.is_none() {
            return;
        }
        #[cfg(target_os = "windows")]
        crate::platform::raise_panel_topmost(self.hwnd());
    }

    /// True when the panel SHOULD be visible (a Panel mode, positioned) but the
    /// taskbar is composited OVER it — the cursor-edge "rude topmost" peek that
    /// a floating overlay cannot beat. Read from what the compositor actually
    /// shows at the panel's center (WindowFromPoint); drives the tray fallback.
    #[cfg(target_os = "windows")]
    /// The panel cannot place itself at all — as opposed to being hidden with
    /// an auto-hidden bar, which is normal.
    ///
    /// `is_covered` cannot answer this: with no rectangle it reads `false`,
    /// i.e. "not obstructed", so a panel that never made it onto the screen
    /// looked exactly like a healthy one and the tray substitute stayed hidden.
    /// The user was left with no indicator at all and no way to tell why.
    pub fn is_unavailable(&self) -> bool {
        Self::is_panel_mode(self.mode) && self.unavailable
    }

    pub fn is_covered(&self) -> bool {
        if !Self::is_panel_mode(self.mode) {
            return false;
        }
        let Some((x, y, w, h)) = self.rect else {
            return false;
        };
        let owner = crate::platform::point_owner(x + w as i32 / 2, y + h as i32 / 2);
        owner != 0 && owner != self.hwnd()
    }

    /// Restart the panel: forget every cached judgement and place it again.
    ///
    /// **Why switching displays was not enough.** `set_display` changed the
    /// target but left `suppressed` alone, and `on_taskbar_moved` returns early
    /// while suppressed — so a panel parked by a fullscreen app could not be
    /// revived by moving it, toggling it, or anything else short of restarting
    /// the whole application. That is what the user hit.
    ///
    /// Clearing `last` matters too: it is the "what did I draw" cache, and a
    /// stale entry means the redraw is skipped as a no-op precisely when the
    /// panel needs re-presenting.
    pub fn restart(&mut self, providers: &[ProviderData]) {
        self.suppressed = false;
        self.unavailable = false;
        self.last = None;
        if !Self::is_panel_mode(self.mode) {
            self.hide();
            return;
        }
        self.reposition();
        self.redraw(providers);
    }

    /// The taskbar moved (auto-hide slide / resolution change) or the tray
    /// area changed width (icon pinned/unpinned) — follow it.
    pub fn on_taskbar_moved(&mut self, providers: &[ProviderData]) {
        if !Self::is_panel_mode(self.mode) || self.suppressed {
            return;
        }
        self.reposition();
        self.redraw(providers);
    }

    /// A fullscreen app took the screen: hide with the taskbar (which sits
    /// under the fullscreen window without moving, so `reposition` cannot see
    /// it) and gate every presentation path until restored — a provider
    /// update tick must not resurrect the overlay over a game. Idempotent.
    /// While hidden, `raise()` and `is_covered()` are no-ops (rect=None).
    pub fn suppress_for_fullscreen(&mut self) {
        if !Self::is_panel_mode(self.mode) || self.suppressed {
            return;
        }
        self.suppressed = true;
        self.hide();
    }

    /// The fullscreen app is gone (alt-tab back to the desktop) — return to
    /// the bar exactly like it does: re-measure the slot and re-present.
    pub fn restore_from_fullscreen(&mut self, providers: &[ProviderData]) {
        self.suppressed = false;
        if !Self::is_panel_mode(self.mode) {
            return;
        }
        self.reposition();
        self.redraw(providers);
    }

    /// Ring and number width, scaled with the taskbar height.
    fn desired_size(&self, slot_h: i32) -> (u32, u32) {
        let scale = (slot_h as f32 / 48.0).clamp(1.0, 3.0);
        // Use the taskbar height for vertical centering.
        let h = slot_h.max(1) as u32;
        let w = (self.content_width * scale).ceil() as u32;
        (w, h)
    }

    fn hide(&mut self) {
        self.cancel_drag();
        self.hide_tooltip();
        #[cfg(target_os = "windows")]
        crate::platform::hide_window(self.hwnd());
        self.rect = None;
    }

    fn resize_pixmap(&mut self, w: u32, h: u32) {
        self.size = (w, h);
        if let Some(pm) = Pixmap::new(w, h) {
            self.pixmap = pm;
        }
    }

    /// Track the taskbar: hide with it (auto-hide), or float just left of the
    /// notification area, vertically centered in the bar.
    fn reposition(&mut self) {
        #[cfg(target_os = "windows")]
        {
            let before = self.rect;
            let Some(slot) = crate::platform::taskbar_slot(self.display) else {
                // No taskbar at all: the panel has nowhere to live, and the tray
                // has nowhere either — but say so, so the indicator can degrade
                // instead of silently showing nothing.
                if before.is_some() {
                    tracing::debug!("panel hidden: no taskbar resolved for {:?}", self.display);
                }
                self.unavailable = true;
                self.hide();
                return;
            };
            if !slot.visible {
                // The bar slid away (auto-hide). NOT unavailable: the tray icon
                // sits in that same bar and is hidden with it, so substituting
                // one for the other would gain nothing and flicker on every
                // slide.
                if before.is_some() {
                    tracing::debug!(
                        "panel hidden: bar auto-hidden (top {}, height {})",
                        slot.top,
                        slot.height
                    );
                }
                self.unavailable = false;
                self.hide();
                return;
            }
            // The panel only supports a bottom taskbar. A left/right (vertical)
            // taskbar reports a full-screen-tall slot; sizing the panel to it
            // would draw a screen-height strip off the edge. Bail out (the tray
            // indicator still works) rather than misrender. A 200%-DPI bottom
            // bar is ~96px, so anything taller than this is not a bottom bar.
            const MAX_BAR_HEIGHT: i32 = 200;
            if slot.height > MAX_BAR_HEIGHT {
                // A vertical taskbar: we refuse to draw on it. The tray icon
                // still works there, so this IS a case for the substitute.
                if before.is_some() {
                    tracing::debug!("panel hidden: bar height {} looks vertical", slot.height);
                }
                self.unavailable = true;
                self.hide();
                return;
            }
            let (w, h) = self.desired_size(slot.height);
            // An estimated tray edge is a guess at where the clock starts, so
            // keep a little more air than when the edge was measured.
            let margin = if slot.tray_found {
                TRAY_MARGIN
            } else {
                TRAY_MARGIN * 2
            };
            let scale = crate::platform::taskbar_geom::bar_scale(slot.height);
            let preferred = slot.tray_left - w as i32 - margin + self.offset.0;
            let safe = if self.position_x.is_none() {
                crate::platform::taskbar_space::occupied(
                    slot.hwnd,
                    (slot.left, slot.top, slot.right, slot.top + slot.height),
                )
                .and_then(|mut occupied| {
                    occupied.push((slot.tray_left, slot.right));
                    crate::platform::taskbar_geom::free_panel_x(
                        slot.left,
                        slot.tray_left,
                        w as i32,
                        preferred,
                        (6.0 * scale).round() as i32,
                        &occupied,
                    )
                })
            } else {
                crate::platform::taskbar_space::deactivate();
                None
            };
            let x = crate::platform::taskbar_geom::panel_x(
                slot.left,
                slot.right,
                w as i32,
                scale,
                self.position_x,
                self.last_position_x,
                safe,
                preferred,
            );
            self.last_position_x = Some(((x - slot.left) as f32 / scale).round() as i32);
            if self.locked && self.position_x.is_none() {
                self.position_x = self.last_position_x;
            }
            self.unavailable = false;
            let y = slot.top + (slot.height - h as i32) / 2;
            if self.size != (w, h) {
                self.resize_pixmap(w, h);
                self.last = None;
            }
            // Re-presenting is needed when reappearing after a hide.
            if self.rect.is_none() {
                self.last = None;
            }
            let y = (y + self.offset.1).clamp(slot.top, slot.top + slot.height - h as i32);
            if before.map(|(bx, _, _, _)| bx) != Some(x) {
                tracing::debug!(
                    "panel placed at {x},{y} (target {:?}, bar top {}, tray_left {}, measured {})",
                    self.display,
                    slot.top,
                    slot.tray_left,
                    slot.tray_found
                );
            }
            self.rect = Some((x, y, w, h));
        }
    }

    /// Paint (near-transparent background) and present via UpdateLayeredWindow:
    /// theme-aware activity ring and adjacent remaining percentage.
    pub fn redraw(&mut self, providers: &[ProviderData]) {
        if !Self::is_panel_mode(self.mode) {
            return;
        }
        let Some((x, y, w, h)) = self.rect else {
            return;
        };
        self.pixmap = render_styled_panel(
            w,
            h,
            &WeeklyQuota::from_providers(providers),
            crate::platform::system_uses_light_theme(),
            &self.appearance,
        );

        // Premultiplied RGBA (tiny-skia) → premultiplied BGRA (top-down) for
        // UpdateLayeredWindow.
        let data = self.pixmap.data();
        let mut bgra = vec![0u8; data.len()];
        for (s, d) in data.chunks_exact(4).zip(bgra.chunks_exact_mut(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
        #[cfg(target_os = "windows")]
        match crate::platform::present_layered(self.hwnd(), &bgra, x, y, w as i32, h as i32) {
            Ok(()) => {
                if self.last_present_error.is_some() {
                    tracing::info!("taskbar panel present recovered");
                    self.last_present_error = None;
                }
            }
            Err(code) => {
                if self.last_present_error != Some(code) {
                    tracing::warn!("taskbar panel present failed, error {code}");
                    self.last_present_error = Some(code);
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (&bgra, x, y);
        }
    }
}

/// Shared by the live panel and render verification; no window is needed.
#[cfg(test)]
fn render_panel(w: u32, h: u32, quota: &WeeklyQuota, light: bool) -> Pixmap {
    render_styled_panel(w, h, quota, light, &Appearance::default())
}

pub(crate) fn appearance_width(style: &Appearance) -> f32 {
    (style.ring_x as f32 + style.ring_size as f32)
        .max(style.number_x as f32 + crate::ui::numbers::width(style, 1.0))
        + 8.0
}

pub(crate) fn render_styled_panel(
    w: u32,
    h: u32,
    quota: &WeeklyQuota,
    light: bool,
    style: &Appearance,
) -> Pixmap {
    let mut pm = Pixmap::new(w, h).expect("panel dimensions");
    pm.fill(color(0, 0, 0, HIT_ALPHA));
    let scale = (h as f32 / 48.0).clamp(1.0, 3.0);
    let side = (style.ring_size as f32 * scale).round() as u32;
    let mut ring = Pixmap::new(side, side).expect("ring dimensions");
    paint_styled_ring(&mut ring, quota, light, style);
    pm.draw_pixmap(
        (style.ring_x as f32 * scale).round() as i32,
        ((h as i32 - side as i32) / 2 + (style.ring_y as f32 * scale).round() as i32)
            .clamp(0, (h as i32 - side as i32).max(0)),
        ring.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );
    let ink = if quota.muted() {
        if light {
            color(112, 112, 120, 255)
        } else {
            color(158, 158, 168, 255)
        }
    } else {
        crate::config::appearance::rgb(&style.number_color)
            .map(|[r, g, b]| color(r, g, b, 255))
            .unwrap_or_else(|| theme_ink(light).0)
    };
    let x = style.number_x as f32 * scale;
    crate::ui::numbers::draw(&mut pm, quota, x, scale, ink, style);
    pm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn customized_numbers_fit_all_readings_weights_and_scales() {
        use crate::config::appearance::NumberWeight;
        for weight in [
            NumberWeight::Regular,
            NumberWeight::Semibold,
            NumberWeight::Bold,
        ] {
            for size in [10, 20, 40] {
                let style = Appearance {
                    number_weight: weight,
                    number_size: size,
                    number_x: 65,
                    ring_x: 0,
                    ring_size: 44,
                    ring_thickness: 12,
                    number_y: 20,
                    ring_y: -20,
                    symbol_percent: 100,
                    ..Default::default()
                };
                for scale in [1.0, 1.5, 2.0] {
                    let width = (appearance_width(&style) * scale).ceil() as u32;
                    let height = (48.0 * scale) as u32;
                    for remaining in [
                        None,
                        Some(0.0),
                        Some(1.0),
                        Some(68.0),
                        Some(88.0),
                        Some(100.0),
                    ] {
                        let mut quota = WeeklyQuota::from_providers(&[]);
                        quota.remaining = remaining;
                        quota.estimated = true;
                        let rendered = render_styled_panel(width, height, &quota, false, &style);
                        // No ink reaches the outer right or bottom edge, even with
                        // the largest symbols, estimated values and vertical offset.
                        assert!((0..height)
                            .all(|y| rendered.pixel(width - 1, y).unwrap().alpha() == HIT_ALPHA));
                        assert!(
                            (0..width).all(
                                |x| rendered.pixel(x, height - 1).unwrap().alpha() == HIT_ALPHA
                            ),
                            "bottom clipping: {weight:?}, {size}, {scale}, {remaining:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn independent_colors_and_muted_state_do_not_modify_quota() {
        let quota = WeeklyQuota::from_providers(&[crate::ui::weekly::tests::data(32)]);
        let original = quota.clone();
        let style = Appearance {
            ring_color: "#00FF00".into(),
            number_color: "#FF0000".into(),
            ..Default::default()
        };
        let w = appearance_width(&style).ceil() as u32;
        for light in [false, true] {
            let rendered = render_styled_panel(w, 48, &quota, light, &style);
            assert!(rendered
                .pixels()
                .iter()
                .any(|p| p.alpha() > 240 && p.green() > 240 && p.red() < 5));
            assert!(rendered
                .pixels()
                .iter()
                .any(|p| p.alpha() > 240 && p.red() > 240 && p.green() < 5));
            let mut stale = quota.clone();
            stale.stale = true;
            let muted = render_styled_panel(w, 48, &stale, light, &style);
            assert!(muted
                .pixels()
                .iter()
                .filter(|p| p.alpha() > 200)
                .all(|p| (p.red() as i32 - p.green() as i32).abs() < 3));
        }
        assert_eq!(quota, original);
    }

    #[test]
    fn ring_boundaries_and_number_spacing() {
        for scale in [1.0, 1.5, 2.0] {
            let w = ((PAD_X * 2.0 + RING_SIZE + NUM_GAP + NUM_W) * scale) as u32;
            let h = (48.0 * scale) as u32;
            for light in [false, true] {
                let empty = WeeklyQuota::from_providers(&[crate::ui::weekly::tests::data(100)]);
                let full = WeeklyQuota::from_providers(&[crate::ui::weekly::tests::data(0)]);
                let unknown = WeeklyQuota::from_providers(&[]);
                let zero = render_panel(w, h, &empty, light);
                let full = render_panel(w, h, &full, light);
                let unknown = render_panel(w, h, &unknown, light);
                let ring_end = ((PAD_X + RING_SIZE) * scale) as u32;
                let text_start = ((PAD_X + RING_SIZE + NUM_GAP) * scale) as u32;
                for y in 0..h {
                    for x in 0..ring_end {
                        assert_eq!(
                            zero.pixel(x, y).unwrap().alpha() > HIT_ALPHA,
                            unknown.pixel(x, y).unwrap().alpha() > HIT_ALPHA,
                            "0% must only draw the track"
                        );
                    }
                    for x in ring_end..text_start {
                        assert_eq!(
                            full.pixel(x, y).unwrap().alpha(),
                            HIT_ALPHA,
                            "ring and digits must remain separated"
                        );
                    }
                }
                let solid = |pm: &Pixmap| {
                    (0..h)
                        .flat_map(|y| (0..ring_end).map(move |x| (x, y)))
                        .filter(|(x, y)| pm.pixel(*x, *y).unwrap().alpha() > 200)
                        .count()
                };
                assert_eq!(solid(&zero), 0);
                assert!(solid(&full) > (100.0 * scale * scale) as usize);
            }
        }
    }

    #[test]
    #[ignore = "writes visual review sheets; run in isolation because language is process-global"]
    fn preview_weekly_ring() {
        use crate::config::schema::Language;
        use crate::ui::tray::render_tooltip;
        let dir = std::path::Path::new("target/weekly-preview");
        std::fs::create_dir_all(dir).unwrap();
        for (language, code) in [(Language::Chinese, "zh-CN"), (Language::English, "en")] {
            crate::i18n::set_language(language);
            for (percent, scale) in [(100, 1.0), (150, 1.5), (200, 2.0)] {
                let mut sheet = Pixmap::new(1400, 1320).unwrap();
                for (col, light) in [true, false].into_iter().enumerate() {
                    let bg = if light {
                        color(242, 242, 246, 255)
                    } else {
                        color(28, 28, 32, 255)
                    };
                    crate::ui::tray::fill_round_rect(
                        &mut sheet,
                        col as f32 * 700.0,
                        0.0,
                        700.0,
                        1320.0,
                        0.0,
                        bg,
                    );
                    let mut states: Vec<_> = [32, 0, 100, 99]
                        .into_iter()
                        .map(|used| {
                            WeeklyQuota::from_providers(&[crate::ui::weekly::tests::data(used)])
                        })
                        .collect();
                    let mut stale = states[0].clone();
                    stale.stale = true;
                    stale.status = "Cached quota (outdated)";
                    states.push(stale);
                    let mut estimated = states[1].clone();
                    estimated.estimated = true;
                    estimated.status = "Estimated quota";
                    states.push(estimated);
                    states.push(WeeklyQuota::from_providers(&[]));
                    for (row, state) in states.into_iter().enumerate() {
                        let x = col as i32 * 700 + 25;
                        let y = row as i32 * 185 + 15;
                        let panel = render_panel(
                            ((PAD_X * 2.0 + RING_SIZE + NUM_GAP + NUM_W) * scale) as u32,
                            (48.0 * scale) as u32,
                            &state,
                            light,
                        );
                        sheet.draw_pixmap(
                            x,
                            y,
                            panel.as_ref(),
                            &tiny_skia::PixmapPaint::default(),
                            tiny_skia::Transform::identity(),
                            None,
                        );
                        // Tooltip gets its own row across the full sheet width in separate artifacts.
                        let tooltip = render_tooltip(&state.tooltip(), 48.0 * scale, light);
                        tooltip
                            .save_png(
                                dir.join(format!("{code}-{percent}-{light}-{row}-tooltip.png")),
                            )
                            .unwrap();
                    }
                }
                sheet
                    .save_png(dir.join(format!("{code}-{percent}.png")))
                    .unwrap();
            }
        }
        crate::i18n::set_language(Language::Auto);
    }
}
