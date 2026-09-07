use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Word,
    Hex,
    Token,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

pub trait Matcher {
    fn find(&self, text: &str, out: &mut Vec<Span>);
}

pub struct Literals {
    ac: Option<AhoCorasick>,
}

impl Literals {
    pub fn new<I, S>(words: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let words: Vec<String> = words
            .into_iter()
            .map(|w| w.as_ref().trim().to_string())
            .filter(|w| !w.is_empty())
            .collect();
        let ac = if words.is_empty() {
            None
        } else {
            AhoCorasickBuilder::new()
                .ascii_case_insensitive(true)
                .match_kind(MatchKind::LeftmostLongest)
                .build(&words)
                .ok()
        };
        Self { ac }
    }
}

impl Matcher for Literals {
    fn find(&self, text: &str, out: &mut Vec<Span>) {
        if let Some(ac) = &self.ac {
            for m in ac.find_iter(text) {
                out.push(Span { start: m.start(), end: m.end(), kind: Kind::Word });
            }
        }
    }
}

struct Pattern {
    re: Regex,
    kind: Kind,
    skip: Option<fn(&str) -> bool>,
}

pub struct Patterns {
    list: Vec<Pattern>,
}

fn skip_ipv4(s: &str) -> bool {
    s.starts_with("127.") || s == "0.0.0.0" || s.starts_with("255.")
}

fn skip_ipv6(s: &str) -> bool {
    let colons = s.matches(':').count();
    (colons < 3 && !s.contains("::")) || s == "::1" || s == "::"
}

impl Patterns {
    pub fn builtin() -> Self {
        let defs: [(&str, Kind, Option<fn(&str) -> bool>); 11] = [
            (r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}", Kind::Word, None),
            (r"\b(?:\d{1,3}\.){3}\d{1,3}\b", Kind::Word, Some(skip_ipv4)),
            (r"\b(?:[0-9a-fA-F]{2}[:-]){5}[0-9a-fA-F]{2}\b", Kind::Hex, None),
            (r"(?:[0-9a-fA-F]{0,4}:){2,7}[0-9a-fA-F]{1,4}\b", Kind::Hex, Some(skip_ipv6)),
            (r"\bAKIA([0-9A-Z]{16})\b", Kind::Token, None),
            (r"\bgh[pousr]_([A-Za-z0-9]{36,})", Kind::Token, None),
            (r"\bgithub_pat_([A-Za-z0-9_]{22,})", Kind::Token, None),
            (r"\bxox[baprs]-([A-Za-z0-9-]{10,})", Kind::Token, None),
            (r"\bsk-([A-Za-z0-9_-]{20,})", Kind::Token, None),
            (r"\beyJ([A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,})", Kind::Token, None),
            (r"(?i)\bbearer\s+([A-Za-z0-9._~+/=-]{16,})", Kind::Token, None),
        ];
        let list = defs
            .into_iter()
            .map(|(re, kind, skip)| Pattern { re: Regex::new(re).expect("builtin regex"), kind, skip })
            .collect();
        Self { list }
    }

    pub fn from_strings<I, S>(patterns: I) -> Result<Self, regex::Error>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut list = Vec::new();
        for p in patterns {
            list.push(Pattern { re: Regex::new(p.as_ref())?, kind: Kind::Word, skip: None });
        }
        Ok(Self { list })
    }
}

impl Matcher for Patterns {
    fn find(&self, text: &str, out: &mut Vec<Span>) {
        for p in &self.list {
            for caps in p.re.captures_iter(text) {
                let whole = caps.get(0).unwrap();
                if p.skip.is_some_and(|f| f(whole.as_str())) {
                    continue;
                }
                let m = caps.get(1).unwrap_or(whole);
                out.push(Span { start: m.start(), end: m.end(), kind: p.kind });
            }
        }
    }
}

#[derive(Default)]
pub struct RuleSet {
    matchers: Vec<Box<dyn Matcher>>,
}

impl RuleSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push<M: Matcher + 'static>(&mut self, m: M) {
        self.matchers.push(Box::new(m));
    }

    pub fn find(&self, text: &str) -> Vec<Span> {
        let mut spans = Vec::new();
        for m in &self.matchers {
            m.find(text, &mut spans);
        }
        spans.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
        let mut merged: Vec<Span> = Vec::with_capacity(spans.len());
        for s in spans {
            match merged.last() {
                Some(last) if s.start < last.end => {}
                _ => merged.push(s),
            }
        }
        merged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(rs: &RuleSet, text: &str) -> Vec<(usize, usize, Kind)> {
        rs.find(text).into_iter().map(|s| (s.start, s.end, s.kind)).collect()
    }

    fn matched<'a>(rs: &RuleSet, text: &'a str) -> Vec<&'a str> {
        rs.find(text).into_iter().map(|s| &text[s.start..s.end]).collect()
    }

    #[test]
    fn literal_case_insensitive_longest() {
        let mut rs = RuleSet::new();
        rs.push(Literals::new(["some", "somebody"]));
        assert_eq!(spans(&rs, "Hi SOMEBODY and some"), vec![(3, 11, Kind::Word), (16, 20, Kind::Word)]);
    }

    #[test]
    fn builtin_patterns() {
        let mut rs = RuleSet::new();
        rs.push(Patterns::builtin());
        assert_eq!(matched(&rs, "mail me@example.org now"), vec!["me@example.org"]);
        assert_eq!(matched(&rs, "host 10.1.2.3 and 127.0.0.1 up"), vec!["10.1.2.3"]);
        assert_eq!(matched(&rs, "at 12:30:45 today"), Vec::<&str>::new());
        assert_eq!(matched(&rs, "addr fe80::1ff:fe23:4567:890a/64"), vec!["fe80::1ff:fe23:4567:890a"]);
        assert_eq!(matched(&rs, "lo ::1 up"), Vec::<&str>::new());
        assert_eq!(spans(&rs, "mac 3c:22:fb:aa:bb:cc"), vec![(4, 21, Kind::Hex)]);
        assert_eq!(
            matched(&rs, "token ghp_abcdefghijklmnopqrstuvwxyz0123456789ABCD end"),
            vec!["abcdefghijklmnopqrstuvwxyz0123456789ABCD"]
        );
        assert_eq!(matched(&rs, "AKIAIOSFODNN7EXAMPLE"), vec!["IOSFODNN7EXAMPLE"]);
        assert_eq!(matched(&rs, "Authorization: Bearer abcdef0123456789xyz"), vec!["abcdef0123456789xyz"]);
    }

    #[test]
    fn literals_win_over_overlapping_patterns() {
        let mut rs = RuleSet::new();
        rs.push(Literals::new(["somebody@example.org"]));
        rs.push(Patterns::builtin());
        assert_eq!(matched(&rs, "from somebody@example.org"), vec!["somebody@example.org"]);
    }
}
