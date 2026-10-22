//! recording executor runs nothing and records attempts.

mod common;

use common::shim_session;
use kernel_spike::{handlers::CreateUserSpec, Cmd, Executor, Idempotency, Outcome, StepHandler};

#[test]
fn recorded_cmd_has_no_effect() {
    let (dir, session) = shim_session(Executor::record());
    let cmd = Cmd::new("useradd")
        .arg("-u")
        .arg("30001")
        .arg("-g")
        .arg("nixbld")
        .arg("nixbld1")
        .idempotency(Idempotency::ProbeFirst);

    let outcome = cmd.run(&session).unwrap();
    assert!(
        matches!(outcome, kernel_spike::CmdOutcome::KilledWithoutStatus),
        "a recorded command claims no result"
    );
    assert_eq!(session.executor.recorded(), vec![cmd.display()]);
    assert!(
        !dir.path().join("state").exists(),
        "the shim must never have run"
    );
}

#[test]
fn dry_run_session_is_effect_free() {
    let (dir, mut session) = shim_session(Executor::record());
    let spec = CreateUserSpec {
        name: "nixbld1".into(),
        uid: 30001,
        primary_group: "nixbld".into(),
    };

    match spec.apply(&mut session).unwrap() {
        Outcome::Indeterminate { .. } => {}
        other => panic!("dry-run apply must be indeterminate, got {other:?}"),
    }
    assert!(
        !dir.path().join("state").exists(),
        "dry-run must not create users"
    );
    assert!(
        session
            .executor
            .recorded()
            .iter()
            .any(|c| c.starts_with("id -u nixbld1")),
        "the attempted probe must be recorded"
    );
}
