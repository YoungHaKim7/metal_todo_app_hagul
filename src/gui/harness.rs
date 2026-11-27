//! Headless test harness for the GUI examples.
//!
//! Everything here is opt-in through environment variables. When none of them
//! is set the examples run as ordinary interactive apps and this module does
//! nothing beyond a few `var_os` lookups at start-up.
//!
//! * `METAL_GUI_FRAMES=<n>` — render exactly `n` frames, then exit with
//!   status 0 instead of running until the window is closed.
//! * `METAL_GUI_SHOT_DIR=<dir>` — directory that receives frame grabs.
//! * `METAL_GUI_SHOT_FRAMES=<a,b,c>` — 0-based frame indices to grab. A grab
//!   copies the drawable that was just rendered into a shared buffer and
//!   writes it as `<dir>/<example>_f<a>.png` (the layer is switched to
//!   `framebufferOnly = NO` for the whole run).
//! * `METAL_GUI_KEYS=<frame:code:frame:code:...>` — synthesise key taps
//!   (`down` on the listed frame, `up` on the next one), e.g.
//!   `10:124,20:124` taps the left arrow on frames 10 and 20.
//! * `METAL_GUI_HOLD=<frame:code:frames>` — hold a key down for a number of
//!   frames, e.g. `10:13:40` holds `W` from frame 10 to frame 50.
//! * `METAL_GUI_MOUSE=<frame:x:y` or `frame:x:y:d>` — move the mouse to
//!   `(x, y)` points (origin top-left) on `frame`, optionally pressing the
//!   left button down until frame `frame + d`.
//! * `METAL_GUI_TEXT=<frame>:<utf8 text>` — inject `text` as one keydown's
//!   `characters` on `frame`, exactly what a real keyboard (or an input
//!   method's commit) delivers; this is how the text path is smoked
//!   headlessly, e.g. `METAL_GUI_TEXT=10:한글 abc`.
//! * `METAL_GUI_CLOSE=<frame>` — click the window's close button on `frame`
//!   (exercises the red-button shutdown path).
//!
//! The synthesised events are real `NSEvent` objects posted on the
//! application queue, so they travel the same `nextEventMatchingMask:` route
//! as events coming from the window server.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

// ---------------------------------------------------------------------------
// Environment.
// ---------------------------------------------------------------------------

/// A key tap: `frame`, HID `code`.
#[derive(Clone, Copy, Debug)]
pub struct KeyTap {
    /// Frame on which the key goes down (it goes up on the next frame).
    pub frame: u64,
    /// HID usage code (see [`crate::gui::key`]).
    pub code: u16,
}

/// A mouse action: move to `(x, y)` points on `frame`; press the left button
/// when `hold_frames > 0` and release it `hold_frames` frames later.
#[derive(Clone, Copy, Debug)]
pub struct MouseAt {
    /// Frame on which the action starts.
    pub frame: u64,
    /// X in points, origin top-left.
    pub x: f64,
    /// Y in points, origin top-left.
    pub y: f64,
    /// Number of frames to keep the button pressed (0 = move only).
    pub hold_frames: u64,
}

/// Parsed harness configuration (all fields empty when not enabled).
#[derive(Debug, Default)]
pub struct Harness {
    /// Total number of frames to render (`None` = run forever).
    pub frames: Option<u64>,
    /// Output directory for grabs.
    pub shot_dir: Option<PathBuf>,
    /// Frame indices to grab.
    pub shot_frames: Vec<u64>,
    /// Keys to tap.
    pub keys: Vec<KeyTap>,
    /// Keys held down: `(frame, code, frames)`.
    pub holds: Vec<(u64, u16, u64)>,
    /// Mouse moves / clicks.
    pub mouse: Vec<MouseAt>,
    /// Text injected as a keydown's characters on a frame.
    pub text: Option<(u64, String)>,
    /// Frame on which the window's close button is "clicked"
    /// (`-[NSWindow performClose:]`), if any.
    pub close_frame: Option<u64>,
    /// Example name used for grab file names.
    pub name: String,
}

fn var(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|v| !v.is_empty())
}

fn parse_list<T>(s: &OsString, f: impl FnMut(&str) -> Option<T>) -> Vec<T> {
    s.to_string_lossy().split(',').filter_map(f).collect()
}

impl Harness {
    /// Read the configuration from the environment.
    pub fn from_env() -> Harness {
        let mut h = Harness::default();
        h.frames = var("METAL_GUI_FRAMES").and_then(|v| v.to_string_lossy().parse().ok());
        h.shot_dir = var("METAL_GUI_SHOT_DIR").map(PathBuf::from);
        h.shot_frames = var("METAL_GUI_SHOT_FRAMES")
            .map(|v| parse_list(&v, |p| p.trim().parse().ok()))
            .unwrap_or_default();
        h.keys = var("METAL_GUI_KEYS")
            .map(|v| {
                parse_list(&v, |p| {
                    let (a, b) = p.trim().split_once(':')?;
                    Some(KeyTap {
                        frame: a.parse().ok()?,
                        code: b.parse().ok()?,
                    })
                })
            })
            .unwrap_or_default();
        h.holds = var("METAL_GUI_HOLD")
            .map(|v| {
                parse_list(&v, |p| {
                    let mut it = p.trim().split(':');
                    let frame = it.next()?.parse().ok()?;
                    let code = it.next()?.parse().ok()?;
                    let len = it.next()?.parse().ok()?;
                    Some((frame, code, len))
                })
            })
            .unwrap_or_default();
        h.mouse = var("METAL_GUI_MOUSE")
            .map(|v| {
                parse_list(&v, |p| {
                    let mut it = p.trim().split(':');
                    let frame = it.next()?.parse().ok()?;
                    let x = it.next()?.parse().ok()?;
                    let y = it.next()?.parse().ok()?;
                    let hold_frames = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    Some(MouseAt {
                        frame,
                        x,
                        y,
                        hold_frames,
                    })
                })
            })
            .unwrap_or_default();
        h.text = var("METAL_GUI_TEXT").and_then(|v| {
            let v = v.to_string_lossy().into_owned();
            let (frame, text) = v.split_once(':')?;
            Some((frame.trim().parse().ok()?, text.to_string()))
        });
        h.close_frame = var("METAL_GUI_CLOSE").and_then(|v| v.to_string_lossy().parse().ok());
        h.name = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "example".to_string());
        h
    }

    /// Is any harness option set (event injection runs)?
    pub fn active(&self) -> bool {
        self.limited()
            || self.close_frame.is_some()
            || !self.keys.is_empty()
            || !self.holds.is_empty()
            || !self.mouse.is_empty()
            || self.text.is_some()
    }

    /// Is the run frame-limited or grabbed (per-frame checks, status 0 exit)?
    pub fn limited(&self) -> bool {
        self.frames.is_some() || self.shot_dir.is_some()
    }

    /// Should the frame with index `index` be grabbed?
    pub fn wants_shot(&self, index: u64) -> bool {
        self.shot_dir.is_some() && self.shot_frames.contains(&index)
    }

    /// Path of the grab for frame `index`.
    pub fn shot_path(&self, index: u64) -> PathBuf {
        self.shot_dir
            .as_deref()
            .unwrap_or(Path::new("."))
            .join(format!("{}_f{index}.png", self.name))
    }

    /// Should the run stop once frame `index - 1` has been rendered?
    /// (`index` has already been advanced past the rendered frame.)
    pub fn is_done(&self, index: u64) -> bool {
        self.frames.is_some_and(|n| index >= n)
    }

    /// Keys that go down on frame `index` (taps and holds).
    pub fn downs(&self, index: u64) -> Vec<u16> {
        let mut v: Vec<u16> = self
            .keys
            .iter()
            .filter(|k| k.frame == index)
            .map(|k| k.code)
            .collect();
        v.extend(self.holds.iter().filter(|h| h.0 == index).map(|h| h.1));
        v
    }

    /// Keys that go up on frame `index` (taps and holds).
    pub fn ups(&self, index: u64) -> Vec<u16> {
        let mut v: Vec<u16> = self
            .keys
            .iter()
            .filter(|k| k.frame + 1 == index)
            .map(|k| k.code)
            .collect();
        v.extend(
            self.holds
                .iter()
                .filter(|h| h.0 + h.2 == index)
                .map(|h| h.1),
        );
        v
    }

    /// Mouse actions that start on frame `index`.
    pub fn mouse_at(&self, index: u64) -> impl Iterator<Item = &MouseAt> {
        self.mouse.iter().filter(move |m| m.frame == index)
    }

    /// Mouse releases due on frame `index`.
    pub fn mouse_up_at(&self, index: u64) -> Vec<(f64, f64)> {
        self.mouse
            .iter()
            .filter(|m| m.hold_frames > 0 && m.frame + m.hold_frames == index)
            .map(|m| (m.x, m.y))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Key characters.
// ---------------------------------------------------------------------------

/// The character a synthesised key event carries for an HID usage code
/// (US layout, no modifiers). `\u{7f}` is `NSDeleteCharacter` (backspace).
pub fn key_char(code: u16) -> &'static str {
    match code {
        0 => "a",
        1 => "s",
        2 => "d",
        3 => "f",
        4 => "h",
        5 => "g",
        6 => "z",
        7 => "x",
        8 => "c",
        9 => "v",
        11 => "b",
        12 => "q",
        13 => "w",
        14 => "e",
        15 => "r",
        16 => "y",
        17 => "t",
        18 => "1",
        19 => "2",
        20 => "3",
        21 => "4",
        22 => "6",
        23 => "5",
        24 => "=",
        25 => "9",
        26 => "7",
        27 => "-",
        28 => "8",
        29 => "0",
        30 => "]",
        31 => "o",
        32 => "u",
        33 => "[",
        34 => "i",
        35 => "p",
        36 => "\r",
        37 => "l",
        38 => "j",
        39 => "'",
        40 => "k",
        41 => ";",
        42 => "\\",
        43 => ",",
        44 => "/",
        45 => "n",
        46 => "m",
        47 => ".",
        48 => "\t",
        49 => " ",
        51 => "\u{7f}",
        53 => "\u{1b}",
        _ => "\0",
    }
}

// ---------------------------------------------------------------------------
// PNG (no dependencies: stored deflate blocks).
// ---------------------------------------------------------------------------

fn crc32(data: &[u8]) -> u32 {
    // Table-free, branch-free bitwise CRC-32 (IEEE).
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            // All-ones mask when the low bit is set, all-zeros otherwise.
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = data.chunks(65_535).collect();
    let n = chunks.len();
    for (i, c) in chunks.iter().enumerate() {
        z.push(if i + 1 == n { 1 } else { 0 });
        z.extend_from_slice(&(c.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        z.extend_from_slice(c);
    }
    z.extend_from_slice(&adler32(data).to_be_bytes());
    z
}

/// Encode 8-bit RGBA pixels (row-major, top row first) as a PNG file.
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> std::io::Result<()> {
    let stride = width as usize * 4;
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    for y in 0..height as usize {
        raw.push(0);
        raw.extend_from_slice(&rgba[y * stride..y * stride + stride]);
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    push_chunk(&mut out, b"IHDR", &ihdr);
    push_chunk(&mut out, b"IDAT", &zlib_store(&raw));
    push_chunk(&mut out, b"IEND", &[]);
    std::fs::write(path, &out)
}
