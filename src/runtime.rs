//! Shared launch/display configuration for the ACP client and demo painter.

use std::path::{Path, PathBuf};

pub fn martty_home_from(
    martty_home: Option<&str>,
    dsh_home: Option<&str>,
    user_home: &str,
) -> PathBuf {
    if let Some(home) = martty_home.filter(|value| !value.is_empty()) {
        return PathBuf::from(home);
    }
    if let Some(home) = dsh_home.filter(|value| !value.is_empty()) {
        return Path::new(home).join(".martty");
    }
    Path::new(user_home).join(".martty")
}

pub fn martty_home() -> PathBuf {
    let martty = std::env::var("MARTTY_HOME").ok();
    let dsh = std::env::var("DSH_HOME").ok();
    let user = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    martty_home_from(martty.as_deref(), dsh.as_deref(), &user)
}

pub fn default_session_root() -> PathBuf {
    martty_home().join("sessions")
}

pub fn settings_path(session_root: &str) -> PathBuf {
    if Path::new(session_root) == default_session_root() {
        martty_home().join("settings.json")
    } else {
        Path::new(session_root).join("settings.json")
    }
}

pub fn legacy_settings_path(session_root: &str) -> PathBuf {
    if Path::new(session_root) == default_session_root() {
        let user = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        Path::new(&user)
            .join(".dsh-tui")
            .join("sessions")
            .join("dsh-tui-settings.json")
    } else {
        Path::new(session_root).join("dsh-tui-settings.json")
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub bin: String,
    pub cordis: String,
    pub workspace: String,
    pub session_root: String,
    pub provider: String,
    pub model: String,
    pub max_tokens: Option<u64>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

impl RuntimeConfig {
    /// Environment for the child process, mirroring the SDK's injection.
    /// Credentials fall back to the local dsh install's store (~/.dsh), so a
    /// machine with a configured dsh needs no exported DEEPSEEK_API_KEY.
    pub fn child_env(&self) -> Vec<(String, String)> {
        let mut env = vec![
            ("DSH_CORDIS_CONFIG".into(), self.cordis.clone()),
            ("DSH_SESSION_ROOT".into(), self.session_root.clone()),
            ("DSH_CWD".into(), self.workspace.clone()),
        ];
        if let Some(url) = &self.base_url {
            env.push(("DEEPSEEK_BASE_URL".into(), url.clone()));
        }
        if let Some(key) = &self.api_key {
            env.push(("DEEPSEEK_API_KEY".into(), key.clone()));
        } else if std::env::var("DEEPSEEK_API_KEY").is_err() {
            let local = local_dsh();
            if let Some(key) = local.api_key {
                env.push(("DEEPSEEK_API_KEY".into(), key));
            }
            if self.base_url.is_none() && std::env::var("DEEPSEEK_BASE_URL").is_err() {
                if let Some(url) = local.base_url {
                    env.push(("DEEPSEEK_BASE_URL".into(), url));
                }
            }
        }
        // A harness configured in settings.json may carry its own environment.
        // Apply it when that harness is the agent being spawned, and let it win
        // over the defaults above.
        if let Some(harness) = crate::harness::selected(&settings_path(&self.session_root)) {
            if harness.argv().join(" ") == self.bin {
                env.extend(harness.env);
            }
        }
        env
    }

    pub fn has_credentials(&self) -> bool {
        self.api_key.is_some()
            || std::env::var("DEEPSEEK_API_KEY").is_ok()
            || local_dsh().api_key.is_some()
    }

    /// Spawn argv for the ACP agent (and Terminal Auth). `demo` falls back
    /// to `dsh-acp` so `/auth` still has a command to run.
    pub fn agent_argv(&self) -> Vec<String> {
        if self.bin.is_empty() || self.bin == "demo" {
            return vec!["dsh-acp".into()];
        }
        self.bin.split_whitespace().map(str::to_string).collect()
    }

    /// Human description of where the API key comes from.
    pub fn credential_source(&self) -> Option<&'static str> {
        if self.api_key.is_some() {
            Some("--api-key flag")
        } else if std::env::var("DEEPSEEK_API_KEY").is_ok() {
            Some("environment")
        } else if local_dsh().api_key.is_some() {
            Some("local dsh (~/.dsh)")
        } else {
            None
        }
    }
}

/// Facts borrowed from a local `dsh` installation.
#[derive(Default, Clone, Debug)]
pub struct LocalDsh {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

/// Read the local dsh credential store and settings (best effort).
pub fn local_dsh() -> LocalDsh {
    let Ok(home) = std::env::var("HOME") else {
        return LocalDsh::default();
    };
    let root = Path::new(&home).join(".dsh");
    let mut out = LocalDsh::default();
    if let Ok(creds) = std::fs::read_to_string(root.join(".credentials.yaml")) {
        out.api_key = yaml_top_level_env(&creds, "DEEPSEEK_API_KEY");
        out.base_url = yaml_top_level_env(&creds, "DEEPSEEK_BASE_URL");
    }
    if let Ok(settings) = std::fs::read_to_string(root.join("settings.yaml")) {
        let (provider, model) = yaml_agent_default_model(&settings);
        out.provider = provider;
        out.model = model;
    }
    out
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    let v = v
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .unwrap_or(v);
    let v = v
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(v);
    v.to_string()
}

/// `KEY: value` at zero indentation where KEY is an env-style name.
fn yaml_top_level_env(yaml: &str, key: &str) -> Option<String> {
    for line in yaml.lines() {
        if line.starts_with([' ', '\t', '#']) {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        if k.trim() == key {
            let v = unquote(v);
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// provider/model under the `agent-default-model:` block.
fn yaml_agent_default_model(yaml: &str) -> (Option<String>, Option<String>) {
    let mut in_block = false;
    let mut provider = None;
    let mut model = None;
    for line in yaml.lines() {
        if !line.starts_with([' ', '\t']) {
            in_block = line.trim_end() == "agent-default-model:";
            continue;
        }
        if !in_block {
            continue;
        }
        let Some((k, v)) = line.trim().split_once(':') else {
            continue;
        };
        let v = unquote(v);
        if v.is_empty() {
            continue;
        }
        match k.trim() {
            "provider" => provider = Some(v),
            "model" => model = Some(v),
            _ => {}
        }
    }
    (provider, model)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(session_root: &str, bin: &str) -> RuntimeConfig {
        RuntimeConfig {
            bin: bin.into(),
            cordis: "acp".into(),
            workspace: "/tmp".into(),
            session_root: session_root.into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        }
    }

    fn root(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("martty-child-env-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn harness_env_applies_only_to_the_configured_agent() {
        let dir = root("harness");
        std::fs::write(
            dir.join("settings.json"),
            r#"{"harnesses":[{"id":"h","command":"/bin/agent","args":["acp"],"env":{"HARNESS_KEY":"yes"}}],"defaultHarness":"h"}"#,
        )
        .unwrap();
        let session_root = dir.to_string_lossy().into_owned();

        let matching = cfg(&session_root, "/bin/agent acp");
        assert!(
            matching
                .child_env()
                .contains(&("HARNESS_KEY".to_owned(), "yes".to_owned())),
            "the configured harness must supply its own environment"
        );

        let other = cfg(&session_root, "/bin/other-agent acp");
        assert!(
            !other
                .child_env()
                .iter()
                .any(|(key, _)| key == "HARNESS_KEY"),
            "an agent that is not the configured harness must not inherit its env"
        );
    }
}
