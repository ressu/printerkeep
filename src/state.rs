//! The state, `.printerkeep-state.json` in the config folder (C3). printerkeep writes it. Each backup
//! includes it, so the printer's branch and a restore have it.
//!
//! For each followed file, the state has the **base**: the shared commit with the version of the file that
//! this printer took last. A file that needs decision also has the shared commit that caused it.

use std::collections::BTreeMap;
use std::path::Path;

pub const STATE_FILE: &str = ".printerkeep-state.json";

const VERSION: u64 = 1;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct State {
    /// By path in the config folder.
    pub files: BTreeMap<String, FileState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileState {
    /// The shared commit with the version of the file that this printer took last.
    pub base: String,
    /// The shared commit whose changes need decision. When the user decides, it becomes the base.
    pub needs_decision: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error(
        "Can't read the state file {file}: {message}.\nTo start again, delete the file. \
         printerkeep then takes the current shared version of each followed file as the starting point."
    )]
    Syntax { file: String, message: String },
    #[error("Can't read {file}: {source}")]
    Read {
        file: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Can't write {file}: {source}")]
    Write {
        file: String,
        #[source]
        source: std::io::Error,
    },
}

/// Reads the state. A missing file gives an empty state.
pub fn load(path: &Path) -> Result<State, StateError> {
    let file = path.display().to_string();
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text).map_err(|message| StateError::Syntax { file, message }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
        Err(source) => Err(StateError::Read { file, source }),
    }
}

/// Writes the state, in one step. An empty state removes the file. Writes nothing if the file has the
/// same content, so that no backup is made without a change.
pub fn save(path: &Path, state: &State) -> Result<(), StateError> {
    let write_error = |source| StateError::Write {
        file: path.display().to_string(),
        source,
    };
    if state.files.is_empty() {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(write_error(e)),
            _ => Ok(()),
        };
    }
    let text = to_json(state);
    if std::fs::read_to_string(path).is_ok_and(|old| old == text) {
        return Ok(());
    }
    crate::files::replace(path, text.as_bytes(), None).map_err(write_error)
}

fn to_json(state: &State) -> String {
    let mut text = format!("{{\n  \"version\": {VERSION},\n  \"files\": {{");
    for (index, (path, file)) in state.files.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push_str(&format!(
            "\n    {}: {{\n      \"base\": {}",
            quote(path),
            quote(&file.base)
        ));
        if let Some(commit) = &file.needs_decision {
            text.push_str(&format!(",\n      \"needs_decision\": {}", quote(commit)));
        }
        text.push_str("\n    }");
    }
    if !state.files.is_empty() {
        text.push_str("\n  ");
    }
    text.push_str("}\n}\n");
    text
}

fn quote(text: &str) -> String {
    let mut out = String::from('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn parse(text: &str) -> Result<State, String> {
    let mut parser = Parser {
        text: text.as_bytes(),
        at: 0,
    };
    let value = parser.value()?;
    parser.space();
    if parser.at != parser.text.len() {
        return Err("text after the end".into());
    }
    let Json::Object(top) = value else {
        return Err("not an object".into());
    };
    match top.get("version") {
        Some(Json::Number(VERSION)) => {}
        Some(Json::Number(other)) => {
            return Err(format!(
                "version {other} is from a newer printerkeep. Update printerkeep"
            ));
        }
        _ => return Err("no version".into()),
    }
    let mut state = State::default();
    let Some(Json::Object(files)) = top.get("files") else {
        return Err("no \"files\"".into());
    };
    for (path, value) in files {
        let Json::Object(fields) = value else {
            return Err(format!("{path}: not an object"));
        };
        let string = |name: &str| match fields.get(name) {
            Some(Json::String(s)) => Ok(Some(s.clone())),
            None => Ok(None),
            Some(_) => Err(format!("{path}: \"{name}\" is not a string")),
        };
        let base = string("base")?.ok_or_else(|| format!("{path}: no \"base\""))?;
        state.files.insert(
            path.clone(),
            FileState {
                base,
                needs_decision: string("needs_decision")?,
            },
        );
    }
    Ok(state)
}

/// The JSON that printerkeep writes: objects, strings and whole numbers.
enum Json {
    Object(BTreeMap<String, Json>),
    String(String),
    Number(u64),
}

struct Parser<'a> {
    text: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        while self.text.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        self.space();
        if self.text.get(self.at) == Some(&byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", byte as char, self.at))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.space();
        match self.text.get(self.at) {
            Some(b'{') => self.object(),
            Some(b'"') => self.string().map(Json::String),
            Some(b'0'..=b'9') => {
                let start = self.at;
                while self.text.get(self.at).is_some_and(u8::is_ascii_digit) {
                    self.at += 1;
                }
                std::str::from_utf8(&self.text[start..self.at])
                    .ok()
                    .and_then(|n| n.parse().ok())
                    .map(Json::Number)
                    .ok_or_else(|| format!("bad number at byte {start}"))
            }
            _ => Err(format!("unexpected value at byte {}", self.at)),
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.expect(b'{')?;
        let mut map = BTreeMap::new();
        self.space();
        if self.text.get(self.at) == Some(&b'}') {
            self.at += 1;
            return Ok(Json::Object(map));
        }
        loop {
            self.space();
            let key = self.string()?;
            self.expect(b':')?;
            let value = self.value()?;
            map.insert(key, value);
            self.space();
            match self.text.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(map));
                }
                _ => return Err(format!("expected ',' or '}}' at byte {}", self.at)),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut bytes = Vec::new();
        loop {
            let Some(&b) = self.text.get(self.at) else {
                return Err("a string has no end".into());
            };
            self.at += 1;
            match b {
                b'"' => break,
                b'\\' => {
                    let Some(&escape) = self.text.get(self.at) else {
                        return Err("a string has no end".into());
                    };
                    self.at += 1;
                    match escape {
                        b'"' | b'\\' | b'/' => bytes.push(escape),
                        b'n' => bytes.push(b'\n'),
                        b'r' => bytes.push(b'\r'),
                        b't' => bytes.push(b'\t'),
                        b'b' => bytes.push(8),
                        b'f' => bytes.push(12),
                        b'u' => {
                            let hex = self
                                .text
                                .get(self.at..self.at + 4)
                                .and_then(|h| std::str::from_utf8(h).ok())
                                .and_then(|h| u32::from_str_radix(h, 16).ok())
                                .and_then(char::from_u32)
                                .ok_or_else(|| format!("bad \\u escape at byte {}", self.at))?;
                            self.at += 4;
                            let mut buffer = [0; 4];
                            bytes.extend_from_slice(hex.encode_utf8(&mut buffer).as_bytes());
                        }
                        _ => return Err(format!("bad escape at byte {}", self.at)),
                    }
                }
                _ => bytes.push(b),
            }
        }
        String::from_utf8(bytes).map_err(|_| "a string is not UTF-8".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read_back() {
        let mut state = State::default();
        state.files.insert(
            "macros/macros.cfg".into(),
            FileState {
                base: "a".repeat(40),
                needs_decision: None,
            },
        );
        state.files.insert(
            "KAMP \"Settings\".cfg".into(),
            FileState {
                base: "b".repeat(40),
                needs_decision: Some("c".repeat(40)),
            },
        );
        let text = to_json(&state);
        assert!(text.starts_with("{\n  \"version\": 1,\n  \"files\": {\n"));
        assert_eq!(parse(&text).unwrap(), state);
        assert_eq!(
            parse(&to_json(&State::default())).unwrap(),
            State::default()
        );
    }

    #[test]
    fn newer_version_is_refused() {
        let error = parse("{\"version\": 2, \"files\": {}}").unwrap_err();
        assert!(error.contains("newer printerkeep"), "{error}");
        assert!(parse("{\"files\": {}}").is_err());
        assert!(parse("{\"version\": 1, \"files\": {\"a\": {}}}").is_err());
    }

    #[test]
    fn save_removes_the_file_for_an_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STATE_FILE);
        let mut state = State::default();
        state.files.insert(
            "a.cfg".into(),
            FileState {
                base: "a".repeat(40),
                needs_decision: None,
            },
        );
        save(&path, &state).unwrap();
        assert_eq!(load(&path).unwrap(), state);
        save(&path, &State::default()).unwrap();
        assert!(!path.exists());
        assert_eq!(load(&path).unwrap(), State::default());
    }
}
