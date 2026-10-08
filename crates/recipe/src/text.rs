use crate::TextSpace;

pub const ANCHORS: [(&str, [f64; 2]); 9] = [
    ("top_left", [0.0, 0.0]),
    ("top", [0.5, 0.0]),
    ("top_right", [1.0, 0.0]),
    ("left", [0.0, 0.5]),
    ("centre", [0.5, 0.5]),
    ("right", [1.0, 0.5]),
    ("bottom_left", [0.0, 1.0]),
    ("bottom", [0.5, 1.0]),
    ("bottom_right", [1.0, 1.0]),
];

pub const MONO: &str = "mono";

pub fn anchor_point(name: &str) -> Option<[f64; 2]> {
    ANCHORS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, point)| *point)
}

pub fn default_anchor(space: TextSpace) -> &'static str {
    match space {
        TextSpace::Overlay => "top_left",
        TextSpace::Surface | TextSpace::Lit => "centre",
    }
}

pub fn anchor_names() -> String {
    ANCHORS
        .iter()
        .map(|(name, _)| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ")
}
