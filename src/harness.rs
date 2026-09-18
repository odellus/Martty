//! The harness Martty's `settings.json` selects.
//!
//! The npm host resolves this and hands the result to Rust as `--agent CMD
//! --agent-arg ARG` (`npm/lib/boot.js` → `painterArgs`). Running the binary
//! directly skips that layer, and the compiled-in fallback is `dsh-acp` — which
//! a crow-cli install does not have, so the spawn dies with ENOENT and the TUI
//! reports `runtime ACP · session unavailable`. Resolving the harness here too
//! makes the bare binary work with no flags and no environment.
//!
//! Mirrors `selectedHarness` / `validateHarness` in `npm/lib/harnesses.js`:
//! `defaultHarness` names an entry in `harnesses`, falling back to the legacy
//! `activeHarness` only while `defaultHarness` is absent entirely. An explicit
//! null default means "use the bundled fallback", so it selects nothing.
//!
//! ```json
//! {
//!   "harnesses": [
//!     {"id": "crow-cli", "command": "/home/thomas/.local/bin/crow-cli",
//!      "args": ["acp"], "env": {}}
//!   ],
//!   "defaultHarness": "crow-cli"
//! }
//! ```

use std::path::Path;

/// A configured harness: the agent to spawn, its arguments, and extra environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Harness {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl Harness {
    /// Spawn argv, the same shape `--agent` / `--agent-arg` produce.
    pub fn argv(&self) -> Vec<String> {
        let mut argv = vec![self.command.clone()];
        argv.extend(self.args.iter().cloned());
        argv
    }
}

/// The harness `defaultHarness` selects, or `None` when settings are absent,
/// unreadable, or name no configured harness. Malformed entries are skipped
/// rather than fatal: this is a fallback path, and the caller still has `dsh-acp`.
pub fn selected(path: &Path) -> Option<Harness> {
    let text = std::fs::read_to_string(path).ok()?;
    let settings: serde_json::Value = serde_json::from_str(&text).ok()?;
    let id = default_id(&settings)?;
    let entry = settings
        .get("harnesses")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("id").and_then(|v| v.as_str()) == Some(id))?;
    harness(entry)
}

/// `persistedDefaultId`: `defaultHarness` when it is a string, the legacy
/// `activeHarness` only while `defaultHarness` is absent, otherwise nothing.
fn default_id(settings: &serde_json::Value) -> Option<&str> {
    match settings.get("defaultHarness") {
        Some(value) => value.as_str(),
        None => settings.get("activeHarness").and_then(|v| v.as_str()),
    }
}

fn harness(entry: &serde_json::Value) -> Option<Harness> {
    let command = entry.get("command").and_then(|v| v.as_str())?.trim();
    if command.is_empty() {
        return None;
    }
    let args = strings(entry.get("args"));
    Some(Harness {
        // npx re-resolves the package per spawn; the host drops these so a
        // configured harness starts offline-clean.
        args: if is_npx(command) {
            args.into_iter()
                .filter(|arg| arg != "--yes" && arg != "--prefer-offline")
                .collect()
        } else {
            args
        },
        command: command.to_owned(),
        env: pairs(entry.get("env")),
    })
}

fn is_npx(command: &str) -> bool {
    let base = Path::new(command)
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    base == "npx" || base == "npx.cmd" || base == "npx.bat" || base == "npx.exe"
}

fn strings(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn pairs(value: Option<&serde_json::Value>) -> Vec<(String, String)> {
    value
        .and_then(|v| v.as_object())
        .map(|map| {
            map.iter()
                .filter(|(key, _)| !key.is_empty())
                .filter_map(|(key, item)| Some((key.clone(), item.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("martty-harness-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    const CROW: &str = r#"{"harnesses":[{"id":"crow-cli","label":"crow-cli","command":"/home/thomas/.local/bin/crow-cli","args":["acp"],"env":{}}],"defaultHarness":"crow-cli","theme":"iceberg"}"#;

    #[test]
    fn default_harness_selects_its_entry() {
        let path = tmp("selected.json");
        std::fs::write(&path, CROW).unwrap();
        let harness = selected(&path).expect("crow-cli is the default harness");
        assert_eq!(
            harness.argv(),
            vec![
                "/home/thomas/.local/bin/crow-cli".to_owned(),
                "acp".to_owned()
            ]
        );
        assert!(harness.env.is_empty());
    }

    #[test]
    fn env_and_args_survive_the_round_trip() {
        let path = tmp("env.json");
        std::fs::write(
            &path,
            r#"{"harnesses":[{"id":"h","command":"/bin/h","args":["--one","two"],"env":{"KEY":"value","EMPTY":""}}],"defaultHarness":"h"}"#,
        )
        .unwrap();
        let harness = selected(&path).unwrap();
        assert_eq!(harness.args, vec!["--one".to_owned(), "two".to_owned()]);
        assert_eq!(
            harness.env,
            vec![
                ("KEY".to_owned(), "value".to_owned()),
                ("EMPTY".to_owned(), String::new())
            ]
        );
    }

    #[test]
    fn legacy_active_harness_is_honored_only_while_default_is_absent() {
        let path = tmp("legacy.json");
        std::fs::write(
            &path,
            r#"{"harnesses":[{"id":"h","command":"/bin/h"}],"activeHarness":"h"}"#,
        )
        .unwrap();
        assert_eq!(
            selected(&path).map(|h| h.command).as_deref(),
            Some("/bin/h")
        );
        // An explicit null default means "use the bundled fallback".
        std::fs::write(
            &path,
            r#"{"harnesses":[{"id":"h","command":"/bin/h"}],"defaultHarness":null,"activeHarness":"h"}"#,
        )
        .unwrap();
        assert!(selected(&path).is_none());
    }

    #[test]
    fn unselectable_settings_fall_through_to_the_bundled_agent() {
        let path = tmp("unselectable.json");
        for settings in [
            "{}",                                                                       // no default
            r#"{"defaultHarness":"nope","harnesses":[{"id":"h","command":"/bin/h"}]}"#, // unknown id
            r#"{"defaultHarness":"h","harnesses":[{"id":"h","command":"  "}]}"#, // blank command
            r#"{"defaultHarness":"h"}"#,                                         // no harnesses
            "not json",
        ] {
            std::fs::write(&path, settings).unwrap();
            assert!(selected(&path).is_none(), "must not select: {settings}");
        }
        assert!(selected(&tmp("absent.json")).is_none());
    }

    #[test]
    fn npx_harnesses_lose_the_reinstall_flags() {
        let path = tmp("npx.json");
        std::fs::write(
            &path,
            r#"{"harnesses":[{"id":"h","command":"npx","args":["--yes","-p","@x/agent","--prefer-offline","acp"]}],"defaultHarness":"h"}"#,
        )
        .unwrap();
        assert_eq!(
            selected(&path).unwrap().args,
            vec!["-p".to_owned(), "@x/agent".to_owned(), "acp".to_owned()]
        );
    }
}
