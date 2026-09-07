use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
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
                out.push(Span { start: m.start(), end: m.end() });
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

    #[test]
    fn literal_case_insensitive_longest() {
        let mut rs = RuleSet::new();
        rs.push(Literals::new(["some", "somebody"]));
        let spans = rs.find("Hi SOMEBODY and some");
        assert_eq!(spans, vec![Span { start: 3, end: 11 }, Span { start: 16, end: 20 }]);
    }
}
