//! 查找 / 替换: matches in the text with Word's options 区分大小写 and 全角半角等同 (ASCII and
//! its fullwidth forms, the ideographic space and the space, count as equal). Matches never
//! overlap and are found left to right.

use std::ops::Range;

use crate::EditDoc;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FindOpts {
    /// 区分大小写.
    pub match_case: bool,
    /// 全角半角等同.
    pub fold_width: bool,
}

/// `c` as compared under `o`.
fn fold(c: char, o: FindOpts) -> char {
    let c = if o.fold_width {
        match c as u32 {
            0x3000 => ' ',
            u @ 0xFF01..=0xFF5E => char::from_u32(u - 0xFF01 + 0x21).unwrap_or(c),
            _ => c,
        }
    } else {
        c
    };
    if o.match_case {
        return c;
    }
    let mut low = c.to_lowercase();
    match (low.next(), low.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// Every match of `needle` in `hay`, left to right, never overlapping.
pub fn find_in(hay: &[char], needle: &str, o: FindOpts) -> Vec<Range<usize>> {
    let pat: Vec<char> = needle.chars().map(|c| fold(c, o)).collect();
    if pat.is_empty() || pat.len() > hay.len() {
        return Vec::new();
    }
    let text: Vec<char> = hay.iter().map(|&c| fold(c, o)).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + pat.len() <= text.len() {
        if text[i..i + pat.len()] == pat[..] {
            out.push(i..i + pat.len());
            i += pat.len();
        } else {
            i += 1;
        }
    }
    out
}

/// Chinese side of a 中英/中数 gap: Han ideographs and CJK / fullwidth punctuation.
fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3001..=0x303F | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF01..=0xFF0F | 0xFF1A..=0xFF20 | 0xFF3B..=0xFF40 | 0xFF5B..=0xFF65 | 0x20000..=0x3FFFF)
}

/// Spaces a 一键消除 removes: ASCII, no-break and ideographic spaces and tabs.
fn is_gap(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\u{00A0}' | '\u{3000}')
}

/// Every run of spaces inside `within` that sits between a Chinese char and an ASCII letter or
/// digit (either order, on one line), left to right. Spaces between two Latin words stay.
pub fn cjk_latin_spaces(hay: &[char], within: Range<usize>) -> Vec<Range<usize>> {
    let latin = |c: char| c.is_ascii_alphanumeric();
    gap_runs(hay, within, |l, r| (is_cjk(l) && latin(r)) || (latin(l) && is_cjk(r)))
}

/// Chinese / fullwidth punctuation, quotes, dashes and the ellipsis.
fn is_wide_punct(c: char) -> bool {
    matches!(c as u32, 0x3001..=0x303F | 0xFF01..=0xFF0F | 0xFF1A..=0xFF20 | 0xFF3B..=0xFF40 | 0xFF5B..=0xFF65 | 0x2010..=0x2027 | 0x00B7)
}

/// Every run of spaces inside `within` between punctuation and text (on one line). Around
/// Chinese punctuation every space goes. ASCII punctuation keeps English spacing: a space before
/// `, . ; : ! ? ) ] }` or after `( [ {` goes; the space after a closing mark (or before an
/// opening one) goes only when the text on the other side is Chinese.
pub fn punct_spaces(hay: &[char], within: Range<usize>) -> Vec<Range<usize>> {
    let closing = |c: char| matches!(c, ',' | '.' | ';' | ':' | '!' | '?' | ')' | ']' | '}');
    let opening = |c: char| matches!(c, '(' | '[' | '{');
    gap_runs(hay, within, |l, r| {
        is_wide_punct(l) || is_wide_punct(r) || closing(r) || opening(l) || (closing(l) && is_cjk(r)) || (is_cjk(l) && opening(r))
    })
}

/// Every run of spaces inside `within` (on one line) whose neighbours `hit(left, right)` accepts,
/// left to right.
fn gap_runs(hay: &[char], within: Range<usize>, hit: impl Fn(char, char) -> bool) -> Vec<Range<usize>> {
    let end = within.end.min(hay.len());
    let mut out = Vec::new();
    let mut i = within.start;
    while i < end {
        if !is_gap(hay[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < hay.len() && is_gap(hay[i]) {
            i += 1;
        }
        let (Some(&l), Some(&r)) = (start.checked_sub(1).and_then(|k| hay.get(k)), hay.get(i)) else { continue };
        if hit(l, r) {
            out.push(start..i.min(end));
        }
    }
    out
}

impl EditDoc {
    /// 一键消除中英/中数空格 in the selection, or the whole text when nothing is selected (one
    /// undo step); returns how many runs were removed.
    pub fn remove_cjk_latin_spaces(&mut self) -> usize {
        let sel = self.selection();
        let within = if sel.is_empty() { 0..self.len() } else { sel };
        let hits = cjk_latin_spaces(self.chars(), within);
        let n = hits.len();
        if n > 0 {
            self.replace_many(hits.into_iter().map(|r| (r, String::new())).collect());
        }
        n
    }

    /// 消除标点空格: spaces between punctuation and text in the selection, or the whole text when
    /// nothing is selected (one undo step); returns how many runs were removed.
    pub fn remove_punct_spaces(&mut self) -> usize {
        let sel = self.selection();
        let within = if sel.is_empty() { 0..self.len() } else { sel };
        let hits = punct_spaces(self.chars(), within);
        let n = hits.len();
        if n > 0 {
            self.replace_many(hits.into_iter().map(|r| (r, String::new())).collect());
        }
        n
    }

    pub fn find_all(&self, needle: &str, o: FindOpts) -> Vec<Range<usize>> {
        find_in(self.chars(), needle, o)
    }

    /// 全部替换 (one undo step); returns how many were replaced. Replaced text gets fresh ids
    /// and the formatting of the text it replaces; every other char keeps its id.
    pub fn replace_all(&mut self, needle: &str, with: &str, o: FindOpts) -> usize {
        let hits = self.find_all(needle, o);
        let n = hits.len();
        self.replace_many(hits.into_iter().map(|r| (r, with.to_string())).collect());
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(match_case: bool, fold_width: bool) -> FindOpts {
        FindOpts { match_case, fold_width }
    }

    #[test]
    fn matches_never_overlap() {
        let d = EditDoc::from_text("aaaa ab");
        assert_eq!(d.find_all("aa", FindOpts::default()), vec![0..2, 2..4]);
        assert_eq!(d.find_all("aaa", FindOpts::default()), vec![0..3]);
        assert!(d.find_all("", FindOpts::default()).is_empty());
        assert!(d.find_all("zz", FindOpts::default()).is_empty());
    }

    #[test]
    fn case_and_width_options() {
        let d = EditDoc::from_text("Abc ＡＢＣ abc");
        assert_eq!(d.find_all("abc", opts(false, false)), vec![0..3, 8..11]);
        assert_eq!(d.find_all("abc", opts(true, false)), vec![8..11]);
        assert_eq!(d.find_all("abc", opts(false, true)), vec![0..3, 4..7, 8..11]);
        assert_eq!(d.find_all("ABC", opts(true, true)), vec![4..7]);
        // The ideographic space is a space when widths fold.
        let d = EditDoc::from_text("你\u{3000}好");
        assert_eq!(d.find_all("你 好", opts(false, true)), vec![0..3]);
    }

    #[test]
    fn cjk_latin_spaces_go_latin_word_spaces_stay() {
        let mut d = EditDoc::from_text("用 Rust 写 2 个  demo，hello world ， ok\n中\u{3000}A");
        assert_eq!(d.remove_cjk_latin_spaces(), 8);
        assert_eq!(d.text(), "用Rust写2个demo，hello world，ok\n中A");
        assert!(d.undo());
        assert_eq!(d.text(), "用 Rust 写 2 个  demo，hello world ， ok\n中\u{3000}A");
        // A space across a newline, or between two Chinese chars, stays.
        let mut d = EditDoc::from_text("中 \nA 中 文");
        assert_eq!(d.remove_cjk_latin_spaces(), 1);
        assert_eq!(d.text(), "中 \nA中 文");
    }

    #[test]
    fn punct_spaces_go_english_spacing_stays() {
        let src = "你好 ， 世界 。 「 引 」 Hello , world ( x ) ok. 中文, 好 (注)\n句号 \u{3000}。";
        let mut d = EditDoc::from_text(src);
        assert_eq!(d.remove_punct_spaces(), 14);
        assert_eq!(d.text(), "你好，世界。「引」Hello, world (x) ok.中文,好(注)\n句号。");
        assert!(d.undo());
        assert_eq!(d.text(), src);
        // Between two words, or across a newline, nothing goes.
        let mut d = EditDoc::from_text("中 文 a b ok. \n，x");
        assert_eq!(d.remove_punct_spaces(), 0);
    }

    #[test]
    fn cjk_latin_spaces_respect_the_range() {
        let hay: Vec<char> = "中 A 中 B".chars().collect();
        assert_eq!(cjk_latin_spaces(&hay, 0..hay.len()), vec![1..2, 3..4, 5..6]);
        assert_eq!(cjk_latin_spaces(&hay, 2..5), vec![3..4]);
    }

    #[test]
    fn replace_all_keeps_other_ids_and_undoes_at_once() {
        let mut d = EditDoc::from_text("猫和狗，猫和鸟");
        let before = d.ids().to_vec();
        assert_eq!(d.replace_all("猫", "小猫", FindOpts::default()), 2);
        assert_eq!(d.text(), "小猫和狗，小猫和鸟");
        // Untouched chars keep their ids; the new ones are fresh.
        let after = d.ids();
        assert_eq!(&after[2..5], &before[1..4]);
        assert_eq!(&after[7..], &before[5..]);
        assert!(!before.contains(&after[0]) && !before.contains(&after[1]));
        assert!(d.undo());
        assert_eq!(d.text(), "猫和狗，猫和鸟");
        assert!(!d.undo());
    }
}
