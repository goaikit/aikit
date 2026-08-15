//! Phase 1 seam tests: `Backend::history_reader()`/`history_mutator()`
//! gating, independent of any concrete adapter. Adapter-specific mapping
//! tests live in `claude.rs` (Phase 2) and the Phase 4 hardening table.

use super::*;
use crate::runner::backend::ALL;

/// Spec 008 §6 invariant: `capabilities().history_store == true` iff
/// `history_reader().is_some()`, in every build configuration. This is the
/// whole point of the capability gate — a client checking the capability
/// must never observe a `None` reader, and vice versa.
#[test]
fn history_store_capability_matches_reader_presence_for_all_backends() {
    for &b in ALL {
        assert_eq!(
            b.capabilities().history_store,
            b.history_reader().is_some(),
            "history_store capability vs history_reader() mismatch for {b:?}"
        );
    }
}

/// Same invariant for mutations: `history_mutations` iff `history_mutator()`
/// is `Some`.
#[test]
fn history_mutations_capability_matches_mutator_presence_for_all_backends() {
    for &b in ALL {
        assert_eq!(
            b.capabilities().history_mutations,
            b.history_mutator().is_some(),
            "history_mutations capability vs history_mutator() mismatch for {b:?}"
        );
    }
}

/// Every non-Claude Backend has no history store, in any build
/// configuration (spec 008 §4 table — only Claude implements it, and only
/// behind `claude-sdk`).
#[test]
fn only_claude_can_ever_have_a_history_reader() {
    for &b in ALL {
        if b != Backend::Claude {
            assert!(
                b.history_reader().is_none(),
                "{b:?} must never have a history reader"
            );
            assert!(
                b.history_mutator().is_none(),
                "{b:?} must never have a history mutator"
            );
        }
    }
}

#[cfg(feature = "claude-sdk")]
#[test]
fn claude_history_reader_and_mutator_are_some_when_claude_sdk_enabled() {
    assert!(Backend::Claude.history_reader().is_some());
    assert!(Backend::Claude.history_mutator().is_some());
    assert!(Backend::Claude.capabilities().history_store);
    assert!(Backend::Claude.capabilities().history_mutations);
    assert_eq!(
        Backend::Claude.history_reader().unwrap().backend(),
        Backend::Claude
    );
}

#[cfg(not(feature = "claude-sdk"))]
#[test]
fn claude_history_reader_and_mutator_are_none_when_claude_sdk_disabled() {
    // The capability is honest: "unsupported," not "supported but empty."
    assert!(Backend::Claude.history_reader().is_none());
    assert!(Backend::Claude.history_mutator().is_none());
    assert!(!Backend::Claude.capabilities().history_store);
    assert!(!Backend::Claude.capabilities().history_mutations);
}
