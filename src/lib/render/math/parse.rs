//! TeX-math → atom tree. A practical recursive-descent parser over
//! the constructs people actually write in markdown math. Unknown
//! commands degrade to upright text rather than failing, and depth /
//! length are bounded so adversarial input can't blow the stack.

use super::symbols::{
    Class, Variant, char_class, char_remap, command, operator_name, styled_letter,
};

#[derive(Debug, Clone)]
pub enum Node {
    /// A single glyph atom (`ch` already mapped to its math code point).
    Symbol { ch: char, class: Class },
    /// A large operator (`\sum`, `\int`, …). `limits` requests
    /// over/under placement of scripts in display style.
    BigOp { ch: char, limits: bool },
    /// An upright operator name (`\sin`, `\lim`). `limits` as above.
    OpName { text: String, limits: bool },
    /// `{ … }` — a single Ord made of a sub-list.
    Group(Vec<Node>),
    /// `\frac` / `\binom` body (bar=false for binomials/`\atop`).
    Frac {
        num: Vec<Node>,
        den: Vec<Node>,
        bar: bool,
    },
    /// `\sqrt[index]{body}`.
    Sqrt {
        index: Option<Vec<Node>>,
        body: Vec<Node>,
    },
    /// A nucleus with optional super/sub-scripts.
    Scripts {
        base: Box<Node>,
        sup: Option<Vec<Node>>,
        sub: Option<Vec<Node>>,
    },
    /// `\left⟨ … \right⟩` (auto-grown fences; `None` = `.` null fence).
    Delimited {
        left: Option<char>,
        right: Option<char>,
        body: Vec<Node>,
    },
    /// `\bigl(` … : a fixed-enlarged delimiter. `level` 1..=4.
    SizedDelim { ch: char, class: Class, level: u8 },
    /// A combining math accent over `body`.
    Accent {
        mark: char,
        stretchy: bool,
        body: Vec<Node>,
    },
    /// `\overline` / `\underline` (rule) and `\overbrace`/`\underbrace`
    /// (stretchy brace), with an optional script label.
    OverUnder {
        body: Vec<Node>,
        over: Option<char>,
        under: Option<char>,
        rule: bool,
    },
    /// Upright text (`\text{…}`, `\operatorname{…}`).
    Text(String),
    /// Explicit horizontal space, in em.
    Space(f32),
    /// A matrix / cases / aligned environment.
    Array {
        rows: Vec<Vec<Vec<Node>>>,
        left: Option<char>,
        right: Option<char>,
        /// `true` for `aligned`/`cases`-style left-set columns.
        align_left: bool,
    },
}

struct Lexer {
    src: Vec<char>,
    i: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Char(char),
    Cmd(String),
    Open,
    Close,
    Sup,
    Sub,
    Amp,
    Newline, // `\\`
    Prime(usize),
}

impl Lexer {
    fn new(s: &str) -> Self {
        Lexer {
            src: s.chars().collect(),
            i: 0,
        }
    }

    fn next(&mut self) -> Option<Tok> {
        loop {
            let c = *self.src.get(self.i)?;
            self.i += 1;
            match c {
                ' ' | '\t' | '\n' | '\r' => continue,
                '{' => return Some(Tok::Open),
                '}' => return Some(Tok::Close),
                '^' => return Some(Tok::Sup),
                '_' => return Some(Tok::Sub),
                '&' => return Some(Tok::Amp),
                '\'' => {
                    let mut n = 1;
                    while self.src.get(self.i) == Some(&'\'') {
                        n += 1;
                        self.i += 1;
                    }
                    return Some(Tok::Prime(n));
                }
                '\\' => {
                    let Some(&d) = self.src.get(self.i) else {
                        return Some(Tok::Char('\\'));
                    };
                    if d == '\\' {
                        self.i += 1;
                        return Some(Tok::Newline);
                    }
                    if d.is_ascii_alphabetic() {
                        let mut name = String::new();
                        while let Some(&e) = self.src.get(self.i) {
                            if e.is_ascii_alphabetic() {
                                name.push(e);
                                self.i += 1;
                            } else {
                                break;
                            }
                        }
                        return Some(Tok::Cmd(name));
                    }
                    // Control symbol: `\,` `\{` `\!` `\ ` `\|` …
                    self.i += 1;
                    return Some(Tok::Cmd(d.to_string()));
                }
                _ => return Some(Tok::Char(c)),
            }
        }
    }

    fn peek(&mut self) -> Option<Tok> {
        let save = self.i;
        let t = self.next();
        self.i = save;
        t
    }
}

pub fn parse(src: &str) -> Vec<Node> {
    // Bound pathological input: STIX layout is O(atoms) but deeply
    // nested braces recurse, so cap both.
    let src: String = src.chars().take(20_000).collect();
    let mut p = Parser {
        lx: Lexer::new(&src),
        depth: 0,
    };
    p.list(StopAt::Eof, Variant::Normal)
}

#[derive(Clone, Copy, PartialEq)]
enum StopAt {
    Eof,
    Brace,
    Right,
}

struct Parser {
    lx: Lexer,
    depth: usize,
}

impl Parser {
    /// Parse a math list until the stop condition. `var` is the
    /// active `\mathXX` letter style.
    fn list(&mut self, stop: StopAt, var: Variant) -> Vec<Node> {
        let mut out: Vec<Node> = Vec::new();
        if self.depth > 200 {
            return out;
        }
        while let Some(tok) = self.lx.peek() {
            match (&tok, stop) {
                (Tok::Close, StopAt::Brace) => {
                    self.lx.next();
                    break;
                }
                (Tok::Cmd(c), StopAt::Right) if c == "right" => break,
                (Tok::Cmd(c), _) if c == "end" => break,
                (Tok::Amp, _) | (Tok::Newline, _) => break,
                _ => {}
            }
            let Some(mut node) = self.atom(var) else {
                break;
            };
            // `\limits` / `\nolimits` immediately after a big operator
            // override its default script placement.
            while let Some(Tok::Cmd(c)) = self.lx.peek() {
                let over = match c.as_str() {
                    "limits" => true,
                    "nolimits" => false,
                    _ => break,
                };
                self.lx.next();
                if let Node::BigOp { limits, .. } | Node::OpName { limits, .. } = &mut node {
                    *limits = over;
                }
            }
            let node = self.scripts(node, var);
            out.push(node);
        }
        out
    }

    fn scripts(&mut self, mut base: Node, var: Variant) -> Node {
        let mut sup: Option<Vec<Node>> = None;
        let mut sub: Option<Vec<Node>> = None;
        loop {
            match self.lx.peek() {
                Some(Tok::Prime(n)) => {
                    self.lx.next();
                    let primes: Vec<Node> = (0..n)
                        .map(|_| Node::Symbol {
                            ch: '\u{2032}',
                            class: Class::Ord,
                        })
                        .collect();
                    match &mut sup {
                        Some(s) => {
                            s.splice(0..0, primes);
                        }
                        None => sup = Some(primes),
                    }
                }
                Some(Tok::Sup) => {
                    self.lx.next();
                    let g = self.arg(var);
                    sup = Some(match sup {
                        Some(mut s) => {
                            s.extend(g);
                            s
                        }
                        None => g,
                    });
                }
                Some(Tok::Sub) => {
                    self.lx.next();
                    sub = Some(self.arg(var));
                }
                _ => break,
            }
        }
        if sup.is_some() || sub.is_some() {
            base = Node::Scripts {
                base: Box::new(base),
                sup,
                sub,
            };
        }
        base
    }

    /// One `{group}` or single token as an argument list.
    fn arg(&mut self, var: Variant) -> Vec<Node> {
        match self.lx.peek() {
            Some(Tok::Open) => {
                self.lx.next();
                self.depth += 1;
                let l = self.list(StopAt::Brace, var);
                self.depth -= 1;
                l
            }
            Some(_) => {
                if let Some(n) = self.atom(var) {
                    vec![n]
                } else {
                    vec![]
                }
            }
            None => vec![],
        }
    }

    fn atom(&mut self, var: Variant) -> Option<Node> {
        let tok = self.lx.next()?;
        Some(match tok {
            Tok::Open => {
                self.depth += 1;
                let l = self.list(StopAt::Brace, var);
                self.depth -= 1;
                Node::Group(l)
            }
            Tok::Close | Tok::Sup | Tok::Sub | Tok::Prime(_) | Tok::Amp | Tok::Newline => {
                // Stray control char — render literally.
                Node::Symbol {
                    ch: match tok {
                        Tok::Sup => '^',
                        Tok::Sub => '_',
                        _ => '\u{FFFD}',
                    },
                    class: Class::Ord,
                }
            }
            Tok::Char(c) => {
                if c.is_ascii_alphabetic() {
                    Node::Symbol {
                        ch: styled_letter(
                            c,
                            if var == Variant::Normal {
                                Variant::Italic
                            } else {
                                var
                            },
                        ),
                        class: Class::Ord,
                    }
                } else if c.is_ascii_digit() {
                    Node::Symbol {
                        ch: styled_letter(c, var),
                        class: Class::Ord,
                    }
                } else {
                    Node::Symbol {
                        ch: char_remap(c),
                        class: char_class(c),
                    }
                }
            }
            Tok::Cmd(name) => self.command(&name, var)?,
        })
    }

    fn command(&mut self, name: &str, var: Variant) -> Option<Node> {
        let em = match name {
            "," => Some(3.0 / 18.0),
            ":" | ">" => Some(4.0 / 18.0),
            ";" => Some(5.0 / 18.0),
            "!" => Some(-3.0 / 18.0),
            " " => Some(6.0 / 18.0),
            "quad" => Some(1.0),
            "qquad" => Some(2.0),
            "thinspace" => Some(3.0 / 18.0),
            "enspace" => Some(0.5),
            _ => None,
        };
        if let Some(e) = em {
            return Some(Node::Space(e));
        }

        match name {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let num = self.arg(var);
                let den = self.arg(var);
                Some(Node::Frac {
                    num,
                    den,
                    bar: true,
                })
            }
            "binom" | "dbinom" | "tbinom" => {
                let num = self.arg(var);
                let den = self.arg(var);
                Some(Node::Delimited {
                    left: Some('('),
                    right: Some(')'),
                    body: vec![Node::Frac {
                        num,
                        den,
                        bar: false,
                    }],
                })
            }
            "sqrt" => {
                let index = if self.lx.peek() == Some(Tok::Char('[')) {
                    self.lx.next();
                    Some(self.until_char(']', var))
                } else {
                    None
                };
                let body = self.arg(var);
                Some(Node::Sqrt { index, body })
            }
            "left" => {
                let d = self.delim_char();
                self.depth += 1;
                let body = self.list(StopAt::Right, var);
                self.depth -= 1;
                let mut right = None;
                if self.lx.peek() == Some(Tok::Cmd("right".into())) {
                    self.lx.next();
                    right = self.delim_char();
                }
                Some(Node::Delimited {
                    left: d,
                    right,
                    body,
                })
            }
            "right" => {
                let _ = self.delim_char();
                None
            }
            "bigl" | "bigr" | "big" | "bigm" => self.sized(1),
            "Bigl" | "Bigr" | "Big" | "Bigm" => self.sized(2),
            "biggl" | "biggr" | "bigg" | "biggm" => self.sized(3),
            "Biggl" | "Biggr" | "Bigg" | "Biggm" => self.sized(4),
            "mathbf" | "boldsymbol" | "bm" | "pmb" => Some(self.styled(Variant::Bold)),
            "mathit" => Some(self.styled(Variant::Italic)),
            "mathrm" | "mathnormal" => Some(self.styled(Variant::Roman)),
            "mathsf" | "textsf" => Some(self.styled(Variant::SansSerif)),
            "mathtt" | "texttt" => Some(self.styled(Variant::Mono)),
            "mathcal" => Some(self.styled(Variant::Script)),
            "mathscr" => Some(self.styled(Variant::Script)),
            "mathbb" => Some(self.styled(Variant::DoubleStruck)),
            "mathfrak" => Some(self.styled(Variant::Fraktur)),
            "text" | "textnormal" | "mbox" | "textbf" | "textit" => {
                Some(Node::Text(self.raw_group()))
            }
            "operatorname" => {
                let t = self.raw_group();
                Some(Node::OpName {
                    text: t,
                    limits: false,
                })
            }
            "hat" => self.accent('\u{0302}', false, var),
            "widehat" => self.accent('\u{0302}', true, var),
            "tilde" => self.accent('\u{0303}', false, var),
            "widetilde" => self.accent('\u{0303}', true, var),
            "bar" => self.accent('\u{0304}', false, var),
            "vec" => self.accent('\u{20D7}', false, var),
            "dot" => self.accent('\u{0307}', false, var),
            "ddot" => self.accent('\u{0308}', false, var),
            "acute" => self.accent('\u{0301}', false, var),
            "grave" => self.accent('\u{0300}', false, var),
            "check" => self.accent('\u{030C}', false, var),
            "breve" => self.accent('\u{0306}', false, var),
            "mathring" => self.accent('\u{030A}', false, var),
            "overline" => {
                let body = self.arg(var);
                Some(Node::OverUnder {
                    body,
                    over: Some('\u{2015}'),
                    under: None,
                    rule: true,
                })
            }
            "underline" => {
                let body = self.arg(var);
                Some(Node::OverUnder {
                    body,
                    over: None,
                    under: Some('\u{2015}'),
                    rule: true,
                })
            }
            "overbrace" => {
                let body = self.arg(var);
                Some(Node::OverUnder {
                    body,
                    over: Some('\u{23DE}'),
                    under: None,
                    rule: false,
                })
            }
            "underbrace" => {
                let body = self.arg(var);
                Some(Node::OverUnder {
                    body,
                    over: None,
                    under: Some('\u{23DF}'),
                    rule: false,
                })
            }
            "begin" => self.environment(var),
            "end" => {
                let _ = self.raw_group();
                None
            }
            // `\limits`/`\nolimits` are consumed by `list()` after a big
            // operator; reaching here means stray use — treat as no-op.
            "limits" | "nolimits" | "displaystyle" | "textstyle" | "scriptstyle"
            | "scriptscriptstyle" => Some(Node::Space(0.0)),
            "not" => {
                // Overstrike the next atom with a slash.
                let nxt = self.atom(var)?;
                Some(Node::Accent {
                    mark: '\u{0338}',
                    stretchy: false,
                    body: vec![nxt],
                })
            }
            _ => {
                if let Some((s, c)) = command(name) {
                    if c == Class::Op {
                        Some(Node::BigOp {
                            ch: s,
                            limits: !matches!(name, "int" | "iint" | "iiint" | "oint"),
                        })
                    } else {
                        Some(Node::Symbol { ch: s, class: c })
                    }
                } else if let Some((text, limits)) = operator_name(name) {
                    Some(Node::OpName {
                        text: text.to_string(),
                        limits,
                    })
                } else {
                    // Unknown command: show it literally so nothing
                    // silently disappears.
                    Some(Node::Text(format!("\\{name}")))
                }
            }
        }
    }

    fn sized(&mut self, level: u8) -> Option<Node> {
        let d = self.delim_char()?;
        Some(Node::SizedDelim {
            ch: d,
            class: match d {
                '(' | '[' | '{' | '\u{27E8}' | '\u{2308}' | '\u{230A}' => Class::Open,
                _ => Class::Close,
            },
            level,
        })
    }

    fn styled(&mut self, v: Variant) -> Node {
        Node::Group(self.arg(v))
    }

    fn accent(&mut self, mark: char, stretchy: bool, var: Variant) -> Option<Node> {
        let body = self.arg(var);
        Some(Node::Accent {
            mark,
            stretchy,
            body,
        })
    }

    /// Read the next delimiter following `\left` / `\bigl` / etc.
    fn delim_char(&mut self) -> Option<char> {
        match self.lx.next()? {
            Tok::Char('.') => None,
            Tok::Char('(') => Some('('),
            Tok::Char(')') => Some(')'),
            Tok::Char('[') => Some('['),
            Tok::Char(']') => Some(']'),
            Tok::Char('|') => Some('\u{007C}'),
            Tok::Char('/') => Some('/'),
            Tok::Char(c) => Some(c),
            Tok::Open => Some('{'),
            Tok::Close => Some('}'),
            Tok::Cmd(c) => command(&c).map(|(s, _)| s).or(match c.as_str() {
                "{" => Some('{'),
                "}" => Some('}'),
                "|" => Some('\u{2016}'),
                _ => None,
            }),
            _ => None,
        }
    }

    /// Collect a `{ … }` (or single token) as *verbatim* literal text.
    /// Reads the source directly rather than through the tokenizer so
    /// inter-word spaces survive (`\text{hi there}`, `\operatorname{lim
    /// sup}`); a leading backslash escapes the next character.
    fn raw_group(&mut self) -> String {
        let lx = &mut self.lx;
        while matches!(lx.src.get(lx.i), Some(' ' | '\t' | '\n' | '\r')) {
            lx.i += 1;
        }
        let mut s = String::new();
        match lx.src.get(lx.i).copied() {
            Some('{') => {
                lx.i += 1;
                let mut depth = 1usize;
                while let Some(&c) = lx.src.get(lx.i) {
                    lx.i += 1;
                    match c {
                        '\\' => {
                            if let Some(&n) = lx.src.get(lx.i) {
                                s.push(n);
                                lx.i += 1;
                            } else {
                                s.push('\\');
                            }
                        }
                        '{' => {
                            depth += 1;
                            s.push('{');
                        }
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            s.push('}');
                        }
                        _ => s.push(c),
                    }
                }
            }
            Some(c) => {
                lx.i += 1;
                s.push(c);
            }
            None => {}
        }
        s
    }

    fn until_char(&mut self, end: char, var: Variant) -> Vec<Node> {
        let mut out = Vec::new();
        while let Some(t) = self.lx.peek() {
            if t == Tok::Char(end) {
                self.lx.next();
                break;
            }
            let Some(n) = self.atom(var) else { break };
            out.push(self.scripts(n, var));
        }
        out
    }

    fn environment(&mut self, var: Variant) -> Option<Node> {
        let name = self.raw_group();
        let name = name.trim();
        let (left, right, align_left) = match name {
            "pmatrix" => (Some('('), Some(')'), false),
            "bmatrix" => (Some('['), Some(']'), false),
            "Bmatrix" => (Some('{'), Some('}'), false),
            "vmatrix" => (Some('\u{007C}'), Some('\u{007C}'), false),
            "Vmatrix" => (Some('\u{2016}'), Some('\u{2016}'), false),
            "cases" => (Some('{'), None, true),
            "matrix" | "array" | "aligned" | "align" | "align*" | "alignedat" | "gathered"
            | "smallmatrix" => (None, None, name.starts_with("align")),
            _ => (None, None, false),
        };
        let mut rows: Vec<Vec<Vec<Node>>> = vec![vec![]];
        loop {
            match self.lx.peek() {
                None => break,
                Some(Tok::Cmd(c)) if c == "end" => {
                    self.lx.next();
                    let _ = self.raw_group();
                    break;
                }
                Some(Tok::Newline) => {
                    self.lx.next();
                    rows.push(vec![]);
                }
                Some(Tok::Amp) => {
                    self.lx.next();
                    rows.last_mut().unwrap().push(vec![]);
                }
                _ => {
                    let cell = self.list(StopAt::Eof, var);
                    if cell.is_empty() {
                        // list() stopped on & / \\ / end without
                        // consuming — advance defensively.
                        if matches!(self.lx.peek(), Some(Tok::Amp) | Some(Tok::Newline)) {
                            continue;
                        }
                        match self.lx.peek() {
                            Some(Tok::Cmd(c)) if c == "end" => continue,
                            None => break,
                            _ => {
                                self.lx.next();
                            }
                        }
                    } else {
                        let row = rows.last_mut().unwrap();
                        if row.is_empty() {
                            row.push(cell);
                        } else {
                            row.last_mut().unwrap().extend(cell);
                        }
                    }
                }
            }
        }
        if rows.last().map(|r| r.is_empty()).unwrap_or(false) {
            rows.pop();
        }
        Some(Node::Array {
            rows,
            left,
            right,
            align_left,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frac_and_scripts() {
        let n = parse("x^2 + \\frac{1}{2}");
        // x with sup, +, frac
        assert!(matches!(n[0], Node::Scripts { .. }));
        assert!(n.iter().any(|t| matches!(t, Node::Frac { .. })));
    }

    #[test]
    fn sqrt_with_index() {
        let n = parse("\\sqrt[3]{x}");
        match &n[0] {
            Node::Sqrt { index, body } => {
                assert!(index.is_some());
                assert_eq!(body.len(), 1);
            }
            _ => panic!("expected sqrt"),
        }
    }

    #[test]
    fn left_right_delimiters() {
        let n = parse("\\left( a \\right)");
        assert!(matches!(
            n[0],
            Node::Delimited {
                left: Some('('),
                right: Some(')'),
                ..
            }
        ));
    }

    #[test]
    fn unknown_command_degrades_to_text() {
        let n = parse("\\foobar x");
        assert!(matches!(&n[0], Node::Text(t) if t == "\\foobar"));
    }

    #[test]
    fn adversarial_inputs_do_not_panic() {
        for s in [
            "",
            "{",
            "}",
            "^",
            "_",
            "\\",
            "\\\\",
            "\\frac",
            "\\sqrt[",
            "\\left(",
            "{{{{{{{{{{",
            "\\begin{matrix}",
            "a & b \\\\ c",
            "x^^_2",
            "\\frac{}{}",
            "\\\\\\\\",
        ] {
            let _ = parse(s);
        }
    }

    #[test]
    fn matrix_environment() {
        let n = parse("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}");
        match &n[0] {
            Node::Array { rows, left, .. } => {
                assert_eq!(*left, Some('('));
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].len(), 2);
            }
            _ => panic!("expected array"),
        }
    }

    fn only(src: &str) -> Node {
        let mut n = parse(src);
        assert_eq!(n.len(), 1, "expected a single node from {src:?}: {n:?}");
        n.pop().unwrap()
    }

    #[test]
    fn scripts_super_sub_and_order() {
        // Both scripts attach to the same nucleus regardless of order.
        for src in ["x^2_n", "x_n^2"] {
            match only(src) {
                Node::Scripts { sup, sub, .. } => {
                    assert!(sup.is_some() && sub.is_some(), "{src}");
                }
                other => panic!("{src}: expected Scripts, got {other:?}"),
            }
        }
    }

    #[test]
    fn primes_become_superscript_runs() {
        match only("x''") {
            Node::Scripts {
                sup: Some(s), sub, ..
            } => {
                assert!(sub.is_none());
                assert_eq!(s.len(), 2);
                assert!(
                    s.iter()
                        .all(|t| matches!(t, Node::Symbol { ch: '\u{2032}', .. }))
                );
            }
            other => panic!("expected primed Scripts, got {other:?}"),
        }
        // A prime then an explicit superscript merge into one run.
        match only("x'^2") {
            Node::Scripts { sup: Some(s), .. } => {
                assert!(s.len() >= 2);
                assert!(matches!(s[0], Node::Symbol { ch: '\u{2032}', .. }));
            }
            other => panic!("expected merged prime+sup, got {other:?}"),
        }
    }

    #[test]
    fn frac_family_and_binom() {
        for cmd in ["frac", "dfrac", "tfrac", "cfrac"] {
            assert!(matches!(
                only(&format!("\\{cmd}{{1}}{{2}}")),
                Node::Frac { bar: true, .. }
            ));
        }
        // \binom is a bar-less fraction wrapped in parentheses.
        match only("\\binom{n}{k}") {
            Node::Delimited {
                left: Some('('),
                right: Some(')'),
                body,
            } => {
                assert!(matches!(body[0], Node::Frac { bar: false, .. }));
            }
            other => panic!("expected delimited binom, got {other:?}"),
        }
    }

    #[test]
    fn sqrt_without_index() {
        match only("\\sqrt{x+1}") {
            Node::Sqrt { index, body } => {
                assert!(index.is_none());
                assert!(!body.is_empty());
            }
            other => panic!("expected sqrt, got {other:?}"),
        }
    }

    #[test]
    fn null_and_sized_delimiters() {
        // `\left.` is a null (invisible) fence.
        match only("\\left. x \\right|") {
            Node::Delimited {
                left: None,
                right: Some('\u{007C}'),
                ..
            } => {}
            other => panic!("expected null-left delimited, got {other:?}"),
        }
        // \bigl( .. \Bigg) carry their enlargement level.
        assert!(matches!(
            parse("\\bigl(")[0],
            Node::SizedDelim {
                ch: '(',
                level: 1,
                ..
            }
        ));
        assert!(matches!(
            parse("\\Biggl[")[0],
            Node::SizedDelim {
                ch: '[',
                level: 4,
                ..
            }
        ));
    }

    #[test]
    fn accents_and_over_under() {
        assert!(matches!(
            only("\\hat{x}"),
            Node::Accent {
                mark: '\u{0302}',
                stretchy: false,
                ..
            }
        ));
        assert!(matches!(
            only("\\widehat{x}"),
            Node::Accent {
                mark: '\u{0302}',
                stretchy: true,
                ..
            }
        ));
        assert!(matches!(
            only("\\vec{v}"),
            Node::Accent {
                mark: '\u{20D7}',
                ..
            }
        ));
        assert!(matches!(
            only("\\overline{x}"),
            Node::OverUnder {
                over: Some(_),
                rule: true,
                ..
            }
        ));
        assert!(matches!(
            only("\\underbrace{x}"),
            Node::OverUnder {
                under: Some('\u{23DF}'),
                rule: false,
                ..
            }
        ));
    }

    #[test]
    fn text_operatorname_and_spacing() {
        assert!(matches!(only("\\text{hi there}"), Node::Text(t) if t == "hi there"));
        match only("\\operatorname{lcm}") {
            Node::OpName {
                text,
                limits: false,
            } => assert_eq!(text, "lcm"),
            other => panic!("expected OpName, got {other:?}"),
        }
        let n = parse("a\\,b\\quad c\\!d");
        let spaces: Vec<f32> = n
            .iter()
            .filter_map(|x| match x {
                Node::Space(e) => Some(*e),
                _ => None,
            })
            .collect();
        assert!(spaces.contains(&(3.0 / 18.0)));
        assert!(spaces.contains(&1.0));
        assert!(spaces.iter().any(|&e| e < 0.0), "\\! is negative space");
    }

    #[test]
    fn font_switches_bake_styled_codepoints() {
        // Default letters are math-italic.
        assert!(matches!(
            only("x"),
            Node::Symbol {
                ch: '\u{1D465}',
                ..
            }
        ));
        // \mathbb{R} bakes the double-struck code point.
        match only("\\mathbb{R}") {
            Node::Group(inner) => assert!(matches!(inner[0], Node::Symbol { ch: '\u{211D}', .. })),
            other => panic!("expected group, got {other:?}"),
        }
        match only("\\mathbf{A}") {
            Node::Group(inner) => assert!(matches!(
                inner[0],
                Node::Symbol {
                    ch: '\u{1D400}',
                    ..
                }
            )),
            other => panic!("expected group, got {other:?}"),
        }
    }

    #[test]
    fn big_operator_limits_flag() {
        assert!(matches!(
            only("\\sum"),
            Node::BigOp {
                ch: '\u{2211}',
                limits: true
            }
        ));
        // Integrals default to no over/under limits.
        assert!(matches!(
            only("\\int"),
            Node::BigOp {
                ch: '\u{222B}',
                limits: false
            }
        ));
    }

    #[test]
    fn limits_and_nolimits_override_the_default() {
        // \int\limits forces stacked limits onto an integral...
        match &parse("\\int\\limits_{0}^{1}")[0] {
            Node::Scripts { base, .. } => assert!(matches!(
                **base,
                Node::BigOp {
                    ch: '\u{222B}',
                    limits: true
                }
            )),
            other => panic!("expected scripted ∫, got {other:?}"),
        }
        // ...and \sum\nolimits forces side-set scripts onto a sum.
        match &parse("\\sum\\nolimits_{i}")[0] {
            Node::Scripts { base, .. } => assert!(matches!(
                **base,
                Node::BigOp {
                    ch: '\u{2211}',
                    limits: false
                }
            )),
            other => panic!("expected scripted ∑, got {other:?}"),
        }
        // \limits on a non-operator is a harmless no-op (no panic).
        let _ = parse("x\\limits^2");
    }

    #[test]
    fn environments_shapes() {
        match only("\\begin{bmatrix} 1 \\\\ 2 \\end{bmatrix}") {
            Node::Array {
                left: Some('['),
                right: Some(']'),
                rows,
                ..
            } => {
                assert_eq!(rows.len(), 2);
            }
            other => panic!("expected bmatrix, got {other:?}"),
        }
        match only("\\begin{cases} a & x>0 \\\\ b & x\\le 0 \\end{cases}") {
            Node::Array {
                left: Some('{'),
                right: None,
                align_left: true,
                rows,
            } => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].len(), 2);
            }
            other => panic!("expected cases, got {other:?}"),
        }
        match only("\\begin{aligned} a &= b \\\\ &= c \\end{aligned}") {
            Node::Array {
                left: None,
                right: None,
                align_left: true,
                rows,
            } => {
                assert_eq!(rows.len(), 2);
            }
            other => panic!("expected aligned, got {other:?}"),
        }
    }

    #[test]
    fn larger_adversarial_corpus_never_panics() {
        for s in [
            "\\frac{\\frac{\\frac{a}{b}}{c}}{d}",
            "\\sqrt[\\sqrt{2}]{\\sqrt{x}}",
            "x^_^_^",
            "\\left\\left\\left",
            "\\right)\\right)",
            "\\begin{matrix}",
            "\\end{matrix}",
            "&&&\\\\\\\\&&",
            "\\mathbb{\\frac{1}{2}}",
            "^{^{^{^{^{x}}}}}",
            "\\hat{\\hat{\\hat{\\hat{x}}}}",
            "\\text{",
            "\\operatorname",
            "\\\\begin{cases}",
            "$\\%\\#\\&\\_\\{\\}$",
            &"{".repeat(5000),
            &"x^".repeat(5000),
        ] {
            let _ = parse(s);
        }
    }

    #[test]
    fn unbraced_single_token_arguments() {
        // TeX: an unbraced argument is exactly one token.
        match only("\\frac ab") {
            Node::Frac { num, den, .. } => {
                assert_eq!(num.len(), 1);
                assert_eq!(den.len(), 1);
            }
            other => panic!("expected \\frac a b, got {other:?}"),
        }
        // \sqrt x — one-token radicand, no index.
        match only("\\sqrt x") {
            Node::Sqrt { index: None, body } => assert_eq!(body.len(), 1),
            other => panic!("expected \\sqrt x, got {other:?}"),
        }
        // x^2y : the 2 is the script, y is a sibling.
        let n = parse("x^2y");
        assert!(matches!(n[0], Node::Scripts { .. }));
        assert_eq!(n.len(), 2, "y must be its own atom: {n:?}");
    }

    #[test]
    fn scriptless_and_degenerate_scripts_dont_panic() {
        // A superscript with no nucleus (start of input / group).
        let _ = parse("^2");
        let _ = parse("_n");
        let _ = parse("{}^{2}");
        // Double subscript (a TeX error) must degrade, not panic, and
        // must not drop the trailing token.
        let n = parse("x_a_b c");
        assert!(matches!(n[0], Node::Scripts { .. }));
        assert!(n.iter().any(|t| matches!(
            t,
            Node::Symbol { ch, .. } if *ch == super::super::symbols::styled_letter('c', super::super::symbols::Variant::Italic)
        )));
        // Unterminated optional index for \sqrt.
        let _ = parse("\\sqrt[3{x}");
        let _ = parse("\\sqrt[}");
    }

    #[test]
    fn common_commands_all_resolve() {
        // Anything an author routinely writes must map to a symbol,
        // an operator name, or a structural node — never silently to
        // literal `\name` text. Catches typo'd / trailing-space table
        // keys.
        // Constructors only — `\right` / `\end` are closers that
        // legitimately produce nothing when used standalone.
        let structural = [
            "frac",
            "dfrac",
            "tfrac",
            "binom",
            "sqrt",
            "left",
            "text",
            "operatorname",
            "hat",
            "vec",
            "bar",
            "tilde",
            "dot",
            "overline",
            "underline",
            "overbrace",
            "underbrace",
            "mathbb",
            "mathbf",
            "mathcal",
            "mathrm",
            "mathfrak",
            "mathsf",
            "quad",
            "qquad",
            "bigl",
            "Big",
        ];
        for cmd in [
            "alpha",
            "beta",
            "gamma",
            "pi",
            "theta",
            "lambda",
            "mu",
            "phi",
            "omega",
            "Gamma",
            "Delta",
            "Omega",
            "sum",
            "prod",
            "int",
            "oint",
            "lim",
            "sin",
            "cos",
            "tan",
            "log",
            "ln",
            "exp",
            "min",
            "max",
            "leq",
            "geq",
            "neq",
            "approx",
            "equiv",
            "in",
            "subset",
            "cup",
            "cap",
            "to",
            "rightarrow",
            "Rightarrow",
            "mapsto",
            "infty",
            "partial",
            "nabla",
            "forall",
            "exists",
            "times",
            "div",
            "pm",
            "cdot",
            "circ",
            "oplus",
            "otimes",
            "langle",
            "rangle",
            "lfloor",
            "rfloor",
            "lceil",
            "rceil",
            "hbar",
            "ell",
            "Re",
            "Im",
            "aleph",
            "emptyset",
            "ldots",
            "cdots",
            "vdots",
            "wedge",
            "vee",
            "neg",
            "uparrow",
            "downarrow",
            "cong",
            "sim",
            "propto",
            "perp",
            "parallel",
            "mid",
            "setminus",
            "star",
            "ast",
            "bullet",
            "dagger",
        ] {
            let n = parse(&format!("\\{cmd}"));
            assert_eq!(n.len(), 1, "\\{cmd} produced {} nodes", n.len());
            let resolved = matches!(
                &n[0],
                Node::Symbol { .. } | Node::BigOp { .. } | Node::OpName { .. } | Node::Space(_)
            );
            assert!(
                resolved,
                "\\{cmd} did not resolve to a symbol/op (got {:?}) — \
                 likely a typo'd or trailing-space table key",
                n[0]
            );
        }
        for cmd in structural {
            // A constructor must produce a node and must NOT fall
            // through to literal `\name` text.
            let n = parse(&format!("\\{cmd}{{x}}{{y}}"));
            let head = n
                .first()
                .unwrap_or_else(|| panic!("\\{cmd} produced nothing"));
            assert!(
                !matches!(head, Node::Text(t) if t.starts_with('\\')),
                "\\{cmd} fell through to literal text"
            );
        }
    }
}
