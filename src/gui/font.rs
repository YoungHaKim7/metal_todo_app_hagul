//! Sharp text for the GUI examples — glyphs rasterized on demand from the
//! system "Apple SD Gothic Neo" face through CoreText, with no third-party
//! crates.
//!
//! The stock pipeline draws colored quads and has no texture support, so a
//! glyph is delivered as *coverage quads*: CoreText rasterizes the glyph
//! once into an 8-bit alpha-only bitmap, and [`Frame::text`](super::Frame)
//! turns every horizontal run of equal coverage into one blended quad whose
//! alpha is that coverage. The outlines get proper anti-aliasing while
//! everything stays snapped to the device-pixel grid, which is what keeps
//! the result sharp:
//!
//! * [`Frame::fitted_size`](super::Frame::fitted_size) rounds text sizes to
//!   whole device pixels;
//! * each glyph is rasterized at exactly that pixel size — one raster per
//!   `(cap height, character)` pair, cached for the app's lifetime;
//! * pen positions, glyph advances and the baseline are rounded to whole
//!   pixels, so glyph edges never straddle two device pixels.
//!
//! # Metrics
//!
//! [`Frame::text`](super::Frame::text) is top-left anchored like the 5x7
//! bitmap font it replaces: the cap height (the top of the uppercase
//! letters and digits) sits `7 * size` drawing units above the `y` it is
//! given ([`CAP_HEIGHT_PER_SIZE`]), so every layout built for the bitmap
//! font keeps working. The face itself is proportional — [`measure`]
//! reports real advances, and [`glyphs`] exposes each glyph's ink box and
//! advance.
//!
//! # Glyph lookup
//!
//! `AppleSDGothicNeo-Regular` (the PostScript name) is tried first, then
//! the family name. CoreText silently substitutes a fallback font when a
//! name is unknown, so the result is verified with `CTFontCopyFullName`,
//! and a one-line warning is printed if Apple SD Gothic Neo is missing (it
//! ships with macOS, so in practice this never happens). Characters the
//! face cannot render are resolved through the system cascade with
//! `CTFontCreateForString`; anything outside the BMP, and characters with
//! no rasterizable glyph anywhere, draw as a space — exactly what the
//! bitmap font did for them.

use std::collections::HashMap;
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Cap height of the uppercase letters, in drawing units per unit of
/// `size` — the 5x7 bitmap font's caps were seven pixels tall, and the
/// example layouts are built on that proportion.
pub const CAP_HEIGHT_PER_SIZE: f32 = 7.0;

/// One rasterized glyph at a fixed device-pixel cap height (see [`glyphs`]).
pub struct Glyph {
    /// Coverage bytes `0..=255`, top row first, `w * h` row-major. Empty
    /// for glyphs without ink (spaces).
    pub coverage: Arc<[u8]>,
    /// Ink box size, in device pixels.
    pub w: u32,
    pub h: u32,
    /// Ink left edge, in device pixels right of the pen (which sits on the
    /// baseline). Negative for glyphs that overhang to the left.
    pub left: i32,
    /// Ink top edge, in device pixels above the baseline. Negative for
    /// descenders.
    pub top: i32,
    /// Pen step to the next glyph, in device pixels, rounded so every glyph
    /// origin lands on the pixel grid. Zero-width for combining marks.
    pub advance: i32,
}

impl Glyph {
    /// Horizontal runs of equal coverage in one row, as `(start, len,
    /// coverage)` — the unit [`Frame::text`](super::Frame::text) renders:
    /// one blended quad per run, with the coverage becoming the quad's
    /// alpha.
    pub fn runs(&self, row: u32) -> impl Iterator<Item = (u32, u32, u8)> + '_ {
        let w = self.w as usize;
        let row = row as usize;
        let cov: &[u8] = if row < self.h as usize {
            &self.coverage[row * w..row * w + w]
        } else {
            &[]
        };
        (0..cov.len()).filter_map(move |col| {
            let c = cov[col];
            // A run starts at a lit pixel whose left neighbor differs.
            if c == 0 || (col > 0 && cov[col - 1] == c) {
                return None;
            }
            let mut end = col + 1;
            while end < cov.len() && cov[end] == c {
                end += 1;
            }
            Some((col as u32, (end - col) as u32, c))
        })
    }
}

// ---------------------------------------------------------------------------
// CoreText / CoreGraphics / CoreFoundation ABI (system frameworks, all
// declarations in use stable since macOS 10.7).
// ---------------------------------------------------------------------------

type CFStringRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CTFontRef = *const c_void;
type CGContextRef = *mut c_void;
/// `CGFloat` is a `double` on 64-bit Apple platforms.
type CGFloat = f64;

#[repr(C)]
struct CGPoint {
    x: CGFloat,
    y: CGFloat,
}

#[repr(C)]
struct CGSize {
    width: CGFloat,
    height: CGFloat,
}

#[repr(C)]
struct CFRange {
    location: isize,
    length: isize,
}

/// `kCFStringEncodingUTF8`.
const K_CF_UTF8: u32 = 0x0800_0100;
/// `kCTFontOrientationHorizontal`.
const K_CT_HORIZONTAL: i32 = 1;
/// `kCGImageAlphaOnly` — an 8-bit coverage bitmap, exactly what a glyph
/// raster should be.
const K_CG_ALPHA_ONLY: u32 = 7;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(
        alloc: CFAllocatorRef,
        string: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CFStringGetCString(
        string: CFStringRef,
        buffer: *mut c_char,
        buffer_size: isize,
        encoding: u32,
    ) -> bool;
    fn CFRelease(cf: *const c_void);
}

#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    fn CTFontCreateWithName(name: CFStringRef, size: CGFloat, matrix: *const c_void) -> CTFontRef;
    fn CTFontCopyFullName(font: CTFontRef) -> CFStringRef;
    fn CTFontGetGlyphsForCharacters(
        font: CTFontRef,
        characters: *const u16,
        glyphs: *mut u16,
        count: isize,
    ) -> bool;
    fn CTFontGetAdvancesForGlyphs(
        font: CTFontRef,
        orientation: i32,
        glyphs: *const u16,
        advances: *mut CGSize,
        count: isize,
    ) -> f64;
    fn CTFontCreateForString(current: CTFontRef, string: CFStringRef, range: CFRange) -> CTFontRef;
    fn CTFontDrawGlyphs(
        font: CTFontRef,
        glyphs: *const u16,
        positions: *const CGPoint,
        count: usize,
        context: CGContextRef,
    );
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: *const c_void,
        bitmap_info: u32,
    ) -> CGContextRef;
    fn CGContextRelease(context: CGContextRef);
    fn CGContextSetRGBFillColor(
        context: CGContextRef,
        red: CGFloat,
        green: CGFloat,
        blue: CGFloat,
        alpha: CGFloat,
    );
}

// ---------------------------------------------------------------------------
// Font handle management.
// ---------------------------------------------------------------------------

/// PostScript name of the regular weight, then the family name as a
/// fallback (both resolve with `CTFontCreateWithName`).
const FONT_NAMES: [&str; 2] = ["AppleSDGothicNeo-Regular", "Apple SD Gothic Neo"];

/// Warned about a missing Apple SD Gothic Neo already?
static WARNED: AtomicBool = AtomicBool::new(false);

/// Wraps a `&str` in a transient CFString (released by the caller).
fn cf_string(s: &str) -> CFStringRef {
    let c = std::ffi::CString::new(s).expect("font name with interior NUL");
    unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), K_CF_UTF8) }
}

/// Does `font` really identify as Apple SD Gothic Neo? CoreText substitutes
/// silently, so the name has to be checked.
fn is_sd_gothic(font: CTFontRef) -> bool {
    if font.is_null() {
        return false;
    }
    unsafe {
        let name = CTFontCopyFullName(font);
        if name.is_null() {
            return false;
        }
        let mut buf = [0u8; 256];
        let ok = CFStringGetCString(
            name,
            buf.as_mut_ptr() as *mut c_char,
            buf.len() as isize,
            K_CF_UTF8,
        );
        CFRelease(name);
        if !ok {
            return false;
        }
        let len = buf.iter().position(|&b| b == 0).unwrap_or(0);
        // Compare loosely: the full name appears as "AppleSDGothicNeo-*"
        // and "Apple SD Gothic Neo" depending on the source.
        let name = String::from_utf8_lossy(&buf[..len])
            .to_lowercase()
            .replace([' ', '-'], "");
        name.starts_with("applesdgothicneo")
    }
}

/// Create the face at `pt_size` (points == device pixels here: rasters
/// happen in an unscaled bitmap context). Returns a handle kept alive for
/// the app's lifetime, or 0 when CoreText failed outright.
fn create_font(pt_size: f64) -> usize {
    let mut substitute: Option<usize> = None;
    for name in FONT_NAMES {
        let cf = cf_string(name);
        let font = unsafe { CTFontCreateWithName(cf, pt_size, std::ptr::null()) };
        unsafe { CFRelease(cf) };
        if is_sd_gothic(font) {
            return font as usize;
        }
        if !font.is_null() {
            substitute.get_or_insert(font as usize);
        }
    }
    if !WARNED.swap(true, Ordering::Relaxed) {
        eprintln!("font: Apple SD Gothic Neo unavailable — using the system substitute");
    }
    substitute.unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Rasterization.
// ---------------------------------------------------------------------------

/// Rasterize one glyph of `font` (already sized so a capital 'H' is the
/// desired cap height) into its own alpha-only bitmap, then crop to the
/// ink box. `pad` is the slack around the pen origin: above/below the
/// baseline and left/right of it, sized generously by the caller (>= the
/// cap height) so ascenders, descenders and parentheses never clip.
fn rasterize(font: usize, glyph_id: u16, pad: usize) -> Glyph {
    let font = font as CTFontRef;
    let ids = [glyph_id];
    let mut advance = CGSize {
        width: 0.0,
        height: 0.0,
    };
    unsafe {
        CTFontGetAdvancesForGlyphs(
            font,
            K_CT_HORIZONTAL,
            ids.as_ptr(),
            &mut advance,
            ids.len() as isize,
        );
    }
    let advance_px = advance.width.round().max(0.0) as i64;

    // Glyph box: pen at `(pad, pad)` from the bottom-left, so the baseline
    // sits `pad` rows above the bottom and `3 * pad` rows below the top —
    // room enough for descenders below and accents/parentheses above.
    let box_w = advance_px as usize + 2 * pad;
    let box_h = 4 * pad;
    let stride = (box_w + 15) & !15;
    let mut buf = vec![0u8; stride * box_h];
    unsafe {
        let ctx = CGBitmapContextCreate(
            buf.as_mut_ptr() as *mut c_void,
            box_w,
            box_h,
            8,
            stride,
            std::ptr::null(), // alpha-only contexts take no colorspace
            K_CG_ALPHA_ONLY,
        );
        if !ctx.is_null() {
            CGContextSetRGBFillColor(ctx, 1.0, 1.0, 1.0, 1.0);
            let pos = [CGPoint {
                x: pad as CGFloat,
                y: pad as CGFloat,
            }];
            CTFontDrawGlyphs(font, ids.as_ptr(), pos.as_ptr(), pos.len(), ctx);
            CGContextRelease(ctx);
        }
    }

    // Scan the ink bounding box. Bitmap-context memory is top-down, so the
    // baseline (drawing y = `pad`) sits at row `box_h - 1 - pad`.
    let baseline_row = box_h - 1 - pad;
    let mut top_row = None;
    let mut bottom_row = 0usize;
    let mut left_col = box_w;
    let mut right_col = 0usize;
    for r in 0..box_h {
        let row = &buf[r * stride..r * stride + box_w];
        for (c, &a) in row.iter().enumerate() {
            // Drop the faintest AA tail — it would only buy extra quads.
            if a < 3 {
                continue;
            }
            if top_row.is_none() {
                top_row = Some(r);
            }
            bottom_row = r;
            left_col = left_col.min(c);
            right_col = right_col.max(c);
        }
    }
    let blank = Glyph {
        coverage: Arc::from([]),
        w: 0,
        h: 0,
        left: 0,
        top: 0,
        advance: advance_px as i32,
    };
    let Some(top) = top_row else {
        return blank; // no ink (space, combining mark on its own, …)
    };
    let w = right_col - left_col + 1;
    let h = bottom_row - top + 1;
    let mut coverage = vec![0u8; w * h];
    for r in 0..h {
        let src = (top + r) * stride + left_col;
        coverage[r * w..(r + 1) * w].copy_from_slice(&buf[src..src + w]);
    }
    Glyph {
        coverage: coverage.into(),
        w: w as u32,
        h: h as u32,
        left: left_col as i32 - pad as i32,
        top: baseline_row as i32 - top as i32,
        advance: advance_px as i32,
    }
}

/// Cap-height ratio of the face: rasterized ink height of 'H' per point of
/// font size. Measured instead of read from font metrics, so it only relies
/// on the raster path itself.
fn calibrate(font: usize) -> Option<f64> {
    if font == 0 {
        return None;
    }
    let mut units = [0u16; 1];
    'H'.encode_utf16(&mut units);
    let mut ids = [0u16; 1];
    unsafe {
        if !CTFontGetGlyphsForCharacters(
            font as CTFontRef,
            units.as_ptr(),
            ids.as_mut_ptr(),
            units.len() as isize,
        ) {
            return None;
        }
    }
    if ids[0] == 0 {
        return None;
    }
    let h = rasterize(font, ids[0], 128).h;
    (h > 0).then(|| h as f64 / 256.0)
}

// ---------------------------------------------------------------------------
// Cache.
// ---------------------------------------------------------------------------

/// Per-app-lifetime raster cache: fonts per cap-height bucket, per-character
/// fallback fonts, and rasterized glyphs. Font handles are held forever on
/// purpose — a handful of sizes and fallback faces for the whole run.
#[derive(Default)]
struct Cache {
    /// Cap height (device px) -> font handle sized for it.
    fonts: HashMap<u32, usize>,
    /// Character -> fallback font handle covering it.
    fallbacks: HashMap<u32, usize>,
    /// `(cap height, character)` -> rasterized glyph.
    glyphs: HashMap<(u32, u32), Arc<Glyph>>,
    /// Ink height of 'H' per point of size (see [`calibrate`]).
    cap_ratio: Option<f64>,
}

impl Cache {
    fn new() -> Cache {
        Cache {
            cap_ratio: calibrate(create_font(256.0)),
            ..Cache::default()
        }
    }

    /// Font sized so a capital 'H' renders `cap_px` device pixels tall.
    fn bucket_font(&mut self, cap_px: u32) -> usize {
        if let Some(&font) = self.fonts.get(&cap_px) {
            return font;
        }
        let pt_size = cap_px as f64 / self.cap_ratio.unwrap_or(0.7);
        let font = create_font(pt_size);
        self.fonts.insert(cap_px, font);
        font
    }

    /// A system font that covers `ch` (cached per character).
    fn fallback_font(&mut self, primary: usize, ch: char) -> usize {
        let key = ch as u32;
        if let Some(&font) = self.fallbacks.get(&key) {
            return font;
        }
        let font = if primary == 0 {
            0
        } else {
            let s = cf_string(&ch.to_string());
            let font = unsafe {
                CTFontCreateForString(
                    primary as CTFontRef,
                    s,
                    CFRange {
                        location: 0,
                        length: 1,
                    },
                )
            };
            unsafe { CFRelease(s) };
            if font.is_null() {
                primary
            } else {
                font as usize
            }
        };
        self.fallbacks.insert(key, font);
        font
    }

    /// First glyph id of `ch` in `font` (0 when the font lacks the char).
    fn first_glyph(font: usize, ch: char) -> u16 {
        if font == 0 {
            return 0;
        }
        let mut units = [0u16; 2];
        // `encode_utf16` returns the unwritten tail; the count is what is
        // left.
        let count = units.len() - ch.encode_utf16(&mut units).len();
        let mut ids = [0u16; 2];
        unsafe {
            if CTFontGetGlyphsForCharacters(
                font as CTFontRef,
                units.as_ptr(),
                ids.as_mut_ptr(),
                count as isize,
            ) {
                ids[0]
            } else {
                0
            }
        }
    }

    fn resolve(&mut self, cap_px: u32, ch: char) -> (usize, u16) {
        // Whitespace/control characters and anything outside the BMP draw
        // as a space (the primary face always covers it).
        let ch = if (ch as u32) > 0xFFFF || ch.is_control() {
            ' '
        } else {
            ch
        };
        let font = self.bucket_font(cap_px);
        let mut gid = Self::first_glyph(font, ch);
        if gid == 0 {
            let fallback = self.fallback_font(font, ch);
            gid = Self::first_glyph(fallback, ch);
            return (fallback, gid);
        }
        (font, gid)
    }

    fn glyph(&mut self, cap_px: u32, ch: char) -> Arc<Glyph> {
        let key = (cap_px, ch as u32);
        if let Some(glyph) = self.glyphs.get(&key) {
            return glyph.clone();
        }
        let glyph = match self.resolve(cap_px, ch) {
            (font, gid) if font != 0 && gid != 0 => {
                // Generous raster slack: cover ascenders above the cap
                // height (parentheses) and full descenders below.
                Arc::new(rasterize(font, gid, cap_px.max(8) as usize))
            }
            _ if ch != ' ' => self.glyph(cap_px, ' '),
            // Nothing rasterizable anywhere: an empty box that still
            // advances roughly like a space so text keeps flowing.
            _ => Arc::new(Glyph {
                coverage: Arc::from([]),
                w: 0,
                h: 0,
                left: 0,
                top: 0,
                advance: (cap_px as f32 * 6.0 / 7.0).round() as i32,
            }),
        };
        self.glyphs.insert(key, glyph.clone());
        glyph
    }
}

static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

fn cache() -> &'static Mutex<Cache> {
    CACHE.get_or_init(|| Mutex::new(Cache::new()))
}

// ---------------------------------------------------------------------------
// Public API.
// ---------------------------------------------------------------------------

/// Device-pixel cap height for text at `size` drawing units on a canvas
/// with `px_per_unit` device pixels per drawing unit (see
/// [`CAP_HEIGHT_PER_SIZE`]).
pub fn cap_height_px(size: f32, px_per_unit: f32) -> u32 {
    ((size * px_per_unit * CAP_HEIGHT_PER_SIZE).round().max(1.0)) as u32
}

/// Rasterize one character at the given device-pixel cap height (from
/// [`cap_height_px`]), served from the cache.
pub fn glyph(cap_px: u32, ch: char) -> Arc<Glyph> {
    let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    cache.glyph(cap_px, ch)
}

/// Rasterize a whole line at the given device-pixel cap height, served from
/// the cache.
pub fn glyphs(cap_px: u32, text: &str) -> Vec<Arc<Glyph>> {
    let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    text.chars().map(|ch| cache.glyph(cap_px, ch)).collect()
}

/// Rendered width of `text` at `size`, in drawing units — the pen advances
/// of every glyph but the last plus the last glyph's ink width, the same
/// extent [`Frame::text`](super::Frame::text) actually paints.
pub fn measure(size: f32, text: &str, px_per_unit: f32) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let cap_px = cap_height_px(size, px_per_unit);
    let glyphs = glyphs(cap_px, text);
    let mut px: i64 = glyphs[..glyphs.len() - 1]
        .iter()
        .map(|g| g.advance as i64)
        .sum();
    let last = glyphs.last().expect("non-empty text");
    px += (last.left + last.w as i32) as i64;
    px.max(0) as f32 / px_per_unit
}
