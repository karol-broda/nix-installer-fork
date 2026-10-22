//! lifecycle and fault injection tests for the shim driven steps.

mod common;

use common::shim_session;
use kernel_spike::{
    handlers::{CreateUserSpec, EnableSystemdUnitSpec},
    Assessment, Code, Cmd, Executor, Idempotency, Outcome, Session, StepHandler,
};

fn user_spec() -> CreateUserSpec {
    CreateUserSpec {
        name: "nixbld1".into(),
        uid: 30001,
        primary_group: "nixbld".into(),
    }
}

fn unit_spec() -> EnableSystemdUnitSpec {
    EnableSystemdUnitSpec {
        unit: "nix-daemon.socket".into(),
    }
}

#[test]
fn user_lifecycle() {
    let (_dir, mut session) = shim_session(Executor::real());
    let spec = user_spec();

    assert_eq!(spec.assess(&session).unwrap(), Assessment::Todo);
    assert_eq!(spec.apply(&mut session).unwrap(), Outcome::Applied);
    assert_eq!(spec.assess(&session).unwrap(), Assessment::Done);
    assert_eq!(spec.revert(&mut session).unwrap(), Outcome::Applied);
    assert_eq!(spec.assess(&session).unwrap(), Assessment::Todo);

    assert!(session.journal().iter().any(|e| e.contains("useradd")));
    assert!(session.journal().iter().any(|e| e.contains("userdel")));
}

#[test]
fn user_uid_drift_refused_then_best_effort() {
    let (dir, mut session) = shim_session(Executor::real());
    std::fs::write(dir.path().join("state"), "nixbld1 29999 nixbld\n").unwrap();

    let spec = user_spec();
    match spec.assess(&session).unwrap() {
        Assessment::Drift { remedy, .. } => assert_eq!(remedy, Code::UserUidMismatch),
        other => panic!("expected uid drift, got {other:?}"),
    }
    assert_eq!(
        spec.apply(&mut session).unwrap_err().code,
        Code::UserUidMismatch
    );
    spec.revert(&mut session).unwrap();
    assert!(
        std::fs::read_to_string(dir.path().join("state"))
            .unwrap()
            .contains("nixbld1 29999"),
        "revert must not delete the drifted user"
    );
}

#[test]
fn useradd_eexist_resolves_by_probe_not_retry() {
    let (dir, mut session) = shim_session(Executor::real());
    std::fs::write(dir.path().join("state"), "nixbld1 30001 nixbld\n").unwrap();

    let spec = user_spec();
    assert_eq!(spec.assess(&session).unwrap(), Assessment::Done);
    assert_eq!(spec.apply(&mut session).unwrap(), Outcome::Applied);
}

#[test]
fn unit_lifecycle() {
    let (_dir, mut session) = shim_session(Executor::real());
    let spec = unit_spec();

    assert_eq!(spec.assess(&session).unwrap(), Assessment::Todo);
    assert_eq!(spec.apply(&mut session).unwrap(), Outcome::Applied);
    assert_eq!(spec.assess(&session).unwrap(), Assessment::Done);
    assert_eq!(spec.revert(&mut session).unwrap(), Outcome::Applied);
    assert_eq!(spec.assess(&session).unwrap(), Assessment::Todo);
}

/// apply Todo, journal and retry Indeterminate, never tolerate Drift.
fn drive_to_done(spec: &CreateUserSpec, session: &mut Session) -> bool {
    for _ in 0..6 {
        match spec.assess(session).unwrap() {
            Assessment::Done => return true,
            Assessment::Todo => {
                assert_eq!(spec.apply(session).unwrap(), Outcome::Applied);
            }
            Assessment::Indeterminate { probe } => {
                session.journal_event(format!("indeterminate: {}", probe.describe()));
            }
            Assessment::Drift { detail, .. } => panic!("unexpected drift: {detail}"),
        }
    }
    false
}

/// killed spawn yields Indeterminate, drive_to_done converges.
#[test]
fn fault_injection_converges_through_indeterminate() {
    let (_dir, mut session) = shim_session(Executor::fault_at(1));
    let spec = user_spec();

    assert!(
        drive_to_done(&spec, &mut session),
        "install must converge after a killed spawn"
    );
    assert!(
        session.journal().iter().any(|e| e.contains("indeterminate")),
        "the indeterminate episode must be journaled"
    );
}

#[test]
fn killed_create_converges_next_apply() {
    let (_dir, mut session) = shim_session(Executor::fault_at(2));
    let spec = user_spec();

    assert_eq!(spec.assess(&session).unwrap(), Assessment::Todo);
    match spec.apply(&mut session).unwrap() {
        Outcome::Indeterminate { .. } => {}
        other => panic!("expected indeterminate after killed useradd, got {other:?}"),
    }

    let (_dir2, mut session2) = shim_session(Executor::real());
    assert_eq!(spec.apply(&mut session2).unwrap(), Outcome::Applied);
    assert_eq!(spec.assess(&session2).unwrap(), Assessment::Done);
}

#[test]
fn retry_refuses_unknown_effect() {
    let (_dir, session) = shim_session(Executor::real());
    let cmd = Cmd::new("useradd").arg("oops").idempotency(Idempotency::UnknownEffect);
    let err = session
        .run_with_retry(cmd, 3, |_| true)
        .expect_err("UnknownEffect must not be retried");
    assert_eq!(err.code, Code::Internal);
}

#[test]
fn killed_without_status_is_never_retried() {
    let (_dir, session) = shim_session(Executor::fault_at(1));
    let cmd = Cmd::new("useradd")
        .arg("-u")
        .arg("1")
        .arg("x")
        .idempotency(Idempotency::SafeToRepeat);
    let outcome = session
        .run_with_retry(cmd, 5, |o| matches!(o, kernel_spike::CmdOutcome::Failure { .. }))
        .unwrap();
    assert!(matches!(outcome, kernel_spike::CmdOutcome::KilledWithoutStatus));
    assert_eq!(
        session.journal().iter().filter(|e| e.starts_with("cmd:")).count(),
        1,
        "a killed spawn must not be retried"
    );
}
