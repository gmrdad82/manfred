use crate::Part;
use manfred_glb::{MoverFacts, mover_json};
use manfred_recipe::{Recipe, Shadow};
use serde_json::{Map, Value, json};

pub(crate) fn of(recipe: &Recipe, parts: &[Part], movers: &[MoverFacts]) -> Vec<Option<Value>> {
    let mut rooted = vec![false; recipe.movers.len()];
    parts
        .iter()
        .map(|part| {
            let mut manfred = Map::new();
            let name = &recipe.objects[part.object].name;
            let owner = recipe.movers.iter().position(|m| m.owns(name));
            if let Some(at) = owner {
                let facts = &movers[at];
                manfred.insert("group".into(), json!(facts.group));
                if !part.hidden && !rooted[at] {
                    rooted[at] = true;
                    manfred.extend(mover_json(facts));
                }
            }
            match part.shadow {
                Shadow::Cast => {}
                Shadow::Only => {
                    manfred.insert("shadow".into(), json!("only"));
                }
                Shadow::None => {
                    manfred.insert("shadow".into(), json!("none"));
                }
            }
            (!manfred.is_empty()).then(|| json!({ "manfred": manfred }))
        })
        .collect()
}
