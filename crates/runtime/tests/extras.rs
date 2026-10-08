mod common;

use common::{assemble, json_chunk, without_extras};
use manfred_runtime::{Error, load, load_dir};
use serde_json::Value;
use std::path::PathBuf;

fn golden() -> PathBuf {
    common::golden("toybox")
}

fn glb() -> Vec<u8> {
    std::fs::read(golden().join("toybox.glb")).unwrap()
}

fn sidecar() -> Value {
    serde_json::from_str(&std::fs::read_to_string(golden().join("toybox.json")).unwrap()).unwrap()
}

fn text(value: &Value) -> String {
    serde_json::to_string(value).unwrap()
}

fn doctored(change: impl FnOnce(&mut Value)) -> Vec<u8> {
    let (mut json, rest) = json_chunk(&glb());
    change(&mut json);
    assemble(&json, &rest)
}

fn root(json: &mut Value, group: &str) -> usize {
    json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .position(|n| {
            n["extras"]["manfred"]["group"] == group
                && n["extras"]["manfred"].get("pivot").is_some()
        })
        .unwrap()
}

#[test]
fn the_toyboxs_glb_gives_the_same_movers_from_its_extras_as_from_its_sidecar() {
    let sidecar_only = load(&without_extras(&glb()), &text(&sidecar())).unwrap();
    let extras_only = load(&glb(), "").unwrap();
    let both = load_dir(&golden(), "toybox").unwrap();
    assert_eq!(sidecar_only.movers.len(), 4);
    assert_eq!(extras_only.movers, sidecar_only.movers);
    assert_eq!(both.movers, sidecar_only.movers);
}

#[test]
fn a_glb_without_extras_loads_as_it_did() {
    let plain = without_extras(&glb());
    let a = load(&plain, &text(&sidecar())).unwrap();
    assert_eq!(a.movers.len(), 4);
    assert!(load(&plain, "").unwrap().movers.is_empty());
}

#[test]
fn the_extras_do_not_change_the_geometry() {
    let a = load(&without_extras(&glb()), &text(&sidecar())).unwrap();
    let mut b = load(&glb(), &text(&sidecar())).unwrap();
    assert!(a.groups.iter().all(Option::is_none));
    assert!(b.groups.iter().any(Option::is_some));
    b.groups = a.groups.clone();
    assert!(a == b);
}

#[test]
fn a_doctored_sidecar_is_a_sidecar_error_naming_the_group() {
    let mut meta = sidecar();
    meta["movers"]["knob"]["travel"] = Value::from(0.5);
    let error = load(&glb(), &text(&meta)).unwrap_err();
    assert!(
        matches!(&error, Error::Sidecar(reason) if reason.contains("`knob`")),
        "{error}"
    );
}

#[test]
fn a_doctored_pivot_in_the_extras_is_the_same_error() {
    let mut meta = sidecar();
    meta["movers"]["lid"]["pivot"][0] = Value::from(9.0);
    let error = load(&glb(), &text(&meta)).unwrap_err();
    assert!(
        matches!(&error, Error::Sidecar(reason) if reason.contains("`lid`")),
        "{error}"
    );
}

#[test]
fn a_group_only_the_sidecar_has_is_named() {
    let mut meta = sidecar();
    meta["movers"]["ghost"] = meta["movers"]["knob"].clone();
    let error = load(&glb(), &text(&meta)).unwrap_err();
    assert!(
        matches!(&error, Error::Sidecar(reason) if reason.contains("`ghost`")),
        "{error}"
    );
}

#[test]
fn a_group_only_the_extras_have_is_named() {
    let mut meta = sidecar();
    meta["movers"].as_object_mut().unwrap().remove("knob");
    let error = load(&glb(), &text(&meta)).unwrap_err();
    assert!(
        matches!(&error, Error::Sidecar(reason) if reason.contains("`knob`")),
        "{error}"
    );
}

#[test]
fn a_sidecar_without_movers_takes_them_from_the_extras() {
    let mut meta = sidecar();
    meta.as_object_mut().unwrap().remove("movers");
    let scene = load(&glb(), &text(&meta)).unwrap();
    assert_eq!(scene.movers, load(&glb(), "").unwrap().movers);
    assert!(scene.mover("knob").is_some());
}

#[test]
fn a_root_without_a_group_and_two_roots_for_one_group_are_glb_errors() {
    let bytes = doctored(|json| {
        let at = root(json, "knob");
        json["nodes"][at]["extras"]["manfred"]
            .as_object_mut()
            .unwrap()
            .remove("group");
    });
    assert!(matches!(load(&bytes, ""), Err(Error::Glb(_))));
    let bytes = doctored(|json| {
        let at = root(json, "knob");
        let twin = json["nodes"][at]["extras"].clone();
        let other = root(json, "lid");
        json["nodes"][other]["extras"] = twin;
    });
    let error = load(&bytes, "").unwrap_err();
    assert!(
        matches!(&error, Error::Glb(reason) if reason.contains("`knob`")),
        "{error}"
    );
}

#[test]
fn another_apps_own_extras_are_left_alone() {
    let bytes = doctored(|json| {
        json["nodes"][0]["extras"] = serde_json::json!({"mover": true, "speed": 2.5});
    });
    let scene = load(&bytes, &text(&sidecar())).unwrap();
    assert_eq!(scene.movers.len(), 4);
}

#[test]
fn a_glb_with_the_old_extras_key_is_a_glb_error_naming_the_new_one() {
    let bytes = doctored(|json| {
        let at = root(json, "knob");
        let old = json["nodes"][at]["extras"]["manfred"].clone();
        json["nodes"][at]["extras"] = serde_json::json!({ "manf": old });
    });
    let error = load(&bytes, "").unwrap_err();
    assert!(
        matches!(&error, Error::Glb(reason) if reason.contains("`extras.manf`") && reason.contains("`extras.manfred`")),
        "{error}"
    );
    assert!(matches!(
        load(&bytes, &text(&sidecar())),
        Err(Error::Glb(_))
    ));
}
