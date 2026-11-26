//! 避头尾 (kinsoku): which marks may not start or end a line. The sets are ChiXie3's
//! `PostPunctuation` / `LeadingPunctuation` tables (docs/chixie3-features.md §避头尾); a mark in
//! both counts as closing only.

const CLOSING: &str = "!),.:;?]}¨·ˇˉ―‖’”…∶、。〃々〉》」』】〕〗！＂＇），．：；？］｀｜｝～￠%";
const OPENING: &str = "([{‘“〈《「『【〔〖（［｛￡￥";

/// Must not start a line.
pub fn is_closing(c: char) -> bool {
    CLOSING.contains(c)
}

/// Must not end a line.
pub fn is_opening(c: char) -> bool {
    OPENING.contains(c) && !is_closing(c)
}

/// Most characters moved to the next line to satisfy the rules; past this the line breaks where
/// it would have and the line is reported in `Document::punct_issues`.
pub(super) const MAX_PULL: usize = 3;

/// Where to break a line that would otherwise break before char `i`, given the line starts at
/// `line_start`: the latest `b` in `i - MAX_PULL..=i` with no closing mark at `b` and no opening
/// mark at `b - 1`, keeping at least one char on the line. `i` when no such `b` exists.
pub(super) fn break_before(chars: &[char], line_start: usize, i: usize) -> usize {
    let ok = |b: usize| !is_closing(chars[b]) && !is_opening(chars[b - 1]);
    (line_start + 1..=i).rev().take(MAX_PULL + 1).find(|&b| ok(b)).unwrap_or(i)
}

/// Where a line breaking before char `i` should break so an ASCII letter / digit run is not cut
/// (`24|H` → `|24H`): the start of the run `i` sits in, if that keeps something on the line.
/// `i` when `i` does not split a run, or the run started the line.
pub(super) fn word_start(chars: &[char], line_start: usize, i: usize) -> usize {
    let word = |k: usize| chars.get(k).is_some_and(char::is_ascii_alphanumeric);
    if i == 0 || !word(i) || !word(i - 1) {
        return i;
    }
    let start = (line_start..i).rev().take_while(|&k| word(k)).last().unwrap_or(i);
    if start > line_start { start } else { i }
}

/// Lines that start with a closing mark after a wrap, or end with an opening mark before one.
pub(super) fn issues(chars: &[char], lines: &[super::LineInfo]) -> Vec<usize> {
    let wrapped_into = |start: usize| start > 0 && start < chars.len() && chars[start - 1] != '\n';
    let mut out = Vec::new();
    for (k, l) in lines.iter().enumerate() {
        let starts_bad = wrapped_into(l.start) && is_closing(chars[l.start]);
        let ends_bad = lines.get(k + 1).is_some_and(|n| wrapped_into(n.start) && n.start == l.end) && l.end > l.start && is_opening(chars[l.end - 1]);
        if starts_bad || ends_bad {
            out.push(k);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets() {
        assert!(is_closing('，') && is_closing('。') && is_closing('”') && is_closing('·'));
        assert!(is_opening('“') && is_opening('《') && !is_opening('·'));
        assert!(!is_closing('你') && !is_opening('你'));
    }

    #[test]
    fn words_move_down_whole() {
        let c: Vec<char> = "24H;24H:22H".chars().collect();
        assert_eq!(word_start(&c, 0, 6), 4); // 24|H → |24H
        assert_eq!(word_start(&c, 0, 4), 4); // after `;`: not inside a run
        assert_eq!(word_start(&c, 4, 6), 6); // the run starts the line: cut it
        let c: Vec<char> = "中文abc".chars().collect();
        assert_eq!(word_start(&c, 0, 4), 2);
    }

    #[test]
    fn break_search() {
        let c: Vec<char> = "你好，世界".chars().collect();
        assert_eq!(break_before(&c, 0, 2), 1); // "，" pulls "好" down
        assert_eq!(break_before(&c, 0, 3), 3);
        let c: Vec<char> = "你“好".chars().collect();
        assert_eq!(break_before(&c, 0, 2), 1); // "“" moves down with "好"
        let c: Vec<char> = "。。。。。".chars().collect();
        assert_eq!(break_before(&c, 0, 4), 4); // nothing allowed: break as before
    }
}
