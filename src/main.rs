mod identity;
mod pseudo;
mod pty;
mod rules;
mod stream;

use std::process;

const USAGE: &str = "usage: anonterm [-h] [--list] [-- command [args...]]

Runs a shell (or the given command) inside a pseudo-terminal and rewrites
personally identifying information in its output before it reaches your
terminal. Keystrokes are forwarded untouched.

  --list   print the literals that would be masked and exit";

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return;
    }

    let literals = identity::from_environment();

    if args.first().is_some_and(|a| a == "--list") {
        for l in &literals {
            println!("{l}");
        }
        return;
    }
    if args.first().is_some_and(|a| a == "--") {
        args.remove(0);
    }
    if args.is_empty() {
        args.push(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()));
    }

    let mut rules = rules::RuleSet::new();
    rules.push(rules::Literals::new(&literals));
    let mut filter = stream::Anonymizer::new(rules, pseudo::Pseudonymizer::from_env());
    match pty::run(&args, &mut filter) {
        Ok(code) => process::exit(code),
        Err(e) => {
            eprintln!("anonterm: {e}");
            process::exit(1);
        }
    }
}
