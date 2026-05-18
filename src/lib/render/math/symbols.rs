//! TeX command / character → Unicode + atom class, plus the
//! Mathematical-Alphanumeric letter transforms used by `\mathbf`,
//! `\mathbb`, `\mathcal`, … . STIX Two Math covers all of Unicode
//! Plane-1 math, so we map to the real code points and let the font
//! supply the right glyph.

/// TeX atom class — drives inter-atom spacing (TeXbook Ch. 18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Ord,
    Op,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
    Inner,
}

/// Math font variant selected by `\mathXX` / `\text`. The full
/// Unicode Mathematical-Alphanumeric taxonomy; a few members
/// (`BoldItalic`, `SansBold`, …) have no `\command` mapped yet but
/// keep the table complete and `styled_letter` total.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// Default: italic letters, upright digits/operators.
    Normal,
    Italic,
    Bold,
    BoldItalic,
    Roman,
    SansSerif,
    SansBold,
    SansItalic,
    Mono,
    Script,
    ScriptBold,
    Fraktur,
    FrakturBold,
    DoubleStruck,
}

fn shift(base: u32, c: char, a: char) -> char {
    char::from_u32(base + (c as u32 - a as u32)).unwrap_or(c)
}

/// Map an ASCII letter/digit to its Mathematical-Alphanumeric code
/// point for `variant`, honouring the Unicode "holes" (letterlike
/// symbols that live outside the contiguous Plane-1 ranges).
pub fn styled_letter(c: char, variant: Variant) -> char {
    use Variant::*;
    let lower = c.is_ascii_lowercase();
    let upper = c.is_ascii_uppercase();
    let digit = c.is_ascii_digit();
    match variant {
        Normal | Roman => c,
        Italic => {
            if c == 'h' {
                return '\u{210E}'; // Planck constant ℎ
            }
            if lower {
                shift(0x1D44E, c, 'a')
            } else if upper {
                shift(0x1D434, c, 'A')
            } else {
                c
            }
        }
        Bold => {
            if lower {
                shift(0x1D41A, c, 'a')
            } else if upper {
                shift(0x1D400, c, 'A')
            } else if digit {
                shift(0x1D7CE, c, '0')
            } else {
                c
            }
        }
        BoldItalic => {
            if lower {
                shift(0x1D482, c, 'a')
            } else if upper {
                shift(0x1D468, c, 'A')
            } else {
                c
            }
        }
        SansSerif => {
            if lower {
                shift(0x1D5BA, c, 'a')
            } else if upper {
                shift(0x1D5A0, c, 'A')
            } else if digit {
                shift(0x1D7E2, c, '0')
            } else {
                c
            }
        }
        SansBold => {
            if lower {
                shift(0x1D5EE, c, 'a')
            } else if upper {
                shift(0x1D5D4, c, 'A')
            } else if digit {
                shift(0x1D7EC, c, '0')
            } else {
                c
            }
        }
        SansItalic => {
            if lower {
                shift(0x1D622, c, 'a')
            } else if upper {
                shift(0x1D608, c, 'A')
            } else {
                c
            }
        }
        Mono => {
            if lower {
                shift(0x1D68A, c, 'a')
            } else if upper {
                shift(0x1D670, c, 'A')
            } else if digit {
                shift(0x1D7F6, c, '0')
            } else {
                c
            }
        }
        Script | ScriptBold => {
            let bold = matches!(variant, ScriptBold);
            if upper {
                // Script-capital holes (letterlike symbols).
                match c {
                    'B' if !bold => return '\u{212C}',
                    'E' if !bold => return '\u{2130}',
                    'F' if !bold => return '\u{2131}',
                    'H' if !bold => return '\u{210B}',
                    'I' if !bold => return '\u{2110}',
                    'L' if !bold => return '\u{2112}',
                    'M' if !bold => return '\u{2133}',
                    'R' if !bold => return '\u{211B}',
                    _ => {}
                }
                shift(if bold { 0x1D4D0 } else { 0x1D49C }, c, 'A')
            } else if lower {
                match c {
                    'e' if !bold => return '\u{212F}',
                    'g' if !bold => return '\u{210A}',
                    'o' if !bold => return '\u{2134}',
                    _ => {}
                }
                shift(if bold { 0x1D4EA } else { 0x1D4B6 }, c, 'a')
            } else {
                c
            }
        }
        Fraktur | FrakturBold => {
            let bold = matches!(variant, FrakturBold);
            if upper {
                match c {
                    'C' if !bold => return '\u{212D}',
                    'H' if !bold => return '\u{210C}',
                    'I' if !bold => return '\u{2111}',
                    'R' if !bold => return '\u{211C}',
                    'Z' if !bold => return '\u{2128}',
                    _ => {}
                }
                shift(if bold { 0x1D56C } else { 0x1D504 }, c, 'A')
            } else if lower {
                shift(if bold { 0x1D586 } else { 0x1D51E }, c, 'a')
            } else {
                c
            }
        }
        DoubleStruck => {
            if upper {
                match c {
                    'C' => return '\u{2102}',
                    'H' => return '\u{210D}',
                    'N' => return '\u{2115}',
                    'P' => return '\u{2119}',
                    'Q' => return '\u{211A}',
                    'R' => return '\u{211D}',
                    'Z' => return '\u{2124}',
                    _ => {}
                }
                shift(0x1D538, c, 'A')
            } else if lower {
                shift(0x1D552, c, 'a')
            } else if digit {
                shift(0x1D7D8, c, '0')
            } else {
                c
            }
        }
    }
}

/// Resolve a `\name` control word to `(unicode, class)`. Returns
/// `None` for non-symbol commands (handled structurally by the
/// parser: `\frac`, `\sqrt`, accents, fonts, spacing, …).
pub fn command(name: &str) -> Option<(char, Class)> {
    use Class::*;
    let v = |s: char, c: Class| Some((s, c));
    match name {
        // Greek lowercase (math italic).
        "alpha" => v('\u{1D6FC}', Ord),
        "beta" => v('\u{1D6FD}', Ord),
        "gamma" => v('\u{1D6FE}', Ord),
        "delta" => v('\u{1D6FF}', Ord),
        "epsilon" => v('\u{1D716}', Ord),
        "varepsilon" => v('\u{1D700}', Ord),
        "zeta" => v('\u{1D701}', Ord),
        "eta" => v('\u{1D702}', Ord),
        "theta" => v('\u{1D703}', Ord),
        "vartheta" => v('\u{1D717}', Ord),
        "iota" => v('\u{1D704}', Ord),
        "kappa" => v('\u{1D705}', Ord),
        "lambda" => v('\u{1D706}', Ord),
        "mu" => v('\u{1D707}', Ord),
        "nu" => v('\u{1D708}', Ord),
        "xi" => v('\u{1D709}', Ord),
        "pi" => v('\u{1D70B}', Ord),
        "varpi" => v('\u{1D71B}', Ord),
        "rho" => v('\u{1D70C}', Ord),
        "varrho" => v('\u{1D71A}', Ord),
        "sigma" => v('\u{1D70E}', Ord),
        "varsigma" => v('\u{1D70D}', Ord),
        "tau" => v('\u{1D70F}', Ord),
        "upsilon" => v('\u{1D710}', Ord),
        "phi" => v('\u{1D719}', Ord),
        "varphi" => v('\u{1D711}', Ord),
        "chi" => v('\u{1D712}', Ord),
        "psi" => v('\u{1D713}', Ord),
        "omega" => v('\u{1D714}', Ord),
        // Greek uppercase (upright, TeX convention).
        "Gamma" => v('\u{0393}', Ord),
        "Delta" => v('\u{0394}', Ord),
        "Theta" => v('\u{0398}', Ord),
        "Lambda" => v('\u{039B}', Ord),
        "Xi" => v('\u{039E}', Ord),
        "Pi" => v('\u{03A0}', Ord),
        "Sigma" => v('\u{03A3}', Ord),
        "Upsilon" => v('\u{03A5}', Ord),
        "Phi" => v('\u{03A6}', Ord),
        "Psi" => v('\u{03A8}', Ord),
        "Omega" => v('\u{03A9}', Ord),
        // Binary operators.
        "pm" => v('\u{00B1}', Bin),
        "mp" => v('\u{2213}', Bin),
        "times" => v('\u{00D7}', Bin),
        "div" => v('\u{00F7}', Bin),
        "cdot" => v('\u{22C5}', Bin),
        "ast" => v('\u{2217}', Bin),
        "star" => v('\u{22C6}', Bin),
        "circ" => v('\u{2218}', Bin),
        "bullet" => v('\u{2219}', Bin),
        "oplus" => v('\u{2295}', Bin),
        "ominus" => v('\u{2296}', Bin),
        "otimes" => v('\u{2297}', Bin),
        "oslash" => v('\u{2298}', Bin),
        "odot" => v('\u{2299}', Bin),
        "wedge" | "land" => v('\u{2227}', Bin),
        "vee" | "lor" => v('\u{2228}', Bin),
        "cap" => v('\u{2229}', Bin),
        "cup" => v('\u{222A}', Bin),
        "sqcap" => v('\u{2293}', Bin),
        "sqcup" => v('\u{2294}', Bin),
        "uplus" => v('\u{228E}', Bin),
        "amalg" => v('\u{2A3F}', Bin),
        "dagger" => v('\u{2020}', Bin),
        "ddagger" => v('\u{2021}', Bin),
        "setminus" => v('\u{2216}', Bin),
        "smallsetminus" => v('\u{2216}', Bin),
        "wr" => v('\u{2240}', Bin),
        "diamond" => v('\u{22C4}', Bin),
        "bigtriangleup" => v('\u{25B3}', Bin),
        "bigtriangledown" => v('\u{25BD}', Bin),
        "triangleleft" => v('\u{25C3}', Bin),
        "triangleright" => v('\u{25B9}', Bin),
        "boxplus" => v('\u{229E}', Bin),
        "boxtimes" => v('\u{22A0}', Bin),
        // Relations.
        "leq" | "le" => v('\u{2264}', Rel),
        "geq" | "ge" => v('\u{2265}', Rel),
        "neq" | "ne" => v('\u{2260}', Rel),
        "equiv" => v('\u{2261}', Rel),
        "approx" => v('\u{2248}', Rel),
        "cong" => v('\u{2245}', Rel),
        "simeq" => v('\u{2243}', Rel),
        "sim" => v('\u{223C}', Rel),
        "propto" => v('\u{221D}', Rel),
        "doteq" => v('\u{2250}', Rel),
        "asymp" => v('\u{224D}', Rel),
        "ll" => v('\u{226A}', Rel),
        "gg" => v('\u{226B}', Rel),
        "in" => v('\u{2208}', Rel),
        "notin" => v('\u{2209}', Rel),
        "ni" => v('\u{220B}', Rel),
        "subset" => v('\u{2282}', Rel),
        "supset" => v('\u{2283}', Rel),
        "subseteq" => v('\u{2286}', Rel),
        "supseteq" => v('\u{2287}', Rel),
        "subsetneq" => v('\u{228A}', Rel),
        "supsetneq" => v('\u{228B}', Rel),
        "sqsubseteq" => v('\u{2291}', Rel),
        "sqsupseteq" => v('\u{2292}', Rel),
        "prec" => v('\u{227A}', Rel),
        "succ" => v('\u{227B}', Rel),
        "preceq" => v('\u{2AAF}', Rel),
        "succeq" => v('\u{2AB0}', Rel),
        "parallel" => v('\u{2225}', Rel),
        "perp" => v('\u{27C2}', Rel),
        "mid" => v('\u{2223}', Rel),
        "models" => v('\u{22A8}', Rel),
        "vdash" => v('\u{22A2}', Rel),
        "dashv" => v('\u{22A3}', Rel),
        "cong " => v('\u{2245}', Rel),
        "ne " => v('\u{2260}', Rel),
        "bowtie" => v('\u{22C8}', Rel),
        "frown" => v('\u{2322}', Rel),
        "smile" => v('\u{2323}', Rel),
        // Arrows (relations).
        "to" | "rightarrow" => v('\u{2192}', Rel),
        "gets" | "leftarrow" => v('\u{2190}', Rel),
        "leftrightarrow" => v('\u{2194}', Rel),
        "Rightarrow" | "implies" => v('\u{21D2}', Rel),
        "Leftarrow" => v('\u{21D0}', Rel),
        "Leftrightarrow" | "iff" => v('\u{21D4}', Rel),
        "mapsto" => v('\u{21A6}', Rel),
        "longrightarrow" => v('\u{27F6}', Rel),
        "longleftarrow" => v('\u{27F5}', Rel),
        "longleftrightarrow" => v('\u{27F7}', Rel),
        "Longrightarrow" => v('\u{27F9}', Rel),
        "longmapsto" => v('\u{27FC}', Rel),
        "uparrow" => v('\u{2191}', Rel),
        "downarrow" => v('\u{2193}', Rel),
        "updownarrow" => v('\u{2195}', Rel),
        "nearrow" => v('\u{2197}', Rel),
        "searrow" => v('\u{2198}', Rel),
        "swarrow" => v('\u{2199}', Rel),
        "nwarrow" => v('\u{2196}', Rel),
        "hookrightarrow" => v('\u{21AA}', Rel),
        "hookleftarrow" => v('\u{21A9}', Rel),
        "rightharpoonup" => v('\u{21C0}', Rel),
        "leftharpoonup" => v('\u{21BC}', Rel),
        // Ordinary symbols.
        "infty" => v('\u{221E}', Ord),
        "partial" => v('\u{2202}', Ord),
        "nabla" => v('\u{2207}', Ord),
        "forall" => v('\u{2200}', Ord),
        "exists" => v('\u{2203}', Ord),
        "nexists" => v('\u{2204}', Ord),
        "emptyset" | "varnothing" => v('\u{2205}', Ord),
        "neg" | "lnot" => v('\u{00AC}', Ord),
        "top" => v('\u{22A4}', Ord),
        "bot" => v('\u{22A5}', Ord),
        "angle" => v('\u{2220}', Ord),
        "measuredangle" => v('\u{2221}', Ord),
        "triangle" => v('\u{25B3}', Ord),
        "square" => v('\u{25A1}', Ord),
        "Box" => v('\u{25A1}', Ord),
        "diamondsuit" => v('\u{2662}', Ord),
        "heartsuit" => v('\u{2661}', Ord),
        "spadesuit" => v('\u{2660}', Ord),
        "clubsuit" => v('\u{2663}', Ord),
        "flat" => v('\u{266D}', Ord),
        "sharp" => v('\u{266F}', Ord),
        "natural" => v('\u{266E}', Ord),
        "hbar" => v('\u{210F}', Ord),
        "ell" => v('\u{2113}', Ord),
        "wp" => v('\u{2118}', Ord),
        "Re" => v('\u{211C}', Ord),
        "Im" => v('\u{2111}', Ord),
        "aleph" => v('\u{2135}', Ord),
        "beth" => v('\u{2136}', Ord),
        "complement" => v('\u{2201}', Ord),
        "prime" => v('\u{2032}', Ord),
        "backprime" => v('\u{2035}', Ord),
        "degree" => v('\u{00B0}', Ord),
        "circledR" => v('\u{00AE}', Ord),
        "checkmark" => v('\u{2713}', Ord),
        "maltese" => v('\u{2720}', Ord),
        "imath" => v('\u{1D6A4}', Ord),
        "jmath" => v('\u{1D6A5}', Ord),
        "surd" => v('\u{221A}', Ord),
        "neg " => v('\u{00AC}', Ord),
        "dots" | "ldots" => v('\u{2026}', Inner),
        "cdots" => v('\u{22EF}', Inner),
        "vdots" => v('\u{22EE}', Inner),
        "ddots" => v('\u{22F1}', Inner),
        "colon" => v('\u{003A}', Punct),
        // Delimiters.
        "langle" => v('\u{27E8}', Open),
        "rangle" => v('\u{27E9}', Close),
        "lceil" => v('\u{2308}', Open),
        "rceil" => v('\u{2309}', Close),
        "lfloor" => v('\u{230A}', Open),
        "rfloor" => v('\u{230B}', Close),
        "lbrace" => v('{', Open),
        "rbrace" => v('}', Close),
        "lbrack" => v('[', Open),
        "rbrack" => v(']', Close),
        "vert" => v('\u{007C}', Ord),
        "Vert" => v('\u{2016}', Ord),
        "|" => v('\u{2016}', Ord),
        "backslash" => v('\u{005C}', Ord),
        // Backslash-escaped literals (`\{`, `\%`, `\#`, …) — common in
        // set-builder notation and units.
        "{" => v('{', Open),
        "}" => v('}', Close),
        "%" => v('%', Ord),
        "#" => v('#', Ord),
        "&" => v('&', Ord),
        "_" => v('_', Ord),
        "$" => v('$', Ord),
        "uparrow " => v('\u{2191}', Ord),
        // Big operators (class Op; the layout gives them limits).
        "sum" => v('\u{2211}', Op),
        "prod" => v('\u{220F}', Op),
        "coprod" => v('\u{2210}', Op),
        "int" => v('\u{222B}', Op),
        "iint" => v('\u{222C}', Op),
        "iiint" => v('\u{222D}', Op),
        "oint" => v('\u{222E}', Op),
        "bigcup" => v('\u{22C3}', Op),
        "bigcap" => v('\u{22C2}', Op),
        "bigsqcup" => v('\u{2A06}', Op),
        "bigvee" => v('\u{22C1}', Op),
        "bigwedge" => v('\u{22C0}', Op),
        "bigodot" => v('\u{2A00}', Op),
        "bigoplus" => v('\u{2A01}', Op),
        "bigotimes" => v('\u{2A02}', Op),
        "biguplus" => v('\u{2A04}', Op),
        // Punctuation / spacing-ish.
        "ldotp" => v('\u{002E}', Punct),
        "cdotp" => v('\u{22C5}', Punct),
        _ => None,
    }
}

/// Operator names typeset upright (`\sin`, `\log`, …). The bool is
/// `true` when the operator takes limits in display style
/// (`\lim`, `\max`, `\det`, …).
pub fn operator_name(name: &str) -> Option<(&'static str, bool)> {
    Some(match name {
        "sin" => ("sin", false),
        "cos" => ("cos", false),
        "tan" => ("tan", false),
        "cot" => ("cot", false),
        "sec" => ("sec", false),
        "csc" => ("csc", false),
        "arcsin" => ("arcsin", false),
        "arccos" => ("arccos", false),
        "arctan" => ("arctan", false),
        "sinh" => ("sinh", false),
        "cosh" => ("cosh", false),
        "tanh" => ("tanh", false),
        "coth" => ("coth", false),
        "log" => ("log", false),
        "ln" => ("ln", false),
        "lg" => ("lg", false),
        "exp" => ("exp", false),
        "deg" => ("deg", false),
        "arg" => ("arg", false),
        "dim" => ("dim", false),
        "hom" => ("hom", false),
        "ker" => ("ker", false),
        "lim" => ("lim", true),
        "limsup" => ("lim sup", true),
        "liminf" => ("lim inf", true),
        "max" => ("max", true),
        "min" => ("min", true),
        "sup" => ("sup", true),
        "inf" => ("inf", true),
        "det" => ("det", true),
        "gcd" => ("gcd", true),
        "Pr" => ("Pr", true),
        _ => return None,
    })
}

/// Class of a single literal character in math mode.
pub fn char_class(c: char) -> Class {
    match c {
        '+' | '-' | '*' | '/' => Class::Bin,
        '=' | '<' | '>' => Class::Rel,
        '(' | '[' => Class::Open,
        ')' | ']' => Class::Close,
        ',' | ';' => Class::Punct,
        '.' | '?' | '!' => Class::Ord,
        _ => Class::Ord,
    }
}

/// A few literal characters render better as a dedicated math glyph.
pub fn char_remap(c: char) -> char {
    match c {
        '-' => '\u{2212}', // minus sign
        '*' => '\u{2217}', // asterisk operator
        '\'' => '\u{2032}', // prime
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_table_class_and_codepoint() {
        // Greek lowercase is math-italic; uppercase upright.
        assert_eq!(command("alpha"), Some(('\u{1D6FC}', Class::Ord)));
        assert_eq!(command("Gamma"), Some(('\u{0393}', Class::Ord)));
        // Big operators are Op (the layout gives them limits).
        assert_eq!(command("sum"), Some(('\u{2211}', Class::Op)));
        assert_eq!(command("int"), Some(('\u{222B}', Class::Op)));
        assert_eq!(command("prod"), Some(('\u{220F}', Class::Op)));
        // Relations / binops / delimiters land in the right class.
        assert_eq!(command("leq"), Some(('\u{2264}', Class::Rel)));
        assert_eq!(command("to"), Some(('\u{2192}', Class::Rel)));
        assert_eq!(command("pm"), Some(('\u{00B1}', Class::Bin)));
        assert_eq!(command("times"), Some(('\u{00D7}', Class::Bin)));
        assert_eq!(command("langle"), Some(('\u{27E8}', Class::Open)));
        assert_eq!(command("rangle"), Some(('\u{27E9}', Class::Close)));
        // Aliases resolve identically.
        assert_eq!(command("le"), command("leq"));
        assert_eq!(command("ge"), command("geq"));
        assert_eq!(command("land"), command("wedge"));
        assert_eq!(command("rightarrow"), command("to"));
        assert_eq!(command("neg"), command("lnot"));
        // Backslash-escaped literals resolve to their glyph.
        assert_eq!(command("{"), Some(('{', Class::Open)));
        assert_eq!(command("}"), Some(('}', Class::Close)));
        assert_eq!(command("%"), Some(('%', Class::Ord)));
        assert_eq!(command("#"), Some(('#', Class::Ord)));
        assert_eq!(command("_"), Some(('_', Class::Ord)));
        // Structural commands are not symbols.
        assert_eq!(command("frac"), None);
        assert_eq!(command("totallybogus"), None);
    }

    #[test]
    fn operator_names_and_limits() {
        assert_eq!(operator_name("sin"), Some(("sin", false)));
        assert_eq!(operator_name("log"), Some(("log", false)));
        assert_eq!(operator_name("lim"), Some(("lim", true)));
        assert_eq!(operator_name("max"), Some(("max", true)));
        assert_eq!(operator_name("limsup"), Some(("lim sup", true)));
        assert_eq!(operator_name("det"), Some(("det", true)));
        assert_eq!(operator_name("frac"), None);
    }

    #[test]
    fn literal_char_class_and_remap() {
        assert_eq!(char_class('+'), Class::Bin);
        assert_eq!(char_class('-'), Class::Bin);
        assert_eq!(char_class('='), Class::Rel);
        assert_eq!(char_class('<'), Class::Rel);
        assert_eq!(char_class('('), Class::Open);
        assert_eq!(char_class(')'), Class::Close);
        assert_eq!(char_class(','), Class::Punct);
        assert_eq!(char_class('x'), Class::Ord);
        // Prettier math glyphs for a few ASCII chars.
        assert_eq!(char_remap('-'), '\u{2212}');
        assert_eq!(char_remap('*'), '\u{2217}');
        assert_eq!(char_remap('\''), '\u{2032}');
        assert_eq!(char_remap('x'), 'x');
    }

    #[test]
    fn styled_letter_contiguous_ranges() {
        use Variant::*;
        assert_eq!(styled_letter('x', Italic), '\u{1D465}');
        assert_eq!(styled_letter('A', Italic), '\u{1D434}');
        assert_eq!(styled_letter('a', Bold), '\u{1D41A}');
        assert_eq!(styled_letter('0', Bold), '\u{1D7CE}');
        assert_eq!(styled_letter('z', SansSerif), '\u{1D5D3}');
        assert_eq!(styled_letter('a', Mono), '\u{1D68A}');
        // Normal / Roman pass through; non-letters always pass through.
        assert_eq!(styled_letter('x', Normal), 'x');
        assert_eq!(styled_letter('x', Roman), 'x');
        assert_eq!(styled_letter('+', Bold), '+');
        assert_eq!(styled_letter('5', Italic), '5'); // italic digits don't exist
    }

    #[test]
    fn styled_letter_unicode_holes() {
        use Variant::*;
        // Letterlike-symbols carve holes out of the Plane-1 ranges.
        assert_eq!(styled_letter('h', Italic), '\u{210E}'); // ℎ Planck
        assert_eq!(styled_letter('C', DoubleStruck), '\u{2102}'); // ℂ
        assert_eq!(styled_letter('N', DoubleStruck), '\u{2115}'); // ℕ
        assert_eq!(styled_letter('R', DoubleStruck), '\u{211D}'); // ℝ
        assert_eq!(styled_letter('Z', DoubleStruck), '\u{2124}'); // ℤ
        assert_eq!(styled_letter('n', DoubleStruck), '\u{1D55F}'); // contiguous
        assert_eq!(styled_letter('B', Script), '\u{212C}'); // ℬ
        assert_eq!(styled_letter('L', Script), '\u{2112}'); // ℒ
        assert_eq!(styled_letter('e', Script), '\u{212F}'); // ℯ
        assert_eq!(styled_letter('A', Script), '\u{1D49C}'); // contiguous
        assert_eq!(styled_letter('H', Fraktur), '\u{210C}'); // ℌ
        assert_eq!(styled_letter('R', Fraktur), '\u{211C}'); // ℜ
        assert_eq!(styled_letter('C', Fraktur), '\u{212D}'); // ℭ
        assert_eq!(styled_letter('a', Fraktur), '\u{1D51E}'); // contiguous
    }
}
