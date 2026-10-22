//! failure codes and the Failure type.

/// stable semantic codes. every variant must be triggerable by a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    /// a file exists with different content than planned. never silently
    /// overwritten.
    DifferentContent,
    UserUidMismatch,
    MissingTool,
    Transient,
    Io,
    /// a bug. file it.
    Internal,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::DifferentContent => "E-CONF-003",
            Code::UserUidMismatch => "E-GROUP-001",
            Code::MissingTool => "E-TOOL-001",
            Code::Transient => "E-TRANSIENT-001",
            Code::Io => "E-IO-001",
            Code::Internal => "E-INTERNAL",
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{detail} [{}]", .code.as_str())]
pub struct Failure {
    pub code: Code,
    pub detail: String,
}

impl Failure {
    pub fn new(code: Code, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}
