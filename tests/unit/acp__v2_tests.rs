//! The v2 stack's own invariants: the stop-reason vocabulary, the usage shape,
//! and the turn board that tells a turn-end from a heartbeat.
//!
//! Everything here is pinned against a live `crow-cli acp2` capture. The board
//! rules are the part crow-cli's own v2 client does not have: agent2 re-emits
//! `idle` every thirty seconds for a parked session, so an idle is only an
//! answer when a turn is waiting for one.

use super::*;

/// A stop reason exactly as it arrives on the wire, then as the UI words it.
fn kind_of(reason: serde_json::Value) -> String {
    let parsed: v2::StopReason = serde_json::from_value(reason).expect("stop reason parses");
    stop_kind(&parsed).into_owned()
}

#[test]
fn the_five_spec_stop_reasons_land_on_the_words_the_ui_already_knows() {
    assert_eq!(kind_of(json!("end_turn")), "completed");
    assert_eq!(kind_of(json!("max_tokens")), "max-tokens");
    assert_eq!(kind_of(json!("max_turn_requests")), "max-turn-requests");
    assert_eq!(kind_of(json!("refusal")), "blocked");
    assert_eq!(kind_of(json!("cancelled")), "interrupted");
}

/// agent2 puts `stopReason: "error"` on the idle when the provider fails,
/// because v2's `PromptResponse` cannot carry it. v1's `StopReason` has no
/// extension arm, so this used to render as `turn ended: unknown` — a word the
/// user cannot act on, and indistinguishable from a reason nobody implemented.
#[test]
fn a_stop_reason_outside_the_spec_keeps_the_agents_own_word() {
    assert_eq!(kind_of(json!("error")), "error");
    assert_eq!(kind_of(json!("some_future_reason")), "some_future_reason");

    // `Other` is the arm that carries it, and it is untagged: the wire spelling
    // is a bare string, not `{"other": "error"}`.
    let parsed: v2::StopReason = serde_json::from_value(json!("error")).unwrap();
    assert!(matches!(parsed, v2::StopReason::Other(reason) if reason == "error"));
}

/// Only `cancelled` reads as an interrupt. A provider error is a failure the
/// transcript should show, not a cancellation the user asked for — and
/// `TurnOutcome::cancelled` is what decides whether the pane says
/// "interrupted — turn cancelled".
#[test]
fn cancelled_means_cancelled_and_nothing_else() {
    let stopped = |reason: v2::StopReason| TurnOutcome::Stopped {
        kind: stop_kind(&reason),
        usage: None,
    };
    assert!(stopped(v2::StopReason::Cancelled).cancelled());
    assert!(!stopped(v2::StopReason::EndTurn).cancelled());
    assert!(!stopped(v2::StopReason::Other("error".into())).cancelled());
    assert!(stopped(v2::StopReason::EndTurn).stopped());
    assert!(stopped(v2::StopReason::Other("error".into())).stopped());
    assert!(!closed().stopped(), "a dead connection is a failure, not a stop");
}

/// The real idle from a live turn:
/// `{"stopReason":"end_turn","usage":{"totalTokens":7661,"inputTokens":7632,"outputTokens":29}}`.
/// `total_tokens` is REQUIRED in v2 and `IdleStateUpdate.usage` is wrapped in
/// `DefaultOnError`, so an agent that omits it loses the whole usage object
/// silently rather than failing the notification.
#[test]
fn usage_reads_the_captured_idle_and_the_optional_counters() {
    let captured: v2::Usage = serde_json::from_value(
        json!({ "totalTokens": 7661, "inputTokens": 7632, "outputTokens": 29 }),
    )
    .expect("the captured usage parses");
    assert_eq!(
        turn_usage(&captured),
        TurnUsage {
            input: 7632,
            output: 29,
            cached: 0,
            reasoning: 0,
        }
    );

    let full: v2::Usage = serde_json::from_value(json!({
        "totalTokens": 100,
        "inputTokens": 60,
        "outputTokens": 30,
        "thoughtTokens": 7,
        "cachedReadTokens": 2,
        "cachedWriteTokens": 3,
    }))
    .unwrap();
    assert_eq!(
        turn_usage(&full),
        TurnUsage {
            input: 60,
            output: 30,
            cached: 5,
            reasoning: 7,
        },
        "read and write cache tokens fold into one number"
    );

    // An agent that omits the required total loses the object, not the turn.
    assert!(serde_json::from_value::<v2::Usage>(
        json!({ "inputTokens": 1, "outputTokens": 1 })
    )
    .is_err());
}

#[test]
fn the_board_delivers_one_outcome_per_armed_turn() {
    let mut board = TurnBoard::default();

    // Arming happens BEFORE the send: a turn that ends fast can reach idle
    // before `session/prompt` returns its empty acknowledgement.
    let mut stops = board.arm("s1");
    assert!(
        board.settle(
            "s1",
            TurnOutcome::Stopped {
                kind: Cow::Borrowed("completed"),
                usage: None,
            }
        ),
        "an armed turn is waiting, so this idle is an answer"
    );
    let outcome = stops.try_recv().expect("the waiter got its outcome");
    assert!(outcome.stopped() && !outcome.cancelled());

    // agent2's thirty-second heartbeat: the same idle again, with nobody armed.
    assert!(
        !board.settle(
            "s1",
            TurnOutcome::Stopped {
                kind: Cow::Borrowed("completed"),
                usage: None,
            }
        ),
        "a repeated idle is a heartbeat, not a second turn-end"
    );
    assert!(stops.try_recv().is_err(), "and it delivered nothing");
}

#[test]
fn an_idle_for_a_session_nobody_armed_is_a_heartbeat() {
    let mut board = TurnBoard::default();
    assert!(!board.settle(
        "unknown",
        TurnOutcome::Stopped {
            kind: Cow::Borrowed("completed"),
            usage: None,
        }
    ));

    // A waiter that gave up (the connection ended, the prompt task was dropped)
    // must not leave the board holding a sender that swallows the next turn's
    // outcome — settle reports the dead channel instead of pretending.
    let stops = board.arm("s2");
    drop(stops);
    assert!(!board.settle(
        "s2",
        TurnOutcome::Stopped {
            kind: Cow::Borrowed("completed"),
            usage: None,
        }
    ));
    assert!(
        board.settle(
            "s2",
            TurnOutcome::Stopped {
                kind: Cow::Borrowed("completed"),
                usage: None,
            }
        ) == false,
        "settle disarms on delivery, so the slot is empty either way"
    );
}

#[test]
fn rearming_a_session_replaces_the_old_waiter() {
    let mut board = TurnBoard::default();
    let mut first = board.arm("s1");
    let mut second = board.arm("s1");
    assert!(board.settle(
        "s1",
        TurnOutcome::Stopped {
            kind: Cow::Borrowed("interrupted"),
            usage: None,
        }
    ));
    let outcome = second.try_recv().expect("the newest waiter is the live one");
    assert!(outcome.cancelled());
    assert!(
        first.try_recv().is_err(),
        "the superseded waiter gets nothing: its sender was replaced"
    );
}

#[test]
fn disarm_leaves_nothing_to_settle() {
    let mut board = TurnBoard::default();
    let mut stops = board.arm("s1");
    board.disarm("s1");
    assert!(!board.settle(
        "s1",
        TurnOutcome::Stopped {
            kind: Cow::Borrowed("completed"),
            usage: None,
        }
    ));
    assert!(stops.try_recv().is_err());
}

/// A poisoned board must not take the connection down with it: a panic in one
/// prompt task leaves the mutex poisoned, and every later turn would fail.
#[test]
fn a_poisoned_board_still_answers() {
    let board = Arc::new(Mutex::new(TurnBoard::default()));
    let inner = Arc::clone(&board);
    let _ = std::thread::spawn(move || {
        let _guard = board_lock(&inner);
        panic!("poison");
    })
    .join();
    assert!(board.is_poisoned());
    let mut stops = board_lock(&board).arm("s1");
    assert!(board_lock(&board).settle(
        "s1",
        TurnOutcome::Stopped {
            kind: Cow::Borrowed("completed"),
            usage: None,
        }
    ));
    assert!(stops.try_recv().is_ok());
}
