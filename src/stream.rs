use crate::pseudo::Pseudonymizer;
use crate::pty::Filter;
use crate::rules::RuleSet;

const MAX_SEQ: usize = 4096;
const MAX_TEXT: usize = 16 * 1024;

enum Seg {
    Text(Vec<u8>),
    Raw(Vec<u8>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Esc,
    EscInter,
    Csi,
    Str,
    StrEsc,
}

pub struct Anonymizer {
    rules: RuleSet,
    pseudo: Pseudonymizer,
    segs: Vec<Seg>,
    text_len: usize,
    state: State,
    seq: Vec<u8>,
}

impl Anonymizer {
    pub fn new(rules: RuleSet, pseudo: Pseudonymizer) -> Self {
        Self { rules, pseudo, segs: Vec::new(), text_len: 0, state: State::Ground, seq: Vec::new() }
    }

    fn push_text(&mut self, b: u8, out: &mut Vec<u8>) {
        match self.segs.last_mut() {
            Some(Seg::Text(t)) => t.push(b),
            _ => self.segs.push(Seg::Text(vec![b])),
        }
        self.text_len += 1;
        if self.text_len >= MAX_TEXT {
            self.flush_into(out);
        }
    }

    fn push_raw(&mut self, bytes: &[u8], breaks: bool, out: &mut Vec<u8>) {
        if self.text_len == 0 {
            self.drain_raw(out);
            out.extend_from_slice(bytes);
            return;
        }
        self.segs.push(Seg::Raw(bytes.to_vec()));
        if breaks {
            self.flush_into(out);
        }
    }

    fn drain_raw(&mut self, out: &mut Vec<u8>) {
        for s in self.segs.drain(..) {
            if let Seg::Raw(r) = s {
                out.extend_from_slice(&r);
            }
        }
    }

    fn finish_seq(&mut self, breaks: bool, out: &mut Vec<u8>) {
        let seq = std::mem::take(&mut self.seq);
        self.state = State::Ground;
        self.push_raw(&seq, breaks, out);
    }

    fn flush_into(&mut self, out: &mut Vec<u8>) {
        if self.text_len == 0 {
            self.drain_raw(out);
            return;
        }
        self.text_len = 0;
        let segs = std::mem::take(&mut self.segs);

        let mut joined = Vec::new();
        for s in &segs {
            if let Seg::Text(t) = s {
                joined.extend_from_slice(t);
            }
        }
        let replaced = std::str::from_utf8(&joined).ok().and_then(|text| {
            let spans = self.rules.find(text);
            if spans.is_empty() {
                return None;
            }
            let mut r = String::with_capacity(text.len());
            let mut pos = 0;
            for sp in spans {
                r.push_str(&text[pos..sp.start]);
                r.push_str(&self.pseudo.replace(&text[sp.start..sp.end], sp.kind));
                pos = sp.end;
            }
            r.push_str(&text[pos..]);
            Some(r)
        });

        let Some(replaced) = replaced else {
            for s in segs {
                match s {
                    Seg::Text(t) | Seg::Raw(t) => out.extend_from_slice(&t),
                }
            }
            return;
        };

        let mut chars = replaced.chars();
        for s in segs {
            match s {
                Seg::Raw(r) => out.extend_from_slice(&r),
                Seg::Text(t) => {
                    let n = t.iter().filter(|&&b| b & 0xC0 != 0x80).count();
                    let mut buf = [0u8; 4];
                    for c in chars.by_ref().take(n) {
                        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    }
                }
            }
        }
    }
}

fn c0_breaks(b: u8) -> bool {
    !matches!(b, 0x00 | 0x07 | 0x0e | 0x0f | 0x7f)
}

fn csi_breaks(seq: &[u8]) -> bool {
    let body = &seq[2..];
    let Some(&fin) = body.last() else { return true };
    match fin {
        b'm' | b'q' | b'n' | b'c' | b't' | b'p' => false,
        b'h' | b'l' if body.first() == Some(&b'?') => {
            const COSMETIC: [&str; 15] = [
                "25", "12", "2004", "1004", "1000", "1002", "1003", "1005", "1006", "1015", "1016",
                "2026", "2027", "2031", "2048",
            ];
            let params = std::str::from_utf8(&body[1..body.len() - 1]).unwrap_or("");
            !params.split(';').all(|p| COSMETIC.contains(&p))
        }
        _ => true,
    }
}

fn esc_breaks(fin: u8) -> bool {
    matches!(fin, b'7' | b'8' | b'D' | b'E' | b'H' | b'M' | b'c')
}

impl Filter for Anonymizer {
    fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) {
        for &b in input {
            if self.state != State::Ground {
                self.seq.push(b);
                if self.seq.len() > MAX_SEQ {
                    self.finish_seq(false, out);
                    continue;
                }
            }
            match self.state {
                State::Ground => {
                    if b == 0x1b {
                        self.seq.push(b);
                        self.state = State::Esc;
                    } else if b < 0x20 || b == 0x7f {
                        self.push_raw(&[b], c0_breaks(b), out);
                    } else {
                        self.push_text(b, out);
                    }
                }
                State::Esc => match b {
                    b'[' => self.state = State::Csi,
                    b']' | b'P' | b'X' | b'^' | b'_' => self.state = State::Str,
                    0x20..=0x2f => self.state = State::EscInter,
                    0x30..=0x7e => self.finish_seq(esc_breaks(b), out),
                    0x1b => {
                        self.seq.pop();
                        let stray = std::mem::replace(&mut self.seq, vec![0x1b]);
                        self.push_raw(&stray, false, out);
                    }
                    _ => self.finish_seq(c0_breaks(b), out),
                },
                State::EscInter => match b {
                    0x20..=0x2f => {}
                    _ => self.finish_seq(false, out),
                },
                State::Csi => match b {
                    0x40..=0x7e => {
                        let breaks = csi_breaks(&self.seq);
                        self.finish_seq(breaks, out);
                    }
                    0x20..=0x3f => {}
                    _ => self.finish_seq(true, out),
                },
                State::Str => match b {
                    0x07 => self.finish_seq(false, out),
                    0x1b => self.state = State::StrEsc,
                    _ => {}
                },
                State::StrEsc => match b {
                    b'\\' => self.finish_seq(false, out),
                    0x1b => {}
                    _ => self.state = State::Str,
                },
            }
        }
    }

    fn flush(&mut self, out: &mut Vec<u8>) {
        self.flush_into(out);
    }

    fn pending(&self) -> bool {
        !self.segs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{Kind, Literals};

    fn filter(words: &[&str]) -> Anonymizer {
        let mut rules = RuleSet::new();
        rules.push(Literals::new(words.iter().copied()));
        Anonymizer::new(rules, Pseudonymizer::new(42))
    }

    fn run(f: &mut Anonymizer, chunks: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for c in chunks {
            f.feed(c, &mut out);
        }
        f.flush(&mut out);
        out
    }

    fn fake(word: &str) -> String {
        Pseudonymizer::new(42).replace(word, Kind::Word)
    }

    #[test]
    fn replaces_in_plain_line() {
        let mut f = filter(&["somebody"]);
        let out = run(&mut f, &[b"hello somebody!\r\n"]);
        assert_eq!(out, format!("hello {}!\r\n", fake("somebody")).as_bytes());
    }

    #[test]
    fn matches_across_sgr_and_chunks() {
        let mut f = filter(&["somebody"]);
        let out = run(&mut f, &[b"some\x1b[0m", b"body\n"]);
        let fake = fake("somebody");
        assert_eq!(out, format!("{}\x1b[0m{}\n", &fake[..4], &fake[4..]).as_bytes());
    }

    #[test]
    fn cursor_movement_breaks_a_run() {
        let mut f = filter(&["somebody"]);
        let input: &[u8] = b"some\x1b[5Cbody\n";
        assert_eq!(run(&mut f, &[input]), input);
    }

    #[test]
    fn untouched_stream_is_byte_identical() {
        let mut f = filter(&["nobody"]);
        let input: &[u8] = b"\x1b[?1049h\x1b[H\x1b[2J\x1b]0;title\x07abc\x1b[K\r\n\x1bP$qm\x1b\\\x1b(B\x1b[1 qxyz\x1b[?25l";
        assert_eq!(run(&mut f, &[input]), input);
    }

    #[test]
    fn utf8_neighbours_survive() {
        let mut f = filter(&["somebody"]);
        let out = run(&mut f, &["ünï somebody ö\n".as_bytes()]);
        assert_eq!(out, format!("ünï {} ö\n", fake("somebody")).as_bytes());
    }
}
