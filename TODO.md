## **DO NOT ASK USER FOR FEEDBACK — THIS IS THE USER FEEDBACK.**
## **DO NOT ASK USER FOR NEXT STEPS — THESE ARE THE NEXT STEPS.**

# TODO — ACP v2 in crow-term (unordered scope capture)

Ordered execution lives in `PLAN.md`. This file is the scope net: everything
that has to happen, with the verification criterion where one is known.
`[~]` = deliberately deferred, with the reason.

User mandate, verbatim:
> "Let's make a plan for adding ACP v2 support for Martty/crow-term. We have a
> v2 client in rust at ~/.agents/crow/src/worktrees/crow-cli-rs that has been
> abandoned in favor of the work we're doing now but which can help us integrate
> our v2 acp agent we have created in python with `crow-cli acp2`.
> https://agentclientprotocol.com/protocol/v2/migration will help you understand
> better. And crow-cli run in ~/.agents/crow/src/crow-cli is already an ACP
> client that shows how to support both v1 and v2 agents through protocol
> handshake during init."

## WHERE THIS STANDS

The testing mandate is **DONE**, and so is the in-TUI harness picker. Suite, all
green at `761f7a5`:

    Rust:  918 unit + 2 cli_help + 1 sigterm_cleanup + 12 startup_session_e2e + 1 tcp_attach

⛔ **The Rust suite is the ONLY gate.** The JS numbers previously listed here
(11 pretest + 434 main) test `npm/lib`, which is **dead code that never
executes** — the product is the Rust binary run directly, no Cordis host, no
plugin tree, no node anywhere. Do not run the JS suite to validate a change and
do not report its totals as product health. This is written into `AGENTS.md`
under "⛔ READ THIS FIRST — this is crow-term, not Martty".

Two pre-existing warnings only (`ui__tests.rs:3591` unused `ctl`,
`input__vim__tests.rs:74` snake_case). `scripts/cargo-guard.sh` now
**warns** instead of cleaning when the cache is over 20 GiB — the warning is
expected, and auto-clean must never come back (it deleted a release build).

Ordered status lives in `PLAN.md`'s phase markers. The short version: Phases 0,
2, 3, 8 done; 1 obsolete (never built, correctly); 4 and 6 half; 5, 7, 9, 10
open. **`/harness` in the Rust binary is BUILT, live-verified and pinned** —
commits `b3e129d` (feature) and `761f7a5` (tests). It respawns the connection
through the union probe, so switching harness also switches protocol; that is
exactly what the npm host cannot do (`acp-client.js` is v1-only, so
`setDefaultAgent` to a v2 recipe hangs there). **One decision is now open and
belongs to the user: the builtin `/harness` shadows the Node plugin's `/harness`
— see the ⭐ block under "Harness / config / JS layer".**

Operating order, learned the hard way: **make it work live against the real
agent over a teed PTY, THEN write the test that pins the shape that broke.** And
every bug-fix assertion gets a mutation check — an assertion never seen to fail
is a comment, not a test.

## Non-negotiables
- [x] **v1 keeps working.** Additive only. Held: the six pre-existing v1 e2e
      tests in `tests/startup_session_e2e.rs` are untouched and green,
      `tests/tcp_attach.rs` is green, and the whole 902-test unit suite passes
      with no v1 expectation edited. (The literal byte-for-byte frame diff was
      never run; the untouched v1 tests are the stronger pin, since a frame
      baseline would have to be re-captured to compare against.)
- [x] **Negotiated, not declared.** No `protocol:` field exists in any harness
      config, and **no pin override was built either** — a pin would only add a
      way to be wrong. The agent reports its version at `initialize`; a v2
      harness entry is the same shape as a v1 one with a different argv
      (`crow-cli` `["acp"]` vs `crow-cli-v2` `["acp2"]` in
      `~/.agents/crow/settings.json`). Never guessed from argv.
- [x] **v2 gated behind `unstable_protocol_v2`** until the protocol stabilizes.
      `Cargo.lock` did not move.
- [~] **Never fail on an unknown enum variant.** Held where it matters — nothing
      panics or errors on an unknown variant, and `TurnOutcome::Stopped{kind:
      Cow<str>}` carries a non-spec stop reason through verbatim (pinned by
      `a_stop_reason_outside_the_spec_keeps_the_agents_own_word`). **But the
      stronger half of the ruling is UNIMPLEMENTED:** an unknown `sessionUpdate`
      discriminator must render *as its kind*, never vanish, and
      `events.rs::parse_session_update` still ends `_ => Vec::new()`. Silent, not
      fatal — which is why it has survived this long.

## Facts already established (do not re-derive)
- [x] `agent-client-protocol` **2.0.0 — the version already pinned — has
      `unstable_protocol_v2`, `schema::v2`, `Client::v2()`, and
      `Client::protocol_connector()`.** No dep bump needed. **Correction: the
      lock DID move, by exactly one new transitive package** — enabling
      `unstable_protocol_v2` pulls `diffy 0.5.1` (+`hashbrown 0.17.1`) into
      `agent-client-protocol-schema`, for v2's `git_patch` diffs. `25254c1`,
      10 added lines, no version change. That is the whole footprint.
- [x] Schema 1.5.0's v2 `SessionUpdate` set is complete for the stable protocol
      (state_update, tool_call_content_chunk, terminal_update,
      terminal_output_chunk, plan_update, plan_removed, whole-message upserts).
      1.7.0 adds only compaction_update + compaction_summary_chunk.
- [x] Rust 1.5.0 v2 structs == Python alpha.3 schema, field for field, on
      ToolCallUpdate / PlanUpdate / PlanItems / TerminalUpdate / IdleStateUpdate /
      SessionConfigOption / RequestPermissionRequest / UpdateSessionNotification.
- [x] **`PromptResponse` has ONLY `_meta`** in Rust 1.5.0, Rust 1.7.0, and Python
      alpha.3. The website's required `messageId` is a NEWER draft than anything
      installed. Tolerate it, never require it. (PLAN RULING 1.)
- [x] **The Rust SDK's `ClientProtocolConnector` was REJECTED — `discover.py`'s
      hand-rolled union probe WAS ported**, as `src/acp/negotiate.rs`. (PLAN
      RULING 3, corrected: the original note said the opposite and would have
      led a future agent to "fix" working code back to the connector.) Two
      reasons: the connector reuses the live connection only when both sides'
      normalized params match **exactly**, and v2's `ClientCapabilities` has no
      `fs`/`terminal` while crow-term's v1 caps advertise both → equality is
      unreachable, so **every v1 agent gets spawned twice**; and its contract is
      `FnMut() -> impl ConnectTo<Client>`, i.e. a re-spawn, which an attach
      endpoint cannot satisfy at all. The Python `adopt_v2` `_state` poke is
      still not needed — the Rust `Channel` is adoptable as-is.
- [x] Because nothing re-spawns, **`AttachStdio{File,File}` and
      `AttachTcp(TcpStream)` need no special case.** They are still consumed by
      value, but the probe rides the connection the endpoint already produced.
      `tests/tcp_attach.rs` green. The old single-impl-only restriction existed
      solely to dodge the connector's factory; it is gone. There is also **no
      v1-retry fallback** — if the union initialize errors, fail fast with the
      agent's own words (pinned by `a_refusal_carries_the_agents_own_words`).
- [x] **`check_blocking` (`acp.rs:238`) WAS a second v1-initialize site** — the
      `--check` path sent `initialize_request()` and read `init.agent_info.name`,
      which against a v2 agent is `-32602` (`info` required) or a silent hang.
      **Fixed and verified live:** `crow-term --check-runtime` against
      `crow-cli acp2` prints `initialize ok in 1.8s → crow-cli acp2`, rc 0.
- [x] agent2 emits `ToolCallUpdate.name`; Rust 1.5.0's struct has no `name` field.
      serde ignores it (no parse failure). Because updates are forwarded as RAW
      JSON, `events.rs` reads `name` directly — the gap is free. Do not bump the dep.
- [x] 2.1.0 removes the `unstable_auth_methods` + `unstable_elicitation` features
      crow-term enables. Bumping is a Phase 9 decision, not a prerequisite.

## Dependency / build
- [x] Add `"unstable_protocol_v2"` to the `agent-client-protocol` features
      (`Cargo.toml:30`, alongside the three existing unstable features).
- [x] Confirm `Cargo.lock` after the feature add — one new transitive package
      (`diffy`), no version change. See the correction above.
- [x] Keep the `scripts/cargo-guard.sh` gate; never bare cargo. **Amended in
      `56536a9`:** the guard used to `cargo clean` `$DSH_TUI_CARGO_TARGET_DIR`
      at 20 GiB, which deleted the user's release build. It now warns and points
      at `scripts/cargo-guard.sh prune`; auto-clean is opt-in via
      `DSH_TUI_RUST_CACHE_AUTOCLEAN=1`. Never reintroduce it — the user builds
      with a bare `cargo build --release -j 6` into `$PWD/target` and tests from
      that binary.
- [ ] Re-check the 591 `cargo fmt --check` diff-marker total after every edit.
      Never run repo-wide `cargo fmt`.

## Rust client structure
- [~] `src/acp.rs` → shared plumbing only, and `src/acp/v1.rs` — **NEVER BUILT,
      AND SHOULD NOT BE.** `ConnectionTo<Agent>` is ONE type for both protocols
      in SDK 2.0.0 (`V2ConnectionTo` does not exist; the protocol lives in the
      *Builder*), so there is no seam to split along. `connect()` stayed a free
      function in `src/acp.rs` (~3111 lines) and both stacks share its helpers
      (`TurnOutcome`, `apply_prompt_finish`, `bind_session`,
      `call_tui_extension`, the auth helpers). Splitting anyway is churn with a
      compile error at every shared helper.
- [x] `src/acp/v2.rs` — NEW (~1465 lines), ported from
      `crow-cli-rs/crow-cli/src/main.rs:1077-1233` and `crow-cli`'s
      `client2/subagent.py`. `connect(channel, negotiated, cfg, bus, cmd_rx)` at
      `:713`; handles 18 `Cmd` variants, with the other 11 in
      `run_version_neutral` — 18 + 11 = 29 = the complete enum, so nothing
      reaches the catch-all.
- [x] `src/acp/negotiate.rs` — NEW (~370 lines), the union probe. See the
      corrected RULING 3 above.
- [x] `src/acp/control.rs` — `run_version_neutral` (`:122-393`) is the ONE place
      version-neutral commands are handled; `run_control` (`:396`) opens with it
      and so does the v2 loop (`v2.rs:1065`). Pinned by
      `version_neutral_commands_are_handled_without_a_typed_request`.
- [x] The bus (`AppEvent::Rpc{method:"session/update", params}` as RAW JSON) stays
      the seam — `events.rs` is still the single interpreter and the whole
      UI/transcript layer is protocol-agnostic. Raw, not typed, so
      `ToolCallUpdate.name` survives schema 1.5.0's missing field.
- [x] The Cordis `UntypedMessage` handler (`_dsh/cordis/tui/*`) IS registered on
      the v2 builder (`v2.rs:742`, dispatching THEME_UPDATE / SLOTS_UPDATE /
      COMMANDS_UPDATE / OVERLAY_UPDATE / APPROVALS_UPDATE / UI_UPDATE), and
      `surface.cordis` is set from the v2 init at `:929`.
- [x] `check_blocking` (`acp.rs:238`, the `--check` path) dual-stacks and prints
      the negotiated version next to the agent name. Verified live:
      `--check-runtime` against `crow-cli acp2` prints
      `initialize ok in 1.8s → crow-cli acp2`, rc 0.

## Turn lifecycle (the semantic core) — IMPLEMENTED, partly unpinned
- [x] v2 `session/prompt` response = ACCEPTANCE, not completion. No `TurnEnd`
      from it. Pinned on a real PTY by
      `a_v2_turn_ends_on_the_idle_state_not_on_the_prompt_acknowledgement`
      (airtight by construction — see PLAN Phase 3's coverage note).
- [x] Turn end comes from `state_update: idle{stopReason, usage}`.
- [x] `state_update: running` → busy. `requires_action` → waiting, does NOT end
      the turn.
- [x] Stop-reason strings match v1's exactly: end_turn→completed,
      max_tokens→max-tokens, max_turn_requests→max-turn-requests, refusal→blocked,
      cancelled→interrupted. Pinned by
      `the_five_spec_stop_reasons_land_on_the_words_the_ui_already_knows` +
      `cancelled_means_cancelled_and_nothing_else`, and a non-spec reason keeps
      the agent's own word (`TurnOutcome::Stopped{kind: Cow<str>}`).
- [x] `UiEvent::Usage` moves from `PromptResponse.usage` (v1) to `idle.usage`
      (v2). Pinned by `usage_reads_the_captured_idle_and_the_optional_counters`,
      which also folds `cachedReadTokens + cachedWriteTokens` and proves that
      omitting v2's REQUIRED `totalTokens` loses the whole usage object
      (`DefaultOnError`).
- [x] Cancel = `session/cancel` then WAIT for `idle{cancelled}` — `abort_turn`
      (`v2.rs:323`) sends `CancelSessionNotification` + `CtlEvent::CancelRequested`,
      and `Interrupted` only fires once the board settles (`:1373-1379`). The
      e2e test asserts `session/cancel` is on the wire AND that `interrupted`
      is drawn.
- [x] One foreground prompt per session. `Cmd::Steer` (`v2.rs:1073`) + `parked` +
      `requeue_parked_prompts` + `drain_ready_sessions` key off the board, not
      off request-future liveness. **⚠️ Implemented but NOT separately pinned** —
      no test steers a running v2 session. Cheapest gap to close.
- [x] Background updates after `idle` do not reopen a turn ("`idle` is not a wire
      boundary") — this is what the arm-before-send / disarm-on-delivery rule
      buys, and it is what makes agent2's 30 s heartbeat idle harmless. Pinned at
      the board level by `an_idle_for_a_session_nobody_armed_is_a_heartbeat`;
      **not pinned at the paint level** (criterion (e)).
- [x] The update handler is installed BEFORE `session/new`/`session/resume` —
      replay updates precede the resume response on the wire, and that ordering
      is the ONLY discriminator between a live echo and a replayed one
      (`ReplayWindow`). Unhandled v2 notifications are dropped silently; there is
      no per-session buffering.
- [x] `session/update` events carry `sessionId` + entity ids but NO prompt/turn
      id. No prompt attribution is possible; `state_update` is session-wide.

## `crow-cli acp2` interop facts (read out of agent2, not guessed) — all confirmed
- [x] **agent2 re-emits `idle` every 30 s for the life of a parked session**
      (`agent2/driver.py:455-463`). A repeated idle is a HEARTBEAT, not a
      turn-end: do not emit a second `TurnEnd`, do not re-settle the transcript,
      do not flip the composer. Only an idle that follows `running` ends a turn.
      Implemented as the arm-before-send / disarm-on-delivery board — the rule
      crow-cli itself lacks — and pinned by
      `an_idle_for_a_session_nobody_armed_is_a_heartbeat`.
- [x] **Idle means promptable, INCLUDING with a delegated task still running**
      (`driver.py:21-25`). Background work continues while idle and its updates
      do not change the state. A client that reads idle as "nothing happening"
      or non-idle as "not promptable" is wrong on both counts.
- [x] On cancel, `driver.stop()` ALREADY emits the cancelled idle
      (`agent.py:399-403`); the agent deliberately does not emit a second one.
      So the client's wait-for-idle after `session/cancel` terminates. Confirmed
      on the wire: `/tmp/wire-v2cancel.jsonl`.
- [x] `agent2/emitter.py:63-69 usage_model` ALWAYS fills `total_tokens`
      (falling back to `input + output`), which is why v2's required
      `Usage.total_tokens` does not bite us. Pinned by
      `v2_idle_usage_without_total_tokens_silently_vanishes` — now
      `usage_reads_the_captured_idle_and_the_optional_counters`.
- [x] v2's prompt response cannot carry `stop_reason="error"` the way v1's could;
      with no provider configured agent2 just goes idle with `end_turn`
      (`agent.py:553-567`). Error surfacing on v2 rides `SessionError`/notices,
      not the ack. (agent2 has been observed putting `stopReason:"error"` on the
      idle instead, which is why `TurnOutcome::Stopped{kind: Cow<str>}` exists.)
- [x] agent2 reads tool supply from `request.mcp_servers` ONLY, never from
      config; `mcp2` serves `execute` and takes no `--include-tools`. Pinned by
      `a_v2_agent_is_negotiated_and_its_tool_supply_carries_its_transport`, which
      also proves the v2 stack ran at all: v1's `McpServer::Stdio` is
      `#[serde(untagged)]` and has no `type` key, so requiring a `type` on every
      server would fail on a v1 `session/new`.

## events.rs discriminators — HALF (PLAN Phase 4)
- [x] `agent_message`, `user_message`, `agent_thought` — the whole-message
      upserts (`content` is a LIST; a chunk carries ONE block). Pinned by the 11
      new `events__tests.rs` cases, payloads verbatim from `/tmp/wire-v2g.jsonl`.
- [x] `state_update` — **deliberately `Vec::new()` at `events.rs:396`.** The v2
      stack owns it: `acp::v2` settles its turn board off the idle before the
      parser ever sees it, so emitting `SessionStatus`/`TurnEnd` here too would
      report every turn twice (and agent2 heartbeats an idle every 30 s).
- [x] `usage_update` → the context-window meter (`events.rs:574`), reading
      `used`/`size`.
- [~] `terminal_update` / `terminal_output_chunk` — **claimed, not painted**
      (`events.rs:403`), and that is the settled decision: terminal bytes have
      two audiences, the model's copy arrives as `tool_call_update.raw_output`
      which the tool cell already paints, and crow-term has no terminal pane.
      So the base64 rules below (decode each chunk independently, retain state
      across a split UTF-8/ANSI boundary, a later `output` snapshot replaces all
      accumulated bytes) are **moot unless a terminal pane ever ships.** Do not
      build a decoder for an audience that does not exist.
- [ ] ⚠️ `tool_call_content_chunk` is lumped into that same `Vec::new()` arm, and
      the comment justifies the *terminal* halves only. The real question is
      different: does the completed `tool_call_update` carry the whole `content`,
      making the streamed chunks redundant, or does dropping them lose tool
      output? **Settle it against a live capture and write the answer into the
      comment** — either way the arm needs its own sentence.
- [x] `plan_update` keyed by `planId` with `type:"items"` — the `plan`|`plan_update`
      arm at `events.rs:491` handles both spellings.
- [ ] Upsert semantics: omitted = unchanged, `null` = cleared, value = replaced,
      chunks append. A whole-message update REPLACES chunk-accumulated content.
      **Open — this is the per-`tool_call_id` patch path** (crow-cli keeps a
      `_tools` dict for exactly this).
- [x] Decided: **neither** a messageId-keyed patch path nor a blanket
      suppression. The double-paint was a *user-echo* problem and the fix is the
      `ReplayWindow` in `acp/v2.rs` — a `user_message`/`user_message_chunk` is
      forwarded only while a replay window is open for that session, because on
      `session/resume` every replayed update arrives BEFORE the resume response
      and that ordering is the only discriminator. `e4802c8`, pinned by four
      `ReplayWindow` unit tests + `a_v2_prompt_reaches_the_pane_once`, and
      mutation-checked three ways.
- [x] v2 has no `tool_call` create — the first `tool_call_update` for an unseen
      `toolCallId` CREATES the cell. Pinned.
- [ ] Read `ToolCallUpdate.name` from raw JSON (schema 1.5.0 has no `name`
      field; it is `unstable_tool_call_name` in 1.7.0). **Still open.**
- [x] `configId` (v2) alongside `id` (v1) — one reader, `config_id_of`
      (`events.rs:758`), tries `configId` → `config_id` → `id`, and every call
      site goes through it. Pinned by the resume test's
      `config["configId"] == "model"` / `config.get("id").is_none()`.
      (This was the empty-model-catalog bug: v2 says `configId`, the reader said
      `id`.) **Not fixed here:** `catalog_from_config_options` splits model ids
      on `/`, a pre-existing v1 assumption.
- [~] v2's `category` — **partly used.** `reasoning_effort_option` finds
      `category == "thought_level"` (`events.rs:819`) and `composition_option`
      excludes anything carrying a category (`:782`). `mode` / `model` /
      `model_config` are still sniffed by id. Tolerating unknown categories is
      the part that matters and it holds.
- [ ] `available_commands_update`: command `input` is now a tagged union
      (`{"type":"text","hint":…}`) — `skills_from_available_commands`
      (`events.rs:967`) reads both. **Open.**
- [ ] `coalesce_session_updates`/`merge_update` must not merge two v2 upserts
      (merging a replace into an append loses the replace). **Open.**
- [ ] `current_mode_update` handling becomes v1-only (`session_modes_from_value`
      at `events.rs:927` is already v1-only; the picker still needs a v2 path via
      `set_config_option` + `category:"mode"`). **Open — PLAN Phase 6.3.**
- [ ] **Unknown `sessionUpdate` discriminators must render as their KIND, never
      vanish** (crow-cli's `client2/main.py` rule: nothing is silently dropped).
      `parse_session_update` still ends `_ => Vec::new()`. **Open, and it is a
      standing ruling, not a nice-to-have.**

## Auth — OPEN (PLAN Phase 5). `login()` exists in `v2.rs`; `logout` does not.
- [ ] `authenticate` → `auth/login`; add `auth/logout`.
- [ ] `authMethods[]`: `id` → `methodId` + REQUIRED `type` (`"agent"`; custom
      types start with `_`). Read both spellings.
- [ ] Non-empty `authMethods` ⇒ agent MUST implement login AND logout, no logout
      capability marker. Omitted/empty ⇒ client MUST NOT call either.
- [ ] Capability reads become PRESENCE checks (`capabilities.session.prompt.image
      != null`), not booleans.
- [ ] Advertising `capabilities.session` at all ⇒ new/list/resume/close/prompt/
      cancel/update are REQUIRED — stop probing them on v2. `session.delete` and
      `session.additionalDirectories` keep markers.
- [ ] `prompt_image_supported`, `load_session_supported`,
      `resume_session_supported`, `list_session_supported` all need v2 branches.
- [ ] The `marttyConnection` error-data stall path, `failed_auth_setups`, and
      prompt parking work unchanged on v2. (The parking machinery itself is
      wired — `parked_prompt`/`requeue_parked_prompts`/`auth_stalled_for` are all
      called from `v2.rs` — it is the v2 auth *shapes* that are unbuilt.)

## Removed surface (v2 connections only — v1 keeps all of it) — PART (PLAN Phase 6)
- [x] No `fs/read_text_file`, `fs/write_text_file`; `acp_fs.rs` not registered.
- [x] No `terminal/create|output|release|wait_for_exit|kill`; `acp_term.rs` not
      registered. **Shipped as auth-only, not `{}`:** `negotiate.rs:157` builds
      `v2::ClientCapabilities::new().auth(v2::AuthCapabilities::new())` — v2's
      `ClientCapabilities` has only auth/elicitation/nes/position_encodings, so
      there is nothing to decline.
- [x] No `session/load` → `session/resume` + `replayFrom:{"type":"start"}`
      (`v2.rs:531`). Plain reconnect = `session/resume` with no `replayFrom`.
      Pinned by `a_v2_resume_repaints_the_transcript_the_agent_replays`, which
      asserts `session/resume` on the wire and `session/load` + `session/new`
      absent.
- [ ] No `session/set_mode`, no `modes` on session responses, no
      `current_mode_update`, no `SessionMode*` types → config options with
      `category:"mode"`.
- [x] `session/set_config_option` v2 shape: `(sessionId, configId, type, value)`,
      `type` = `"id"` | `"boolean"`; response returns the FULL updated array.
      One helper, `v2.rs:587`, called from all five sites (`:1216`, `:1228`,
      `:1244`, `:1297`, `:1324`) — model select, efforts, presets and
      `Cmd::SetConfigOption`.
- [ ] Diffs: `changes[]` (add/delete/modify/move/copy + fileType + mimeType) plus
      optional `patch{format:"git_patch", text}` with ABSOLUTE paths and no commit
      metadata. Render `patch.text`; drive trees/summaries from `changes`; handle
      a patch-less diff. **No mechanical mapping back to oldText/newText — don't try.**
- [~] Permissions: REQUIRED `title` (prompt copy) + optional `subject` union
      (`tool_call` | `command`). **The ask and the answer work and are pinned** —
      `a_v2_permission_ask_reaches_the_user_and_the_answer_reaches_the_agent`
      draws the title plus both option names and kinds, Enter takes the first
      `allow_once`, and the reply on the wire is
      `{"outcome":{"outcome":"selected","optionId":"allow"}}`. Handler at
      `v2.rs:838-900`. **Still open:** rendering the `command` subject's
      `command` + absolute `cwd` (the stub sends one; the test does not assert it
      is drawn), applying a `tool_call` subject as an ordinary upsert, and the
      v1 path at `src/acp.rs:1653` still reads the tool call's `title` as the
      prompt text. Approval authorizes the AGENT to execute; it never asks the
      client to. Unknown outcome ⇒ MUST NOT be treated as approval.
- [x] MCP config: every server carries a REQUIRED `type` (`"stdio"` | `"http"`);
      SSE is gone; `args`/`env`/`headers` now optional. `mcp_supply.rs` grew
      `wire_servers_v2()` (`:138`) next to v1's `wire_servers()` (`:134`), and
      `from_settings_json` (`:164`) maps `("sse",url)`→Sse, `(_,url)`→Http, else
      Stdio requiring `command`. **This was a real panic**: v1's
      `McpServer::Stdio` is `#[serde(untagged)]` (no `type` key) and v2's is
      internally tagged and *requires* `{"type":"stdio"}`, so handing v1 servers
      to a v2 `session/new` blew up and stalled the pane at `starting runtime`
      with no error anywhere. Pinned by the tool-supply e2e test.
- [ ] ID renames: auth `id`→`methodId`, config option `id`→`configId`, select
      group `group`→`groupId`. Payload type names: `UpdateSessionNotification`,
      `CancelSessionNotification`, `LoginAuthRequest/Response`, `LogoutAuthRequest/Response`.

## Harness / config / JS layer

**⭐ THE USER'S STANDING SECONDARY MANDATE — BUILT, LIVE-VERIFIED, PINNED.**
> "okay that's what I was thinking. we don't have /harness. we need to implement
> a way to pick inside TUI."

`b3e129d` + `761f7a5`. `/harness` is now a Rust builtin: no argument opens a
picker over the configured recipes (meta column = the argv, `· active` on the
current one, selection snapped to it), `/harness <id>` switches directly. It
persists `defaultHarness` and **respawns the connection through the union
probe**, so switching harness also switches protocol. Proven on a real PTY
against two stub harnesses: `wire-v1.jsonl` freezes at 2 frames, `wire-v2.jsonl`
gains `initialize` + `session/new` + `session/prompt`, and the two `session/new`
requests carry the same MCP supply **untagged** on the v1 side and
`"type":"stdio"` on the v2 side — a different stack, not a different process
only. Both directions (v1→v2 and v2→v1) verified.

**How the respawn works** (the hard part, and it was not the swappable `cmd_tx`
this note predicted): `Cmd::SwitchHarness{argv}` is intercepted by a **relay
thread inside `run_blocking`** — the only code holding both the painter's
`Receiver<Cmd>` and the endpoint being replaced. Dropping the relay's sender
tears the stack down through the *existing* channel-close path (both command
loops already `break` when `recv()` yields nothing, the same exit as
`Cmd::Shutdown`), and the receiver goes back through a second channel so the
supervisor loop respawns on `AcpEndpoint::Spawn(argv)`. **Neither command loop
needed a new code path**; each got a loud `refuse_harness_switch` arm so a future
direct wiring fails visibly instead of falling into the version-specific
catch-all. `Controller::start_acp` and `&Controller` were never touched.

**⚠️ The bug this uncovered, and it was mine.** One tokio runtime now serves
every generation, so the *detached* `tokio::spawn(driver)` in
`negotiate_and_run` outlived its connection holding the transport — and the
transport holds the child guard. **One leaked agent process per `/harness`.**
Before the supervisor loop the runtime was dropped on return, which killed the
task, so this was new. Fixed by keeping the `JoinHandle` and aborting it on
every path out (including a probe that never got an answer), the way
`check_blocking` already does. The e2e pins it via a pid file the recipe writes
and `/proc/<pid>/stat` — a zombie counts as gone. **Lesson: making a runtime
long-lived turns every detached task in it into a leak.**

**✅ THE NAME COLLISION IS DECIDED — there never was one.** `/harness` also
exists as a **Node client plugin** (`npm/lib/harness-view.js:540`,
`ctx.tuiCommands.register({name:'harness', …})`, input
`[id] [--new] | add | remove [id] | find [query]`), and `App::slash_matches`
(`src/app.rs:3200-3207`) filters plugin commands whose name collides with a
builtin, so the new builtin shadows that overlay. This was filed as an open
decision on the assumption the Node host was a live surface worth preserving.
**It is not — `npm/lib` is dead code and never executes.** The product is the
Rust binary, run directly: no Cordis host, no plugin tree, no agent pool, no
node anywhere. So the builtin owns `/harness`, there is no overlay to preserve,
no "host-overridable builtin" category to invent, and nothing to rename. The
registry catalog / downloads / install / removal that plugin offered were never
reachable in this product; if harness installation is ever wanted it gets built
in Rust. Recorded in `AGENTS.md` under "⛔ READ THIS FIRST — this is crow-term,
not Martty" so no future agent re-derives the Node layer as live.

**The originating hang bug, for the record:** the host's switch calls
`ctx.acpClient.setDefaultAgent(...)` and `npm/lib/acp-client.js` is v1-only, so
switching to a v2 harness through the npm host hangs. The Rust builtin does not
have that problem — it re-negotiates.

Scope shipped: pick among configured harnesses and respawn. The registry
catalog, downloads, install and removal stay in Node.

- [x] Ship the real target as a harness entry:
      `uv --project /home/thomas/.agents/crow/src/crow-cli run crow-cli acp2`
      with `mcpServers: {crow-mcp2: {transport: stdio, command: …/.venv/bin/crow-cli, args: [mcp2]}}`
      (the working `v2-agent` entry in `~/.agents/crow/config.yaml`).
      **Done differently and better:** `~/.agents/crow/settings.json` now carries
      `crow-cli` (`["acp"]`) and `crow-cli-v2` (`["acp2"]`, label
      `crow-cli (ACP v2)`), with `defaultHarness` back on `crow-cli`. Because the
      protocol is NEGOTIATED, the entry needs no `protocol:` field — same shape,
      different argv. Backup at `/tmp/agents-crow-settings.json.bak`.
- [x] `mcp2` takes NO `--include-tools`; agent2 reads tool supply from
      `request.mcp_servers`, never from config. A `--config-file`'s `mcpServers`
      is inert for agent2.
- [ ] Badge the negotiated version (`acp` vs `acp2`) via the existing
      `src/harness_badge.rs` / `npm/lib/harness-badge.js` tag, using
      `Protocol::tag()`. **Open — PLAN Phase 2.5.**
- [ ] `CtlEvent::Initialized{server}` carries the negotiated version. Only three
      sites: `bus.rs:98`, `acp.rs`, `app.rs:3861`. **Open — PLAN Phase 2.5.**
- [ ] DECIDE + WRITE DOWN: does `npm/lib/acp-client.js` / `acp-host.js` /
      `acp-agent-pool.js` / `acp-session-*.js` / `acp-registry.snapshot.json`
      need v2 in this sprint, or stay v1 and merely LIST a v2 agent's argv for
      the Rust binary to negotiate? (crow-term's Rust client is the product
      surface; the JS layer is the harness manager.)
- [~] `src/harness.rs`, `harness-discovery.js`: a v2 agent is discovered and
      launchable like any other (it is — `--check-runtime` proves the whole path).
      **Switchable-in-Rust is now DONE** (the ⭐ block above). Badged is the
      remaining half — PLAN Phase 2.5 — and it **must be driven from Rust**: the
      badge is fed from a Cordis slot snapshot (`conversation.harness`), and
      nothing sends one, so `app.harness_badge` is permanently empty today.
      `harness-discovery.js` / `harness-badge.js` are dead code, not a source.
- [x] **Rebrand gap — MOOT, not unfixed.**
      `npm/lib/tui-plugin-store.js:79-87 marttyHome()` still resolves
      `MARTTY_HOME` → `$DSH_HOME/.martty` → `~/.martty` and has never heard of
      `CROW_HOME`, so `tuiPluginRoot()` would put plugins in `~/.martty/plugins`.
      **It never runs** — `npm/lib` is dead code (see ⛔ READ THIS FIRST in
      `AGENTS.md`). Do not fix it; there is nothing to fix.
      The Rust side is correct: `src/runtime.rs` walks `CROW_HOME` →
      `MARTTY_HOME` → `$DSH_HOME/.agents/crow` → `~/.agents/crow`, and
      `~/.martty/settings.json` survives only in `legacy_settings_paths_from()`
      ("Settings files this build no longer writes but must still read").

## Tests — the mandate, DONE

Suite, all green: **918 unit + 2 cli_help + 1 sigterm_cleanup + 12
startup_session_e2e + 1 tcp_attach**. Two pre-existing warnings only
(`ui__tests.rs:3591` unused `ctl`, `input__vim__tests.rs:74` snake_case).
⛔ **That is the whole gate.** The JS suite (11 pretest + 434 main) tests
`npm/lib`, which is dead code that never executes — it is not part of product
health and its totals are no longer tracked here.

- [x] Phase 0: `tests/unit/acp__v2_wire_tests.rs` (305 lines, `25254c1`) — the v2
      `InitializeRequest` wire keys, one deserialization fixture per v2
      `SessionUpdate` variant, and the pin that `name` survives in raw JSON but
      not in the 1.5.0 struct, so a future dep bump goes red.
- [~] Phase 1: **obsolete** — there was no split, so no `mod` paths moved.
- [x] Phase 2: `tests/unit/acp__negotiate_tests.rs` (11 tests) — two scripted
      peers over ONE `Channel::duplex()` (not a factory; the connector was
      rejected). A v1-only peer that answers `protocolVersion: 1` (**must answer
      1, not echo 2** — crow's settled ruling) lands on the v1 stack; a v2 peer
      lands on the v2 stack. Plus a banner before the adopter, a peer-opened
      request not eaten by the probe, a batched answer split not dropped, a
      refusal in the agent's own words, an unspoken version refused by number, a
      missing version refused, a hang-up as an error not a hang, a silent peer
      timing out, and id-zero reuse.
- [~] Phase 3: (b) and (c) pinned end-to-end on a real PTY; the board pinned in
      unit tests. **(d) steer-while-running and (e) post-idle chunk are
      implemented but NOT pinned** — see PLAN Phase 3's coverage note.
- [~] Phase 4: 11 new `events__tests.rs` fixtures, payloads verbatim from
      `/tmp/wire-v2g.jsonl` / `-v2resume.jsonl`; `configId` populates the model
      catalog (pinned through the resume e2e). The mid-UTF-8 terminal decode is
      **moot** (terminal chunks are claimed, not painted). A whole-message update
      after chunks painting once is pinned for the USER echo (`ReplayWindow`);
      the agent-message half rides the same gate.
- [ ] Phase 5: v2 auth fixtures — one `type:"agent"` method drives the overlay to
      a successful `auth/login` then resumes the parked prompt; empty
      `authMethods` emits no CTA; `prompt_image` set from the presence check.
      **Open — `login()` exists in `v2.rs`, `logout` does not.**
- [~] Phase 6: resume-with-replay paints then idles ✅ and a v2 permission
      renders its `title` ✅ (both e2e). The `fs/read_text_file`
      method-not-found pin, the mode picker via `set_config_option`, and the
      `command`-subject render are **open**.
- [x] Phase 8: done against the STUB, deliberately — a live `crow-cli acp2` run
      burns model tokens, needs `alibaba` reachable, and cannot run in CI. Every
      shape the stub sends was copied from a real capture, so it is a recording
      of the live agent, not an invention. Live verification over a teed PTY was
      done separately (`/tmp/wire-v2g.jsonl` and friends).
      **8.1 (`scripts/real-agent-e2e.py`) is ⛔ OBSOLETE, not blocked:** that
      runner drives `dsh --profile tui-test` through pexpect — the NODE host —
      which does not exist in this product. It cannot be unblocked by giving the
      JS layer v2, because the JS layer never runs (Phase 7.3). The equivalent
      coverage is `tests/startup_session_e2e.rs`, which drives the shipped Rust
      binary on a real PTY. Delete the script, its `real-agent-e2e.test.mjs` and
      `make real-agent-e2e` when the npm packaging question is settled.
- [x] **The harness picker: 14 new tests, three rungs** (`761f7a5`). Not a
      numbered phase — it was the standing secondary mandate.
      * `tests/unit/acp__harness_relay_tests.rs` (4) drives `relay_commands` with
        plain channels, because everything load-bearing about it IS a channel
        behaviour: what it forwards, what it intercepts, and how it tells a
        switch from a shutdown. `bounded_join` exists so a relay that never exits
        (the bug under test — it would hold `switch_tx` and block the controller
        thread forever) fails in 5 s instead of hanging the suite.
      * `tests/unit/app__harness_tests.rs` (9) drives a real `App` + `Controller`
        against a temp `CROW_HOME`: the picker lists the recipes and marks the
        active one, an empty list explains itself instead of opening, an unknown
        or already-active id changes nothing, a switch persists and forgets what
        the dead connection was delivering, and the three settings-file shapes —
        patch-not-rewrite, create-if-absent, quarantine-if-unparseable.
      * `tests/startup_session_e2e.rs` gained
        `a_harness_switch_respawns_the_agent_and_the_new_one_answers` (11 → 12),
        which boots with **no `--agent` at all** so the binary resolves the
        harness from `settings.json` like a bare launch, then proves the switch
        end to end: the new wire log gains `initialize` + `session/new`, the
        `crow-mcp` stdio entry loses/gains its `"type":"stdio"` tag (v1 untagged,
        v2 tagged — the protocol really changed), `defaultHarness` tracks, the
        other recipes and `mcpServers` survive, the replaced agent process is
        **gone**, and the next prompt is answered by the new one.
- [x] No mocks where a real scripted peer will do. The e2e tests drive the
      **shipped binary** on a real PTY; the unit tests drive in-process SDK
      agents (`Agent.builder().on_receive_request(…)`) or a `Channel::duplex()`
      peer.
- [x] **Three mutation checks** — every bug-fix assertion was made to fail on
      purpose: `ReplayWindow::forwards → true` reproduced the reported double
      print verbatim (`left: 2, right: 1`); `forwards → !is_user_echo` broke only
      the resume test (`never drew ["an older prompt"]`); dropping the neutral
      lane reproduced the verbatim `AgentsSnapshot … is not supported on a v2
      connection` flood. All reverted, `git diff --stat src/` empty after each.
      **An assertion never seen to fail is a comment, not a test.**
- [x] **Nineteen more mutation checks for the harness picker** — every new
      assertion was seen to fail, and each mutant was caught by exactly the test
      that owns it (no blanket failures, which would mean the tests overlap
      instead of partitioning).
      * Relay, 5 mutants on `relay_commands`: no `Starting{runtime:"harness"}`
        emit; no interception at all; a dead receiver handed back; a shutdown
        delivered as a switch; a `relay_tx` send error ignored.
      * App, 11 mutants: no row marked active; no `sel` snap; the already-active
        guard removed; no `clear_delivery_state()`; persist rewriting instead of
        patching (→ 3 tests fail, correctly); no quarantine; an empty list still
        opening a picker; the id not trimmed; no tip on a fresh pane; tipping
        unconditionally; no persist before the respawn.
      * E2E, 3 mutants on `src/acp.rs`: the supervisor not respawning (fails at
        `wait_for_frames`); the next generation getting a dead receiver (fails as
        a **hang**, `⠼ contacting runtime 29s`, not a crash — which is why
        `bounded_join` and the wire-log deadlines exist); and the driver left
        detached, which reproduced **BUG 1** deliberately and failed at
        `assert_agent_gone`.
      All reverted; `git diff --stat src/` empty after each.

### Test-harness facts worth keeping
- [x] `tests/fixtures/stub_acp_agent.py` is dual-stack: `STUB_PROTOCOL=2` answers
      the union initialize as v2, `STUB_CAPS=resume|load|both` picks the session
      capability, `STUB_LOAD_FAIL=1` refuses, `STUB_HOLD=1` parks a turn
      mid-flight so a cancel has something to interrupt, `STUB_ASK_PERMISSION=1`
      sends a v2 `session/request_permission` with a `command` subject. Its
      `replay_v2(sid)` emits all five updates BEFORE the resume response, which
      is the only discriminator the client has.
- [x] **Validate a Python fixture with `python3 -m py_compile`, NEVER
      `ast.parse`.** `ast.parse` compiles with `PyCF_ONLY_AST` and skips the
      symtable pass, so `global X` after a module-level `X = …` sails through
      `ast.parse` and is a hard `SyntaxError` at runtime. That is exactly how the
      stub was broken.
- [x] `tests/startup_session_e2e.rs` grew `Pane::send(keys)` (the harness could
      previously only READ the PTY), `Pane::request_params(method)`, and
      `launch_with(tag, args, stub, settings)` which writes a `settings.json`
      into the temp HOME. **The settings file is load-bearing:** without
      `mcpServers`, `mcp_supply::load()` falls through to the developer's real
      `~/.agents/crow/config.yaml`, making any `session/new` assertion
      machine-dependent.
- [x] `tests/startup_session_e2e.rs` after the picker work: `e2e_home(tag)` is
      the ONE spelling of a test's temp home, so a test that seeds
      `settings.json` and a recipe that points at a wire log cannot diverge;
      `spawn_pane(tag, args, settings, stub)` is the single spawn (it only adds
      `--agent python3 --agent-arg FIXTURE` when `stub.is_some()`);
      `launch_from_settings(tag, args, settings)` boots with **no `--agent`**, so
      the binary resolves the harness from settings exactly like a bare launch;
      `Pane::log(name)` / `Pane::log_request_params(log, method)` generalise
      `wire()` to a second log; `wait_for_frames(pane, log, want)` polls with a
      30 s deadline and panics with `diagnosis()` (and panics early if the child
      exited); `assert_agent_gone(pid_file)` reads `/proc/<pid>/stat` and counts
      `Z` as gone.
- [x] **A harness entry cannot carry `env`** — `AcpAgent::from_args(argv)` has no
      env support and `main.rs agent_argv()` uses only `harness.argv()`, so the
      `env` map is dropped by the Rust binary today (the npm host applies it via
      `harnessEnvironment`). Every test recipe that needs `STUB_LOG=…` therefore
      has to be a `/bin/sh -c "… exec python3 FIXTURE"` wrapper. `$$` still gives
      the right pid because `exec` replaces the shell.
- [x] `assert_agent_gone` parses `/proc/<pid>/stat` with `rsplit_once(')')`, not
      `split_whitespace` — field 2 (`comm`) can contain spaces and parens.
- [x] **`json!` is not const-evaluable.** `const TWO: serde_json::Value =
      json!({…})` is E0015 + E0493 (ten of the 69 errors came from that one
      line). Write `fn two() -> serde_json::Value` instead.
- [x] `PickerKind` and `AppEvent` have **no `Debug`**, so `assert_eq!` on either
      is E0277. Use `assert!(matches!(picker.kind, PickerKind::Harness))`, and
      map `AppEvent` to a `String` before comparing (`Relay::said()`).
- [x] `TIP_TTL` is **4 s** (`src/app.rs:45`). A PTY driver that waits 5 s in one
      gulp will never see a tip; the e2e's 25 ms poll does. This is also why the
      switch confirmation had to be a transcript notice *and* a tip — see BUG 2.
- [x] **Never run repo-wide `cargo fmt`.** There are 116 pre-existing rustfmt
      markers in `acp.rs`, 260 in `app.rs`, 91 in `ui.rs`. Format only files that
      are entirely new; for existing files cross-reference `rustfmt --check`
      diff line numbers against `git diff -U0` added ranges (naive line matching
      false-positives on `#[path]`-included test files — filter on the absolute
      path). `chain_width` defaults to 60, so a 61-char method chain MUST stay
      split and a 66-char one must not be joined.
- [x] ~~`cd npm && npm ci --no-audit --no-fund` before trusting ANY JS result~~
      **Moot.** The JS suite tests dead code; there is no JS result to trust.
      Do not install `npm/node_modules` to validate a crow-term change.

## Docs — OPEN (PLAN Phase 10)
- [ ] `docs/acp-v2.md` (new): the rulings, the seams, the turn-lifecycle
      difference, what was removed, how to add a v2 harness, and why v2 matters
      beyond parity (multi-client observe / event-driven orchestration).
- [ ] `docs/architecture.md` + `.en.md`: the `acp/negotiate.rs` + `acp/v2.rs`
      seams and the union probe. **NOT an `acp/v1.rs` split — that file does not
      exist and never will** (see Rust client structure above).
- [ ] `docs/harness-management{,.en}.md`, `docs/harness-cli.md`, `docs/harness-tui.md`:
      negotiated-not-declared, i.e. **a v2 harness is an argv, not a flag.**
      There is no pin override to document.
      **The new `/harness` builtin needs documenting here too** — unblocked: the
      name collision is decided (✅ under "Harness / config / JS layer" above),
      the Rust builtin owns `/harness`, and the `npm/lib/harness-view.js` overlay
      is dead code. Document the builtin as the only `/harness` there is; do not
      document the Node overlay as a live surface.
      ⚠️ `docs/plugins.md` and `docs/migration.md` describe the dead
      Cordis/plugin architecture. Mark them historical the way `AGENTS.md` now
      does, or an agent that reads them as current will design against a host
      that does not exist.
      ⚠️ These are **Chinese-first** docs — `harness-tui.md`, `harness-cli.md`,
      `harness-management.md`, `architecture.md`, `migration.md`, `plugins.md`,
      `sessions.md`, `README.md`. Read and write them in Chinese directly; do not
      hunt for the `.en.md` and do not assume the English one is the source.
- [~] `CHANGELOG.md` `[Unreleased] ### Added`. The `/harness` picker entry is
      written; the v2 negotiation entries are not.
- [ ] No doc claims a behavior the tests do not pin.

## Deferred (with reasons)
- [~] Dep bump to 2.1.0 — Phase 9.1. Costs two features we use; the raw-JSON
      `name` read makes it unnecessary for parity.
- [~] The `crow-cli-rs` git-rev dep (`rust-sdk` rev `7d21931`) — Phase 9.2.
      crow-term ships via npm + a static ELF; a git dep breaks `--locked`.
- [~] Multi-client observe / verifier attaching to the same `sessionId` —
      Phase 9.3. The real prize of v2, but out of scope for parity.
- [~] JSON-RPC batch arrays on stdio — Phase 9.4. Check whether the SDK transport
      already handles a batch line; if not, file upstream rather than patch locally.
- [~] The remote transport RFD (streamable HTTP + SSE / WebSocket) — not part of
      the core v2 surface. `agent-client-protocol-http` exists if it ever matters.
- [~] Reviving `crow-cli-rs` as a product — abandoned by decision; mine it only.

## crow-cli execute tool / ACP v2 terminal streaming
- [ ] **Make the `execute` tool comply with the ACP v2 terminal specifications.**
      Today the persistent-kernel cell returns its whole stdout in one shot when
      the cell finishes. That is wrong: `execute` is supposed to *stream* kernel
      output as it is produced, on a **separate channel from what the LLM sees** —
      the client/terminal gets the live stream (v2 `session/update` terminal /
      tool-output chunks), the model gets the settled result. Wire the kernel's
      output through the v2 streaming path instead of buffering to a single
      tool-result payload.

## Long-session render perf — mirror crow_cli.tui's conversation window
Symptom: the composer lags badly as a session grows. Cause (measured, release,
100 cols, warm markdown cache): `ui::draw_chat` calls `Transcript::layout()`
every frame, which walks **every** cell and `l.clone()`s **every** cached body
line into a fresh `Vec<Line<'static>>` (transcript.rs:1375/1422/1484/1517)
*before* slicing the ~40-row viewport. `AppEvent::Term(_) => true` in
`event_requires_immediate_frame` (main.rs:277) sets `frames.immediate`, so every
keystroke bypasses the 33 ms pacer and pays the full O(total-lines) clone.

  25 turns /    75 cells /   1,200 lines ->  0.42 ms/frame
 100 turns /   300 cells /   4,800 lines ->  1.8 ms/frame
 400 turns / 1,200 cells /  19,200 lines -> 13.7 ms/frame  (~73 fps ceiling)
1600 turns / 4,800 cells /  76,800 lines -> 67 ms/frame    (~15 fps)
 300 turns of fat `read` output, 900 cells / 123,300 lines -> 24.7 ms/frame

Dead linear, ~0.2 us/line, no asymptote. Cold layout after a **resize** is far
worse (785 ms at 76.8k lines) because width is in the `CellRender` key, so a
resize re-runs markdown+syntect for the whole session. There is **no cap**:
`Transcript.cells` only grows; the sole reset is `/clear`.

### How crow_cli.tui does it (the thing to mirror)
`src/crow_cli/tui/widgets/conversation.py`:
1. **Retained widgets, not immediate-mode re-materialization.**
   `Window(VerticalScroll) > ContentsGrid > Contents(VerticalGroup)`; every
   message/tool-call/terminal is a mounted child (`post()` -> `contents.mount`).
   Textual's compositor renders only children intersecting the viewport, so
   paint is O(visible) *always*. There is no "walk every block per frame" step.
2. **Hysteresis prune at block granularity** — `check_prune()` / `prune_window()`,
   fired from `post()` via `call_after_refresh` (never from render):
   - `ui.prune_low_mark` default **1500** rows (target, min 100)
   - `ui.prune_excess`   default **1000** rows
   - high_mark = low + excess = **2500**: below it, `prune_window` returns
     immediately. Above it, walk children oldest-first accumulating outer
     heights *with margin-collapse math*
     (`prune_height = prune_height - bottom_margin + max(bottom_margin, top) + bottom + child_height`)
     and stop as soon as `height - prune_height <= low_mark`, then
     `remove_children(prune_children)`. The 1000-row band is the amortization:
     one prune per ~1000 new rows, not per frame.
3. **Re-anchor after pruning**: `cursor_offset = -1`, `cursor.visible = False`,
   `cursor.follow(None)`, `contents.refresh(layout=True)`, remove, then
   `call_later(self.window.anchor)` so the view sticks to the bottom instead of
   jumping when 1000 rows vanish above it.
4. **`/crow:clear [n]` is just `prune_window(n, n)`** — clear is not a special
   case, it is the prune with a zero/low target. `low_mark == 0` drops everything.
5. **Tool calls are NOT hidden to buy perf.** `ToolCall.expanded` defaults False
   but `check_expand()` auto-expands per `tools.expand` (default **`fail`**:
   failed calls open themselves; choices never/always/success/fail/both). Sole
   exception: `kind == "read"` never auto-expands ("can generate a lot of
   noise"). Collapsed content is `display: none`
   (`&.-expanded #tool-content { display: block }`) so a collapsed block costs
   **zero** layout height. The header always renders:
   `▶/▼ 🔧 <title>` + status (`⌛` pending / `✔` completed / `failed` pill).
   You always see that a tool ran, what it was, and whether it worked.

### Plan (both fixes; neither reduces visibility)
- [x] **P1 — viewport-only layout** — DONE (the retained-widget equivalent; the real
      fix). Add a universal per-cell row-count cache `Cell::rows` keyed on the
      same inputs as `CellRender` (version, width, theme, tone, expanded,
      thumbs, locale), *recorded by the emitter itself* (`rows = out.len() - first`)
      so measure and paint cannot drift. Refactor `layout()` into
      `layout_cells(range, base_line)` — the match arms stay byte-identical, only
      the loop gains a range filter and `base_line` offsets for
      `owners`/`users`/`images` (they already carry absolute line indices).
      `measure()` walks cells using warm counts and lays out only cold ones
      (i.e. the newly-mutated tail). `draw_chat` then: measure -> resolve
      `[start,end)` exactly as today -> binary-search the cell range ->
      `layout_cells(range, start)`. Per-frame cost becomes O(visible rows),
      constant forever. `layout()` stays as `layout_cells(0..n, 0)` so every
      existing test is untouched.
      Invariant test: `sum(cell rows) == layout().lines.len()` across widths and
      a fixture transcript covering every `CellKind`, expanded and collapsed.
      Landed as `windowed_layout_matches_the_full_layout_slice` (5 widths x
      collapse x thumbs x 11 window offsets x 5 lengths, byte-compared against
      the full layout), `windowed_layout_materializes_only_the_window` and
      `measure_cache_survives_mutation_and_resize`. Measured (release, 100 cols,
      40-row window at the tail):

          25 turns /    75 cells /  1,175 rows ->  9.99 us/frame  (was  316 us,   32x)
         100 turns /   300 cells /  4,700 rows -> 10.34 us/frame  (was 2.44 ms,  236x)
         400 turns / 1,200 cells / 18,800 rows -> 11.07 us/frame  (was 11.9 ms, 1073x)
        1600 turns / 4,800 cells / 75,200 rows -> 16.15 us/frame  (was 57.0 ms, 3528x)

      Flat: 10 -> 16 us across a 64x growth in session size.
      Still open: `App::jump_to_user_prompt` (app.rs:5380) does a full `layout()`
      on every ↥ click to get `layout.users`. It is a click handler, not a frame
      path, so it is a one-off hitch rather than the reported lag — derive it from
      the prefix sum (`line = sums[ci] + 1`, `end = sums[ci + 1]`) when convenient.
- [x] **P2 — hysteresis prune** — DONE. `Transcript::prune_oldest(rows)` drops
      whole cells oldest-first until the laid-out height is back at or under
      `PRUNE_LOW_ROWS` (14,000), fired only above `PRUNE_HIGH_ROWS` (15,000).
      The 1,000-row band is the amortization, exactly as in `prune_window`.
      Deliberate departures from the original sketch:
      * **15,000 / 14,000, not crow-cli's 2,500 / 1,500.** P1 made height free
        at paint time, so this bounds *memory and the resize rebuild*, not frame
        cost — there is no reason to throw away scrollback at 2,500 rows when a
        75,000-row session already paints in 16 us. Lots of scrollback, bounded,
        not ludicrous.
      * **Constants, not settings keys.** crow-term's `UiSettings` (locale.rs:117)
        is flat camelCase with no nested `ui` block; adding one for two numbers
        nobody has asked to tune is not worth it. Promote to
        `~/.agents/crow/settings.json` if that changes.
      * **Fired from `draw_chat`, not from `apply()`.** The cut is computed from
        the prefix-sum table, and the only place the laid-out height is known is
        the frame that just measured it (`Transcript` carries no width/theme of
        its own). crow-cli is the same shape — `check_prune` runs from
        `call_after_refresh`, not from `post()`. The hysteresis band makes it
        once per 1,000 new rows.
      * **`/clear` stays its own thing.** Folding it into `prune(0, 0)` would
        mean routing a reset through the settled-cell gate, which exists to
        *protect* in-flight work. `/clear` is allowed to drop everything.
      Gate: pruning shifts every cell index and bumps `gen`, so it refuses to
      run while an index is outstanding — `Transcript::settled_prefix()` caps the
      cut below the first unsettled cell (`Tool { ok: None }` /
      `Shell { output: None }` / streaming `Assistant`|`Reasoning`), and
      `App::prune_scrollback` additionally waits for `pending_steer_cells` and
      `shell_pending` to drain. The bound is soft by design: the transcript runs
      a little past the high mark until they settle. Re-anchoring:
      `chat_view.manual_top` shifts up by the removed rows (absolute),
      `scroll_up` is already bottom-relative so it is untouched, and
      `prompt_jump_cell` is dropped the way crow-cli drops its block cursor.
      The re-measure after a cut is cheap — `Vec::drain` moves `Cell`s intact, so
      the survivors keep their `rows`/`render` caches; only the prefix-sum table
      is rebuilt. Tests: `prune_drops_the_oldest_rows_and_keeps_the_tail_intact`
      (the surviving tail is byte-identical to `before.lines[removed..]`, owners
      rebase by the cell delta, prompt spans rebase on both axes),
      `prune_never_removes_an_unsettled_cell`, `prune_rebases_the_indexes_that_point_into_the_tail`
      (`tools` / `plan_cell` — a late `ToolResult` updates in place instead of
      appending an orphan), `prune_bumps_the_generation_so_stale_handles_no_op`,
      `prune_is_inert_below_the_high_mark`, plus the app-level
      `prune_scrollback_waits_for_outstanding_cell_indexes`,
      `prune_scrollback_shifts_the_scroll_anchor_and_drops_the_jump_cursor`,
      `prune_scrollback_is_inert_below_the_high_mark` and the end-to-end
      `draw_prunes_scrollback_past_the_high_mark_and_keeps_the_tail` (a real
      frame prunes, the newest prompt is still painted, the pruned head is not,
      and the next frame inside the band does nothing).
      Still open: **parked/subagent transcripts are not pruned** — they are never
      drawn, so `measure` never runs on them and the bound never reaches them.
      They cost memory only, not frame time. Prune them on park, or on a timer,
      if long multi-agent sessions start to hurt.
- [ ] **P3 — do NOT make `collapse_all` the perf lever.** crow-term defaults
      `expanded: true` for Tool/Reasoning (transcript.rs:156 — "a wall of `▸`
      chevrons makes every turn a clicking exercise"); that is *more* visible
      than crow-cli's default and P1 makes it affordable. Keep it. Optionally
      mirror crow-cli's `kind == "read"` no-auto-expand as a *setting*, off by
      default — never as the fix.
- [ ] **P4 — stop forcing `immediate` for plain composer character input**; let
      the 33 ms pacer coalesce held keys. Perf hygiene, not the fix.
