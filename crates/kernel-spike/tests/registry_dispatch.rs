//! the serde tag comes from `#[step]` alone and name dispatch round-trips.

use kernel_spike::{
    by_name, handlers, registry, Assessment, Executor, HostFacts, Session,
};

fn file_spec(tmp: &std::path::Path, i: u32) -> handlers::CreateFileSpec {
    handlers::CreateFileSpec {
        path: tmp.join(format!("registry-{i}.conf")),
        content: format!("content {i}\n"),
        mode: 0o644,
    }
}

#[test]
fn names_are_unique() {
    let names: Vec<&str> = registry().iter().map(|v| v.name).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(names.len(), sorted.len(), "duplicate step names in registry");
}

#[test]
fn serde_tag_comes_from_single_declaration() {
    let dir = tempfile::tempdir().unwrap();
    let value = serde_json::to_value(file_spec(dir.path(), 1)).unwrap();
    assert_eq!(value["step"], "create_file");
}

#[test]
fn roundtrip_through_registry_dispatch() {
    let dir = tempfile::tempdir().unwrap();

    let user = handlers::CreateUserSpec {
        name: "nixbld1".into(),
        uid: 30001,
        primary_group: "nixbld".into(),
    };
    let unit = handlers::EnableSystemdUnitSpec {
        unit: "nix-daemon.socket".into(),
    };

    let samples = vec![
        serde_json::to_value(file_spec(dir.path(), 2)).unwrap(),
        serde_json::to_value(&user).unwrap(),
        serde_json::to_value(&unit).unwrap(),
    ];

    for value in samples {
        let name = value["step"].as_str().expect("tag present").to_string();
        let vtable = by_name(&name).unwrap_or_else(|| panic!("no vtable for {name}"));
        let erased = (vtable.deserialize)(value).expect("deserializes");
        assert_eq!(erased.name(), name);
        assert_eq!(erased.meta().name, name);
    }
}

#[test]
fn erased_dispatch_assesses() {
    let dir = tempfile::tempdir().unwrap();
    let spec = file_spec(dir.path(), 3);
    let value = serde_json::to_value(&spec).unwrap();
    let erased = (by_name("create_file").unwrap().deserialize)(value).unwrap();

    let mut session = Session::new(HostFacts::fabricated(false), Executor::real());
    assert_eq!(erased.assess(&session).unwrap(), Assessment::Todo);
    erased.apply(&mut session).unwrap();
    assert_eq!(erased.assess(&session).unwrap(), Assessment::Done);
    erased.revert(&mut session).unwrap();
    assert_eq!(erased.assess(&session).unwrap(), Assessment::Todo);
}
