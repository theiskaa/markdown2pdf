//! TeX-math → atom tree. A practical recursive-descent parser over
//! the constructs people actually write in markdown math. Unknown
//! commands degrade to upright text rather than failing, and depth /
//! length are bounded so adversarial input can't blow the stack.

use super::symbols::{
    char_class, char_remap, command, operator_name, styled_letter, Class, Variant,
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
    Frac { num: Vec<Node>, den: Vec<Node>, bar: bool },
    /// `\sqrt[index]{body}`.
    Sqrt { index: Option<Vec<Node>>, body: Vec<Node> },
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
        loop {
            let Some(tok) = self.lx.peek() else { break };
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
            let Some(node) = self.atom(var) else { break };
            // Attach trailing scripts / primes.
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
                        ch: styled_letter(c, if var == Variant::Normal { Variant::Italic } else { var }),
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
        // Spacing.
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
                Some(Node::Frac { num, den, bar: true })
            }
            "binom" | "dbinom" | "tbinom" => {
                let num = self.arg(var);
                let den = self.arg(var);
                Some(Node::Delimited {
                    left: Some('('),
                    right: Some(')'),
                    body: vec![Node::Frac { num, den, bar: false }],
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
                // consume \right
                let mut right = None;
                if self.lx.peek() == Some(Tok::Cmd("right".into())) {
                    self.lx.next();
                    right = self.delim_char();
                }
                Some(Node::Delimited { left: d, right, body })
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
            "limits" | "nolimits" | "displaystyle" | "textstyle" | "scriptstyle"
            | "limits " | "," => Some(Node::Space(0.0)),
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

    /// Collect a `{ … }` (or single token) as literal text.
    fn raw_group(&mut self) -> String {
        let mut s = String::new();
        if self.lx.peek() == Some(Tok::Open) {
            self.lx.next();
            let mut depth = 1;
            while let Some(t) = self.lx.next() {
                match t {
                    Tok::Open => depth += 1,
                    Tok::Close => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    Tok::Char(c) => s.push(c),
                    Tok::Cmd(c) => {
                        s.push_str(&c);
                        s.push(' ');
                    }
                    _ => {}
                }
            }
        } else if let Some(Tok::Char(c)) = self.lx.peek() {
            self.lx.next();
            s.push(c);
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
            "matrix" | "array" | "aligned" | "align" | "align*" | "alignedat"
            | "gathered" | "smallmatrix" => (None, None, name.starts_with("align")),
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
            "", "{", "}", "^", "_", "\\", "\\\\", "\\frac", "\\sqrt[",
            "\\left(", "{{{{{{{{{{", "\\begin{matrix}", "a & b \\\\ c",
            "x^^_2", "\\frac{}{}", "\\\\\\\\",
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
}
