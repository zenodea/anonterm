mod config;
mod identity;
mod pseudo;
mod pty;
mod rules;
mod stream;

use std::path::PathBuf;
use std::process;

const USAGE: &str = "usage: anonterm [options] [-- command [args...]]

Runs a shell (or the given command) inside a pseudo-terminal and rewrites
personally identifying information in its output before it reaches your
terminal. Keystrokes are forwarded untouched.

  -c, --config FILE   read FILE instead of ~/.config/anonterm/config.toml
  -l, --literal WORD  also mask WORD (repeatable)
  -p, --pattern RE    also mask matches of regex RE (repeatable)
      --no-builtin    skip environment-derived literals and builtin patterns
  -m, --mask[=CHAR]   replace matches with CHAR (default *) instead of pseudonyms
      --list          print the literals and patterns in effect and exit
  -h, --help          show this help

Config file keys: literals, patterns, builtin = true|false, mask = \"*\".
Set ANONTERM_SEED to make replacements reproducible across runs.";

struct Options {
    config: Option<PathBuf>,
    literals: Vec<String>,
    patterns: Vec<String>,
    builtin: bool,
    mask: Option<char>,
    list: bool,
    command: Vec<String>,
}

fn parse_args() -> Result<Options, String> {
    let mut opts = Options {
        config: None,
        literals: Vec::new(),
        patterns: Vec::new(),
        builtin: true,
        mask: None,
        list: false,
        command: Vec::new(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |flag: &str| args.next().ok_or(format!("{flag} needs a value"));
        match a.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                process::exit(0);
            }
            "-c" | "--config" => opts.config = Some(PathBuf::from(value(&a)?)),
            "-l" | "--literal" => opts.literals.push(value(&a)?),
            "-p" | "--pattern" => opts.patterns.push(value(&a)?),
            "--no-builtin" => opts.builtin = false,
            "-m" | "--mask" => opts.mask = Some('*'),
            s if s.starts_with("--mask=") => opts.mask = Some(single_char(&s[7..])?),
            "--list" => opts.list = true,
            "--" => {
                opts.command = args.collect();
                break;
            }
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            _ => {
                opts.command = std::iter::once(a).chain(args).collect();
                break;
            }
        }
    }
    Ok(opts)
}

fn single_char(s: &str) -> Result<char, String> {
    let mut it = s.chars();
    match (it.next(), it.next()) {
        (Some(c), None) => Ok(c),
        _ => Err(format!("mask must be a single character, got {s:?}")),
    }
}

fn fail(msg: impl std::fmt::Display, code: i32) -> ! {
    eprintln!("anonterm: {msg}");
    process::exit(code)
}

fn main() {
    let mut opts = parse_args().unwrap_or_else(|e| fail(format!("{e}\n\n{USAGE}"), 2));
    let cfg = config::load(opts.config.as_deref()).unwrap_or_else(|e| fail(e, 2));
    let builtin = opts.builtin && cfg.builtin.unwrap_or(true);
    let style = match opts.mask.or_else(|| cfg.mask.as_deref().and_then(|m| single_char(m).ok())) {
        Some(c) => pseudo::Style::Mask(c),
        None => pseudo::Style::Pseudonym,
    };

    let mut literals = if builtin { identity::from_environment() } else { Vec::new() };
    literals.extend(cfg.literals);
    literals.append(&mut opts.literals);
    let mut patterns = cfg.patterns;
    patterns.append(&mut opts.patterns);

    if opts.list {
        for l in &literals {
            println!("literal  {l}");
        }
        for p in &patterns {
            println!("pattern  {p}");
        }
        if builtin {
            println!("builtin  emails, ipv4, ipv6, mac, common secret tokens");
        }
        return;
    }

    let mut rules = rules::RuleSet::new();
    rules.push(rules::Literals::new(&literals));
    let custom = rules::Patterns::from_strings(&patterns).unwrap_or_else(|e| fail(format!("bad pattern: {e}"), 2));
    rules.push(custom);
    if builtin {
        rules.push(rules::Patterns::builtin());
    }

    if opts.command.is_empty() {
        opts.command.push(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()));
    }

    let mut filter = stream::Anonymizer::new(rules, pseudo::Pseudonymizer::from_env(style));
    match pty::run(&opts.command, &mut filter) {
        Ok(code) => process::exit(code),
        Err(e) => fail(e, 1),
    }
}
