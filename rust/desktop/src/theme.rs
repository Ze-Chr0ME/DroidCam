//! Native, DPI-aware painting. No webview or GPU loop is needed for the chrome.
use std::{ffi::OsStr, os::windows::ffi::OsStrExt, ptr::null_mut};
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*};

pub const BG: u32 = 0x111214;
pub const PANEL: u32 = 0x17181B;
pub const CARD: u32 = 0x202226;
pub const CARD_HI: u32 = 0x2A2C31;
pub const LINE: u32 = 0x2E3035;
pub const INK: u32 = 0xE9EAEC;
pub const MUTED: u32 = 0x8E929A;
pub const ACCENT: u32 = 0x4C82F7;
pub const ACCENT_HI: u32 = 0x6092FA;
pub const ACCENT_DOWN: u32 = 0x3C6CD6;
pub const GREEN: u32 = 0x3FB97A;
pub const AMBER: u32 = 0xE0A84A;
pub const WHITE: u32 = 0xFFFFFF;
pub const VIDEO: u32 = 0x08090A;
pub fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}
pub fn color(rgb: u32) -> u32 {
    ((rgb & 255) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 255)
}
pub fn scale(n: i32, dpi: u32) -> i32 {
    ((n as i64 * dpi as i64 + 48) / 96) as i32
}
pub fn rect(x: i32, y: i32, w: i32, h: i32, dpi: u32) -> RECT {
    RECT {
        left: scale(x, dpi),
        top: scale(y, dpi),
        right: scale(x + w, dpi),
        bottom: scale(y + h, dpi),
    }
}
pub struct Fonts {
    pub body: HFONT,
    pub small: HFONT,
    pub strong: HFONT,
    pub title: HFONT,
    pub hero: HFONT,
}
impl Fonts {
    pub unsafe fn new(dpi: u32) -> Self {
        let font = |size, weight, name: &str| {
            CreateFontW(
                -scale(size, dpi),
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                CLEARTYPE_QUALITY as u32,
                0,
                wide(name).as_ptr(),
            )
        };
        Self {
            body: font(14, 400, "Segoe UI"),
            small: font(12, 400, "Segoe UI"),
            strong: font(14, 600, "Segoe UI"),
            title: font(18, 600, "Segoe UI"),
            hero: font(17, 600, "Segoe UI"),
        }
    }
}
impl Drop for Fonts {
    fn drop(&mut self) {
        unsafe {
            for f in [self.body, self.small, self.strong, self.title, self.hero] {
                DeleteObject(f);
            }
        }
    }
}
pub struct Surface {
    pub dc: HDC,
    bitmap: HBITMAP,
    original: HGDIOBJ,
    pub width: i32,
    pub height: i32,
}
impl Surface {
    pub unsafe fn new(dc: HDC, w: i32, h: i32) -> Self {
        let memory = CreateCompatibleDC(dc);
        let bitmap = CreateCompatibleBitmap(dc, w.max(1), h.max(1));
        let original = SelectObject(memory, bitmap);
        Self {
            dc: memory,
            bitmap,
            original,
            width: w,
            height: h,
        }
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.original);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}
pub struct Paint<'a> {
    pub dc: HDC,
    pub dpi: u32,
    pub fonts: &'a Fonts,
}
impl Paint<'_> {
    pub unsafe fn fill(&self, r: RECT, rgb: u32) {
        let brush = CreateSolidBrush(color(rgb));
        FillRect(self.dc, &r, brush);
        DeleteObject(brush);
    }
    pub unsafe fn round(&self, r: RECT, radius: i32, fill: u32, border: u32) {
        let brush = CreateSolidBrush(color(fill));
        let pen = CreatePen(PS_SOLID, 1, color(border));
        let oldb = SelectObject(self.dc, brush);
        let oldp = SelectObject(self.dc, pen);
        RoundRect(
            self.dc,
            r.left,
            r.top,
            r.right,
            r.bottom,
            scale(radius, self.dpi),
            scale(radius, self.dpi),
        );
        SelectObject(self.dc, oldb);
        SelectObject(self.dc, oldp);
        DeleteObject(brush);
        DeleteObject(pen);
    }
    pub unsafe fn outline(&self, r: RECT, radius: i32, border: u32, width: i32) {
        let pen = CreatePen(PS_SOLID, scale(width, self.dpi).max(1), color(border));
        let oldb = SelectObject(self.dc, GetStockObject(NULL_BRUSH));
        let oldp = SelectObject(self.dc, pen);
        let radius = scale(radius, self.dpi);
        RoundRect(self.dc, r.left, r.top, r.right, r.bottom, radius, radius);
        SelectObject(self.dc, oldb);
        SelectObject(self.dc, oldp);
        DeleteObject(pen);
    }
    pub unsafe fn dot(&self, cx: i32, cy: i32, radius: i32, rgb: u32) {
        let r = RECT {
            left: scale(cx - radius, self.dpi),
            top: scale(cy - radius, self.dpi),
            right: scale(cx + radius, self.dpi),
            bottom: scale(cy + radius, self.dpi),
        };
        let brush = CreateSolidBrush(color(rgb));
        let pen = CreatePen(PS_SOLID, 1, color(rgb));
        let oldb = SelectObject(self.dc, brush);
        let oldp = SelectObject(self.dc, pen);
        Ellipse(self.dc, r.left, r.top, r.right, r.bottom);
        SelectObject(self.dc, oldb);
        SelectObject(self.dc, oldp);
        DeleteObject(brush);
        DeleteObject(pen);
    }
    pub unsafe fn text(&self, s: &str, r: RECT, font: HFONT, rgb: u32, flags: u32) {
        let previous = SelectObject(self.dc, font);
        SetBkMode(self.dc, TRANSPARENT as i32);
        SetTextColor(self.dc, color(rgb));
        let mut r = r;
        let s = wide(s);
        DrawTextW(
            self.dc,
            s.as_ptr(),
            (s.len() - 1) as i32,
            &mut r,
            DT_NOPREFIX | flags,
        );
        SelectObject(self.dc, previous);
    }
    pub unsafe fn small(&self, s: &str, x: i32, y: i32, w: i32) {
        self.text(
            s,
            rect(x, y, w, 24, self.dpi),
            self.fonts.small,
            MUTED,
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
        );
    }
    pub unsafe fn bitmap(&self, bytes: &[u8], size: (usize, usize), r: RECT, smooth: bool) {
        let (w, h) = size;
        if w == 0 || h == 0 || bytes.len() != w * h * 4 {
            return;
        }
        let factor =
            ((r.right - r.left) as f64 / w as f64).min((r.bottom - r.top) as f64 / h as f64);
        let factor = if smooth {
            factor
        } else {
            factor.floor().max(1.)
        };
        let (dw, dh) = (
            (w as f64 * factor).round() as i32,
            (h as f64 * factor).round() as i32,
        );
        let bitmap = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..std::mem::zeroed()
            },
            ..std::mem::zeroed()
        };
        let previous = SetStretchBltMode(self.dc, if smooth { HALFTONE } else { COLORONCOLOR });
        SetBrushOrgEx(self.dc, 0, 0, null_mut());
        StretchDIBits(
            self.dc,
            r.left + (r.right - r.left - dw) / 2,
            r.top + (r.bottom - r.top - dh) / 2,
            dw,
            dh,
            0,
            0,
            w as i32,
            h as i32,
            bytes.as_ptr() as _,
            &bitmap,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
        SetStretchBltMode(self.dc, previous);
    }
}
