//! Presentation-only owner drawing over muda's native HMENU tree.
//! Windows retains dismissal, keyboard traversal, checks and command dispatch.
use super::native_controls::{draw_label, fill_rounded, native_color, sized_font};
use super::theme::{SurfaceTheme, UiMetrics};
use std::{cell::RefCell, collections::HashMap};
use windows::{
    core::PWSTR,
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::Threading::GetCurrentThreadId,
        UI::{
            Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT},
            HiDpi::GetDpiForWindow,
            Shell::{DefSubclassProc, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
};

#[cfg(test)]
thread_local! { static TEST_LIGHT: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
pub(crate) fn test_theme(light: Option<bool>) {
    TEST_LIGHT.with(|value| value.set(light));
}
#[cfg(test)]
pub(crate) fn test_theme_name() -> &'static str {
    if TEST_LIGHT.with(|v| v.get()).unwrap_or(false) {
        "light"
    } else {
        "dark"
    }
}
fn menu_theme() -> SurfaceTheme {
    #[cfg(test)]
    if let Some(light) = TEST_LIGHT.with(|v| v.get()) {
        return SurfaceTheme::new(light);
    }
    SurfaceTheme::new(crate::platform::apps_use_light_theme())
}

#[derive(Clone)]
struct Item {
    menu: HMENU,
    index: u32,
    text: String,
    separator: bool,
    submenu: bool,
    footer: bool,
}
#[derive(Default)]
struct Registry {
    next: usize,
    items: HashMap<usize, Item>,
    menus: HashMap<isize, HBRUSH>,
    hook: Option<HHOOK>,
    paint_hook: Option<HHOOK>,
    surface: Option<COLORREF>,
    retired_brushes: Vec<HBRUSH>,
}
thread_local! { static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default()); }
pub(crate) struct MenuStyle {
    keys: Vec<usize>,
    menus: Vec<HMENU>,
}
impl MenuStyle {
    pub fn new(root: isize) -> Self {
        let mut style = Self {
            keys: Vec::new(),
            menus: Vec::new(),
        };
        unsafe {
            REGISTRY.with(|registry| {
                let mut r = registry.borrow_mut();
                if r.hook.is_none() {
                    r.hook =
                        SetWindowsHookExW(WH_CALLWNDPROC, Some(hook), None, GetCurrentThreadId())
                            .ok();
                    r.paint_hook = SetWindowsHookExW(
                        WH_CALLWNDPROCRET,
                        Some(paint_hook),
                        None,
                        GetCurrentThreadId(),
                    )
                    .ok();
                }
            });
            // Fail safely to the untouched native menu if the thread hook is unavailable.
            if REGISTRY.with(|r| r.borrow().hook.is_some() && r.borrow().paint_hook.is_some()) {
                if let Err(error) = style.register(HMENU(root as _)) {
                    tracing::warn!(%error, "menu styling failed; restoring native menu");
                    drop(style);
                    return Self {
                        keys: Vec::new(),
                        menus: Vec::new(),
                    };
                }
            }
        }
        style
    }
    unsafe fn register(&mut self, menu: HMENU) -> windows::core::Result<()> {
        self.menus.push(menu);
        let theme = menu_theme();
        let brush = CreateSolidBrush(native_color(theme.surface));
        let info = MENUINFO {
            cbSize: std::mem::size_of::<MENUINFO>() as u32,
            fMask: MIM_BACKGROUND,
            hbrBack: brush,
            ..Default::default()
        };
        if let Err(error) = SetMenuInfo(menu, &info) {
            let _ = DeleteObject(brush);
            return Err(error);
        }
        REGISTRY.with(|r| {
            let mut registry = r.borrow_mut();
            registry.menus.insert(menu.0 as isize, brush);
            registry.surface = Some(native_color(theme.surface));
        });
        for index in 0..GetMenuItemCount(menu).max(0) as u32 {
            let mut text = [0u16; 512];
            let mut info = MENUITEMINFOW {
                cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
                fMask: MIIM_FTYPE | MIIM_STRING | MIIM_SUBMENU,
                dwTypeData: PWSTR(text.as_mut_ptr()),
                cch: 511,
                ..Default::default()
            };
            GetMenuItemInfoW(menu, index, true, &mut info)?;
            let text = String::from_utf16_lossy(&text[..info.cch as usize]);
            let submenu = info.hSubMenu;
            let item = Item {
                menu,
                index,
                footer: text.starts_with("QuotaBar v"),
                text,
                separator: info.fType.contains(MFT_SEPARATOR),
                submenu: !submenu.is_invalid(),
            };
            let key = REGISTRY.with(|r| {
                let mut r = r.borrow_mut();
                r.next += 1;
                let key = r.next;
                r.items.insert(key, item);
                key
            });
            self.keys.push(key);
            info.fMask = MIIM_FTYPE | MIIM_DATA;
            info.fType |= MFT_OWNERDRAW;
            info.dwItemData = key;
            SetMenuItemInfoW(menu, index, true, &info)?;
            if !submenu.is_invalid() {
                self.register(submenu)?;
            }
        }
        Ok(())
    }
}
impl Drop for MenuStyle {
    fn drop(&mut self) {
        unsafe {
            REGISTRY.with(|r| {
                let mut r = r.borrow_mut();
                for key in &self.keys {
                    if let Some(item) = r.items.remove(key) {
                        let mut info = MENUITEMINFOW {
                            cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
                            fMask: MIIM_FTYPE | MIIM_DATA,
                            ..Default::default()
                        };
                        if GetMenuItemInfoW(item.menu, item.index, true, &mut info).is_ok() {
                            info.fType &= !MFT_OWNERDRAW;
                            info.dwItemData = 0;
                            let _ = SetMenuItemInfoW(item.menu, item.index, true, &info);
                        }
                    }
                }
                for menu in &self.menus {
                    if let Some(brush) = r.menus.remove(&(menu.0 as isize)) {
                        let info = MENUINFO {
                            cbSize: std::mem::size_of::<MENUINFO>() as u32,
                            fMask: MIM_BACKGROUND,
                            ..Default::default()
                        };
                        let _ = SetMenuInfo(*menu, &info);
                        let _ = DeleteObject(brush);
                    }
                }
                if r.menus.is_empty() {
                    for brush in r.retired_brushes.drain(..) {
                        let _ = DeleteObject(brush);
                    }
                    r.surface = None;
                    for hook in [r.hook.take(), r.paint_hook.take()].into_iter().flatten() {
                        let _ = UnhookWindowsHookEx(hook);
                    }
                }
            });
        }
    }
}
unsafe extern "system" fn hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 {
        let event = &*(lp.0 as *const CWPSTRUCT);
        if [WM_INITMENU, WM_INITMENUPOPUP].contains(&event.message)
            && REGISTRY.with(|r| r.borrow().menus.contains_key(&(event.wParam.0 as isize)))
        {
            refresh_theme();
            if !SetWindowSubclass(event.hwnd, Some(owner_proc), 0x5142, 0).as_bool() {
                tracing::warn!("could not attach menu owner; restoring native drawing");
                restore_native(HMENU(event.wParam.0 as _));
            }
        }
    }
    CallNextHookEx(None, code, wp, lp)
}
unsafe fn refresh_theme() {
    let color = native_color(menu_theme().surface);
    let menus = REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        if r.surface == Some(color) {
            return Vec::new();
        }
        r.surface = Some(color);
        r.menus.keys().copied().collect::<Vec<_>>()
    });
    for menu in menus {
        let brush = CreateSolidBrush(color);
        let info = MENUINFO {
            cbSize: std::mem::size_of::<MENUINFO>() as u32,
            fMask: MIM_BACKGROUND,
            hbrBack: brush,
            ..Default::default()
        };
        if SetMenuInfo(HMENU(menu as _), &info).is_ok() {
            REGISTRY.with(|r| {
                let mut r = r.borrow_mut();
                if let Some(old) = r.menus.insert(menu, brush) {
                    r.retired_brushes.push(old);
                }
            });
        } else {
            let _ = DeleteObject(brush);
        }
    }
}
unsafe extern "system" fn owner_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    if (msg == WM_MEASUREITEM && (*(lp.0 as *const MEASUREITEMSTRUCT)).CtlType.0 == 1)
        || (msg == WM_DRAWITEM && (*(lp.0 as *const DRAWITEMSTRUCT)).CtlType.0 == 1)
    {
        let key = if msg == WM_MEASUREITEM {
            (*(lp.0 as *const MEASUREITEMSTRUCT)).itemData
        } else {
            (*(lp.0 as *const DRAWITEMSTRUCT)).itemData
        };
        let item = REGISTRY.with(|r| r.borrow().items.get(&key).cloned());

        if let Some(item) = item {
            let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.;
            let font = sized_font(
                scale,
                if item.footer {
                    UiMetrics::CAPTION
                } else {
                    UiMetrics::BODY
                },
                400,
            );
            if msg == WM_MEASUREITEM {
                let m = &mut *(lp.0 as *mut MEASUREITEMSTRUCT);
                let dc = GetDC(hwnd);
                let old = SelectObject(dc, font);
                let mut rect = RECT::default();
                if !item.text.is_empty() {
                    DrawTextW(
                        dc,
                        &mut item.text.encode_utf16().collect::<Vec<_>>(),
                        &mut rect,
                        DT_CALCRECT | DT_SINGLELINE,
                    );
                }
                m.itemWidth =
                    ((rect.right as f32 + 56. * scale).clamp(184. * scale, 340. * scale)) as u32;
                m.itemHeight = ((if item.separator {
                    9.
                } else {
                    UiMetrics::CONTROL_HEIGHT
                }) * scale)
                    .round() as u32;
                SelectObject(dc, old);
                ReleaseDC(hwnd, dc);
            } else {
                let d = &*(lp.0 as *const DRAWITEMSTRUCT);
                let saved = SaveDC(d.hDC);
                let theme = menu_theme();
                let mut rect = d.rcItem;
                let bg = CreateSolidBrush(native_color(theme.surface));
                FillRect(d.hDC, &rect, bg);
                let _ = DeleteObject(bg);
                if item.separator {
                    rect.left += (12. * scale) as i32;
                    rect.right -= (12. * scale) as i32;
                    rect.top = (rect.top + rect.bottom) / 2;
                    rect.bottom = rect.top + 1;
                    let brush = CreateSolidBrush(native_color(theme.border));
                    FillRect(d.hDC, &rect, brush);
                    let _ = DeleteObject(brush);
                } else {
                    let selected = d.itemState.0 & 1 != 0;
                    let disabled = d.itemState.0 & 6 != 0;
                    if selected && !disabled {
                        rect.left += (4. * scale) as i32;
                        rect.right -= (4. * scale) as i32;
                        fill_rounded(d.hDC, rect, theme.hover, UiMetrics::CONTROL_RADIUS * scale);
                    }
                    let ink = if disabled || item.footer {
                        theme.text_tertiary
                    } else {
                        theme.text
                    };
                    rect = d.rcItem;
                    rect.left += (32. * scale) as i32;
                    rect.right -= (28. * scale) as i32;
                    draw_label(d.hDC, &item.text, rect, font, ink, DT_LEFT);
                    if GetMenuState(item.menu, item.index, MF_BYPOSITION) & MF_CHECKED.0 != 0 {
                        rect = d.rcItem;
                        rect.right = rect.left + (32. * scale) as i32;
                        draw_label(d.hDC, "✓", rect, font, theme.accent, DT_CENTER);
                    }
                    // The native popup paints its own submenu arrow after WM_DRAWITEM.
                    // Leave the DC colors ready for that monochrome glyph.
                    SetTextColor(d.hDC, native_color(ink));
                    SetBkColor(d.hDC, native_color(theme.surface));
                }
                if saved != 0 {
                    let _ = RestoreDC(d.hDC, saved);
                }
            }
            let _ = DeleteObject(font);
            return LRESULT(1);
        }
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}

unsafe extern "system" fn find_popup(hwnd: HWND, lp: LPARAM) -> BOOL {
    let target = &mut *(lp.0 as *mut (HMENU, HWND));
    let mut name = [0u16; 32];
    let n = GetClassNameW(hwnd, &mut name);
    if String::from_utf16_lossy(&name[..n.max(0) as usize]) == "#32768" {
        let mut item = RECT::default();
        let mut window = RECT::default();
        if GetMenuItemRect(None, target.0, 0, &mut item).is_ok()
            && GetWindowRect(hwnd, &mut window).is_ok()
            && item.left >= window.left
            && item.right <= window.right
            && item.top >= window.top
            && item.bottom <= window.bottom
        {
            target.1 = hwnd;
            return BOOL(0);
        }
    }
    BOOL(1)
}
// Observe completed native painting without replacing the system popup wndproc.
// Installing a subclass during WM_DRAWITEM interrupts the popup's first paint.
unsafe extern "system" fn paint_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 {
        let event = &*(lp.0 as *const CWPRETSTRUCT);
        if [WM_PAINT, WM_NCPAINT, WM_MOUSEMOVE, WM_TIMER, WM_KEYDOWN].contains(&event.message) {
            let mut class = [0u16; 32];
            let n = GetClassNameW(event.hwnd, &mut class);
            if String::from_utf16_lossy(&class[..n.max(0) as usize]) != "#32768" {
                return CallNextHookEx(None, code, wp, lp);
            }
            let menus: Vec<HMENU> =
                REGISTRY.with(|r| r.borrow().menus.keys().map(|&m| HMENU(m as _)).collect());
            for menu in menus {
                let mut target = (menu, HWND::default());
                let _ = find_popup(
                    event.hwnd,
                    LPARAM((&mut target as *mut (HMENU, HWND)) as isize),
                );
                if !target.1.is_invalid() {
                    paint_arrows(event.hwnd, menu.0 as usize);
                    break;
                }
            }
        }
    }
    CallNextHookEx(None, code, wp, lp)
}
unsafe fn paint_arrows(hwnd: HWND, menu: usize) {
    let entries = REGISTRY.with(|r| {
        r.borrow()
            .items
            .values()
            .filter(|item| item.menu.0 as usize == menu && item.submenu)
            .cloned()
            .collect::<Vec<_>>()
    });
    if entries.is_empty() {
        return;
    }
    let dc = GetDC(hwnd);
    let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.;
    let theme = menu_theme();
    let font = sized_font(scale, UiMetrics::BODY, 400);
    for item in entries {
        let mut rect = RECT::default();
        if GetMenuItemRect(None, item.menu, item.index, &mut rect).is_err() {
            continue;
        }
        let mut origin = POINT {
            x: rect.left,
            y: rect.top,
        };
        let _ = ScreenToClient(hwnd, &mut origin);
        rect = RECT {
            left: origin.x,
            top: origin.y,
            right: origin.x + rect.right - rect.left,
            bottom: origin.y + rect.bottom - rect.top,
        };
        let row = rect;
        rect.left = rect.right - (22. * scale) as i32;
        let saved = SaveDC(dc);
        IntersectClipRect(dc, rect.left, rect.top, rect.right, rect.bottom);
        let selected = GetMenuState(item.menu, item.index, MF_BYPOSITION) & MF_HILITE.0 != 0;
        let bg = CreateSolidBrush(native_color(theme.surface));
        FillRect(dc, &rect, bg);
        let _ = DeleteObject(bg);
        if selected {
            let mut highlight = row;
            highlight.left += (4. * scale) as i32;
            highlight.right -= (4. * scale) as i32;
            fill_rounded(
                dc,
                highlight,
                theme.hover,
                UiMetrics::CONTROL_RADIUS * scale,
            );
        }
        draw_label(dc, "›", rect, font, theme.text_secondary, DT_CENTER);
        if saved != 0 {
            let _ = RestoreDC(dc, saved);
        }
    }
    let _ = DeleteObject(font);
    ReleaseDC(hwnd, dc);
}

unsafe fn restore_native(menu: HMENU) {
    let info = MENUINFO {
        cbSize: std::mem::size_of::<MENUINFO>() as u32,
        fMask: MIM_BACKGROUND,
        ..Default::default()
    };
    let _ = SetMenuInfo(menu, &info);
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        if let Some(brush) = r.menus.remove(&(menu.0 as isize)) {
            r.retired_brushes.push(brush);
        }
    });
    for index in 0..GetMenuItemCount(menu).max(0) as u32 {
        let mut info = MENUITEMINFOW {
            cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_FTYPE | MIIM_SUBMENU,
            ..Default::default()
        };
        if GetMenuItemInfoW(menu, index, true, &mut info).is_ok() {
            info.fType &= !MFT_OWNERDRAW;
            let child = info.hSubMenu;
            info.fMask = MIIM_FTYPE;
            let _ = SetMenuItemInfoW(menu, index, true, &info);
            if !child.is_invalid() {
                restore_native(child);
            }
        }
    }
}

#[cfg(test)]
pub(super) unsafe fn test_menu_paths(root: HMENU) -> Vec<Vec<usize>> {
    unsafe fn walk(menu: HMENU, prefix: Vec<usize>, root: bool, paths: &mut Vec<Vec<usize>>) {
        let mut ordinal = 0;
        for index in 0..GetMenuItemCount(menu).max(0) as u32 {
            let flags = GetMenuState(menu, index, MF_BYPOSITION);
            if flags & (MF_SEPARATOR.0 | MF_DISABLED.0 | MF_GRAYED.0) != 0 {
                continue;
            }
            let child = GetSubMenu(menu, index as i32);
            if !child.is_invalid() {
                let mut path = prefix.clone();
                path.extend(std::iter::repeat_n(0x28, ordinal + usize::from(root)));
                path.push(0x27);
                paths.push(path.clone());
                walk(child, path, false, paths);
            }
            ordinal += 1;
        }
    }
    let mut paths = vec![Vec::new()];
    walk(root, Vec::new(), true, &mut paths);
    paths
}

#[cfg(test)]
pub(super) unsafe fn test_assert_painted(hwnd: HWND, image: &tiny_skia::Pixmap) {
    let entries = REGISTRY.with(|r| r.borrow().items.values().cloned().collect::<Vec<_>>());
    let theme = menu_theme();
    let mut checked = 0;
    for item in entries {
        let mut target = (item.menu, HWND::default());
        let _ = find_popup(hwnd, LPARAM((&mut target as *mut (HMENU, HWND)) as isize));
        if target.1.is_invalid() || item.separator || item.text.is_empty() {
            continue;
        }
        let mut rect = RECT::default();
        GetMenuItemRect(None, item.menu, item.index, &mut rect).unwrap();
        let mut origin = POINT {
            x: rect.left,
            y: rect.top,
        };
        let _ = ScreenToClient(hwnd, &mut origin);
        let ink = native_color(
            if item.footer || GetMenuState(item.menu, item.index, MF_BYPOSITION) & 3 != 0 {
                theme.text_tertiary
            } else {
                theme.text
            },
        );
        let mut pixels = 0;
        for y in origin.y..origin.y + rect.bottom - rect.top {
            for x in origin.x + 30..origin.x + rect.right - rect.left - 30 {
                if x >= 0
                    && y >= 0
                    && image.pixel(x as u32, y as u32).is_some_and(|p| {
                        COLORREF(
                            p.red() as u32 | ((p.green() as u32) << 8) | ((p.blue() as u32) << 16),
                        ) == ink
                    })
                {
                    pixels += 1;
                }
            }
        }
        assert!(
            pixels > 5,
            "menu row {:?} was not painted ({} text pixels)",
            item.text,
            pixels
        );
        checked += 1;
    }
    assert!(checked > 0, "no registered menu matched popup");
}
