//! the context handlers get. host facts, spawn env, journal.

use std::{cell::RefCell, collections::BTreeMap};

use crate::{cmd::Executor, Cmd, CmdOutcome, Code, Failure, Idempotency};

#[derive(Debug, Clone)]
pub struct HostFacts {
    pub is_darwin: bool,
}

impl HostFacts {
    #[cfg(target_os = "macos")]
    pub fn detect() -> Self {
        Self { is_darwin: true }
    }

    #[cfg(not(target_os = "macos"))]
    pub fn detect() -> Self {
        Self { is_darwin: false }
    }

    /// build facts for a host that is not this one.
    pub fn fabricated(is_darwin: bool) -> Self {
        Self { is_darwin }
    }
}

/// owns every side effect.
#[derive(Debug)]
pub struct Session {
    pub facts: HostFacts,
    /// spawn environment and PATH resolution. tests put shims here.
    pub env: BTreeMap<String, String>,
    pub executor: Executor,
    journal: RefCell<Vec<String>>,
}

impl Session {
    pub fn new(facts: HostFacts, executor: Executor) -> Self {
        Self {
            facts,
            env: BTreeMap::new(),
            executor,
            journal: RefCell::new(Vec::new()),
        }
    }

    pub fn journal_event(&self, event: String) {
        self.journal.borrow_mut().push(event);
    }

    pub fn journal(&self) -> Vec<String> {
        self.journal.borrow().clone()
    }

    /// refuses UnknownEffect. killed spawns return immediately, never
    /// retried.
    pub fn run_with_retry(
        &self,
        cmd: Cmd,
        max_attempts: u32,
        transient: fn(&CmdOutcome) -> bool,
    ) -> Result<CmdOutcome, Failure> {
        if cmd.idempotency == Idempotency::UnknownEffect {
            return Err(Failure::new(
                Code::Internal,
                format!(
                    "refusing to retry `{}` classified UnknownEffect; remediation is a probe",
                    cmd.display()
                ),
            ));
        }
        let max_attempts = max_attempts.max(1);
        let mut attempt = 0;
        loop {
            attempt += 1;
            let outcome = cmd.run(self)?;
            let exhausted = attempt == max_attempts;
            if matches!(outcome, CmdOutcome::KilledWithoutStatus) || exhausted || !transient(&outcome)
            {
                return Ok(outcome);
            }
        }
    }
}
