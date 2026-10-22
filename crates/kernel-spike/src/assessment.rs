//! what a step sees in the world and what an effect achieved.

use std::path::PathBuf;

use crate::{Cmd, CmdOutcome, Code, Failure, Idempotency, Session};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assessment {
    Done,
    Todo,
    /// world conflicts with the spec, likely by a human. never silently
    /// overwritten.
    Drift { detail: String, remedy: Code },
    /// cannot tell. the probe says what to check. resolve by re-assessing,
    /// never by blind retry.
    Indeterminate { probe: Probe },
}

impl Assessment {
    pub fn drift(remedy: Code, detail: impl Into<String>) -> Self {
        Assessment::Drift {
            detail: detail.into(),
            remedy,
        }
    }

    /// shared apply policy. Done to Applied, Drift to error, Indeterminate
    /// through, Todo as None.
    pub fn apply_gate(self) -> Result<Option<Outcome>, Failure> {
        match self {
            Assessment::Done => Ok(Some(Outcome::Applied)),
            Assessment::Todo => Ok(None),
            Assessment::Drift { detail, remedy } => Err(Failure::new(remedy, detail)),
            Assessment::Indeterminate { probe } => Ok(Some(Outcome::Indeterminate { probe })),
        }
    }

    /// shared revert policy. Done undone by the handler, Todo nothing,
    /// Drift left alone, Indeterminate through.
    pub fn revert_gate(self) -> RevertGate {
        match self {
            Assessment::Done => RevertGate::Revert,
            Assessment::Todo => RevertGate::NothingToDo,
            Assessment::Drift { detail, .. } => RevertGate::BestEffort { detail },
            Assessment::Indeterminate { probe } => RevertGate::Unknown { probe },
        }
    }
}

/// the handler supplies only the Revert arm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevertGate {
    Revert,
    NothingToDo,
    BestEffort { detail: String },
    Unknown { probe: Probe },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// verified the world satisfies the spec.
    Applied,
    /// the effect possibly happened. re-assess to resolve.
    Indeterminate { probe: Probe },
}

/// how to re-check an indeterminate effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    UserExists { name: String },
    UnitActive { unit: String },
    PathContent { path: PathBuf },
}

/// raw observation, uninterpreted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observation {
    Present(String),
    Absent,
    Unavailable,
}

impl Probe {
    pub fn describe(&self) -> String {
        match self {
            Probe::UserExists { name } => format!("does user `{name}` exist, and with which uid"),
            Probe::UnitActive { unit } => format!("is unit `{unit}` active"),
            Probe::PathContent { path } => {
                format!("does `{}` exist, and with which content", path.display())
            }
        }
    }

    /// returns Unavailable under a recording executor since a recorded
    /// command has no result.
    pub fn observe(&self, s: &Session) -> Observation {
        match self {
            Probe::UserExists { name } => {
                let cmd = Cmd::new("id")
                    .arg("-u")
                    .arg(name.clone())
                    .idempotency(Idempotency::SafeToRepeat);
                match cmd.run(s) {
                    Ok(CmdOutcome::Success { stdout, .. }) => {
                        Observation::Present(stdout.trim().to_string())
                    }
                    Ok(CmdOutcome::Failure { status: 1, .. }) => Observation::Absent,
                    _ => Observation::Unavailable,
                }
            }
            Probe::UnitActive { unit } => {
                let cmd = Cmd::new("systemctl")
                    .arg("is-active")
                    .arg(unit.clone())
                    .idempotency(Idempotency::SafeToRepeat);
                match cmd.run(s) {
                    Ok(CmdOutcome::Success { stdout, .. }) => {
                        Observation::Present(stdout.trim().to_string())
                    }
                    Ok(CmdOutcome::Failure { .. }) => Observation::Absent,
                    _ => Observation::Unavailable,
                }
            }
            Probe::PathContent { path } => match std::fs::read_to_string(path) {
                Ok(content) => Observation::Present(content),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Observation::Absent,
                Err(_) => Observation::Unavailable,
            },
        }
    }
}

/// build an Internal failure.
pub fn internal(detail: impl Into<String>) -> crate::Failure {
    crate::Failure::new(Code::Internal, detail)
}
