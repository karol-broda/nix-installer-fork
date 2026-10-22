//! assess apply revert steps behind a `#[step]` registry with `Cmd` as the
//! only spawn point. see `README.md` for the go/no-go criteria.

// #[step] emits spike_kernel paths. alias the crate to itself so they
// resolve internally.
extern crate self as spike_kernel;

mod assessment;
mod cmd;
mod failure;
pub mod handlers;
mod registry;
mod session;

pub use assessment::{Assessment, Observation, Outcome, Probe, RevertGate};
pub use cmd::{Cmd, CmdOutcome, Executor, Idempotency};
pub use failure::{Code, Failure};
pub use registry::{by_name, registry};
pub use session::{HostFacts, Session};

/// metadata declared once by `#[step]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepMeta {
    pub name: &'static str,
    pub category: &'static str,
    pub touches: &'static [&'static str],
}

/// emitted by `#[step]` for every step.
pub trait StepDecl {
    const META: StepMeta;
}

/// no apply change without the paired assess change.
pub trait StepHandler {
    fn assess(&self, s: &Session) -> Result<Assessment, Failure>;
    fn apply(&self, s: &mut Session) -> Result<Outcome, Failure>;
    fn revert(&self, s: &mut Session) -> Result<Outcome, Failure>;
}

/// a step eligible for registry dispatch.
pub trait StepSpec:
    StepHandler
    + serde::Serialize
    + serde::de::DeserializeOwned
    + Send
    + Sync
    + StepDecl
{
}

impl<T> StepSpec for T where
    T: StepHandler
        + serde::Serialize
        + serde::de::DeserializeOwned
        + Send
        + Sync
        + StepDecl
{
}

/// a step behind a name, deserialized from json.
pub trait ErasedStep {
    fn name(&self) -> &'static str;
    fn meta(&self) -> StepMeta;
    fn assess(&self, s: &Session) -> Result<Assessment, Failure>;
    fn apply(&self, s: &mut Session) -> Result<Outcome, Failure>;
    fn revert(&self, s: &mut Session) -> Result<Outcome, Failure>;
    fn to_value(&self) -> serde_json::Value;
}

impl<T: StepSpec> ErasedStep for T {
    fn name(&self) -> &'static str {
        T::META.name
    }

    fn meta(&self) -> StepMeta {
        T::META
    }

    fn assess(&self, s: &Session) -> Result<Assessment, Failure> {
        StepHandler::assess(self, s)
    }

    fn apply(&self, s: &mut Session) -> Result<Outcome, Failure> {
        StepHandler::apply(self, s)
    }

    fn revert(&self, s: &mut Session) -> Result<Outcome, Failure> {
        StepHandler::revert(self, s)
    }

    fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("step specs serialize infallibly")
    }
}

/// everything needed to dispatch a step by name.
#[derive(Debug, Clone, Copy)]
pub struct StepVTable {
    pub name: &'static str,
    pub meta: StepMeta,
    pub deserialize: fn(serde_json::Value) -> Result<Box<dyn ErasedStep>, serde_json::Error>,
}

pub fn vtable<T: StepSpec + 'static>() -> StepVTable {
    StepVTable {
        name: <T as StepDecl>::META.name,
        meta: <T as StepDecl>::META,
        deserialize: |value| {
            Ok(Box::new(serde_json::from_value::<T>(value)?) as Box<dyn ErasedStep>)
        },
    }
}
