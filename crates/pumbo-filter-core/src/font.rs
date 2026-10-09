//! 5x7 bitmap glyphs used by the CAPTCHA renderer. Codes are drawn in upper case.

pub const GLYPH_W: usize = 5;
pub const GLYPH_H: usize = 7;

type Glyph = [&'static str; GLYPH_H];

/// Returns the glyph for a character (letters are case-insensitive).
pub fn glyph(c: char) -> Option<&'static Glyph> {
    let g: &'static Glyph = match c.to_ascii_lowercase() {
        'a' => &[".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
        'b' => &["####.", "#...#", "#...#", "####.", "#...#", "#...#", "####."],
        'c' => &[".###.", "#...#", "#....", "#....", "#....", "#...#", ".###."],
        'd' => &["####.", "#...#", "#...#", "#...#", "#...#", "#...#", "####."],
        'e' => &["#####", "#....", "#....", "####.", "#....", "#....", "#####"],
        'f' => &["#####", "#....", "#....", "####.", "#....", "#....", "#...."],
        'g' => &[".###.", "#...#", "#....", "#.###", "#...#", "#...#", ".####"],
        'h' => &["#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
        'i' => &[".###.", "..#..", "..#..", "..#..", "..#..", "..#..", ".###."],
        'j' => &["..###", "...#.", "...#.", "...#.", "...#.", "#..#.", ".##.."],
        'k' => &["#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#"],
        'l' => &["#....", "#....", "#....", "#....", "#....", "#....", "#####"],
        'm' => &["#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#"],
        'n' => &["#...#", "#...#", "##..#", "#.#.#", "#..##", "#...#", "#...#"],
        'o' => &[".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
        'p' => &["####.", "#...#", "#...#", "####.", "#....", "#....", "#...."],
        'q' => &[".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#"],
        'r' => &["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"],
        's' => &[".####", "#....", "#....", ".###.", "....#", "....#", "####."],
        't' => &["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."],
        'u' => &["#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
        'v' => &["#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#.."],
        'w' => &["#...#", "#...#", "#...#", "#.#.#", "#.#.#", "#.#.#", ".#.#."],
        'x' => &["#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#"],
        'y' => &["#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#.."],
        'z' => &["#####", "....#", "...#.", "..#..", ".#...", "#....", "#####"],
        '0' => &[".###.", "#...#", "#..##", "#.#.#", "##..#", "#...#", ".###."],
        '1' => &["..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###."],
        '2' => &[".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####"],
        '3' => &["#####", "...#.", "..#..", "...#.", "....#", "#...#", ".###."],
        '4' => &["...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#."],
        '5' => &["#####", "#....", "####.", "....#", "....#", "#...#", ".###."],
        '6' => &["..##.", ".#...", "#....", "####.", "#...#", "#...#", ".###."],
        '7' => &["#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#..."],
        '8' => &[".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###."],
        '9' => &[".###.", "#...#", "#...#", ".####", "....#", "...#.", ".##.."],
        _ => return None,
    };
    Some(g)
}

/// Whether the glyph has ink at cell `(x, y)`.
pub fn ink(g: &Glyph, x: i32, y: i32) -> bool {
    if x < 0 || y < 0 {
        return false;
    }
    g.get(y as usize).and_then(|row| row.as_bytes().get(x as usize)).is_some_and(|b| *b == b'#')
}

/// Whether glyph-space point `(u, v)` (cells are 1x1, centres at `x + 0.5`) lies on
/// a stroke. Strokes join the centres of neighbouring ink cells, including
/// diagonal ones, so slanted letters stay connected when scaled up.
pub fn stroke(g: &Glyph, u: f32, v: f32, radius: f32) -> bool {
    let cx = u.floor() as i32;
    let cy = v.floor() as i32;
    let r2 = radius * radius;
    for y in (cy - 1)..=(cy + 1) {
        for x in (cx - 1)..=(cx + 1) {
            if !ink(g, x, y) {
                continue;
            }
            let (ax, ay) = (x as f32 + 0.5, y as f32 + 0.5);
            if (u - ax).powi(2) + (v - ay).powi(2) <= r2 {
                return true;
            }
            for (dx, dy) in [(1, 0), (0, 1), (1, 1), (-1, 1)] {
                if !ink(g, x + dx, y + dy) {
                    continue;
                }
                // Skip diagonals that run along a straight neighbour pair (keeps corners crisp).
                if dx != 0 && dy != 0 && (ink(g, x + dx, y) || ink(g, x, y + dy)) {
                    continue;
                }
                let (bx, by) = (ax + dx as f32, ay + dy as f32);
                if seg_dist2(u, v, ax, ay, bx, by) <= r2 {
                    return true;
                }
            }
        }
    }
    false
}

fn seg_dist2(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let (qx, qy) = (ax + t * dx, ay + t * dy);
    (px - qx).powi(2) + (py - qy).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_glyphs_are_well_formed() {
        for c in "abcdefghijklmnopqrstuvwxyz0123456789".chars() {
            let g = glyph(c).unwrap_or_else(|| panic!("missing {c}"));
            assert!(g.iter().all(|r| r.len() == GLYPH_W), "bad row width for {c}");
            assert!(g.iter().any(|r| r.contains('#')));
        }
        assert!(glyph('!').is_none());
        assert_eq!(glyph('A'), glyph('a'));
    }

    #[test]
    fn glyphs_are_distinct() {
        let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyz0123456789".chars().collect();
        for (i, a) in chars.iter().enumerate() {
            for b in chars.iter().skip(i + 1) {
                assert_ne!(glyph(*a), glyph(*b), "{a} and {b} look the same");
            }
        }
    }
}
