---
title: "Your app's own generators"
description: "Link the manfred library into a small build binary of your app's own, with generators that make meshes, a sidecar writer and a material catalogue, and load the result back with the runtime loader."
sidebar:
  order: 7
---

Manfred holds no app's things. An app keeps its generators, its sidecar writer, its material catalogue and its recipes in its own repository, and drives Manfred through a small build binary that links the `manfred` library and registers them. That binary is `manfred` with the app's names added: every command, flag and output is the same.

## Link the library

```toml
[dependencies]
manfred = { git = "https://github.com/gmrdad82/manfred.git", tag = "vX.Y.Z" }
serde_json = "1"
```

Replace `vX.Y.Z` with a [release](https://github.com/gmrdad82/manfred/releases) tag. The [API reference](../../api/manfred/index.html) lists every public item.

## A generator

A generator takes the `params` a recipe gives it and returns named pieces of mesh:

```rust
use std::process::ExitCode;

use manfred::build::{Call, Piece, Registry};
use manfred::mesh::rounded_box;

fn cube(call: &Call) -> Result<Vec<Piece>, String> {
    let name = call.params.get("name").and_then(|v| v.as_str()).ok_or("wants a `name`")?;
    let side = call
        .params
        .get("side")
        .map_or(Some(1.0), |v| v.as_f64())
        .ok_or("`side` wants a number")?;
    let mut mesh = rounded_box([side; 3], 0.0, 1);
    mesh.set_material("cube");
    Ok(vec![Piece { name: name.to_string(), mesh }])
}

fn main() -> ExitCode {
    let mut registry = Registry::new();
    registry.register("app/cube", cube);
    registry
        .register_materials_toml("[materials.cube]\nbase = [0.8, 0.3, 0.1]\nroughness = 0.35\n")
        .expect("the app's materials are TOML");
    let args: Vec<String> = std::env::args().skip(1).collect();
    manfred::cli::main(&args, &registry)
}
```

A recipe asks for it by name:

```toml
[[object]]
name = "big"
kind = "generator"
generator = "app/cube"
params = { name = "big_cube", side = 2.0 }
```

An `Err` from a generator is reported with the object and the generator named: ``object[big].generator: app/cube: wants a `name` ``. `manfred` alone refuses a recipe that names a generator only the app registers. `manfred::generators` holds a kit for them: gradient noise, CPython's and numpy's random generators call for call, and a face-list mesh type with Blender's normals and triangulation, for porting generator scripts.

## A sidecar of your own

The `facts` sidecar dialect is Manfred's; an app that reads other keys registers its own writer under a dialect name, reading the scene's typed facts:

```rust
use manfred::build::SceneFacts;
use serde_json::{Value, json};

fn sidecar(facts: &SceneFacts) -> Result<Value, String> {
    let camera = facts.cameras.first().ok_or("the app's sidecar wants a camera")?;
    Ok(json!({ "app": 1, "eye": camera.position, "look_at": camera.target }))
}

// registry.register_sidecar("app", sidecar);
```

and its recipes say `sidecar_dialect = "app"` in `[output]`. A recipe's `[sidecar]` table is merged over what the writer returns, for keys the app owns.

## Commands of your own

`manfred::cli::main_with(&args, &registry, &host)` adds subcommands: each `Command { name, usage, run }` runs when its name is the first argument, with the rest of the line as given, and shows in `--help`.

## Load it in the app

`manfred::runtime` turns a GLB and its sidecar back into plain data an app or a game draws:

```rust
let scene = manfred::runtime::load_files(Path::new("scene/desk.glb"), Path::new("scene/desk.json"))?;
```

`load_read` takes the app's own sidecar reader instead of `facts`. The runtime never touches the GPU or a window; its raster half hands the scene to the engine's live renderer.

The complete example, with a test, is [`examples/app_build.rs`](https://github.com/gmrdad82/manfred/blob/main/examples/app_build.rs).
