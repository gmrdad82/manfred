use manfred_recipe::{Goes, Recipe, Table, properties};

use crate::Error;
use crate::bake::{Section, list, number};

pub(super) type Sent = Vec<(&'static str, &'static str, Vec<(f64, Vec<f64>)>)>;

pub(super) fn tracks(recipe: &Recipe, entry: &str, table: Table) -> Sent {
    let Some(keyed) = recipe.keyed.get(entry) else {
        return Vec::new();
    };
    let section = entry.split('.').next().unwrap_or_default();
    let step = recipe.timeline.map_or(1.0 / 60.0, |t| t.step());
    properties(section)
        .iter()
        .filter_map(|p| match p.goes {
            Goes::Scene(to, name) if to == table => keyed
                .get(p.name)
                .map(|channel| (name, p.name, channel.linear(step))),
            _ => None,
        })
        .collect()
}

pub(super) fn put(s: &mut Section<'_>, sent: &Sent) -> Result<(), Error> {
    if sent.is_empty() {
        return Ok(());
    }
    let mut parts = Vec::with_capacity(sent.len());
    for (name, property, keys) in sent {
        let at = format!("{}.keys.{property}", s.at);
        let pairs = keys
            .iter()
            .map(|(t, value)| {
                let value = match value.as_slice() {
                    [one] => number(*one, &at)?,
                    many => list(many, &at)?,
                };
                Ok(format!("[{}, {value}]", number(*t, &at)?))
            })
            .collect::<Result<Vec<String>, Error>>()?;
        parts.push(format!("{name} = [{}]", pairs.join(", ")));
    }
    s.put("keys", &format!("{{ {} }}", parts.join(", ")));
    Ok(())
}
