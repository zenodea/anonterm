use std::collections::HashMap;

use crate::rules::Kind;

fn fnv1a(seed: u64, s: &str) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64 ^ seed;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

const VOWELS: &[u8] = b"aeiou";
const CONSONANTS: &[u8] = b"bdfgklmnprstvz";
const HEX: &[u8] = b"abcdef";
const ALNUM: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

pub struct Pseudonymizer {
    seed: u64,
    cache: HashMap<String, String>,
}

impl Pseudonymizer {
    pub fn new(seed: u64) -> Self {
        Self { seed, cache: HashMap::new() }
    }

    pub fn from_env() -> Self {
        let seed = std::env::var("ANONTERM_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(random_seed);
        Self::new(seed)
    }

    pub fn replace(&mut self, original: &str, kind: Kind) -> String {
        if let Some(r) = self.cache.get(original) {
            return r.clone();
        }
        let r = self.generate(original, kind);
        self.cache.insert(original.to_string(), r.clone());
        r
    }

    fn generate(&self, original: &str, kind: Kind) -> String {
        let mut rng = fnv1a(self.seed, original);
        let mut out = String::with_capacity(original.len());
        let mut want_vowel = splitmix(&mut rng) & 1 == 0;
        for c in original.chars() {
            let r = splitmix(&mut rng);
            let pick = |set: &[u8]| set[(r % set.len() as u64) as usize] as char;
            if c.is_alphabetic() {
                let letter = match kind {
                    Kind::Hex => pick(HEX),
                    Kind::Token => pick(ALNUM),
                    Kind::Word if want_vowel => pick(VOWELS),
                    Kind::Word => pick(CONSONANTS),
                };
                want_vowel = !want_vowel;
                out.push(if c.is_uppercase() { letter.to_ascii_uppercase() } else { letter });
            } else if c.is_ascii_digit() {
                out.push((b'0' + (r % 10) as u8) as char);
            } else {
                out.push(c);
                want_vowel = true;
            }
        }
        out
    }
}

fn random_seed() -> u64 {
    use std::io::Read;
    let mut b = [0u8; 8];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map(|_| u64::from_le_bytes(b))
        .unwrap_or(0x5eed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_shape() {
        let mut p = Pseudonymizer::new(1);
        let r = p.replace("Some.Body@example.org", Kind::Word);
        assert_eq!(r.chars().count(), "Some.Body@example.org".chars().count());
        assert_eq!(r.as_bytes()[4], b'.');
        assert_eq!(r.as_bytes()[9], b'@');
        assert!(r.chars().next().unwrap().is_ascii_uppercase());
        assert!(r.chars().nth(1).unwrap().is_ascii_lowercase());
        assert_ne!(r, "Some.Body@example.org");
    }

    #[test]
    fn deterministic_and_seed_dependent() {
        let mut a = Pseudonymizer::new(7);
        let mut b = Pseudonymizer::new(7);
        let mut c = Pseudonymizer::new(8);
        assert_eq!(a.replace("somebody", Kind::Word), b.replace("somebody", Kind::Word));
        assert_ne!(a.replace("somebody", Kind::Word), c.replace("somebody", Kind::Word));
    }

    #[test]
    fn digits_stay_digits() {
        let mut p = Pseudonymizer::new(3);
        let r = p.replace("192.168.1.42", Kind::Word);
        assert!(r.chars().all(|c| c.is_ascii_digit() || c == '.'));
        assert_eq!(r.len(), 12);
    }
}
