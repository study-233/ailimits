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
const PAD_X: f32 = 6.0;
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

type PanelRect = (i32, i32, u32, u32);

/// Pixel inputs deliberately exclude screen position: moving an unchanged
/// panel only needs to present the existing bitmap at its new coordinates.
#[derive(Clone, PartialEq)]
struct FrameState {
    quotas: Vec<WeeklyQuota>,
    light: bool,
    appearance: Appearance,
    warning_remaining: f32,
    size: (u32, u32),
    hovered: bool,
    pressed: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum FrameUpdate {
    Unchanged,
    PresentCached,
    Rendered,
}

#[derive(Default)]
struct PanelFrame {
    state: Option<FrameState>,
    bgra: Vec<u8>,
    /// Only successful presentation makes a placement reusable. Hiding or a
    /// failed present must leave another presentation pending, even unchanged.
    presented: Option<PanelRect>,
}

impl PanelFrame {
    fn prepare(&mut self, state: FrameState, rect: PanelRect, force: bool) -> FrameUpdate {
        if force || self.state.as_ref() != Some(&state) {
            let pixmap = render_frame(&state);
            self.bgra.resize(pixmap.data().len(), 0);
            // Premultiplied RGBA -> BGRA for UpdateLayeredWindow.
            for (src, dst) in pixmap
                .data()
                .chunks_exact(4)
                .zip(self.bgra.chunks_exact_mut(4))
            {
                dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
            }
            self.state = Some(state);
            self.presented = None;
            FrameUpdate::Rendered
        } else if self.presented != Some(rect) {
            FrameUpdate::PresentCached
        } else {
            FrameUpdate::Unchanged
        }
    }

    fn hide(&mut self) {
        self.presented = None;
    }
}

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

pub struct TaskbarPanel {
    warning_remaining: f32,
    hovered: bool,
    pressed: bool,
    click_pending: bool,
    locked: bool,
    appearance: Appearance,
    content_width: f32,
    position_x: Option<i32>,
    last_position_x: Option<i32>,
    drag: Option<PanelDrag>,
    window: Rc<Window>,
    frame: PanelFrame,
    mode: IndicatorKind,
    size: (u32, u32),
    /// Current screen placement, or None while hidden (taskbar slid away).
    rect: Option<PanelRect>,
    /// True while a fullscreen app owns the screen: EVERY presentation path
    /// (update ticks, taskbar moves, raises) is gated on this, so nothing can
    /// resurrect the overlay over a game between fallback evaluations.
    suppressed: bool,
    /// The panel could not place itself: no taskbar resolved, or the bar is a
    /// shape we refuse to draw on. Distinct from "the bar is auto-hidden",
    /// which is normal and needs no substitute — the tray icon lives in that
    /// same bar and would be hidden with it.
    unavailable: bool,
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

impl TaskbarPanel {
    /// Create the (hidden) overlay window up front; it is shown and embedded
    /// only when the indicator switches to a Panel mode.
    pub fn new(event_loop: &tao::event_loop::EventLoop<crate::app::UserEvent>) -> Result<Self> {
        let mut builder = tao::window::WindowBuilder::new()
            .with_title("QuotaBar Panel")
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
        Ok(Self {
            warning_remaining: 20.0,
            hovered: false,
            pressed: false,
            click_pending: false,
            locked: false,
            appearance: Appearance::default(),
            content_width: appearance_width(&Appearance::default()),
            position_x: None,
            last_position_x: None,
            drag: None,
            window,
            frame: PanelFrame::default(),
            mode: IndicatorKind::Off,
            size: (INIT_W, INIT_H),
            rect: None,
            suppressed: false,
            unavailable: false,
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
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn set_warning_threshold(&mut self, used: u8) {
        self.warning_remaining = 100.0 - used.min(100) as f32;
    }

    pub fn set_appearance(&mut self, mut appearance: Appearance) {
        appearance.normalize();
        self.content_width = appearance_width(&appearance);
        self.appearance = appearance;
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
        self.click_pending = true;
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
            // Locked entries still track the gesture, so a drag does not
            // become a click merely because moving the panel is disabled.
            if self.locked {
                return;
            }
            let x =
                (drag.left + dx).clamp(slot.left, (slot.right - self.size.0 as i32).max(slot.left));
            self.position_x = Some(((x - slot.left) as f32 / scale).round() as i32);
            self.update(providers, false);
        }
        #[cfg(not(windows))]
        let _ = providers;
    }

    pub fn appearance(&self) -> &Appearance {
        &self.appearance
    }
    #[cfg(windows)]
    pub fn anchor_rect(&self) -> Option<windows::Win32::Foundation::RECT> {
        self.rect
            .map(|(x, y, w, h)| windows::Win32::Foundation::RECT {
                left: x,
                top: y,
                right: x + w as i32,
                bottom: y + h as i32,
            })
    }
    pub fn take_click(&mut self) -> bool {
        let click = self.click_pending && !self.drag.as_ref().is_some_and(|d| d.moved);
        self.click_pending = false;
        click
    }
    /// Returns the new persistent X only for a completed drag, not a click.
    pub fn finish_drag(&mut self) -> Option<i32> {
        let drag = self.drag.take()?;
        self.release_capture();
        if drag.moved && !self.locked {
            self.position_x
        } else {
            None
        }
    }

    pub fn cancel_drag(&mut self) {
        self.click_pending = false;
        if let Some(drag) = self.drag.take() {
            self.position_x = drag.original;
            self.last_position_x = drag.last_original;
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

    pub fn set_interaction(&mut self, hovered: bool, pressed: bool, providers: &[ProviderData]) {
        if self.hovered != hovered || self.pressed != pressed {
            self.hovered = hovered;
            self.pressed = pressed;
            if !self.suppressed {
                self.redraw(providers);
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
        self.present(providers, force);
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
    /// Force fresh pixels and presentation even when the cached inputs and
    /// placement match: this is also the explicit recovery action.
    pub fn restart(&mut self, providers: &[ProviderData]) {
        self.suppressed = false;
        self.unavailable = false;
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
        self.update(providers, false);
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
        self.update(providers, false);
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
        #[cfg(target_os = "windows")]
        crate::platform::hide_window(self.hwnd());
        self.rect = None;
        self.frame.hide();
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
            self.size = (w, h);
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
        self.present(providers, true);
    }

    fn present(&mut self, providers: &[ProviderData], force: bool) {
        if !Self::is_panel_mode(self.mode) || self.suppressed {
            return;
        }
        let Some(rect @ (x, y, w, h)) = self.rect else {
            return;
        };
        let state = FrameState {
            quotas: super::quota_view::selected(providers, &self.appearance),
            light: crate::platform::system_uses_light_theme(),
            appearance: self.appearance.clone(),
            warning_remaining: self.warning_remaining,
            size: (w, h),
            hovered: self.hovered,
            pressed: self.pressed,
        };
        if self.frame.prepare(state, rect, force) == FrameUpdate::Unchanged {
            return;
        }
        #[cfg(target_os = "windows")]
        match crate::platform::present_layered(
            self.hwnd(),
            &self.frame.bgra,
            x,
            y,
            w as i32,
            h as i32,
        ) {
            Ok(()) => {
                self.frame.presented = Some(rect);
                if self.last_present_error.is_some() {
                    tracing::info!("taskbar panel present recovered");
                    self.last_present_error = None;
                }
            }
            Err(code) => {
                self.frame.presented = None;
                if self.last_present_error != Some(code) {
                    tracing::warn!("taskbar panel present failed, error {code}");
                    self.last_present_error = Some(code);
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (x, y);
            self.frame.presented = Some(rect);
        }
    }
}

fn render_frame(state: &FrameState) -> Pixmap {
    let (w, h) = state.size;
    let pixmap = super::quota_view::render_with_threshold(
        w,
        h,
        &state.quotas,
        state.light,
        &state.appearance,
        state.warning_remaining,
    );
    if !state.hovered && !state.pressed {
        return pixmap;
    }
    let mut surface = Pixmap::new(w, h).expect("panel hover");
    surface.fill(color(0, 0, 0, HIT_ALPHA));
    let alpha = if state.pressed { 20 } else { 12 };
    let ink = if state.light {
        color(0, 0, 0, alpha)
    } else {
        color(255, 255, 255, alpha)
    };
    let scale = h as f32 / 48.;
    super::tray::fill_round_rect(
        &mut surface,
        0.,
        2. * scale,
        w as f32,
        h as f32 - 4. * scale,
        super::theme::UiMetrics::CONTROL_RADIUS * scale,
        ink,
    );
    surface.draw_pixmap(
        0,
        0,
        pixmap.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );
    surface
}

/// Shared by the live panel and render verification; no window is needed.
#[cfg(test)]
fn render_panel(w: u32, h: u32, quota: &WeeklyQuota, light: bool) -> Pixmap {
    render_styled_panel(w, h, quota, light, &Appearance::default())
}

pub(crate) fn appearance_width(style: &Appearance) -> f32 {
    super::quota_view::width(style)
}

pub(crate) fn single_ring_width(style: &Appearance) -> f32 {
    (style.ring_x as f32 + style.ring_size as f32)
        .max(style.number_x as f32 + crate::ui::numbers::width(style, 1.0))
        + 6.0
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

    fn frame_state() -> FrameState {
        let appearance = Appearance::default();
        FrameState {
            quotas: vec![WeeklyQuota::from_providers(&[
                crate::ui::weekly::tests::data(32),
            ])],
            light: false,
            size: (appearance_width(&appearance).ceil() as u32, 48),
            appearance,
            warning_remaining: 20.,
            hovered: false,
            pressed: false,
        }
    }

    #[test]
    fn stationary_checks_skip_work_and_moves_reuse_pixels() {
        let state = frame_state();
        let rect = (100, 900, state.size.0, state.size.1);
        let mut frame = PanelFrame::default();
        assert_eq!(
            frame.prepare(state.clone(), rect, false),
            FrameUpdate::Rendered
        );
        frame.presented = Some(rect);
        let pixels = frame.bgra.clone();
        let buffer = frame.bgra.as_ptr();
        for _ in 0..120 {
            assert_eq!(
                frame.prepare(state.clone(), rect, false),
                FrameUpdate::Unchanged
            );
        }
        // Both horizontal placement changes and the shell's vertical slide
        // still need presenting, but neither changes the bitmap.
        for moved in [(120, 900, rect.2, rect.3), (120, 905, rect.2, rect.3)] {
            assert_eq!(
                frame.prepare(state.clone(), moved, false),
                FrameUpdate::PresentCached
            );
            assert_eq!(frame.bgra, pixels);
            assert_eq!(frame.bgra.as_ptr(), buffer);
            frame.presented = Some(moved);
            assert_eq!(
                frame.prepare(state.clone(), moved, false),
                FrameUpdate::Unchanged
            );
        }
    }

    #[test]
    fn hidden_or_failed_present_is_retried_without_rasterizing() {
        let state = frame_state();
        let rect = (100, 900, state.size.0, state.size.1);
        let mut frame = PanelFrame::default();
        assert_eq!(
            frame.prepare(state.clone(), rect, false),
            FrameUpdate::Rendered
        );
        // A failed first present must never make a subsequent check a no-op.
        assert_eq!(
            frame.prepare(state.clone(), rect, false),
            FrameUpdate::PresentCached
        );
        frame.presented = Some(rect);
        let pixels = frame.bgra.clone();
        frame.hide();
        assert_eq!(
            frame.prepare(state.clone(), rect, false),
            FrameUpdate::PresentCached
        );
        assert_eq!(frame.bgra, pixels);
        frame.presented = Some(rect);
        frame.hide();
        // Quota changes while hidden must be drawn before the panel returns.
        let mut changed = state;
        changed.quotas[0].remaining = Some(12.);
        assert_eq!(frame.prepare(changed, rect, false), FrameUpdate::Rendered);
        assert_ne!(frame.bgra, pixels);
    }

    #[test]
    fn visible_inputs_and_forced_refresh_invalidate_pixels() {
        let baseline = frame_state();
        let rect = (100, 900, baseline.size.0, baseline.size.1);
        let mut variants = Vec::new();
        let mut add = |change: fn(&mut FrameState)| {
            let mut state = baseline.clone();
            change(&mut state);
            variants.push(state);
        };
        add(|s| s.light = true);
        add(|s| s.hovered = true);
        add(|s| {
            s.hovered = true;
            s.pressed = true;
        });
        add(|s| s.warning_remaining = 75.);
        add(|s| s.appearance.ring_color = "#00FF00".into());
        add(|s| s.size = (s.size.0 * 2, s.size.1 * 2));
        add(|s| s.quotas[0].remaining = Some(12.));
        add(|s| s.quotas[0].stale = true);
        add(|s| s.quotas[0].estimated = true);
        add(|s| s.quotas[0].remaining = None);
        for state in variants {
            let mut frame = PanelFrame::default();
            frame.prepare(baseline.clone(), rect, false);
            frame.presented = Some(rect);
            let next = (rect.0, rect.1, state.size.0, state.size.1);
            assert_eq!(
                frame.prepare(state.clone(), next, false),
                FrameUpdate::Rendered
            );
            assert_eq!(frame.bgra.len(), (state.size.0 * state.size.1 * 4) as usize);
            frame.presented = Some(next);
            assert_eq!(
                frame.prepare(state.clone(), next, false),
                FrameUpdate::Unchanged
            );
            assert_eq!(frame.prepare(state, next, true), FrameUpdate::Rendered);
        }
    }

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
