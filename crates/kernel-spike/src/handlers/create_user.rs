//! one user via useradd or dscl, chosen by host facts. tests use shims in
//! tests/shims.

use spike_macros::step;

use crate::{
    assessment::internal,
    Assessment, Cmd, CmdOutcome, Code, Failure, Idempotency, Outcome, Probe, RevertGate,
    Session, StepHandler,
};

/// id exits 1 for no such user.
const ID_NO_SUCH_USER: i32 = 1;
/// useradd exits 9 for an existing user. the shim mirrors it.
const USERADD_EEXIST: i32 = 9;

#[step(name = "create_user", category = "users")]
pub struct CreateUserSpec {
    pub name: String,
    pub uid: u32,
    pub primary_group: String,
}

/// the id -u probe.
enum Lookup {
    Known(Option<u32>),
    Unknown,
}

impl CreateUserSpec {
    fn lookup(&self, s: &Session) -> Result<Lookup, Failure> {
        let cmd = Cmd::new("id")
            .arg("-u")
            .arg(self.name.clone())
            .idempotency(Idempotency::SafeToRepeat);
        match cmd.run(s)? {
            CmdOutcome::Success { stdout, .. } => match stdout.trim().parse::<u32>() {
                Ok(uid) => Ok(Lookup::Known(Some(uid))),
                Err(_) => Err(internal(format!(
                    "`id -u {}` printed unparseable output `{stdout}`",
                    self.name
                ))),
            },
            CmdOutcome::Failure {
                status: ID_NO_SUCH_USER,
                ..
            } => Ok(Lookup::Known(None)),
            CmdOutcome::Failure { status, stderr, .. } => Err(Failure::new(
                Code::Transient,
                format!("`id -u {}` failed with status {status}: {stderr}", self.name),
            )),
            CmdOutcome::KilledWithoutStatus => Ok(Lookup::Unknown),
        }
    }

    fn create_cmd(&self, s: &Session) -> Cmd {
        if s.facts.is_darwin {
            // minimal dscl, not exercised by the shims
            Cmd::new("dscl")
                .arg(".")
                .arg("-create")
                .arg(format!("/Users/{}", self.name))
                .arg("PrimaryGroupID")
                .arg(self.primary_group.clone())
                .idempotency(Idempotency::ProbeFirst)
        } else {
            Cmd::new("useradd")
                .arg("-u")
                .arg(self.uid.to_string())
                .arg("-g")
                .arg(self.primary_group.clone())
                .arg(self.name.clone())
                .idempotency(Idempotency::ProbeFirst)
        }
    }

    fn delete_cmd(&self, s: &Session) -> Cmd {
        if s.facts.is_darwin {
            Cmd::new("dscl")
                .arg(".")
                .arg("-delete")
                .arg(format!("/Users/{}", self.name))
                .idempotency(Idempotency::SafeToRepeat)
        } else {
            Cmd::new("userdel")
                .arg(self.name.clone())
                .idempotency(Idempotency::SafeToRepeat)
        }
    }

    /// the re-probe decides. never a retry.
    fn resolve_existing(&self, s: &mut Session) -> Result<Outcome, Failure> {
        match self.assess(s)?.apply_gate()? {
            Some(Outcome::Applied) => Ok(Outcome::Applied),
            other => Err(internal(format!(
                "user creation reported already-exists, but the re-probe says {other:?}"
            ))),
        }
    }

    fn indeterminate(&self) -> Outcome {
        Outcome::Indeterminate {
            probe: Probe::UserExists {
                name: self.name.clone(),
            },
        }
    }
}

impl StepHandler for CreateUserSpec {
    fn assess(&self, s: &Session) -> Result<Assessment, Failure> {
        match self.lookup(s)? {
            Lookup::Known(None) => Ok(Assessment::Todo),
            Lookup::Known(Some(uid)) if uid == self.uid => Ok(Assessment::Done),
            Lookup::Known(Some(other)) => Ok(Assessment::drift(
                Code::UserUidMismatch,
                format!(
                    "user `{}` exists with uid {other}, but {} was planned",
                    self.name, self.uid
                ),
            )),
            Lookup::Unknown => Ok(Assessment::Indeterminate {
                probe: Probe::UserExists {
                    name: self.name.clone(),
                },
            }),
        }
    }

    fn apply(&self, s: &mut Session) -> Result<Outcome, Failure> {
        if let Some(outcome) = self.assess(s)?.apply_gate()? {
            return Ok(outcome);
        }

        match self.create_cmd(s).run(s)? {
            CmdOutcome::Success { .. } => Ok(Outcome::Applied),
            CmdOutcome::Failure {
                status: USERADD_EEXIST,
                ..
            } => self.resolve_existing(s),
            CmdOutcome::Failure { status, stderr, .. } => Err(Failure::new(
                Code::Transient,
                format!(
                    "creating user `{}` failed with status {status}: {stderr}",
                    self.name
                ),
            )),
            CmdOutcome::KilledWithoutStatus => Ok(self.indeterminate()),
        }
    }

    fn revert(&self, s: &mut Session) -> Result<Outcome, Failure> {
        match self.assess(s)?.revert_gate() {
            RevertGate::Revert => match self.delete_cmd(s).run(s)? {
                CmdOutcome::Success { .. } => Ok(Outcome::Applied),
                CmdOutcome::Failure { status, stderr, .. } => Err(Failure::new(
                    Code::Transient,
                    format!(
                        "deleting user `{}` failed with status {status}: {stderr}",
                        self.name
                    ),
                )),
                CmdOutcome::KilledWithoutStatus => Ok(self.indeterminate()),
            },
            RevertGate::NothingToDo => Ok(Outcome::Applied),
            RevertGate::BestEffort { detail } => {
                s.journal_event(format!(
                    "notice: not deleting drifted user `{}` ({detail})",
                    self.name
                ));
                Ok(Outcome::Applied)
            }
            RevertGate::Unknown { .. } => Ok(self.indeterminate()),
        }
    }
}
