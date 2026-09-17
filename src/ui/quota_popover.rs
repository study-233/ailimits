//! Native quota panel and chart details. All graph samples come from account history.
use super::{
    native_controls::*,
    theme::{Color, SurfaceTheme},
    weekly::WeeklyQuota,
};
use crate::{
    app::UserEvent,
    config::{
        appearance::Appearance,
        panel::{ModuleId, PanelConfig},
    },
    i18n::t,
    meter::{
        self,
        extras::{Availability, Snapshot},
        history::{AccountHistory, Observation},
    },
    providers::{MetricWindow, ProviderData, ProviderId},
};
use chrono::{DateTime, Duration, Local, Utc};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use tao::event_loop::EventLoopProxy;
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{SetScrollInfo, DRAWITEMSTRUCT},
            HiDpi::*,
            Input::KeyboardAndMouse::{GetAsyncKeyState, SetFocus},
            WindowsAndMessaging::*,
        },
    },
};
const WIDTH: f32 = 400.;
const HEADER: f32 = 78.;
const FOOTER: f32 = 78.;
const SETTINGS: usize = 1;
const QUIT: usize = 2;
const REFRESH: usize = 3;
const HISTORY: usize = 4;
const TOKENS: usize = 5;
const CYCLE: usize = 6;
const SEVEN: usize = 7;
const THIRTY: usize = 8;
thread_local! {static ACTIVE:Cell<HWND>=const{Cell::new(HWND(std::ptr::null_mut()))};static DETAIL:Cell<HWND>=const{Cell::new(HWND(std::ptr::null_mut()))};}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartKind {
    Quota,
    Tokens,
}
#[derive(Clone, PartialEq)]
pub(crate) struct PanelContext {
    pub layout: PanelConfig,
    pub extras: Snapshot,
    pub history: Rc<AccountHistory>,
    pub threshold: u8,
    pub interval: u64,
}
impl Default for PanelContext {
    fn default() -> Self {
        Self {
            layout: PanelConfig::default(),
            extras: Snapshot::default(),
            history: Rc::default(),
            threshold: 80,
            interval: 60,
        }
    }
}
#[derive(Clone, PartialEq)]
struct View {
    quotas: Vec<WeeklyQuota>,
    seconds: [Option<u64>; 2],
    updated: Option<DateTime<Utc>>,
    live: [bool; 2],
    plan: String,
    chinese: bool,
    style: Appearance,
}
impl View {
    fn new(data: &[ProviderData], style: &Appearance) -> Self {
        let codex = data.iter().find(|d| d.id == ProviderId::Codex);
        Self {
            quotas: vec![
                WeeklyQuota::for_window(data, MetricWindow::Session),
                WeeklyQuota::for_window(data, MetricWindow::Long),
            ],
            seconds: [MetricWindow::Session, MetricWindow::Long].map(|window| {
                codex
                    .and_then(|d| d.metrics.iter().find(|m| m.window == window))
                    .and_then(|m| m.window_seconds)
            }),
            updated: codex.map(|d| d.updated_at),
            live: [MetricWindow::Session, MetricWindow::Long].map(|window| {
                codex.is_some_and(|d| {
                    d.received_at.is_some()
                        && d.metrics
                            .iter()
                            .find(|m| m.window == window)
                            .is_some_and(|m| m.observed_at == Some(d.updated_at))
                })
            }),
            chinese: crate::i18n::is_chinese(),
            plan: codex
                .and_then(|d| d.plan_type.as_ref())
                .map(|p| {
                    format!(
                        "ChatGPT {}",
                        match p.as_str() {
                            "plus" => "Plus",
                            "pro" => "Pro",
                            "prolite" => "Pro Lite",
                            "free" => "Free",
                            "team" => "Team",
                            "go" => "Go",
                            other => other,
                        }
                    )
                })
                .unwrap_or_else(|| t("Plan unavailable").into()),
            style: style.clone(),
        }
    }
}
struct Resources {
    body: HFONT,
    caption: HFONT,
    title: HFONT,
    bold: HFONT,
    background: HBRUSH,
}
impl Resources {
    unsafe fn new(scale: f32, light: bool) -> Rc<Self> {
        Rc::new(Self {
            body: sized_font(scale, 13., 400),
            caption: sized_font(scale, 11., 400),
            title: sized_font(scale, 21., 600),
            bold: sized_font(scale, 13., 600),
            background: CreateSolidBrush(native_color(SurfaceTheme::new(light).background)),
        })
    }
}
impl Drop for Resources {
    fn drop(&mut self) {
        unsafe {
            for f in [self.body, self.caption, self.title, self.bold] {
                let _ = DeleteObject(f);
            }
            let _ = DeleteObject(self.background);
        }
    }
}
#[derive(Clone)]
struct State {
    view: View,
    context: PanelContext,
    scale: f32,
    light: bool,
    resources: Rc<Resources>,
    proxy: EventLoopProxy<UserEvent>,
    anchor: RECT,
    dismissed_by_entry: bool,
    scroll: f32,
    detail: Option<ChartKind>,
    days: u32,
    hover: Option<f32>,
    charts: Rc<RefCell<[Option<Rc<ChartImage>>; 2]>>,
}
unsafe fn snapshot(hwnd: HWND) -> Option<State> {
    let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<State>;
    (!p.is_null()).then(|| (*p).borrow().clone())
}
unsafe fn change(hwnd: HWND, f: impl FnOnce(&mut State)) {
    let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<State>;
    if !p.is_null() {
        f(&mut (*p).borrow_mut());
    }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn rect(s: f32, x: f32, y: f32, w: f32, h: f32) -> RECT {
    RECT {
        left: (x * s).round() as i32,
        top: (y * s).round() as i32,
        right: ((x + w) * s).round() as i32,
        bottom: ((y + h) * s).round() as i32,
    }
}
fn placement(a: RECT, w: RECT, width: i32, height: i32, gap: i32) -> RECT {
    let x = (a.right - width).clamp(w.left, (w.right - width).max(w.left));
    let above = a.top - gap - height;
    let y = if above >= w.top {
        above
    } else {
        a.bottom + gap
    }
    .clamp(w.top, (w.bottom - height).max(w.top));
    RECT {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    }
}
fn section_height(id: ModuleId, c: &PanelContext) -> f32 {
    match id {
        ModuleId::FiveHour | ModuleId::Weekly => 174.,
        ModuleId::ResetCredits => match &c.extras.credits {
            Availability::Ready(credits) => {
                76. + credits.items.as_ref().map_or(0, Vec::len) as f32 * 47.
            }
            _ => 90.,
        },
        _ => 171.,
    }
}
fn sections(c: &PanelContext) -> Vec<(ModuleId, f32, f32)> {
    let mut y = 0.;
    c.layout
        .modules
        .iter()
        .filter(|m| m.visible)
        .map(|m| {
            let h = section_height(m.id, c);
            let row = (m.id, y, h);
            y += h;
            row
        })
        .collect()
}
fn content_height(c: &PanelContext) -> f32 {
    sections(c).iter().map(|(_, _, h)| h).sum::<f32>().max(100.)
}
fn message<T>(v: &Availability<T>) -> &'static str {
    match v {
        Availability::NotLoaded => "Not loaded",
        Availability::Loading => "Loading",
        Availability::Unavailable(s) => s,
        Availability::Ready(_) => "",
    }
}

pub(crate) struct QuotaPopover {
    hwnd: HWND,
    details: HWND,
    proxy: EventLoopProxy<UserEvent>,
    context: PanelContext,
}
impl QuotaPopover {
    pub fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            hwnd: HWND::default(),
            details: HWND::default(),
            proxy,
            context: PanelContext::default(),
        }
    }
    pub fn is_visible(&self) -> bool {
        unsafe { IsWindowVisible(self.hwnd).as_bool() }
    }
    pub fn configure(&mut self, context: PanelContext) {
        if self.context == context {
            return;
        }
        self.context = context.clone();
        unsafe {
            for hwnd in [self.hwnd, self.details] {
                if IsWindow(hwnd).as_bool() {
                    change(hwnd, |s| s.context = context.clone());
                    refresh(hwnd);
                }
            }
        }
    }
    pub fn update(&self, data: &[ProviderData], style: &Appearance) {
        unsafe {
            for hwnd in [self.hwnd, self.details] {
                if !IsWindow(hwnd).as_bool() {
                    continue;
                }
                let view = View::new(data, style);
                if snapshot(hwnd).is_some_and(|s| s.view == view) {
                    continue;
                }
                change(hwnd, |s| s.view = view);
                if IsWindowVisible(hwnd).as_bool() {
                    refresh(hwnd);
                }
            }
        }
    }
    pub fn cancel_entry_click(&self) {
        unsafe {
            change(self.hwnd, |s| s.dismissed_by_entry = false);
        }
    }
    pub fn hide(&self) {
        unsafe {
            hide(self.hwnd);
        }
    }
    pub fn show_more(&self, menu: &muda::Submenu) {
        unsafe {
            use muda::ContextMenu as _;
            menu.show_context_menu_for_hwnd(self.hwnd.0 as isize, None);
            hide(self.hwnd);
        }
    }
    pub fn toggle(
        &mut self,
        data: &[ProviderData],
        style: &Appearance,
        anchor: Option<RECT>,
    ) -> anyhow::Result<()> {
        unsafe {
            if self.is_visible() {
                hide(self.hwnd);
                return Ok(());
            }
            if snapshot(self.hwnd).is_some_and(|s| s.dismissed_by_entry) {
                self.cancel_entry_click();
                return Ok(());
            }
            let anchor = anchor.unwrap_or_else(|| {
                let mut p = POINT::default();
                let _ = GetCursorPos(&mut p);
                RECT {
                    left: p.x,
                    top: p.y,
                    right: p.x + 1,
                    bottom: p.y + 1,
                }
            });
            let view = View::new(data, style);
            if !IsWindow(self.hwnd).as_bool() {
                match create(
                    self.proxy.clone(),
                    view.clone(),
                    self.context.clone(),
                    anchor,
                    None,
                ) {
                    Ok(h) => self.hwnd = h,
                    Err(e) => {
                        return Err(e);
                    }
                }
            }
            change(self.hwnd, |s| {
                s.view = view;
                s.context = self.context.clone();
                s.anchor = anchor;
                s.scroll = 0.;
            });
            refresh(self.hwnd);
            ACTIVE.with(|a| a.set(self.hwnd));
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(GetDlgItem(self.hwnd, REFRESH as i32).unwrap_or(self.hwnd));
            SetTimer(self.hwnd, 1, 60_000, None);
            let _ = self.proxy.send_event(UserEvent::RefreshQuota(false));
            Ok(())
        }
    }
    pub fn open_history(&mut self, kind: ChartKind) {
        unsafe {
            let Some(s) = snapshot(self.hwnd) else {
                return;
            };
            if IsWindow(self.details).as_bool() {
                let _ = DestroyWindow(self.details);
            }
            match create(
                self.proxy.clone(),
                s.view,
                self.context.clone(),
                s.anchor,
                Some(kind),
            ) {
                Ok(h) => {
                    self.details = h;
                    DETAIL.with(|a| a.set(h));
                    refresh(h);
                    let _ = ShowWindow(h, SW_SHOW);
                    let _ = SetForegroundWindow(h);
                }
                Err(e) => tracing::warn!("history window: {e}"),
            }
            hide(self.hwnd);
        }
    }
}
impl Drop for QuotaPopover {
    fn drop(&mut self) {
        unsafe {
            for h in [self.hwnd, self.details] {
                if IsWindow(h).as_bool() {
                    let _ = DestroyWindow(h);
                }
            }
        }
    }
}
unsafe fn create(
    proxy: EventLoopProxy<UserEvent>,
    view: View,
    context: PanelContext,
    anchor: RECT,
    detail: Option<ChartKind>,
) -> anyhow::Result<HWND> {
    let instance = GetModuleHandleW(None)?;
    let class = WNDCLASSW {
        lpfnWndProc: Some(proc),
        hInstance: instance.into(),
        lpszClassName: w!("QuotaBarCompletePanel"),
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        style: CS_DROPSHADOW,
        ..Default::default()
    };
    RegisterClassW(&class);
    let styles = if detail.is_some() {
        WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN
    } else {
        WS_POPUP | WS_CLIPCHILDREN | WS_VSCROLL
    };
    let hwnd = CreateWindowExW(
        WS_EX_CONTROLPARENT
            | if detail.is_some() {
                WINDOW_EX_STYLE(0)
            } else {
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST
            },
        class.lpszClassName,
        w!("QuotaBar"),
        styles,
        anchor.left,
        anchor.top,
        400,
        600,
        None,
        None,
        instance,
        None,
    )?;
    let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.;
    if detail.is_some() {
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(
            MonitorFromRect(&anchor, MONITOR_DEFAULTTONEAREST),
            &mut info,
        );
        let width = (640. * scale).round() as i32;
        let height = (500. * scale).round() as i32;
        let width = width.min(info.rcWork.right - info.rcWork.left);
        let height = height.min(info.rcWork.bottom - info.rcWork.top);
        let r = placement(anchor, info.rcWork, width, height, (8. * scale) as i32);
        let _ = SetWindowPos(
            hwnd,
            None,
            r.left,
            r.top,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }

    let light = crate::platform::apps_use_light_theme();
    let state = State {
        view,
        context,
        scale,
        light,
        resources: Resources::new(scale, light),
        proxy,
        anchor,
        dismissed_by_entry: false,
        scroll: 0.,
        detail,
        days: if detail == Some(ChartKind::Tokens) {
            30
        } else {
            0
        },
        hover: None,
        charts: Rc::new(RefCell::new([None, None])),
    };
    SetWindowLongPtrW(
        hwnd,
        GWLP_USERDATA,
        Box::into_raw(Box::new(RefCell::new(state))) as isize,
    );
    let controls = if detail.is_some() {
        vec![
            (CYCLE, "Current cycle"),
            (SEVEN, "7 days"),
            (THIRTY, "30 days"),
        ]
    } else {
        vec![
            (SETTINGS, "Settings"),
            (QUIT, "Quit"),
            (REFRESH, "Refresh"),
            (HISTORY, "Quota history"),
            (TOKENS, "Token activity"),
        ]
    };
    for (id, text) in controls {
        let text = wide(t(text));
        let _ = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("BUTTON"),
            PCWSTR(text.as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
            0,
            0,
            1,
            1,
            hwnd,
            HMENU(id as _),
            instance,
            None,
        )?;
    }
    Ok(hwnd)
}
unsafe fn hide(hwnd: HWND) {
    let _ = KillTimer(hwnd, 1);
    let _ = ShowWindow(hwnd, SW_HIDE);
    ACTIVE.with(|a| {
        if a.get() == hwnd {
            a.set(HWND::default())
        }
    });
}
#[allow(clippy::too_many_arguments)] // Coordinates mirror native control placement.
unsafe fn button(
    hwnd: HWND,
    id: usize,
    s: &State,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    visible: bool,
) {
    if let Ok(h) = GetDlgItem(hwnd, id as i32) {
        let _ = ShowWindow(h, if visible { SW_SHOW } else { SW_HIDE });
        if visible {
            let r = rect(s.scale, x, y, width, height);
            let _ = MoveWindow(h, r.left, r.top, r.right - r.left, r.bottom - r.top, true);
        }
    }
}
unsafe fn refresh(hwnd: HWND) {
    let Some(mut s) = snapshot(hwnd) else {
        return;
    };
    for (id, text) in [
        (SETTINGS, "Settings"),
        (QUIT, "Quit"),
        (REFRESH, "Refresh"),
        (HISTORY, "Quota history"),
        (TOKENS, "Token activity"),
        (CYCLE, "Current cycle"),
        (SEVEN, "7 days"),
        (THIRTY, "30 days"),
    ] {
        if let Ok(child) = GetDlgItem(hwnd, id as i32) {
            let mut current = [0u16; 128];
            let n = GetWindowTextW(child, &mut current);
            let translated = t(text);
            if String::from_utf16_lossy(&current[..n.max(0) as usize]) != translated {
                let text = wide(translated);
                let _ = SetWindowTextW(child, PCWSTR(text.as_ptr()));
            }
        }
    }
    let light = crate::platform::apps_use_light_theme();
    if s.light != light {
        s.light = light;
        s.resources = Resources::new(s.scale, light);
        change(hwnd, |st| {
            st.light = light;
            st.resources = s.resources.clone();
        });
    }
    let dark = BOOL::from(!light);
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        &dark as *const _ as _,
        std::mem::size_of_val(&dark) as u32,
    );
    let corner = DWMWCP_ROUND;
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        &corner as *const _ as _,
        std::mem::size_of_val(&corner) as u32,
    );
    if s.detail.is_none() {
        let monitor = MonitorFromRect(&s.anchor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let h = ((HEADER + content_height(&s.context) + FOOTER) * s.scale).round() as i32;
        let height = h.min(info.rcWork.bottom - info.rcWork.top - 16).max(160);
        let width = (WIDTH * s.scale).round() as i32;
        let r = placement(s.anchor, info.rcWork, width, height, (6. * s.scale) as i32);
        let _ = SetWindowPos(
            hwnd,
            None,
            r.left,
            r.top,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
        if snapshot(hwnd).is_some_and(|current| current.scale != s.scale) {
            return;
        }
    }
    let mut r = RECT::default();
    let _ = GetClientRect(hwnd, &mut r);
    let height = r.bottom as f32 / s.scale;
    if s.detail.is_some() {
        button(
            hwnd,
            CYCLE,
            &s,
            20.,
            70.,
            145.,
            30.,
            s.detail == Some(ChartKind::Quota),
        );
        button(hwnd, SEVEN, &s, 175., 70., 90., 30., true);
        button(hwnd, THIRTY, &s, 275., 70., 100., 30., true);
    } else {
        s.scroll = s.scroll.clamp(
            0.,
            (content_height(&s.context) - (height - HEADER - FOOTER)).max(0.),
        );
        change(hwnd, |st| st.scroll = s.scroll);
        let si = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: content_height(&s.context).ceil() as i32 - 1,
            nPage: (height - HEADER - FOOTER).max(1.) as u32,
            nPos: s.scroll as i32,
            ..Default::default()
        };
        SetScrollInfo(hwnd, SB_VERT, &si, true);
        let _ = GetClientRect(hwnd, &mut r);
        let width = r.right as f32 / s.scale;
        button(
            hwnd,
            SETTINGS,
            &s,
            16.,
            height - FOOTER + 10.,
            120.,
            32.,
            true,
        );
        button(hwnd, QUIT, &s, width - 76., height - 39., 60., 26., true);
        button(hwnd, REFRESH, &s, width - 94., 22., 78., 32., true);
        for (id, module) in [
            (HISTORY, ModuleId::QuotaHistory),
            (TOKENS, ModuleId::TokenActivity),
        ] {
            let pos = sections(&s.context)
                .into_iter()
                .find(|(m, _, _)| *m == module)
                .map(|(_, y, _)| HEADER + y - s.scroll + 12.);
            let y = pos.unwrap_or(0.);
            button(
                hwnd,
                id,
                &s,
                16.,
                y,
                185.,
                28.,
                pos.is_some() && y >= HEADER && y + 28. <= height - FOOTER,
            );
        }
    }
    let _ = RedrawWindow(hwnd, None, None, RDW_INVALIDATE | RDW_ALLCHILDREN);
}
pub(crate) fn handle_message(raw: *const std::ffi::c_void) -> bool {
    if raw.is_null() {
        return false;
    }
    let message = unsafe { &*(raw as *const MSG) };
    if !(WM_KEYFIRST..=WM_KEYLAST).contains(&message.message) {
        return false;
    }
    [ACTIVE.with(Cell::get), DETAIL.with(Cell::get)]
        .into_iter()
        .any(|hwnd| unsafe {
            if hwnd.is_invalid()
                || !IsWindowVisible(hwnd).as_bool()
                || (message.hwnd != hwnd && !IsChild(hwnd, message.hwnd).as_bool())
            {
                return false;
            }
            if message.message == WM_KEYDOWN && message.wParam.0 == 27 {
                if snapshot(hwnd).is_some_and(|s| s.detail.is_some()) {
                    let _ = DestroyWindow(hwnd);
                } else {
                    hide(hwnd);
                }
                return true;
            }
            IsDialogMessageW(hwnd, message).as_bool()
        })
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCDESTROY {
        let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RefCell<State>;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        if !p.is_null() {
            drop(Box::from_raw(p));
        }
        ACTIVE.with(|a| {
            if a.get() == hwnd {
                a.set(HWND::default())
            }
        });
        DETAIL.with(|a| {
            if a.get() == hwnd {
                a.set(HWND::default())
            }
        });
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let Some(s) = snapshot(hwnd) else {
        return DefWindowProcW(hwnd, msg, wp, lp);
    };
    match msg {
        WM_CLOSE => {
            if s.detail.is_some() {
                let _ = DestroyWindow(hwnd);
            } else {
                hide(hwnd);
            }
            LRESULT(0)
        }
        WM_ACTIVATE if wp.0 & 0xffff == WA_INACTIVE as usize && s.detail.is_none() => {
            let mut p = POINT::default();
            let _ = GetCursorPos(&mut p);
            if p.x >= s.anchor.left
                && p.x < s.anchor.right
                && p.y >= s.anchor.top
                && p.y < s.anchor.bottom
                && GetAsyncKeyState(1) < 0
            {
                change(hwnd, |st| st.dismissed_by_entry = true);
            }
            hide(hwnd);
            LRESULT(0)
        }
        WM_COMMAND => {
            if wp.0 >> 16 == BN_CLICKED as usize {
                match wp.0 & 0xffff {
                    SETTINGS => {
                        hide(hwnd);
                        let _ = s.proxy.send_event(UserEvent::OpenQuotaSettings);
                    }
                    QUIT => {
                        let _ = s
                            .proxy
                            .send_event(UserEvent::Command(crate::app::AppCommand::Quit));
                    }
                    REFRESH => {
                        let _ = s.proxy.send_event(UserEvent::RefreshQuota(true));
                    }
                    HISTORY | TOKENS => {
                        let _ = s.proxy.send_event(UserEvent::OpenHistory(
                            if wp.0 & 0xffff == HISTORY {
                                ChartKind::Quota
                            } else {
                                ChartKind::Tokens
                            },
                        ));
                    }
                    CYCLE | SEVEN | THIRTY => {
                        change(hwnd, |st| {
                            st.days = match wp.0 & 0xffff {
                                SEVEN => 7,
                                THIRTY => 30,
                                _ => 0,
                            };
                            st.hover = None;
                        });
                        refresh(hwnd);
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL | WM_VSCROLL if s.detail.is_none() => {
            let mut r = RECT::default();
            let _ = GetClientRect(hwnd, &mut r);
            let page = (r.bottom as f32 / s.scale - HEADER - FOOTER).max(1.);
            let amount = if msg == WM_MOUSEWHEEL {
                -((wp.0 >> 16) as u16 as i16 as f32) / 120. * 48.
            } else {
                match (wp.0 & 0xffff) as u32 {
                    0 => -32.,
                    1 => 32.,
                    2 => -page,
                    3 => page,
                    4 | 5 => {
                        let mut si = SCROLLINFO {
                            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                            fMask: SIF_TRACKPOS,
                            ..Default::default()
                        };
                        let _ = GetScrollInfo(hwnd, SB_VERT, &mut si);
                        si.nTrackPos as f32 - s.scroll
                    }
                    6 => -s.scroll,
                    7 => content_height(&s.context),
                    _ => 0.,
                }
            };
            change(hwnd, |st| {
                st.scroll =
                    (st.scroll + amount).clamp(0., (content_height(&st.context) - page).max(0.))
            });
            refresh(hwnd);
            LRESULT(0)
        }
        WM_MOUSEMOVE if s.detail.is_some() => {
            let x = (lp.0 & 0xffff) as u16 as i16 as f32 / s.scale;
            let y = ((lp.0 >> 16) & 0xffff) as u16 as i16 as f32 / s.scale;
            let hover = if y >= 120. { Some(x) } else { None };
            if s.hover != hover {
                change(hwnd, |st| st.hover = hover);
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP if s.detail.is_none() => {
            let y = ((lp.0 >> 16) & 0xffff) as u16 as i16 as f32 / s.scale;
            let mut r = RECT::default();
            let _ = GetClientRect(hwnd, &mut r);
            if y >= HEADER && y < r.bottom as f32 / s.scale - FOOTER {
                for (id, top, h) in sections(&s.context) {
                    if y >= HEADER + top - s.scroll && y < HEADER + top + h - s.scroll {
                        let kind = match id {
                            ModuleId::QuotaHistory => Some(ChartKind::Quota),
                            ModuleId::TokenActivity => Some(ChartKind::Tokens),
                            _ => None,
                        };
                        if let Some(k) = kind {
                            let _ = s.proxy.send_event(UserEvent::OpenHistory(k));
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_TIMER | WM_THEMECHANGED | WM_SETTINGCHANGE => {
            if IsWindowVisible(hwnd).as_bool() {
                refresh(hwnd);
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let scale = (wp.0 & 0xffff) as f32 / 96.;
            change(hwnd, |st| {
                st.scale = scale;
                st.resources = Resources::new(scale, st.light);
            });
            if s.detail.is_some() {
                let r = *(lp.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            refresh(hwnd);
            LRESULT(0)
        }
        WM_GETMINMAXINFO if s.detail.is_some() => {
            let info = &mut *(lp.0 as *mut MINMAXINFO);
            info.ptMinTrackSize = POINT {
                x: (430. * s.scale) as i32,
                y: (350. * s.scale) as i32,
            };
            LRESULT(0)
        }
        WM_SIZE if s.detail.is_some() => {
            refresh(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_DRAWITEM => {
            let d = &*(lp.0 as *const DRAWITEMSTRUCT);
            let theme = SurfaceTheme::new(s.light);
            FillRect(d.hDC, &d.rcItem, s.resources.background);
            let selected = (d.CtlID as usize == CYCLE && s.days == 0)
                || (d.CtlID as usize == SEVEN && s.days == 7)
                || (d.CtlID as usize == THIRTY && s.days == 30);
            fill_rounded(
                d.hDC,
                d.rcItem,
                if selected {
                    theme.secondary
                } else {
                    theme.background
                },
                6. * s.scale,
            );
            let mut text = [0u16; 100];
            let n = GetWindowTextW(d.hwndItem, &mut text);
            let chart_button = [HISTORY, TOKENS, SETTINGS].contains(&(d.CtlID as usize));
            let mut label_rect = d.rcItem;
            if chart_button {
                label_rect.left += (8. * s.scale) as i32;
            }
            draw_label(
                d.hDC,
                &String::from_utf16_lossy(&text[..n.max(0) as usize]),
                label_rect,
                s.resources.bold,
                theme.text,
                if chart_button { DT_LEFT } else { DT_CENTER },
            );
            if d.itemState.0 & 0x10 != 0 {
                let _ = DrawFocusRect(d.hDC, &d.rcItem);
            }
            LRESULT(1)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            paint_window(hwnd, dc, &s);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_PRINTCLIENT => {
            paint_window(hwnd, HDC(wp.0 as _), &s);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
struct Painter<'a> {
    dc: HDC,
    s: &'a State,
    width: f32,
}
impl Painter<'_> {
    #[allow(clippy::too_many_arguments)] // A text run includes geometry, font, ink and alignment.
    unsafe fn text(
        &self,
        text: &str,
        x: f32,
        y: f32,
        width: f32,
        font: HFONT,
        ink: Color,
        align: DRAW_TEXT_FORMAT,
    ) {
        draw_label(
            self.dc,
            text,
            rect(self.s.scale, x, y, width, 24.),
            font,
            ink,
            align,
        );
    }
    unsafe fn caption(&self, text: &str, y: f32) {
        self.text(
            text,
            24.,
            y,
            self.width - 48.,
            self.s.resources.caption,
            SurfaceTheme::new(self.s.light).text_secondary,
            DT_LEFT,
        );
    }
    unsafe fn pair(&self, label: &str, value: &str, y: f32, ink: Color) {
        self.text(
            label,
            24.,
            y,
            self.width - 145.,
            self.s.resources.body,
            ink,
            DT_LEFT,
        );
        self.text(
            value,
            self.width - 122.,
            y,
            98.,
            self.s.resources.bold,
            ink,
            DT_RIGHT,
        );
    }
    unsafe fn bar(&self, y: f32, value: Option<f32>, color: Color) {
        fill_rounded(
            self.dc,
            rect(self.s.scale, 24., y, self.width - 48., 8.),
            SurfaceTheme::new(self.s.light).secondary,
            4. * self.s.scale,
        );
        if let Some(v) = value {
            if v > 0. {
                fill_rounded(
                    self.dc,
                    rect(
                        self.s.scale,
                        24.,
                        y,
                        (self.width - 48.) * v.clamp(0., 100.) / 100.,
                        8.,
                    ),
                    color,
                    4. * self.s.scale,
                );
            }
        }
    }
    unsafe fn line(&self, a: (f32, f32), b: (f32, f32), color: Color, dashed: bool) {
        SetBkMode(self.dc, TRANSPARENT);
        let pen = CreatePen(
            if dashed { PS_DASH } else { PS_SOLID },
            if dashed {
                1
            } else {
                (1.5 * self.s.scale).round() as i32
            },
            native_color(color),
        );
        let old = SelectObject(self.dc, pen);
        let _ = MoveToEx(
            self.dc,
            (a.0 * self.s.scale) as i32,
            (a.1 * self.s.scale) as i32,
            None,
        );
        let _ = LineTo(
            self.dc,
            (b.0 * self.s.scale) as i32,
            (b.1 * self.s.scale) as i32,
        );
        SelectObject(self.dc, old);
        let _ = DeleteObject(pen);
    }
    unsafe fn separator(&self, y: f32) {
        self.line(
            (24., y),
            (self.width - 24., y),
            SurfaceTheme::new(self.s.light).secondary,
            false,
        );
    }
}
fn percent(p: Option<f32>) -> String {
    p.map(|v| format!("{v:.0}%")).unwrap_or_else(|| "—".into())
}
fn short(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1e9)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1000 {
        format!("{:.1}K", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}
unsafe fn paint_window(hwnd: HWND, dc: HDC, s: &State) {
    let mut client = RECT::default();
    if GetClientRect(hwnd, &mut client).is_err() || client.right <= 0 || client.bottom <= 0 {
        return;
    }
    let mem = CreateCompatibleDC(dc);
    let bitmap = CreateCompatibleBitmap(dc, client.right, client.bottom);
    if mem.is_invalid() || bitmap.is_invalid() {
        if !mem.is_invalid() {
            let _ = DeleteDC(mem);
        }
        if !bitmap.is_invalid() {
            let _ = DeleteObject(bitmap);
        }
        return;
    }
    let old = SelectObject(mem, bitmap);
    FillRect(mem, &client, s.resources.background);
    let p = Painter {
        dc: mem,
        s,
        width: client.right as f32 / s.scale,
    };
    let height = client.bottom as f32 / s.scale;
    let theme = SurfaceTheme::new(s.light);
    let now = Utc::now();
    if let Some(kind) = s.detail {
        p.text(
            t(if kind == ChartKind::Quota {
                "Quota history"
            } else {
                "Token activity"
            }),
            24.,
            20.,
            p.width - 48.,
            s.resources.title,
            theme.text,
            DT_LEFT,
        );
        let note = if kind == ChartKind::Quota {
            "Observed data · gaps are not interpolated"
        } else {
            "Missing dates are shown as gaps"
        };
        p.caption(t(note), 45.);
        if kind == ChartKind::Tokens {
            let values =
                meter::history::daily_values(&s.context.history, now.date_naive(), s.days.max(1));
            let total = values
                .iter()
                .filter_map(|(_, v)| *v)
                .fold(0u64, u64::saturating_add);
            let known = values.iter().filter(|(_, v)| v.is_some()).count();
            p.text(
                &format!(
                    "{}: {} ({known}/{})",
                    t("Reported total"),
                    if known == 0 {
                        "—".into()
                    } else {
                        short(total)
                    },
                    values.len()
                ),
                24.,
                height - 78.,
                p.width - 48.,
                s.resources.caption,
                theme.text_secondary,
                DT_LEFT,
            );
        }
        let hover = chart(&p, kind, 125., (height - 240.).max(50.), s.days, now);
        p.caption(
            &hover.unwrap_or_else(|| t("Point to a sample to inspect its date and value").into()),
            height - 52.,
        );
    } else {
        let saved = SaveDC(mem);
        let _ = IntersectClipRect(
            mem,
            0,
            (HEADER * s.scale) as i32,
            client.right,
            ((height - FOOTER) * s.scale) as i32,
        );
        let rows = sections(&s.context);
        if rows.is_empty() {
            p.caption(
                t("All modules are hidden. Change this in Settings."),
                HEADER + 32.,
            );
        }
        for (id, top, h) in rows {
            let y = HEADER + top - s.scroll;
            if y + h < HEADER || y > height - FOOTER {
                continue;
            }
            match id {
                ModuleId::FiveHour | ModuleId::Weekly => {
                    let index = usize::from(id == ModuleId::Weekly);
                    let q = &s.view.quotas[index];
                    let time = meter::time_remaining(q.reset_at, s.view.seconds[index], now);
                    let pace = meter::pace(
                        q.remaining,
                        time,
                        s.view.live[index] && !q.muted() && q.reset_at.is_some_and(|r| r > now),
                    );
                    let state = match pace {
                        meter::Pace::OnPace => format!("✓ {}", t("On pace")),
                        meter::Pace::AbovePace => format!("! {}", t("Above pace")),
                        _ => format!(
                            "— {}",
                            t(if q.muted() {
                                q.status
                            } else {
                                "Pace unavailable"
                            })
                        ),
                    };
                    let color = if q.muted() {
                        theme.text_secondary
                    } else if q
                        .remaining
                        .is_some_and(|r| r <= 100. - s.context.threshold as f32)
                    {
                        Color::rgba(225, 106, 101, 255)
                    } else if pace == meter::Pace::AbovePace {
                        Color::rgba(233, 185, 73, 255)
                    } else {
                        theme.text
                    };
                    p.text(
                        t(id.title()),
                        24.,
                        y + 10.,
                        155.,
                        s.resources.bold,
                        theme.text,
                        DT_LEFT,
                    );
                    p.text(
                        &state,
                        178.,
                        y + 10.,
                        p.width - 202.,
                        s.resources.body,
                        if pace == meter::Pace::OnPace {
                            Color::rgba(67, 174, 131, 255)
                        } else {
                            color
                        },
                        DT_RIGHT,
                    );
                    p.pair(t("Quota remaining"), &q.label(), y + 42., theme.text);
                    p.bar(y + 70., q.remaining, color);
                    p.pair(t("Time remaining"), &percent(time), y + 85., theme.text);
                    p.bar(y + 113., time, Color::rgba(101, 174, 245, 255));
                    let countdown = q
                        .reset_at
                        .map(|r| {
                            if r <= now {
                                t("Waiting for update").into()
                            } else {
                                format!(
                                    "{} {}",
                                    t("Resets in"),
                                    super::text::format_duration((r - now).num_seconds())
                                )
                            }
                        })
                        .unwrap_or_else(|| t("Reset time unavailable").into());
                    p.text(
                        &countdown,
                        24.,
                        y + 130.,
                        175.,
                        s.resources.caption,
                        theme.text_secondary,
                        DT_LEFT,
                    );
                    let reset = q
                        .reset_at
                        .map(|r| r.with_timezone(&Local).format("%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "—".into());
                    p.text(
                        &reset,
                        205.,
                        y + 130.,
                        p.width - 229.,
                        s.resources.caption,
                        theme.text_secondary,
                        DT_RIGHT,
                    );
                }
                ModuleId::ResetCredits => {
                    p.text(
                        t(id.title()),
                        24.,
                        y + 12.,
                        220.,
                        s.resources.bold,
                        theme.text,
                        DT_LEFT,
                    );
                    if let Availability::Ready(c) = &s.context.extras.credits {
                        p.text(
                            &format!("{} {}", c.count, t("available")),
                            235.,
                            y + 12.,
                            p.width - 259.,
                            s.resources.bold,
                            theme.text,
                            DT_RIGHT,
                        );
                        let note = if c.count == 0 {
                            "No reset opportunities available"
                        } else if c.items.as_ref().is_none_or(Vec::is_empty) {
                            "Only the available count was returned"
                        } else {
                            "Expiry of each available opportunity"
                        };
                        p.caption(t(note), y + 39.);
                        if let Some(items) = &c.items {
                            for (i, credit) in items.iter().enumerate() {
                                let row = y + 70. + 47. * i as f32;
                                p.bar(row, credit.remaining(now), theme.text);
                                let expiry = credit
                                    .expires
                                    .map(|r| {
                                        format!(
                                            "{} {}",
                                            t("Expires"),
                                            r.with_timezone(&Local).format("%Y-%m-%d %H:%M")
                                        )
                                    })
                                    .unwrap_or_else(|| t("Expiry unavailable").into());
                                p.caption(&expiry, row + 11.);
                            }
                        }
                    } else {
                        p.caption(t(message(&s.context.extras.credits)), y + 43.);
                    }
                }
                ModuleId::QuotaHistory | ModuleId::TokenActivity => {
                    let kind = if id == ModuleId::QuotaHistory {
                        ChartKind::Quota
                    } else {
                        ChartKind::Tokens
                    };
                    p.text(
                        t(id.title()),
                        24.,
                        y + 14.,
                        180.,
                        s.resources.bold,
                        theme.text,
                        DT_LEFT,
                    );
                    let summary = if kind == ChartKind::Quota {
                        percent(s.view.quotas[1].remaining)
                    } else {
                        let values =
                            meter::history::daily_values(&s.context.history, now.date_naive(), 30);
                        let n = values
                            .iter()
                            .filter_map(|(_, v)| *v)
                            .fold(0u64, u64::saturating_add);
                        if values.iter().any(|(_, v)| v.is_some()) {
                            short(n)
                        } else {
                            "—".into()
                        }
                    };
                    p.text(
                        &format!("{summary}  ›"),
                        225.,
                        y + 14.,
                        p.width - 249.,
                        s.resources.bold,
                        theme.text,
                        DT_RIGHT,
                    );
                    p.caption(
                        t(if kind == ChartKind::Quota {
                            "Current weekly cycle"
                        } else {
                            "30 days · reported total"
                        }),
                        y + 39.,
                    );
                    chart(
                        &p,
                        kind,
                        y + 72.,
                        72.,
                        if kind == ChartKind::Quota { 0 } else { 30 },
                        now,
                    );
                }
            }
            p.separator(y + h - 1.);
        }
        let _ = RestoreDC(mem, saved);
        p.text(
            "QuotaBar",
            24.,
            13.,
            240.,
            s.resources.title,
            theme.text,
            DT_LEFT,
        );
        p.caption(&s.view.plan, 43.);
        p.separator(HEADER - 1.);
        p.separator(height - FOOTER);
        let updated = s
            .view
            .updated
            .map(|d| {
                format!(
                    "{} {}",
                    t("Last updated"),
                    d.with_timezone(&Local).format("%H:%M")
                )
            })
            .unwrap_or_else(|| t("Not updated yet").into());
        p.caption(&updated, height - 39.);
    }
    let _ = BitBlt(dc, 0, 0, client.right, client.bottom, mem, 0, 0, SRCCOPY);
    SelectObject(mem, old);
    let _ = DeleteObject(bitmap);
    let _ = DeleteDC(mem);
}
fn quota_domain(s: &State, days: u32, now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    if days == 0 {
        if let Some(reset) = s.view.quotas[1].reset_at {
            if let Some(seconds) = s.view.seconds[1] {
                if let Ok(seconds) = i64::try_from(seconds) {
                    return (reset - Duration::seconds(seconds), reset);
                }
            }
        }
    }
    (
        now - Duration::days(if days == 0 { 7 } else { days as i64 }),
        now,
    )
}
#[derive(Clone, PartialEq)]
struct ChartKey {
    history: Rc<AccountHistory>,
    tokens: Availability<std::collections::BTreeMap<chrono::NaiveDate, u64>>,
    days: u32,
    day: chrono::NaiveDate,
    light: bool,
    chinese: bool,
    scale: u32,
    width: i32,
    height: i32,
    reset: Option<DateTime<Utc>>,
    seconds: Option<u64>,
    interval: u64,
}
struct ChartImage {
    key: ChartKey,
    bitmap: HBITMAP,
    at: DateTime<Utc>,
}
impl Drop for ChartImage {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.bitmap);
        }
    }
}
unsafe fn chart(
    p: &Painter<'_>,
    kind: ChartKind,
    top: f32,
    height: f32,
    days: u32,
    now: DateTime<Utc>,
) -> Option<String> {
    let slot = usize::from(kind == ChartKind::Tokens);
    let key = ChartKey {
        history: p.s.context.history.clone(),
        tokens: p.s.context.extras.tokens.clone(),
        days,
        day: now.date_naive(),
        light: p.s.light,
        chinese: crate::i18n::is_chinese(),
        scale: p.s.scale.to_bits(),
        width: (p.width * p.s.scale).round() as i32,
        height: ((height + 26.) * p.s.scale).ceil() as i32,
        reset: p.s.view.quotas[1].reset_at,
        seconds: p.s.view.seconds[1],
        interval: p.s.context.interval,
    };
    if key.width <= 0 || key.height <= 0 {
        return None;
    }
    let cached = p.s.charts.borrow()[slot].clone();
    let chart = if let Some(c) = cached.filter(|c| c.key == key) {
        c
    } else {
        let dc = CreateCompatibleDC(p.dc);
        let bitmap = CreateCompatibleBitmap(p.dc, key.width, key.height);
        if dc.is_invalid() || bitmap.is_invalid() {
            if !dc.is_invalid() {
                let _ = DeleteDC(dc);
            }
            if !bitmap.is_invalid() {
                let _ = DeleteObject(bitmap);
            }
            return None;
        }
        let old = SelectObject(dc, bitmap);
        FillRect(
            dc,
            &RECT {
                left: 0,
                top: 0,
                right: key.width,
                bottom: key.height,
            },
            p.s.resources.background,
        );
        let mut state = p.s.clone();
        state.hover = None;
        let painter = Painter {
            dc,
            s: &state,
            width: p.width,
        };
        let _ = render_chart(&painter, kind, 0., height, days, now);
        SelectObject(dc, old);
        let _ = DeleteDC(dc);
        let c = Rc::new(ChartImage {
            key,
            bitmap,
            at: now,
        });
        p.s.charts.borrow_mut()[slot] = Some(c.clone());
        c
    };
    let dc = CreateCompatibleDC(p.dc);
    let old = SelectObject(dc, chart.bitmap);
    let _ = BitBlt(
        p.dc,
        0,
        (top * p.s.scale).round() as i32,
        chart.key.width,
        chart.key.height,
        dc,
        0,
        0,
        SRCCOPY,
    );
    SelectObject(dc, old);
    let _ = DeleteDC(dc);
    p.s.detail?;
    let hx = p.s.hover?;
    if !(24. ..p.width - 24.).contains(&hx) {
        return None;
    }
    if kind == ChartKind::Tokens {
        let days = days.max(1);
        let index = ((hx - 24.) / (p.width - 48.) * days as f32) as usize;
        let values =
            meter::history::daily_values(&p.s.context.history, chart.at.date_naive(), days);
        let (date, value) = values.get(index)?;
        Some(format!(
            "{date}   {}",
            value
                .map(|v| format!("{v} tokens"))
                .unwrap_or_else(|| t("Missing data").into())
        ))
    } else {
        let (start, end) = quota_domain(p.s, days, chart.at);
        let x = |at: DateTime<Utc>| {
            24. + ((at - start).num_seconds() as f32 / (end - start).num_seconds().max(1) as f32)
                * (p.width - 48.)
        };
        let point =
            p.s.context
                .history
                .quota
                .iter()
                .filter(|v| {
                    v.window == MetricWindow::Long
                        && v.at >= start
                        && v.at <= end
                        && (days != 0 || v.reset == p.s.view.quotas[1].reset_at)
                })
                .min_by(|a, b| (x(a.at) - hx).abs().total_cmp(&(x(b.at) - hx).abs()))?;
        p.line(
            (x(point.at), top),
            (x(point.at), top + height),
            SurfaceTheme::new(p.s.light).text_secondary,
            true,
        );
        Some(format!(
            "{}   {:.0}% {}",
            point.at.with_timezone(&Local).format("%Y-%m-%d %H:%M"),
            point.remaining,
            t("remaining")
        ))
    }
}
unsafe fn render_chart(
    p: &Painter<'_>,
    kind: ChartKind,
    top: f32,
    height: f32,
    days: u32,
    now: DateTime<Utc>,
) -> Option<String> {
    let theme = SurfaceTheme::new(p.s.light);
    let color = Color::rgba(100, 189, 208, 255);
    let width = p.width - 48.;
    let bottom = top + height;
    let detail = p.s.detail.is_some();
    p.line(
        (24., bottom),
        (p.width - 24., bottom),
        theme.secondary,
        false,
    );
    if kind == ChartKind::Quota {
        let (start, end) = quota_domain(p.s, days, now);
        let points: Vec<&Observation> =
            p.s.context
                .history
                .quota
                .iter()
                .filter(|v| {
                    v.window == MetricWindow::Long
                        && v.at >= start
                        && v.at <= end
                        && (days != 0 || v.reset == p.s.view.quotas[1].reset_at)
                })
                .collect();
        let x = |at: DateTime<Utc>| {
            24. + ((at - start).num_seconds() as f32 / (end - start).num_seconds().max(1) as f32)
                * width
        };
        let y = |v: f32| bottom - v.clamp(0., 100.) / 100. * height;
        if days == 0 && p.s.view.seconds[1].is_some() && p.s.view.quotas[1].reset_at.is_some() {
            p.line(
                (24., top),
                (p.width - 24., bottom),
                Color::rgba(82, 134, 175, 255),
                true,
            );
        }
        if points.is_empty() {
            p.caption(t("No observations in this period"), top + height / 2. - 12.);
        } else {
            for (i, point) in points.iter().enumerate() {
                if i > 0 && meter::history::connects(points[i - 1], point, p.s.context.interval) {
                    p.line(
                        (x(points[i - 1].at), y(points[i - 1].remaining)),
                        (x(point.at), y(point.remaining)),
                        color,
                        false,
                    );
                }
                fill_rounded(
                    p.dc,
                    rect(p.s.scale, x(point.at) - 2., y(point.remaining) - 2., 4., 4.),
                    color,
                    2. * p.s.scale,
                );
            }
        }
        p.text(
            &start.with_timezone(&Local).format("%m-%d").to_string(),
            24.,
            bottom + 2.,
            90.,
            p.s.resources.caption,
            theme.text_secondary,
            DT_LEFT,
        );
        p.text(
            &end.with_timezone(&Local).format("%m-%d").to_string(),
            p.width - 114.,
            bottom + 2.,
            90.,
            p.s.resources.caption,
            theme.text_secondary,
            DT_RIGHT,
        );
        if detail {
            if let Some(hx) = p.s.hover {
                if (24. ..=p.width - 24.).contains(&hx) {
                    if let Some(point) = points
                        .iter()
                        .min_by(|a, b| (x(a.at) - hx).abs().total_cmp(&(x(b.at) - hx).abs()))
                    {
                        p.line(
                            (x(point.at), top),
                            (x(point.at), bottom),
                            theme.text_secondary,
                            true,
                        );
                        return Some(format!(
                            "{}   {:.0}% {}",
                            point.at.with_timezone(&Local).format("%Y-%m-%d %H:%M"),
                            point.remaining,
                            t("remaining")
                        ));
                    }
                }
            }
        }
    } else {
        let values =
            meter::history::daily_values(&p.s.context.history, now.date_naive(), days.max(1));
        let max = values
            .iter()
            .filter_map(|(_, v)| *v)
            .max()
            .unwrap_or(1)
            .max(1);
        let step = width / values.len() as f32;
        let unavailable = message(&p.s.context.extras.tokens);
        if values.iter().all(|(_, v)| v.is_none()) {
            p.caption(
                t(if unavailable.is_empty() {
                    "No token observations in this period"
                } else {
                    unavailable
                }),
                top + height / 2. - 12.,
            );
        }
        for (i, (_, v)) in values.iter().enumerate() {
            let x = 24. + step * i as f32 + 1.;
            match v {
                Some(n) => {
                    let h = (*n as f64 / max as f64 * height as f64) as f32;
                    fill_rounded(
                        p.dc,
                        rect(
                            p.s.scale,
                            x,
                            bottom - h.max(2.),
                            (step - 3.).max(1.),
                            h.max(2.),
                        ),
                        if *n == 0 { theme.text_secondary } else { color },
                        2. * p.s.scale,
                    );
                }
                None => {
                    p.line(
                        (x, bottom - 1.),
                        (x + (step - 3.).max(1.), bottom - 1.),
                        theme.text_secondary,
                        true,
                    );
                }
            }
        }
        if !unavailable.is_empty() && values.iter().any(|(_, v)| v.is_some()) {
            p.text(
                t(unavailable),
                24.,
                top,
                width,
                p.s.resources.caption,
                theme.text_secondary,
                DT_LEFT,
            );
        }
        if let (Some(first), Some(last)) = (values.first(), values.last()) {
            p.text(
                &first.0.format("%m-%d").to_string(),
                24.,
                bottom + 2.,
                90.,
                p.s.resources.caption,
                theme.text_secondary,
                DT_LEFT,
            );
            p.text(
                &last.0.format("%m-%d").to_string(),
                p.width - 114.,
                bottom + 2.,
                90.,
                p.s.resources.caption,
                theme.text_secondary,
                DT_RIGHT,
            );
        }
        if detail {
            if let Some(hx) = p.s.hover {
                if (24. ..p.width - 24.).contains(&hx) {
                    let index = ((hx - 24.) / step) as usize;
                    if let Some((date, value)) = values.get(index) {
                        return Some(format!(
                            "{date}   {}",
                            value
                                .map(|v| format!("{v} tokens"))
                                .unwrap_or_else(|| t("Missing data").into())
                        ));
                    }
                }
            }
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "isolated hidden Win32 panel and charts; no credentials, network or foreground window"]
    fn native_complete_panel_hidden_render_and_scroll() {
        use crate::meter::extras::{Credit, Credits};
        use tao::{event_loop::EventLoopBuilder, platform::windows::EventLoopBuilderExtWindows};
        let event_loop = EventLoopBuilder::<UserEvent>::with_user_event()
            .with_any_thread(true)
            .build();
        crate::i18n::set_language(crate::config::schema::Language::Chinese);
        let now = Utc::now();
        let reset = DateTime::from_timestamp((now + Duration::days(6)).timestamp(), 0).unwrap();
        let mut data=crate::providers::ProviderData{
            account_key:Some("fixture".into()),plan_type:Some("plus".into()),id:ProviderId::Codex,status:crate::providers::ProviderStatus::Ok,
            metrics:crate::providers::codex::parse_wham_usage(&format!(r#"{{"rate_limit":{{"primary_window":{{"used_percent":24,"limit_window_seconds":18000,"reset_at":{}}},"secondary_window":{{"used_percent":32,"limit_window_seconds":604800,"reset_at":{}}}}}}}"#,(now+Duration::hours(3)).timestamp(),reset.timestamp())).unwrap(),updated_at:now,received_at:Some(std::time::Instant::now())
        };
        for m in &mut data.metrics {
            m.observed_at = Some(now);
        }
        let mut history = AccountHistory::default();
        for i in 0..40 {
            history.quota.push(Observation {
                at: now - Duration::minutes(39 - i) * 15,
                remaining: 100. - i as f32 * 0.8,
                reset: Some(reset),
                seconds: Some(604800),
                window: MetricWindow::Long,
            });
        }
        for i in 0..30 {
            if i != 9 {
                history.tokens.insert(
                    (now - Duration::days(i)).date_naive(),
                    if i == 12 {
                        0
                    } else {
                        (i as u64 * 37123 + 5123) % 160000
                    },
                );
            }
        }
        let context = PanelContext {
            history: Rc::new(history),
            extras: Snapshot {
                account: Some("fixture".into()),
                credits: Availability::Ready(Credits {
                    count: 3,
                    items: Some(vec![
                        Credit {
                            granted: Some(now - Duration::days(10)),
                            expires: Some(now + Duration::days(8))
                        };
                        3
                    ]),
                }),
                tokens: Availability::Ready(Default::default()),
                ..Default::default()
            },
            ..Default::default()
        };
        let anchor = RECT {
            left: 100,
            top: 900,
            right: 500,
            bottom: 940,
        };
        let dir = std::path::Path::new("target/quotabar-complete");
        std::fs::create_dir_all(dir).unwrap();
        unsafe {
            let h = create(
                event_loop.create_proxy(),
                View::new(&[data], &Appearance::default()),
                context,
                anchor,
                None,
            )
            .unwrap();
            refresh(h);
            let visible = IsWindowVisible(h).as_bool();
            assert!(!visible);
            for scale in [1., 1.25, 1.5, 2.] {
                change(h, |s| {
                    s.scale = scale;
                    s.resources = Resources::new(scale, s.light);
                });
                refresh(h);
                let state = snapshot(h).unwrap();
                let mut client = RECT::default();
                GetClientRect(h, &mut client).unwrap();
                assert!(client.right > 0 && client.bottom > 0);
                assert_eq!(state.view.quotas.len(), 2);
                super::super::appearance_dialog::tests::capture_offscreen(
                    h,
                    &dir.join(format!("panel-{}dpi.png", (scale * 96.) as i32)),
                );
            }
            let before = snapshot(h).unwrap().charts.borrow()[0].clone();
            super::super::appearance_dialog::tests::capture_offscreen(
                h,
                &dir.join("panel-cached.png"),
            );
            let after = snapshot(h).unwrap().charts.borrow()[0].clone();
            if let (Some(a), Some(b)) = (before, after) {
                assert!(Rc::ptr_eq(&a, &b));
            }
            SendMessageW(h, WM_VSCROLL, WPARAM(SB_BOTTOM.0 as usize), LPARAM(0));
            assert!(snapshot(h).unwrap().scroll > 0.);
            super::super::appearance_dialog::tests::capture_offscreen(
                h,
                &dir.join("panel-scrolled.png"),
            );
            SetTimer(h, 1, 60000, None);
            hide(h);
            assert!(KillTimer(h, 1).is_err());
            let state = snapshot(h).unwrap();
            for kind in [ChartKind::Quota, ChartKind::Tokens] {
                let detail = create(
                    event_loop.create_proxy(),
                    state.view.clone(),
                    state.context.clone(),
                    anchor,
                    Some(kind),
                )
                .unwrap();
                refresh(detail);
                SendMessageW(detail, WM_COMMAND, WPARAM(SEVEN), LPARAM(0));
                assert_eq!(snapshot(detail).unwrap().days, 7);
                SendMessageW(detail, WM_COMMAND, WPARAM(THIRTY), LPARAM(0));
                assert_eq!(snapshot(detail).unwrap().days, 30);
                super::super::appearance_dialog::tests::capture_offscreen(
                    detail,
                    &dir.join(format!("details-{kind:?}.png")),
                );
                DestroyWindow(detail).unwrap();
            }
            change(h, |s| {
                for m in &mut s.context.layout.modules {
                    m.visible = false;
                }
            });
            refresh(h);
            super::super::appearance_dialog::tests::capture_offscreen(
                h,
                &dir.join("panel-empty.png"),
            );
            assert!(GetDlgItem(h, SETTINGS as i32).is_ok());
            DestroyWindow(h).unwrap();
        }
    }
    #[test]
    fn independent_layout_and_placement() {
        let c = PanelContext::default();
        let style = Appearance::default();
        assert_eq!(View::new(&[], &style).quotas.len(), 2);
        assert_eq!(sections(&c).len(), 5);
        let mut hidden = c;
        for m in &mut hidden.layout.modules {
            m.visible = false;
        }
        assert!(sections(&hidden).is_empty());
        assert!(content_height(&hidden) > 0.);
        let work = RECT {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1040,
        };
        let a = RECT {
            left: -10,
            top: 1040,
            right: 0,
            bottom: 1080,
        };
        let r = placement(a, work, 400, 800, 6);
        assert!(r.left >= work.left && r.right <= work.right && r.top >= 0 && r.bottom <= 1040);
    }
}
