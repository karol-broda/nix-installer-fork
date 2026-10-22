//! a session with shims on PATH and a per test state file. no global
//! mutation, parallel safe.

use std::{collections::BTreeMap, path::PathBuf};

use kernel_spike::{Executor, HostFacts, Session};

pub fn shim_session(executor: Executor) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().expect("tempdir");
    let shims = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/shims");
    let inherited = std::env::var("PATH").unwrap_or_default();
    let mut env = BTreeMap::new();
    env.insert(
        "PATH".to_string(),
        format!("{}:{}", shims.display(), inherited),
    );
    let state = dir.path().join("state");
    env.insert(
        "SPIKE_SHIM_STATE".to_string(),
        state.display().to_string(),
    );
    let mut session = Session::new(HostFacts::fabricated(false), executor);
    session.env = env;
    (dir, session)
}
