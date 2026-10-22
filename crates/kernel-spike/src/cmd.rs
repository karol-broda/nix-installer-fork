//! every spawn goes through Cmd. the Executor decides what really runs.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    path::PathBuf,
    process::{Command, Stdio},
};

use crate::{Code, Failure};

/// how safe a command is to repeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Idempotency {
    SafeToRepeat,
    /// re-run only after a re-probe.
    ProbeFirst,
    /// no definite answer whether it took effect. never retried, only
    /// probed.
    UnknownEffect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmdOutcome {
    Success { stdout: String, stderr: String },
    Failure {
        status: i32,
        stdout: String,
        stderr: String,
    },
    /// died without exit status. maps to Indeterminate.
    KilledWithoutStatus,
}

#[derive(Debug, Clone)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
    pub idempotency: Idempotency,
}

impl Cmd {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            idempotency: Idempotency::UnknownEffect,
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn idempotency(mut self, idempotency: Idempotency) -> Self {
        self.idempotency = idempotency;
        self
    }

    pub fn display(&self) -> String {
        format!("{} {}", self.program, self.args.join(" "))
    }

    pub fn run(&self, s: &crate::Session) -> Result<CmdOutcome, Failure> {
        s.journal_event(format!(
            "cmd: {} [{:?}]",
            self.display(),
            self.idempotency
        ));
        s.executor.execute(self, &s.env)
    }
}

/// how a Cmd actually runs.
#[derive(Debug)]
pub enum Executor {
    Real,
    /// records instead of executing. returns KilledWithoutStatus since a
    /// recorded command has no result.
    Record(RefCell<Vec<String>>),
    /// the spawn at fail_at_nth reports KilledWithoutStatus, all others
    /// run for real.
    FaultInject {
        fail_at_nth: usize,
        spawned: Cell<usize>,
    },
}

impl Executor {
    pub fn real() -> Self {
        Executor::Real
    }

    pub fn record() -> Self {
        Executor::Record(RefCell::new(Vec::new()))
    }

    pub fn fault_at(nth: usize) -> Self {
        Executor::FaultInject {
            fail_at_nth: nth,
            spawned: Cell::new(0),
        }
    }

    pub fn recorded(&self) -> Vec<String> {
        match self {
            Executor::Record(log) => log.borrow().clone(),
            _ => Vec::new(),
        }
    }

    fn execute(&self, cmd: &Cmd, env: &BTreeMap<String, String>) -> Result<CmdOutcome, Failure> {
        match self {
            Executor::Record(log) => {
                log.borrow_mut().push(cmd.display());
                Ok(CmdOutcome::KilledWithoutStatus)
            }
            Executor::FaultInject {
                fail_at_nth,
                spawned,
            } => {
                let n = spawned.get() + 1;
                spawned.set(n);
                if n == *fail_at_nth {
                    Ok(CmdOutcome::KilledWithoutStatus)
                } else {
                    spawn_for_real(cmd, env)
                }
            }
            Executor::Real => spawn_for_real(cmd, env),
        }
    }
}

fn spawn_for_real(cmd: &Cmd, env: &BTreeMap<String, String>) -> Result<CmdOutcome, Failure> {
    let program = resolve_program(&cmd.program, env);
    let mut command = Command::new(&program);
    command.args(&cmd.args).envs(env).stdin(Stdio::null());
    match command.output() {
        Err(err) => Err(Failure::new(
            Code::MissingTool,
            format!("could not execute `{}`: {err}", cmd.program),
        )),
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            match output.status.code() {
                Some(0) => Ok(CmdOutcome::Success { stdout, stderr }),
                Some(status) => Ok(CmdOutcome::Failure { status, stdout, stderr }),
                None => Ok(CmdOutcome::KilledWithoutStatus),
            }
        }
    }
}

/// bare names resolve against session PATH first. tests put shims there.
fn resolve_program(program: &str, env: &BTreeMap<String, String>) -> PathBuf {
    if program.contains('/') {
        return PathBuf::from(program);
    }
    let path = env
        .get("PATH")
        .cloned()
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_default();
    for dir in path.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = PathBuf::from(dir).join(program);
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from(program)
}
