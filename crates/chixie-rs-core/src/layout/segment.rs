//! 作文格分词: word boundaries from jieba (cargo feature `segment`), so a grid layout can keep a
//! word on one row. Without the feature there are no words and the option does nothing.

/// `ends[i]`: the end (exclusive char index) of the word of two or more chars starting at `i`.
#[cfg(feature = "segment")]
pub(super) fn word_ends(chars: &[char]) -> Vec<Option<usize>> {
    use std::sync::OnceLock;
    // Loading the dictionary takes a moment; only done once, and only when the option is on.
    static JIEBA: OnceLock<jieba_rs::Jieba> = OnceLock::new();
    let text: String = chars.iter().collect();
    let mut ends = vec![None; chars.len()];
    for t in JIEBA.get_or_init(jieba_rs::Jieba::new).cut(&text, true) {
        if t.end > t.start + 1 && t.end <= chars.len() {
            ends[t.start] = Some(t.end);
        }
    }
    ends
}

#[cfg(not(feature = "segment"))]
pub(super) fn word_ends(chars: &[char]) -> Vec<Option<usize>> {
    vec![None; chars.len()]
}
