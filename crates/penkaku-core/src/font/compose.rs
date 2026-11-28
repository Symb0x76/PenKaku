//! Maths symbols a font lacks, made from its own strokes so a formula writes in one hand instead
//! of a missing box or another font's glyph: `≤` is its `<` over its `-`, `×` its `+` turned, `÷`
//! its `-` between two of its `.`, and so on. Only `√` and the ring of `①` are drawn from scratch
//! (sized by the font's digits); 抖动 and 变形 later make them look written like the rest. A font
//! with only one half of a bracket pair (`{` but no `}`) gets the other half as its mirror image.

use std::f32::consts::{FRAC_PI_2, TAU};

use super::{Font, Glyph, Point, bounds};
use crate::layout::glyph_for;

type Strokes = Vec<Vec<Point>>;

/// The symbols [`Font::compose_math`] can make, besides the circled numbers and bracket halves.
const SYMBOLS: [char; 19] = ['−', '±', '∓', '×', '÷', '≤', '≥', '≠', '≡', '≈', '/', '∥', '°', '·', '⋅', '√', '∕', 'Θ', 'θ'];

/// Paired brackets: a font with only one half gets the other as its mirror image (`{` → `}`), or
/// turned half round for corner brackets (`「` → `」`).
const PAIRS: [(char, char, bool); 13] = [
    ('(', ')', false),
    ('[', ']', false),
    ('{', '}', false),
    ('<', '>', false),
    ('（', '）', false),
    ('［', '］', false),
    ('｛', '｝', false),
    ('【', '】', false),
    ('《', '》', false),
    ('〈', '〉', false),
    ('〔', '〕', false),
    ('「', '」', true),
    ('『', '』', true),
];

impl Font {
    /// Symbols the font lacks, made from its own strokes (see the module docs): the missing half
    /// of a bracket pair, then `− ± ∓ × ÷ ≤ ≥ ≠ ≡ ≈ / ∥ ° · √ Θ θ` and the circled numbers
    /// `⓪ ①..㊿`. Symbols the font has (directly or as their full/half-width counterpart), or
    /// whose parts it lacks, are left alone. Returns the symbols it made.
    pub fn compose_math(&mut self) -> Vec<char> {
        // Brackets first, so a mirrored `>` can go on into `≥`.
        let mut chars = self.compose_with(PAIRS.into_iter().flat_map(|(a, b, _)| [a, b]), other_half);
        chars.extend(self.compose_with(SYMBOLS.into_iter().chain((0..=50).filter_map(circled_char)), make));
        chars
    }

    /// Each of `targets` the font lacks, made by `make`; returns those made.
    fn compose_with(&mut self, targets: impl Iterator<Item = char>, make: fn(&Font, char) -> Option<Strokes>) -> Vec<char> {
        let has = |c: char| glyph_for(self, c).is_some_and(|g| !g.strokes.is_empty());
        let made: Vec<(char, Glyph)> = targets.filter(|&c| !has(c)).filter_map(|c| Some((c, finish(make(self, c)?)?))).collect();
        let chars = made.iter().map(|(c, _)| *c).collect();
        self.glyphs.extend(made);
        chars
    }
}

/// The missing half `c` of a bracket pair from the half `f` has: mirrored left to right, or for
/// corner brackets turned half round with each stroke reversed so it is still written from the
/// top. It keeps its height on the line.
fn other_half(f: &Font, c: char) -> Option<Strokes> {
    let &(a, b, turn) = PAIRS.iter().find(|p| p.0 == c || p.1 == c)?;
    let src = glyph_for(f, if c == a { b } else { a }).map(|g| g.strokes.clone()).filter(|s| bounds(s).is_some())?;
    let (mx, my) = mid(&src);
    Some(if turn {
        map(&src, |(x, y)| (2.0 * mx - x, 2.0 * my - y)).into_iter().map(|mut st| {
            st.reverse();
            st
        }).collect()
    } else {
        map(&src, |(x, y)| (2.0 * mx - x, y))
    })
}

/// The strokes of `c` built from `f`'s glyphs, or `None` when `f` lacks a part.
fn make(f: &Font, c: char) -> Option<Strokes> {
    let part = |c: char| glyph_for(f, c).map(|g| g.strokes.clone()).filter(|s| bounds(s).is_some());
    // The maths axis: where `+` is centred (a font's `-` is often a low hyphen).
    let axis = || part('+').or_else(|| part('=')).map(|p| mid(&p).1);
    // Top and foot of the font's digits.
    let digits = || part('0').or_else(|| part('1')).and_then(|p| bounds(&p)).map(|b| (b.1, b.3));
    match c {
        '−' => {
            let plus = part('+')?;
            Some(at(&fit_width(&part('-')?, size(&plus).0), 0.0, mid(&plus).1))
        }
        '±' | '∓' => {
            let plus = scaled(&part('+')?, 0.85);
            let (w, h) = size(&plus);
            let dash = fit_width(&part('-')?, w);
            let parts = if c == '±' { [plus, dash] } else { [dash, plus] };
            Some(stack(&parts, 0.15 * h, axis()?))
        }
        '×' => {
            let plus = part('+')?;
            let m = mid(&plus);
            Some(at(&scaled(&rotated(&plus, 45.0, m), 0.8), 0.0, m.1))
        }
        '÷' => {
            let (plus, dot) = (part('+')?, part('.')?);
            let (w, _) = size(&plus);
            let (y, k) = (mid(&plus).1, 0.3 * w);
            let mut s = at(&dot, 0.0, y - k);
            s.extend(at(&fit_width(&part('-')?, w), 0.0, y));
            s.extend(at(&dot, 0.0, y + k));
            Some(s)
        }
        '≤' | '≥' => {
            let head = scaled(&part(if c == '≤' { '<' } else { '>' })?, 0.85);
            let (w, h) = size(&head);
            let bar = fit_width(&part('-')?, w);
            Some(stack(&[head, bar], 0.2 * h, axis()?))
        }
        '≠' => {
            let mut eq = part('=')?;
            let ((x, y), (w, h)) = (mid(&eq), size(&eq));
            // A slash through both bars, top to the right, drawn downwards.
            let half = h.max(0.35 * w);
            eq.push(vec![(x + 0.22 * w, y - half), (x - 0.22 * w, y + half)]);
            Some(eq)
        }
        '≡' => {
            let eq = part('=')?;
            let (w, h) = size(&eq);
            let y = mid(&eq).1;
            Some(stack(&[eq, fit_width(&part('-')?, w)], 0.7 * h, y))
        }
        '≈' => {
            let wave = part('~')?;
            let h = size(&wave).1;
            // Waves as far apart as the `=` bars, but never touching.
            let gap = part('=').map_or(0.0, |e| size(&e).1 - h).max(0.4 * h);
            let y = axis().unwrap_or(mid(&wave).1);
            Some(stack(&[wave.clone(), wave], gap, y))
        }
        '/' | '∕' => match part('|') {
            // The font's `|` leant right.
            Some(bar) => Some(rotated(&bar, 20.0, mid(&bar))),
            None => {
                let (top, foot) = digits()?;
                Some(vec![vec![(0.45 * (foot - top), top), (0.0, foot)]])
            }
        },
        '∥' => {
            let bar = part('|')?;
            let (w, h) = size(&bar);
            let mut s = bar.clone();
            s.extend(moved(&bar, w + 0.12 * h, 0.0));
            Some(s)
        }
        '°' => {
            let ring = part('o').or_else(|| part('0'))?;
            let (top, foot) = digits().unwrap_or_else(|| bounds(&ring).map(|b| (b.1, b.3)).expect("parts have points"));
            let small = scaled(&ring, 0.3 * (foot - top) / size(&ring).1);
            let b = bounds(&small)?;
            Some(moved(&small, 0.0, top - b.1))
        }
        '·' | '⋅' => Some(at(&part('.')?, 0.0, axis()?)),
        'Θ' | 'θ' => {
            // `O` (or `0` for θ) with a short bar across its middle.
            let ring = part(if c == 'Θ' { 'O' } else { '0' })?;
            let (w, _) = size(&ring);
            let (x, y) = mid(&ring);
            let mut s = ring;
            s.extend(at(&fit_width(&part('-')?, 0.55 * w), x, y));
            Some(s)
        }
        '√' => {
            // One stroke: a short rise, down to the foot, up to the digits' top, then a short bar.
            let (top, foot) = digits()?;
            let h = foot - top;
            Some(vec![vec![
                (0.0, foot - 0.42 * h),
                (0.14 * h, foot - 0.52 * h),
                (0.34 * h, foot),
                (0.62 * h, top),
                (1.05 * h, top),
            ]])
        }
        _ => {
            let n = circled_value(c)?;
            let (top, foot) = digits()?;
            let h = foot - top;
            // The number in the font's digits, side by side.
            let mut num = Vec::new();
            let mut x = 0.0;
            for d in n.to_string().chars() {
                let g = part(d)?;
                let b = bounds(&g)?;
                num.extend(moved(&g, x - b.0, 0.0));
                x += b.2 - b.0 + 0.12 * h;
            }
            // A ring a little taller than a digit, the number shrunk into it.
            let d = 1.2 * h;
            let (cx, cy) = (d / 2.0, (top + foot) / 2.0);
            let (w, nh) = size(&num);
            let k = (0.55 * d / nh).min(0.62 * d / w);
            let mut s = at(&scaled(&num, k), cx, cy);
            s.push(ring(cx, cy, d / 2.0));
            Some(s)
        }
    }
}

/// `⓪ ① .. ㊿` for 0..=50.
fn circled_char(n: u32) -> Option<char> {
    char::from_u32(match n {
        0 => 0x24EA,
        1..=20 => 0x2460 + n - 1,
        21..=35 => 0x3251 + n - 21,
        36..=50 => 0x32B1 + n - 36,
        _ => return None,
    })
}

fn circled_value(c: char) -> Option<u32> {
    match c as u32 {
        0x24EA => Some(0),
        c @ 0x2460..=0x2473 => Some(c - 0x2460 + 1),
        c @ 0x3251..=0x325F => Some(c - 0x3251 + 21),
        c @ 0x32B1..=0x32BF => Some(c - 0x32B1 + 36),
        _ => None,
    }
}

/// A hand-drawn ring around `(cx, cy)`: from just right of the top, anticlockwise as a hand draws
/// it, a little past where it began, slightly squashed and uneven.
fn ring(cx: f32, cy: f32, r: f32) -> Vec<Point> {
    const N: usize = 40;
    let (start, sweep) = (-FRAC_PI_2 + 0.2, TAU + 0.35);
    (0..=N)
        .map(|i| {
            let t = start - sweep * i as f32 / N as f32;
            let r = r * (1.0 + 0.03 * (2.0 * t).sin());
            (cx + r * t.cos(), cy + 0.94 * r * t.sin())
        })
        .collect()
}

/// A glyph of `s` starting at x = 0 with its width as advance, like the fonts' own glyphs.
fn finish(s: Strokes) -> Option<Glyph> {
    let (x0, _, x1, _) = bounds(&s)?;
    Some(Glyph { advance: x1 - x0, strokes: moved(&s, -x0, 0.0) })
}

/// Parts one under another, `gap` apart, centred on one vertical line, the whole centred on `y`.
fn stack(parts: &[Strokes], gap: f32, y: f32) -> Strokes {
    let mut out = Vec::new();
    let mut top = 0.0;
    for p in parts {
        let Some((x0, y0, x1, y1)) = bounds(p) else { continue };
        out.extend(moved(p, -(x0 + x1) / 2.0, top - y0));
        top += y1 - y0 + gap;
    }
    at(&out, 0.0, y)
}

fn map(s: &[Vec<Point>], f: impl Fn(Point) -> Point) -> Strokes {
    s.iter().map(|st| st.iter().map(|&p| f(p)).collect()).collect()
}

fn moved(s: &[Vec<Point>], dx: f32, dy: f32) -> Strokes {
    map(s, |(x, y)| (x + dx, y + dy))
}

/// `s` moved so its box is centred on `(x, y)`.
fn at(s: &[Vec<Point>], x: f32, y: f32) -> Strokes {
    let (mx, my) = mid(s);
    moved(s, x - mx, y - my)
}

/// `s` scaled by `k` about its box's centre.
fn scaled(s: &[Vec<Point>], k: f32) -> Strokes {
    let (mx, my) = mid(s);
    map(s, |(x, y)| (mx + (x - mx) * k, my + (y - my) * k))
}

/// `s` stretched or squeezed sideways about its centre to width `w`.
fn fit_width(s: &[Vec<Point>], w: f32) -> Strokes {
    let (mx, _) = mid(s);
    let k = match size(s).0 {
        sw if sw > 0.0 => w / sw,
        _ => 1.0,
    };
    map(s, |(x, y)| (mx + (x - mx) * k, y))
}

/// `s` turned `deg` degrees about `c`, clockwise on the page (y is down).
fn rotated(s: &[Vec<Point>], deg: f32, (cx, cy): Point) -> Strokes {
    let (sin, cos) = deg.to_radians().sin_cos();
    map(s, |(x, y)| (cx + (x - cx) * cos - (y - cy) * sin, cy + (x - cx) * sin + (y - cy) * cos))
}

fn mid(s: &[Vec<Point>]) -> Point {
    bounds(s).map_or((0.0, 0.0), |(x0, y0, x1, y1)| ((x0 + x1) / 2.0, (y0 + y1) / 2.0))
}

fn size(s: &[Vec<Point>]) -> (f32, f32) {
    bounds(s).map_or((0.0, 0.0), |(x0, y0, x1, y1)| (x1 - x0, y1 - y0))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// A tiny font in glyph units (y down, foot at 7), maths axis at y = 5.
    fn font() -> Font {
        let g = |strokes: Strokes| finish(strokes).unwrap();
        let mut glyphs = HashMap::from([
            ('+', g(vec![vec![(0.0, 5.0), (2.0, 5.0)], vec![(1.0, 4.0), (1.0, 6.0)]])),
            ('-', g(vec![vec![(0.0, 6.2), (1.0, 6.2)]])),
            ('=', g(vec![vec![(0.0, 4.6), (2.0, 4.6)], vec![(0.0, 5.4), (2.0, 5.4)]])),
            ('<', g(vec![vec![(2.0, 4.0), (0.0, 5.0), (2.0, 6.0)]])),
            ('>', g(vec![vec![(0.0, 4.0), (2.0, 5.0), (0.0, 6.0)]])),
            ('.', g(vec![vec![(0.0, 7.0), (0.1, 7.0)]])),
            ('|', g(vec![vec![(0.0, 1.0), (0.0, 8.0)]])),
            ('o', g(vec![vec![(0.0, 5.0), (1.0, 5.0), (1.0, 7.0), (0.0, 7.0), (0.0, 5.0)]])),
        ]);
        for d in '0'..='9' {
            glyphs.insert(d, g(vec![vec![(0.0, 2.5), (1.5, 2.5), (1.5, 7.0), (0.0, 7.0)]]));
        }
        Font { glyphs }
    }

    fn b(f: &Font, c: char) -> (f32, f32, f32, f32) {
        bounds(&f.get(c).unwrap().strokes).unwrap()
    }

    fn near(a: f32, e: f32) -> bool {
        (a - e).abs() < 1e-3
    }

    #[test]
    fn makes_what_the_font_lacks_from_its_own_parts() {
        let mut f = font();
        let made = f.compose_math();
        // No `~`, so no `≈`; no `O`, so no `Θ`; no bracket halves at all except `<` `>`, which it
        // has both of. Everything else and all 51 circled numbers.
        assert!(!made.contains(&'≈') && f.get('≈').is_none() && f.get('Θ').is_none());
        assert_eq!(made.len(), SYMBOLS.len() - 2 + 51);
        // Run again: nothing is missing any more.
        assert!(f.compose_math().is_empty());
        // Every glyph starts at x = 0 and advances by its width.
        for c in made {
            let (x0, _, x1, _) = b(&f, c);
            assert!(near(x0, 0.0) && near(f.get(c).unwrap().advance, x1), "{c}");
        }
    }

    #[test]
    fn missing_bracket_half_is_the_mirror_of_the_other() {
        let mut f = font();
        f.glyphs.remove(&'>');
        f.glyphs.insert('{', Glyph { advance: 1.0, strokes: vec![vec![(1.0, 1.0), (0.4, 2.0), (0.0, 4.0), (0.4, 6.0), (1.0, 7.0)]] });
        f.glyphs.insert('「', Glyph { advance: 2.0, strokes: vec![vec![(2.0, 1.0), (0.0, 1.0), (0.0, 4.0)]] });
        let made = f.compose_math();
        assert!(made.contains(&'}') && made.contains(&'>') && made.contains(&'」'));
        // Flipped left to right, same height, still drawn from the top.
        assert_eq!(f.get('}').unwrap().strokes, vec![vec![(0.0, 1.0), (0.6, 2.0), (1.0, 4.0), (0.6, 6.0), (0.0, 7.0)]]);
        // The mirrored `>` goes on into `≥` in the same call.
        assert!(made.contains(&'≥'));
        // Corner bracket turned half round, its stroke reversed: down first, then along the foot.
        assert_eq!(f.get('」').unwrap().strokes, vec![vec![(2.0, 1.0), (2.0, 4.0), (0.0, 4.0)]]);
    }

    #[test]
    fn theta_is_its_ring_with_a_bar_across() {
        let mut f = font();
        f.glyphs.insert('O', Glyph { advance: 2.0, strokes: vec![vec![(1.0, 2.0), (0.0, 4.5), (1.0, 7.0), (2.0, 4.5), (1.0, 2.0)]] });
        f.compose_math();
        for (c, ring) in [('Θ', 'O'), ('θ', '0')] {
            let s = &f.get(c).unwrap().strokes;
            assert_eq!(s.len(), 2, "{c}");
            assert_eq!(s[0], f.get(ring).unwrap().strokes[0], "{c} keeps its ring");
            let bar = bounds(std::slice::from_ref(&s[1])).unwrap();
            let rb = bounds(&s[..1]).unwrap();
            assert!(near((bar.1 + bar.3) / 2.0, (rb.1 + rb.3) / 2.0) && near(bar.2 - bar.0, 0.55 * (rb.2 - rb.0)), "{c}");
        }
    }

    #[test]
    fn leaves_symbols_the_font_has() {
        let mut f = font();
        f.glyphs.insert('≤', Glyph { advance: 9.0, strokes: vec![vec![(0.0, 0.0), (1.0, 1.0)]] });
        assert!(!f.compose_math().contains(&'≤'));
        assert_eq!(f.get('≤').unwrap().advance, 9.0);
    }

    #[test]
    fn operators_sit_on_the_maths_axis() {
        let mut f = font();
        f.compose_math();
        for c in ['−', '±', '∓', '×', '÷', '≤', '≥', '·'] {
            let (_, y0, _, y1) = b(&f, c);
            assert!(near((y0 + y1) / 2.0, 5.0), "{c} centred at {}", (y0 + y1) / 2.0);
        }
        // `−` is the `-` raised to the axis and as wide as `+`, not the low hyphen.
        assert_eq!(f.get('−').unwrap().strokes, vec![vec![(0.0, 5.0), (2.0, 5.0)]]);
        // `≤`: the `<` above, its bar below it.
        let le = &f.get('≤').unwrap().strokes;
        assert_eq!(le.len(), 2);
        assert!(le[1][0].1 > le[0].iter().map(|p| p.1).fold(f32::MIN, f32::max));
        // `≠`: both bars and one slash leaning right.
        let ne = &f.get('≠').unwrap().strokes;
        assert_eq!(ne.len(), 3);
        assert!(ne[2][0].0 > ne[2][1].0 && ne[2][0].1 < ne[2][1].1);
    }

    #[test]
    fn slash_is_the_bar_leant_right() {
        let mut f = font();
        f.compose_math();
        let s = &f.get('/').unwrap().strokes[0];
        // Top (smaller y) is to the right of the foot.
        assert!(s[0].1 < s[1].1 && s[0].0 > s[1].0);
        // Without a `|`, a plain line as tall as the digits.
        let mut f = font();
        f.glyphs.remove(&'|');
        f.compose_math();
        let (_, y0, _, y1) = b(&f, '/');
        assert!(near(y0, 2.5) && near(y1, 7.0));
        assert!(f.get('∥').is_none());
    }

    #[test]
    fn root_is_one_stroke_from_the_digits_foot_to_their_top() {
        let mut f = font();
        f.compose_math();
        let r = &f.get('√').unwrap().strokes;
        assert_eq!(r.len(), 1);
        let (_, y0, _, y1) = b(&f, '√');
        assert!(near(y0, 2.5) && near(y1, 7.0));
    }

    #[test]
    fn circled_numbers_put_the_digits_inside_a_ring() {
        assert_eq!((circled_char(0), circled_char(1), circled_char(20), circled_char(21), circled_char(50)), (Some('⓪'), Some('①'), Some('⑳'), Some('㉑'), Some('㊿')));
        assert!((0..=50).all(|n| circled_value(circled_char(n).unwrap()) == Some(n)));
        let mut f = font();
        f.compose_math();
        let s = &f.get('⑫').unwrap().strokes;
        // Two digits, then the ring.
        assert_eq!(s.len(), 3);
        let ring = s.last().unwrap();
        let rb = bounds(std::slice::from_ref(ring)).unwrap();
        let nb = bounds(&s[..2]).unwrap();
        assert!(nb.0 > rb.0 && nb.2 < rb.2 && nb.1 > rb.1 && nb.3 < rb.3);
        // The ring is centred on the digits' middle and closes on itself (with a little overlap).
        assert!(((rb.1 + rb.3) / 2.0 - 4.75).abs() < 0.2);
        let (a, z) = (ring[0], *ring.last().unwrap());
        assert!(((a.0 - z.0).powi(2) + (a.1 - z.1).powi(2)).sqrt() < 0.3 * (rb.2 - rb.0));
    }
}
