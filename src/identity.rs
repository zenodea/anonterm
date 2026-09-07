use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const MIN_LEN: usize = 3;

pub fn from_environment() -> Vec<String> {
    let mut set = BTreeSet::new();
    let mut add = |s: &str| {
        let s = s.trim();
        if s.chars().count() >= MIN_LEN && !s.contains('*') {
            set.insert(s.to_string());
        }
    };

    for var in ["USER", "LOGNAME"] {
        if let Ok(v) = std::env::var(var) {
            add(&v);
        }
    }

    if let Ok(h) = nix::unistd::gethostname() {
        let h = h.to_string_lossy();
        add(&h);
        if let Some(short) = h.split('.').next() {
            add(short);
        }
    }

    if let Some(home) = std::env::var_os("HOME") {
        if let Some(last) = Path::new(&home).file_name() {
            add(&last.to_string_lossy());
        }
    }

    for key in ["user.name", "user.email"] {
        for v in git_config(key) {
            add(&v);
            if key == "user.name" {
                for part in v.split_whitespace() {
                    if part.chars().count() >= 4 {
                        add(part);
                    }
                }
            }
            if let Some((local, _)) = v.split_once('@') {
                add(local);
            }
        }
    }

    for v in ssh_config_values() {
        add(&v);
    }

    set.into_iter().collect()
}

fn git_config(key: &str) -> Vec<String> {
    Command::new("git")
        .args(["config", "--global", "--get-all", key])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn ssh_config_values() -> Vec<String> {
    let Some(home) = std::env::var_os("HOME") else { return Vec::new() };
    let path: PathBuf = [home.as_os_str(), ".ssh".as_ref(), "config".as_ref()].iter().collect();
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    let mut out = Vec::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let Some(key) = it.next() else { continue };
        if key.eq_ignore_ascii_case("host")
            || key.eq_ignore_ascii_case("hostname")
            || key.eq_ignore_ascii_case("user")
        {
            out.extend(it.map(str::to_string));
        }
    }
    out
}
