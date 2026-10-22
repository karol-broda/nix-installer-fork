# kernel spike

a spike of a redesigned installer kernel. run `cargo test -p kernel-spike`.
everything runs unprivileged because the shims in `tests/shims` stand in
for useradd, id, userdel and systemctl.

## what is here

- `crates/spike-macros`. `#[step]` emits the serde tag, the derives and the
  `StepMeta` impl from one declaration
- `crates/kernel-spike`. `Assessment`, `Outcome`, `Probe`, a `Session` with
  fabricated host facts and injectable env, `Cmd` as the only spawn point
  with `Record` and `FaultInject` executors, and three handlers

the three handlers are `create_file`, `create_user` and
`enable_systemd_unit`. there is no binary and no receipt handling. the
spike is disposable.

## criteria and how they turned out

the criteria were written down before the tests were run.

1. one declaration. the `#[step]` string is the serde tag, the registry
   name and the journal identity. `registry_dispatch.rs` checks the tag and
   the round-trip through the registry. passes
2. handlers stay writable. the lifecycle tests in `user_and_unit_shims.rs`
   pass. whether the handlers are pleasant to read is yours to judge,
   `enable_systemd_unit.rs` is the one to read
3. indeterminate flows. a killed spawn yields `Indeterminate`, the loop in
   `drive_to_done` converges by re-assessing, killed spawns are never
   retried. the fault tests in `user_and_unit_shims.rs` pass
4. name dispatch. steps deserialize by name, erased instances are live
   handlers, names are unique. `registry_dispatch.rs` passes
5. dry run. a recording executor runs nothing and records attempts.
   `dry_run.rs` passes
6. properties on `create_file`. apply twice equals apply once, revert
   restores the pre-state, `Done` exactly when content and mode match,
   drift is refused on apply and left alone on revert.
   `create_file_properties.rs` passes

`cargo check -p nix-installer` still passes, the main tree is untouched.

## notes from building it

a recorded command has no result. the record executor reports no definite
answer and handlers degrade to `Indeterminate` instead of acting on
fabricated empty output.

the revert test first wrote content without the planned mode and the
kernel called it `Drift` and refused. mode checking works.

session owned PATH resolution is what makes the shim injection parallel
safe with no global mutation.
