#![allow(dead_code)]

use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn golden(dir: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden")
        .join(dir)
}

pub fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp/runtime-tests")
        .join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn n(v: &Value) -> f32 {
    v.as_f64().unwrap() as f32
}

pub fn v3(v: &Value) -> [f32; 3] {
    [n(&v[0]), n(&v[1]), n(&v[2])]
}

pub fn v4(v: &Value) -> [f32; 4] {
    [n(&v[0]), n(&v[1]), n(&v[2]), n(&v[3])]
}

pub fn json_chunk(glb: &[u8]) -> (Value, Vec<u8>) {
    let length = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let json = serde_json::from_slice(&glb[20..20 + length]).unwrap();
    (json, glb[20 + length..].to_vec())
}

pub fn assemble(json: &Value, rest: &[u8]) -> Vec<u8> {
    let mut body = serde_json::to_vec(json).unwrap();
    while !body.len().is_multiple_of(4) {
        body.push(b' ');
    }
    let mut out = b"glTF".to_vec();
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&((12 + 8 + body.len() + rest.len()) as u32).to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&body);
    out.extend_from_slice(rest);
    out
}

pub fn without_extras(glb: &[u8]) -> Vec<u8> {
    let (mut json, rest) = json_chunk(glb);
    for node in json["nodes"].as_array_mut().unwrap() {
        node.as_object_mut().unwrap().remove("extras");
    }
    assemble(&json, &rest)
}
