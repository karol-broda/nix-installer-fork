//! property tests for create_file. a small deterministic generator
//! provides the cases.

use std::os::unix::fs::PermissionsExt as _;

use kernel_spike::{
    handlers::CreateFileSpec, Assessment, Code, Executor, HostFacts, Outcome, Session, StepHandler,
};

struct Lcg(u64);

impl Lcg {
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    fn content(&mut self, len: usize) -> String {
        (0..len)
            .map(|_| char::from(b'a' + (self.next_u64() % 26) as u8))
            .collect()
    }

    fn mode(&mut self) -> u32 {
        // a few sane modes to exercise the mode checking path
        [0o644, 0o600, 0o755, 0o640][(self.next_u64() % 4) as usize]
    }
}

fn spec_for(dir: &std::path::Path, rng: &mut Lcg, i: usize) -> CreateFileSpec {
    let len = 1 + (rng.next_u64() % 64) as usize;
    CreateFileSpec {
        path: dir.join(format!("prop-{i}.conf")),
        content: rng.content(len),
        mode: rng.mode(),
    }
}

/// apply twice equals apply once.
#[test]
fn a1_idempotence() {
    let dir = tempfile::tempdir().unwrap();
    let mut rng = Lcg(0xC0FFEE);

    for i in 0..64 {
        let spec = spec_for(dir.path(), &mut rng, i);
        let mut session = Session::new(HostFacts::fabricated(false), Executor::real());

        spec.apply(&mut session).unwrap();
        let world_after_first = std::fs::read(&spec.path).unwrap();
        let mode_after_first = std::fs::metadata(&spec.path).unwrap().permissions().mode();

        assert_eq!(spec.apply(&mut session).unwrap(), Outcome::Applied);
        assert_eq!(
            std::fs::read(&spec.path).unwrap(),
            world_after_first,
            "second apply must not change content"
        );
        assert_eq!(
            std::fs::metadata(&spec.path).unwrap().permissions().mode(),
            mode_after_first,
            "second apply must not change mode"
        );
        assert_eq!(spec.assess(&session).unwrap(), Assessment::Done);
    }
}

/// revert restores the pre-state.
#[test]
fn a2_revert_restores_pre_state() {
    let dir = tempfile::tempdir().unwrap();
    let mut rng = Lcg(0xBEEF);

    for i in 0..64 {
        let spec = spec_for(dir.path(), &mut rng, i);
        let mut session = Session::new(HostFacts::fabricated(false), Executor::real());

        spec.apply(&mut session).unwrap();
        spec.revert(&mut session).unwrap();
        assert!(
            !spec.path.exists(),
            "revert must restore the absent pre-state"
        );

        std::fs::write(&spec.path, &spec.content).unwrap();
        std::fs::set_permissions(
            &spec.path,
            std::fs::Permissions::from_mode(spec.mode),
        )
        .unwrap();
        assert_eq!(spec.assess(&session).unwrap(), Assessment::Done);
        spec.revert(&mut session).unwrap();
        assert!(!spec.path.exists());
    }
}

/// Done exactly when content and mode match.
#[test]
fn a4_assessment_truth() {
    let dir = tempfile::tempdir().unwrap();
    let spec = CreateFileSpec {
        path: dir.path().join("a4.conf"),
        content: "planned".into(),
        mode: 0o644,
    };
    let session = Session::new(HostFacts::fabricated(false), Executor::real());

    assert_eq!(spec.assess(&session).unwrap(), Assessment::Todo);

    std::fs::write(&spec.path, "planned").unwrap();
    assert_eq!(spec.assess(&session).unwrap(), Assessment::Done);

    let mut perms = std::fs::metadata(&spec.path).unwrap().permissions();
    perms.set_mode(0o600);
    std::fs::set_permissions(&spec.path, perms).unwrap();
    match spec.assess(&session).unwrap() {
        Assessment::Drift { remedy, .. } => assert_eq!(remedy, Code::DifferentContent),
        other => panic!("expected drift on mode mismatch, got {other:?}"),
    }

    std::fs::write(&spec.path, "user-edited").unwrap();
    match spec.assess(&session).unwrap() {
        Assessment::Drift { remedy, .. } => assert_eq!(remedy, Code::DifferentContent),
        other => panic!("expected drift on content mismatch, got {other:?}"),
    }
    let mut session = session;
    let refusal = spec.apply(&mut session).unwrap_err();
    assert_eq!(refusal.code, Code::DifferentContent);

    spec.revert(&mut session).unwrap();
    assert_eq!(
        std::fs::read_to_string(&spec.path).unwrap(),
        "user-edited",
        "revert must not delete human-owned content"
    );
    assert!(
        session.journal().iter().any(|e| e.contains("notice:")),
        "revert must journal the notice"
    );
}
