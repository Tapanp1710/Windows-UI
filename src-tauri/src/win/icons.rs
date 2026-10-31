//! High-resolution app icon extraction.
//!
//! The brief asks for each app's *real* icon at high resolution, not the blurry 32px
//! one the window handle gives you. So instead of `WM_GETICON`, this goes through the
//! shell's own icon pipeline:
//!
//!   `SHCreateItemFromParsingName` -> `IShellItemImageFactory::GetImage(256x256)`
//!
//! That is the same call Explorer uses to draw large icons, so it returns the crisp
//! 256px asset when the app ships one, and a clean upscale when it does not. It also
//! works for three different kinds of target with no special-casing:
//!
//!   - a plain `.exe` path            -> the exe's icon resource
//!   - a `.lnk` path (taskbar pins)   -> the shortcut's resolved icon
//!   - `shell:AppsFolder\<AUMID>`     -> Store/UWP app logos, which have no exe icon
//!
//! The HBITMAP it hands back is top-down 32-bit BGRA. We convert to RGBA, un-multiply
//! the alpha, and encode a PNG data URL for the webview.

use base64::Engine;
use windows::core::{Interface, HSTRING, PCWSTR};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{DeleteObject, GetObjectW, BITMAP, DIBSECTION, HBITMAP};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
    SIIGBF_RESIZETOFIT,
};

/// Icon edge length we request. 256 is the largest size Windows icon resources
/// normally carry; the dock renders at 44 CSS px, so this stays sharp at any dock
/// size setting and on any DPI.
const ICON_PX: i32 = 256;

/// Produce a `data:image/png;base64,...` URL for anything the shell can parse.
///
/// Returns `None` rather than erroring: a missing icon should leave a blank plate,
/// never take the dock down.
pub fn icon_data_url(parsing_name: &str) -> Option<String> {
    let rgba = icon_rgba(parsing_name)?;
    encode_data_url(&rgba)
}

/// The same, for a shell item the caller already has.
///
/// The launcher enumerates `shell:AppsFolder` and gets an `IShellItem` per app, so
/// it can ask that item for its icon directly. Going back through a parsing name
/// does not work for all of them: the names the AppsFolder hands out do not all
/// round-trip through `SHCreateItemFromParsingName`, and every legacy app came back
/// with a blank page icon.
///
/// `px` is the requested edge length. The launcher asks for less than the dock does
/// because it sends a couple of hundred of these across the IPC boundary at once.
pub fn icon_data_url_from_item(item: &windows::Win32::UI::Shell::IShellItem, px: i32) -> Option<String> {
    unsafe {
        let factory: IShellItemImageFactory = item.cast().ok()?;
        let size = SIZE { cx: px, cy: px };
        let hbitmap: HBITMAP = factory
            .GetImage(size, SIIGBF_RESIZETOFIT | SIIGBF_BIGGERSIZEOK)
            .ok()?;
        let rgba = hbitmap_to_rgba(hbitmap);
        let _ = DeleteObject(hbitmap);
        encode_data_url(&rgba?)
    }
}

fn encode_data_url(rgba: &Rgba) -> Option<String> {
    let png = encode_png(rgba)?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    Some(format!("data:image/png;base64,{b64}"))
}

struct Rgba {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

fn icon_rgba(parsing_name: &str) -> Option<Rgba> {
    unsafe {
        let wide = HSTRING::from(parsing_name);
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;

        let size = SIZE {
            cx: ICON_PX,
            cy: ICON_PX,
        };
        // BIGGERSIZEOK lets the shell hand back the next size up rather than a
        // blurry upscale; RESIZETOFIT keeps the aspect ratio.
        let hbitmap: HBITMAP = factory
            .GetImage(size, SIIGBF_RESIZETOFIT | SIIGBF_BIGGERSIZEOK)
            .ok()?;

        let result = hbitmap_to_rgba(hbitmap);
        let _ = DeleteObject(hbitmap);
        result
    }
}

/// Copy a 32-bit BGRA HBITMAP into straight (non-premultiplied) RGBA.
///
/// **Row order has to be asked for, not assumed.** A DIB with a positive `biHeight`
/// is stored *bottom-up*: the first row in memory is the bottom row of the image.
/// Negative means top-down. `BITMAP.bmHeight` from `GetObjectW` is always positive,
/// so it cannot answer the question - reading it as top-down silently rendered every
/// shell icon upside down, which is what "the images are inverted" was. Querying a
/// `DIBSECTION` instead gives the real `biHeight` and its sign.
unsafe fn hbitmap_to_rgba(hbitmap: HBITMAP) -> Option<Rgba> {
    let mut info = BITMAP::default();
    let written = GetObjectW(
        hbitmap,
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut info as *mut BITMAP as *mut _),
    );
    if written == 0 || info.bmWidth <= 0 || info.bmHeight <= 0 || info.bmBitsPixel != 32 {
        return None;
    }

    // If this is a DIB section we get the header and with it the row order. If it is
    // not, `GetObjectW` writes less than a DIBSECTION and top-down is the safe guess
    // (it is what the shell's own image factory documents).
    let mut dib = DIBSECTION::default();
    let dib_written = GetObjectW(
        hbitmap,
        std::mem::size_of::<DIBSECTION>() as i32,
        Some(&mut dib as *mut DIBSECTION as *mut _),
    );
    let top_down = if dib_written as usize == std::mem::size_of::<DIBSECTION>() {
        dib.dsBmih.biHeight < 0
    } else {
        true
    };

    let w = info.bmWidth as usize;
    let h = info.bmHeight as usize;
    let stride = info.bmWidthBytes as usize;
    if info.bmBits.is_null() {
        return None;
    }

    let src = std::slice::from_raw_parts(info.bmBits as *const u8, stride * h);
    let mut out = vec![0u8; w * h * 4];

    for y in 0..h {
        // `y` is the destination row, counted from the top. A bottom-up DIB holds
        // that row at the mirrored offset.
        let sy = if top_down { y } else { h - 1 - y };
        let row = &src[sy * stride..sy * stride + w * 4];
        for x in 0..w {
            let b = row[x * 4];
            let g = row[x * 4 + 1];
            let r = row[x * 4 + 2];
            let a = row[x * 4 + 3];

            // The shell returns premultiplied alpha. Dividing it back out avoids the
            // dark halo you otherwise get around antialiased icon edges once the
            // browser composites them over a light glass plate.
            let (r, g, b) = if a == 0 {
                (0, 0, 0)
            } else if a == 255 {
                (r, g, b)
            } else {
                let un = |c: u8| ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;
                (un(r), un(g), un(b))
            };

            let o = (y * w + x) * 4;
            out[o] = r;
            out[o + 1] = g;
            out[o + 2] = b;
            out[o + 3] = a;
        }
    }

    Some(Rgba {
        width: w as u32,
        height: h as u32,
        pixels: out,
    })
}

fn encode_png(img: &Rgba) -> Option<Vec<u8>> {
    let buf = image::RgbaImage::from_raw(img.width, img.height, img.pixels.clone())?;
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(buf)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;
    Some(out)
}

/// Build the parsing name for a Store/UWP app from its AppUserModelID.
pub fn apps_folder_parsing_name(aumid: &str) -> String {
    format!("shell:AppsFolder\\{aumid}")
}
