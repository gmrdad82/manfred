use crate::{NodeDiff, NodeSummary, Report, Summary};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

const BOUNDS_TOLERANCE: f64 = 1e-4;
const AREA_TOLERANCE: f64 = 0.01;
const UV1_AREA_TOLERANCE: f64 = 0.001;

fn keyed(summary: &Summary) -> Vec<(String, &NodeSummary)> {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    summary
        .nodes
        .iter()
        .map(|node| {
            let count = seen.entry(node.name.as_str()).or_insert(0);
            *count += 1;
            let key = if *count == 1 {
                node.name.clone()
            } else {
                format!("{}#{}", node.name, count)
            };
            (key, node)
        })
        .collect()
}

fn flatten(path: &str, value: &Value, out: &mut BTreeMap<String, Value>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                let next = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                flatten(&next, inner, out);
            }
        }
        other => {
            out.insert(path.to_string(), other.clone());
        }
    }
}

fn leaves(extras: &Option<Value>) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    if let Some(value) = extras {
        flatten("", value, &mut out);
    }
    out
}

fn extras_marks(a: &Option<Value>, b: &Option<Value>) -> Vec<String> {
    let left = leaves(a);
    let right = leaves(b);
    let mut out = Vec::new();
    for (key, value) in &left {
        match right.get(key) {
            Some(other) if other == value => {}
            Some(other) => out.push(format!("extras {key} {value} -> {other}")),
            None => out.push(format!("extras {key} only in a")),
        }
    }
    for key in right.keys() {
        if !left.contains_key(key) {
            out.push(format!("extras {key} only in b"));
        }
    }
    out
}

fn marks(a: &NodeSummary, b: &NodeSummary, extras: bool) -> Vec<String> {
    let mut out = Vec::new();
    if a.triangles != b.triangles {
        out.push(format!("triangles {} -> {}", a.triangles, b.triangles));
    }
    let moved = (0..2).any(|side| {
        (0..3).any(|axis| (a.bounds[side][axis] - b.bounds[side][axis]).abs() > BOUNDS_TOLERANCE)
    });
    if moved {
        out.push(format!(
            "bounds {} -> {}",
            bounds_text(&a.bounds),
            bounds_text(&b.bounds)
        ));
    }
    let largest = a.area.max(b.area);
    if (a.area - b.area).abs() > AREA_TOLERANCE * largest && largest > 1e-12 {
        out.push(format!("area {:.6} -> {:.6}", a.area, b.area));
    }
    if a.slots != b.slots {
        out.push(format!(
            "slots [{}] -> [{}]",
            a.slots.join(", "),
            b.slots.join(", ")
        ));
    }
    if a.uvs != b.uvs {
        out.push(format!("uvs {} -> {}", a.uvs, b.uvs));
    }
    if a.uvs1 != b.uvs1 {
        out.push(format!("uvs1 {} -> {}", a.uvs1, b.uvs1));
    }
    if (a.uv1_area - b.uv1_area).abs() > UV1_AREA_TOLERANCE {
        out.push(format!(
            "uv1 area use {:.2}% -> {:.2}%",
            a.uv1_area * 100.0,
            b.uv1_area * 100.0
        ));
    }
    if extras {
        out.extend(extras_marks(&a.extras, &b.extras));
    }
    out
}

fn bounds_text(bounds: &[[f64; 3]; 2]) -> String {
    let [lo, hi] = bounds;
    format!(
        "[{:.4} {:.4} {:.4}]..[{:.4} {:.4} {:.4}]",
        lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]
    )
}

fn carries_extras(summary: &Summary) -> bool {
    summary.nodes.iter().any(|node| node.extras.is_some())
}

pub fn diff(a: &Summary, b: &Summary) -> Report {
    let left = keyed(a);
    let right = keyed(b);
    let in_left: BTreeMap<&str, &NodeSummary> =
        left.iter().map(|(k, n)| (k.as_str(), *n)).collect();
    let in_right: BTreeMap<&str, &NodeSummary> =
        right.iter().map(|(k, n)| (k.as_str(), *n)).collect();
    let extras = carries_extras(a) && carries_extras(b);
    let mut report = Report::default();
    for (key, node) in &left {
        match in_right.get(key.as_str()) {
            Some(other) => {
                let found = marks(node, other, extras);
                if !found.is_empty() {
                    report.nodes.push(NodeDiff {
                        name: key.clone(),
                        marks: found,
                    });
                }
            }
            None => report.only_a.push(key.clone()),
        }
    }
    for (key, _) in &right {
        if !in_left.contains_key(key.as_str()) {
            report.only_b.push(key.clone());
        }
    }
    let common = |own: &[(String, &NodeSummary)], other: &BTreeMap<&str, &NodeSummary>| {
        own.iter()
            .filter(|(key, _)| other.contains_key(key.as_str()))
            .map(|(key, _)| key.clone())
            .collect::<Vec<String>>()
    };
    report.order_differs = common(&left, &in_right) != common(&right, &in_left);
    if a.materials != b.materials {
        report.materials_differ = true;
        report.materials_a = a.materials.clone();
        report.materials_b = b.materials.clone();
    }
    report
}

impl Report {
    pub fn differs(&self) -> bool {
        !self.nodes.is_empty()
            || !self.only_a.is_empty()
            || !self.only_b.is_empty()
            || self.order_differs
            || self.materials_differ
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.differs() {
            return writeln!(f, "no differences beyond tolerance");
        }
        for node in &self.nodes {
            writeln!(f, "~ {}", node.name)?;
            for mark in &node.marks {
                writeln!(f, "    {mark}")?;
            }
        }
        for name in &self.only_a {
            writeln!(f, "- {name} (only in a)")?;
        }
        for name in &self.only_b {
            writeln!(f, "+ {name} (only in b)")?;
        }
        if self.order_differs {
            writeln!(f, "! node order differs")?;
        }
        if self.materials_differ {
            writeln!(f, "! materials differ")?;
            writeln!(f, "    a: [{}]", self.materials_a.join(", "))?;
            writeln!(f, "    b: [{}]", self.materials_b.join(", "))?;
        }
        Ok(())
    }
}
