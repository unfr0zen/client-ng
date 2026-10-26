//! The unit round the face, as nest-qemu's page draws it (`ui/index.html`):
//! a brushed steel ring holding a black lens, the wordmark over the
//! display and the sensor window's line under it, the cover glass catching
//! the light from the upper left, the status light over the wordmark, and
//! the shadow the unit casts down and to the right. The window is the
//! canvas: the unit with a margin for its shadow, clear elsewhere. The
//! frame rate sits under the sensor window's line.
//!
//! Lengths are the page's units (its viewBox is the unit, 340 across);
//! the canvas adds MARGIN round it.

use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Mask, MaskType, Paint, PathBuilder, Pixmap, Point, RadialGradient,
    Shader, SpreadMode, Stroke, Transform,
};

/// The unit across, and the canvas across with the margin for the shadow.
pub const UNIT: f32 = 340.0;
const MARGIN: f32 = 40.0;
pub const CANVAS: f32 = UNIT + 2.0 * MARGIN;

/// The unit's centre, the ring's outer edge, the lens's and the display's.
const CENTRE: f32 = UNIT / 2.0;
const RING: f32 = 166.0;
const LENS: f32 = 151.0;
pub const DISPLAY: f32 = 112.0;

/// The wordmark's place and scale, and its outline (M, L and Z only).
const WORDMARK: (f32, f32, f32, f32) = (153.8, 35.3, 0.01649, 0.01485);
const WORDMARK_PATH: &str = include_str!("wordmark.path");
/// The print's grey.
const PRINT: (u8, u8, u8) = (0x68, 0x69, 0x6c);

/// The status light over the wordmark: its centre and radius, unlit and
/// lit, and the glow round it lit.
const LED: (f32, f32) = (170.0, 28.0);
const LED_R: f32 = 1.2;
const LED_OFF: (u8, u8, u8) = (0x1b, 0x21, 0x1d);
const LED_ON: (u8, u8, u8) = (0x2f, 0xbf, 0x55);
const LED_GLOW: f32 = 1.5;

/// The frame rate's line: its middle's height and a glyph's dot.
const LABEL_Y: f32 = 309.0;
const LABEL_DOT: f32 = 1.4;

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, 255)
}

/// Where the face sits on the canvas: its left and top, and its side.
pub fn face() -> (f32, f32, f32) {
    (MARGIN + CENTRE - DISPLAY, MARGIN + CENTRE - DISPLAY, 2.0 * DISPLAY)
}

/// What a point of the canvas is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The lens: the unit's button.
    Button,
    /// The steel ring, by which the window is moved.
    Ring,
    /// Outside the unit.
    Outside,
}

/// What lies at canvas point (x, y).
pub fn part(x: f32, y: f32) -> Part {
    let (x, y) = (x - MARGIN, y - MARGIN);
    let d = (x - CENTRE).hypot(y - CENTRE);
    if d <= LENS {
        Part::Button
    } else if d <= RING {
        Part::Ring
    } else {
        Part::Outside
    }
}

fn circle(cx: f32, cy: f32, r: f32) -> tiny_skia::Path {
    PathBuilder::from_circle(cx, cy, r).expect("a circle of positive radius")
}

fn stops(list: &[(f32, Color)]) -> Vec<GradientStop> {
    list.iter().map(|&(at, c)| GradientStop::new(at, c)).collect()
}

fn paint(shader: Option<Shader<'static>>) -> Paint<'static> {
    Paint { shader: shader.expect("a gradient with stops"), anti_alias: true, ..Paint::default() }
}

fn solid(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

fn white(alpha: f32) -> Color {
    Color::from_rgba(1.0, 1.0, 1.0, alpha).unwrap()
}

fn black(alpha: f32) -> Color {
    Color::from_rgba(0.0, 0.0, 0.0, alpha).unwrap()
}

/// A point of a circle's bounding box, as SVG's objectBoundingBox units
/// place a gradient's ends.
fn in_box(r: f32, fx: f32, fy: f32) -> Point {
    Point::from_xy(CENTRE - r + 2.0 * r * fx, CENTRE - r + 2.0 * r * fy)
}

/// A shadow of the unit: a disc at (cx, cy) blurred by `sigma`, `alpha`
/// dark, over what `pixmap` holds.
fn shadow(pixmap: &mut Pixmap, k: f32, cx: f32, cy: f32, sigma: f32, alpha: f32) {
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    let (cx, cy, r) = ((cx + MARGIN) * k, (cy + MARGIN) * k, (RING - 1.0) * k);
    let mut a: Vec<f32> = (0..w * h).map(|i| {
        let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
        ((r - (x - cx).hypot(y - cy)) + 0.5).clamp(0.0, 1.0)
    }).collect();
    blur(&mut a, w, h, sigma * k);
    for (px, a) in pixmap.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(a) {
        // Black under what is there: premultiplied, only the alpha grows.
        let under = a * alpha * (1.0 - f32::from(px[3]) / 255.0);
        px[3] = (f32::from(px[3]) + under * 255.0).round().min(255.0) as u8;
    }
}

/// A gaussian blur of `sigma` pixels, as three box blurs each way.
fn blur(a: &mut [f32], w: usize, h: usize, sigma: f32) {
    let radius = (((12.0 * sigma * sigma / 3.0 + 1.0).sqrt() - 1.0) / 2.0).round().max(1.0) as usize;
    let mut line = Vec::new();
    for _ in 0..3 {
        for y in 0..h {
            box_blur(a, y * w, 1, w, radius, &mut line);
        }
        for x in 0..w {
            box_blur(a, x, w, h, radius, &mut line);
        }
    }
}

/// One box blur along a line of `n` values `step` apart from `start`, the
/// line's ends held beyond it.
fn box_blur(a: &mut [f32], start: usize, step: usize, n: usize, radius: usize, line: &mut Vec<f32>) {
    line.clear();
    line.extend((0..n).map(|i| a[start + i * step]));
    let at = |i: isize| line[i.clamp(0, n as isize - 1) as usize];
    let r = radius as isize;
    let mut sum: f32 = (-r..=r).map(at).sum();
    let width = (2 * radius + 1) as f32;
    for i in 0..n as isize {
        a[start + i as usize * step] = sum / width;
        sum += at(i + r + 1) - at(i - r);
    }
}

/// The wordmark's outline, in the unit's units.
fn wordmark() -> tiny_skia::Path {
    let (tx, ty, sx, sy) = WORDMARK;
    let mut pb = PathBuilder::new();
    let mut words = WORDMARK_PATH.split_whitespace();
    while let Some(word) = words.next() {
        let mut point = |pb: &mut PathBuilder, first: bool| {
            let x: f32 = words.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let y: f32 = words.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let (x, y) = (tx + x * sx, ty + y * sy);
            if first { pb.move_to(x, y) } else { pb.line_to(x, y) }
        };
        match word {
            "M" => point(&mut pb, true),
            "L" => point(&mut pb, false),
            "Z" => pb.close(),
            _ => {}
        }
    }
    pb.finish().expect("the wordmark's outline")
}

/// What lies under the face, `k` pixels a unit: the shadow, the ring, the
/// lens, the wordmark, the status light unlit and the sensor window's line.
pub fn chrome(k: f32) -> Pixmap {
    let side = (CANVAS * k).round() as u32;
    let mut pm = Pixmap::new(side, side).expect("a canvas of positive size");
    let t = Transform::from_scale(k, k).pre_translate(MARGIN, MARGIN);
    let ring = circle(CENTRE, CENTRE, RING);
    let lens = circle(CENTRE, CENTRE, LENS);

    // Brushed stainless, lighter up-left, and its rolled profile.
    let steel = LinearGradient::new(in_box(RING, 0.15, 0.0), in_box(RING, 0.85, 1.0),
        stops(&[(0.0, rgb(0xcf, 0xd1, 0xd4)), (0.4, rgb(0xb4, 0xb7, 0xbb)), (0.65, rgb(0x9a, 0x9d, 0xa1)), (1.0, rgb(0x85, 0x88, 0x8c))]),
        SpreadMode::Pad, Transform::identity());
    pm.fill_path(&ring, &paint(steel), FillRule::Winding, t, None);
    let c = Point::from_xy(CENTRE, CENTRE);
    let roll = RadialGradient::new(c, c, RING,
        stops(&[(0.91, black(0.6)), (0.918, black(0.0)), (0.93, white(0.38)), (0.948, white(0.0)), (0.975, black(0.12)), (1.0, black(0.38))]),
        SpreadMode::Pad, Transform::identity());
    pm.fill_path(&ring, &paint(roll), FillRule::Winding, t, None);
    pm.stroke_path(&circle(CENTRE, CENTRE, 165.6), &solid(rgb(0x4d, 0x50, 0x54)), &Stroke { width: 0.8, ..Stroke::default() }, t, None);

    // The black lens.
    let glass_centre = in_box(LENS, 0.5, 0.45);
    let glass = RadialGradient::new(glass_centre, glass_centre, 0.6 * 2.0 * LENS,
        stops(&[(0.0, rgb(0x0b, 0x0b, 0x0c)), (0.85, rgb(0x10, 0x10, 0x12)), (1.0, rgb(0x1c, 0x1c, 0x1f))]),
        SpreadMode::Pad, Transform::identity());
    pm.fill_path(&lens, &paint(glass), FillRule::Winding, t, None);
    pm.stroke_path(&lens, &solid(rgb(0x05, 0x05, 0x05)), &Stroke { width: 1.5, ..Stroke::default() }, t, None);

    // The print on the lens.
    pm.fill_path(&wordmark(), &solid(rgb(PRINT.0, PRINT.1, PRINT.2)), FillRule::EvenOdd, t, None);
    pm.fill_path(&circle(LED.0, LED.1, LED_R), &solid(rgb(LED_OFF.0, LED_OFF.1, LED_OFF.2)), FillRule::Winding, t, None);
    let mut pb = PathBuilder::new();
    pb.move_to(82.0, 291.0);
    pb.quad_to(170.0, 303.0, 258.0, 291.0);
    pm.stroke_path(&pb.finish().unwrap(), &solid(rgb(0x2a, 0x2a, 0x2c)), &Stroke { width: 1.2, ..Stroke::default() }, t, None);

    // Lit from the upper left: a soft shadow cast down and to the right, a
    // tighter one where the ring meets the wall.
    shadow(&mut pm, k, 172.0, 173.0, 2.5, 0.35);
    shadow(&mut pm, k, 179.0, 181.0, 10.0, 0.38);
    pm
}

/// The cover glass over the face, `k` pixels a unit: a sheen fading toward
/// the middle, brightest along the rim at the upper left.
pub fn cover(k: f32) -> Pixmap {
    let side = (CANVAS * k).round() as u32;
    let mut pm = Pixmap::new(side, side).expect("a canvas of positive size");
    let t = Transform::from_scale(k, k).pre_translate(MARGIN, MARGIN);
    let lens = circle(CENTRE, CENTRE, LENS);
    let sheen = LinearGradient::new(in_box(LENS, 0.08, 0.05), in_box(LENS, 0.62, 0.6),
        stops(&[(0.0, white(0.2)), (0.45, white(0.07)), (1.0, white(0.0))]), SpreadMode::Pad, Transform::identity());
    pm.fill_path(&lens, &paint(sheen), FillRule::Winding, t, None);

    let mut fade = Pixmap::new(side, side).expect("a canvas of positive size");
    let to_dark = LinearGradient::new(in_box(LENS, 0.0, 0.0), in_box(LENS, 0.7, 0.7),
        stops(&[(0.0, rgb(255, 255, 255)), (0.6, rgb(0, 0, 0))]), SpreadMode::Pad, Transform::identity());
    fade.fill_path(&lens, &paint(to_dark), FillRule::Winding, t, None);
    let upper_left = Mask::from_pixmap(fade.as_ref(), MaskType::Luminance);
    let c = Point::from_xy(CENTRE, CENTRE);
    let rim = RadialGradient::new(c, c, LENS, stops(&[(0.86, white(0.0)), (0.97, white(0.16)), (1.0, white(0.0))]),
        SpreadMode::Pad, Transform::identity());
    pm.fill_path(&lens, &paint(rim), FillRule::Winding, t, Some(&upper_left));
    pm
}

/// The status light lit, `k` pixels a unit, and where it goes on the
/// canvas: green, glowing.
pub fn led(k: f32) -> (Pixmap, f32, f32) {
    let half = LED_R + 4.0 * LED_GLOW;
    let side = (2.0 * half * k).ceil() as u32;
    let mut pm = Pixmap::new(side, side).expect("a light of positive size");
    let (w, c) = (side as usize, half * k);
    let mut glow: Vec<f32> = (0..w * w).map(|i| {
        let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
        (LED_R * k - (x - c).hypot(y - c) + 0.5).clamp(0.0, 1.0)
    }).collect();
    blur(&mut glow, w, w, LED_GLOW * k);
    let (r, g, b) = LED_ON;
    for (px, a) in pm.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(glow) {
        let a = a * 0.6;
        px.copy_from_slice(&[r, g, b].map(|v| (f32::from(v) * a).round() as u8).into_iter().chain([(a * 255.0).round() as u8]).collect::<Vec<u8>>());
    }
    let dot = Transform::from_scale(k, k).pre_translate(half - LED.0, half - LED.1);
    pm.fill_path(&circle(LED.0, LED.1, LED_R), &solid(rgb(r, g, b)), FillRule::Winding, dot, None);
    (pm, MARGIN + LED.0 - half, MARGIN + LED.1 - half)
}

/// The frame rate's digits and letters, 3x5, a row a 3-bit number, the top
/// first.
fn glyph(c: char) -> [u8; 5] {
    match c {
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [7, 1, 7, 4, 7],
        '3' => [7, 1, 3, 1, 7],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 7, 1, 7],
        '6' => [7, 4, 7, 5, 7],
        '7' => [7, 1, 2, 2, 2],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 7],
        'F' => [7, 4, 6, 4, 4],
        'P' => [7, 5, 7, 4, 4],
        'S' => [7, 4, 7, 1, 7],
        _ => [0; 5],
    }
}

/// `text` in the print's grey, `k` pixels a unit, and where it goes on
/// the canvas: centred under the sensor window's line.
pub fn label(text: &str, k: f32) -> (Pixmap, f32, f32) {
    let dot = LABEL_DOT * k;
    let n = text.chars().count().max(1);
    let (w, h) = (((n * 4 - 1) as f32 * dot).ceil() as u32, (5.0 * dot).ceil() as u32);
    let mut pm = Pixmap::new(w.max(1), h.max(1)).expect("a label of positive size");
    let fill = solid(rgb(PRINT.0, PRINT.1, PRINT.2));
    for (i, c) in text.chars().enumerate() {
        for (row, bits) in glyph(c).into_iter().enumerate() {
            for col in (0..3).filter(|col| bits >> (2 - col) & 1 == 1) {
                let x = (i * 4 + col) as f32 * dot;
                if let Some(r) = tiny_skia::Rect::from_xywh(x, row as f32 * dot, dot, dot) {
                    pm.fill_rect(r, &fill, Transform::identity(), None);
                }
            }
        }
    }
    let left = MARGIN + CENTRE - w as f32 / k / 2.0;
    let top = MARGIN + LABEL_Y - 2.5 * LABEL_DOT;
    (pm, left, top)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_close_press_or_move_by_where_they_land() {
        let at = |x: f32, y: f32| part(x + MARGIN, y + MARGIN);
        assert_eq!(at(CENTRE, CENTRE), Part::Button);
        assert_eq!(at(CENTRE, CENTRE + 300.0 / 2.0), Part::Button, "the lens below the sensor window's line");
        assert_eq!(at(CENTRE + 160.0, CENTRE), Part::Ring);
        assert_eq!(at(0.0, 0.0), Part::Outside);
        assert_eq!(part(1.0, CANVAS - 1.0), Part::Outside);
    }

    #[test]
    fn the_canvas_is_clear_round_the_unit_and_shadowed_below_right() {
        let k = 1.0;
        let pm = chrome(k);
        let alpha = |x: f32, y: f32| pm.pixel((x * k) as u32, (y * k) as u32).unwrap().alpha();
        assert_eq!(alpha(1.0, 1.0), 0, "the top left corner clear");
        let (c, r) = (MARGIN + CENTRE, RING);
        assert_eq!(alpha(c, c), 255, "the lens opaque");
        let off = (r + 6.0) / std::f32::consts::SQRT_2;
        let (below_right, above_left) = (alpha(c + off, c + off), alpha(c - off, c - off));
        assert!(below_right > above_left && below_right > 20, "the shadow falls down and right: {below_right} vs {above_left}");
    }

    #[test]
    fn the_frame_rate_sits_on_the_lens_below_the_display() {
        let (pm, left, top) = label("120 FPS", 2.0);
        let (w, h) = (pm.width() as f32 / 2.0, pm.height() as f32 / 2.0);
        let (fx, fy, fs) = face();
        for (x, y) in [(left, top), (left + w, top), (left, top + h), (left + w, top + h)] {
            assert_eq!(part(x, y), Part::Button, "({x}, {y}) on the lens");
            assert!((x - fx - fs / 2.0).hypot(y - fy - fs / 2.0) > DISPLAY, "({x}, {y}) off the display");
        }
        assert!((left + w / 2.0 - (MARGIN + CENTRE)).abs() < 0.5, "centred");
    }
}
