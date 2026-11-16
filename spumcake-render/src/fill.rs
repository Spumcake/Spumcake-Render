//! Which pixels a polygon covers, by Pillow 12.1.1's own rule.
//!
//! The pre-vis builders fill each projected polygon with Pillow's `ImageDraw.polygon`, which truncates every
//! corner to a whole pixel and fills by its own scanline rule (`libImaging/Draw.c`, `polygon_generic`, on an
//! 8-bit image). A GPU's usual rule covers other pixels along every boundary, enough to miss the published
//! renders, so the engine covers exactly what Pillow covers. Each step below follows the C code, in its order
//! and its float precision.

/// A run of covered pixels on one row, from `x0` to `x1` inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub y: i32,
    pub x0: i32,
    pub x1: i32,
}

#[derive(Clone, Copy)]
struct Edge {
    xmin: i32,
    xmax: i32,
    ymin: i32,
    ymax: i32,
    x0: i32,
    y0: i32,
    dx: f32,
}

fn edge(x0: i32, y0: i32, x1: i32, y1: i32) -> Edge {
    Edge {
        xmin: x0.min(x1),
        xmax: x0.max(x1),
        ymin: y0.min(y1),
        ymax: y0.max(y1),
        x0,
        y0,
        dx: if y0 == y1 { 0.0 } else { (x1 - x0) as f32 / (y1 - y0) as f32 },
    }
}

/// Where the edge crosses row `y`, in the C code's float arithmetic.
fn cross(e: &Edge, y: i32) -> f32 {
    (y - e.y0) as f32 * e.dx + e.x0 as f32
}

/// `ROUND_UP` in Draw.c: half away from zero.
fn round_up(f: f32) -> i32 {
    if f >= 0.0 { (f + 0.5).floor() as i32 } else { -((f.abs() as f64 + 0.5).floor()) as i32 }
}

/// `ROUND_DOWN` in Draw.c: half toward zero.
fn round_down(f: f32) -> i32 {
    if f >= 0.0 { (f - 0.5).ceil() as i32 } else { -((f.abs() as f64 - 0.5).ceil()) as i32 }
}

/// Fills the polygon with corners `points`, in pixels, on an image `width` by `height`, and appends the
/// covered runs to `out`, row by row from the top, each row's runs merged and in order.
pub fn fill(points: &[[f64; 2]], width: i32, height: i32, out: &mut Vec<Span>) {
    if points.is_empty() || width <= 0 || height <= 0 {
        return;
    }
    // Python's floats reach Draw.c through a plain C cast, which truncates toward zero.
    let xy: Vec<(i32, i32)> = points.iter().map(|p| (p[0] as i32, p[1] as i32)).collect();
    let n = xy.len();

    let mut edges: Vec<Edge> = Vec::with_capacity(n);
    let mut i = 0;
    while i + 1 < n {
        let (x0, y0) = xy[i];
        let (x1, y1) = xy[i + 1];
        if y0 == y1 && i != 0 && y0 == xy[i - 1].1 {
            // A horizontal line straight after another one extends it, as Draw.c does.
            let last = edges.last_mut().unwrap();
            if x1 > x0 && x0 > xy[i - 1].0 {
                last.xmax = x1;
                i += 1;
                continue;
            } else if x1 < x0 && x0 < xy[i - 1].0 {
                last.xmin = x1;
                i += 1;
                continue;
            }
        }
        edges.push(edge(x0, y0, x1, y1));
        i += 1;
    }
    if xy[n - 1] != xy[0] {
        edges.push(edge(xy[n - 1].0, xy[n - 1].1, xy[0].0, xy[0].1));
    }

    // Every run as (row, first, last), sorted and merged at the end.
    let mut runs: Vec<(i32, i32, i32)> = Vec::new();
    let mut ymin = height - 1;
    let mut ymax = 0;
    let mut table: Vec<Edge> = Vec::with_capacity(edges.len());
    let mut flat: Vec<(i32, i32, i32)> = Vec::new();
    for e in &edges {
        ymin = ymin.min(e.ymin);
        ymax = ymax.max(e.ymax);
        if e.ymin == e.ymax {
            flat.push((e.ymin, e.xmin, e.xmax));
            continue;
        }
        table.push(*e);
    }
    let ymin = ymin.max(0);
    let ymax = ymax.min(height);
    let hline = |runs: &mut Vec<(i32, i32, i32)>, mut x0: i32, y: i32, mut x1: i32| {
        if y < 0 || y >= height {
            return;
        }
        if x0 < 0 {
            x0 = 0;
        } else if x0 >= width {
            return;
        }
        if x1 < 0 {
            return;
        } else if x1 >= width {
            x1 = width - 1;
        }
        if x0 <= x1 {
            runs.push((y, x0, x1));
        }
    };
    for &(y, x0, x1) in &flat {
        hline(&mut runs, x0, y, x1);
    }

    let mut xx: Vec<f32> = Vec::with_capacity(table.len() * 2);
    for y in ymin..=ymax {
        xx.clear();
        for (i, current) in table.iter().enumerate() {
            if y < current.ymin || y > current.ymax {
                continue;
            }
            xx.push(cross(current, y));
            if y == current.ymax && y < ymax {
                // "Needed to draw consistent polygons."
                let last = *xx.last().unwrap();
                xx.push(last);
            } else if (y == current.ymin || y == current.ymax) && current.dx != 0.0 {
                // "Connect discontiguous corners."
                for other in &table[..i] {
                    if (y != other.ymin && y != other.ymax) || other.dx == 0.0 {
                        continue;
                    }
                    let last = *xx.last().unwrap();
                    if last.round() == cross(other, y).round() {
                        let offset = if y == current.ymax { -1 } else { 1 };
                        let adjacent = cross(current, y + offset);
                        if y + offset >= other.ymin && y + offset <= other.ymax {
                            let adjacent_other = cross(other, y + offset);
                            if last > adjacent + 1.0 && last > adjacent_other + 1.0 {
                                *xx.last_mut().unwrap() = adjacent.max(adjacent_other).round() + 1.0;
                            } else if last < adjacent - 1.0 && last < adjacent_other - 1.0 {
                                *xx.last_mut().unwrap() = adjacent.min(adjacent_other).round() - 1.0;
                            }
                            break;
                        }
                    }
                }
            }
        }
        xx.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut k = 1;
        while k < xx.len() {
            hline(&mut runs, round_up(xx[k - 1]), y, round_down(xx[k]));
            k += 2;
        }
    }

    if runs.is_empty() {
        return;
    }
    runs.sort_unstable();
    let (mut y, mut x0, mut x1) = runs[0];
    for &(row, a, b) in &runs[1..] {
        if row == y && a <= x1 + 1 {
            x1 = x1.max(b);
        } else {
            out.push(Span { y, x0, x1 });
            (y, x0, x1) = (row, a, b);
        }
    }
    out.push(Span { y, x0, x1 });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Polygons filled by Pillow 12.1.1 itself, with the runs it covered, written by
    /// `tests/fixtures/make-pillow-fill.py`.
    #[test]
    fn covers_what_pillow_covers() {
        let text = include_str!("../tests/fixtures/pillow-fill.txt");
        let mut cases = 0;
        for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let parts: Vec<&str> = line.split('|').map(str::trim).collect();
            let size: Vec<i32> = parts[0].split_whitespace().map(|v| v.parse().unwrap()).collect();
            let coords: Vec<f64> = parts[1].split_whitespace().map(|v| v.parse().unwrap()).collect();
            let points: Vec<[f64; 2]> = coords.chunks(2).map(|c| [c[0], c[1]]).collect();
            let expected: Vec<Span> = parts[2]
                .split_whitespace()
                .map(|run| {
                    let (y, xs) = run.split_once(':').unwrap();
                    let (x0, x1) = xs.split_once('-').unwrap();
                    Span { y: y.parse().unwrap(), x0: x0.parse().unwrap(), x1: x1.parse().unwrap() }
                })
                .collect();
            let mut got = Vec::new();
            fill(&points, size[0], size[1], &mut got);
            assert_eq!(got, expected, "polygon {points:?}");
            cases += 1;
        }
        assert!(cases >= 1000, "only {cases} cases");
    }
}
