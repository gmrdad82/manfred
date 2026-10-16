use toml_edit::Value;

use super::{Cx, Tab, bad_item, bad_value, num_value};
use crate::Error;
use crate::keyframes::{Channel, Context, Interp, Keyed, entry_of, properties, property, refusal};

fn values(cx: &Cx, path: &str, value: &Value, dims: usize) -> Result<Vec<f64>, Error> {
    if dims == 1 {
        return Ok(vec![num_value(cx, path, value)?]);
    }
    let expected = format!("an array of {dims} numbers");
    match value {
        Value::Array(list) if list.len() == dims => list
            .iter()
            .enumerate()
            .map(|(i, v)| num_value(cx, &format!("{path}[{i}]"), v))
            .collect(),
        _ => Err(bad_value(cx, path, value, &expected)),
    }
}

pub(super) fn read(
    cx: &mut Cx,
    tab: &mut Tab<'_>,
    section: &str,
    name: Option<&str>,
    context: Context,
) -> Result<(), Error> {
    let keys = tab.peek(cx, "keys");
    let interp = tab.peek(cx, "interp");
    let Some(item) = keys else {
        if let Some(item) = interp {
            return Err(Error::new(
                tab.kpath("interp"),
                Some(cx.item_line(item)),
                "`interp` says how keys blend, expected a `keys` table beside it",
            ));
        }
        return Ok(());
    };
    let path = tab.kpath("keys");
    let Some(table) = item.as_table_like() else {
        return Err(bad_item(
            cx,
            &path,
            item,
            "a table of <property> = [[t, value], ...]",
        ));
    };
    let mut keyed = Keyed::new();
    let names: Vec<&str> = properties(section)
        .iter()
        .filter(|p| refusal(section, p.name, context).is_none())
        .map(|p| p.name)
        .collect();
    for (key, entry) in table.iter() {
        let at = format!("{path}.{key}");
        let line = cx.item_line(entry);
        cx.lines.insert(format!("{}.keys.{key}", tab.ipath), line);
        let Some(spec) = property(section, key) else {
            return Err(Error::new(
                at,
                Some(line),
                format!(
                    "`{key}` is not keyable on {section}, expected one of: {}",
                    names.join(", ")
                ),
            ));
        };
        if let Some(text) = refusal(section, key, context) {
            return Err(Error::new(at, Some(line), text));
        }
        let dims = spec.dims();
        let shape = if dims == 1 {
            "[t, value]".to_string()
        } else {
            format!("[t, [{dims} numbers]]")
        };
        let Some(Value::Array(list)) = entry.as_value() else {
            return Err(bad_item(
                cx,
                &at,
                entry,
                &format!("an array of {shape} keys"),
            ));
        };
        if list.is_empty() {
            return Err(Error::new(
                at,
                Some(line),
                format!("expected at least one {shape} key"),
            ));
        }
        let mut out: Vec<(f64, Vec<f64>)> = Vec::with_capacity(list.len());
        for (i, pair) in list.iter().enumerate() {
            let here = format!("{at}[{i}]");
            let two = pair.as_array().filter(|a| a.len() == 2);
            let (Some(t), Some(v)) = (
                two.and_then(|a| a.iter().next()),
                two.and_then(|a| a.get(1)),
            ) else {
                return Err(bad_value(cx, &here, pair, &format!("a {shape} key")));
            };
            let t = num_value(cx, &format!("{here}[0]"), t)?;
            let value = values(cx, &format!("{here}[1]"), v, dims)?;
            let line = Some(cx.value_line(pair));
            if t < 0.0 {
                return Err(Error::new(
                    here,
                    line,
                    format!("the time is seconds of at least 0, found {t}"),
                ));
            }
            if let Some((before, _)) = out.last()
                && t <= *before
            {
                return Err(Error::new(
                    here,
                    line,
                    format!("the times rise strictly, found {t} after {before}"),
                ));
            }
            if let Some(bad) = value.iter().find(|v| !spec.fits(**v)) {
                return Err(Error::new(
                    here,
                    line,
                    format!("{key} takes {}, found {bad}", spec.rule_text()),
                ));
            }
            if key == "cone" && value[0] > value[1] {
                return Err(Error::new(
                    here,
                    line,
                    format!(
                        "expected [inner, outer] degrees with inner <= outer, found [{}, {}]",
                        value[0], value[1]
                    ),
                ));
            }
            out.push((t, value));
        }
        keyed.insert(
            key.to_string(),
            Channel {
                keys: out,
                interp: Interp::Linear,
            },
        );
    }
    if let Some(item) = interp {
        let path = tab.kpath("interp");
        let Some(table) = item.as_table_like() else {
            return Err(bad_item(
                cx,
                &path,
                item,
                "a table of <property> = \"linear\", \"smooth\" or \"step\"",
            ));
        };
        for (key, entry) in table.iter() {
            let at = format!("{path}.{key}");
            let line = Some(cx.item_line(entry));
            let Some(channel) = keyed.get_mut(key) else {
                return Err(Error::new(
                    at,
                    line,
                    format!("`{key}` has no keys, expected a property of `keys`"),
                ));
            };
            channel.interp = match entry.as_str().and_then(Interp::of) {
                Some(interp) => interp,
                None => {
                    return Err(bad_item(
                        cx,
                        &at,
                        entry,
                        "\"linear\", \"smooth\" or \"step\"",
                    ));
                }
            };
        }
    }
    if !keyed.is_empty() {
        cx.keyed.insert(entry_of(section, name), keyed);
    }
    Ok(())
}
