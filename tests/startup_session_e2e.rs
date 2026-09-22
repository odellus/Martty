//! Tier 3 — end to end: the shipped `crow-term` binary on a real PTY, driving the
//! stub ACP agent in `tests/fixtures/stub_acp_agent.py`.
//!
//! The rungs below this one: `tests/unit/acp__tests.rs` unit-tests the re-attach
//! helpers and drives `connect()` against an in-process mock agent, and
//! `tests/unit/app__resume_tests.rs` covers the App side of the startup bind.
//! Here the flags go on a real command line and the assertions read pane pixels
//! plus the agent's own JSONL wire log.

use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/stub_acp_agent.py"
);
const SESSION: &str = "coolname";
const MODEL: &str = "qwen3.8-max";
const STARTUP: [&str; 4] = ["--session-id", SESSION, "--model", MODEL];
const ROWS: usize = 30;
const COLS: usize = 110;

/// Every test here spawns a real TUI plus a real agent process. Five of those
/// at once on a cold page cache contend hard enough to miss the startup
/// deadline, so they take turns: the guard is held for the Pane's lifetime and
/// released after the child is reaped. Poisoning is ignored on purpose — one
/// test's assertion failure must not cascade into four spurious ones.
static GATE: Mutex<()> = Mutex::new(());

/// A live crow-term on a PTY: `text` is everything it has drawn, ANSI-stripped.
struct Pane {
    _gate: MutexGuard<'static, ()>,
    child: Child,
    master: File,
    home: PathBuf,
    raw: Vec<u8>,
    text: String,
}

impl Pane {
    /// Read the PTY until every needle has been drawn. A pane that never gets
    /// there is the failure this tier exists to catch, so the deadline panics
    /// with the squeezed pane instead of hanging the suite.
    fn expect(&mut self, needles: &[&str]) {
        // Warm this lands in well under a second, and cold (freshly relinked
        // 190 MB binary, swap-constrained box) in under two. A real failure
        // still fails; it just takes the rest of the slack.
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            self.pump();
            let missing: Vec<&str> = needles
                .iter()
                .copied()
                .filter(|needle| !self.text.contains(needle))
                .collect();
            if missing.is_empty() {
                return;
            }
            if let Some(status) = self.child.try_wait().expect("poll crow-term") {
                panic!(
                    "crow-term exited ({status}) before drawing {missing:?}\n{}",
                    self.diagnosis()
                );
            }
            if Instant::now() >= deadline {
                panic!("crow-term never drew {missing:?}\n{}", self.diagnosis());
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    /// Type into the pane. `keys` goes to the PTY verbatim, so `\r` submits and
    /// `\x1b` is the interrupt the composer binds to escape.
    fn send(&mut self, keys: &str) {
        self.master
            .write_all(keys.as_bytes())
            .expect("write to the PTY");
        self.master.flush().expect("flush the PTY");
    }

    /// The startup path must not crash or exit: a bound (or refused) session
    /// leaves the TUI up and usable.
    fn assert_alive(&mut self) {
        self.pump();
        assert!(
            self.child.try_wait().expect("poll crow-term").is_none(),
            "crow-term exited on its own during startup\n{}",
            self.squeezed()
        );
    }

    /// Every request the stub agent received, in order.
    fn methods(&self) -> Vec<String> {
        self.wire()
            .iter()
            .filter_map(|entry| entry["msg"]["method"].as_str())
            .map(str::to_owned)
            .collect()
    }

    /// The `params` of the first request the agent received under `method`, or
    /// null when it never got one.
    fn request_params(&self, method: &str) -> serde_json::Value {
        self.wire()
            .iter()
            .find(|entry| entry["msg"]["method"].as_str() == Some(method))
            .map(|entry| entry["msg"]["params"].clone())
            .unwrap_or(serde_json::Value::Null)
    }

    /// The `value` of each `session/set_config_option` the agent received —
    /// how `--model` shows up on the wire.
    fn model_requests(&self) -> Vec<String> {
        self.wire()
            .iter()
            .filter(|entry| entry["msg"]["method"].as_str() == Some("session/set_config_option"))
            .filter_map(|entry| entry["msg"]["params"]["value"].as_str())
            .map(str::to_owned)
            .collect()
    }

    fn wire(&self) -> Vec<serde_json::Value> {
        let body = std::fs::read_to_string(self.home.join("wire.jsonl")).unwrap_or_default();
        body.lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .collect()
    }

    fn pump(&mut self) {
        let mut chunk = [0_u8; 16384];
        loop {
            match self.master.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => self.raw.extend_from_slice(&chunk[..read]),
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                // The slave is gone: nothing more will ever arrive.
                Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
                Err(error) => panic!("read PTY: {error}"),
            }
        }
        self.text = render_frame(&self.raw, ROWS, COLS);
    }

    /// Everything worth reading when a pane never got where it was going: what
    /// was drawn, what reached the agent, and what crow-term said on stderr.
    fn diagnosis(&self) -> String {
        format!(
            "pane:\n{}\nwire: {:?}\nmodel writes: {:?}\nstderr: {}",
            self.squeezed(),
            self.methods(),
            self.model_requests(),
            std::fs::read_to_string(self.home.join("stderr.txt"))
                .unwrap_or_default()
                .trim()
        )
    }

    /// Pane text is one long run of padded, `\r`-separated rows; collapse it to
    /// something a panic message can carry.
    fn squeezed(&self) -> String {
        let mut out = String::new();
        for row in self.text.replace('\r', "\n").split('\n') {
            let flat = row.split_whitespace().collect::<Vec<_>>().join(" ");
            if !flat.is_empty() {
                out.push_str(&flat);
                out.push('\n');
            }
        }
        out
    }
}

impl Drop for Pane {
    fn drop(&mut self) {
        // SIGTERM is the way a user quits; sigterm_cleanup.rs owns the teardown
        // assertions, this only has to not leak a child or a temp dir.
        unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.child.try_wait().ok().flatten().is_none() {
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

/// Start crow-term on a fresh PTY against the stub agent. `None` (a skipped test)
/// when this machine has no python3 to run the fixture with.
fn launch(tag: &str, caps: &str, refuse: bool, args: &[&str]) -> Option<Pane> {
    let mut stub = vec![("STUB_CAPS", caps)];
    if refuse {
        stub.push(("STUB_LOAD_FAIL", "1"));
    }
    launch_with(tag, args, &stub, None)
}

/// The launch core. `stub` is the fixture's environment — `STUB_PROTOCOL=2` is
/// what makes it answer the union `initialize` as a v2 agent — and `settings` is
/// the body of `<CROW_HOME>/settings.json`.
///
/// That file is how a test pins the MCP supply. Without it `mcp_supply::load()`
/// falls through to the developer's real `~/.agents/crow/config.yaml`, and an
/// assertion about the servers in `session/new` would then be reading whatever
/// happened to be configured on the machine running the suite.
fn launch_with(
    tag: &str,
    args: &[&str],
    stub: &[(&str, &str)],
    settings: Option<&str>,
) -> Option<Pane> {
    if Command::new("python3").arg("--version").output().is_err() {
        eprintln!("skipping {tag}: python3 not found");
        return None;
    }
    let gate = GATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let home = std::env::temp_dir().join(format!(
        "crow-term-startup-e2e-{}-{tag}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&home);
    for sub in ["ws", "sessions"] {
        std::fs::create_dir_all(home.join(sub)).expect("create e2e home");
    }
    if let Some(body) = settings {
        std::fs::write(home.join("settings.json"), body).expect("write e2e settings");
    }

    let mut master_fd = -1;
    let mut slave_fd = -1;
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master_fd,
                &mut slave_fd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0,
        "allocate a real pseudo-terminal"
    );
    let size = libc::winsize {
        ws_row: ROWS as u16,
        ws_col: COLS as u16,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    assert_eq!(unsafe { libc::ioctl(slave_fd, libc::TIOCSWINSZ, &size) }, 0);

    let master = unsafe { File::from_raw_fd(master_fd) };
    let slave = unsafe { File::from_raw_fd(slave_fd) };
    let stderr = File::create(home.join("stderr.txt")).expect("capture stderr");
    let mut command = Command::new(env!("CARGO_BIN_EXE_crow-term"));
    command
        .arg("--agent")
        .arg("python3")
        .arg("--agent-arg")
        .arg(FIXTURE)
        .arg("-w")
        .arg(home.join("ws"))
        .arg("--session-root")
        .arg(home.join("sessions"))
        .args(args)
        .env("TERM", "xterm-256color")
        .env("CROW_HOME", &home)
        .env("STUB_LOG", home.join("wire.jsonl"))
        .env_remove("TERM_PROGRAM")
        .stdin(Stdio::from(slave.try_clone().expect("clone PTY stdin")))
        .stdout(Stdio::from(slave))
        .stderr(Stdio::from(stderr));
    for (key, value) in stub {
        command.env(key, value);
    }
    let child = command.spawn().expect("start crow-term on the PTY");

    let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
    assert!(flags >= 0);
    assert_eq!(
        unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0
    );
    Some(Pane {
        _gate: gate,
        child,
        master,
        home,
        raw: Vec::new(),
        text: String::new(),
    })
}

/// Replay the PTY stream into the screen the user would see.
///
/// ratatui repaints by diff, so the raw stream is a run of partial updates with
/// cursor moves between them and a phrase can be split across writes — asserting
/// on the stripped stream once read a perfectly good run as `qwn3.8-max` because
/// the `e` landed in a later repaint. Model the cells instead.
fn render_frame(raw: &[u8], rows: usize, cols: usize) -> String {
    let mut grid = vec![vec![' '; cols]; rows];
    let buf: Vec<char> = String::from_utf8_lossy(raw).chars().collect();
    let (mut row, mut col, mut i) = (0_usize, 0_usize, 0_usize);
    let last_row = rows - 1;
    let last_col = cols - 1;
    while i < buf.len() {
        let c = buf[i];
        if c == '\x1b' {
            if buf.get(i + 1) == Some(&'[') {
                let mut end = i + 2;
                let mut params = String::new();
                while end < buf.len()
                    && (buf[end].is_ascii_digit() || matches!(buf[end], ';' | '?' | '>' | '<'))
                {
                    params.push(buf[end]);
                    end += 1;
                }
                match buf.get(end).copied().unwrap_or('m') {
                    'H' | 'f' => {
                        let mut at = params.split(';').map(|part| csi_num(part, 1));
                        row = at.next().unwrap_or(1).saturating_sub(1).min(last_row);
                        col = at.next().unwrap_or(1).saturating_sub(1).min(last_col);
                    }
                    'A' => row = row.saturating_sub(csi_num(&params, 1)),
                    'B' => row = (row + csi_num(&params, 1)).min(last_row),
                    'C' => col = (col + csi_num(&params, 1)).min(last_col),
                    'D' => col = col.saturating_sub(csi_num(&params, 1)),
                    // Erase in line: 0 = cursor to end, 2 = whole row.
                    'K' if csi_num(&params, 0) == 2 => {
                        grid[row].iter_mut().for_each(|cell| *cell = ' ')
                    }
                    'K' => grid[row][col..].iter_mut().for_each(|cell| *cell = ' '),
                    _ => {}
                }
                i = end + 1;
                continue;
            }
            if buf.get(i + 1) == Some(&']') {
                let mut end = i + 2;
                while end < buf.len() && buf[end] != '\u{7}' && buf[end] != '\x1b' {
                    end += 1;
                }
                i = if buf.get(end) == Some(&'\x1b') {
                    end + 2
                } else {
                    end + 1
                };
                continue;
            }
            i += 2;
            continue;
        }
        match c {
            '\r' => col = 0,
            '\n' => row = (row + 1).min(last_row),
            other if (other as u32) >= 0x20 => {
                grid[row][col] = other;
                if col >= last_col {
                    col = 0;
                    row = (row + 1).min(last_row);
                } else {
                    col += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    grid.iter()
        .map(|line| line.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn csi_num(params: &str, default: usize) -> usize {
    params.parse::<usize>().unwrap_or(default)
}

fn assert_reattached(methods: &[String], used: &str, avoided: &[&str]) {
    assert!(
        methods.iter().any(|m| m == used),
        "expected {used} on the wire, got {methods:?}"
    );
    for method in avoided {
        assert!(
            !methods.iter().any(|m| m == method),
            "{method} must not be sent, got {methods:?}"
        );
    }
}

#[test]
fn startup_session_id_loads_when_the_agent_only_advertises_load_session() {
    let Some(mut pane) = launch("load", "load", false, &STARTUP) else {
        return;
    };
    pane.expect(&["loaded coolname", MODEL, "replayed history line"]);
    let methods = pane.methods();
    assert_reattached(&methods, "session/load", &["session/resume", "session/new"]);
    assert_eq!(pane.model_requests(), vec![MODEL.to_owned()]);
    pane.assert_alive();
}

#[test]
fn startup_session_id_resumes_when_the_agent_advertises_resume() {
    let Some(mut pane) = launch("resume", "resume", false, &STARTUP) else {
        return;
    };
    pane.expect(&["resumed coolname", MODEL]);
    let methods = pane.methods();
    assert_reattached(&methods, "session/resume", &["session/load", "session/new"]);
    assert_eq!(pane.model_requests(), vec![MODEL.to_owned()]);
    pane.assert_alive();
}

#[test]
fn startup_session_id_with_no_reattach_method_fails_instead_of_starting_fresh() {
    let Some(mut pane) = launch("none", "none", false, &STARTUP) else {
        return;
    };
    pane.expect(&[
        "connection failed",
        "neither session/resume nor session/load",
    ]);
    // Both spellings were probed, and the fallback nobody asked for was not.
    let methods = pane.methods();
    assert_reattached(&methods, "session/load", &["session/new"]);
    assert_reattached(&methods, "session/resume", &["session/new"]);
    pane.assert_alive();
}

#[test]
fn startup_session_id_the_agent_refuses_reports_the_refusal() {
    let Some(mut pane) = launch("refused", "load", true, &STARTUP) else {
        return;
    };
    pane.expect(&["connection failed", "no such session: coolname"]);
    // A real refusal is an answer, not a "try the other spelling" prompt.
    let methods = pane.methods();
    assert_reattached(&methods, "session/load", &["session/resume", "session/new"]);
    pane.assert_alive();
}

#[test]
fn no_session_id_flag_starts_a_fresh_session_and_keeps_the_welcome_banner() {
    let Some(mut pane) = launch("fresh", "both", false, &[]) else {
        return;
    };
    pane.expect(&["https://crow-ai.dev", "stub-default"]);
    let methods = pane.methods();
    assert_reattached(&methods, "session/new", &["session/load", "session/resume"]);
    assert!(
        pane.model_requests().is_empty(),
        "no --model, no config write: {:?}",
        pane.model_requests()
    );
    pane.assert_alive();
}

#[test]
fn a_replayed_tool_call_shows_its_command_and_output_however_the_agent_sends_them() {
    let Some(mut pane) = launch("toolcall", "load", false, &STARTUP) else {
        return;
    };
    // t1 carries `rawInput`, t2 carries only a fenced `content` block the way
    // crow-cli does. Both frame the command; the completion's echo of it is
    // not drawn a second time.
    pane.expect(&[
        "\u{250c}\u{2500} python \u{2500}",
        "print(6 * 7)",
        "print(6 * 9)",
        "54",
        // Thoughts open too: the body is on screen with no click, under a
        // heading that says how many lines it has.
        "the stub thinks in italics",
        "thought",
    ]);
    let screen = pane.text.clone();
    assert_eq!(
        screen.matches("print(6 * 9)").count(),
        1,
        "the echoed command is drawn once:\n{screen}"
    );
    pane.assert_alive();
}


// ---------------------------------------------------------------------------
// ACP v2 — the same binary on the same PTY, with a stub that answers the union
// `initialize` as a v2 agent. Every shape below is one v1 never had to get
// right, and each one is a thing that actually broke.
// ---------------------------------------------------------------------------

/// The stub speaking v2. `STUB_CAPS=both` is what a real v2 agent looks like
/// from here: `capabilities.session` is a presence marker, so resume and list
/// are both on and `session/load` does not exist at all.
const V2_STUB: [(&str, &str); 2] = [("STUB_PROTOCOL", "2"), ("STUB_CAPS", "both")];

/// A pinned tool supply. With no `mcpServers` in `<CROW_HOME>/settings.json`
/// the launch falls through to the developer's real `~/.agents/crow/config.yaml`
/// and these assertions become about that machine instead of about crow-term.
/// One stdio and one http, because stdio is the transport the two protocols
/// disagree about.
const SUPPLY: &str = r#"{"mcpServers":{
  "crow-mcp":{"command":"/bin/echo","args":["mcp"],"env":{"FOO":"bar"}},
  "remote":{"url":"https://example.invalid/mcp","headers":{"Authorization":"Bearer t"}}
}}"#;

#[test]
fn a_v2_agent_is_negotiated_and_its_tool_supply_carries_its_transport() {
    let Some(mut pane) = launch_with("v2-new", &[], &V2_STUB, Some(SUPPLY)) else {
        return;
    };
    // The session came up at all, which is the negotiation working: one union
    // `initialize` went out carrying both spellings, and the stub answering it
    // with `protocolVersion: 2` selected the v2 stack.
    pane.expect(&["stub-default"]);
    let methods = pane.methods();
    assert_reattached(&methods, "session/new", &["session/load", "session/resume"]);

    // The panic this pins: v1's `McpServer::Stdio` is `#[serde(untagged)]` — no
    // `type` key on the wire — and v2's is internally tagged, so re-serialising
    // the v1 JSON as v2 died inside the connection's main_fn. The tokio task
    // went away with the pane stuck at `starting runtime`, `session/new` never
    // written, and no error on any channel. An untagged server here is that
    // failure one deserialize away.
    let servers = pane.request_params("session/new")["mcpServers"]
        .as_array()
        .expect("session/new carries mcpServers")
        .clone();
    assert_eq!(servers.len(), 2, "{servers:?}");
    for server in &servers {
        assert!(
            server.get("type").is_some(),
            "an MCP server with no transport tag cannot be read back as v2: {server}"
        );
    }
    let stdio = servers
        .iter()
        .find(|server| server["type"] == "stdio")
        .expect("the stdio server survives the crossing");
    assert_eq!(stdio["name"], "crow-mcp");
    assert_eq!(stdio["command"], "/bin/echo");
    assert_eq!(stdio["args"], serde_json::json!(["mcp"]));
    pane.assert_alive();
}

#[test]
fn a_v2_prompt_reaches_the_pane_once() {
    let Some(mut pane) = launch_with("v2-echo", &[], &V2_STUB, Some(SUPPLY)) else {
        return;
    };
    pane.expect(&["stub-default"]);
    pane.send("reply with exactly: PONG\r");
    pane.expect(&["stub reply ok"]);
    let screen = pane.text.clone();
    // crow-term draws the user's line as it submits, and a v2 agent hands the
    // same line straight back as a `user_message` update. Each is correct on
    // its own; together they printed every prompt twice. The copy that has to
    // survive is the replayed one, which arrives inside the resume window, so
    // the live echo is dropped and this stays at one.
    assert_eq!(
        screen.matches("reply with exactly: PONG").count(),
        1,
        "the prompt is drawn once:\n{}",
        pane.squeezed()
    );
    pane.assert_alive();
}

#[test]
fn a_v2_turn_ends_on_the_idle_state_not_on_the_prompt_acknowledgement() {
    let stub = [
        ("STUB_PROTOCOL", "2"),
        ("STUB_CAPS", "both"),
        ("STUB_HOLD", "1"),
    ];
    let Some(mut pane) = launch_with("v2-hold", &[], &stub, Some(SUPPLY)) else {
        return;
    };
    pane.expect(&["stub-default"]);
    pane.send("do something slow\r");
    pane.expect(&["working"]);
    // `session/prompt` has been answered `{}` by now — the stub sends that
    // before it says anything else, exactly as the real agent does. A client
    // that read the acknowledgement as the result went idle right there, and
    // idle is the one state esc does nothing in: no `session/cancel`, no
    // `interrupted`, and the agent's turn left running with nobody watching it.
    pane.send("\x1b");
    pane.expect(&["interrupted"]);
    assert!(
        pane.methods().iter().any(|method| method == "session/cancel"),
        "esc cancelled the turn that was still open: {:?}",
        pane.methods()
    );
    pane.assert_alive();
}

#[test]
fn a_v2_resume_repaints_the_transcript_the_agent_replays() {
    let Some(mut pane) = launch_with("v2-resume", &STARTUP, &V2_STUB, Some(SUPPLY)) else {
        return;
    };
    pane.expect(&[
        "resumed coolname",
        MODEL,
        "an older prompt",
        "the stub thinks in italics",
        "replayed history line",
        "print(6 * 7)",
        "42",
    ]);
    let methods = pane.methods();
    assert_reattached(&methods, "session/resume", &["session/load", "session/new"]);
    // `replayFrom` is the reason v2 could drop `session/load`: the agent re-sends
    // the transcript over the same update stream instead of the client reading a
    // log it does not have.
    assert_eq!(
        pane.request_params("session/resume")["replayFrom"]["type"],
        "start",
        "resume asks for the transcript from the beginning"
    );
    assert_eq!(pane.model_requests(), vec![MODEL.to_owned()]);
    // v2 renamed the option identifier and dropped the old key rather than
    // aliasing it, so a client still writing `id` silently sets nothing.
    let config = pane.request_params("session/set_config_option");
    assert_eq!(config["configId"], "model");
    assert!(config.get("id").is_none(), "v2 has no `id`: {config}");

    let screen = pane.text.clone();
    // The same `user_message` notification the live turn drops. Here it is the
    // pane's only copy of the prompt, so the replay window has to let it
    // through — and let it through once.
    assert_eq!(
        screen.matches("an older prompt").count(),
        1,
        "the replayed prompt is drawn once:\n{}",
        pane.squeezed()
    );
    pane.assert_alive();
}

#[test]
fn a_v2_permission_ask_reaches_the_user_and_the_answer_reaches_the_agent() {
    let stub = [
        ("STUB_PROTOCOL", "2"),
        ("STUB_CAPS", "both"),
        ("STUB_ASK_PERMISSION", "1"),
    ];
    let Some(mut pane) = launch_with("v2-perm", &[], &stub, Some(SUPPLY)) else {
        return;
    };
    pane.expect(&["stub-default"]);
    pane.send("run the script\r");
    // v2 moved the copy the user reads to a required top-level `title` and put
    // the call it describes in `subject`. A client still reading the title off
    // the tool call, as v1 does, draws a box with nothing in it.
    pane.expect(&[
        "approval",
        "Run this script?",
        "Allow once",
        "allow_once",
        "Reject once",
        "reject_once",
    ]);
    // Enter takes the highlighted option, which is the first `allow_once`.
    pane.send("\r");
    pane.expect(&["permission answer: allow"]);

    // The agent got a real v2 outcome and not an empty object. `outcome` is
    // tagged twice: once as the response's only field, once as the variant.
    let reply = pane
        .wire()
        .into_iter()
        .find(|entry| entry["msg"]["id"].as_str() == Some("perm-1"))
        .expect("the agent's permission request was answered");
    let answer = &reply["msg"]["result"]["outcome"];
    assert_eq!(answer["outcome"], "selected");
    assert_eq!(answer["optionId"], "allow");
    pane.assert_alive();
}
