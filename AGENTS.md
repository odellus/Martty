# Agent notes

## ⛔ READ THIS FIRST — this is crow-term, not Martty

**The product is the Rust binary. There is no Node in it. Period.**

The repo directory is still called `Martty`, the npm package is still named
`@openma/deepseek-harness-tui`, and `npm/lib/` still holds ~60 JS files. All of
it is vestigial. **Nothing under `npm/` executes.** The user runs
`./target/release/crow-term` directly; there is no Cordis host, no dsh profile,
no client-plugin tree, no `ctx.acpClient`, no agent pool, no second process.

Consequences that have already bitten an agent:

- **The JS suite is not a gate and is not product health.** `npm test --prefix
  npm` (434 tests over `scripts/*.test.mjs`) exercises dead code. Do not run it
  to validate a change, and do not report its totals as though they meant
  something. **The Rust suite is the only gate.**
- **There is no plugin command namespace to collide with.** Slash commands are
  exactly `SLASH_COMMANDS` in `src/app.rs`, resolved client-side in the painter.
  A builtin name is the only thing that exists. `/harness` is the Rust builtin;
  the `commandRegistration` in `npm/lib/harness-view.js` is unreachable by
  construction and irrelevant.
- **Anything that "arrives from a Cordis slot snapshot" never arrives.**
  `app.harness_badge` is fed from `conversation.harness`, i.e. Node-driven, so
  it is permanently empty. Chrome, badges and palettes must be driven from the
  Rust side (`CtlEvent` / `AppEvent`), never from a host that does not exist.
- **`scripts/real-agent-e2e.py` is obsolete, not blocked.** It drives the Node
  host through pexpect. There is no Node host.
- **`npm/lib/tui-plugin-store.js marttyHome()` is moot** — it resolves
  `MARTTY_HOME` → `~/.martty` and has never heard of `CROW_HOME`, and it never
  runs. Config lives at `~/.agents/crow/settings.json`
  (`CROW_HOME` → `MARTTY_HOME` → `$DSH_HOME/.agents/crow` → `~/.agents/crow`,
  see `src/runtime.rs`).
- The **one** remaining `node` spawn in `src/` is `main.rs`'s `--demo-skin`
  gallery path (`npm/lib/demo-skin.js`). It is a dev skin gallery, not product
  surface, and it fails without node on PATH by design.

The sections below that describe Cordis, plugins, profiles and the npm bundle
are **historical Martty architecture**. They are kept so the git history and the
`npm/` tree stay interpretable. They do not describe this product, and nothing
in them is a constraint on crow-term.

Read [docs/architecture.md](docs/architecture.md) before changing UI or the
runtime. [docs/plugins.md](docs/plugins.md) and [docs/migration.md](docs/migration.md)
describe the dead Node architecture — read them only to understand `npm/`.

## Commands & verification

- **Rust is the only gate**: `cargo test --locked` and `cargo check --locked --tests`. `make rust-test` / `make rust-build` wrap cargo in `scripts/cargo-guard.sh` (cache-size + disk guards, `DSH_TUI_RUST_CACHE_MAX_GIB`). No clippy/fmt gate exists. Never run repo-wide `cargo fmt` — `acp.rs`, `app.rs` and `ui.rs` carry hundreds of pre-existing rustfmt markers; format only files that are entirely new, and check `rustfmt --check` line numbers against `git diff -U0` for existing ones.
- ⛔ `scripts/cargo-guard.sh` must **never** auto-clean the target dir. It used to `cargo clean` `$DSH_TUI_CARGO_TARGET_DIR` (= `$PWD/target`) at 20 GiB and deleted a release build mid-test-run. Over the limit it warns; pruning is explicit (`cargo-guard.sh prune`). The user builds with bare `cargo build --release -j 6` into `$PWD/target` and tests from it. Do not reintroduce auto-clean.
- Linker OOM: on small-RAM machines `cc`/`ld` can get killed (signal 9) linking the test binary. Retry with `RUSTFLAGS="-C link-arg=-fuse-ld=mold" cargo test` (mold is much lighter). `release` uses `lto = true` and is slow — verify with debug builds.
- ⛔ **JS tests are dead code** — see READ THIS FIRST. `scripts/*.test.mjs` (`node --test`, run via `npm test --prefix npm`) exercise `npm/lib`, which never executes. Not a gate; do not report their totals.
- Rust unit tests live in `tests/unit/*__tests.rs` but are wired in by a `#[cfg(test)] #[path = "../tests/unit/…"] mod tests;` include at the bottom of the owning `src/*.rs` file — add that include for new test files.
- No-TTY visual check: `cargo run -- --dump-frame 100x34` renders the canned demo frame as text; useful to diff transcript/markdown output before and after a rendering change.
- Demos: `crow-term --demo` stays on built-in `default` theme; `crow-term --demo-skin` is the gallery (`ember`) path and is the one flag that still shells out to `node`. ⛔ `make tui-test` (drives a dsh profile) and `make real-agent-e2e` (`scripts/real-agent-e2e.py`, pexpect against the Node host) are both dead — there is no profile and no host.
- Driving the real binary in tests: `tests/startup_session_e2e.rs` spawns the **shipped binary** on a real PTY against `tests/fixtures/stub_acp_agent.py` (dual-stack: `STUB_PROTOCOL=1|2`, `STUB_CAPS=load|resume|both|none`, `STUB_LOG` writes a wire log). That, not a JS harness, is how behaviour gets proven. Validate a Python fixture with `python3 -m py_compile`, never `ast.parse`.

## Release & versioning

- The version lives in two files that must match the `v*` tag: `Cargo.toml` and `npm/package.json` (`scripts/check-release-tag.mjs <tag> npm/package.json Cargo.toml`, run from `.github/workflows/package-npm.yml`). There is no `npm-martty/` — an older note here claimed a third file.
- Add user-visible changes to `CHANGELOG.md` under `[Unreleased]` (Keep-a-Changelog style).
- CI (`package-npm.yml`, PRs + tags) still runs `npm test --prefix npm` and `test:profile-install-matrix` alongside the Rust tests/check. Those two jobs test the dead layer; the Rust jobs are the ones that mean anything. Release builds additionally run static-ELF and old-glibc smoke checks.
- ⛔ The npm bundle is a **delivery wrapper for the static ELF**, not a runtime. `npm/bin/martty.js`, `npm/lib/boot.js` and the `@deepseek-ai/cordis` / `@openma/deepseek-harness-acp` deps are how the binary used to be launched; nothing loads them now. Do not add features there, and do not "fix" rebrand gaps in `npm/lib` — they cannot be reached.

## crow-term architecture — the constraints that ARE real

- **The protocol is negotiated, never declared.** `src/acp/negotiate.rs` sends
  one union `initialize` over an already-live `Channel`, then `classify` →
  `adopt` picks the stack: `src/acp.rs` (v1) or `src/acp/v2.rs`. A harness entry
  is an **argv** — there is no `protocol:` field and no pin override. v2 is
  gated behind the `unstable_protocol_v2` feature.
- **One connection serves every tab.** There is no per-agent pool. Replacing the
  agent means replacing the live connection: `Cmd::SwitchHarness { argv }` is
  intercepted by `relay_commands` *above* `run()` (a stack cannot recover
  `cmd_rx` — both move it into a forwarder thread that dies on send failure),
  and `run_blocking` is a supervisor loop that respawns and renegotiates.
- ⛔ **`run_blocking` keeps ONE tokio runtime for every generation.** Never
  `tokio::spawn` and drop the `JoinHandle` inside it — a detached task outlives
  its connection holding the transport, and the transport holds the child guard,
  so the agent process leaks. `negotiate_and_run` keeps the handle and calls
  `driver.abort()` on every path out. Making a runtime long-lived turns every
  detached task in it into a leak.
- The bus is `std::sync::mpsc`. Teardown is channel-close driven
  (`let Some(mut cmd) = cmd else { break }`), the same exit as `Cmd::Shutdown`.
  `AppEvent` and `PickerKind` have **no `Debug`**; `Cmd` derives `Debug, Clone`
  only, no `PartialEq`. Tests must map events to strings or use `matches!`.
- **Transcript paint comes from ACP `session/update`.** Live, the painter echoes
  user prompts locally and the parser skips the agent's copy; on replay the log
  is the only source. The discriminator is wire order — v2's `ReplayWindow`
  gates the echo on whether the update arrived before the `session/resume`
  response. Do not "fix" the double print by dropping the echo.
- `concat_text_blocks` joins with **no separator**. A test pins that. It is not a
  bug to fix.
- Settings live at `~/.agents/crow/settings.json` (`src/runtime.rs`). Writes
  **patch, never rewrite** (`serde_json` `preserve_order` is on transitively, so
  key order survives), create-if-absent, and **quarantine** a non-empty
  unparseable file rather than replace it.
- crow-term is a **binary crate**, edition 2021, rustc 1.95 → avoid
  `let_chains`. Unit tests live in `tests/unit/*__tests.rs` and are wired in by
  a `#[cfg(test)] #[path = "../tests/unit/…"] mod …;` include at the bottom of
  the owning `src/*.rs` (from `src/acp/*.rs` the path is `../../tests/unit/…`).
  Test modules sit inside the source module, so `use super::*` reaches private
  items. `cargo test --lib` fails — use `--bin crow-term`.
- Slash commands are `SLASH_COMMANDS` in `src/app.rs`, **name-sorted and a test
  enforces it**. Every builtin needs an explicit zh description in
  `src/locale.rs command_desc` — `every_builtin_command_has_an_explicit_zh_desc`
  fails otherwise.

## Protocol & plugin constraints — ⛔ HISTORICAL, describes the dead Node layer

Everything below this line is Martty's Cordis/dsh/npm architecture. It is kept
so `npm/` and the git history stay interpretable. **None of it constrains
crow-term.**

- The ACP client is a Cordis plugin providing `ctx.acpClient`. It is either the client composition root or an `insert` in another client tree. It attaches to an ACP agent by spawn (`dsh-acp` / `dsh --profile acp` / other) or `config.stream`. It does not import a harness.
- Do not sync plugin ids, `inject`, or fibers. Server plugins surface through ACP; client plugins stay on the client tree. Both sides negotiate protocol 0 through `initialize` `_meta.dsh.cordis`; only then may they use `_dsh/cordis/*` Extension Requests/Notifications. TUI painter methods are the `_dsh/cordis/tui/*` child domain. This is serialized capability projection, not plugin-tree sync.
- The primary profile path has two processes: the dsh Host tree mounts Base + the ACP plugin, then its runner starts a separate TUI Client process. ACP uses that Client process's stdin/stdout. The Client process receives the user TTY on fd 3/4 and maps it to the Rust painter; those descriptors never carry Host↔Client ACP. Standalone `crow-term` may instead spawn any ACP agent on standard pipes (`dsh-tui` remains a compatibility alias).
- Third-party packs are sibling `insert` rows plus `inject = ['tuiTheme']`. Do not nest them with `ctx.plugin` inside the runner.
- TUI npm may depend on `@deepseek-ai/cordis` only among `@deepseek-ai/*`; do not depend directly on `dsh-*`. It deliberately carries `@openma/deepseek-harness-acp` as a runtime dependency and exports its own Creator overlay. The TUI bundle mounts both Host plugins on the Base tree; neither enters the separate Client tree.
- The `tui-theme` service catalog starts with builtin `default`. Palette packs are sibling `insert` rows that `register` into the catalog (like agent presets). `/theme` and `tuiTheme.activate` switch which pack covers. `--demo-skin` is the only shipped path that selects gallery `ember`.
- The first shell slot proof is `chrome.right`: plugins inject `tuiSlots` and contribute validated `TuiNode` trees. Conversation timeline slots remain later work.
- Do not give plugins the TTY, kitty, raw mode, or global terminal size.
- Transcript paint comes from ACP `session/update`. Do not drive chrome or palette from `session/update`. Do not put TUI chrome in ACP `_meta`.
- ACP auth follows Backchat: read `initialize.authMethods`, submit in-app forms through `authenticate` `_meta`, run Terminal Auth (`_meta["terminal-auth"]`) on the TTY, then `authenticate` to re-check. `/login` stays the agent's slash when advertised. Phase 1 demo is `--demo-skin` (`ember`).
- Token names, kinds, and slot names are append-only; field meaning changes bump `protocol`.
