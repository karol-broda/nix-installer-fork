//! enable and start one unit. assess and apply are both systemctl runs.
//! tests use a shim in tests/shims.

use spike_macros::step;

use crate::{
    Assessment, Cmd, CmdOutcome, Code, Failure, Idempotency, Outcome, Probe, RevertGate,
    Session, StepHandler,
};

#[step(name = "enable_systemd_unit", category = "supervision")]
pub struct EnableSystemdUnitSpec {
    pub unit: String,
}

impl EnableSystemdUnitSpec {
    fn is_active_cmd(&self) -> Cmd {
        Cmd::new("systemctl")
            .arg("is-active")
            .arg(self.unit.clone())
            .idempotency(Idempotency::SafeToRepeat)
    }

    fn enable_cmd(&self) -> Cmd {
        Cmd::new("systemctl")
            .arg("enable")
            .arg("--now")
            .arg(self.unit.clone())
            .idempotency(Idempotency::ProbeFirst)
    }

    fn disable_cmd(&self) -> Cmd {
        Cmd::new("systemctl")
            .arg("disable")
            .arg("--now")
            .arg(self.unit.clone())
            .idempotency(Idempotency::SafeToRepeat)
    }

    fn indeterminate(&self) -> Outcome {
        Outcome::Indeterminate {
            probe: Probe::UnitActive {
                unit: self.unit.clone(),
            },
        }
    }

    fn disable(&self, s: &mut Session) -> Result<Outcome, Failure> {
        match self.disable_cmd().run(s)? {
            CmdOutcome::Success { .. } => Ok(Outcome::Applied),
            CmdOutcome::Failure { status, stderr, .. } => {
                s.journal_event(format!(
                    "notice: disabling unit `{}` failed ({status}: {stderr})",
                    self.unit
                ));
                Ok(Outcome::Applied)
            }
            CmdOutcome::KilledWithoutStatus => Ok(self.indeterminate()),
        }
    }
}

impl StepHandler for EnableSystemdUnitSpec {
    fn assess(&self, s: &Session) -> Result<Assessment, Failure> {
        match self.is_active_cmd().run(s)? {
            CmdOutcome::Success { .. } => Ok(Assessment::Done),
            // is-active exits non-zero for inactive and unknown alike,
            // both are Todo here
            CmdOutcome::Failure { status, stderr, .. } => {
                s.journal_event(format!(
                    "unit `{}` not active (status {status}: {stderr})",
                    self.unit
                ));
                Ok(Assessment::Todo)
            }
            CmdOutcome::KilledWithoutStatus => Ok(Assessment::Indeterminate {
                probe: Probe::UnitActive {
                    unit: self.unit.clone(),
                },
            }),
        }
    }

    fn apply(&self, s: &mut Session) -> Result<Outcome, Failure> {
        if let Some(outcome) = self.assess(s)?.apply_gate()? {
            return Ok(outcome);
        }

        match self.enable_cmd().run(s)? {
            CmdOutcome::Success { .. } => Ok(Outcome::Applied),
            CmdOutcome::Failure { status, stderr, .. } => Err(Failure::new(
                Code::Transient,
                format!(
                    "enabling unit `{}` failed with status {status}: {stderr}",
                    self.unit
                ),
            )),
            CmdOutcome::KilledWithoutStatus => Ok(self.indeterminate()),
        }
    }

    fn revert(&self, s: &mut Session) -> Result<Outcome, Failure> {
        match self.assess(s)?.revert_gate() {
            RevertGate::Revert => self.disable(s),
            RevertGate::NothingToDo => Ok(Outcome::Applied),
            RevertGate::BestEffort { .. } => Ok(Outcome::Applied),
            RevertGate::Unknown { .. } => Ok(self.indeterminate()),
        }
    }
}
