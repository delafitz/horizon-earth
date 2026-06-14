//! Rectilinear vector stroke font.
//!
//! Each glyph is a list of line segments on a 3x5 node grid (x in 0..=2, y in
//! 0..=4 with y=0 at the top). Segments are drawn as GPU lines in screen space,
//! matching the thin vector style of the coastlines. Mostly horizontal/vertical
//! with a few 45-degree strokes where a letter needs them.

/// Grid extent: nodes span 0..=GW in x and 0..=GH in y.
pub const GW: f32 = 2.0;
pub const GH: f32 = 4.0;

type Seg = [u8; 4]; // x0, y0, x1, y1

/// Line segments for a printable ASCII byte. Unknown glyphs fall back to a box.
pub fn strokes(ch: u8) -> &'static [Seg] {
    match ch {
        b' ' => &[],
        b'0' => &[[0, 0, 2, 0], [2, 0, 2, 4], [2, 4, 0, 4], [0, 4, 0, 0]],
        b'1' => &[[0, 1, 1, 0], [1, 0, 1, 4], [0, 4, 2, 4]],
        b'2' => &[[0, 0, 2, 0], [2, 0, 2, 2], [2, 2, 0, 2], [0, 2, 0, 4], [0, 4, 2, 4]],
        b'3' => &[[0, 0, 2, 0], [2, 0, 2, 4], [2, 4, 0, 4], [0, 2, 2, 2]],
        b'4' => &[[0, 0, 0, 2], [0, 2, 2, 2], [2, 0, 2, 4]],
        b'5' => &[[2, 0, 0, 0], [0, 0, 0, 2], [0, 2, 2, 2], [2, 2, 2, 4], [2, 4, 0, 4]],
        b'6' => &[[2, 0, 0, 0], [0, 0, 0, 4], [0, 4, 2, 4], [2, 4, 2, 2], [2, 2, 0, 2]],
        b'7' => &[[0, 0, 2, 0], [2, 0, 1, 4]],
        b'8' => &[[0, 0, 2, 0], [0, 0, 0, 4], [2, 0, 2, 4], [0, 4, 2, 4], [0, 2, 2, 2]],
        b'9' => &[[0, 2, 0, 0], [0, 0, 2, 0], [2, 0, 2, 4], [0, 2, 2, 2], [0, 4, 2, 4]],
        b'A' => &[[0, 4, 0, 0], [0, 0, 2, 0], [2, 0, 2, 4], [0, 2, 2, 2]],
        b'B' => &[
            [0, 0, 0, 4], [0, 0, 2, 0], [2, 0, 2, 2], [0, 2, 2, 2], [2, 2, 2, 4], [0, 4, 2, 4],
        ],
        b'C' => &[[2, 0, 0, 0], [0, 0, 0, 4], [0, 4, 2, 4]],
        b'D' => &[
            [0, 0, 0, 4], [0, 0, 1, 0], [1, 0, 2, 1], [2, 1, 2, 3], [2, 3, 1, 4], [1, 4, 0, 4],
        ],
        b'E' => &[[2, 0, 0, 0], [0, 0, 0, 4], [0, 2, 2, 2], [0, 4, 2, 4]],
        b'F' => &[[2, 0, 0, 0], [0, 0, 0, 4], [0, 2, 2, 2]],
        b'G' => &[[2, 0, 0, 0], [0, 0, 0, 4], [0, 4, 2, 4], [2, 4, 2, 2], [2, 2, 1, 2]],
        b'H' => &[[0, 0, 0, 4], [2, 0, 2, 4], [0, 2, 2, 2]],
        b'I' => &[[0, 0, 2, 0], [1, 0, 1, 4], [0, 4, 2, 4]],
        b'J' => &[[0, 0, 2, 0], [2, 0, 2, 4], [2, 4, 0, 4], [0, 4, 0, 3]],
        b'K' => &[[0, 0, 0, 4], [2, 0, 0, 2], [0, 2, 2, 4]],
        b'L' => &[[0, 0, 0, 4], [0, 4, 2, 4]],
        b'M' => &[[0, 4, 0, 0], [0, 0, 1, 2], [1, 2, 2, 0], [2, 0, 2, 4]],
        b'N' => &[[0, 4, 0, 0], [0, 0, 2, 4], [2, 4, 2, 0]],
        b'O' => &[[0, 0, 2, 0], [2, 0, 2, 4], [2, 4, 0, 4], [0, 4, 0, 0]],
        b'P' => &[[0, 0, 0, 4], [0, 0, 2, 0], [2, 0, 2, 2], [2, 2, 0, 2]],
        b'Q' => &[[0, 0, 2, 0], [2, 0, 2, 4], [2, 4, 0, 4], [0, 4, 0, 0], [1, 3, 2, 4]],
        b'R' => &[[0, 0, 0, 4], [0, 0, 2, 0], [2, 0, 2, 2], [2, 2, 0, 2], [0, 2, 2, 4]],
        b'S' => &[[2, 0, 0, 0], [0, 0, 0, 2], [0, 2, 2, 2], [2, 2, 2, 4], [2, 4, 0, 4]],
        b'T' => &[[0, 0, 2, 0], [1, 0, 1, 4]],
        b'U' => &[[0, 0, 0, 4], [0, 4, 2, 4], [2, 4, 2, 0]],
        b'V' => &[[0, 0, 1, 4], [1, 4, 2, 0]],
        b'W' => &[[0, 0, 0, 4], [0, 4, 1, 2], [1, 2, 2, 4], [2, 4, 2, 0]],
        b'X' => &[[0, 0, 2, 4], [2, 0, 0, 4]],
        b'Y' => &[[0, 0, 1, 2], [2, 0, 1, 2], [1, 2, 1, 4]],
        b'Z' => &[[0, 0, 2, 0], [2, 0, 0, 4], [0, 4, 2, 4]],
        b'(' => &[[2, 0, 1, 1], [1, 1, 1, 3], [1, 3, 2, 4]],
        b')' => &[[0, 0, 1, 1], [1, 1, 1, 3], [1, 3, 0, 4]],
        b'-' => &[[0, 2, 2, 2]],
        b'+' => &[[1, 1, 1, 3], [0, 2, 2, 2]],
        b'/' => &[[2, 0, 0, 4]],
        b'.' => &[[0, 4, 1, 4]],
        b':' => &[[1, 1, 1, 2], [1, 3, 1, 4]],
        // Fallback: a box, so an unsupported character is at least visible.
        _ => &[[0, 0, 2, 0], [2, 0, 2, 4], [2, 4, 0, 4], [0, 4, 0, 0]],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rasterise a glyph to an ASCII grid so letterforms can be eyeballed in the
    /// terminal (`cargo test --bin horizon -- --nocapture glyph_preview`).
    fn render(ch: u8) -> String {
        const SX: i32 = 3; // pixels per x grid unit
        const SY: i32 = 2; // pixels per y grid unit
        let w = (GW as i32 * SX) + 1;
        let h = (GH as i32 * SY) + 1;
        let mut grid = vec![vec![b' '; w as usize]; h as usize];
        for s in strokes(ch) {
            let (mut x0, mut y0) = (s[0] as i32 * SX, s[1] as i32 * SY);
            let (x1, y1) = (s[2] as i32 * SX, s[3] as i32 * SY);
            let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
            let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
            let mut err = dx + dy;
            loop {
                grid[y0 as usize][x0 as usize] = b'#';
                if x0 == x1 && y0 == y1 {
                    break;
                }
                let e2 = 2 * err;
                if e2 >= dy {
                    err += dy;
                    x0 += sx;
                }
                if e2 <= dx {
                    err += dx;
                    y0 += sy;
                }
            }
        }
        grid.iter()
            .map(|row| String::from_utf8_lossy(row).into_owned())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn glyph_preview() {
        let chars = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789()-+/.:";
        for &c in chars {
            println!("\n'{}'\n{}", c as char, render(c));
        }
    }
}
