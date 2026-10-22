//! create_file writes one file with fixed content and permission bits.

use std::{
    fs::Metadata,
    io::Write as _,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use spike_macros::step;

use crate::{Assessment, Code, Failure, Outcome, Probe, RevertGate, Session, StepHandler};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn io_failure(what: &str, path: &Path, err: std::io::Error) -> Failure {
    Failure::new(Code::Io, format!("{what} `{}`: {err}", path.display()))
}

fn mode_of(meta: &Metadata) -> u32 {
    meta.permissions().mode() & 0o777
}

#[step(name = "create_file", category = "files")]
pub struct CreateFileSpec {
    pub path: PathBuf,
    pub content: String,
    pub mode: u32,
}

impl CreateFileSpec {
    fn compare_with_existing(&self, meta: &Metadata) -> Result<Assessment, Failure> {
        let actual =
            std::fs::read(&self.path).map_err(|e| io_failure("reading", &self.path, e))?;
        if actual != self.content.as_bytes() {
            return Ok(Assessment::drift(
                Code::DifferentContent,
                format!(
                    "`{}` exists with different content than planned",
                    self.path.display()
                ),
            ));
        }
        if mode_of(meta) != self.mode {
            return Ok(Assessment::drift(
                Code::DifferentContent,
                format!(
                    "`{}` exists with mode {:o}, planned {:o}",
                    self.path.display(),
                    mode_of(meta),
                    self.mode
                ),
            ));
        }
        Ok(Assessment::Done)
    }

    /// sibling temp keeps the rename on one filesystem.
    fn write_atomically(&self) -> Result<Outcome, Failure> {
        let tmp = self.path.with_file_name(format!(
            ".spike-tmp-{}-{}",
            std::process::id(),
            TMP_COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        let mut file =
            std::fs::File::create(&tmp).map_err(|e| io_failure("creating", &tmp, e))?;
        file.write_all(self.content.as_bytes())
            .map_err(|e| io_failure("writing", &tmp, e))?;
        file.sync_all()
            .map_err(|e| io_failure("syncing", &tmp, e))?;
        drop(file);
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(self.mode))
            .map_err(|e| io_failure("chmod", &tmp, e))?;
        std::fs::rename(&tmp, &self.path)
            .map_err(|e| io_failure("renaming into place", &self.path, e))?;
        Ok(Outcome::Applied)
    }

    fn unreadable(&self, s: &Session, err: std::io::Error) -> Assessment {
        s.journal_event(format!(
            "probe unavailable: stat `{}`: {err}",
            self.path.display()
        ));
        Assessment::Indeterminate {
            probe: Probe::PathContent {
                path: self.path.clone(),
            },
        }
    }
}

impl StepHandler for CreateFileSpec {
    fn assess(&self, s: &Session) -> Result<Assessment, Failure> {
        match std::fs::metadata(&self.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Assessment::Todo),
            Err(e) => Ok(self.unreadable(s, e)),
            Ok(meta) => self.compare_with_existing(&meta),
        }
    }

    fn apply(&self, s: &mut Session) -> Result<Outcome, Failure> {
        if let Some(outcome) = self.assess(s)?.apply_gate()? {
            return Ok(outcome);
        }
        self.write_atomically()
    }

    fn revert(&self, s: &mut Session) -> Result<Outcome, Failure> {
        match self.assess(s)?.revert_gate() {
            RevertGate::Revert => {
                std::fs::remove_file(&self.path)
                    .map_err(|e| io_failure("removing", &self.path, e))?;
                Ok(Outcome::Applied)
            }
            RevertGate::NothingToDo => Ok(Outcome::Applied),
            RevertGate::BestEffort { detail } => {
                s.journal_event(format!(
                    "notice: leaving drifted `{}` untouched ({detail})",
                    self.path.display()
                ));
                Ok(Outcome::Applied)
            }
            RevertGate::Unknown { probe } => Ok(Outcome::Indeterminate { probe }),
        }
    }
}
