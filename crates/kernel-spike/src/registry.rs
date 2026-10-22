//! explicit step registration. name uniqueness asserted by test.

use crate::{
    handlers::{CreateFileSpec, CreateUserSpec, EnableSystemdUnitSpec},
    vtable, StepVTable,
};

/// every step this crate knows.
pub fn registry() -> Vec<StepVTable> {
    vec![
        vtable::<CreateFileSpec>(),
        vtable::<CreateUserSpec>(),
        vtable::<EnableSystemdUnitSpec>(),
    ]
}

pub fn by_name(name: &str) -> Option<StepVTable> {
    registry().into_iter().find(|v| v.name == name)
}
