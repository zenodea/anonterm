mod pty;

use std::process;

const USAGE: &str = "usage: anonterm [-h] [-- command [args...]]

Runs a shell (or the given command) inside a pseudo-terminal and rewrites
personally identifying information in its output before it reaches your
terminal. Keystrokes are forwarded untouched.";

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return;
    }
    if args.first().is_some_and(|a| a == "--") {
        args.remove(0);
    }
    if args.is_empty() {
        args.push(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()));
    }

    let mut filter = pty::Passthrough;
    match pty::run(&args, &mut filter) {
        Ok(code) => process::exit(code),
        Err(e) => {
            eprintln!("anonterm: {e}");
            process::exit(1);
        }
    }
}
