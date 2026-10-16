use super::{Cx, STR, STRS, Tab, V3, follow_keys, label, pick, pick_req, rig_keys, table_list};
use crate::{Axis, Constrained, Constraint, ConstraintKind, CopyOf, Error, LookAt, Owner};

#[derive(Clone, Copy)]
enum Type {
    FollowPath,
    LookAt,
    Copy,
    Rig,
}

const TYPES: [(&str, Type); 4] = [
    ("follow_path", Type::FollowPath),
    ("look_at", Type::LookAt),
    ("copy", Type::Copy),
    ("rig", Type::Rig),
];

const AXES: [(&str, Option<Axis>); 6] = [
    ("x", Some(Axis::X)),
    ("-x", Some(Axis::NegX)),
    ("y", Some(Axis::Y)),
    ("-y", Some(Axis::NegY)),
    ("z", Some(Axis::Z)),
    ("-z", Some(Axis::NegZ)),
];

const CHANNELS: [&str; 3] = ["translation", "rotation", "scale"];

pub(super) fn list(
    cx: &mut Cx,
    tab: &mut Tab<'_>,
    owner: &Owner,
) -> Result<Vec<Constraint>, Error> {
    let Some(item) = tab.peek(cx, "constraint") else {
        return Ok(Vec::new());
    };
    let base = tab.kpath("constraint");
    let ibase = tab.kipath("constraint");
    let mut out = Vec::new();
    for (i, (t, line)) in table_list(cx, &base, item)?.into_iter().enumerate() {
        let path = format!("{base}[{}]", label(t, "name", i));
        let ipath = format!("{ibase}#{i}");
        let mut c = Tab::new(cx, t, path.clone(), ipath.clone(), line);
        let name = c.opt(cx, "name", STR)?;
        let kind = match pick_req(&mut c, cx, "type", &TYPES)? {
            Type::FollowPath => {
                ConstraintKind::FollowPath(follow_keys(cx, &mut c, owner.name().to_string())?)
            }
            Type::LookAt => ConstraintKind::LookAt(LookAt {
                target: c.req(cx, "target", STR)?,
                up: c.or(cx, "up", [0.0, 1.0, 0.0], V3)?,
                offset: c.or(cx, "offset", [0.0; 3], V3)?,
                axis: pick(&mut c, cx, "axis", None, &AXES)?,
            }),
            Type::Copy => {
                let target = c.req(cx, "target", STR)?;
                let offset = c.or(cx, "offset", [0.0; 3], V3)?;
                let rotate = c.or(cx, "rotate", [0.0; 3], V3)?;
                let key = c.kpath("channels");
                let channels = c.or(
                    cx,
                    "channels",
                    vec!["translation".to_string(), "rotation".to_string()],
                    STRS,
                )?;
                if channels.is_empty() {
                    return Err(Error::new(
                        key,
                        Some(line),
                        "expected one channel at least, of \"translation\", \"rotation\" and \"scale\"",
                    ));
                }
                if let Some(odd) = channels.iter().find(|c| !CHANNELS.contains(&c.as_str())) {
                    return Err(Error::new(
                        key,
                        Some(line),
                        format!(
                            "expected \"translation\", \"rotation\" or \"scale\", found \"{odd}\""
                        ),
                    ));
                }
                let has = |name: &str| channels.iter().any(|c| c == name);
                ConstraintKind::Copy(CopyOf {
                    target,
                    offset,
                    rotate,
                    translation: has("translation"),
                    rotation: has("rotation"),
                    scale: has("scale"),
                })
            }
            Type::Rig => ConstraintKind::Rig(Box::new(rig_keys(cx, &mut c)?)),
        };
        c.finish(cx)?;
        out.push(Constraint {
            name,
            path,
            ipath,
            kind,
        });
    }
    Ok(out)
}

pub(super) fn keep(cx: &mut Cx, owner: Owner, list: Vec<Constraint>) {
    if !list.is_empty() {
        cx.constraints.push(Constrained { owner, list });
    }
}
