//! Shared GUI runtime for the 100 Metal examples — a tiny immediate-mode
//! layer on top of the crate's raw Objective-C ABI:
//!
//! * `App` owns an AppKit window (`NSApplication` + layer-hosted
//!   `CAMetalLayer`), pumps the event loop by hand
//!   (`nextEventMatchingMask:untilDate:inMode:dequeue:`) and renders every
//!   frame through the library's Metal wrappers into a 4x multisampled
//!   target that is resolved into the layer's drawable.
//! * `Frame` is what each example's closure receives per frame: a pure
//!   vertex collector (rects, circles, lines, CoreText-rendered text) plus
//!   keyboard/mouse state. After the closure returns, the collected
//!   vertices are encoded with a stock pipeline and presented to the
//!   layer's drawable.
//! * `draw_custom` lets an example bring its own Metal shader: build a
//!   pipeline with [`make_pipeline`] (name the functions `vertex_main` /
//!   `fragment_main`), then draw [`Vertex`] triangles with an `f32` uniform
//!   array. Uniform layout convention: `u[0] = (width, height, time,
//!   scale)`, `u[1..]` = the floats passed to `draw_custom`.
//!
//! # Coordinates
//!
//! Everything is drawn in *drawing units*. By default a unit is half a
//! point, on every display: the gallery was laid out on that grid (a
//! 640×400-point window is a 1280×800 canvas), so [`Frame::width`] /
//! [`Frame::height`] are the same numbers on a Retina and on a non-Retina
//! screen, and no caption outgrows its window because the backing scale
//! differs. An app whose layout is computed from the window size in points
//! is created with [`App::new_points`] instead: it draws in points and its
//! canvas is exactly `width × height`. In both cases [`Frame::scale`]
//! converts a size given in points to drawing units, and the mouse position
//! comes in drawing units, origin top-left.
//!
//! ESC closes every example; so do the red close button, `⌘W` and `⌘Q`.
//! `⌘V` is intercepted before AppKit and lands on the same typed-text queue
//! `Input::text` serves; `⌘C` / `⌘X` reach the example, which stores what
//! they carry through [`Frame::set_clipboard`].
//!
//! Headless smoke-testing (frame limits, PNG grabs of the drawable,
//! synthesised key/mouse events) is available through [`harness`].

#![allow(unsafe_op_in_unsafe_fn)]
#![allow(dead_code)]

mod appkit;
mod font;
pub mod harness;

use std::collections::HashSet;
use std::time::Instant;

use metal_cpp_rs::metal::device::MTLDevice;
use metal_cpp_rs::metal::enums::{
    MTLBlendFactor, MTLCommandBufferStatus, MTLLoadAction, MTLPixelFormat, MTLPrimitiveType,
    MTLResourceOptions, MTLStorageMode, MTLStoreAction, MTLTextureType, MTLTextureUsage,
};
use metal_cpp_rs::metal::pipeline::{MTLRenderPipelineDescriptor, MTLRenderPipelineState};
use metal_cpp_rs::metal::resources::{MTLBuffer, MTLTexture, MTLTextureDescriptor};
use metal_cpp_rs::metal::types::{MTLOrigin, MTLSize};
use metal_cpp_rs::objc_abi::{NSUInteger, Raw, Ref, Strong, class_name_of};
use metal_cpp_rs::prelude::*;
use metal_cpp_rs::quartz_core::{CAMetalLayer, CGSize};

use appkit::{
    ACTIVATION_POLICY_REGULAR, EVENT_KEY_DOWN, EVENT_KEY_UP, EVENT_LEFT_MOUSE_DOWN,
    EVENT_LEFT_MOUSE_DRAGGED, EVENT_LEFT_MOUSE_UP, EVENT_MOUSE_MOVED, EVENT_RIGHT_MOUSE_DOWN,
    EVENT_RIGHT_MOUSE_DRAGGED, EVENT_RIGHT_MOUSE_UP, MODIFIER_FLAG_COMMAND, MODIFIER_FLAG_CONTROL,
    MODIFIER_FLAG_OPTION, MODIFIER_FLAG_SHIFT, NSApplication, NSDate, NSEvent,
    NSPASTEBOARD_TYPE_STRING, NSPasteboard, NSPoint, NSRect, NSWindow, WINDOW_BACKING_BUFFERED,
    WINDOW_STYLE_DEFAULT, appkit_force_link, current_modifier_flags, install_main_menu,
    layer_contents_scale, set_layer_contents_scale,
};
use harness::Harness;

// ---------------------------------------------------------------------------
// Key codes (HID usage codes, as returned by `-[NSEvent keyCode]`).
// ---------------------------------------------------------------------------

/// HID key codes for the arrow keys, space bar, and so on.
pub mod key {
    /// Left arrow.
    pub const LEFT: u16 = 123;
    /// Right arrow.
    pub const RIGHT: u16 = 124;
    /// Down arrow.
    pub const DOWN: u16 = 125;
    /// Up arrow.
    pub const UP: u16 = 126;
    /// Space bar.
    pub const SPACE: u16 = 49;
    /// Return (Enter).
    pub const ENTER: u16 = 36;
    /// Escape.
    pub const ESC: u16 = 53;
    /// Delete (backspace).
    pub const BACKSPACE: u16 = 51;
    /// Forward delete.
    pub const DELETE: u16 = 117;
    /// Tab.
    pub const TAB: u16 = 48;
    /// A
    pub const A: u16 = 0;
    /// B
    pub const B: u16 = 11;
    /// C
    pub const C: u16 = 8;
    /// D
    pub const D: u16 = 2;
    /// E
    pub const E: u16 = 14;
    /// F
    pub const F: u16 = 3;
    /// G
    pub const G: u16 = 5;
    /// H
    pub const H: u16 = 4;
    /// I
    pub const I: u16 = 34;
    /// J
    pub const J: u16 = 38;
    /// K
    pub const K: u16 = 40;
    /// L
    pub const L: u16 = 37;
    /// M
    pub const M: u16 = 46;
    /// N
    pub const N: u16 = 45;
    /// O
    pub const O: u16 = 31;
    /// P
    pub const P: u16 = 35;
    /// Q
    pub const Q: u16 = 12;
    /// R
    pub const R: u16 = 15;
    /// S
    pub const S: u16 = 1;
    /// T
    pub const T: u16 = 17;
    /// U
    pub const U: u16 = 32;
    /// V
    pub const V: u16 = 9;
    /// W
    pub const W: u16 = 13;
    /// X
    pub const X: u16 = 7;
    /// Y
    pub const Y: u16 = 16;
    /// Z
    pub const Z: u16 = 6;
    /// Digit 0
    pub const NUM_0: u16 = 29;
    /// Digit 1
    pub const NUM_1: u16 = 18;
    /// Digit 2
    pub const NUM_2: u16 = 19;
    /// Digit 3
    pub const NUM_3: u16 = 20;
    /// Digit 4
    pub const NUM_4: u16 = 21;
    /// Digit 5
    pub const NUM_5: u16 = 23;
    /// Digit 6
    pub const NUM_6: u16 = 22;
    /// Digit 7
    pub const NUM_7: u16 = 26;
    /// Digit 8
    pub const NUM_8: u16 = 28;
    /// Digit 9
    pub const NUM_9: u16 = 25;
}

// ---------------------------------------------------------------------------
// Constants.
// ---------------------------------------------------------------------------

/// 16 MB of vertex scratch — 524,288 [`Vertex`]es (about 175k triangles).
const VERTEX_BUFFER_BYTES: u64 = 16 << 20;
/// 1 MB of uniform scratch — 4096 chunks of 256 bytes.
const UNIFORM_BUFFER_BYTES: u64 = 1 << 20;
/// Uniform chunks are 256-byte aligned (64 `f32`s).
const UNIFORM_CHUNK_FLOATS: usize = 64;
/// Units per point — the drawing grid every example is laid out on (see
/// the module docs).
pub const UNITS_PER_POINT: f32 = 2.0;
/// Sample count of the render target. On a non-Retina display a unit is
/// half a pixel, so the thinnest strokes are sub-pixel; multisampling keeps
/// them an even shade instead of turning them into rows of dots.
const SAMPLE_COUNT: u64 = 4;

const DEFAULT_SHADER: &str = r#"
#include <metal_stdlib>
using namespace metal;

struct GuiVertex {
    float2 pos;
    float4 color;
};

struct GuiOut {
    float4 position [[position]];
    float4 color;
};

vertex GuiOut vertex_main(const device GuiVertex* vertices [[buffer(0)]],
                          constant float4* uniforms [[buffer(1)]],
                          uint vid [[vertex_id]]) {
    float2 size = uniforms[0].xy;
    float2 ndc = float2(vertices[vid].pos.x / size.x * 2.0 - 1.0,
                        1.0 - vertices[vid].pos.y / size.y * 2.0);
    GuiOut out;
    out.position = float4(ndc, 0.0, 1.0);
    out.color = vertices[vid].color;
    return out;
}

fragment float4 fragment_main(GuiOut in [[stage_in]]) {
    return in.color;
}
"#;

// ---------------------------------------------------------------------------
// Data types.
// ---------------------------------------------------------------------------

/// RGBA color, components in `0.0..=1.0`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Color {
    /// Red.
    pub r: f32,
    /// Green.
    pub g: f32,
    /// Blue.
    pub b: f32,
    /// Alpha.
    pub a: f32,
}

impl Color {
    /// Opaque color from RGB components.
    pub const fn rgb(r: f32, g: f32, b: f32) -> Color {
        Color { r, g, b, a: 1.0 }
    }

    /// Color from RGBA components.
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
        Color { r, g, b, a }
    }

    /// HSV color — `h` in turns (`0.0..=1.0` wraps), `s`/`v` in `0.0..=1.0`.
    pub fn hsv(h: f32, s: f32, v: f32) -> Color {
        let h = h.rem_euclid(1.0) * 6.0;
        let i = h.floor();
        let f = h - i;
        let p = v * (1.0 - s);
        let q = v * (1.0 - s * f);
        let t = v * (1.0 - s * (1.0 - f));
        let (r, g, b) = match i as u32 % 6 {
            0 => (v, t, p),
            1 => (q, v, p),
            2 => (p, v, t),
            3 => (p, q, v),
            4 => (t, p, v),
            _ => (v, p, q),
        };
        Color::rgb(r, g, b)
    }

    /// Linear interpolation between two colors.
    pub fn lerp(a: Color, b: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        Color::rgba(
            a.r + (b.r - a.r) * t,
            a.g + (b.g - a.g) * t,
            a.b + (b.b - a.b) * t,
            a.a + (b.a - a.a) * t,
        )
    }

    /// White.
    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);
    /// Black.
    pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);
    /// Red.
    pub const RED: Color = Color::rgb(1.0, 0.25, 0.25);
    /// Green.
    pub const GREEN: Color = Color::rgb(0.25, 1.0, 0.35);
    /// Blue.
    pub const BLUE: Color = Color::rgb(0.3, 0.5, 1.0);
    /// Yellow.
    pub const YELLOW: Color = Color::rgb(1.0, 0.9, 0.25);
    /// Cyan.
    pub const CYAN: Color = Color::rgb(0.25, 0.9, 1.0);
    /// Magenta.
    pub const MAGENTA: Color = Color::rgb(1.0, 0.35, 0.9);
    /// Orange.
    pub const ORANGE: Color = Color::rgb(1.0, 0.6, 0.2);
    /// Gray.
    pub const GRAY: Color = Color::rgb(0.55, 0.55, 0.6);
    /// Dark gray.
    pub const DARK: Color = Color::rgb(0.12, 0.12, 0.15);
}

/// One triangle-list vertex — position in pixel coordinates plus RGBA color.
///
/// The layout mirrors the Metal Shading Language declaration every shader
/// in this directory uses:
///
/// ```metal
/// struct GuiVertex { float2 pos; float4 color; };
/// ```
///
/// MSL lays structs out like C++11, so `color` is padded to the 16-byte
/// alignment of `float4` and the stride is 32 bytes. A packed 24-byte Rust
/// struct reads as garbage: vertex `n` would take its position from the
/// colour channels of earlier vertices.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct Vertex {
    /// X position (pixels, origin top-left).
    pub x: f32,
    /// Y position (pixels, origin top-left).
    pub y: f32,
    /// Padding that aligns `r` to 16 bytes (must stay zero).
    pub pad: [f32; 2],
    /// Red.
    pub r: f32,
    /// Green.
    pub g: f32,
    /// Blue.
    pub b: f32,
    /// Alpha.
    pub a: f32,
}

impl Vertex {
    /// Vertex at `(x, y)` painted with `color`.
    pub fn new(x: f32, y: f32, color: Color) -> Vertex {
        Vertex {
            x,
            y,
            pad: [0.0; 2],
            r: color.r,
            g: color.g,
            b: color.b,
            a: color.a,
        }
    }
}

/// Keyboard/mouse state for the current frame.
#[derive(Clone, Default)]
pub struct Input {
    keys_down: HashSet<u16>,
    keys_pressed: HashSet<u16>,
    /// `(x, y)` in drawing units, origin top-left. Until the first mouse
    /// event it is the canvas centre, so nothing heads for the corner.
    mouse_pos: (f32, f32),
    /// Has a mouse event been received yet?
    mouse_known: bool,
    mouse_down: bool,
    mouse_pressed: bool,
    mouse_released: bool,
    text: String,
    shift: bool,
    ctrl: bool,
    opt: bool,
    cmd: bool,
}

impl Input {
    /// Is `code` currently held down?
    pub fn key(&self, code: u16) -> bool {
        self.keys_down.contains(&code)
    }

    /// Was `code` pressed this frame?
    pub fn key_pressed(&self, code: u16) -> bool {
        self.keys_pressed.contains(&code)
    }

    /// Characters typed — or pasted with `⌘V` — this frame: printable text
    /// of any script (lowercase letters, Korean jamo and syllables, …).
    /// Control characters never reach the queue, and line breaks from
    /// pasteboard content arrive as spaces.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Mouse position in drawing units, origin top-left.
    pub fn mouse(&self) -> (f32, f32) {
        self.mouse_pos
    }

    /// Left mouse button held down?
    pub fn mouse_down(&self) -> bool {
        self.mouse_down
    }

    /// Left mouse button pressed this frame?
    pub fn mouse_pressed(&self) -> bool {
        self.mouse_pressed
    }

    /// Left mouse button released this frame?
    pub fn mouse_released(&self) -> bool {
        self.mouse_released
    }

    /// Shift modifier held?
    pub fn shift(&self) -> bool {
        self.shift
    }

    /// Control modifier held?
    pub fn ctrl(&self) -> bool {
        self.ctrl
    }

    /// Option/Alt modifier held?
    pub fn opt(&self) -> bool {
        self.opt
    }

    /// Command modifier held?
    pub fn cmd(&self) -> bool {
        self.cmd
    }

    fn begin_frame(&mut self) {
        self.keys_pressed.clear();
        self.mouse_pressed = false;
        self.mouse_released = false;
        self.text.clear();
    }
}

/// One `draw_custom` entry to encode after the default pipeline pass.
struct CustomDraw {
    pso: Raw,
    first_vertex: u32,
    vertex_count: u32,
    uniform_offset_bytes: u32,
}

/// Per-frame drawing surface handed to the example's closure.
///
/// The coordinates handed to the drawing methods are *drawing units*; see
/// the module docs for the default half-point grid and for
/// [`App::new_points`].
pub struct Frame {
    /// Canvas width in drawing units.
    pub width: f32,
    /// Canvas height in drawing units.
    pub height: f32,
    /// Seconds since the app started.
    pub time: f32,
    /// Seconds since the previous frame (clamped to 0.1).
    pub dt: f32,
    /// Multiply a size given in points by this to get drawing units.
    pub scale: f32,
    /// Input state for this frame.
    pub input: Input,

    clear: Color,
    verts: Vec<Vertex>,
    customs: Vec<CustomDraw>,
    pending_uniforms: Vec<Vec<f32>>,
    /// Text quads are collected separately and drawn LAST so fullscreen
    /// custom shaders can never bury captions.
    text_verts: Vec<Vertex>,
    /// Canvas units per drawing unit (1 on the default grid, 2 for an app
    /// laid out in points).
    zoom: f32,
    /// Canvas size in canvas units — what the vertex shader divides the
    /// vertex positions by (unlike `width` / `height`, this is not in
    /// drawing units).
    canvas: (f32, f32),
    /// Device pixels per drawing unit: text is snapped to whole device
    /// pixels with it.
    px_per_unit: f32,
}

impl Frame {
    /// `(mouse_x, mouse_y)` in drawing units, origin top-left.
    pub fn mouse(&self) -> (f32, f32) {
        self.input.mouse_pos
    }

    /// Mouse X in drawing units.
    pub fn mouse_x(&self) -> f32 {
        self.mouse().0
    }

    /// Mouse Y in drawing units.
    pub fn mouse_y(&self) -> f32 {
        self.mouse().1
    }

    /// Device-pixel cap height the glyphs for `s` should be rasterized at,
    /// in drawing units: `size` rounded to whole device pixels (text stays
    /// crisp at any backing scale; never below one device pixel), then
    /// stepped down pixel by pixel while `s` would be wider than the canvas
    /// — a long caption shrinks instead of being cut off at both ends,
    /// which matters because rounding a half-pixel size up can make a line
    /// a third wider than it was laid out to be.
    fn fitted_size(&self, size: f32, s: &str) -> f32 {
        let k = self.px_per_unit;
        let mut px = (size * k).round().max(1.0);
        while px > 1.0 && font::measure(px / k, s, k) > self.width {
            px -= 1.0;
        }
        px / k
    }

    /// Append a filled `w`×`h` quad (two triangles, six vertices) to
    /// `verts`, converting drawing units to canvas units.
    fn quad(verts: &mut Vec<Vertex>, z: f32, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let (x, y, x1, y1) = (x * z, y * z, (x + w) * z, (y + h) * z);
        verts.extend_from_slice(&[
            Vertex::new(x, y, color),
            Vertex::new(x1, y, color),
            Vertex::new(x, y1, color),
            Vertex::new(x1, y, color),
            Vertex::new(x1, y1, color),
            Vertex::new(x, y1, color),
        ]);
    }

    /// Set the background color used to clear this frame.
    pub fn set_clear(&mut self, color: Color) {
        self.clear = color;
    }

    /// Replace the system clipboard's plain-text content — the write side
    /// of `⌘C` / `⌘X` (the read side, `⌘V`, is wired to [`Input::text`]).
    pub fn set_clipboard(&mut self, text: &str) {
        unsafe {
            let board = NSPasteboard::generalPasteboard();
            board.clearContents();
            let ty = NSString::from_str(NSPASTEBOARD_TYPE_STRING);
            let s = NSString::from_str(text);
            board.setString_forType(s.as_ref(), ty.as_ref());
        }
    }

    /// Add a filled rectangle.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let z = self.zoom;
        Self::quad(&mut self.verts, z, x, y, w, h, color);
    }

    /// Add a rectangle outline `border` drawing units thick.
    pub fn frame_rect(&mut self, x: f32, y: f32, w: f32, h: f32, border: f32, color: Color) {
        self.rect(x, y, w, border, color);
        self.rect(x, y + h - border, w, border, color);
        self.rect(x, y + border, border, h - 2.0 * border, color);
        self.rect(x + w - border, y + border, border, h - 2.0 * border, color);
    }

    /// Add a triangle from three points.
    pub fn tri(&mut self, a: (f32, f32), b: (f32, f32), c: (f32, f32), color: Color) {
        let z = self.zoom;
        self.verts.push(Vertex::new(a.0 * z, a.1 * z, color));
        self.verts.push(Vertex::new(b.0 * z, b.1 * z, color));
        self.verts.push(Vertex::new(c.0 * z, c.1 * z, color));
    }

    /// Add a thick line segment (`width` in drawing units).
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Color) {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-4 {
            return;
        }
        let (nx, ny) = (-dy / len * width * 0.5, dx / len * width * 0.5);
        self.tri(
            (x0 + nx, y0 + ny),
            (x1 + nx, y1 + ny),
            (x0 - nx, y0 - ny),
            color,
        );
        self.tri(
            (x1 + nx, y1 + ny),
            (x1 - nx, y1 - ny),
            (x0 - nx, y0 - ny),
            color,
        );
    }

    /// Add a filled circle (`segments` edge triangles, default 40).
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, color: Color) {
        self.circle_segments(cx, cy, r, 40, color);
    }

    /// Add a filled circle with an explicit segment count.
    pub fn circle_segments(&mut self, cx: f32, cy: f32, r: f32, segments: usize, color: Color) {
        if segments < 3 {
            return;
        }
        for i in 0..segments {
            let a0 = (i as f32) / segments as f32 * std::f32::consts::TAU;
            let a1 = ((i + 1) as f32) / segments as f32 * std::f32::consts::TAU;
            self.tri(
                (cx, cy),
                (cx + a0.cos() * r, cy + a0.sin() * r),
                (cx + a1.cos() * r, cy + a1.sin() * r),
                color,
            );
        }
    }

    /// Add an annulus (donut) from `r_inner` to `r_outer`.
    pub fn ring(&mut self, cx: f32, cy: f32, r_outer: f32, r_inner: f32, color: Color) {
        self.ring_segments(cx, cy, r_outer, r_inner, 48, color);
    }

    /// Add an annulus with an explicit segment count.
    pub fn ring_segments(
        &mut self,
        cx: f32,
        cy: f32,
        r_outer: f32,
        r_inner: f32,
        segments: usize,
        color: Color,
    ) {
        if segments < 3 {
            return;
        }
        // One quad per arc segment, between the inner and the outer edge.
        let edge = |angle: f32, r: f32| (cx + angle.cos() * r, cy + angle.sin() * r);
        for i in 0..segments {
            let a0 = (i as f32) / segments as f32 * std::f32::consts::TAU;
            let a1 = ((i + 1) as f32) / segments as f32 * std::f32::consts::TAU;
            let (o0, o1) = (edge(a0, r_outer), edge(a1, r_outer));
            let (i0, i1) = (edge(a0, r_inner), edge(a1, r_inner));
            self.tri(o0, o1, i0, color);
            self.tri(o1, i1, i0, color);
        }
    }

    /// Draw one line of text, top-left anchored. `size` sets the cap height
    /// to `7 * size` drawing units (the 5x7 font's uppercase height, which
    /// the layouts are built on), rounded to whole device pixels by
    /// [`Frame::fitted_size`](Self::fitted_size). Glyphs are rasterized from
    /// Apple SD Gothic Neo (see the [`font`] module) into coverage quads —
    /// one blended quad per horizontal run of equal coverage, with pen
    /// positions and the baseline snapped to the device pixel grid. Text is
    /// drawn after every custom shader pass, so it always sits on top.
    pub fn text(&mut self, x: f32, y: f32, size: f32, s: &str, color: Color) {
        let (size, z) = (self.fitted_size(size, s), self.zoom);
        let k = self.px_per_unit;
        let cap_px = font::cap_height_px(size, k);
        let mut pen_px = (x * k).round() as i64;
        let baseline_px = (y * k).round() as i64 + cap_px as i64;
        for g in font::glyphs(cap_px, s) {
            let (x0, y0) = (pen_px + g.left as i64, baseline_px - g.top as i64);
            for row in 0..g.h {
                for (start, len, cov) in g.runs(row) {
                    // The coverage becomes the quad's alpha, so the
                    // anti-aliased raster composites exactly as if it had
                    // been blitted from a texture.
                    let alpha = cov as f32 / 255.0 * color.a;
                    Self::quad(
                        &mut self.text_verts,
                        z,
                        (x0 + start as i64) as f32 / k,
                        (y0 + row as i64) as f32 / k,
                        len as f32 / k,
                        1.0 / k,
                        Color::rgba(color.r, color.g, color.b, alpha),
                    );
                }
            }
            pen_px += g.advance as i64;
        }
    }

    /// Draw text centered horizontally at `cx`.
    pub fn text_center(&mut self, cx: f32, y: f32, size: f32, s: &str, color: Color) {
        let w = self.text_width(size, s);
        self.text(cx - w * 0.5, y, size, s, color);
    }

    /// Width of `s` rendered at `size` (for layout math).
    pub fn text_width(&self, size: f32, s: &str) -> f32 {
        font::measure(self.fitted_size(size, s), s, self.px_per_unit)
    }

    /// Draw triangles with a custom pipeline (see the module docs for the
    /// vertex/uniform conventions). Positions are in drawing units like
    /// everything else; the uniform chunk reports the canvas in *canvas*
    /// units, so `pos / u[0].xy` still lands in normalized coordinates.
    pub fn draw_custom(
        &mut self,
        pso: &MTLRenderPipelineState,
        uniforms: &[f32],
        verts: &[Vertex],
    ) {
        if verts.is_empty() || self.verts.len() + verts.len() > self.vert_capacity() {
            return;
        }
        let first = self.verts.len() as u32;
        let z = self.zoom;
        self.verts.extend(
            verts
                .iter()
                .map(|v| Vertex::new(v.x * z, v.y * z, Color::rgba(v.r, v.g, v.b, v.a))),
        );
        let uniform_offset_bytes = self.customs.len() as usize * UNIFORM_CHUNK_FLOATS * 4;
        if (uniform_offset_bytes as u64 + UNIFORM_CHUNK_FLOATS as u64 * 4) > UNIFORM_BUFFER_BYTES {
            self.verts.truncate(first as usize);
            return;
        }
        self.customs.push(CustomDraw {
            pso: pso.as_raw(),
            first_vertex: first,
            vertex_count: verts.len() as u32,
            uniform_offset_bytes: uniform_offset_bytes as u32,
        });
        self.pending_uniforms.push(uniforms.to_vec());
    }

    fn vert_capacity(&self) -> usize {
        (VERTEX_BUFFER_BYTES / std::mem::size_of::<Vertex>() as u64) as usize
    }
}

// ---------------------------------------------------------------------------
// App.
// ---------------------------------------------------------------------------

/// Tiny xorshift64\* PRNG — the examples' only randomness source (the crate
/// is zero-dependency, so there is no `rand`). Seed it from
/// [`Rng::from_entropy`].
pub struct Rng(u64);

impl Rng {
    /// Deterministic PRNG from an explicit seed.
    pub fn new(seed: u64) -> Rng {
        Rng(seed.max(1))
    }

    /// Seed from the current wall-clock nanos.
    pub fn from_entropy() -> Rng {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15);
        Rng::new(nanos ^ 0x2545F4914F6CDD1D)
    }

    /// Next raw 64-bit value.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    /// Uniform `f32` in `0.0..1.0`.
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1 << 24) as f32
    }

    /// Uniform `i64` in `lo..=hi` (inclusive; `hi >= lo`).
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % (hi - lo + 1) as u64) as i64
    }

    /// Uniform `usize` index in `0..len` (`len > 0`).
    pub fn index(&mut self, len: usize) -> usize {
        self.range(0, len as i64 - 1) as usize
    }
}

/// Windowed Metal application — create one, then hand a per-frame closure to
/// [`App::run`].
pub struct App {
    device: Strong<MTLDevice>,
    queue: Strong<MTLCommandQueue>,
    layer: Strong<CAMetalLayer>,
    window: Strong<NSWindow>,
    ns_app: Strong<NSApplication>,
    default_pso: Strong<MTLRenderPipelineState>,
    vertex_buffer: Strong<MTLBuffer>,
    uniform_buffer: Strong<MTLBuffer>,
    /// Multisampled render target, resolved into the drawable every frame.
    msaa_texture: Strong<MTLTexture>,
    input: Input,
    started: Instant,
    last_frame: Instant,
    width_points: f32,
    height_points: f32,
    /// Backing scale (device pixels per point) of the display the window
    /// is on.
    backing_scale: f32,
    /// Canvas units per drawing unit (1, or 2 for an app laid out in
    /// points).
    zoom: f32,
    running: bool,
}

impl App {
    /// Create a window titled `title` with a `width`×`height` point content
    /// area and a `CAMetalLayer` swapchain attached to its content view.
    /// The drawing canvas is `2 × width` by `2 × height` units.
    ///
    /// # Panics
    /// Panics when no Metal device is available or AppKit fails to
    /// initialize (i.e. not running on macOS).
    pub fn new(title: &str, width: f64, height: f64) -> App {
        App::build(title, width, height, 1.0)
    }

    /// Like [`App::new`], for an app whose layout is written in points: the
    /// drawing canvas is exactly `width × height` drawing units, so a
    /// layout computed from the window size needs no conversion.
    pub fn new_points(title: &str, width: f64, height: f64) -> App {
        App::build(title, width, height, UNITS_PER_POINT)
    }

    fn build(title: &str, width: f64, height: f64, zoom: f32) -> App {
        // Load AppKit and QuartzCore before any Objective-C class lookup.
        appkit_force_link();
        metal_cpp_rs::quartz_core::quartz_core_force_link();

        unsafe {
            let device = MTLCreateSystemDefaultDevice();
            if device.is_nil() {
                panic!("MTLCreateSystemDefaultDevice() returned nil — Metal is unavailable");
            }
            let queue = device.newCommandQueue();
            if queue.is_nil() {
                panic!("newCommandQueue() returned nil");
            }

            let ns_app = NSApplication::sharedApplication();
            ns_app.setActivationPolicy(ACTIVATION_POLICY_REGULAR);
            // `run` pumps the event loop below in place of
            // `-[NSApplication run]`, so the start-up half of AppKit has to
            // be driven by hand: until `finishLaunching` has run the app is
            // not registered with the window server, cannot activate, and
            // its window never becomes key (so no keyboard input arrives).
            ns_app.finishLaunching();
            install_main_menu(&ns_app, title);

            let layer = CAMetalLayer::layer();
            layer.setDevice(device.as_ref());
            layer.setPixelFormat(MTLPixelFormat::PixelFormatBGRA8Unorm);
            layer.setFramebufferOnly(true);
            layer.setMaximumDrawableCount(3);

            let window = NSWindow::init_with_content_rect(
                NSWindow::alloc(),
                NSRect {
                    origin: NSPoint { x: 0.0, y: 0.0 },
                    size: CGSize::Make(width, height),
                },
                WINDOW_STYLE_DEFAULT,
                WINDOW_BACKING_BUFFERED,
                false,
            );
            // The `Strong` owns the single +1 of alloc/init. Programmatic
            // windows default to `releasedWhenClosed = YES`, which would
            // make AppKit drop a reference it does not hold.
            window.setReleasedWhenClosed(false);
            window.setTitle(NSString::from_str(title).as_ref());
            window.setAcceptsMouseMovedEvents(true);
            // `cascadeTopLeftFromPoint:` only computes a position; the
            // window has to be moved there itself.
            let top_left = window.cascadeTopLeftFromPoint(NSPoint { x: 80.0, y: 60.0 });
            window.setFrameTopLeftPoint(top_left);

            // Layer-hosting view: assign the layer first, then turn on
            // layer backing (the order AppKit documents).
            let content = window.contentView();
            content.setLayer(layer.as_ref().as_raw());
            content.setWantsLayer(true);

            let bounds = content.bounds();
            let scale = window.backingScaleFactor().max(1.0);
            set_layer_contents_scale(layer.as_ref().as_raw(), scale);
            layer.setDrawableSize(CGSize::Make(
                bounds.size.width * scale,
                bounds.size.height * scale,
            ));

            ns_app.activate();
            window.makeKeyAndOrderFront(std::ptr::null_mut());

            let default_pso = make_pipeline(&device, DEFAULT_SHADER);
            let vertex_buffer = device.newBuffer(
                VERTEX_BUFFER_BYTES,
                MTLResourceOptions::ResourceStorageModeShared,
            );
            let uniform_buffer = device.newBuffer(
                UNIFORM_BUFFER_BYTES,
                MTLResourceOptions::ResourceStorageModeShared,
            );

            App {
                device,
                queue,
                layer,
                window,
                ns_app,
                default_pso,
                vertex_buffer,
                uniform_buffer,
                msaa_texture: Strong::nil(),
                input: Input::default(),
                started: Instant::now(),
                last_frame: Instant::now(),
                width_points: bounds.size.width as f32,
                height_points: bounds.size.height as f32,
                backing_scale: scale as f32,
                zoom,
                running: true,
            }
        }
    }

    /// The Metal device backing this app.
    pub fn device(&self) -> &Strong<MTLDevice> {
        &self.device
    }

    /// Run the event loop until ESC or the close button ends it. Calls `f`
    /// once per frame with a fresh [`Frame`].
    ///
    /// # Panics
    /// Panics if the AppKit event loop cannot be entered (non-macOS host).
    pub fn run<F: FnMut(&mut Frame)>(mut self, mut f: F) {
        let harness = Harness::from_env();
        let report = harness.limited();
        if report {
            // Grabs read the drawable back, which `framebufferOnly` forbids.
            unsafe {
                self.layer.setFramebufferOnly(false);
                self.report_window("start");
            }
        }
        let wall = Instant::now();
        let mut index = 0u64;
        while self.running {
            autoreleasepool(|| unsafe {
                self.inject(&harness, index);
                self.pump_events();
                if self.running {
                    self.sync_size();
                    let mut frame = self.make_frame();
                    f(&mut frame);
                    if self.running {
                        self.render(&frame, &harness, index);
                    }
                }
                index += 1;
                if harness.is_done(index) {
                    self.running = false;
                }
            });
        }
        if report {
            unsafe { self.report_window("end") };
            let secs = wall.elapsed().as_secs_f64();
            eprintln!(
                "{}: {} frames in {:.2}s ({:.1} fps)",
                harness.name,
                index,
                secs,
                index as f64 / secs.max(1e-6)
            );
            std::process::exit(0);
        }
        unsafe { self.ns_app.terminate(std::ptr::null_mut()) };
    }

    /// One-line window/layer state dump for the test harness (`phase` tells
    /// start of run from end of run, since activation is asynchronous).
    unsafe fn report_window(&self, phase: &str) {
        let content = self.window.contentView();
        let view_layer = content.layer();
        let bounds = content.bounds();
        eprintln!(
            "{}[{}]: key={} active={} releasedWhenClosed={} viewLayer={} metalLayer={} contentsScale={} drawable={:?} bounds={}x{} scale={}",
            Harness::from_env().name,
            phase,
            self.window.isKeyWindow(),
            self.ns_app.isActive(),
            self.window.isReleasedWhenClosed(),
            class_name_of(view_layer),
            view_layer == self.layer.as_ref().as_raw(),
            layer_contents_scale(self.layer.as_ref().as_raw()),
            self.layer.drawableSize(),
            bounds.size.width,
            bounds.size.height,
            self.backing_scale,
        );
    }

    /// Post the synthesised events the harness schedules for frame `index`.
    unsafe fn inject(&mut self, harness: &Harness, index: u64) {
        if !harness.active() {
            return;
        }
        if harness.close_frame == Some(index) {
            self.window.performClose(std::ptr::null_mut());
        }
        let window = self.window.windowNumber();
        let origin = NSPoint { x: 0.0, y: 0.0 };
        for code in harness.downs(index) {
            let ev = NSEvent::key_event(
                EVENT_KEY_DOWN,
                origin,
                window,
                harness::key_char(code),
                code,
            );
            self.ns_app.postEvent_atStart(ev.as_ref(), false);
        }
        for code in harness.ups(index) {
            let ev =
                NSEvent::key_event(EVENT_KEY_UP, origin, window, harness::key_char(code), code);
            self.ns_app.postEvent_atStart(ev.as_ref(), false);
        }
        // `locationInWindow` is bottom-left anchored; the harness takes
        // top-left anchored points like the examples' drawing coordinates.
        for m in harness.mouse_at(index) {
            let location = NSPoint {
                x: m.x,
                y: self.height_points as f64 - m.y,
            };
            let kind = if m.hold_frames > 0 {
                EVENT_LEFT_MOUSE_DOWN
            } else {
                EVENT_MOUSE_MOVED
            };
            let ev = NSEvent::mouse_event(kind, location, window);
            self.ns_app.postEvent_atStart(ev.as_ref(), false);
        }
        for (x, y) in harness.mouse_up_at(index) {
            let location = NSPoint {
                x,
                y: self.height_points as f64 - y,
            };
            let ev = NSEvent::mouse_event(EVENT_LEFT_MOUSE_UP, location, window);
            self.ns_app.postEvent_atStart(ev.as_ref(), false);
        }
        // Text injection: one keydown carrying the whole string as its
        // `characters` — the same shape a real keyboard layout (or an
        // input method's commit) produces. Key code 0 is arbitrary: the
        // text path reads `characters`, not `keyCode`.
        if let Some((frame, text)) = &harness.text {
            if *frame == index && !text.is_empty() {
                let ev = NSEvent::key_event(EVENT_KEY_DOWN, origin, window, text, 0);
                self.ns_app.postEvent_atStart(ev.as_ref(), false);
            }
        }
    }

    unsafe fn pump_events(&mut self) {
        self.input.begin_frame();
        // Modifier state is polled, not tracked through flags-changed
        // events: those only reach a key window, so a Shift held before the
        // user clicks into the window would go unnoticed.
        let flags = current_modifier_flags();
        self.input.shift = flags & MODIFIER_FLAG_SHIFT != 0;
        self.input.ctrl = flags & MODIFIER_FLAG_CONTROL != 0;
        self.input.opt = flags & MODIFIER_FLAG_OPTION != 0;
        self.input.cmd = flags & MODIFIER_FLAG_COMMAND != 0;
        let mode = NSString::from_str("kCFRunLoopDefaultMode");
        // Block briefly for the next event, then drain everything pending.
        let mut until = NSDate::dateWithTimeIntervalSinceNow(1.0 / 120.0);
        loop {
            let event = self.ns_app.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSUInteger::MAX,
                until.as_ref().as_raw(),
                mode.as_ref().as_raw(),
                true,
            );
            if event.is_nil() {
                break;
            }
            if self.handle_event(event.as_ref()) {
                self.ns_app.sendEvent(event.as_ref());
            }
            if !self.running {
                break;
            }
            until = NSDate::distantPast();
        }
        if self.running {
            self.running = self.window.isVisible() || self.window.isMiniaturized();
        }
    }

    /// Handle an event for GUI bookkeeping. Returns `true` when the event
    /// should still be forwarded to AppKit (`-sendEvent:`): mouse events
    /// (window dragging, click-to-activate) and Command-key events, which
    /// the main menu turns into `⌘Q` / `⌘W` — except the clipboard
    /// shortcuts, which are handled here. Every other key event is
    /// consumed here, because a bare `NSView` does not implement
    /// `keyDown:` and AppKit beeps for each one it is handed.
    unsafe fn handle_event(&mut self, ev: Ref<NSEvent>) -> bool {
        match ev.eventType() {
            EVENT_KEY_DOWN => {
                let code = ev.keyCode();
                if code == key::ESC {
                    self.running = false;
                    return false;
                }
                if self.input.keys_down.insert(code) {
                    self.input.keys_pressed.insert(code);
                }
                let flags = ev.modifierFlags();
                let cmd = flags & MODIFIER_FLAG_COMMAND != 0;
                // Clipboard shortcuts ride the Command slot: ⌘V pulls the
                // pasteboard straight onto the typed-text queue, ⌘C / ⌘X
                // are left to the example (which knows what its input
                // holds, see `Frame::set_clipboard`) — but they are
                // consumed here either way, so AppKit never beeps at them.
                if cmd && flags & (MODIFIER_FLAG_CONTROL | MODIFIER_FLAG_OPTION) == 0 {
                    match code {
                        key::V => self.paste_from_clipboard(),
                        key::C | key::X => {}
                        _ => return cmd,
                    }
                    return false;
                }
                // `characters` of a shortcut is not something the user typed.
                if flags & (MODIFIER_FLAG_COMMAND | MODIFIER_FLAG_CONTROL) == 0 {
                    let chars = ev.characters();
                    if !chars.is_nil() {
                        for ch in chars.to_string().chars() {
                            // Printable text of any script: lowercase
                            // letters, Korean jamo, punctuation. Control
                            // characters (Enter arrives as a key code,
                            // not text) never reach the queue.
                            if !ch.is_control() {
                                self.input.text.push(ch);
                            }
                        }
                    }
                }
                cmd
            }
            EVENT_KEY_UP => {
                self.input.keys_down.remove(&ev.keyCode());
                false
            }
            EVENT_LEFT_MOUSE_DOWN => {
                self.input.mouse_down = true;
                self.input.mouse_pressed = true;
                self.update_mouse(ev);
                true
            }
            EVENT_LEFT_MOUSE_UP => {
                self.input.mouse_down = false;
                self.input.mouse_released = true;
                self.update_mouse(ev);
                true
            }
            EVENT_RIGHT_MOUSE_DOWN | EVENT_RIGHT_MOUSE_UP => true,
            EVENT_MOUSE_MOVED | EVENT_LEFT_MOUSE_DRAGGED | EVENT_RIGHT_MOUSE_DRAGGED => {
                self.update_mouse(ev);
                true
            }
            _ => true,
        }
    }

    /// Read the general pasteboard's plain-text content (⌘V) and queue it
    /// as if typed, through [`Input::text`]. Line breaks and tabs become
    /// spaces — the input is a single line — and control characters are
    /// dropped.
    unsafe fn paste_from_clipboard(&mut self) {
        let board = NSPasteboard::generalPasteboard();
        let ty = NSString::from_str(NSPASTEBOARD_TYPE_STRING);
        let text = board.stringForType(ty.as_ref());
        if text.is_nil() {
            return;
        }
        // Normalize CRLF / CR to LF first, so Windows-style pasteboard
        // content does not turn into two spaces.
        let raw = text.to_string();
        let mut normalized = String::with_capacity(raw.len());
        let mut chars = raw.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\r' {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                normalized.push('\n');
            } else {
                normalized.push(c);
            }
        }
        for ch in normalized.chars() {
            match ch {
                '\n' | '\t' => self.input.text.push(' '),
                c if !c.is_control() => self.input.text.push(c),
                _ => {}
            }
        }
    }

    unsafe fn update_mouse(&mut self, ev: Ref<NSEvent>) {
        let p = ev.locationInWindow();
        // `locationInWindow` is bottom-left anchored and in points; the
        // position is kept in canvas units.
        self.input.mouse_pos = (
            p.x as f32 * UNITS_PER_POINT,
            (self.height_points as f64 - p.y) as f32 * UNITS_PER_POINT,
        );
        self.input.mouse_known = true;
    }

    unsafe fn sync_size(&mut self) {
        let content = self.window.contentView();
        let bounds = content.bounds();
        // Follows the window across displays: the backing scale changes as
        // soon as the larger part of the window sits on another screen.
        let scale = self.window.backingScaleFactor().max(1.0);
        if (bounds.size.width as f32) != self.width_points
            || (bounds.size.height as f32) != self.height_points
            || (scale as f32) != self.backing_scale
        {
            self.width_points = bounds.size.width as f32;
            self.height_points = bounds.size.height as f32;
            self.backing_scale = scale as f32;
            set_layer_contents_scale(self.layer.as_ref().as_raw(), scale);
            self.layer.setDrawableSize(CGSize::Make(
                bounds.size.width * scale,
                bounds.size.height * scale,
            ));
        }
    }

    fn make_frame(&mut self) -> Frame {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        // Canvas size in units, before conversion to drawing units.
        let (uw, uh) = (
            self.width_points * UNITS_PER_POINT,
            self.height_points * UNITS_PER_POINT,
        );
        let (zoom, backing) = (self.zoom, self.backing_scale);
        let mut input = self.input.clone();
        input.mouse_pos = if input.mouse_known {
            (input.mouse_pos.0 / zoom, input.mouse_pos.1 / zoom)
        } else {
            // No mouse event yet: report the centre of the canvas.
            (uw * 0.5 / zoom, uh * 0.5 / zoom)
        };
        Frame {
            width: uw / zoom,
            height: uh / zoom,
            time: self.started.elapsed().as_secs_f32(),
            dt,
            scale: UNITS_PER_POINT / zoom,
            input,
            clear: Color::rgb(0.05, 0.05, 0.08),
            verts: Vec::with_capacity(4096),
            customs: Vec::new(),
            pending_uniforms: Vec::new(),
            text_verts: Vec::with_capacity(4096),
            zoom,
            canvas: (uw, uh),
            px_per_unit: zoom * backing / UNITS_PER_POINT,
        }
    }

    /// Make sure the multisampled target matches the drawable's size.
    unsafe fn ensure_msaa_target(&mut self, drawable_texture: Ref<MTLTexture>) {
        let (w, h) = (drawable_texture.width(), drawable_texture.height());
        if !self.msaa_texture.is_nil()
            && self.msaa_texture.width() == w
            && self.msaa_texture.height() == h
        {
            return;
        }
        let descriptor = MTLTextureDescriptor::texture2DDescriptor(
            MTLPixelFormat::PixelFormatBGRA8Unorm,
            w,
            h,
            false,
        );
        // A multisampled texture is its own texture type: with the plain
        // `2D` type Metal rejects `sampleCount > 1` ("type that does not
        // allow sampleCount").
        descriptor.setTextureType(MTLTextureType::TextureType2DMultisample);
        descriptor.setSampleCount(SAMPLE_COUNT);
        descriptor.setUsage(MTLTextureUsage::TextureUsageRenderTarget);
        // Only ever bound as a color attachment: keep it on the GPU.
        descriptor.setStorageMode(MTLStorageMode::StorageModePrivate);
        self.msaa_texture = self.device.newTexture(descriptor.as_ref());
    }

    unsafe fn render(&mut self, frame: &Frame, harness: &Harness, index: u64) {
        let drawable = self.layer.nextDrawable();
        if drawable.is_nil() {
            if harness.limited() {
                eprintln!(
                    "{}: frame {index}: nextDrawable() returned nil",
                    harness.name
                );
            }
            return;
        }

        // Upload vertices + uniform chunks into the shared scratch buffers.
        let vert_dst = self.vertex_buffer.contents() as *mut Vertex;
        let total_verts = frame.verts.len() + frame.text_verts.len();
        let vert_bytes = total_verts * std::mem::size_of::<Vertex>();
        if vert_bytes > VERTEX_BUFFER_BYTES as usize {
            return;
        }
        if !frame.verts.is_empty() {
            std::ptr::copy_nonoverlapping(frame.verts.as_ptr(), vert_dst, frame.verts.len());
        }
        if !frame.text_verts.is_empty() {
            std::ptr::copy_nonoverlapping(
                frame.text_verts.as_ptr(),
                vert_dst.add(frame.verts.len()),
                frame.text_verts.len(),
            );
        }
        // Upload uniform chunks into the shared scratch buffer. Chunk `n`
        // serves the `n`-th custom draw: `u[0]` = frame globals, `u[1..]` =
        // the example's floats (at most 60, the rest of the 64-float chunk).
        // The globals carry the canvas in *canvas* units: that is the space
        // the vertex positions are in.
        let unif_dst = self.uniform_buffer.contents() as *mut f32;
        let globals = [frame.canvas.0, frame.canvas.1, frame.time, frame.scale];
        for (chunk, uniforms) in frame.pending_uniforms.iter().enumerate() {
            let base = chunk * UNIFORM_CHUNK_FLOATS;
            for (i, v) in globals.iter().enumerate() {
                *unif_dst.add(base + i) = *v;
            }
            for (i, v) in uniforms.iter().take(UNIFORM_CHUNK_FLOATS - 4).enumerate() {
                *unif_dst.add(base + 4 + i) = *v;
            }
        }
        // The stock pipeline reads chunk 0 even when there is no custom draw.
        for (i, v) in globals.iter().enumerate() {
            *unif_dst.add(i) = *v;
        }

        // Scene renders into the multisampled target and resolves into the
        // drawable in the same pass (`StoreActionMultisampleResolve`).
        let target = drawable.texture();
        self.ensure_msaa_target(target);
        if self.msaa_texture.is_nil() {
            return;
        }

        let render_pass = MTLRenderPassDescriptor::renderPassDescriptor();
        let color0 = render_pass.colorAttachments().objectAtIndexedSubscript(0);
        let attachment = color0.as_attachment();
        attachment.setTexture(self.msaa_texture.as_ref());
        attachment.setResolveTexture(target);
        attachment.setLoadAction(MTLLoadAction::LoadActionClear);
        attachment.setStoreAction(MTLStoreAction::StoreActionMultisampleResolve);
        color0.setClearColor(metal_cpp_rs::metal::types::MTLClearColor::Make(
            frame.clear.r as f64,
            frame.clear.g as f64,
            frame.clear.b as f64,
            frame.clear.a as f64,
        ));

        let command_buffer = self.queue.commandBuffer();
        let encoder = command_buffer.renderCommandEncoder(render_pass.as_ref());

        if !frame.verts.is_empty() {
            // The vertex shader converts unit coordinates to NDC with the
            // canvas size it reads from the vertex-stage uniform buffer.
            encoder.setRenderPipelineState(self.default_pso.as_ref());
            encoder.setVertexBuffer(self.vertex_buffer.as_ref(), 0, 0);
            encoder.setVertexBuffer(self.uniform_buffer.as_ref(), 0, 1);
            encoder.drawPrimitives(
                MTLPrimitiveType::PrimitiveTypeTriangle,
                0,
                frame.verts.len() as u64,
            );
        }

        for entry in &frame.customs {
            let pso = Ref::<MTLRenderPipelineState>::from_raw(entry.pso);
            encoder.setRenderPipelineState(pso);
            encoder.setVertexBuffer(self.vertex_buffer.as_ref(), 0, 0);
            encoder.setVertexBuffer(
                self.uniform_buffer.as_ref(),
                entry.uniform_offset_bytes as u64,
                1,
            );
            encoder.setFragmentBuffer(
                self.uniform_buffer.as_ref(),
                entry.uniform_offset_bytes as u64,
                0,
            );
            encoder.drawPrimitives(
                MTLPrimitiveType::PrimitiveTypeTriangle,
                entry.first_vertex as u64,
                entry.vertex_count as u64,
            );
        }

        // Text is encoded last so it always sits above custom shaders.
        if !frame.text_verts.is_empty() {
            encoder.setRenderPipelineState(self.default_pso.as_ref());
            encoder.setVertexBuffer(self.vertex_buffer.as_ref(), 0, 0);
            encoder.setVertexBuffer(self.uniform_buffer.as_ref(), 0, 1);
            encoder.drawPrimitives(
                MTLPrimitiveType::PrimitiveTypeTriangle,
                frame.verts.len() as u64,
                frame.text_verts.len() as u64,
            );
        }

        encoder.endEncoding();

        // A grab copies the drawable into a shared buffer *before* the
        // drawable is handed to the compositor, so the PNG is exactly what
        // the window was given. Rows are padded to 256 bytes (the buffer
        // row-alignment requirement of some macOS GPU families).
        let grab = if harness.wants_shot(index) {
            let (w, h) = (target.width(), target.height());
            let bytes_per_row = (w * 4 + 255) & !255;
            let buffer = self.device.newBuffer(
                bytes_per_row * h,
                MTLResourceOptions::ResourceStorageModeShared,
            );
            let blit = command_buffer.blitCommandEncoder();
            blit.copyFromTextureToBuffer(
                target,
                0,
                0,
                MTLOrigin::Make(0, 0, 0),
                MTLSize::Make(w, h, 1),
                buffer.as_ref(),
                0,
                bytes_per_row,
                bytes_per_row * h,
            );
            blit.endEncoding();
            Some((buffer, w, h, bytes_per_row))
        } else {
            None
        };

        command_buffer.presentDrawable(drawable.as_ref().as_raw());
        command_buffer.commit();

        if harness.limited() {
            // Surface GPU-side failures instead of silently drawing garbage.
            command_buffer.waitUntilCompleted();
            if command_buffer.status() == MTLCommandBufferStatus::CommandBufferStatusError {
                let e = command_buffer.error();
                let msg = if e.is_nil() {
                    "<no error>".to_string()
                } else {
                    e.localizedDescription().to_string()
                };
                eprintln!(
                    "{}: frame {index}: command buffer error: {msg}",
                    harness.name
                );
            }
        }

        if let Some((buffer, w, h, bytes_per_row)) = grab {
            let src = buffer.contents() as *const u8;
            let mut rgba = vec![0u8; (w * h * 4) as usize];
            for y in 0..h as usize {
                for x in 0..w as usize {
                    // BGRA8Unorm bytes → RGBA.
                    let p = src.add(y * bytes_per_row as usize + x * 4);
                    let o = (y * w as usize + x) * 4;
                    rgba[o] = *p.add(2);
                    rgba[o + 1] = *p.add(1);
                    rgba[o + 2] = *p;
                    rgba[o + 3] = 255;
                }
            }
            let path = harness.shot_path(index);
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(e) = harness::write_png(&path, w as u32, h as u32, &rgba) {
                eprintln!("{}: could not write {}: {e}", harness.name, path.display());
            }
        }
    }
}

/// Build a render pipeline whose shaders use the GUI vertex layout: the
/// vertex function reads `const device GuiVertex* [[buffer(0)]]` (a
/// [`Vertex`]: `float2 pos; float4 color;`, 32 bytes with MSL padding) and
/// `constant float4* [[buffer(1)]]` uniforms (`u[0] = (width, height, time,
/// scale)` in units); name the functions `vertex_main` and `fragment_main`.
///
/// # Panics
/// Panics with the shader-compiler error when the MSL source is invalid.
pub fn make_pipeline(
    device: &Strong<MTLDevice>,
    msl_source: &str,
) -> Strong<MTLRenderPipelineState> {
    unsafe {
        let source = NSString::from_str(msl_source);
        let options = MTLCompileOptions::new();
        let mut error = Strong::<NSError>::nil();
        let library = device.newLibrary(source.as_ref(), options.as_ref(), &mut error);
        if library.is_nil() {
            let msg = if error.is_nil() {
                "<no error object>".to_string()
            } else {
                error.localizedDescription().to_string()
            };
            panic!("shader compile failed: {msg}");
        }
        let vertex_fn = library.newFunction(NSString::from_str("vertex_main").as_ref());
        let fragment_fn = library.newFunction(NSString::from_str("fragment_main").as_ref());
        if vertex_fn.is_nil() || fragment_fn.is_nil() {
            panic!("shader is missing vertex_main/fragment_main");
        }
        let desc = MTLRenderPipelineDescriptor::new();
        desc.setVertexFunction(vertex_fn.as_ref());
        desc.setFragmentFunction(fragment_fn.as_ref());
        // Must match the sample count of the attachment the pipeline draws
        // into (`App::render`).
        desc.setRasterSampleCount(SAMPLE_COUNT);
        let color0 = desc.colorAttachments().objectAtIndexedSubscript(0);
        color0.setPixelFormat(MTLPixelFormat::PixelFormatBGRA8Unorm);
        // Straight alpha blending — `rgba(.., a)` overlays and fades work
        // in every pipeline this helper builds (including the default one).
        color0.setBlendingEnabled(true);
        color0.setSourceRGBBlendFactor(MTLBlendFactor::BlendFactorSourceAlpha);
        color0.setDestinationRGBBlendFactor(MTLBlendFactor::BlendFactorOneMinusSourceAlpha);
        color0.setSourceAlphaBlendFactor(MTLBlendFactor::BlendFactorOne);
        color0.setDestinationAlphaBlendFactor(MTLBlendFactor::BlendFactorOneMinusSourceAlpha);
        let mut error = Strong::<NSError>::nil();
        let pso = device.newRenderPipelineState(desc.as_ref(), &mut error);
        if pso.is_nil() {
            let msg = if error.is_nil() {
                "<no error object>".to_string()
            } else {
                error.localizedDescription().to_string()
            };
            panic!("pipeline creation failed: {msg}");
        }
        pso
    }
}
