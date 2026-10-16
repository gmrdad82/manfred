use std::collections::BTreeMap;

use crate::variant::{apply_variant, glob_matches};
use crate::{
    Bake, Camera, CameraPath, Casters, ConstraintKind, Follow, Kind, Owner, PathKind, PlateView,
    Plates, Problem, Projection, Recipe, Rig, RigFocus, RigTracks, SCENE_SUFFIX, SkyKind, Trace,
    Uv, View,
};

struct Out<'a> {
    r: &'a Recipe,
    list: Vec<Problem>,
}

struct At {
    path: String,
    ip: String,
}

impl At {
    fn new(path: impl Into<String>, ip: impl Into<String>) -> At {
        At {
            path: path.into(),
            ip: ip.into(),
        }
    }

    fn child(&self, path: &str, ip: &str) -> At {
        At::new(format!("{}.{path}", self.path), format!("{}.{ip}", self.ip))
    }
}

impl Out<'_> {
    fn add(&mut self, path: impl Into<String>, ipath: &str, text: impl Into<String>) {
        self.list.push(self.r.problem(path, ipath, text));
    }

    fn same(&mut self, both: &str, text: impl Into<String>) {
        self.add(both, both, text);
    }

    fn at(&mut self, at: &At, key: &str, text: impl Into<String>) {
        self.add(
            format!("{}.{key}", at.path),
            &format!("{}.{key}", at.ip),
            text,
        );
    }

    fn near(&mut self, at: &At, key: &str, text: impl Into<String>) {
        self.add(format!("{}.{key}", at.path), &at.ip, text);
    }

    fn missing(&mut self, at: &At, key: &str, what: &str, name: &str, expected: &str) {
        self.at(
            at,
            key,
            format!("no {what} named `{name}`, expected {expected}"),
        );
    }
}

const MAX_GUTTER: u32 = 64;

pub fn missing_material_text<'a>(
    defined: impl IntoIterator<Item = &'a str>,
    material: &str,
) -> String {
    let known: Vec<&str> = defined.into_iter().collect();
    let known = if known.is_empty() {
        "it defines none".to_string()
    } else {
        format!("it defines {}", known.join(", "))
    };
    format!(
        "material \"{material}\" is defined by neither the app's catalogue nor the recipe's [materials] ({known}), expected a [materials.{material}] table or a catalogue entry; the engine holds no presets"
    )
}

pub fn validate(r: &Recipe) -> Vec<Problem> {
    let mut out = Out {
        r,
        list: Vec::new(),
    };
    objects(&mut out);
    uses(&mut out);
    cameras(&mut out);
    plate(&mut out);
    lights(&mut out);
    movers(&mut out);
    routes(&mut out);
    follows(&mut out);
    stages(&mut out);
    markers(&mut out);
    constraints(&mut out);
    output(&mut out);
    sky(&mut out);
    look(&mut out);
    trace(&mut out);
    bake(&mut out);
    sidecar(&mut out);
    materials(&mut out);
    keyframes(&mut out);
    if let Some(d) = &r.daylight {
        if !(0.0..=24.0).contains(&d.hour) {
            out.same("sun.hour", "expected an hour from 0 to 24");
        }
        if d.reference_hour != crate::REFERENCE_HOUR {
            out.same(
                "sun.reference_hour",
                format!(
                    "Manfred lights every scene against its reference hour, expected `reference_hour = {:?}` or the key removed",
                    crate::REFERENCE_HOUR
                ),
            );
        }
    }
    if r.plate.is_none() {
        for twin in &r.twins {
            out.list.push(twin.clone());
        }
    }
    variants(&mut out);
    let mut list = out.list;
    list.sort_by(|a, b| (a.line, &a.path, &a.text).cmp(&(b.line, &b.path, &b.text)));
    list
}

fn keyframes(out: &mut Out<'_>) {
    let r = out.r;
    let constrained = r.constrained();
    let driven = |owner: Owner| constrained.iter().any(|c| c.owner == owner);
    for (index, object) in r.objects.iter().enumerate() {
        if r.keyed.contains_key(&format!("object.{}", object.name))
            && driven(Owner::Object(object.name.clone()))
        {
            out.add(
                format!("object[{}].keys", object.name),
                &format!("object#{index}.keys"),
                "a [[follow]] or a constraint drives this object's transform, expected its keys or its constraints removed",
            );
        }
    }
    let scene_camera = r.scene_camera().map(|c| c.name.as_str());
    for (index, camera) in r.cameras.iter().enumerate() {
        let Some(keyed) = r.keyed.get(&format!("camera.{}", camera.name)) else {
            continue;
        };
        let at = At::new(
            format!("camera[{}].keys", camera.name),
            format!("camera#{index}.keys"),
        );
        if driven(Owner::Camera(camera.name.clone())) {
            for key in ["position", "target"] {
                if keyed.contains_key(key) {
                    out.at(
                        &at,
                        key,
                        "the camera's [camera.rig] or constraints place and aim it, expected the keyed position and target removed or the constraints",
                    );
                }
            }
        }
        if r.rig_of(camera)
            .is_some_and(|rig| rig.focus == Some(RigFocus::Follow))
            && keyed.contains_key("focus_m")
        {
            out.at(
                &at,
                "focus_m",
                "rig.focus = \"follow\" already sets the focus over time, expected one of the two",
            );
        }
        if keyed.contains_key("exposure") {
            if scene_camera != Some(camera.name.as_str()) {
                out.at(
                    &at,
                    "exposure",
                    "a camera's keyed exposure is the scene's [finish] exposure, so only the scene camera (output.scene_camera, else the first) keys it",
                );
            } else if r
                .keyed
                .get("look")
                .is_some_and(|k| k.contains_key("exposure"))
            {
                out.at(
                    &at,
                    "exposure",
                    "[look] keys the exposure too, expected one of the two",
                );
            }
        }
    }
}

fn materials(out: &mut Out<'_>) {
    for (name, value) in &out.r.materials {
        let e = match serde_json::from_value::<pfx_materials::Material>(value.clone()) {
            Ok(material) => {
                lobes(out, name, &material);
                continue;
            }
            Err(e) => e,
        };
        let text = e.to_string();
        let field = text
            .strip_prefix("unknown field `")
            .and_then(|rest| rest.split('`').next())
            .map(str::to_string)
            .or_else(|| {
                value.as_object()?.iter().find_map(|(key, one)| {
                    let alone = serde_json::json!({ key: one });
                    serde_json::from_value::<pfx_materials::Material>(alone)
                        .is_err()
                        .then(|| key.clone())
                })
            });
        let path = match field {
            Some(field) => format!("materials.{name}.{field}"),
            None => format!("materials.{name}"),
        };
        out.same(&path, text);
    }
}

fn lobes(out: &mut Out<'_>, name: &str, material: &pfx_materials::Material) {
    let unit = |v: &f32| (0.0..=1.0).contains(v);
    let colour = "expected a linear RGB colour of three numbers from 0 to 1";
    let one = "expected a number from 0 to 1";
    for (key, fine, expected, found) in [
        (
            "fabric.color",
            material.fabric.color.iter().all(unit),
            colour,
            format!("{:?}", material.fabric.color),
        ),
        (
            "fabric.roughness",
            unit(&material.fabric.roughness),
            one,
            material.fabric.roughness.to_string(),
        ),
        (
            "anisotropy.strength",
            unit(&material.anisotropy.strength),
            one,
            material.anisotropy.strength.to_string(),
        ),
        (
            "anisotropy.rotation",
            material.anisotropy.rotation.is_finite(),
            "expected a finite angle in radians",
            material.anisotropy.rotation.to_string(),
        ),
        (
            "scatter.color",
            material.scatter.color.iter().all(unit),
            colour,
            format!("{:?}", material.scatter.color),
        ),
        (
            "scatter.radius",
            material.scatter.radius > 0.0 && material.scatter.radius.is_finite(),
            "expected a radius above 0, in metres",
            material.scatter.radius.to_string(),
        ),
        (
            "scatter.amount",
            unit(&material.scatter.amount),
            one,
            material.scatter.amount.to_string(),
        ),
    ] {
        if !fine {
            out.same(
                &format!("materials.{name}.{key}"),
                format!("{expected}, found {found}"),
            );
        }
    }
}

fn duplicates(out: &mut Out<'_>, what: &str, key: &str, names: &[&str]) {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, name) in names.iter().enumerate() {
        let ipath = format!("{what}#{i}.{key}");
        if name.is_empty() {
            out.add(
                format!("{what}[].{key}"),
                &ipath,
                "expected a non-empty name",
            );
            continue;
        }
        match seen.get(name) {
            Some(&first) => {
                let first_line = out
                    .r
                    .line_of(&format!("{what}#{first}.{key}"))
                    .map_or_else(String::new, |l| format!(", first used on line {l}"));
                out.add(
                    format!("{what}[{name}].{key}"),
                    &ipath,
                    format!("duplicate name{first_line}, expected a name no other {what} uses"),
                );
            }
            None => {
                seen.insert(name, i);
            }
        }
    }
}

fn positive(out: &mut Out<'_>, at: &At, key: &str, value: f64) {
    if value <= 0.0 {
        out.at(at, key, "expected a number above 0");
    }
}

fn sizes(out: &mut Out<'_>, at: &At, o: &crate::Object) {
    let size = match &o.kind {
        Kind::Plane { size, .. } | Kind::Card { size, .. } => size,
        _ => return,
    };
    if size.iter().any(|v| *v <= 0.0) {
        out.at(at, "size", "expected both sizes above 0");
    }
}

fn at_least(out: &mut Out<'_>, at: &At, key: &str, value: u32, min: u32) {
    if value < min {
        out.at(at, key, format!("expected a whole number of {min} or more"));
    }
}

fn text(
    out: &mut Out<'_>,
    at: &At,
    spans: &[crate::Span],
    wrap: Option<f64>,
    anchor: Option<&str>,
) {
    for (i, span) in spans.iter().enumerate() {
        let at = at.child(&format!("spans[{i}]"), &format!("spans#{i}"));
        positive(out, &at, "size", span.size);
        if let Some(weight) = span.weight
            && !(1..=1000).contains(&weight)
        {
            out.at(
                &at,
                "weight",
                "expected a weight from 1 to 1000, 400 regular and 700 bold",
            );
        }
        if span.color.iter().any(|c| !c.is_finite() || *c < 0.0) {
            out.at(&at, "color", "expected linear RGB of 0 or more");
        }
        if span.family.as_deref() == Some("") {
            out.at(&at, "family", "expected \"mono\" or a font file's path");
        }
    }
    if let Some(wrap) = wrap {
        positive(out, at, "wrap", wrap);
    }
    if let Some(anchor) = anchor
        && crate::anchor_point(anchor).is_none()
    {
        out.at(
            at,
            "anchor",
            format!(
                "expected one of: {}, found \"{anchor}\"",
                crate::anchor_names()
            ),
        );
    }
}

fn uses(out: &mut Out<'_>) {
    let r = out.r;
    for (i, u) in r.uses.iter().enumerate() {
        let at = At::new(format!("use[{}]", u.prefix), format!("use#{i}"));
        let Some(parent) = &u.parent else {
            continue;
        };
        if u.objects.contains(parent) {
            out.at(
                &at,
                "parent",
                format!(
                    "`{parent}` is one of this use's own copies, expected an object of the recipe"
                ),
            );
        } else if !r.objects.iter().any(|o| &o.name == parent) {
            out.missing(
                &at,
                "parent",
                "object",
                parent,
                "the name of an object of the recipe",
            );
        }
    }
}

fn ids(out: &mut Out<'_>) {
    let r = out.r;
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, o) in r.objects.iter().enumerate() {
        let Some(id) = &o.id else {
            continue;
        };
        let at = At::new(format!("object[{}]", o.name), format!("object#{i}"));
        if let Err(e) = pfx_scene::Id::parse(id) {
            out.at(
                &at,
                "id",
                format!(
                    "`{id}` is not an id ({e}), expected 10 lowercase Crockford base32 characters, such as \"7k2m9q4xzr\""
                ),
            );
            continue;
        }
        match seen.get(id.as_str()) {
            Some(&first) => {
                let first = &r.objects[first].name;
                out.at(
                    &at,
                    "id",
                    format!(
                        "`{id}` is also the id of `{first}`, expected an id no other object uses"
                    ),
                );
            }
            None => {
                seen.insert(id, i);
            }
        }
    }
}

fn objects(out: &mut Out<'_>) {
    let r = out.r;
    let names: Vec<&str> = r.objects.iter().map(|o| o.name.as_str()).collect();
    duplicates(out, "object", "name", &names);
    ids(out);
    for (i, o) in r.objects.iter().enumerate() {
        let at = At::new(format!("object[{}]", o.name), format!("object#{i}"));
        if let Some(parent) = &o.parent {
            if parent == &o.name {
                out.at(&at, "parent", "an object cannot be its own parent");
            } else if !names.contains(&parent.as_str()) {
                out.missing(
                    &at,
                    "parent",
                    "object",
                    parent,
                    "the name of another object",
                );
            } else if cycle(r, &o.name) {
                out.at(&at, "parent", "the parents form a loop");
            }
        }
        if o.material.is_some() && !o.materials.is_empty() {
            out.at(
                &at,
                "materials",
                "`material` and `materials` are both set, expected only one",
            );
        }
        if o.smooth && o.smooth_angle.is_some() {
            out.at(
                &at,
                "smooth_angle",
                "`smooth` and `smooth_angle` are both set, expected only one",
            );
        }
        if o.flat_faces && o.smooth_angle.is_none() {
            out.at(
                &at,
                "flat_faces",
                "`flat_faces` needs `smooth_angle`, expected both",
            );
        }
        uv(out, &at, o);
        detail_uv(out, &at, o);
        if o.bevel.is_some() && !matches!(o.kind, Kind::Box { .. } | Kind::Cylinder { .. }) {
            out.at(
                &at,
                "bevel",
                "`bevel` is for box and cylinder, expected `profile` or `rim` for this kind",
            );
        }
        kind(out, &at, o, &names);
        plate_keys(out, &at, o, i);
    }
}

fn plate_keys(out: &mut Out<'_>, at: &At, o: &crate::Object, index: usize) {
    let r = out.r;
    if o.dynamic.is_some()
        && let Kind::Pocket { into, .. } = &o.kind
    {
        out.at(
            at,
            "dynamic",
            format!("a pocket is cut into its target, expected `dynamic` on `{into}`"),
        );
    }
    if o.proxy.is_none() {
        return;
    }
    let writes = r
        .bake
        .as_ref()
        .and_then(|bake| bake.plates.as_ref())
        .is_some_and(|plates| plates.dir.is_some());
    if matches!(o.kind, Kind::Pocket { .. } | Kind::Text { .. }) || o.hidden {
        out.at(
            at,
            "proxy",
            format!(
                "a proxy stands in for an object the scene draws, and this {} draws no mesh of its own, expected no `proxy`",
                if o.hidden { "hidden object" } else { o.kind.name() }
            ),
        );
    } else if r.dynamic(index) {
        out.at(
            at,
            "proxy",
            "proxies stand in for static objects and this object is dynamic, expected no `proxy`",
        );
    } else if !writes {
        out.at(
            at,
            "proxy",
            "no proxies file is written without a plate folder, expected `dir` in `[bake.plates]`",
        );
    }
}

fn uv(out: &mut Out<'_>, at: &At, o: &crate::Object) {
    let r = out.r;
    match &o.uv {
        Some(Uv::Plate) if r.plate.is_none() => out.at(
            at,
            "uv",
            "uv \"plate\" projects onto the plate, so the recipe needs a `[plate]`",
        ),
        Some(Uv::Plate) if r.cameras.is_empty() => out.at(
            at,
            "uv",
            "uv \"plate\" projects through the plate's camera, so the recipe needs a `[[camera]]`",
        ),
        Some(Uv::Rect { rect, .. }) if rect[2] <= rect[0] || rect[3] <= rect[1] => out.at(
            &at.child("uv", "uv"),
            "rect",
            "the rect has no area, expected x1 > x0 and y1 > y0",
        ),
        Some(Uv::Cylinder { band, wall, .. }) => {
            for (name, span) in [("band", band), ("wall", wall)] {
                if let Some([lo, hi]) = span
                    && hi <= lo
                {
                    out.at(
                        &at.child("uv", "uv"),
                        name,
                        "expected [low, high] with high above low",
                    );
                }
            }
        }
        _ => {}
    }
}

fn detail_uv(out: &mut Out<'_>, at: &At, o: &crate::Object) {
    let Some(detail) = &o.detail_uv else {
        return;
    };
    let inner = at.child("detail_uv", "detail_uv");
    if !(detail.texels_per_m > 0.0 && detail.texels_per_m.is_finite()) {
        out.at(&inner, "texels_per_m", "expected a number above 0");
    }
    if detail.gutter > MAX_GUTTER {
        out.at(
            &inner,
            "gutter",
            format!("expected at most {MAX_GUTTER} texels"),
        );
    }
    let empty = match o.kind {
        Kind::Pocket { .. } => Some("a pocket cuts into its target, so `detail_uv` goes there"),
        Kind::Text { .. } => Some("text carries no mesh, so it takes no `detail_uv`"),
        _ => None,
    };
    if let Some(text) = empty {
        out.at(at, "detail_uv", text);
    }
    if o.uv == Some(Uv::None) {
        out.at(
            at,
            "detail_uv",
            "`detail_uv` writes TEXCOORD_1 beside TEXCOORD_0, so it does not go with uv \"none\"",
        );
    }
}

fn plate(out: &mut Out<'_>) {
    let r = out.r;
    let Some(camera) = r.plate.as_ref().and_then(|p| p.camera.as_deref()) else {
        return;
    };
    if !r.cameras.iter().any(|c| c.name == camera) {
        out.missing(
            &At::new("plate", "plate"),
            "camera",
            "camera",
            camera,
            "the name of a `[[camera]]`",
        );
    }
}

fn cycle(r: &Recipe, start: &str) -> bool {
    let mut at = start;
    for _ in 0..=r.objects.len() {
        let Some(parent) = r
            .objects
            .iter()
            .find(|o| o.name == at)
            .and_then(|o| o.parent.as_deref())
        else {
            return false;
        };
        if parent == start {
            return true;
        }
        at = parent;
    }
    true
}

fn kind(out: &mut Out<'_>, at: &At, o: &crate::Object, names: &[&str]) {
    match &o.kind {
        Kind::Box { size } => {
            for (n, v) in ["x", "y", "z"].iter().zip(size) {
                if *v <= 0.0 {
                    out.at(at, "size", format!("expected the {n} size above 0"));
                }
            }
        }
        Kind::Cylinder {
            radius,
            depth,
            segments,
            ..
        } => {
            positive(out, at, "radius", *radius);
            positive(out, at, "depth", *depth);
            at_least(out, at, "segments", *segments, 3);
        }
        Kind::Sphere {
            radius,
            segments,
            rings,
        } => {
            positive(out, at, "radius", *radius);
            at_least(out, at, "segments", *segments, 3);
            at_least(out, at, "rings", *rings, 2);
        }
        Kind::Plane {
            bend, curl, cup, ..
        } => {
            sizes(out, at, o);
            if let Some(b) = bend {
                positive(out, at, "bend.radius", b.radius);
            }
            if let Some(c) = curl {
                positive(out, at, "curl.width", c.width);
            }
            if let Some(c) = cup
                && !(0.0..1.0).contains(&c.from)
            {
                out.at(
                    at,
                    "cup.from",
                    "expected a fraction of the half width from 0 up to but not including 1",
                );
            }
        }
        Kind::Card { .. } => sizes(out, at, o),
        Kind::Disc { radius, segments } => {
            positive(out, at, "radius", *radius);
            at_least(out, at, "segments", *segments, 3);
        }
        Kind::Torus {
            major,
            minor,
            segments,
            rings,
        } => {
            positive(out, at, "major", *major);
            positive(out, at, "minor", *minor);
            at_least(out, at, "segments", *segments, 3);
            at_least(out, at, "rings", *rings, 3);
        }
        Kind::Capsule { radius, length } => {
            positive(out, at, "radius", *radius);
            positive(out, at, "length", *length);
        }
        Kind::Pocket { into, depth, .. } => {
            positive(out, at, "depth", *depth);
            if into == &o.name {
                out.at(at, "into", "a pocket cannot cut into itself");
            } else if !names.contains(&into.as_str()) {
                out.missing(
                    at,
                    "into",
                    "object",
                    into,
                    "the object the pocket cuts into",
                );
            }
        }
        Kind::Lathe {
            profile, segments, ..
        } => {
            at_least(out, at, "segments", *segments, 3);
            if profile.len() < 2 {
                out.at(at, "profile", "expected at least 2 points");
            }
        }
        Kind::Sweep {
            radii,
            ring,
            along: Some(_),
            ..
        } => {
            at_least(out, at, "ring", *ring, 3);
            if radii.len() != 1 {
                out.at(
                    at,
                    "radii",
                    format!(
                        "a sweep along an outline takes one `radius`, found {}",
                        radii.len()
                    ),
                );
            }
            if radii.iter().any(|r| *r <= 0.0) {
                out.at(at, "radius", "expected a number above 0");
            }
        }
        Kind::Sweep {
            path: points,
            radii,
            ring,
            ..
        } => {
            at_least(out, at, "ring", *ring, 3);
            if points.len() < 2 {
                out.at(at, "path", "expected at least 2 points");
            }
            if radii.len() != 1 && radii.len() != points.len() {
                out.at(
                    at,
                    "radii",
                    format!(
                        "expected 1 radius or one per path point ({}), found {}",
                        points.len(),
                        radii.len()
                    ),
                );
            }
            if radii.iter().any(|r| *r <= 0.0) {
                out.at(at, "radii", "expected every radius above 0");
            }
        }
        Kind::Text {
            spans,
            wrap,
            anchor,
            on,
            ..
        } => {
            text(out, at, spans, *wrap, anchor.as_deref());
            if let Some(on) = on {
                if on == &o.name {
                    out.at(at, "on", "text cannot sit on itself");
                } else if !names.contains(&on.as_str()) {
                    out.missing(at, "on", "object", on, "the object the text sits on");
                }
                if o.parent.is_some() {
                    out.at(
                        at,
                        "parent",
                        "`on` and `parent` are both set, expected only one: `on` already places the text on its object",
                    );
                }
            }
        }
        _ => {}
    }
    if let Kind::Card { image, slot, .. } = &o.kind {
        match (image, slot) {
            (Some(_), Some(_)) => out.at(
                at,
                "slot",
                "`image` and `slot` are both set, expected only one",
            ),
            (None, None) => out.near(
                at,
                "image",
                "a card needs `image` (a file) or `slot` (a name)",
            ),
            _ => {}
        }
    }
}

fn cameras(out: &mut Out<'_>) {
    let r = out.r;
    let names: Vec<&str> = r.cameras.iter().map(|c| c.name.as_str()).collect();
    duplicates(out, "camera", "name", &names);
    for (i, c) in r.cameras.iter().enumerate() {
        let at = At::new(format!("camera[{}]", c.name), format!("camera#{i}"));
        match c.kind {
            Projection::Perspective => {
                match (c.fov_y_deg, c.focal_mm) {
                    (Some(_), Some(_)) => out.at(
                        &at,
                        "focal_mm",
                        "`fov_y_deg` and `focal_mm` are both set, expected only one",
                    ),
                    (None, None) => out.near(
                        &at,
                        "fov_y_deg",
                        "a perspective camera needs `fov_y_deg`, or `focal_mm` with `sensor_mm`",
                    ),
                    (None, Some(_)) if c.sensor_mm.is_none() => out.near(
                        &at,
                        "sensor_mm",
                        "`focal_mm` needs `sensor_mm`, the sensor's longest side in millimetres",
                    ),
                    _ => {}
                }
                if let Some(f) = c.fov_y_deg
                    && !(f > 0.0 && f < 180.0)
                {
                    out.at(&at, "fov_y_deg", "expected degrees above 0 and below 180");
                }
                if c.ortho_height.is_some() {
                    out.at(
                        &at,
                        "ortho_height",
                        "`ortho_height` is for orthographic cameras",
                    );
                }
            }
            Projection::Orthographic => {
                if c.shift != [0.0, 0.0] {
                    out.at(&at, "shift", "`shift` is only for perspective cameras");
                }
                match c.ortho_height {
                    None => out.near(
                        &at,
                        "ortho_height",
                        "an orthographic camera needs `ortho_height`, the view height in metres",
                    ),
                    Some(h) if h <= 0.0 => out.at(&at, "ortho_height", "expected a number above 0"),
                    _ => {}
                }
                if c.fov_y_deg.is_some() || c.focal_mm.is_some() {
                    out.near(
                        &at,
                        "fov_y_deg",
                        "`fov_y_deg` and `focal_mm` are for perspective cameras",
                    );
                }
            }
        }
        if c.position == c.target {
            out.at(
                &at,
                "target",
                "the target is the camera's own position, expected a point to look at",
            );
        }
        if let Some(path) = &c.path {
            camera_path(out, &at.child("path", "path"), c, path);
        }
        if let Some(rig) = &c.rig {
            camera_rig(out, &at, c, rig);
        }
    }
}

fn camera_rig(out: &mut Out<'_>, camera: &At, c: &Camera, rig: &Rig) {
    let at = camera.child("rig", "rig");
    if c.path.is_some() {
        out.near(
            camera,
            "rig",
            "a camera takes `path` or `rig`, expected only one",
        );
    }
    if c.kind != Projection::Perspective {
        out.near(camera, "rig", "a rig is for perspective cameras");
    }
    rig_fstop(out, &at, c, rig);
}

fn rig_fstop(out: &mut Out<'_>, at: &At, c: &Camera, rig: &Rig) {
    match rig.focus {
        Some(RigFocus::Distance(metres)) if !(metres > 0.0 && metres.is_finite()) => {}
        Some(_) if !(c.fstop > 0.0 && c.fstop.is_finite()) => out.at(
            at,
            "focus",
            "a rig's focus needs depth of field, expected an `fstop` above 0 on the camera",
        ),
        _ => {}
    }
}

const TARGET: &str =
    "an object, one of its pieces' nodes (`<object>.<piece>`), `camera.<name>` or `light.<name>`";

fn target(out: &mut Out<'_>, at: &At, key: &str, name: &str) {
    if out.r.target_owner(name).is_none() {
        out.missing(at, key, "object, camera or light", name, TARGET);
    }
}

fn rig_keys(out: &mut Out<'_>, at: &At, rig: &Rig, camera: bool) {
    target(out, at, "follow", &rig.follow);
    if rig.offset == rig.look {
        out.at(
            at,
            "look",
            "the aim point is the camera's own offset, expected a point to look at",
        );
    }
    if !(rig.lag >= 0.0 && rig.lag.is_finite()) {
        out.at(at, "lag", "expected seconds of 0 or more");
    }
    if !(rig.look_ahead >= 0.0 && rig.look_ahead.is_finite()) {
        out.at(at, "look_ahead", "expected seconds of 0 or more");
    }
    if !(rig.yaw_lag >= 0.0 && rig.yaw_lag.is_finite()) {
        out.at(at, "yaw_lag", "expected seconds of 0 or more");
    }
    if !rig.bank.is_finite() {
        out.at(
            at,
            "bank",
            "expected degrees per m/s² of sideways acceleration",
        );
    }
    if let Some(shake) = &rig.shake {
        let at = at.child("shake", "shake");
        if !(shake.translation >= 0.0 && shake.translation.is_finite()) {
            out.at(&at, "translation", "expected metres of 0 or more");
        }
        if !(shake.roll >= 0.0 && shake.roll < 90.0) {
            out.at(&at, "roll", "expected degrees from 0 to below 90");
        }
        if !(shake.frequency > 0.0 && shake.frequency.is_finite()) {
            out.at(&at, "frequency", "expected hertz above 0");
        }
    }
    let keyed = at.child("tracks", "tracks");
    for name in RigTracks::NAMES {
        let Some(keys) = rig.tracks.get(name) else {
            continue;
        };
        if keys.is_empty() {
            out.at(&keyed, name, "expected one [t, value] key at least");
        } else if keys.iter().flatten().any(|v| !v.is_finite()) {
            out.at(&keyed, name, "expected finite times and values");
        } else if keys.windows(2).any(|w| w[1][0] <= w[0][0]) {
            out.at(&keyed, name, "expected the keys' times to rise");
        } else if keys[0][0] < 0.0 {
            out.at(&keyed, name, "expected times from 0 up");
        } else if name == "distance" && keys.iter().any(|k| k[1] <= 0.0) {
            out.at(&keyed, name, "expected metres above 0 behind the object");
        } else if matches!(name, "lag" | "shake" | "look_ahead") && keys.iter().any(|k| k[1] < 0.0)
        {
            out.at(&keyed, name, "expected values of 0 or more");
        } else if name == "focal" && keys.iter().any(|k| k[1] <= 0.0) {
            out.at(&keyed, name, "expected millimetres above 0");
        } else if name == "fov" && keys.iter().any(|k| k[1] <= 0.0 || k[1] >= 180.0) {
            out.at(&keyed, name, "expected degrees above 0 and below 180");
        } else if name == "pitch" && keys.iter().any(|k| k[1].abs() > 90.0) {
            out.at(&keyed, name, "expected degrees from -90 to 90");
        }
    }
    if let Some(keys) = &rig.tracks.look_offset {
        let name = RigTracks::LOOK_OFFSET;
        if keys.is_empty() {
            out.at(&keyed, name, "expected one [t, [x, y, z]] key at least");
        } else if keys
            .iter()
            .any(|(t, v)| !t.is_finite() || v.iter().any(|c| !c.is_finite()))
        {
            out.at(&keyed, name, "expected finite times and values");
        } else if keys.windows(2).any(|w| w[1].0 <= w[0].0) {
            out.at(&keyed, name, "expected the keys' times to rise");
        } else if keys[0].0 < 0.0 {
            out.at(&keyed, name, "expected times from 0 up");
        }
    }
    if rig.tracks.focal.is_some() && rig.tracks.fov.is_some() {
        out.at(
            &keyed,
            "fov",
            "a lens is keyed by `focal` or `fov`, expected only one",
        );
    }
    if !camera && (rig.tracks.focal.is_some() || rig.tracks.fov.is_some()) {
        out.at(
            &keyed,
            if rig.tracks.fov.is_some() {
                "fov"
            } else {
                "focal"
            },
            "a lens is a camera's, expected the track removed on a rig of an object or a light",
        );
    }
    let smooth = at.child("smooth", "smooth");
    for (name, seconds) in &rig.smooth {
        if !(*seconds > 0.0 && seconds.is_finite()) {
            out.at(&smooth, name, "expected seconds above 0");
        }
    }
    if let Some(spring) = &rig.bank_spring {
        let at = at.child("bank_spring", "bank_spring");
        if !(spring.frequency > 0.0 && spring.frequency.is_finite()) {
            out.at(&at, "frequency", "expected hertz above 0");
        }
        if !(spring.damping > 0.0 && spring.damping.is_finite()) {
            out.at(&at, "damping", "expected a damping ratio above 0");
        }
    }
    if let Some(release) = rig.release
        && !(release >= 0.0 && release.is_finite())
    {
        out.at(at, "release", "expected seconds of 0 or more");
    }
    if !(rig.release_ease > 0.0 && rig.release_ease.is_finite()) {
        out.at(at, "release_ease", "expected seconds above 0");
    }
    match rig.focus {
        Some(_) if !camera => out.at(
            at,
            "focus",
            "focus is a camera's, expected the key removed on a rig of an object or a light",
        ),
        Some(RigFocus::Distance(metres)) if !(metres > 0.0 && metres.is_finite()) => {
            out.at(at, "focus", "expected \"follow\" or metres above 0")
        }
        _ => {}
    }
}

fn camera_path(out: &mut Out<'_>, at: &At, c: &Camera, path: &CameraPath) {
    match &path.kind {
        PathKind::Orbit(orbit) => {
            if orbit.radius_scale <= 0.0 {
                out.at(at, "radius_scale", "expected a number above 0");
            }
            match orbit.elevation_deg(c.position) {
                None => out.at(
                    at,
                    "target",
                    "the camera is straight above or below the orbit's target, expected a target the camera can turn around",
                ),
                Some(from) if (from + orbit.rise_deg).abs() >= 90.0 => out.at(
                    at,
                    "rise_deg",
                    format!(
                        "the camera starts {from:.1}° above the target and would end {:.1}° above it, expected an end between -90 and 90",
                        from + orbit.rise_deg
                    ),
                ),
                Some(_) => {}
            }
        }
        PathKind::Keys(keys) => {
            if keys.is_empty() {
                out.near(at, "keys", "expected at least one key");
            }
            let mut before = f64::NEG_INFINITY;
            for (i, key) in keys.iter().enumerate() {
                let here = at.child(&format!("keys[{i}]"), &format!("keys#{i}"));
                if !(0.0..=1.0).contains(&key.t) {
                    out.at(&here, "t", "expected a time from 0 to 1");
                } else if key.t <= before {
                    out.at(&here, "t", "expected a time after the key before it");
                }
                before = key.t;
                if key.position == key.target {
                    out.at(
                        &here,
                        "target",
                        "the target is the key's own position, expected a point to look at",
                    );
                }
                match key.focal_mm {
                    Some(f) if f <= 0.0 => {
                        out.at(&here, "focal_mm", "expected millimetres above 0")
                    }
                    Some(_) if c.kind != Projection::Perspective || c.focal_mm.is_none() => out.at(
                        &here,
                        "focal_mm",
                        "a key's focal_mm needs a perspective camera with its own focal_mm",
                    ),
                    _ => {}
                }
            }
        }
    }
}

fn lights(out: &mut Out<'_>) {
    let r = out.r;
    let names: Vec<&str> = r.lights.iter().map(|l| l.name.as_str()).collect();
    duplicates(out, "light", "name", &names);
    if r.lights.len() > 4 {
        for (i, l) in r.lights.iter().enumerate().skip(4) {
            out.add(
                format!("light[{}]", l.name),
                &format!("light#{i}"),
                format!(
                    "the engine takes 4 point lights at most, found {}",
                    r.lights.len()
                ),
            );
        }
    }
}

fn movers(out: &mut Out<'_>) {
    let r = out.r;
    let groups: Vec<&str> = r.movers.iter().map(|m| m.group.as_str()).collect();
    for (i, g) in groups.iter().enumerate() {
        if let Some(first) = groups.iter().position(|x| x == g)
            && first != i
        {
            out.add(
                format!("mover[{g}].group"),
                &format!("mover#{i}.group"),
                "duplicate group, expected a group no other mover uses",
            );
        }
    }
    for (i, m) in r.movers.iter().enumerate() {
        let at = At::new(format!("mover[{}]", m.group), format!("mover#{i}"));
        if m.axis == [0.0, 0.0, 0.0] {
            out.at(&at, "axis", "expected an axis that is not [0, 0, 0]");
        }
        for part in &m.parts {
            if !r.objects.iter().any(|o| &o.name == part) {
                out.missing(&at, "parts", "object", part, "the name of a part");
            }
        }
        let by_name = format!("mv_{}", m.group);
        let prefix = format!("mv_{}__", m.group);
        if m.parts.is_empty() && !r.objects.iter().any(|o| m.owns(&o.name)) {
            out.at(
                &at,
                "group",
                format!("no object named `{by_name}` or `{prefix}<part>`, and no `parts`, so the mover moves nothing"),
            );
        }
    }
}

fn routes(out: &mut Out<'_>) {
    let r = out.r;
    let names: Vec<&str> = r.routes.iter().map(|route| route.name.as_str()).collect();
    duplicates(out, "route", "name", &names);
    for (i, route) in r.routes.iter().enumerate() {
        if let Err(text) = manfred_motion::Route::new(&route.points, route.closed) {
            let at = At::new(format!("route[{}]", route.name), format!("route#{i}"));
            out.at(&at, "points", text);
        }
    }
}

fn stages(out: &mut Out<'_>) {
    let r = out.r;
    let names: Vec<&str> = r.stages.iter().map(|stage| stage.name.as_str()).collect();
    duplicates(out, "stage", "name", &names);
    for (i, stage) in r.stages.iter().enumerate() {
        let at = At::new(format!("stage[{}]", stage.name), format!("stage#{i}"));
        for glob in &stage.objects {
            if !r
                .objects
                .iter()
                .any(|o| crate::stage::selects(glob, &o.name))
            {
                out.at(
                    &at,
                    "objects",
                    format!("`{glob}` matches no object, expected an object's name, a glob, or <object>.<piece>"),
                );
            }
        }
        if let Some(camera) = &stage.camera
            && !r.cameras.iter().any(|c| c.name == *camera)
        {
            out.missing(
                &at,
                "camera",
                "camera",
                camera,
                "the name of a `[[camera]]`",
            );
        }
    }
}

fn markers(out: &mut Out<'_>) {
    let r = out.r;
    let names: Vec<&str> = r.markers.iter().map(|m| m.name.as_str()).collect();
    duplicates(out, "marker", "name", &names);
    for (i, m) in r.markers.iter().enumerate() {
        let at = At::new(format!("marker[{}]", m.name), format!("marker#{i}"));
        match &r.timeline {
            None => out.at(
                &at,
                "t",
                "a marker names a time on the timeline, expected a [timeline]",
            ),
            Some(timeline) => {
                let end = timeline.seconds();
                if !(m.t.is_finite() && (0.0..=end).contains(&m.t)) {
                    out.at(
                        &at,
                        "t",
                        format!("expected seconds within the timeline, from 0 to {end}"),
                    );
                }
            }
        }
    }
}

fn follows(out: &mut Out<'_>) {
    let r = out.r;
    for (i, f) in r.follows.iter().enumerate() {
        let at = At::new(format!("follow[{}]", f.object), format!("follow#{i}"));
        if r.follows[..i].iter().any(|g| g.object == f.object) {
            out.at(
                &at,
                "object",
                "another [[follow]] already moves this object, expected one per object",
            );
        }
        if !r.objects.iter().any(|o| o.name == f.object) {
            out.missing(&at, "object", "object", &f.object, "the name of an object");
        }
        if r.timeline.is_none() {
            out.at(
                &at,
                "object",
                "a follow moves its object along the timeline, expected a [timeline]",
            );
        }
        follow_keys(out, &at, f);
    }
}

fn follow_keys(out: &mut Out<'_>, at: &At, f: &Follow) {
    if !out.r.routes.iter().any(|route| route.name == f.route) {
        out.missing(at, "route", "route", &f.route, "the name of a [[route]]");
    }
    if f.start < 0.0 {
        out.at(at, "start", "expected metres along the route from 0 up");
    }
    if f.speed.is_empty() {
        out.at(at, "speed", "expected one [t, m/s] key at least");
    }
    if f.speed.windows(2).any(|w| w[1][0] <= w[0][0]) {
        out.at(
            at,
            "speed",
            "expected keys in rising time, each t after the last",
        );
    }
    if f.speed.iter().any(|k| k[0] < 0.0 || k[1] < 0.0) {
        out.at(at, "speed", "expected times and speeds from 0 up");
    }
    if let Some(d) = &f.drift {
        let at = at.child("drift", "drift");
        if d.amount < 0.0 {
            out.at(&at, "amount", "expected metres from 0 up");
        }
        if d.period <= 0.0 {
            out.at(&at, "period", "expected seconds above 0");
        }
    }
}

fn constraints(out: &mut Out<'_>) {
    let r = out.r;
    let all = r.constrained();
    for entry in &all {
        for c in &entry.list {
            let at = At::new(c.path.clone(), c.ipath.clone());
            let explicit = c.ipath.contains(".constraint#");
            let camera = match &entry.owner {
                Owner::Camera(name) => r.cameras.iter().find(|camera| camera.name == *name),
                _ => None,
            };
            if explicit {
                if r.timeline.is_none() {
                    out.at(
                        &at,
                        "type",
                        "a constraint moves its owner along the timeline, expected a [timeline]",
                    );
                }
                if let Some(camera) = camera {
                    if camera.path.is_some() {
                        out.at(
                            &at,
                            "type",
                            "a camera takes `path` or constraints, expected only one",
                        );
                    }
                    if camera.kind != Projection::Perspective {
                        out.at(&at, "type", "a constrained camera is a perspective camera");
                    }
                }
                if let Owner::Light(name) = &entry.owner
                    && r.lights
                        .iter()
                        .any(|l| l.name == *name && l.parent.is_some())
                {
                    out.at(
                        &at,
                        "type",
                        "a constrained light starts from its `position` and rides its own node, expected `position` in place of `parent`",
                    );
                }
            }
            match &c.kind {
                ConstraintKind::FollowPath(f) => {
                    if explicit {
                        follow_keys(out, &at, f);
                    }
                }
                ConstraintKind::LookAt(l) => {
                    target(out, &at, "target", &l.target);
                    if l.up.iter().all(|v| *v == 0.0) || l.up.iter().any(|v| !v.is_finite()) {
                        out.at(
                            &at,
                            "up",
                            "expected a finite direction that is not [0, 0, 0]",
                        );
                    }
                }
                ConstraintKind::Copy(copy) => target(out, &at, "target", &copy.target),
                ConstraintKind::Rig(rig) => {
                    rig_keys(out, &at, rig, camera.is_some());
                    if explicit && let Some(camera) = camera {
                        rig_fstop(out, &at, camera, rig);
                    }
                }
            }
        }
    }
    if let Err(cycle) = r.constraint_order(&all) {
        out.add(cycle.path, &cycle.ipath, cycle.text);
    }
}

fn scene(out: &mut Out<'_>) {
    let r = out.r;
    let o = &r.output;
    if let Some(scene) = &o.scene
        && !scene.ends_with(".toml")
    {
        out.same(
            "output.scene",
            format!("`{scene}` is not a TOML file, expected a path ending in `{SCENE_SUFFIX}`"),
        );
    }
    if o.scene_format > 1 {
        out.same(
            "output.scene_format",
            format!(
                "Manfred writes scene format 0 or 1, not {}, expected 1 (the default) or 0",
                o.scene_format
            ),
        );
    }
    if o.scene.is_none() && r.lines.0.contains_key("output.scene_format") {
        out.same(
            "output.scene_format",
            "says how to write the scene, but there is no `scene` to write, expected `output.scene`",
        );
    }
    let Some(camera) = &o.scene_camera else {
        return;
    };
    if o.scene.is_none() {
        out.same(
            "output.scene_camera",
            "names the scene's camera, but there is no `scene` to write, expected `output.scene`",
        );
    }
    if !r.cameras.iter().any(|c| c.name == *camera) {
        out.missing(
            &At::new("output", "output"),
            "scene_camera",
            "camera",
            camera,
            "the name of a `[[camera]]`",
        );
    }
}

fn output(out: &mut Out<'_>) {
    scene(out);
    let o = &out.r.output;
    let dialect = o.sidecar_dialect.as_str();
    if !o.writes_sidecar() {
        if o.sidecar.is_some() {
            out.add(
                "output.sidecar_dialect",
                "output.sidecar",
                "`sidecar` is set but `sidecar_dialect` is none, expected the name of a registered sidecar writer",
            );
        }
        return;
    }
    if dialect.is_empty() {
        out.same(
            "output.sidecar_dialect",
            "expected the name of a registered sidecar writer, or \"none\"",
        );
    }
    if o.sidecar.is_none() {
        out.add(
            "output.sidecar",
            "output",
            format!("missing, expected the sidecar file the {dialect} dialect writes"),
        );
    }
}

fn sky(out: &mut Out<'_>) {
    let Some(sky) = &out.r.sky else { return };
    let kind = match sky.kind {
        SkyKind::Hdr => "hdr",
        SkyKind::Analytic => "analytic",
        SkyKind::Room => "room",
        SkyKind::Mix => "mix",
    };
    if matches!(sky.kind, SkyKind::Hdr | SkyKind::Mix) && sky.path.is_none() {
        out.add(
            "sky.path",
            "sky",
            format!("kind \"{kind}\" needs `path`, the .hdr file"),
        );
    }
    if sky.path.is_some() && sky.source.is_none() {
        out.add(
            "sky.source",
            "sky.path",
            "every HDR has a `source` line naming where it came from and its licence",
        );
    }
    if matches!(sky.kind, SkyKind::Analytic | SkyKind::Room) {
        let draws = if sky.kind == SkyKind::Room {
            "draws the room, not a file"
        } else {
            "draws no file"
        };
        if sky.path.is_some() {
            out.same(
                "sky.path",
                format!("kind \"{kind}\" {draws}, expected `path` removed"),
            );
        }
        if sky.prepare.is_some() || sky.balance.is_some() {
            out.same(
                "sky.prepare",
                format!("kind \"{kind}\" {draws}, so there is nothing to prepare, expected `[sky.prepare]` removed"),
            );
        }
    }
    if matches!(sky.kind, SkyKind::Room | SkyKind::Mix) && sky.room.is_none() {
        out.add(
            "sky.room",
            "sky",
            format!("kind \"{kind}\" draws the lamp room, expected a `[sky.room]` table"),
        );
    }
    if sky.ambient.is_some() && sky.kind != SkyKind::Room {
        out.same(
            "sky.ambient",
            format!("`ambient = \"authored\"` scales the lamp room by the authored sun's hourly ambient, expected it only with kind \"room\", not \"{kind}\""),
        );
    }
    match sky.fill {
        Some(_) if sky.kind != SkyKind::Mix => out.same(
            "sky.fill",
            "`fill` scales the room's lamps over the HDR, expected it only with kind \"mix\"",
        ),
        Some(fill) if !(fill >= 0.0 && fill.is_finite()) => {
            out.same("sky.fill", "expected a number from 0 up")
        }
        _ => {}
    }
    if let Some(balance) = sky.balance
        && balance.iter().any(|v| !(v.is_finite() && *v > 0.0))
    {
        out.same("sky.prepare.balance", "expected three numbers above 0");
    }
    if !(sky.intensity.is_finite() && sky.intensity >= 0.0) {
        out.same("sky.intensity", "expected a number from 0 up");
    }
    if let Some(room) = &sky.room {
        self::room(out, room);
    }
}

fn room(out: &mut Out<'_>, room: &crate::Room) {
    let sky_room = At::new("sky.room", "sky.room");
    if !(2..=8192).contains(&room.width) {
        out.same(
            "sky.room.width",
            "expected the room texture's width in texels, from 2 to 8192",
        );
    }
    let colour = |c: &[f64; 3]| c.iter().all(|v| v.is_finite() && *v >= 0.0);
    for (key, c) in [
        ("floor", &room.floor),
        ("wall", &room.wall),
        ("ceiling", &room.ceiling),
    ] {
        if !colour(c) {
            out.at(
                &sky_room,
                key,
                "expected a linear RGB colour of numbers from 0 up",
            );
        }
    }
    let mut seen: Vec<&str> = Vec::new();
    for (i, lamp) in room.lights.iter().enumerate() {
        let label = if lamp.name.is_empty() {
            i.to_string()
        } else {
            lamp.name.clone()
        };
        let at = sky_room.child(&format!("lights[{label}]"), &format!("lights#{i}"));
        if !lamp.name.is_empty() {
            if seen.contains(&lamp.name.as_str()) {
                out.at(
                    &at,
                    "name",
                    "duplicate name, expected a name no other lamp uses",
                );
            }
            seen.push(&lamp.name);
        }
        if !colour(&lamp.color) {
            out.at(
                &at,
                "color",
                "expected a linear RGB colour of numbers from 0 up",
            );
        }
        for (key, value) in [("az", lamp.az), ("el", lamp.el), ("open", lamp.open)] {
            if !value.is_finite() {
                out.at(&at, key, "expected a finite number");
            }
        }
        for (key, value) in [
            ("power", lamp.power),
            ("soft", lamp.soft),
            ("slats", lamp.slats),
        ] {
            if !(value.is_finite() && value >= 0.0) {
                out.at(&at, key, "expected a number from 0 up");
            }
        }
        for (key, value) in [("width", lamp.width), ("height", lamp.height)] {
            if !(value > 0.0 && value < 180.0) {
                out.at(
                    &at,
                    key,
                    "expected an angle in degrees above 0 and below 180",
                );
            }
        }
    }
}

fn fireflies(out: &mut Out<'_>, at: &str, clamp_indirect: f64, filter_glossy: f64) {
    if !(0.0..=Trace::CLAMP_INDIRECT_MAX).contains(&clamp_indirect) {
        out.same(
            &format!("{at}.clamp_indirect"),
            format!(
                "clamp_indirect = {clamp_indirect} is outside 0 to {}, expected the luminance that caps each light contribution after the first bounce, or 0 for off",
                Trace::CLAMP_INDIRECT_MAX
            ),
        );
    }
    if !(0.0..=Trace::FILTER_GLOSSY_MAX).contains(&filter_glossy) {
        out.same(
            &format!("{at}.filter_glossy"),
            format!(
                "filter_glossy = {filter_glossy} is outside 0 to {}, expected the least roughness after a diffuse bounce, or 0 for off",
                Trace::FILTER_GLOSSY_MAX
            ),
        );
    }
}

fn trace(out: &mut Out<'_>) {
    let trace = out.r.trace;
    fireflies(out, "trace", trace.clamp_indirect, trace.filter_glossy);
    if trace.spp == Some(0) {
        out.same(
            "trace.spp",
            "spp = 0 is below 1, expected the most samples any pixel takes",
        );
    }
    if let Some(threshold) = trace.threshold.filter(|&t| t < 0.0) {
        out.same(
            "trace.threshold",
            format!(
                "threshold = {threshold} is below 0, expected the noise under which a pixel stops, or 0 to run every pixel to spp"
            ),
        );
    }
    if let Some(min_samples) = trace.min_samples.filter(|&n| n < 2) {
        out.same(
            "trace.min_samples",
            format!(
                "min_samples = {min_samples} is below 2, expected the samples every pixel takes before it may stop"
            ),
        );
    }
}

fn look(out: &mut Out<'_>) {
    let Some(look) = &out.r.look else { return };
    if !(0.0..=1.0).contains(&look.grade.haze) {
        out.same("look.grade.haze", "expected an amount from 0 to 1");
    }
    if !(look.exposure.is_finite() && look.exposure > 0.0) {
        out.same("look.exposure", "expected a number above 0");
    }
    if look.view == View::Standard {
        let g = &look.grade;
        let keys = [
            ("vignette", g.vignette, 0.0),
            ("saturation", g.saturation, 1.0),
            ("warmth", g.warmth, 0.0),
            ("bloom", g.bloom, 0.0),
            ("haze", g.haze, 0.0),
        ];
        let grade = At::new("look.grade", "look.grade");
        for (key, value, neutral) in keys {
            if value != neutral {
                out.at(
                    &grade,
                    key,
                    format!("the standard view has no grade, expected {neutral:?} or `view = \"graded\"`"),
                );
            }
        }
    }
}

fn bake(out: &mut Out<'_>) {
    let r = out.r;
    let Some(bake) = &r.bake else {
        if r.output.bake.is_some() {
            out.same(
                "output.bake",
                "a bake recipe needs its anchors, samples and volumes, expected a `[bake]` table",
            );
        }
        return;
    };
    if let Some(file) = &r.output.bake
        && std::path::Path::new(file).file_name() != Some(std::ffi::OsStr::new("bake.toml"))
    {
        out.same(
            "output.bake",
            "`pfx bake --scene <folder>` reads the folder's `bake.toml`, expected a path ending in `bake.toml`",
        );
    }
    if r.output.bake.is_none() {
        out.same(
            "bake",
            "`[bake]` is set but nothing writes it, expected `bake` in `[output]`",
        );
    }
    if let Err(text) = bake_product(bake) {
        match bake.product {
            Some(_) => out.same("bake.product", text),
            None => out.add("bake.product", "bake", text),
        }
    }
    if bake.anchors.is_empty()
        || bake.anchors.iter().any(|h| !(0.0..=24.0).contains(h))
        || bake.anchors.windows(2).any(|w| w[0] >= w[1])
    {
        out.same(
            "bake.anchors",
            "expected rising, distinct hours from 0 to 24",
        );
    }
    if bake.samples == 0 {
        out.same("bake.samples", "expected a whole number above 0");
    }
    if bake.volumes.is_empty() {
        out.add(
            "bake.volume",
            "bake",
            "missing, expected at least one `[[bake.volume]]`",
        );
    }
    let mut seen: Vec<&str> = Vec::new();
    for (i, v) in bake.volumes.iter().enumerate() {
        let at = At::new(
            format!("bake.volume[{}]", v.name),
            format!("bake.volume#{i}"),
        );
        let label = !v.name.is_empty()
            && v.name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !label {
            out.at(
                &at,
                "name",
                "expected a name of ASCII letters, digits, `-` and `_`",
            );
        } else if seen.contains(&v.name.as_str()) {
            out.at(
                &at,
                "name",
                "duplicate name, expected a name no other volume uses",
            );
        }
        seen.push(&v.name);
        if !(v.spacing.is_finite() && v.spacing > 0.0) {
            out.at(&at, "spacing", "expected a number above 0");
        }
        if (0..3).any(|k| {
            v.max[k]
                .partial_cmp(&v.min[k])
                .is_none_or(std::cmp::Ordering::is_lt)
        }) {
            out.at(&at, "max", "expected every axis of `max` at or above `min`");
        } else if v.spacing > 0.0
            && (0..3).any(|k| {
                let ratio = (v.max[k] - v.min[k]) / v.spacing;
                (ratio - ratio.round()).abs() > 1e-5
            })
        {
            out.at(
                &at,
                "spacing",
                "expected `max` minus `min` a whole number of spacings on every axis",
            );
        }
    }
    if let Some(plates) = &bake.plates {
        self::plates(out, plates);
    }
}

fn rising(hours: &[f64]) -> bool {
    !hours.is_empty()
        && hours
            .iter()
            .all(|h| h.is_finite() && (0.0..=24.0).contains(h))
        && hours.windows(2).all(|w| w[0] < w[1])
}

fn plate_label(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn plates(out: &mut Out<'_>, p: &Plates) {
    let r = out.r;
    if r.output.scene.is_none() {
        out.same(
            "bake.plates",
            "a plate traces the static part of the scene Manfred writes, expected `scene` in `[output]`",
        );
    }
    let [low, high] = Plates::SIDE;
    if p.size.iter().any(|side| !(low..=high).contains(side)) {
        out.same(
            "bake.plates.size",
            format!("expected a live frame's width and height, each from {low} to {high} pixels"),
        );
    }
    if !(0.0..=1.0).contains(&p.overscan) {
        out.same(
            "bake.plates.overscan",
            "expected the margin round the view, from 0 to 1 of its width and height",
        );
    }
    if !(0.25..=4.0).contains(&p.scale) {
        out.same(
            "bake.plates.scale",
            "expected plate texels per live pixel, from 0.25 to 4",
        );
    }
    if let Some(anchors) = &p.anchors
        && !rising(anchors)
    {
        out.same(
            "bake.plates.anchors",
            "expected rising, distinct hours from 0 to 24",
        );
    }
    layers(out, &p.layers);
    fireflies(
        out,
        "bake.plates",
        p.clamp_indirect.unwrap_or(0.0),
        p.filter_glossy.unwrap_or(0.0),
    );
    if !(p.threshold.is_finite() && p.threshold > 0.0) {
        out.same("bake.plates.threshold", "expected a number above 0");
    }
    if !(p.growth.is_finite() && p.growth > 1.0) {
        out.same("bake.plates.growth", "expected a number above 1");
    }
    if p.min_samples < 2 {
        out.same("bake.plates.min_samples", "expected 2 samples or more");
    }
    if p.max_samples > Plates::SAMPLES_CAP {
        out.same(
            "bake.plates.max_samples",
            format!("expected {} samples at most", Plates::SAMPLES_CAP),
        );
    } else if p.max_samples < p.min_samples {
        let key = if r.line_of("bake.plates.max_samples") == r.line_of("bake.plates") {
            "bake.plates.min_samples"
        } else {
            "bake.plates.max_samples"
        };
        out.same(
            key,
            format!(
                "`max_samples` ({}) is below `min_samples` ({}), expected at least as many",
                p.max_samples, p.min_samples
            ),
        );
    }
    if p.dir.as_deref() == Some("") {
        out.same(
            "bake.plates.dir",
            "expected the plate folder `pfx bake` writes, relative to the recipe",
        );
    }
    if p.casters != Casters::Meshes && p.dir.is_none() {
        out.same(
            "bake.plates.casters",
            "the scene's `[plates]` is written only for a plate folder, expected `dir` beside `casters`",
        );
    }
    if p.views.is_empty() && r.cameras.is_empty() {
        out.same(
            "bake.plates",
            "a plate's view starts from the scene's camera, expected a `[[camera]]`",
        );
    }
    if p.views.is_empty()
        && let Some(camera) = r
            .output
            .scene_camera
            .as_ref()
            .and_then(|name| r.cameras.iter().find(|c| &c.name == name))
            .or(r.cameras.first())
        && camera.kind == Projection::Orthographic
    {
        out.same(
            "bake.plates",
            format!(
                "a plate is perspective and the scene's camera `{}` is orthographic, expected a `[[bake.plates.view]]` through a perspective camera",
                camera.name
            ),
        );
    }
    let mut seen: Vec<&str> = Vec::new();
    for (i, v) in p.views.iter().enumerate() {
        let at = At::new(
            format!("bake.plates.view[{}]", v.name),
            format!("bake.plates.view#{i}"),
        );
        if !plate_label(&v.name) {
            out.at(
                &at,
                "name",
                "expected a name of up to 64 ASCII letters, digits, `-` and `_`",
            );
        } else if seen.contains(&v.name.as_str()) {
            out.at(
                &at,
                "name",
                "duplicate name, expected a name no other view uses",
            );
        }
        seen.push(&v.name);
        view(out, &at, v);
    }
}

fn layers(out: &mut Out<'_>, layers: &[String]) {
    let key = "bake.plates.layers";
    for (i, name) in layers.iter().enumerate() {
        if !Plates::LAYERS.contains(&name.as_str()) {
            out.same(
                key,
                format!(
                    "unknown layer `{name}`, expected some of {}",
                    Plates::LAYERS.join(", ")
                ),
            );
            return;
        }
        if layers[..i].contains(name) {
            out.same(key, format!("layer `{name}` is named twice"));
            return;
        }
    }
    let has = |name: &str| layers.iter().any(|layer| layer == name);
    if !has("color") || !has("depth") {
        out.same(key, "expected `color` and `depth` among the layers");
    } else if has("transfer") && !(has("sun") && has("normal")) {
        out.same(
            key,
            "`transfer` relights the sun's share between anchors, expected `sun` and `normal` with it",
        );
    }
}

fn view(out: &mut Out<'_>, at: &At, v: &PlateView) {
    let r = out.r;
    if let Some(name) = &v.camera {
        match r.cameras.iter().find(|c| &c.name == name) {
            None => out.missing(at, "camera", "camera", name, "the name of a `[[camera]]`"),
            Some(camera) if camera.kind == Projection::Orthographic => out.at(
                at,
                "camera",
                format!("a plate is perspective and `{name}` is orthographic, expected a perspective camera"),
            ),
            Some(_) => {}
        }
    }
    if v.fov.is_some() && (v.focal.is_some() || v.sensor.is_some()) {
        out.at(
            at,
            "fov",
            "`fov` and `focal` or `sensor` are both set, expected one",
        );
    }
    if let Some(fov) = v.fov
        && !(fov > 0.0 && fov < 179.0)
    {
        out.at(
            at,
            "fov",
            "expected a vertical field of view above 0 and below 179 degrees",
        );
    }
    if v.sensor.is_some() && v.focal.is_none() {
        out.at(at, "sensor", "`sensor` needs `focal`, expected both");
    }
    for (key, value) in [("focal", v.focal), ("sensor", v.sensor), ("near", v.near)] {
        if let Some(value) = value
            && !(value.is_finite() && value > 0.0)
        {
            out.at(at, key, "expected a number above 0");
        }
    }
    if let Some(far) = v.far
        && !(far.is_finite() && far > v.near.unwrap_or(0.0))
    {
        out.at(at, "far", "expected a number above 0 and above `near`");
    }
    let finite = |values: &[f64]| values.iter().all(|value| value.is_finite());
    for (key, value) in [("at", v.at), ("look_at", v.look_at), ("up", v.up)] {
        if let Some(value) = value
            && !finite(&value)
        {
            out.at(at, key, "expected finite numbers");
        }
    }
    if let Some(shift) = v.shift
        && !finite(&shift)
    {
        out.at(at, "shift", "expected finite numbers");
    }
}

pub fn bake_product(bake: &Bake) -> Result<&str, String> {
    match bake.product.as_deref() {
        Some(product) if slug(product) => Ok(product),
        Some(other) => Err(format!(
            "\"{other}\" is not an app's name, expected lowercase letters, digits and `-`, starting with a letter or digit"
        )),
        None => Err("missing, expected `product`, the name of the app the bake is for".into()),
    }
}

pub fn slug(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn sidecar(out: &mut Out<'_>) {
    let r = out.r;
    if !r.sidecar.is_empty() && !r.output.writes_sidecar() {
        out.same(
            "sidecar",
            "`[sidecar]` has no file to go in, expected `output.sidecar_dialect` naming a registered sidecar writer",
        );
    }
}

fn files(r: &Recipe) -> Vec<String> {
    let o = &r.output;
    let mut all = vec![o.glb.clone().unwrap_or_else(|| format!("{}.glb", r.name))];
    if o.writes_sidecar() {
        all.extend(o.sidecar.clone());
    }
    all.extend(o.bake.clone());
    all.extend(o.scene.clone());
    all
}

fn variants(out: &mut Out<'_>) {
    let r = out.r;
    let mut taken = files(r);
    for (name, v) in &r.variants {
        let ip = format!("variant.{name}");
        for glob in &v.drop {
            if !r.objects.iter().any(|o| glob_matches(glob, &o.name)) {
                out.same(
                    &format!("{ip}.drop"),
                    format!("`{glob}` matches no object, expected a name or a glob that matches at least one"),
                );
            }
        }
        match apply_variant(r, name) {
            Err(e) => {
                let line = r.line_of(&ip);
                out.list.push(Problem {
                    path: e.path,
                    line: e.line.or(line),
                    text: e.text,
                });
            }
            Ok(applied) => {
                let written = files(&applied);
                for file in &written {
                    if taken.contains(file) {
                        out.add(
                            format!("{ip}.output"),
                            &ip,
                            format!("writes `{file}`, which the recipe or another variant writes too, expected a file of its own under `[variant.{name}.output]`"),
                        );
                    }
                }
                taken.extend(written);
            }
        }
    }
}
