//! Parser for the Mermaid flowchart subset PenKaku draws: `flowchart` / `graph` with a
//! direction, nodes `A`, `A[t]`, `A(t)`, `A([t])`, `A((t))`, `A{t}` (plus a few shapes drawn as
//! their nearest cousin), links `-->` `---` `-.->` `-.-` `==>` `===` with `|label|` or
//! `-- label -->` text, chains `A --> B --> C`, `A & B --> C`, `;` separators and `%%` comments.
//! Styling statements (`classDef`, `style`, `click`, …) and `subgraph` / `end` wrappers are
//! accepted and ignored (the nodes inside a subgraph are still drawn).

/// Flow direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dir {
    /// Top to bottom (`TB` / `TD`).
    #[default]
    Down,
    /// Bottom to top (`BT`).
    Up,
    /// Left to right (`LR`).
    Right,
    /// Right to left (`RL`).
    Left,
}

/// Node outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shape {
    #[default]
    Rect,
    Round,
    Stadium,
    Circle,
    Diamond,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: String,
    /// Text drawn inside; `'\n'` breaks a line (`<br>` in the source).
    pub label: String,
    pub shape: Shape,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub label: Option<String>,
    /// Arrowhead at `to`.
    pub arrow: bool,
    /// Dotted link (`-.->`), drawn dashed.
    pub dashed: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Graph {
    pub dir: Dir,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// A parse failure: 1-based source line and a message for the user (Chinese).
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub line: usize,
    pub msg: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "第 {} 行：{}", self.line, self.msg)
    }
}

/// Statements that only style or group things; skipped.
const IGNORED: [&str; 8] = ["classDef", "class", "style", "linkStyle", "click", "subgraph", "end", "direction"];

pub fn parse(src: &str) -> Result<Graph, ParseError> {
    let mut g = Graph::default();
    let mut header = false;
    for (ln, raw) in src.lines().enumerate() {
        let line = ln + 1;
        let text = match raw.find("%%") {
            Some(k) => &raw[..k],
            None => raw,
        };
        for stmt in split_statements(text) {
            let stmt = stmt.trim();
            if stmt.is_empty() {
                continue;
            }
            if !header {
                g.dir = parse_header(stmt).ok_or_else(|| ParseError { line, msg: "第一行应为 flowchart TD / graph LR 等".into() })?;
                header = true;
                continue;
            }
            let word = stmt.split_whitespace().next().unwrap_or("");
            if IGNORED.contains(&word) {
                continue;
            }
            Statement { g: &mut g, s: stmt.chars().collect(), p: 0, line }.run()?;
        }
    }
    if !header {
        return Err(ParseError { line: 1, msg: "内容为空".into() });
    }
    if g.nodes.is_empty() {
        return Err(ParseError { line: 1, msg: "没有任何节点".into() });
    }
    Ok(g)
}

/// Split at `;` outside brackets and quotes.
fn split_statements(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut quoted, mut start) = (0i32, false, 0);
    for (k, c) in s.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '[' | '(' | '{' if !quoted => depth += 1,
            ']' | ')' | '}' if !quoted => depth -= 1,
            ';' if !quoted && depth <= 0 => {
                out.push(&s[start..k]);
                start = k + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

fn parse_header(s: &str) -> Option<Dir> {
    let mut w = s.split_whitespace();
    if !matches!(w.next()?, "flowchart" | "graph") {
        return None;
    }
    Some(match w.next().unwrap_or("TB") {
        "TB" | "TD" => Dir::Down,
        "BT" => Dir::Up,
        "LR" => Dir::Right,
        "RL" => Dir::Left,
        _ => return None,
    })
}

fn id_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Mermaid label text: quotes stripped, `<br>` as a line break, surrounding space trimmed.
fn clean_label(t: &str) -> String {
    let t = t.trim();
    let t = t.strip_prefix('"').and_then(|t| t.strip_suffix('"')).unwrap_or(t);
    t.replace("<br/>", "\n").replace("<br />", "\n").replace("<br>", "\n").lines().map(str::trim).collect::<Vec<_>>().join("\n")
}

struct Statement<'a> {
    g: &'a mut Graph,
    s: Vec<char>,
    p: usize,
    line: usize,
}

/// A parsed link between two node groups.
struct Link {
    label: Option<String>,
    arrow: bool,
    dashed: bool,
}

impl Statement<'_> {
    fn err(&self, msg: &str) -> ParseError {
        ParseError { line: self.line, msg: msg.into() }
    }

    fn ws(&mut self) {
        while self.s.get(self.p).is_some_and(|c| c.is_whitespace()) {
            self.p += 1;
        }
    }

    fn starts(&self, pat: &str) -> bool {
        let n = pat.chars().count();
        self.p + n <= self.s.len() && self.s[self.p..self.p + n].iter().copied().eq(pat.chars())
    }

    /// `node (& node)* (link node (& node)*)*`.
    fn run(mut self) -> Result<(), ParseError> {
        let mut left = self.group()?;
        loop {
            self.ws();
            if self.p >= self.s.len() {
                return Ok(());
            }
            let link = self.link()?;
            let right = self.group()?;
            for &a in &left {
                for &b in &right {
                    self.g.edges.push(Edge { from: a, to: b, label: link.label.clone(), arrow: link.arrow, dashed: link.dashed });
                }
            }
            left = right;
        }
    }

    fn group(&mut self) -> Result<Vec<usize>, ParseError> {
        let mut v = vec![self.node()?];
        loop {
            self.ws();
            if self.s.get(self.p) != Some(&'&') {
                return Ok(v);
            }
            self.p += 1;
            v.push(self.node()?);
        }
    }

    fn node(&mut self) -> Result<usize, ParseError> {
        self.ws();
        let start = self.p;
        while self.s.get(self.p).copied().is_some_and(id_char) {
            self.p += 1;
        }
        if self.p == start {
            return Err(self.err("缺少节点名"));
        }
        let id: String = self.s[start..self.p].iter().collect();
        // (open, close, shape), longest openers first.
        const SHAPES: [(&str, &str, Shape); 10] = [
            ("((", "))", Shape::Circle),
            ("([", "])", Shape::Stadium),
            ("[(", ")]", Shape::Round),
            ("[[", "]]", Shape::Rect),
            ("{{", "}}", Shape::Diamond),
            ("[", "]", Shape::Rect),
            ("(", ")", Shape::Round),
            ("{", "}", Shape::Diamond),
            (">", "]", Shape::Rect),
            ("", "", Shape::Rect),
        ];
        let mut def = None;
        for (open, close, shape) in SHAPES {
            if open.is_empty() || !self.starts(open) {
                continue;
            }
            self.p += open.chars().count();
            let from = self.p;
            let mut quoted = false;
            loop {
                if self.p >= self.s.len() {
                    return Err(self.err(&format!("节点 {id} 缺少 {close}")));
                }
                if self.s[self.p] == '"' {
                    quoted = !quoted;
                } else if !quoted && self.starts(close) {
                    break;
                }
                self.p += 1;
            }
            let text: String = self.s[from..self.p].iter().collect();
            self.p += close.chars().count();
            def = Some((clean_label(&text), shape));
            break;
        }
        let ix = match self.g.nodes.iter().position(|n| n.id == id) {
            Some(i) => i,
            None => {
                self.g.nodes.push(Node { label: id.clone(), id, shape: Shape::Rect });
                self.g.nodes.len() - 1
            }
        };
        if let Some((label, shape)) = def {
            self.g.nodes[ix].label = label;
            self.g.nodes[ix].shape = shape;
        }
        Ok(ix)
    }

    /// A run of `-`, `=` and `.`, with an optional `>`.
    fn link_token(&mut self) -> String {
        let start = self.p;
        while self.s.get(self.p).is_some_and(|c| matches!(c, '-' | '=' | '.')) {
            self.p += 1;
        }
        if self.p > start && self.s.get(self.p) == Some(&'>') {
            self.p += 1;
        }
        self.s[start..self.p].iter().collect()
    }

    fn link(&mut self) -> Result<Link, ParseError> {
        let tok = self.link_token();
        let opener = matches!(tok.as_str(), "--" | "==" | "-.");
        if (tok.len() < 3 && !opener) || !tok.contains(['-', '=']) {
            return Err(self.err("无法识别的连线（应为 --> 、--- 、-.-> 、==> 等）"));
        }
        let mut link = Link { label: None, arrow: tok.ends_with('>'), dashed: tok.contains('.') };
        if opener {
            // `-- text -->`: the text runs to the closing link.
            const CLOSERS: [&str; 6] = ["-->", "---", "==>", "===", ".->", ".-"];
            let from = self.p;
            while self.p < self.s.len() && !CLOSERS.iter().any(|c| self.starts(c)) {
                self.p += 1;
            }
            if self.p >= self.s.len() {
                return Err(self.err("连线文字缺少结尾（如 -- 文字 -->）"));
            }
            link.label = Some(clean_label(&self.s[from..self.p].iter().collect::<String>()));
            let close = self.link_token();
            link.arrow = close.ends_with('>');
            link.dashed |= close.contains('.');
        }
        self.ws();
        if self.s.get(self.p) == Some(&'|') {
            let from = self.p + 1;
            let Some(k) = self.s[from..].iter().position(|&c| c == '|') else { return Err(self.err("连线文字缺少结尾的 |")) };
            link.label = Some(clean_label(&self.s[from..from + k].iter().collect::<String>()));
            self.p = from + k + 1;
        }
        link.label = link.label.filter(|l| !l.is_empty());
        Ok(link)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(g: &Graph) -> Vec<(&str, &str, Shape)> {
        g.nodes.iter().map(|n| (n.id.as_str(), n.label.as_str(), n.shape)).collect()
    }

    #[test]
    fn nodes_shapes_and_chains() {
        let g = parse("flowchart LR\n  A[开始] --> B{判断?}\n  B -->|是| C(完成)\n  B -- 否 --> D((重试)) --> A").unwrap();
        assert_eq!(g.dir, Dir::Right);
        assert_eq!(labels(&g), vec![("A", "开始", Shape::Rect), ("B", "判断?", Shape::Diamond), ("C", "完成", Shape::Round), ("D", "重试", Shape::Circle)]);
        let e: Vec<_> = g.edges.iter().map(|e| (e.from, e.to, e.label.as_deref(), e.arrow)).collect();
        assert_eq!(e, vec![(0, 1, None, true), (1, 2, Some("是"), true), (1, 3, Some("否"), true), (3, 0, None, true)]);
    }

    #[test]
    fn link_styles_groups_and_statements() {
        let g = parse("graph TD; a & b -.-> c; c === d %% note\n d ==> e").unwrap();
        assert_eq!(g.dir, Dir::Down);
        let e: Vec<_> = g.edges.iter().map(|e| (e.from, e.to, e.arrow, e.dashed)).collect();
        assert_eq!(e, vec![(0, 2, true, true), (1, 2, true, true), (2, 3, false, false), (3, 4, true, false)]);
    }

    #[test]
    fn quotes_breaks_stadium_and_ignored_lines() {
        let g = parse("flowchart TB\nclassDef x fill:#f00\nsubgraph S\n  s([\"a; b\"]) --> t[\"x<br>y\"]\nend\nstyle s fill:#0f0").unwrap();
        assert_eq!(labels(&g), vec![("s", "a; b", Shape::Stadium), ("t", "x\ny", Shape::Rect)]);
    }

    #[test]
    fn later_definition_wins_and_bare_ids_label_themselves() {
        let g = parse("graph LR\nA --> B\nB[终点]").unwrap();
        assert_eq!(labels(&g), vec![("A", "A", Shape::Rect), ("B", "终点", Shape::Rect)]);
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn errors_name_the_line() {
        assert_eq!(parse("sequenceDiagram\nA->>B: hi").unwrap_err().line, 1);
        assert_eq!(parse("graph TD\nA --> B\nB[open").unwrap_err().line, 3);
        assert_eq!(parse("graph TD\nA -> B").unwrap_err().line, 2);
        assert!(parse("").is_err());
        assert!(parse("graph TD").is_err());
    }
}
