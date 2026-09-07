use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub literals: Vec<String>,
    pub patterns: Vec<String>,
    pub builtin: Option<bool>,
}

pub fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")))?;
    Some(base.join("anonterm").join("config.toml"))
}

pub fn load(path: Option<&Path>) -> Result<Config, String> {
    let (path, required) = match path {
        Some(p) => (p.to_path_buf(), true),
        None => match default_path() {
            Some(p) => (p, false),
            None => return Ok(Config::default()),
        },
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) if !required => return Ok(Config::default()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_fields() {
        let c: Config = toml::from_str(
            r#"
            literals = ["acme", "project-x"]
            patterns = ['\bPROJ-\d+\b']
            builtin = false
            "#,
        )
        .unwrap();
        assert_eq!(c.literals, vec!["acme", "project-x"]);
        assert_eq!(c.patterns, vec![r"\bPROJ-\d+\b"]);
        assert_eq!(c.builtin, Some(false));
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(toml::from_str::<Config>("literal = []").is_err());
    }
}
