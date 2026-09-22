//! MCP servers handed to the agent at `session/new` and `session/load`.
//!
//! In ACP the **client owns tool supply**: an agent starts with exactly the servers
//! the client passes and nothing else. A client that sends none gets an agent that
//! can only talk — crow-cli logs "runs toolless until a load/new_session provides
//! them" and will print a hallucinated tool call as prose. Martty is DSH-first and
//! DSH owns its tools through the Cordis tree, so nothing here was ever wired up;
//! against a bare ACP agent that silence meant a mute agent.
//!
//! Servers are read, in precedence order, from:
//!   1. `mcpServers` in Martty's own `settings.json` (same dict shape as below),
//!   2. `mcpServers` in crow-cli's config (`~/.agents/crow/config.yaml`) — the
//!      crow-cli.tui / crow-term passthrough convention, so pointing Martty at
//!      `crow-cli acp` supplies crow's tools with zero configuration,
//!   3. nothing — a toolless agent is a legitimate (if dull) configuration.
//!
//! These are passed through untouched: Martty never connects to them itself. The
//! crow config is read tolerantly — crow-cli parses it with PyYAML, which accepts
//! duplicate keys, so Martty must not reject a document crow is happy with.
//!
//! ```yaml
//! mcpServers:
//!   crow-mcp:
//!     transport: http
//!     url: "http://localhost:2769/mcp"
//!   playwright:
//!     transport: stdio
//!     command: npx
//!     args: ["-y", "@playwright/mcp"]
//!     env: {FOO: bar}
//! ```

use std::path::{Path, PathBuf};

use agent_client_protocol::schema::v1::{
    EnvVariable, HttpHeader, McpServer as WireMcpServer, McpServerHttp, McpServerSse,
    McpServerStdio,
};

pub const CROW_CONFIG: &str = "~/.agents/crow/config.yaml";

#[derive(Debug, Clone)]
pub enum McpServer {
    Stdio {
        name: String,
        command: PathBuf,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
    Http {
        name: String,
        url: String,
        headers: Vec<(String, String)>,
    },
    Sse {
        name: String,
        url: String,
        headers: Vec<(String, String)>,
    },
}

impl McpServer {
    fn into_wire(self) -> WireMcpServer {
        match self {
            Self::Stdio {
                name,
                command,
                args,
                env,
            } => WireMcpServer::Stdio(
                McpServerStdio::new(name, command).args(args).env(
                    env.into_iter()
                        .map(|(name, value)| EnvVariable::new(name, value))
                        .collect(),
                ),
            ),
            Self::Http { name, url, headers } => {
                WireMcpServer::Http(McpServerHttp::new(name, url).headers(wire_headers(headers)))
            }
            Self::Sse { name, url, headers } => {
                WireMcpServer::Sse(McpServerSse::new(name, url).headers(wire_headers(headers)))
            }
        }
    }
}

fn wire_headers(pairs: Vec<(String, String)>) -> Vec<HttpHeader> {
    pairs
        .into_iter()
        .map(|(name, value)| HttpHeader::new(name, value))
        .collect()
}

/// The tool belt for the next `session/new` / `session/load`, in wire shape.
pub fn wire_servers() -> Vec<WireMcpServer> {
    load().into_iter().map(McpServer::into_wire).collect()
}

/// The v2 spelling of the same supply.
///
/// v1 and v2 `McpServer` are field-identical — stdio is name/command/args/env,
/// http and sse are name/url/headers — so the wire JSON is the wire JSON and
/// the conversion is a re-read of it. A schema that stops being identical
/// should fail here, loudly, rather than hand the agent a toolless session.
pub fn wire_servers_v2() -> Vec<agent_client_protocol::schema::v2::McpServer> {
    let value = serde_json::to_value(wire_servers()).expect("mcp supply serializes");
    serde_json::from_value(value).expect("v2 McpServer matches the v1 wire shape")
}

fn load() -> Vec<McpServer> {
    let settings =
        crate::runtime::settings_path(&crate::runtime::default_session_root().to_string_lossy());
    if let Some(servers) = from_settings_json(&settings) {
        return servers;
    }
    from_crow_config(&expand_home(CROW_CONFIG)).unwrap_or_default()
}

/// `mcpServers` out of Martty's settings.json. `None` = key absent (fall through to
/// the crow config); `Some(vec)` = key present, even when it declares no servers.
fn from_settings_json(path: &Path) -> Option<Vec<McpServer>> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let entries = value.get("mcpServers")?.as_object()?;
    let mut servers = Vec::new();
    for (name, entry) in entries {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let str_field = |key: &str| entry.get(key).and_then(|v| v.as_str()).map(str::to_owned);
        let pairs = |key: &str| -> Vec<(String, String)> {
            entry
                .get(key)
                .and_then(|v| v.as_object())
                .map(|map| {
                    map.iter()
                        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                        .collect()
                })
                .unwrap_or_default()
        };
        let strings = |key: &str| -> Vec<String> {
            entry
                .get(key)
                .and_then(|v| v.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };
        let transport = str_field("transport");
        let url = str_field("url");
        match (transport.as_deref(), url) {
            (Some("sse"), Some(url)) => servers.push(McpServer::Sse {
                name: name.clone(),
                url,
                headers: pairs("headers"),
            }),
            (_, Some(url)) => servers.push(McpServer::Http {
                name: name.clone(),
                url,
                headers: pairs("headers"),
            }),
            _ => {
                let Some(command) = str_field("command") else {
                    eprintln!(
                        "warning: skipping mcpServers.{name} — no url (http/sse) or command (stdio)"
                    );
                    continue;
                };
                servers.push(McpServer::Stdio {
                    name: name.clone(),
                    command: PathBuf::from(command),
                    args: strings("args"),
                    env: pairs("env"),
                });
            }
        }
    }
    Some(servers)
}

/// `mcpServers` out of crow-cli's config.yaml — the crow-cli.tui passthrough shape.
fn from_crow_config(path: &Path) -> Option<Vec<McpServer>> {
    if !path.exists() {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let node = mcp_servers_node(&text)?;
    let entries = node.as_hash()?;
    let mut servers = Vec::new();
    for (name, entry) in entries {
        let Some(name) = name.as_str() else { continue };
        let transport = entry["transport"].as_str();
        let url = entry["url"].as_str().map(str::to_owned);
        let server = match (transport, url) {
            (Some("sse"), Some(url)) => McpServer::Sse {
                name: name.to_owned(),
                url,
                headers: yaml_pairs(&entry["headers"]),
            },
            (_, Some(url)) => McpServer::Http {
                name: name.to_owned(),
                url,
                headers: yaml_pairs(&entry["headers"]),
            },
            _ => {
                let command = entry["command"].as_str()?;
                McpServer::Stdio {
                    name: name.to_owned(),
                    command: PathBuf::from(command),
                    args: yaml_strings(&entry["args"]),
                    env: yaml_pairs(&entry["env"]),
                }
            }
        };
        servers.push(server);
    }
    Some(servers)
}

/// The `mcpServers` node of a crow config document.
///
/// crow-cli reads this file with PyYAML, which accepts duplicate keys (last one
/// wins); yaml-rust2 rejects the entire document over them. A config crow itself
/// is happy with would then leave Martty toolless, so when the whole-document
/// parse fails we retry against just the `mcpServers` block — the duplicates live
/// in the model and agent sections, never here.
fn mcp_servers_node(text: &str) -> Option<yaml_rust2::Yaml> {
    let document = match yaml_rust2::YamlLoader::load_from_str(text) {
        Ok(documents) => documents.into_iter().next()?,
        Err(_) => yaml_rust2::YamlLoader::load_from_str(&mcp_servers_block(text)?)
            .ok()?
            .into_iter()
            .next()?,
    };
    Some(document["mcpServers"].clone())
}

/// The top-level `mcpServers:` block sliced out by indentation, for the retry path
/// in [`mcp_servers_node`]: the key itself, then every indented, blank or comment
/// line up to the next top-level key.
fn mcp_servers_block(text: &str) -> Option<String> {
    let mut lines = text
        .lines()
        .skip_while(|line| !line.starts_with("mcpServers:"));
    let mut block = String::from(lines.next()?);
    for line in lines {
        let continues = line.starts_with(' ')
            || line.starts_with('\t')
            || line.trim().is_empty()
            || line.trim_start().starts_with('#');
        if !continues {
            break;
        }
        block.push('\n');
        block.push_str(line);
    }
    Some(block)
}

fn yaml_pairs(yaml: &yaml_rust2::Yaml) -> Vec<(String, String)> {
    yaml.as_hash()
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| {
                    Some((key.as_str()?.to_owned(), value.as_str()?.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn yaml_strings(yaml: &yaml_rust2::Yaml) -> Vec<String> {
    yaml.as_vec()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn expand_home(path: &str) -> PathBuf {
    match std::env::var("HOME") {
        Ok(home) => path
            .strip_prefix("~")
            .map(|rest| PathBuf::from(home).join(rest.trim_start_matches('/')))
            .unwrap_or_else(|| PathBuf::from(path)),
        Err(_) => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("martty-mcp-supply-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn write(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap();
    }

    const CROW_YAML: &str = "mcpServers:\n  crow-mcp:\n    transport: stdio\n    command: /bin/crow-mcp\n    args: [mcp]\n  remote:\n    url: \"http://localhost:2769/mcp\"\n";

    const SETTINGS_JSON: &str = "{\"mcpServers\": {\"playwright\": {\"transport\": \"stdio\", \"command\": \"npx\", \"args\": [\"-y\", \"@playwright/mcp\"], \"env\": {\"FOO\": \"bar\"}}, \"stream\": {\"transport\": \"sse\", \"url\": \"http://x/sse\"}}}";

    #[test]
    fn settings_json_wins_over_crow_config() {
        let settings = tmp("settings-win.json");
        let crow = tmp("crow-win.yaml");
        write(&settings, SETTINGS_JSON);
        write(&crow, CROW_YAML);
        let servers = from_settings_json(&settings).unwrap();
        assert_eq!(
            servers.len(),
            2,
            "settings.json key present must not fall through"
        );
        assert!(from_crow_config(&crow).is_some());
    }

    #[test]
    fn absent_settings_key_falls_through_to_crow_config() {
        let settings = tmp("settings-absent.json");
        write(&settings, "{\"theme\": \"iceberg\"}");
        assert!(from_settings_json(&settings).is_none());
        let crow = tmp("crow-fallback.yaml");
        write(&crow, CROW_YAML);
        let servers = from_crow_config(&crow).unwrap();
        assert_eq!(servers.len(), 2);
        match &servers[0] {
            McpServer::Stdio {
                name,
                command,
                args,
                ..
            } => {
                assert_eq!(name, "crow-mcp");
                assert_eq!(command, &PathBuf::from("/bin/crow-mcp"));
                assert_eq!(args, &vec!["mcp".to_owned()]);
            }
            other => panic!("expected stdio, got {other:?}"),
        }
        // url without transport defaults to http, matching crow-cli.tui's _to_wire
        assert!(matches!(&servers[1], McpServer::Http { name, .. } if name == "remote"));
    }

    /// The shape of a real crow config: block-style args, commented-out servers,
    /// and a duplicated model key further down that PyYAML tolerates.
    const DUPLICATE_KEY_YAML: &str = "mcpServers:\n  crow-mcp:\n    #transport: http\n    transport: stdio\n    command: /bin/crow-cli\n    args:\n    - mcp\n    - '--include-tools'\n    - execute\n  #playwright:\n  #  command: npx\n\n# Yo!\nmodels:\n  qwen3.8-max:\n    model: qwen3.8-max\n  qwen3.8-max:\n    model: qwen3.8-max\ndb_uri: sqlite:////~/.agents/crow/crow.db\n";

    #[test]
    fn duplicate_keys_elsewhere_do_not_cost_the_servers() {
        let crow = tmp("crow-duplicate-keys.yaml");
        write(&crow, DUPLICATE_KEY_YAML);
        assert!(
            yaml_rust2::YamlLoader::load_from_str(DUPLICATE_KEY_YAML).is_err(),
            "fixture must actually trip the strict parser"
        );
        let servers = from_crow_config(&crow).expect("tolerant parse must recover the block");
        assert_eq!(servers.len(), 1, "commented-out playwright is not a server");
        match &servers[0] {
            McpServer::Stdio {
                name,
                command,
                args,
                ..
            } => {
                assert_eq!(name, "crow-mcp");
                assert_eq!(command, &PathBuf::from("/bin/crow-cli"));
                assert_eq!(
                    args,
                    &vec![
                        "mcp".to_owned(),
                        "--include-tools".to_owned(),
                        "execute".to_owned()
                    ]
                );
            }
            other => panic!("expected stdio, got {other:?}"),
        }
    }

    #[test]
    fn transport_branch_order_matches_crow_cli_tui() {
        let settings = tmp("settings-branch.json");
        write(
            &settings,
            "{\"mcpServers\": {\"a\": {\"transport\": \"sse\", \"url\": \"http://x/sse\"}, \"b\": {\"transport\": \"http\", \"url\": \"http://x/mcp\"}, \"c\": {\"url\": \"http://x/mcp\"}, \"d\": {\"command\": \"/bin/x\"}, \"e\": {\"nothing\": true}}}",
        );
        let servers = from_settings_json(&settings).unwrap();
        assert_eq!(
            servers.len(),
            4,
            "entry with neither url nor command is skipped"
        );
        assert!(matches!(&servers[0], McpServer::Sse { .. }));
        assert!(matches!(&servers[1], McpServer::Http { .. }));
        assert!(matches!(&servers[2], McpServer::Http { .. }));
        assert!(matches!(&servers[3], McpServer::Stdio { .. }));
    }

    #[test]
    fn wire_shape_carries_env_and_headers() {
        let settings = tmp("settings-wire.json");
        write(
            &settings,
            "{\"mcpServers\": {\"p\": {\"command\": \"npx\", \"env\": {\"FOO\": \"bar\"}}, \"h\": {\"url\": \"http://x\", \"headers\": {\"Authorization\": \"Bearer t\"}}}}",
        );
        let wire = from_settings_json(&settings)
            .unwrap()
            .into_iter()
            .map(McpServer::into_wire)
            .collect::<Vec<_>>();
        match &wire[0] {
            WireMcpServer::Stdio(stdio) => {
                assert_eq!(stdio.env.len(), 1);
                assert_eq!(stdio.env[0].name, "FOO");
                assert_eq!(stdio.env[0].value, "bar");
            }
            other => panic!("expected stdio, got {other:?}"),
        }
        match &wire[1] {
            WireMcpServer::Http(http) => {
                assert_eq!(http.headers.len(), 1);
                assert_eq!(http.headers[0].name, "Authorization");
            }
            other => panic!("expected http, got {other:?}"),
        }
    }

    #[test]
    fn missing_files_mean_toolless_not_error() {
        let nope = tmp("does-not-exist.json");
        assert!(from_settings_json(&nope).is_none());
        assert!(from_crow_config(&nope).is_none());
    }
}
