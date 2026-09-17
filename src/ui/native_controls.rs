//! Shared GDI drawing primitives. Native controls keep their input and accessibility behavior.
use super::theme::Color;
use windows::{
    core::w,
    Win32::{
        Foundation::{COLORREF, RECT},
        Graphics::Gdi::*,
    },
};

pub(super) fn native_color(c: Color) -> COLORREF {
    COLORREF(c.r as u32 | (c.g as u32) << 8 | (c.b as u32) << 16)
}
pub(super) unsafe fn fill_rounded(dc: HDC, rect: RECT, color: Color, radius: f32) {
    let brush = CreateSolidBrush(native_color(color));
    rounded(dc, rect, brush, (radius * 2.).round() as i32);
    let _ = DeleteObject(brush);
}
pub(super) unsafe fn draw_label(
    dc: HDC,
    text: &str,
    mut rect: RECT,
    font: HFONT,
    ink: Color,
    align: DRAW_TEXT_FORMAT,
) {
    if text.is_empty() {
        return;
    }
    let old = SelectObject(dc, font);
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, native_color(ink));
    DrawTextW(
        dc,
        &mut text.encode_utf16().collect::<Vec<_>>(),
        &mut rect,
        align | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
    );
    SelectObject(dc, old);
}

pub(super) unsafe fn sized_font(scale: f32, size: f32, weight: i32) -> HFONT {
    // GDI cannot select the variable font on some Windows installations.
    // Verify its resolved family once; otherwise use the supported Segoe UI face.
    static VARIABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let variable = *VARIABLE.get_or_init(|| {
        let probe = create_font(12, 400, w!("Segoe UI Variable"));
        let dc = GetDC(None);
        let old = SelectObject(dc, probe);
        let mut name = [0u16; 64];
        GetTextFaceW(dc, Some(&mut name));
        let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        let supported = String::from_utf16_lossy(&name[..len]).starts_with("Segoe UI Variable");
        SelectObject(dc, old);
        ReleaseDC(None, dc);
        let _ = DeleteObject(probe);
        supported
    });
    create_font(
        (size * scale).round() as i32,
        weight,
        if variable {
            w!("Segoe UI Variable")
        } else {
            w!("Segoe UI")
        },
    )
}
unsafe fn create_font(size: i32, weight: i32, face: windows::core::PCWSTR) -> HFONT {
    CreateFontW(
        -size,
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        CLEARTYPE_QUALITY.0 as u32,
        0,
        face,
    )
}
pub(super) unsafe fn rounded(dc: HDC, r: RECT, brush: HBRUSH, radius: i32) {
    let old = SelectObject(dc, brush);
    let pen = SelectObject(dc, GetStockObject(NULL_PEN));
    let _ = RoundRect(dc, r.left, r.top, r.right, r.bottom, radius, radius);
    SelectObject(dc, old);
    SelectObject(dc, pen);
}
