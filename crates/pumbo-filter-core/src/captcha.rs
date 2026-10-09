//! CAPTCHA images: a short code drawn with distorted bitmap glyphs, noise lines
//! and dots, rendered straight into map colour indices.

use std::sync::Arc;

use crate::font::{self, GLYPH_H, GLYPH_W};
use pumbo_common::map::{self as mapimg, Canvas, SIZE, color};
use pumbo_common::random::Rng;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captcha {
    /// Characters as drawn (upper case letters, digits).
    pub answer: String,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub length: u32,
    pub alphabet: Vec<char>,
    pub curves: u32,
    pub noise_dots: u32,
}

const BACKGROUNDS: &[u8] =
    &[color(mapimg::SAND, 2), color(mapimg::SNOW, 2), color(mapimg::QUARTZ, 2), color(mapimg::SNOW, 1)];
const INKS: &[u8] = &[
    color(mapimg::BLACK, 2),
    color(mapimg::BLUE, 1),
    color(mapimg::RED, 1),
    color(mapimg::GREEN, 0),
    color(mapimg::PURPLE, 1),
    color(mapimg::BROWN, 1),
    color(mapimg::GRAY, 1),
];
const SPECKLE: &[u8] = &[color(mapimg::LIGHT_GRAY, 2), color(mapimg::SAND, 1), color(mapimg::LIGHT_GRAY, 1)];

pub fn generate(seed: u64, style: &Style) -> Captcha {
    let mut rng = Rng::new(seed);
    let len = style.length.clamp(1, 6) as usize;
    let alphabet: Vec<char> = style.alphabet.iter().copied().filter(|c| font::glyph(*c).is_some()).collect();
    let alphabet = if alphabet.is_empty() { vec!['a', 'c', 'e'] } else { alphabet };
    let answer: String = (0..len).map(|_| rng.pick(&alphabet).copied().unwrap_or('a').to_ascii_uppercase()).collect();

    let bg = rng.pick(BACKGROUNDS).copied().unwrap_or(color(mapimg::SNOW, 2));
    let mut canvas = Canvas::new(bg);
    // light speckle so the background is not a flat colour
    for _ in 0..900 {
        let x = rng.below(SIZE as u32) as i32;
        let y = rng.below(SIZE as u32) as i32;
        canvas.set(x, y, rng.pick(SPECKLE).copied().unwrap_or(bg));
    }

    // Glyph scale so the whole code fits with margins.
    let cell_w = (118.0 / len as f32).min(40.0);
    let scale = (cell_w / (GLYPH_W as f32 + 1.2)).min(7.0);
    let total_w = cell_w * len as f32;
    let start_x = (SIZE as f32 - total_w) / 2.0;
    let ink = rng.pick(INKS).copied().unwrap_or(color(mapimg::BLACK, 2));
    let ripple_amp = rng.range(1.0, 2.6);
    let ripple_freq = rng.range(0.08, 0.16);
    let ripple_phase = rng.range(0.0, std::f32::consts::TAU);

    for (i, ch) in answer.chars().enumerate() {
        let Some(g) = font::glyph(ch) else { continue };
        let colour = if rng.below(3) == 0 { rng.pick(INKS).copied().unwrap_or(ink) } else { ink };
        let angle = rng.range(-0.28, 0.28);
        let cx = start_x + cell_w * (i as f32 + 0.5) + rng.range(-2.0, 2.0);
        let cy = SIZE as f32 / 2.0 + rng.range(-9.0, 9.0);
        let sx = scale * rng.range(0.9, 1.05);
        let sy = scale * rng.range(1.0, 1.2);
        let radius = rng.range(0.42, 0.55);
        let (sin, cos) = angle.sin_cos();
        let half_w = GLYPH_W as f32 / 2.0;
        let half_h = GLYPH_H as f32 / 2.0;
        let reach = (half_w.max(half_h) * sx.max(sy) * 1.5) as i32 + 2;
        for py in (cy as i32 - reach)..(cy as i32 + reach) {
            for px in (cx as i32 - reach)..(cx as i32 + reach) {
                let wobble = ripple_amp * ((py as f32) * ripple_freq + ripple_phase).sin();
                let dx = px as f32 - cx - wobble;
                let dy = py as f32 - cy;
                // inverse rotation into glyph space
                let u = (dx * cos + dy * sin) / sx + half_w;
                let v = (-dx * sin + dy * cos) / sy + half_h;
                if font::stroke(g, u, v, radius) {
                    canvas.set(px, py, colour);
                }
            }
        }
    }

    // noise curves crossing the text, in ink colours so they cannot be filtered out by colour
    for _ in 0..style.curves {
        let c = if rng.below(2) == 0 { ink } else { rng.pick(INKS).copied().unwrap_or(ink) };
        let amp = rng.range(6.0, 22.0);
        let freq = rng.range(0.02, 0.07);
        let phase = rng.range(0.0, std::f32::consts::TAU);
        let base = rng.range(35.0, 93.0);
        let tilt = rng.range(-0.25, 0.25);
        let thick = 1 + rng.below(2) as i32;
        for x in 0..SIZE as i32 {
            let y = base + tilt * (x as f32 - 64.0) + amp * (x as f32 * freq + phase).sin();
            for t in 0..thick {
                canvas.set(x, y as i32 + t, c);
            }
        }
    }
    for _ in 0..style.noise_dots {
        let x = rng.below(SIZE as u32) as i32;
        let y = rng.below(SIZE as u32) as i32;
        let c = if rng.below(2) == 0 { bg } else { rng.pick(INKS).copied().unwrap_or(ink) };
        canvas.set(x, y, c);
        if rng.below(3) == 0 {
            canvas.set(x + 1, y, c);
        }
    }
    Captcha { answer, pixels: canvas.px }
}

/// Compares a player's answer.
pub fn matches(answer: &str, input: &str, ignore_case: bool) -> bool {
    let input: String = input.trim().chars().filter(|c| !c.is_whitespace()).collect();
    if ignore_case { answer.eq_ignore_ascii_case(&input) } else { answer == input }
}

/// Pool of pre-generated images, refreshed gradually.
pub struct Pool {
    items: Vec<Arc<Captcha>>,
    target: usize,
    cursor: usize,
    seed: u64,
    counter: u64,
    style: Style,
    refreshing: bool,
}

impl Pool {
    pub fn new(target: usize, style: Style, seed: u64) -> Self {
        Self {
            items: Vec::with_capacity(target),
            target: target.max(1),
            cursor: 0,
            seed,
            counter: 0,
            style,
            refreshing: false,
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn set_style(&mut self, target: usize, style: Style) {
        self.target = target.max(1);
        self.style = style;
        self.items.truncate(self.target);
        self.cursor = 0;
    }

    fn next_seed(&mut self) -> u64 {
        self.counter = self.counter.wrapping_add(1);
        self.seed ^ self.counter.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Starts replacing every image, a few per call to [`Pool::work`].
    pub fn start_refresh(&mut self) {
        self.refreshing = true;
        self.cursor = 0;
    }

    pub fn is_refreshing(&self) -> bool {
        self.refreshing
    }

    /// Generates up to `budget` images: first fills the pool, then continues a
    /// refresh if one is running. Returns how many were generated.
    pub fn work(&mut self, budget: usize) -> usize {
        let mut done = 0;
        while done < budget {
            if self.items.len() < self.target {
                let seed = self.next_seed();
                self.items.push(Arc::new(generate(seed, &self.style)));
            } else if self.refreshing {
                let seed = self.next_seed();
                let fresh = Arc::new(generate(seed, &self.style));
                if let Some(slot) = self.items.get_mut(self.cursor) {
                    *slot = fresh;
                }
                self.cursor += 1;
                if self.cursor >= self.items.len() {
                    self.cursor = 0;
                    self.refreshing = false;
                }
            } else {
                break;
            }
            done += 1;
        }
        done
    }

    pub fn pick(&self, random: u64) -> Option<Arc<Captcha>> {
        if self.items.is_empty() {
            return None;
        }
        let idx = (random % self.items.len() as u64) as usize;
        self.items.get(idx).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> Style {
        Style { length: 3, alphabet: "acdefhkmnprstuvwxy234678".chars().collect(), curves: 3, noise_dots: 220 }
    }

    #[test]
    fn deterministic_and_varied() {
        let a = generate(42, &style());
        let b = generate(42, &style());
        let c = generate(43, &style());
        assert_eq!(a, b);
        assert_ne!(a.pixels, c.pixels);
        assert_eq!(a.pixels.len(), mapimg::PIXELS);
        assert_eq!(a.answer.len(), 3);
        assert!(a.answer.chars().all(|ch| style().alphabet.contains(&ch.to_ascii_lowercase())));
    }

    #[test]
    fn code_is_drawn() {
        for seed in 0..50 {
            let cap = generate(seed, &Style { curves: 0, noise_dots: 0, ..style() });
            let ink = cap.pixels.iter().filter(|p| INKS.contains(p)).count();
            // three glyphs at this scale cover well over 1000 pixels
            assert!(ink > 1000, "seed {seed}: {ink}");
            // the code stays inside the image
            assert!(ink < mapimg::PIXELS / 2, "seed {seed}: {ink}");
        }
    }

    #[test]
    fn long_codes_fit() {
        let cap = generate(9, &Style { length: 6, ..style() });
        assert_eq!(cap.answer.len(), 6);
    }

    #[test]
    fn answers() {
        assert!(matches("AB3", "ab3", true));
        assert!(matches("AB3", " a b 3 ", true));
        assert!(!matches("AB3", "ab3", false));
        assert!(!matches("AB3", "ab", true));
    }

    #[test]
    fn pool_fills_and_refreshes() {
        let mut p = Pool::new(10, style(), 1);
        assert!(p.pick(0).is_none());
        assert_eq!(p.work(4), 4);
        assert_eq!(p.len(), 4);
        p.work(100);
        assert_eq!(p.len(), 10);
        assert_eq!(p.work(5), 0);
        let before = p.pick(3);
        p.start_refresh();
        let mut total = 0;
        while p.is_refreshing() {
            total += p.work(3);
        }
        assert_eq!(total, 10);
        assert_ne!(before, p.pick(3));
    }

    /// Writes a few samples as PNG for a visual check:
    /// `PUMBO_CAPTCHA_PNG=/tmp/dir cargo test captcha_preview -- --ignored`
    #[test]
    #[ignore]
    fn captcha_preview() {
        let Ok(dir) = std::env::var("PUMBO_CAPTCHA_PNG") else { return };
        for seed in 0..6u64 {
            let cap = generate(seed * 7919, &style());
            let path = format!("{dir}/captcha_{seed}_{}.png", cap.answer);
            std::fs::write(&path, png(&cap.pixels)).unwrap();
        }
    }

    /// Uncompressed PNG (stored deflate blocks), enough for previews.
    fn png(pixels: &[u8]) -> Vec<u8> {
        fn crc(data: &[u8]) -> u32 {
            let mut c = 0xFFFF_FFFFu32;
            for b in data {
                c ^= u32::from(*b);
                for _ in 0..8 {
                    c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                }
            }
            !c
        }
        fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend_from_slice(data);
            out.extend_from_slice(&body);
            out.extend_from_slice(&crc(&body).to_be_bytes());
        }
        let scale = 3usize;
        let w = SIZE * scale;
        let mut raw = Vec::new();
        for y in 0..w {
            raw.push(0u8);
            for x in 0..w {
                let (r, g, b) = mapimg::rgb(pixels[(y / scale) * SIZE + x / scale]);
                raw.extend_from_slice(&[r, g, b]);
            }
        }
        let mut z = vec![0x78, 0x01];
        for (i, block) in raw.chunks(65535).enumerate() {
            let last = (i + 1) * 65535 >= raw.len();
            z.push(u8::from(last));
            z.extend_from_slice(&(block.len() as u16).to_le_bytes());
            z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
            z.extend_from_slice(block);
        }
        let (mut a, mut b) = (1u32, 0u32);
        for byte in &raw {
            a = (a + u32::from(*byte)) % 65521;
            b = (b + a) % 65521;
        }
        z.extend_from_slice(&((b << 16) | a).to_be_bytes());
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&(w as u32).to_be_bytes());
        ihdr.extend_from_slice(&(w as u32).to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &ihdr);
        chunk(&mut out, b"IDAT", &z);
        chunk(&mut out, b"IEND", &[]);
        out
    }
}
