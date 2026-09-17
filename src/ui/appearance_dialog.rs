//! Modeless native appearance editor. Preview is transient; Save is explicit.
use crate::{
    app::UserEvent,
    config::appearance::{rgb, Appearance, NumberWeight},
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
        UI::{Controls::Dialogs::*, HiDpi::*, WindowsAndMessaging::*},
    },
};

const SAVE: usize = 1;
const CANCEL: usize = 2;
const RESET: usize = 3;
const RING_COLOR: usize = 100;
const NUMBER_COLOR: usize = 101;
const WEIGHT: usize = 110;
const STATUS: usize = 120;
const CHOOSE_RING: usize = 130;
const CHOOSE_NUMBER: usize = 131;
const IDS: [usize; 8] = [102, 103, 104, 105, 106, 107, 108, 109];

pub(crate) struct AppearanceDialog {
    hwnd: HWND,
    proxy: EventLoopProxy<UserEvent>,
}
struct State {
    proxy: EventLoopProxy<UserEvent>,
    draft: Appearance,
    scale: f32,
    font: HFONT,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
unsafe fn label(hwnd: HWND, id: usize, text: &str) {
    let value = wide(text);
    let _ = SetWindowTextW(
        GetDlgItem(hwnd, id as i32).unwrap_or_default(),
        PCWSTR(value.as_ptr()),
    );
}
unsafe fn value(hwnd: HWND, id: usize) -> String {
    let mut buffer = [0u16; 128];
    let len = GetWindowTextW(GetDlgItem(hwnd, id as i32).unwrap_or_default(), &mut buffer);
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}
unsafe fn read(hwnd: HWND) -> Option<Appearance> {
    let mut values = Vec::new();
    for id in IDS {
        values.push(value(hwnd, id).trim().parse::<i32>().ok()?);
    }
    let selected = SendMessageW(
        GetDlgItem(hwnd, WEIGHT as i32).ok()?,
        CB_GETCURSEL,
        WPARAM(0),
        LPARAM(0),
    )
    .0;
    let style = Appearance {
        ring_color: value(hwnd, RING_COLOR).trim().to_string(),
        number_color: value(hwnd, NUMBER_COLOR).trim().to_string(),
        ring_size: values[0],
        ring_thickness: values[1],
        ring_x: values[2],
        ring_y: values[3],
        number_size: values[4],
        number_x: values[5],
        number_y: values[6],
        symbol_percent: values[7],
        number_weight: match selected {
            0 => NumberWeight::Regular,
            2 => NumberWeight::Bold,
            _ => NumberWeight::Semibold,
        },
    };
    style.validate().then_some(style)
}
unsafe fn fill(hwnd: HWND, style: &Appearance) {
    label(hwnd, RING_COLOR, &style.ring_color);
    label(hwnd, NUMBER_COLOR, &style.number_color);
    for (id, n) in IDS.into_iter().zip([
        style.ring_size,
        style.ring_thickness,
        style.ring_x,
        style.ring_y,
        style.number_size,
        style.number_x,
        style.number_y,
        style.symbol_percent,
    ]) {
        label(hwnd, id, &n.to_string());
    }
    let selected = match style.number_weight {
        NumberWeight::Regular => 0,
        NumberWeight::Semibold => 1,
        NumberWeight::Bold => 2,
    };
    SendMessageW(
        GetDlgItem(hwnd, WEIGHT as i32).unwrap_or_default(),
        CB_SETCURSEL,
        WPARAM(selected),
        LPARAM(0),
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
    pub fn open(&mut self, appearance: &Appearance) -> anyhow::Result<()> {
        tracing::debug!("appearance: open requested");
        unsafe {
            if IsWindow(self.hwnd).as_bool() {
                let _ = SetForegroundWindow(self.hwnd);
                return Ok(());
            }
            let instance = GetModuleHandleW(None)?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: instance.into(),
                lpszClassName: w!("AiLimitsAppearance"),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as _),
                ..Default::default()
            };
            RegisterClassW(&class);
            let title = wide(t("Appearance settings…"));
            self.hwnd = CreateWindowExW(
                WS_EX_CONTROLPARENT,
                class.lpszClassName,
                PCWSTR(title.as_ptr()),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                720,
                580,
                None,
                None,
                instance,
                None,
            )?;
            let scale = GetDpiForWindow(self.hwnd).max(96) as f32 / 96.0;
            let font = CreateFontW(
                -(16.0 * scale).round() as i32,
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                DEFAULT_CHARSET.0 as u32,
                OUT_DEFAULT_PRECIS.0 as u32,
                CLIP_DEFAULT_PRECIS.0 as u32,
                CLEARTYPE_QUALITY.0 as u32,
                0,
                w!("Segoe UI"),
            );
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: (720.0 * scale) as i32,
                bottom: (548.0 * scale) as i32,
            };
            let _ = AdjustWindowRectExForDpi(
                &mut rect,
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
                false,
                WS_EX_CONTROLPARENT,
                (96.0 * scale) as u32,
            );
            let _ = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOMOVE | SWP_NOZORDER,
            );
            // Create controls before installing the state, so EN_CHANGE cannot preview partial initialization.
            let add = |class: PCWSTR,
                       text: &str,
                       id: usize,
                       x: f32,
                       y: f32,
                       width: f32,
                       height: f32,
                       extra: u32|
             -> anyhow::Result<()> {
                let text = wide(text);
                let child = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    class,
                    PCWSTR(text.as_ptr()),
                    WS_CHILD | WS_VISIBLE | WINDOW_STYLE(extra),
                    (x * scale) as i32,
                    (y * scale) as i32,
                    (width * scale) as i32,
                    (height * scale) as i32,
                    self.hwnd,
                    HMENU(id as _),
                    instance,
                    None,
                )?;
                SendMessageW(child, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
                Ok(())
            };
            add(w!("STATIC"), t("Ring"), 0, 20., 16., 320., 26., 0)?;
            add(w!("STATIC"), t("Numbers"), 0, 380., 16., 320., 26., 0)?;
            for (x, id, button) in [
                (20., RING_COLOR, CHOOSE_RING),
                (380., NUMBER_COLOR, CHOOSE_NUMBER),
            ] {
                add(w!("STATIC"), t("Color"), 0, x, 55., 138., 24., 0)?;
                add(
                    w!("EDIT"),
                    "",
                    id,
                    x + 140.,
                    52.,
                    100.,
                    26.,
                    WS_TABSTOP.0 | WS_BORDER.0 | ES_AUTOHSCROLL as u32,
                )?;
                add(
                    w!("BUTTON"),
                    t("Choose…"),
                    button,
                    x + 248.,
                    52.,
                    72.,
                    26.,
                    WS_TABSTOP.0,
                )?;
            }
            for (x, y, id, title) in [
                (20., 92., 102, "Diameter (12–44)"),
                (20., 132., 103, "Thickness (1–12)"),
                (20., 172., 104, "Horizontal (0–300)"),
                (20., 212., 105, "Vertical (−20–20)"),
                (380., 92., 106, "Size (10–40)"),
                (380., 172., 107, "Horizontal (0–300)"),
                (380., 212., 108, "Vertical (−20–20)"),
                (380., 252., 109, "Symbols % (40–100)"),
            ] {
                add(w!("STATIC"), t(title), 0, x, y + 3., 175., 24., 0)?;
                add(
                    w!("EDIT"),
                    "",
                    id,
                    x + 180.,
                    y,
                    140.,
                    26.,
                    WS_TABSTOP.0 | WS_BORDER.0 | ES_AUTOHSCROLL as u32,
                )?;
            }
            add(w!("STATIC"), t("Weight"), 0, 380., 135., 175., 24., 0)?;
            add(
                w!("COMBOBOX"),
                "",
                WEIGHT,
                560.,
                132.,
                140.,
                160.,
                WS_TABSTOP.0 | CBS_DROPDOWNLIST as u32 | WS_VSCROLL.0,
            )?;
            let combo = GetDlgItem(self.hwnd, WEIGHT as i32)?;
            for text in [t("Regular"), t("Semibold"), t("Bold")] {
                let text = wide(text);
                SendMessageW(
                    combo,
                    CB_ADDSTRING,
                    WPARAM(0),
                    LPARAM(text.as_ptr() as isize),
                );
            }
            add(
                w!("STATIC"),
                t("Colors: #RRGGBB; numbers also accept auto."),
                0,
                20.,
                294.,
                680.,
                24.,
                0,
            )?;
            add(w!("STATIC"),t("Offsets and sizes use display-independent pixels. Vertical movement stays inside the taskbar."),0,20.,320.,680.,40.,0)?;
            add(
                w!("STATIC"),
                t("Live preview · light / dark"),
                0,
                20.,
                365.,
                680.,
                24.,
                0,
            )?;
            add(w!("STATIC"), "", STATUS, 20., 461., 680., 32., 0)?;
            add(
                w!("BUTTON"),
                t("Restore defaults"),
                RESET,
                20.,
                506.,
                155.,
                30.,
                WS_TABSTOP.0,
            )?;
            add(
                w!("BUTTON"),
                t("Cancel"),
                CANCEL,
                490.,
                506.,
                95.,
                30.,
                WS_TABSTOP.0,
            )?;
            add(
                w!("BUTTON"),
                t("Save"),
                SAVE,
                605.,
                506.,
                95.,
                30.,
                WS_TABSTOP.0 | BS_DEFPUSHBUTTON as u32,
            )?;
            fill(self.hwnd, appearance);
            let state = Box::new(RefCell::new(State {
                proxy: self.proxy.clone(),
                draft: appearance.clone(),
                scale,
                font,
            }));
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
            ACTIVE.with(|active| active.set(self.hwnd));
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.hwnd);
            tracing::debug!("appearance: window shown");
        }
        Ok(())
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RefCell<State>;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    if msg == WM_NCDESTROY {
        ACTIVE.with(|active| active.set(HWND::default()));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        let state = Box::from_raw(ptr);
        let _ = DeleteObject(state.borrow().font);
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let Ok(mut state) = (*ptr).try_borrow_mut() else {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    };
    match msg {
        WM_DPICHANGED => {
            let new_scale = (wparam.0 & 0xffff) as f32 / 96.0;
            let ratio = new_scale / state.scale;
            let mut description = LOGFONTW::default();
            GetObjectW(
                state.font,
                std::mem::size_of::<LOGFONTW>() as i32,
                Some((&mut description as *mut LOGFONTW).cast()),
            );
            description.lfHeight = (description.lfHeight as f32 * ratio).round() as i32;
            let new_font = CreateFontIndirectW(&description);
            struct Resize {
                parent: HWND,
                ratio: f32,
                font: HFONT,
                scale: f32,
            }
            unsafe extern "system" fn child(hwnd: HWND, data: LPARAM) -> BOOL {
                let data = &*(data.0 as *const Resize);
                let mut rect = RECT::default();
                let _ = GetWindowRect(hwnd, &mut rect);
                let mut point = POINT {
                    x: rect.left,
                    y: rect.top,
                };
                let _ = ScreenToClient(data.parent, &mut point);
                let height = if GetDlgCtrlID(hwnd) == WEIGHT as i32 {
                    (160.0 * data.scale) as i32
                } else {
                    ((rect.bottom - rect.top) as f32 * data.ratio).round() as i32
                };
                let _ = MoveWindow(
                    hwnd,
                    (point.x as f32 * data.ratio).round() as i32,
                    (point.y as f32 * data.ratio).round() as i32,
                    ((rect.right - rect.left) as f32 * data.ratio).round() as i32,
                    height,
                    true,
                );
                SendMessageW(hwnd, WM_SETFONT, WPARAM(data.font.0 as usize), LPARAM(1));
                BOOL(1)
            }
            let resize = Resize {
                parent: hwnd,
                ratio,
                font: new_font,
                scale: new_scale,
            };
            let _ = EnumChildWindows(
                hwnd,
                Some(child),
                LPARAM((&resize as *const Resize) as isize),
            );
            let _ = DeleteObject(state.font);
            state.font = new_font;
            state.scale = new_scale;
            let rect = &*(lparam.0 as *const RECT);
            let _ = SetWindowPos(
                hwnd,
                None,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            let _ = InvalidateRect(hwnd, None, true);
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => {
            let dc = HDC(wparam.0 as _);
            SetBkColor(dc, COLORREF(0xFFFFFF));
            SetTextColor(dc, COLORREF(0x181818));
            LRESULT(GetStockObject(WHITE_BRUSH).0 as isize)
        }
        WM_CLOSE => {
            tracing::debug!("appearance: close requested");
            let _ = state.proxy.send_event(UserEvent::AppearanceCancel);
            drop(state);
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xffff;
            let code = (wparam.0 >> 16) as u32;
            if id == CANCEL || id == SAVE {
                tracing::debug!("appearance: command {id}, code {code}");
                if id == SAVE {
                    let Some(style) = read(hwnd) else {
                        label(hwnd,STATUS,t("Check colors and ranges; thickness must be less than half the diameter."));
                        return LRESULT(0);
                    };
                    let _ = state.proxy.send_event(UserEvent::AppearanceSave(style));
                } else {
                    let _ = state.proxy.send_event(UserEvent::AppearanceCancel);
                }
                drop(state);
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }
            if id == RESET {
                fill(hwnd, &Appearance::default());
            }
            if id == CHOOSE_RING || id == CHOOSE_NUMBER {
                let target = if id == CHOOSE_RING {
                    RING_COLOR
                } else {
                    NUMBER_COLOR
                };
                let [r, g, b] = rgb(&value(hwnd, target)).unwrap_or([255, 55, 116]);
                let mut custom = [
                    0x007437ffu32,
                    0x00ff8030,
                    0x00ccd520,
                    0x0050bc28,
                    0x003099ff,
                    0x00ff60b0,
                    0x00ffffff,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                ];
                let mut choice = CHOOSECOLORW {
                    lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
                    hwndOwner: hwnd,
                    rgbResult: COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16),
                    lpCustColors: custom.as_mut_ptr().cast(),
                    Flags: CC_FULLOPEN | CC_RGBINIT,
                    ..Default::default()
                };
                if ChooseColorW(&mut choice).as_bool() {
                    let n = choice.rgbResult.0;
                    label(
                        hwnd,
                        target,
                        &format!(
                            "#{:02X}{:02X}{:02X}",
                            n & 255,
                            (n >> 8) & 255,
                            (n >> 16) & 255
                        ),
                    );
                }
            }
            if id == RESET
                || id == CHOOSE_RING
                || id == CHOOSE_NUMBER
                || code == EN_CHANGE
                || code == CBN_SELCHANGE
            {
                if let Some(style) = read(hwnd) {
                    label(hwnd, STATUS, "");
                    state.draft = style.clone();
                    let _ = state.proxy.send_event(UserEvent::AppearancePreview(style));
                    let _ = InvalidateRect(hwnd, None, true);
                } else {
                    label(hwnd,STATUS,t("Check colors and ranges; thickness must be less than half the diameter."));
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            let mut quota = super::weekly::WeeklyQuota::from_providers(&[]);
            quota.remaining = Some(68.);
            quota.stale = false;
            quota.estimated = false;
            let width = super::taskbar_panel::appearance_width(&state.draft).ceil() as u32;
            for (light, y) in [(true, 394.), (false, 425.)] {
                let panel = super::taskbar_panel::render_styled_panel(
                    width,
                    48,
                    &quota,
                    light,
                    &state.draft,
                );
                let mut bg = tiny_skia::Pixmap::new(width, 48).unwrap();
                bg.fill(if light {
                    tiny_skia::Color::from_rgba8(242, 242, 246, 255)
                } else {
                    tiny_skia::Color::from_rgba8(28, 28, 32, 255)
                });
                bg.draw_pixmap(
                    0,
                    0,
                    panel.as_ref(),
                    &tiny_skia::PixmapPaint::default(),
                    tiny_skia::Transform::identity(),
                    None,
                );
                let mut bytes = bg.data().to_vec();
                for pixel in bytes.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                }
                let info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: width as i32,
                        biHeight: -48,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                StretchDIBits(
                    dc,
                    (20. * state.scale) as i32,
                    (y * state.scale) as i32,
                    (width as f32 * 0.6 * state.scale) as i32,
                    (28.8 * state.scale) as i32,
                    0,
                    0,
                    width as i32,
                    48,
                    Some(bytes.as_ptr().cast()),
                    &info,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        _ => {
            drop(state);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
    }
}
