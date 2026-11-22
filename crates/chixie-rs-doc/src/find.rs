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

impl EditDoc {
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
