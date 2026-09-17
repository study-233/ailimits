//! Native preferences: one draft, event-driven preview, explicit commit or rollback.
use super::native_controls::{draw_label, fill_rounded, native_color, rounded, sized_font};
use super::theme::{Color, SurfaceTheme, UiMetrics};
use crate::{
    app::UserEvent,
    config::appearance::{rgb, Appearance, DisplayStyle, NumberWeight, QuotaPeriods, TrayDisplay},
    i18n::t,
};
use std::cell::RefCell;
use tao::event_loop::EventLoopProxy;
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{
                Dialogs::*, SetScrollInfo, SetWindowTheme, DRAWITEMSTRUCT, EM_SETMARGINS,
                MEASUREITEMSTRUCT, WM_MOUSELEAVE,
            },
            HiDpi::*,
            Input::KeyboardAndMouse::{
                GetFocus, IsWindowEnabled, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
            },
            Shell::{DefSubclassProc, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
};
const SAVE: usize = 1;
const CANCEL: usize = 2;
const RESET: usize = 3;
const ADVANCED: usize = 4;
const RINGS: usize = 10;
const BARS: usize = 11;
const WEEKLY: usize = 12;
const FIVE: usize = 13;
const BOTH: usize = 14;
const SIZE: usize = 15;
const TRAY: usize = 16;
const RING_COLOR: usize = 100;
const NUMBER_COLOR: usize = 101;
const SESSION_COLOR: usize = 111;
const WEIGHT: usize = 110;
const STATUS: usize = 120;
const CHOOSE_RING: usize = 130;
const CHOOSE_NUMBER: usize = 131;
const CHOOSE_SESSION: usize = 132;
const IDS: [usize; 8] = [102, 103, 104, 105, 106, 107, 108, 109];
#[derive(Clone)]
struct Control {
    hwnd: HWND,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    advanced: bool,
    footer: bool,
}
pub(crate) struct AppearanceDialog {
    hwnd: HWND,
    proxy: EventLoopProxy<UserEvent>,
}
#[derive(Clone)]
struct State {
    proxy: EventLoopProxy<UserEvent>,
    draft: Appearance,
    panel_draft: crate::config::panel::PanelConfig,
    panel_page: bool,
    drag: Option<usize>,
    drop_row: Option<usize>,
    scale: f32,
    font: HFONT,
    title_font: HFONT,
    caption_font: HFONT,
    controls: Vec<Control>,
    advanced: bool,
    scroll: i32,
    light: bool,
    background: HBRUSH,
    card: HBRUSH,
}
// Never hold a RefCell borrow across a Win32 call: child controls synchronously
// send paint/focus notifications back to their parent. Snapshots own no handles.
unsafe fn snapshot(hwnd: HWND) -> Option<State> {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<State>;
    (!ptr.is_null()).then(|| (*ptr).borrow().clone())
}
unsafe fn publish(hwnd: HWND, state: &State) {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<State>;
    if !ptr.is_null() {
        *(*ptr).borrow_mut() = state.clone();
    }
}
struct ControlUpdate(HWND, HANDLE);
impl ControlUpdate {
    unsafe fn new(hwnd: HWND) -> Self {
        let previous = GetPropW(hwnd, w!("QuotaBarUpdating"));
        let _ = SetPropW(hwnd, w!("QuotaBarUpdating"), HANDLE(1usize as _));
        Self(hwnd, previous)
    }
}
impl Drop for ControlUpdate {
    fn drop(&mut self) {
        unsafe {
            if self.1.is_invalid() {
                let _ = RemovePropW(self.0, w!("QuotaBarUpdating"));
            } else {
                let _ = SetPropW(self.0, w!("QuotaBarUpdating"), self.1);
            }
        }
    }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn cr(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}
unsafe fn label(hwnd: HWND, id: usize, text: &str) {
    let v = wide(text);
    let _ = SetWindowTextW(
        GetDlgItem(hwnd, id as i32).unwrap_or_default(),
        PCWSTR(v.as_ptr()),
    );
}
unsafe fn value(hwnd: HWND, id: usize) -> String {
    let mut b = [0u16; 128];
    let n = GetWindowTextW(GetDlgItem(hwnd, id as i32).unwrap_or_default(), &mut b);
    String::from_utf16_lossy(&b[..n.max(0) as usize])
}
unsafe fn read(hwnd: HWND, draft: &Appearance) -> Option<Appearance> {
    let mut s = draft.clone();
    let mut values = Vec::new();
    for id in IDS {
        values.push(value(hwnd, id).trim().parse::<i32>().ok()?);
    }
    s.ring_color = value(hwnd, RING_COLOR).trim().into();
    s.session_color = value(hwnd, SESSION_COLOR).trim().into();
    s.number_color = value(hwnd, NUMBER_COLOR).trim().into();
    if s.display_style == DisplayStyle::Rings {
        s.ring_size = values[0];
        s.ring_thickness = values[1];
        s.ring_x = values[2];
        s.ring_y = values[3];
        s.number_x = values[5];
    } else {
        s.bar_width = values[0];
        s.bar_thickness = values[1];
        s.bar_x = values[2];
        s.bar_y = values[3];
        s.bar_number_x = values[5];
    }
    s.number_size = values[4];
    s.number_y = values[6];
    s.symbol_percent = values[7];
    s.number_weight = match SendMessageW(
        GetDlgItem(hwnd, WEIGHT as i32).ok()?,
        CB_GETCURSEL,
        WPARAM(0),
        LPARAM(0),
    )
    .0
    {
        0 => NumberWeight::Regular,
        2 => NumberWeight::Bold,
        _ => NumberWeight::Semibold,
    };
    s.tray_display = match SendMessageW(
        GetDlgItem(hwnd, TRAY as i32).ok()?,
        CB_GETCURSEL,
        WPARAM(0),
        LPARAM(0),
    )
    .0
    {
        1 => TrayDisplay::Ring,
        2 => TrayDisplay::Number,
        3 => TrayDisplay::RingState,
        _ => TrayDisplay::Auto,
    };
    s.validate().then_some(s)
}
unsafe fn fill(hwnd: HWND, s: &Appearance) {
    let _update = ControlUpdate::new(hwnd);
    label(hwnd, RING_COLOR, &s.ring_color);
    label(hwnd, SESSION_COLOR, &s.session_color);
    label(hwnd, NUMBER_COLOR, &s.number_color);
    SendMessageW(
        GetDlgItem(hwnd, TRAY as i32).unwrap_or_default(),
        CB_SETCURSEL,
        WPARAM(match s.tray_display {
            TrayDisplay::Auto => 0,
            TrayDisplay::Ring => 1,
            TrayDisplay::Number => 2,
            TrayDisplay::RingState => 3,
        }),
        LPARAM(0),
    );
    let rings = s.display_style == DisplayStyle::Rings;
    let vals = if rings {
        [
            s.ring_size,
            s.ring_thickness,
            s.ring_x,
            s.ring_y,
            s.number_size,
            s.number_x,
            s.number_y,
            s.symbol_percent,
        ]
    } else {
        [
            s.bar_width,
            s.bar_thickness,
            s.bar_x,
            s.bar_y,
            s.number_size,
            s.bar_number_x,
            s.number_y,
            s.symbol_percent,
        ]
    };
    for (id, v) in IDS.into_iter().zip(vals) {
        label(hwnd, id, &v.to_string());
    }
    label(
        hwnd,
        202,
        t(if rings { "Ring diameter" } else { "Bar width" }),
    );
    label(
        hwnd,
        203,
        t(if rings {
            "Ring thickness"
        } else {
            "Bar thickness"
        }),
    );
    SendMessageW(
        GetDlgItem(hwnd, WEIGHT as i32).unwrap_or_default(),
        CB_SETCURSEL,
        WPARAM(match s.number_weight {
            NumberWeight::Regular => 0,
            NumberWeight::Semibold => 1,
            NumberWeight::Bold => 2,
        }),
        LPARAM(0),
    );
    SendMessageW(
        GetDlgItem(hwnd, SIZE as i32).unwrap_or_default(),
        CB_SETCURSEL,
        WPARAM(match (s.ring_size, s.number_size, s.bar_width) {
            (24, 16, 64) => 0,
            (28, 20, 88) => 1,
            (36, 24, 112) => 2,
            _ => 3,
        }),
        LPARAM(0),
    );
}
unsafe fn layout(hwnd: HWND, s: &mut State, resize: bool) {
    let _update = ControlUpdate::new(hwnd);
    publish(hwnd, s);
    let height = if s.advanced && !s.panel_page {
        788.0
    } else {
        548.0
    };
    let content = (height * s.scale).round() as i32;
    if resize {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let available = (info.rcWork.bottom - info.rcWork.top - 64).max(240);
        let mut r = RECT {
            left: 0,
            top: 0,
            right: (520.0 * s.scale) as i32,
            bottom: content.min(available),
        };
        let _ = AdjustWindowRectExForDpi(
            &mut r,
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VSCROLL | WS_CLIPCHILDREN,
            false,
            WS_EX_CONTROLPARENT,
            GetDpiForWindow(hwnd).max(96),
        );
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            r.right - r.left,
            r.bottom - r.top,
            SWP_NOMOVE | SWP_NOZORDER,
        );
        let mut wr = RECT::default();
        let _ = GetWindowRect(hwnd, &mut wr);
        let x = wr.left.clamp(
            info.rcWork.left,
            (info.rcWork.right - (wr.right - wr.left)).max(info.rcWork.left),
        );
        let y = wr.top.clamp(
            info.rcWork.top,
            (info.rcWork.bottom - (wr.bottom - wr.top)).max(info.rcWork.top),
        );
        let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
    }
    let mut client = RECT::default();
    let _ = GetClientRect(hwnd, &mut client);
    s.scroll = s.scroll.clamp(0, (content - client.bottom).max(0));
    publish(hwnd, s);
    let si = SCROLLINFO {
        cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
        nMin: 0,
        nMax: content - 1,
        nPage: client.bottom.max(1) as u32,
        nPos: s.scroll,
        ..Default::default()
    };
    SetScrollInfo(hwnd, SB_VERT, &si, true);
    for c in &s.controls {
        let id = GetDlgCtrlID(c.hwnd) as usize;
        let visible = if [300, 400, 401, SAVE, CANCEL, RESET, STATUS].contains(&id) {
            true
        } else if id == 301 {
            false
        } else if (410..440).contains(&id) {
            s.panel_page
        } else {
            !s.panel_page && (!c.advanced || s.advanced)
        };
        let _ = ShowWindow(c.hwnd, if visible { SW_SHOW } else { SW_HIDE });
        let y = c.y
            + if c.footer && s.advanced && !s.panel_page {
                240.0
            } else {
                0.0
            };
        let _ = MoveWindow(
            c.hwnd,
            (c.x * s.scale) as i32,
            (y * s.scale) as i32 - s.scroll,
            (c.w * s.scale) as i32,
            (c.h * s.scale) as i32,
            true,
        );
        let id = GetDlgCtrlID(c.hwnd) as usize;
        if IDS.contains(&id) || [RING_COLOR, NUMBER_COLOR, SESSION_COLOR].contains(&id) {
            let margin = (8. * s.scale).round() as usize;
            SendMessageW(
                c.hwnd,
                EM_SETMARGINS,
                WPARAM(3),
                LPARAM((margin | (margin << 16)) as isize),
            );
        }
        if [SIZE, TRAY, WEIGHT].contains(&id) {
            SendMessageW(
                c.hwnd,
                CB_SETITEMHEIGHT,
                WPARAM(usize::MAX),
                LPARAM((26. * s.scale) as isize),
            );
            SendMessageW(
                c.hwnd,
                CB_SETITEMHEIGHT,
                WPARAM(0),
                LPARAM((28. * s.scale) as isize),
            );
        }
    }
    label(
        hwnd,
        ADVANCED,
        t(if s.advanced && !s.panel_page {
            "Hide advanced settings"
        } else {
            "Advanced settings"
        }),
    );
    let _ = RedrawWindow(
        hwnd,
        None,
        None,
        RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
    );
}
/// No scroll transaction (and no repaint) at either boundary or when content fits.
fn scroll_destination(current: i32, requested: i32, content: i32, viewport: i32) -> Option<i32> {
    let next = requested.clamp(0, (content - viewport).max(0));
    (next != current).then_some(next)
}

/// Scrolling moves existing controls; their size, visibility and font stay intact.
unsafe fn scroll_to(hwnd: HWND, s: &mut State, requested: i32) {
    let mut client = RECT::default();
    if GetClientRect(hwnd, &mut client).is_err() {
        return;
    }
    let content = ((if s.advanced && !s.panel_page {
        788.0
    } else {
        548.0
    }) * s.scale)
        .round() as i32;
    let Some(next) = scroll_destination(s.scroll, requested, content, client.bottom) else {
        return;
    };
    let _update = ControlUpdate::new(hwnd);
    s.scroll = next;
    publish(hwnd, s);

    let controls: Vec<_> = s
        .controls
        .iter()
        .filter(|c| IsWindowVisible(c.hwnd).as_bool())
        .collect();
    let flags = SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOREDRAW;
    let position = |c: &Control| {
        let y = c.y
            + if c.footer && s.advanced && !s.panel_page {
                240.0
            } else {
                0.0
            };
        ((c.x * s.scale) as i32, (y * s.scale) as i32 - next)
    };
    let batch = BeginDeferWindowPos(controls.len() as i32).and_then(|mut batch| {
        for c in &controls {
            let (x, y) = position(c);
            batch = DeferWindowPos(batch, c.hwnd, None, x, y, 0, 0, flags)?;
        }
        EndDeferWindowPos(batch)
    });
    if let Err(error) = batch {
        // A failed batch must not leave controls at a different scroll offset.
        tracing::warn!(%error, "deferred preferences scroll failed; applying positions directly");
        for c in controls {
            let (x, y) = position(c);
            let _ = SetWindowPos(c.hwnd, None, x, y, 0, 0, flags);
        }
    }
    let info = SCROLLINFO {
        cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
        fMask: SIF_POS,
        nPos: next,
        ..Default::default()
    };
    SetScrollInfo(hwnd, SB_VERT, &info, true);
    // Coalesce wheel events in the normal message loop. Paint the final positions
    // without exposing an erased background between individual child moves.
    let _ = RedrawWindow(
        hwnd,
        None,
        None,
        RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_NOERASE,
    );
}

thread_local! { static ACTIVE: std::cell::Cell<HWND> = const { std::cell::Cell::new(HWND(std::ptr::null_mut())) }; }

/// Let native controls handle Tab, Enter and Escape through the host message pump.
pub(crate) unsafe fn handle_message(message: *const std::ffi::c_void) -> bool {
    let message = &*message.cast::<MSG>();
    // IsDialogMessage may dispatch non-keyboard messages itself. Passing the
    // whole host queue would consume tao's wakeups and strand menu/user events.
    if !(WM_KEYFIRST..=WM_KEYLAST).contains(&message.message) {
        return false;
    }
    ACTIVE.with(|active| {
        let hwnd = active.get();
        if !hwnd.is_invalid()
            && IsWindow(hwnd).as_bool()
            && (message.hwnd == hwnd || IsChild(hwnd, message.hwnd).as_bool())
            && message.message == WM_KEYDOWN
            && message.wParam.0 == 0x1b
        {
            // Esc always rolls the draft back, including when focus is in an edit.
            SendMessageW(hwnd, WM_COMMAND, WPARAM(CANCEL), LPARAM(0));
            return true;
        }
        !hwnd.is_invalid()
            && IsWindow(hwnd).as_bool()
            && (message.hwnd == hwnd || IsChild(hwnd, message.hwnd).as_bool())
            && IsDialogMessageW(hwnd, message).as_bool()
    })
}

impl AppearanceDialog {
    pub fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            hwnd: HWND::default(),
            proxy,
        }
    }
    #[cfg(test)]
    pub fn open(&mut self, appearance: &Appearance) -> anyhow::Result<()> {
        self.open_settings(
            appearance,
            &crate::config::panel::PanelConfig::default(),
            false,
        )
    }
    pub fn open_settings(
        &mut self,
        appearance: &Appearance,
        panel: &crate::config::panel::PanelConfig,
        panel_page: bool,
    ) -> anyhow::Result<()> {
        self.create_settings(appearance, panel, panel_page, true)
    }
    fn create_settings(
        &mut self,
        appearance: &Appearance,
        panel: &crate::config::panel::PanelConfig,
        panel_page: bool,
        present: bool,
    ) -> anyhow::Result<()> {
        unsafe {
            if IsWindow(self.hwnd).as_bool() {
                if let Some(mut state) = snapshot(self.hwnd) {
                    state.panel_page = panel_page;
                    state.scroll = 0;
                    layout(self.hwnd, &mut state, true);
                }
                let _ = SetForegroundWindow(self.hwnd);
                return Ok(());
            }
            let instance = GetModuleHandleW(None)?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: instance.into(),
                lpszClassName: w!("QuotaBarAppearance"),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                hIcon: LoadIconW(instance, PCWSTR(1usize as _)).unwrap_or_default(),
                ..Default::default()
            };
            RegisterClassW(&class);
            let title = wide(&format!("QuotaBar · {}", t("Settings")));
            self.hwnd = CreateWindowExW(
                WS_EX_CONTROLPARENT,
                class.lpszClassName,
                PCWSTR(title.as_ptr()),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VSCROLL | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                700,
                540,
                None,
                None,
                instance,
                None,
            )?;
            let scale = GetDpiForWindow(self.hwnd).max(96) as f32 / 96.0;
            let font = make_font(scale);
            let title_font = sized_font(scale, UiMetrics::TITLE, 600);
            let caption_font = sized_font(scale, UiMetrics::CAPTION, 400);
            let mut controls = Vec::new();
            let mut add = |class: PCWSTR,
                           text: &str,
                           id: usize,
                           x: f32,
                           y: f32,
                           width: f32,
                           height: f32,
                           extra: u32,
                           advanced: bool,
                           footer: bool|
             -> anyhow::Result<()> {
                let text = wide(text);
                let child = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    class,
                    PCWSTR(text.as_ptr()),
                    WS_CHILD | WS_VISIBLE | WINDOW_STYLE(extra),
                    0,
                    0,
                    1,
                    1,
                    self.hwnd,
                    HMENU(id as _),
                    instance,
                    None,
                )?;
                SendMessageW(
                    child,
                    WM_SETFONT,
                    WPARAM(
                        if id == 300 {
                            title_font
                        } else if [301, 302, 303, STATUS].contains(&id) {
                            caption_font
                        } else {
                            font
                        }
                        .0 as usize,
                    ),
                    LPARAM(1),
                );
                if extra & WS_TABSTOP.0 != 0 {
                    let _ = SetWindowSubclass(child, Some(control_proc), 1, 0);
                    let _ = SetWindowTheme(
                        child,
                        if [SIZE, TRAY, WEIGHT].contains(&id) {
                            w!("")
                        } else {
                            w!("Explorer")
                        },
                        PCWSTR::null(),
                    );
                }
                controls.push(Control {
                    hwnd: child,
                    x,
                    y,
                    w: width,
                    h: height,
                    advanced,
                    footer,
                });
                Ok(())
            };
            let button = WS_TABSTOP.0 | BS_OWNERDRAW as u32 | BS_NOTIFY as u32;
            let edit = WS_TABSTOP.0 | WS_BORDER.0 | ES_AUTOHSCROLL as u32;
            let combo = WS_TABSTOP.0
                | CBS_DROPDOWNLIST as u32
                | CBS_OWNERDRAWFIXED as u32
                | CBS_HASSTRINGS as u32
                | WS_VSCROLL.0;
            add(
                w!("STATIC"),
                t("Settings"),
                300,
                24.,
                16.,
                472.,
                30.,
                0,
                false,
                false,
            )?;
            add(
                w!("STATIC"),
                t("Customize your taskbar quota display."),
                301,
                24.,
                52.,
                472.,
                20.,
                0,
                false,
                false,
            )?;
            add(
                w!("STATIC"),
                t("Display style"),
                0,
                24.,
                212.,
                116.,
                24.,
                0,
                false,
                false,
            )?;
            add(
                w!("BUTTON"),
                t("Rings"),
                RINGS,
                160.,
                204.,
                166.,
                32.,
                button,
                false,
                false,
            )?;
            add(
                w!("BUTTON"),
                t("Progress bars"),
                BARS,
                330.,
                204.,
                166.,
                32.,
                button,
                false,
                false,
            )?;
            add(
                w!("STATIC"),
                t("Quota periods"),
                0,
                24.,
                256.,
                116.,
                24.,
                0,
                false,
                false,
            )?;
            for (id, title, x) in [
                (WEEKLY, "Weekly", 160.),
                (FIVE, "5 hours", 274.),
                (BOTH, "Both periods", 388.),
            ] {
                add(
                    w!("BUTTON"),
                    t(title),
                    id,
                    x,
                    248.,
                    108.,
                    32.,
                    button,
                    false,
                    false,
                )?;
            }
            for (choose, field, title, x) in [
                (CHOOSE_RING, RING_COLOR, "Weekly", 24.),
                (CHOOSE_SESSION, SESSION_COLOR, "5 hours", 268.),
            ] {
                add(
                    w!("BUTTON"),
                    t(title),
                    choose,
                    x,
                    296.,
                    28.,
                    28.,
                    button,
                    false,
                    false,
                )?;
                add(
                    w!("STATIC"),
                    t(title),
                    0,
                    x + 36.,
                    300.,
                    64.,
                    24.,
                    0,
                    false,
                    false,
                )?;
                add(
                    w!("EDIT"),
                    "",
                    field,
                    x + 108.,
                    296.,
                    120.,
                    28.,
                    edit,
                    false,
                    false,
                )?;
            }
            add(
                w!("STATIC"),
                t("Size"),
                0,
                24.,
                348.,
                116.,
                24.,
                0,
                false,
                false,
            )?;
            add(
                w!("COMBOBOX"),
                "",
                SIZE,
                300.,
                340.,
                196.,
                160.,
                combo,
                false,
                false,
            )?;
            add(
                w!("STATIC"),
                t("Tray display"),
                0,
                24.,
                392.,
                200.,
                24.,
                0,
                false,
                false,
            )?;
            add(
                w!("COMBOBOX"),
                "",
                TRAY,
                300.,
                384.,
                196.,
                160.,
                combo,
                false,
                false,
            )?;
            add(
                w!("STATIC"),
                t("Preview"),
                0,
                24.,
                84.,
                180.,
                24.,
                0,
                false,
                false,
            )?;
            add(
                w!("STATIC"),
                t("Light"),
                302,
                24.,
                112.,
                200.,
                20.,
                0,
                false,
                false,
            )?;
            add(
                w!("STATIC"),
                t("Dark"),
                303,
                268.,
                112.,
                200.,
                20.,
                0,
                false,
                false,
            )?;
            add(
                w!("BUTTON"),
                t("Advanced settings"),
                ADVANCED,
                24.,
                432.,
                472.,
                32.,
                button,
                false,
                false,
            )?;
            for (i, (id, title)) in IDS
                .into_iter()
                .zip([
                    "Ring diameter",
                    "Ring thickness",
                    "Graphic X",
                    "Graphic Y",
                    "Text size",
                    "Text X",
                    "Text Y",
                    "Symbols %",
                ])
                .enumerate()
            {
                let x = 24. + (i % 2) as f32 * 244.;
                let y = 480. + (i / 2) as f32 * 40.;
                add(
                    w!("STATIC"),
                    t(title),
                    id + 100,
                    x,
                    y + 4.,
                    140.,
                    24.,
                    0,
                    true,
                    false,
                )?;
                add(w!("EDIT"), "", id, x + 148., y, 80., 28., edit, true, false)?;
            }
            add(
                w!("STATIC"),
                t("Text weight"),
                0,
                24.,
                648.,
                96.,
                24.,
                0,
                true,
                false,
            )?;
            add(
                w!("COMBOBOX"),
                "",
                WEIGHT,
                128.,
                640.,
                124.,
                140.,
                combo,
                true,
                false,
            )?;
            add(
                w!("BUTTON"),
                t("Number color"),
                CHOOSE_NUMBER,
                268.,
                640.,
                104.,
                32.,
                button,
                true,
                false,
            )?;
            add(
                w!("EDIT"),
                "",
                NUMBER_COLOR,
                380.,
                642.,
                116.,
                28.,
                edit,
                true,
                false,
            )?;
            add(
                w!("STATIC"),
                t("Use auto to follow the taskbar theme."),
                0,
                24.,
                684.,
                472.,
                20.,
                0,
                true,
                false,
            )?;
            add(
                w!("STATIC"),
                "",
                STATUS,
                24.,
                468.,
                472.,
                20.,
                0,
                false,
                true,
            )?;
            add(
                w!("BUTTON"),
                t("Restore defaults"),
                RESET,
                24.,
                500.,
                164.,
                32.,
                button,
                false,
                true,
            )?;
            add(
                w!("BUTTON"),
                t("Cancel"),
                CANCEL,
                300.,
                500.,
                92.,
                32.,
                button,
                false,
                true,
            )?;
            add(
                w!("BUTTON"),
                t("Save"),
                SAVE,
                404.,
                500.,
                92.,
                32.,
                button,
                false,
                true,
            )?;
            add(
                w!("BUTTON"),
                t("Taskbar appearance"),
                400,
                24.,
                48.,
                230.,
                30.,
                button,
                false,
                false,
            )?;
            add(
                w!("BUTTON"),
                t("Panel content"),
                401,
                266.,
                48.,
                230.,
                30.,
                button,
                false,
                false,
            )?;
            for i in 0..5 {
                let y = 116. + i as f32 * 52.;
                add(
                    w!("BUTTON"),
                    "",
                    410 + i,
                    48.,
                    y,
                    196.,
                    32.,
                    WS_TABSTOP.0 | BS_AUTOCHECKBOX as u32,
                    false,
                    false,
                )?;
                add(
                    w!("BUTTON"),
                    "↑",
                    420 + i,
                    250.,
                    y,
                    32.,
                    32.,
                    button,
                    false,
                    false,
                )?;
                add(
                    w!("BUTTON"),
                    "↓",
                    430 + i,
                    288.,
                    y,
                    32.,
                    32.,
                    button,
                    false,
                    false,
                )?;
            }
            for (id, items) in [
                (SIZE, vec!["Compact", "Standard", "Large", "Custom"]),
                (WEIGHT, vec!["Regular", "Semibold", "Bold"]),
                (
                    TRAY,
                    vec!["Auto", "Follow style", "Number", "Style + state"],
                ),
            ] {
                for text in items {
                    let text = wide(t(text));
                    SendMessageW(
                        GetDlgItem(self.hwnd, id as i32)?,
                        CB_ADDSTRING,
                        WPARAM(0),
                        LPARAM(text.as_ptr() as isize),
                    );
                }
            }
            for id in [SIZE, TRAY, WEIGHT] {
                SendMessageW(
                    GetDlgItem(self.hwnd, id as i32)?,
                    CB_SETITEMHEIGHT,
                    WPARAM(usize::MAX),
                    LPARAM((24. * scale) as isize),
                );
            }
            fill(self.hwnd, appearance);
            let light = crate::platform::apps_use_light_theme();
            window_theme(self.hwnd, light);
            let state = Box::new(RefCell::new(State {
                proxy: self.proxy.clone(),
                draft: appearance.clone(),
                panel_draft: panel.clone(),
                panel_page,
                drag: None,
                drop_row: None,
                scale,
                font,
                title_font,
                caption_font,
                controls,
                advanced: false,
                scroll: 0,
                light,
                background: CreateSolidBrush(native_color(SurfaceTheme::new(light).background)),
                card: CreateSolidBrush(native_color(SurfaceTheme::new(light).surface)),
            }));
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
            let mut initial = snapshot(self.hwnd).unwrap();
            fill_panel(self.hwnd, &initial);
            layout(self.hwnd, &mut initial, true);
            ACTIVE.with(|a| a.set(self.hwnd));
            if present {
                let _ = ShowWindow(self.hwnd, SW_SHOW);
                let _ = SetForegroundWindow(self.hwnd);
            }
            Ok(())
        }
    }
}
/// Keep native keyboard/UI Automation behavior, customize only the closed face.
unsafe extern "system" fn control_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    if msg == WM_MOUSEMOVE && GetPropW(hwnd, w!("QuotaBarHot")).is_invalid() {
        let _ = SetPropW(hwnd, w!("QuotaBarHot"), HANDLE(1usize as _));
        let mut tracking = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: hwnd,
            ..Default::default()
        };
        let _ = TrackMouseEvent(&mut tracking);
        let _ = InvalidateRect(hwnd, None, false);
    }
    if msg == WM_MOUSELEAVE {
        let _ = RemovePropW(hwnd, w!("QuotaBarHot"));
        let _ = InvalidateRect(hwnd, None, false);
    }
    if msg == WM_NCDESTROY {
        let _ = RemovePropW(hwnd, w!("QuotaBarHot"));
    }
    let id = GetDlgCtrlID(hwnd) as usize;
    if msg == WM_NCPAINT
        && (IDS.contains(&id) || [RING_COLOR, NUMBER_COLOR, SESSION_COLOR].contains(&id))
    {
        let parent = GetParent(hwnd).unwrap_or_default();
        let ptr = GetWindowLongPtrW(parent, GWLP_USERDATA) as *const RefCell<State>;
        if !ptr.is_null() {
            if let Some(s) = snapshot(parent) {
                let theme = SurfaceTheme::new(s.light);
                let dc = GetWindowDC(hwnd);
                let mut r = RECT::default();
                let _ = GetWindowRect(hwnd, &mut r);
                r.right -= r.left;
                r.bottom -= r.top;
                r.left = 0;
                r.top = 0;
                let brush = CreateSolidBrush(native_color(if GetFocus() == hwnd {
                    theme.accent
                } else {
                    theme.border
                }));
                FrameRect(dc, &r, brush);
                let _ = DeleteObject(brush);
                ReleaseDC(hwnd, dc);
                return LRESULT(0);
            }
        }
    }
    if [SIZE, TRAY, WEIGHT].contains(&id) {
        if [WM_PAINT, WM_PRINTCLIENT, WM_PRINT].contains(&msg) {
            let Some(s) = snapshot(GetParent(hwnd).unwrap_or_default()) else {
                return DefSubclassProc(hwnd, msg, wp, lp);
            };
            let mut ps = PAINTSTRUCT::default();
            let dc = if msg == WM_PAINT {
                BeginPaint(hwnd, &mut ps)
            } else {
                HDC(wp.0 as _)
            };
            paint_combo_face(hwnd, dc, &s);
            if msg == WM_PAINT {
                let _ = EndPaint(hwnd, &ps);
            }
            return LRESULT(0);
        }
        // The stock combo also paints synchronously while handling selection/focus.
        // Finish that processing first, then paint the entire closed face.
        let result = DefSubclassProc(hwnd, msg, wp, lp);
        if [
            WM_SETFOCUS,
            WM_KILLFOCUS,
            CB_SETCURSEL,
            CB_SHOWDROPDOWN,
            WM_LBUTTONDOWN,
            WM_LBUTTONUP,
            WM_KEYDOWN,
            WM_KEYUP,
            WM_ENABLE,
            WM_THEMECHANGED,
            WM_CAPTURECHANGED,
        ]
        .contains(&msg)
        {
            if let Some(s) = snapshot(GetParent(hwnd).unwrap_or_default()) {
                let dc = GetDC(hwnd);
                paint_combo_face(hwnd, dc, &s);
                ReleaseDC(hwnd, dc);
                let _ = InvalidateRect(hwnd, None, false);
            }
        }
        return result;
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}

unsafe fn paint_combo_face(hwnd: HWND, dc: HDC, s: &State) {
    let theme = SurfaceTheme::new(s.light);
    let mut rect = RECT::default();
    let _ = GetClientRect(hwnd, &mut rect);
    FillRect(dc, &rect, s.background);
    let hot = !GetPropW(hwnd, w!("QuotaBarHot")).is_invalid();
    fill_rounded(
        dc,
        rect,
        if hot { theme.hover } else { theme.surface },
        UiMetrics::CONTROL_RADIUS * s.scale,
    );
    let focused = GetFocus() == hwnd;
    let bottom = RECT {
        left: rect.left + (6. * s.scale) as i32,
        right: rect.right - (6. * s.scale) as i32,
        top: rect.bottom - (if focused { 2. } else { 1. } * s.scale) as i32,
        bottom: rect.bottom,
    };
    let brush = CreateSolidBrush(native_color(if focused {
        theme.accent
    } else {
        theme.border
    }));
    FillRect(dc, &bottom, brush);
    let _ = DeleteObject(brush);
    let ink = if IsWindowEnabled(hwnd).as_bool() {
        theme.text
    } else {
        theme.text_tertiary
    };
    let selection = SendMessageW(hwnd, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
    let text = combo_text(hwnd, selection);
    rect.left += (12. * s.scale) as i32;
    rect.right -= (28. * s.scale) as i32;
    draw_label(dc, &text, rect, s.font, ink, DT_LEFT);
    rect.left = rect.right;
    rect.right += (24. * s.scale) as i32;
    draw_label(dc, "⌄", rect, s.font, ink, DT_CENTER);
}

unsafe fn combo_text(hwnd: HWND, index: isize) -> String {
    if index < 0 {
        return String::new();
    }
    let length = SendMessageW(hwnd, CB_GETLBTEXTLEN, WPARAM(index as usize), LPARAM(0)).0;
    if length < 0 {
        return String::new();
    }
    let mut text = vec![0u16; length as usize + 1];
    let n = SendMessageW(
        hwnd,
        CB_GETLBTEXT,
        WPARAM(index as usize),
        LPARAM(text.as_mut_ptr() as isize),
    )
    .0;
    if n < 0 {
        return String::new();
    }
    String::from_utf16_lossy(&text[..n as usize])
}

unsafe fn make_font(scale: f32) -> HFONT {
    sized_font(scale, UiMetrics::BODY, 400)
}
unsafe fn choose(hwnd: HWND, id: usize) {
    let [r, g, b] = rgb(&value(hwnd, id)).unwrap_or([255, 55, 116]);
    let mut custom = [0u32; 16];
    let mut c = CHOOSECOLORW {
        lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: hwnd,
        rgbResult: cr(r, g, b),
        lpCustColors: custom.as_mut_ptr().cast(),
        Flags: CC_FULLOPEN | CC_RGBINIT,
        ..Default::default()
    };
    if ChooseColorW(&mut c).as_bool() {
        let n = c.rgbResult.0;
        label(
            hwnd,
            id,
            &format!(
                "#{:02X}{:02X}{:02X}",
                n & 255,
                (n >> 8) & 255,
                (n >> 16) & 255
            ),
        );
    }
}
unsafe fn window_theme(hwnd: HWND, light: bool) {
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
    let dark = BOOL(i32::from(!light));
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        (&dark as *const BOOL).cast(),
        std::mem::size_of::<BOOL>() as u32,
    );
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    #[cfg(test)]
    if msg == WM_APP + 100 {
        tests::capture_menu_timer(hwnd, 0, 42, 0);
        return LRESULT(0);
    }
    if msg == WM_MEASUREITEM {
        let m = &mut *(lp.0 as *mut MEASUREITEMSTRUCT);
        if [SIZE, TRAY, WEIGHT].contains(&(m.CtlID as usize)) {
            let scale = snapshot(hwnd)
                .map(|s| s.scale)
                .unwrap_or(GetDpiForWindow(hwnd).max(96) as f32 / 96.);
            m.itemHeight = (28. * scale).round() as u32;
            return LRESULT(1);
        }
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RefCell<State>;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    if msg == WM_NCDESTROY {
        ACTIVE.with(|a| a.set(HWND::default()));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        let s = Box::from_raw(ptr);
        let s = s.borrow();
        let _ = DeleteObject(s.font);
        let _ = DeleteObject(s.title_font);
        let _ = DeleteObject(s.caption_font);
        let _ = DeleteObject(s.background);
        let _ = DeleteObject(s.card);
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    // Clone, then release the borrow BEFORE dispatching any Windows API.
    let mut s = (*ptr).borrow().clone();
    match msg {
        DM_GETDEFID => LRESULT(((DC_HASDEFID << 16) | SAVE as u32) as isize),
        WM_SETTINGCHANGE | WM_THEMECHANGED => {
            let light = crate::platform::apps_use_light_theme();
            if light != s.light {
                s.light = light;
                let old_background = s.background;
                let old_card = s.card;
                s.background = CreateSolidBrush(native_color(SurfaceTheme::new(light).background));
                s.card = CreateSolidBrush(native_color(SurfaceTheme::new(light).surface));
                publish(hwnd, &s);
                let _ = DeleteObject(old_background);
                let _ = DeleteObject(old_card);
                window_theme(hwnd, light);
                let _ = RedrawWindow(
                    hwnd,
                    None,
                    None,
                    RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
                );
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN if s.panel_page => {
            let x = (lp.0 & 0xffff) as u16 as i16 as f32 / s.scale;
            let y = (((lp.0 >> 16) & 0xffff) as u16 as i16 as f32 + s.scroll as f32) / s.scale;
            if (18. ..44.).contains(&x) && (116. ..376.).contains(&y) {
                s.drag = Some(((y - 116.) / 52.) as usize);
                s.drop_row = s.drag;
                publish(hwnd, &s);
                windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(hwnd);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE if s.drag.is_some() => {
            let y = (((lp.0 >> 16) & 0xffff) as u16 as i16 as f32 + s.scroll as f32) / s.scale;
            s.drop_row = Some(((y - 116.) / 52.).clamp(0., 4.) as usize);
            publish(hwnd, &s);
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_LBUTTONUP if s.drag.is_some() => {
            let from = s.drag.take().unwrap();
            let to = s.drop_row.take().unwrap_or(from);
            s.panel_draft.move_to(from, to);
            publish(hwnd, &s);
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
            panel_changed(hwnd, &s);
            LRESULT(0)
        }
        WM_CAPTURECHANGED if s.drag.is_some() => {
            s.drag = None;
            s.drop_row = None;
            publish(hwnd, &s);
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = s.proxy.send_event(UserEvent::PanelCancel);
            let _ = s.proxy.send_event(UserEvent::AppearanceCancel);
            drop(s);
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DPICHANGED => {
            s.scale = (wp.0 & 0xffff) as f32 / 96.;
            let old = s.font;
            let old_title = s.title_font;
            let old_caption = s.caption_font;
            s.title_font = sized_font(s.scale, UiMetrics::TITLE, 600);
            s.caption_font = sized_font(s.scale, UiMetrics::CAPTION, 400);
            s.font = make_font(s.scale);
            publish(hwnd, &s);
            let _update = ControlUpdate::new(hwnd);
            for c in &s.controls {
                let id = GetDlgCtrlID(c.hwnd) as usize;
                let font = if id == 300 {
                    s.title_font
                } else if [301, 302, 303, STATUS].contains(&id) {
                    s.caption_font
                } else {
                    s.font
                };
                SendMessageW(c.hwnd, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
                if [SIZE, TRAY, WEIGHT].contains(&id) {
                    SendMessageW(
                        c.hwnd,
                        CB_SETITEMHEIGHT,
                        WPARAM(usize::MAX),
                        LPARAM((24. * s.scale) as isize),
                    );
                }
            }
            let _ = DeleteObject(old);
            let _ = DeleteObject(old_title);
            let _ = DeleteObject(old_caption);
            let r = &*(lp.0 as *const RECT);
            let _ = SetWindowPos(hwnd, None, r.left, r.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
            layout(hwnd, &mut s, true);
            LRESULT(0)
        }
        WM_VSCROLL | WM_MOUSEWHEEL => {
            let requested = if msg == WM_MOUSEWHEEL {
                s.scroll - ((wp.0 >> 16) as u16 as i16 as i32) / 120 * (48. * s.scale) as i32
            } else {
                match wp.0 as u16 as u32 {
                    0 => s.scroll - (24. * s.scale) as i32,
                    1 => s.scroll + (24. * s.scale) as i32,
                    2 => s.scroll - (200. * s.scale) as i32,
                    3 => s.scroll + (200. * s.scale) as i32,
                    4 | 5 => ((wp.0 >> 16) & 0xffff) as i32,
                    _ => s.scroll,
                }
            };
            scroll_to(hwnd, &mut s, requested);
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            let dc = HDC(wp.0 as _);
            let theme = SurfaceTheme::new(s.light);
            let id = GetDlgCtrlID(HWND(lp.0 as _)) as usize;
            SetTextColor(
                dc,
                native_color(if id == STATUS {
                    theme.danger
                } else if [301, 302, 303].contains(&id) {
                    theme.text_secondary
                } else {
                    theme.text
                }),
            );
            SetBkMode(dc, TRANSPARENT);
            if msg == WM_CTLCOLORSTATIC || msg == WM_CTLCOLORBTN {
                LRESULT(s.background.0 as isize)
            } else {
                SetBkColor(dc, native_color(theme.surface));
                LRESULT(s.card.0 as isize)
            }
        }
        // WM_PAINT fills the complete background in its back buffer.
        WM_ERASEBKGND => LRESULT(1),
        WM_DRAWITEM => {
            let d = &*(lp.0 as *const DRAWITEMSTRUCT);
            let id = d.CtlID as usize;
            let theme = SurfaceTheme::new(s.light);
            if [SIZE, TRAY, WEIGHT].contains(&id) {
                let selected = d.itemState.0 & 1 != 0;
                let brush = CreateSolidBrush(native_color(if selected {
                    theme.hover
                } else {
                    theme.surface
                }));
                FillRect(d.hDC, &d.rcItem, brush);
                let _ = DeleteObject(brush);
                let mut rect = d.rcItem;
                rect.left += (12. * s.scale) as i32;
                rect.right -= (8. * s.scale) as i32;
                draw_label(
                    d.hDC,
                    &combo_text(d.hwndItem, d.itemID as i32 as isize),
                    rect,
                    s.font,
                    if d.itemState.0 & 4 != 0 {
                        theme.text_tertiary
                    } else {
                        theme.text
                    },
                    DT_LEFT,
                );
                if d.itemState.0 & 0x1000 != 0 {
                    paint_combo_face(d.hwndItem, d.hDC, &s);
                }
                return LRESULT(1);
            }
            let selected = match id {
                400 => !s.panel_page,
                401 => s.panel_page,
                RINGS => s.draft.display_style == DisplayStyle::Rings,
                BARS => s.draft.display_style == DisplayStyle::Bars,
                WEEKLY => s.draft.periods == QuotaPeriods::Weekly,
                FIVE => s.draft.periods == QuotaPeriods::FiveHours,
                BOTH => s.draft.periods == QuotaPeriods::Both,
                _ => false,
            };
            let hot = !GetPropW(d.hwndItem, w!("QuotaBarHot")).is_invalid();
            let pressed = d.itemState.0 & 1 != 0;
            let disabled = d.itemState.0 & 4 != 0;
            let quiet = [RESET, ADVANCED, CHOOSE_NUMBER].contains(&id);
            let fill = if pressed {
                theme.pressed
            } else if id == SAVE {
                theme.accent
            } else if hot {
                theme.hover
            } else if selected || !quiet {
                theme.surface
            } else {
                theme.background
            };
            FillRect(d.hDC, &d.rcItem, s.background);
            fill_rounded(d.hDC, d.rcItem, fill, UiMetrics::CONTROL_RADIUS * s.scale);
            if [CHOOSE_RING, CHOOSE_SESSION].contains(&id) {
                let custom = if id == CHOOSE_RING {
                    &s.draft.ring_color
                } else {
                    &s.draft.session_color
                };
                if let Some([r, g, b]) = rgb(custom) {
                    let mut rect = d.rcItem;
                    let inset = (4. * s.scale) as i32;
                    rect.left += inset;
                    rect.right -= inset;
                    rect.top += inset;
                    rect.bottom -= inset;
                    fill_rounded(d.hDC, rect, Color::rgba(r, g, b, 255), 4. * s.scale);
                }
            } else {
                let ink = if disabled {
                    theme.text_tertiary
                } else if id == SAVE && !pressed {
                    theme.on_accent
                } else if id == RESET {
                    theme.text_secondary
                } else {
                    theme.text
                };
                let mut rect = d.rcItem;
                let align = if id == ADVANCED {
                    rect.left += (12. * s.scale) as i32;
                    DT_LEFT
                } else {
                    DT_CENTER
                };
                draw_label(d.hDC, &value(hwnd, id), rect, s.font, ink, align);
                if id == ADVANCED {
                    rect.left = rect.right - (32. * s.scale) as i32;
                    draw_label(
                        d.hDC,
                        if s.advanced && !s.panel_page {
                            "⌃"
                        } else {
                            "›"
                        },
                        rect,
                        s.font,
                        theme.text_secondary,
                        DT_CENTER,
                    );
                }
                if selected {
                    let r = d.rcItem;
                    let width = (20. * s.scale) as i32;
                    fill_rounded(
                        d.hDC,
                        RECT {
                            left: (r.left + r.right - width) / 2,
                            right: (r.left + r.right + width) / 2,
                            top: r.bottom - (3. * s.scale) as i32,
                            bottom: r.bottom - (1. * s.scale) as i32,
                        },
                        theme.accent,
                        s.scale,
                    );
                }
            }
            if d.itemState.0 & 0x10 != 0 {
                let mut r = d.rcItem;
                let inset = (2. * s.scale) as i32;
                r.left += inset;
                r.right -= inset;
                r.top += inset;
                r.bottom -= inset;
                let brush = CreateSolidBrush(native_color(theme.accent));
                FrameRect(d.hDC, &r, brush);
                let _ = DeleteObject(brush);
            }
            LRESULT(1)
        }
        WM_COMMAND => {
            if !GetPropW(hwnd, w!("QuotaBarUpdating")).is_invalid() {
                return LRESULT(0);
            }
            let id = wp.0 & 0xffff;
            let code = (wp.0 >> 16) as u32;
            if [EN_SETFOCUS, CBN_SETFOCUS, BN_SETFOCUS].contains(&code) {
                if let Ok(child) = GetDlgItem(hwnd, id as i32) {
                    let mut r = RECT::default();
                    let mut client = RECT::default();
                    let _ = GetWindowRect(child, &mut r);
                    let _ = GetClientRect(hwnd, &mut client);
                    let mut pt = POINT {
                        x: r.left,
                        y: r.top,
                    };
                    let _ = ScreenToClient(hwnd, &mut pt);
                    let bottom = pt.y + r.bottom - r.top;
                    let requested = if pt.y < 0 {
                        s.scroll + pt.y
                    } else if bottom > client.bottom {
                        s.scroll + bottom - client.bottom
                    } else {
                        s.scroll
                    };
                    scroll_to(hwnd, &mut s, requested);
                }
                return LRESULT(0);
            }
            if [
                SAVE,
                CANCEL,
                RESET,
                ADVANCED,
                RINGS,
                BARS,
                WEEKLY,
                FIVE,
                BOTH,
                CHOOSE_RING,
                CHOOSE_SESSION,
                CHOOSE_NUMBER,
            ]
            .contains(&id)
                && code != BN_CLICKED
            {
                return LRESULT(0);
            }
            if id == CANCEL || id == SAVE {
                if id == SAVE {
                    let Some(draft) = read(hwnd, &s.draft) else {
                        label(hwnd, STATUS, t("Check colors and ranges."));
                        return LRESULT(0);
                    };
                    let _ = s
                        .proxy
                        .send_event(UserEvent::PanelSave(s.panel_draft.clone()));
                    let _ = s.proxy.send_event(UserEvent::AppearanceSave(draft));
                } else {
                    let _ = s.proxy.send_event(UserEvent::PanelCancel);
                    let _ = s.proxy.send_event(UserEvent::AppearanceCancel);
                }
                drop(s);
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }
            if [400, 401].contains(&id) {
                s.panel_page = id == 401;
                s.scroll = 0;
                layout(hwnd, &mut s, true);
                return LRESULT(0);
            }
            if (410..440).contains(&id) && code == BN_CLICKED {
                if (410..415).contains(&id) {
                    let i = id - 410;
                    s.panel_draft.modules[i].visible = !s.panel_draft.modules[i].visible;
                }
                if (420..425).contains(&id) {
                    let i = id - 420;
                    s.panel_draft.move_to(i, i.saturating_sub(1));
                }
                if (430..435).contains(&id) {
                    let i = id - 430;
                    s.panel_draft.move_to(i, (i + 1).min(4));
                }
                panel_changed(hwnd, &s);
                return LRESULT(0);
            }
            if id == RESET && s.panel_page {
                s.panel_draft = crate::config::panel::PanelConfig::default();
                panel_changed(hwnd, &s);
                return LRESULT(0);
            }
            if id == ADVANCED {
                s.advanced = !s.advanced;
                layout(hwnd, &mut s, true);
                return LRESULT(0);
            }
            if [RINGS, BARS, WEEKLY, FIVE, BOTH].contains(&id) {
                let Some(current) = read(hwnd, &s.draft) else {
                    label(hwnd, STATUS, t("Check colors and ranges."));
                    return LRESULT(0);
                };
                s.draft = current;
                match id {
                    RINGS => s.draft.display_style = DisplayStyle::Rings,
                    BARS => s.draft.display_style = DisplayStyle::Bars,
                    WEEKLY => s.draft.periods = QuotaPeriods::Weekly,
                    FIVE => s.draft.periods = QuotaPeriods::FiveHours,
                    BOTH => s.draft.periods = QuotaPeriods::Both,
                    _ => {}
                }
                publish(hwnd, &s);
                fill(hwnd, &s.draft);
            }
            if id == RESET {
                s.draft = Appearance::default();
                publish(hwnd, &s);
                fill(hwnd, &s.draft);
            }
            if id == SIZE && code == CBN_SELCHANGE {
                let n = SendMessageW(
                    GetDlgItem(hwnd, SIZE as i32).unwrap_or_default(),
                    CB_GETCURSEL,
                    WPARAM(0),
                    LPARAM(0),
                )
                .0;
                if (0..3).contains(&n) {
                    let (diameter, font, width) =
                        [(24, 16, 64), (28, 20, 88), (36, 24, 112)][n as usize];
                    s.draft.ring_size = diameter;
                    s.draft.number_size = font;
                    s.draft.bar_width = width;
                    s.draft.number_x = s.draft.ring_x + diameter + 7;
                    s.draft.normalize();
                    publish(hwnd, &s);
                    fill(hwnd, &s.draft);
                    SendMessageW(
                        GetDlgItem(hwnd, SIZE as i32).unwrap_or_default(),
                        CB_SETCURSEL,
                        WPARAM(n as usize),
                        LPARAM(0),
                    );
                }
            }
            if let Some(target) = match id {
                CHOOSE_RING => Some(RING_COLOR),
                CHOOSE_SESSION => Some(SESSION_COLOR),
                CHOOSE_NUMBER => Some(NUMBER_COLOR),
                _ => None,
            } {
                choose(hwnd, target);
                // The modal picker pumps theme/DPI messages; use the current
                // handles and draft after it closes, never the pre-dialog copy.
                let Some(current) = snapshot(hwnd) else {
                    return LRESULT(0);
                };
                s = current;
            }
            if code == EN_CHANGE || code == CBN_SELCHANGE || code == BN_CLICKED {
                if let Some(draft) = read(hwnd, &s.draft) {
                    s.draft = draft;
                    publish(hwnd, &s);
                    label(hwnd, STATUS, "");
                    let _ = s
                        .proxy
                        .send_event(UserEvent::AppearancePreview(s.draft.clone()));
                    let _ = RedrawWindow(
                        hwnd,
                        None,
                        None,
                        RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
                    );
                } else {
                    label(hwnd, STATUS, t("Check colors and ranges."));
                }
            }
            LRESULT(0)
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
        _ => {
            drop(s);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
    }
}

/// Present the background and both previews together, after rendering offscreen.
unsafe fn paint_window(hwnd: HWND, dc: HDC, s: &State) {
    let mut client = RECT::default();
    if GetClientRect(hwnd, &mut client).is_err() || client.right <= 0 || client.bottom <= 0 {
        return;
    }
    let memory = CreateCompatibleDC(dc);
    let bitmap = CreateCompatibleBitmap(dc, client.right, client.bottom);
    if memory.is_invalid() || bitmap.is_invalid() {
        if !memory.is_invalid() {
            let _ = DeleteDC(memory);
        }
        if !bitmap.is_invalid() {
            let _ = DeleteObject(bitmap);
        }
        FillRect(dc, &client, s.background);
        paint(dc, s);
        return;
    }
    let previous = SelectObject(memory, bitmap);
    FillRect(memory, &client, s.background);
    paint(memory, s);
    let _ = BitBlt(dc, 0, 0, client.right, client.bottom, memory, 0, 0, SRCCOPY);
    SelectObject(memory, previous);
    let _ = DeleteObject(bitmap);
    let _ = DeleteDC(memory);
}

unsafe fn paint(dc: HDC, s: &State) {
    if s.panel_page {
        paint_panel(dc, s);
        return;
    }
    let mut quotas = super::quota_view::selected(&[], &s.draft);
    for q in &mut quotas {
        q.remaining = Some(if q.window == crate::providers::MetricWindow::Long {
            68.
        } else {
            93.
        });
        q.stale = false;
        q.estimated = false;
    }
    for (light, x) in [(true, 24.), (false, 268.)] {
        let rect = RECT {
            left: (x * s.scale) as i32,
            top: (136. * s.scale) as i32 - s.scroll,
            right: ((x + 228.) * s.scale) as i32,
            bottom: (188. * s.scale) as i32 - s.scroll,
        };
        let surface = SurfaceTheme::new(light).secondary;
        let brush = CreateSolidBrush(native_color(surface));
        rounded(
            dc,
            rect,
            brush,
            (2. * UiMetrics::SURFACE_RADIUS * s.scale) as i32,
        );
        let _ = DeleteObject(brush);
        let width = super::quota_view::width(&s.draft).ceil() as u32;
        let factor = s.scale.min(212. * s.scale / width as f32);
        let panel = super::quota_view::render(
            (width as f32 * s.scale).ceil() as u32,
            (48. * s.scale).round() as u32,
            &quotas,
            light,
            &s.draft,
        );
        let mut bg = tiny_skia::Pixmap::new(panel.width(), panel.height()).unwrap();
        bg.fill(surface.to_skia_color());
        bg.draw_pixmap(
            0,
            0,
            panel.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            tiny_skia::Transform::identity(),
            None,
        );
        let mut bytes = bg.data().to_vec();
        for p in bytes.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: bg.width() as i32,
                biHeight: -(bg.height() as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let dw = (width as f32 * factor).round() as i32;
        let dh = (48. * factor).round() as i32;
        StretchDIBits(
            dc,
            rect.left + (rect.right - rect.left - dw) / 2,
            rect.top + (rect.bottom - rect.top - dh) / 2,
            dw,
            dh,
            0,
            0,
            bg.width() as i32,
            bg.height() as i32,
            Some(bytes.as_ptr().cast()),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }
}

unsafe fn fill_panel(hwnd: HWND, s: &State) {
    for (i, m) in s.panel_draft.modules.iter().enumerate() {
        label(hwnd, 410 + i, t(m.id.title()));
        if let Ok(h) = GetDlgItem(hwnd, (410 + i) as i32) {
            SendMessageW(h, BM_SETCHECK, WPARAM(usize::from(m.visible)), LPARAM(0));
        }
        if let Ok(h) = GetDlgItem(hwnd, (420 + i) as i32) {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(h, i > 0);
        }
        if let Ok(h) = GetDlgItem(hwnd, (430 + i) as i32) {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(h, i < 4);
        }
    }
}
unsafe fn panel_changed(hwnd: HWND, s: &State) {
    publish(hwnd, s);
    fill_panel(hwnd, s);
    let _ = s
        .proxy
        .send_event(UserEvent::PanelPreview(s.panel_draft.clone()));
    let _ = RedrawWindow(hwnd, None, None, RDW_INVALIDATE | RDW_ALLCHILDREN);
}
unsafe fn paint_panel(dc: HDC, s: &State) {
    let theme = SurfaceTheme::new(s.light);
    let r = |x: f32, y: f32, w: f32, h: f32| RECT {
        left: (x * s.scale) as i32,
        top: (y * s.scale) as i32 - s.scroll,
        right: ((x + w) * s.scale) as i32,
        bottom: ((y + h) * s.scale) as i32 - s.scroll,
    };
    draw_label(
        dc,
        t("Show and arrange modules"),
        r(24., 84., 300., 24.),
        s.font,
        theme.text,
        DT_LEFT,
    );
    draw_label(
        dc,
        t("Preview"),
        r(346., 84., 150., 24.),
        s.caption_font,
        theme.text_secondary,
        DT_LEFT,
    );
    for i in 0..5 {
        draw_label(
            dc,
            "⠿",
            r(20., 116. + i as f32 * 52., 24., 32.),
            s.font,
            theme.text_secondary,
            DT_CENTER,
        );
    }
    if let Some(row) = s.drop_row {
        fill_rounded(
            dc,
            r(20., 112. + row as f32 * 52., 304., 3.),
            theme.accent,
            1.,
        );
    }
    fill_rounded(dc, r(340., 116., 156., 330.), theme.surface, 8. * s.scale);
    draw_label(
        dc,
        "QuotaBar",
        r(352., 128., 132., 26.),
        s.font,
        theme.text,
        DT_LEFT,
    );
    let mut y = 166.;
    for m in s.panel_draft.modules.iter().filter(|m| m.visible) {
        draw_label(
            dc,
            t(m.id.title()),
            r(352., y, 132., 24.),
            s.caption_font,
            theme.text,
            DT_LEFT,
        );
        fill_rounded(
            dc,
            r(352., y + 27., 132., 4.),
            theme.secondary,
            2. * s.scale,
        );
        fill_rounded(dc, r(352., y + 27., 88., 4.), theme.accent, 2. * s.scale);
        y += 46.;
    }
    if !s.panel_draft.modules.iter().any(|m| m.visible) {
        draw_label(
            dc,
            t("All modules hidden"),
            r(352., 166., 132., 30.),
            s.caption_font,
            theme.text_secondary,
            DT_LEFT,
        );
    }
    draw_label(
        dc,
        t("Settings"),
        r(352., 416., 132., 24.),
        s.caption_font,
        theme.text_secondary,
        DT_LEFT,
    );
    draw_label(
        dc,
        t("Drag the handle or use the arrows."),
        r(24., 391., 300., 24.),
        s.caption_font,
        theme.text_secondary,
        DT_LEFT,
    );
    draw_label(
        dc,
        t("Hidden modules keep their position."),
        r(24., 419., 300., 24.),
        s.caption_font,
        theme.text_secondary,
        DT_LEFT,
    );
    draw_label(
        dc,
        t("History recording continues while hidden."),
        r(24., 447., 472., 24.),
        s.caption_font,
        theme.text_secondary,
        DT_LEFT,
    );
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use tao::{
        event::Event,
        event_loop::{ControlFlow, EventLoopBuilder},
        platform::{run_return::EventLoopExtRunReturn, windows::EventLoopBuilderExtWindows},
    };

    #[test]
    fn stationary_scroll_does_not_schedule_a_redraw() {
        // Basic settings fit: wheel input in either direction is a no-op.
        for requested in [-96, -48, 0, 48, 96] {
            assert_eq!(scroll_destination(0, requested, 548, 548), None);
            assert_eq!(scroll_destination(0, requested, 548, 800), None);
        }
        // Repeated wheel events at the top/bottom, and end-scroll notifications.
        assert_eq!(scroll_destination(0, -48, 788, 548), None);
        assert_eq!(scroll_destination(240, 288, 788, 548), None);
        assert_eq!(scroll_destination(120, 120, 788, 548), None);
    }

    #[test]
    fn scrolling_clamps_to_the_viewport_at_each_supported_scale() {
        for scale in [1.0f32, 1.25, 1.5, 2.0] {
            let content = (788. * scale).round() as i32;
            let viewport = (548. * scale).round() as i32;
            let step = (48. * scale) as i32;
            let maximum = content - viewport;
            assert_eq!(scroll_destination(0, step, content, viewport), Some(step));
            assert_eq!(scroll_destination(step, -step, content, viewport), Some(0));
            assert_eq!(
                scroll_destination(step, content, content, viewport),
                Some(maximum)
            );
            assert_eq!(
                scroll_destination(maximum, content, content, viewport),
                None
            );
            // After the work area grows, an old offset must return to zero.
            assert_eq!(
                scroll_destination(maximum, maximum, content, content),
                Some(0)
            );
        }
    }

    pub(crate) unsafe fn capture_offscreen(hwnd: HWND, path: &std::path::Path) {
        let mut r = RECT::default();
        GetClientRect(hwnd, &mut r).unwrap();
        let dc = GetDC(hwnd);
        let memory = CreateCompatibleDC(dc);
        let bitmap = CreateCompatibleBitmap(dc, r.right, r.bottom);
        let old = SelectObject(memory, bitmap);
        SendMessageW(
            hwnd,
            WM_PRINTCLIENT,
            WPARAM(memory.0 as usize),
            LPARAM(PRF_CLIENT as isize),
        );
        unsafe extern "system" fn child_paint(child: HWND, lp: LPARAM) -> BOOL {
            let (parent, dc) = *(lp.0 as *const (HWND, HDC));
            if GetWindowLongW(child, GWL_STYLE) as u32 & WS_VISIBLE.0 == 0 {
                return BOOL(1);
            }
            let mut origin = POINT::default();
            let _ = ClientToScreen(child, &mut origin);
            let _ = ScreenToClient(parent, &mut origin);
            let saved = SaveDC(dc);
            let _ = SetViewportOrgEx(dc, origin.x, origin.y, None);
            SendMessageW(
                child,
                WM_PRINT,
                WPARAM(dc.0 as usize),
                LPARAM(PRF_CLIENT as isize),
            );
            let _ = RestoreDC(dc, saved);
            BOOL(1)
        }
        let context = (hwnd, memory);
        let _ = EnumChildWindows(
            hwnd,
            Some(child_paint),
            LPARAM(&context as *const _ as isize),
        );
        SelectObject(memory, old);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: r.right,
                biHeight: -r.bottom,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bytes = vec![0u8; (r.right * r.bottom * 4) as usize];
        assert_ne!(
            GetDIBits(
                memory,
                bitmap,
                0,
                r.bottom as u32,
                Some(bytes.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS
            ),
            0
        );
        for p in bytes.chunks_exact_mut(4) {
            p.swap(0, 2);
            p[3] = 255;
        }
        tiny_skia::Pixmap::from_vec(
            bytes,
            tiny_skia::IntSize::from_wh(r.right as u32, r.bottom as u32).unwrap(),
        )
        .unwrap()
        .save_png(path)
        .unwrap();
        let _ = DeleteObject(bitmap);
        let _ = DeleteDC(memory);
        ReleaseDC(hwnd, dc);
    }

    pub(crate) unsafe fn capture(hwnd: HWND, path: &std::path::Path) {
        // Drain normal WM_PAINT delivery, never invalidate/repair the window.
        let mut message = MSG::default();
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
        let mut r = RECT::default();
        GetClientRect(hwnd, &mut r).unwrap();
        let dc = GetDC(hwnd);
        let memory = CreateCompatibleDC(dc);
        let bitmap = CreateCompatibleBitmap(dc, r.right, r.bottom);
        let old = SelectObject(memory, bitmap);
        let mut class = [0u16; 32];
        let len = GetClassNameW(hwnd, &mut class);
        if String::from_utf16_lossy(&class[..len.max(0) as usize]) == "#32768" {
            // Native menus are composed by the shell; their window DC may
            // expose the surface underneath. Capture their on-screen pixels.
            let mut origin = POINT::default();
            let _ = ClientToScreen(hwnd, &mut origin);
            let screen = GetDC(None);
            BitBlt(
                memory,
                0,
                0,
                r.right,
                r.bottom,
                screen,
                origin.x,
                origin.y,
                SRCCOPY | CAPTUREBLT,
            )
            .unwrap();
            ReleaseDC(None, screen);
        } else {
            BitBlt(memory, 0, 0, r.right, r.bottom, dc, 0, 0, SRCCOPY).unwrap();
        }
        SelectObject(memory, old);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: r.right,
                biHeight: -r.bottom,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bytes = vec![0u8; (r.right * r.bottom * 4) as usize];
        assert_ne!(
            GetDIBits(
                memory,
                bitmap,
                0,
                r.bottom as u32,
                Some(bytes.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS
            ),
            0
        );
        for p in bytes.chunks_exact_mut(4) {
            p.swap(0, 2);
            p[3] = 255;
        }
        tiny_skia::Pixmap::from_vec(
            bytes,
            tiny_skia::IntSize::from_wh(r.right as u32, r.bottom as u32).unwrap(),
        )
        .unwrap()
        .save_png(path)
        .unwrap();
        let _ = DeleteObject(bitmap);
        let _ = DeleteDC(memory);
        ReleaseDC(hwnd, dc);
    }

    thread_local! { static MENU_CASE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
    unsafe extern "system" fn capture_popup(hwnd: HWND, lp: LPARAM) -> BOOL {
        let mut name = [0u16; 32];
        let n = GetClassNameW(hwnd, &mut name);
        if String::from_utf16_lossy(&name[..n.max(0) as usize]) == "#32768"
            && IsWindowVisible(hwnd).as_bool()
        {
            let count = &mut *(lp.0 as *mut usize);
            let tag = if crate::i18n::is_chinese() {
                "zh"
            } else {
                "en"
            };
            let theme = crate::ui::native_menu::test_theme_name();
            let case = MENU_CASE.with(|v| v.get());
            let path = std::path::Path::new("target/quotabar-preview")
                .join(format!("menu-{tag}-{theme}-case{case}-popup{count}.png"));
            capture(hwnd, &path);
            crate::ui::native_menu::test_assert_painted(
                hwnd,
                &tiny_skia::Pixmap::load_png(&path).unwrap(),
            );
            *count += 1;
        }
        BOOL(1)
    }
    pub(super) unsafe extern "system" fn capture_menu_timer(hwnd: HWND, _: u32, id: usize, _: u32) {
        let _ = KillTimer(hwnd, id);
        let mut count = 0usize;
        let _ = EnumThreadWindows(
            windows::Win32::System::Threading::GetCurrentThreadId(),
            Some(capture_popup),
            LPARAM((&mut count as *mut usize) as isize),
        );
        assert!(count > 0);
        if MENU_CASE.with(|v| v.get()) > 0 {
            assert!(count > 1, "submenu must actually open");
        }
        let _ = EndMenu();
    }

    #[test]
    #[ignore = "opens real native menus and captures their production drawing"]
    fn native_context_menu_visual_check() {
        use muda::ContextMenu as _;
        let event_loop = EventLoopBuilder::<UserEvent>::with_user_event()
            .with_any_thread(true)
            .build();
        let mut dialog = AppearanceDialog::new(event_loop.create_proxy());
        let dir = std::path::Path::new("target/quotabar-preview");
        std::fs::create_dir_all(dir).unwrap();
        for light in [true, false] {
            crate::ui::native_menu::test_theme(Some(light));
            for (lang, tag) in [
                (crate::config::schema::Language::English, "en"),
                (crate::config::schema::Language::Chinese, "zh"),
            ] {
                crate::i18n::set_language(lang);
                dialog.open(&Appearance::default()).unwrap();
                let menu = crate::ui::context_menu::ContextMenu::new(
                    &crate::config::schema::Config::default(),
                )
                .unwrap();
                unsafe {
                    // Keep this isolated test owner above unrelated desktop apps.
                    // GetDC captures visible pixels, so an occluded menu is not a
                    // meaningful rendering assertion.
                    let _ = SetWindowPos(dialog.hwnd, HWND_TOPMOST, 40, 40, 0, 0, SWP_NOSIZE);
                    let _ = SetForegroundWindow(dialog.hwnd);
                    let paths =
                        crate::ui::native_menu::test_menu_paths(HMENU(menu.menu.hpopupmenu() as _));
                    assert!(
                        paths
                            .iter()
                            .any(|path| path.iter().filter(|&&key| key == 0x27).count() >= 3),
                        "the More menu must retain nested provider actions"
                    );
                    for (case, keys) in paths.into_iter().enumerate() {
                        MENU_CASE.with(|v| v.set(case));
                        let owner = dialog.hwnd.0 as isize;
                        let worker = std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_millis(180));
                            for key in keys {
                                let _ = PostMessageW(
                                    HWND(owner as _),
                                    WM_KEYDOWN,
                                    WPARAM(key),
                                    LPARAM(0),
                                );
                                std::thread::sleep(std::time::Duration::from_millis(50));
                            }
                            std::thread::sleep(std::time::Duration::from_millis(220));
                            SendMessageW(HWND(owner as _), WM_APP + 100, WPARAM(0), LPARAM(0));
                        });
                        menu.menu.show_context_menu_for_hwnd(
                            dialog.hwnd.0 as isize,
                            Some(muda::dpi::PhysicalPosition::new(80, 80).into()),
                        );
                        worker.join().unwrap();
                        assert!(dir
                            .join(format!(
                                "menu-{tag}-{}-case{case}-popup0.png",
                                crate::ui::native_menu::test_theme_name()
                            ))
                            .exists());
                    }
                    SendMessageW(dialog.hwnd, WM_COMMAND, WPARAM(CANCEL), LPARAM(0));
                }
            }
        }
        crate::ui::native_menu::test_theme(None);
    }

    #[test]
    #[ignore = "isolated hidden Win32 controls; no user config or foreground window"]
    fn native_panel_settings_hidden_controls() {
        let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event()
            .with_any_thread(true)
            .build();
        let original = crate::config::panel::PanelConfig::default();
        let mut dialog = AppearanceDialog::new(event_loop.create_proxy());
        crate::i18n::set_language(crate::config::schema::Language::Chinese);
        dialog
            .create_settings(&Appearance::default(), &original, true, false)
            .unwrap();
        unsafe {
            let h = dialog.hwnd;
            assert!(!IsWindowVisible(h).as_bool());
            assert_eq!(snapshot(h).unwrap().panel_draft, original);
            SendMessageW(h, WM_COMMAND, WPARAM(410), LPARAM(0));
            SendMessageW(h, WM_COMMAND, WPARAM(430), LPARAM(0));
            assert_eq!(
                snapshot(h).unwrap().panel_draft.modules[1].id,
                crate::config::panel::ModuleId::FiveHour
            );
            assert!(!snapshot(h).unwrap().panel_draft.modules[1].visible);
            let scale = snapshot(h).unwrap().scale;
            let point = |x: f32, y: f32| {
                LPARAM(
                    (((y * scale) as i32 as u32) << 16 | ((x * scale) as i32 as u32 & 0xffff))
                        as isize,
                )
            };
            SendMessageW(h, WM_LBUTTONDOWN, WPARAM(1), point(28., 130.));
            SendMessageW(h, WM_MOUSEMOVE, WPARAM(1), point(28., 336.));
            SendMessageW(h, WM_LBUTTONUP, WPARAM(0), point(28., 336.));
            assert_eq!(
                snapshot(h).unwrap().panel_draft.modules[4].id,
                crate::config::panel::ModuleId::Weekly
            );
            let dir = std::path::Path::new("target/quotabar-complete");
            std::fs::create_dir_all(dir).unwrap();
            capture_offscreen(h, &dir.join("panel-settings-zh.png"));
            SendMessageW(h, WM_COMMAND, WPARAM(RESET), LPARAM(0));
            assert_eq!(snapshot(h).unwrap().panel_draft, original);
            for i in 0..5 {
                SendMessageW(h, WM_COMMAND, WPARAM(410 + i), LPARAM(0));
            }
            assert!(snapshot(h)
                .unwrap()
                .panel_draft
                .modules
                .iter()
                .all(|m| !m.visible));
            capture_offscreen(h, &dir.join("panel-settings-hidden-zh.png"));
            SendMessageW(h, WM_COMMAND, WPARAM(CANCEL), LPARAM(0));
            dialog
                .create_settings(&Appearance::default(), &original, true, false)
                .unwrap();
            assert_eq!(snapshot(dialog.hwnd).unwrap().panel_draft, original);
            SendMessageW(dialog.hwnd, WM_COMMAND, WPARAM(430), LPARAM(0));
            SendMessageW(dialog.hwnd, WM_COMMAND, WPARAM(SAVE), LPARAM(0));
        }
        let mut canceled = false;
        let mut saved = None;
        event_loop.run_return(|event, _, flow| match event {
            tao::event::Event::UserEvent(UserEvent::PanelCancel) => canceled = true,
            tao::event::Event::UserEvent(UserEvent::PanelSave(p)) => saved = Some(p),
            tao::event::Event::MainEventsCleared => *flow = ControlFlow::Exit,
            _ => {}
        });
        assert!(canceled);
        let saved = saved.unwrap();
        assert_eq!(
            saved.modules[1].id,
            crate::config::panel::ModuleId::FiveHour
        );
        assert_eq!(
            toml::from_str::<crate::config::panel::PanelConfig>(&toml::to_string(&saved).unwrap())
                .unwrap(),
            saved
        );
    }

    #[test]
    #[ignore = "opens an isolated native preferences window; writes screenshots without changing user config"]
    fn native_preferences_preview_save_cancel_and_dpi() {
        let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event()
            .with_any_thread(true)
            .build();
        let mut dialog = AppearanceDialog::new(event_loop.create_proxy());
        let initial = Appearance::default();
        let dir = std::path::Path::new("target/quotabar-preview");
        std::fs::create_dir_all(dir).unwrap();
        for (language, tag) in [
            (crate::config::schema::Language::English, "en"),
            (crate::config::schema::Language::Chinese, "zh"),
        ] {
            crate::i18n::set_language(language);
            dialog.open(&initial).unwrap();
            unsafe {
                let hwnd = dialog.hwnd;
                for light in [true, false] {
                    for dpi in [96usize, 120, 144, 192] {
                        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<State>;
                        {
                            let mut s = (*ptr).borrow_mut();
                            s.light = light;
                            let _ = DeleteObject(s.background);
                            let _ = DeleteObject(s.card);
                            let theme = SurfaceTheme::new(light);
                            s.background = CreateSolidBrush(native_color(theme.background));
                            s.card = CreateSolidBrush(native_color(theme.surface));
                        }
                        window_theme(hwnd, light);
                        let mut rect = RECT::default();
                        GetWindowRect(hwnd, &mut rect).unwrap();
                        SendMessageW(
                            hwnd,
                            WM_DPICHANGED,
                            WPARAM(dpi | (dpi << 16)),
                            LPARAM((&rect as *const RECT) as isize),
                        );
                        capture(
                            hwnd,
                            &dir.join(format!(
                                "settings-{tag}-{}-{dpi}.png",
                                if light { "light" } else { "dark" }
                            )),
                        );
                        let state = (*ptr).borrow();
                        let mut client = RECT::default();
                        GetClientRect(hwnd, &mut client).unwrap();
                        for c in state.controls.iter().filter(|c| !c.advanced) {
                            let mut r = RECT::default();
                            GetWindowRect(c.hwnd, &mut r).unwrap();
                            assert!(
                                r.right - r.left <= client.right,
                                "control wider than window"
                            );
                            assert!(
                                (c.x + c.w) * state.scale <= client.right as f32 + 1.,
                                "horizontal clipping: id={}, right={}, client={}, dpi={}",
                                GetDlgCtrlID(c.hwnd),
                                (c.x + c.w) * state.scale,
                                client.right,
                                dpi
                            );
                        }
                    }
                }
                // A restricted work area must scroll the focused footer into view.
                let mut window = RECT::default();
                GetWindowRect(hwnd, &mut window).unwrap();
                SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    window.right - window.left,
                    640,
                    SWP_NOMOVE | SWP_NOZORDER,
                )
                .unwrap();
                layout(hwnd, &mut snapshot(hwnd).unwrap(), false);
                SendMessageW(
                    hwnd,
                    WM_COMMAND,
                    WPARAM(SAVE | ((BN_SETFOCUS as usize) << 16)),
                    LPARAM(0),
                );
                let mut save = RECT::default();
                GetWindowRect(GetDlgItem(hwnd, SAVE as i32).unwrap(), &mut save).unwrap();
                let mut bottom = POINT {
                    x: save.left,
                    y: save.bottom,
                };
                let _ = ScreenToClient(hwnd, &mut bottom);
                let mut client = RECT::default();
                GetClientRect(hwnd, &mut client).unwrap();
                assert!(
                    bottom.y <= client.bottom,
                    "focused Save must stay reachable on short work areas"
                );
                layout(hwnd, &mut snapshot(hwnd).unwrap(), true);
                capture(hwnd, &dir.join(format!("settings-{tag}.png")));
                SendMessageW(hwnd, WM_COMMAND, WPARAM(BARS), LPARAM(0));
                SendMessageW(hwnd, WM_COMMAND, WPARAM(BOTH), LPARAM(0));
                SendMessageW(hwnd, WM_COMMAND, WPARAM(ADVANCED), LPARAM(0));
                assert_eq!(
                    read(
                        hwnd,
                        &(*((GetWindowLongPtrW(hwnd, GWLP_USERDATA)) as *const RefCell<State>))
                            .borrow()
                            .draft
                    )
                    .unwrap()
                    .periods,
                    QuotaPeriods::Both
                );
                capture(hwnd, &dir.join(format!("settings-{tag}-advanced.png")));
                label(hwnd, RING_COLOR, "invalid");
                SendMessageW(hwnd, WM_COMMAND, WPARAM(SAVE), LPARAM(0));
                assert!(
                    IsWindow(hwnd).as_bool(),
                    "invalid input must not commit or close"
                );
                label(hwnd, RING_COLOR, &initial.ring_color);
                let mut rect = RECT::default();
                GetWindowRect(hwnd, &mut rect).unwrap();
                for dpi in [96usize, 120, 144, 192] {
                    SendMessageW(
                        hwnd,
                        WM_DPICHANGED,
                        WPARAM(dpi | (dpi << 16)),
                        LPARAM((&rect as *const RECT) as isize),
                    );
                    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<State>;
                    assert_eq!((*ptr).borrow().scale, dpi as f32 / 96.0);
                }
                let enter = MSG {
                    hwnd: GetDlgItem(hwnd, RING_COLOR as i32).unwrap(),
                    message: WM_KEYDOWN,
                    wParam: WPARAM(0x0d),
                    ..Default::default()
                };
                assert!(handle_message((&enter as *const MSG).cast()));
                assert!(
                    !IsWindow(hwnd).as_bool(),
                    "Enter must commit valid settings"
                );
            }
            dialog.open(&initial).unwrap();
            unsafe {
                SendMessageW(dialog.hwnd, WM_COMMAND, WPARAM(FIVE), LPARAM(0));
                let key = MSG {
                    hwnd: GetDlgItem(dialog.hwnd, RING_COLOR as i32).unwrap(),
                    message: WM_KEYDOWN,
                    wParam: WPARAM(0x1b),
                    ..Default::default()
                };
                assert!(handle_message((&key as *const MSG).cast()));
                assert!(!IsWindow(dialog.hwnd).as_bool());
            }
        }
        // Warm UI caches above, then verify repeated close cycles release native objects.
        unsafe {
            use windows::Win32::System::Threading::{
                GetCurrentProcess, GetGuiResources, GR_GDIOBJECTS,
            };
            let before = GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS);
            for _ in 0..10 {
                dialog.open(&initial).unwrap();
                SendMessageW(dialog.hwnd, WM_COMMAND, WPARAM(CANCEL), LPARAM(0));
            }
            let after = GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS);
            assert!(
                after <= before + 2,
                "GDI objects grew from {before} to {after}"
            );
        }
        let mut saves = Vec::new();
        let mut canceled = 0;
        let mut previews = 0;
        event_loop.run_return(|event, _, flow| {
            *flow = ControlFlow::Poll;
            match event {
                Event::UserEvent(UserEvent::AppearanceSave(s)) => saves.push(s),
                Event::UserEvent(UserEvent::AppearanceCancel) => canceled += 1,
                Event::UserEvent(UserEvent::AppearancePreview(_)) => previews += 1,
                Event::MainEventsCleared => *flow = ControlFlow::Exit,
                _ => {}
            }
        });
        assert_eq!(saves.len(), 2);
        assert_eq!(canceled, 12);
        assert!(previews >= 6);
        for s in saves {
            assert_eq!(s.display_style, DisplayStyle::Bars);
            assert_eq!(s.periods, QuotaPeriods::Both);
        }
        assert_eq!(initial, Appearance::default());
    }
}
