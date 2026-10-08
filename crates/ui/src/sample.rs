pub const RECIPE_NAME: &str = "toybox";
pub const RECIPE_PATH: &str = "scenes/toybox.toml";
pub const LAST_PATCH: &str = "objects.mv_knob.location.z = -0.006";

pub const OBJECTS: &[(&str, &str)] = &[
    ("body", "lacquer_red"),
    ("lip_shelf", "lacquer_red"),
    ("lip_dial", "lacquer_red"),
    ("lip_bin", "lacquer_red"),
    ("lip_belt", "lacquer_red"),
    ("mv_lid", "card_cream"),
    ("pane_mask", "lacquer_plain"),
    ("pane", "glass"),
    ("mv_belt", "tape"),
    ("belt_line", "brass"),
    ("belt_slot_top", "rubber"),
    ("belt_slot_bottom", "rubber"),
    ("mv_roller", "brass"),
    ("mv_lid__button0", "card_pad"),
    ("mv_lid__button1", "card_pad"),
    ("mv_lid__button2", "card_pad"),
    ("mv_lid__button3", "card_pad"),
    ("mv_lid__button4", "card_pad"),
    ("gasket", "rubber"),
    ("shelf_back", "felt"),
    ("mv_key_a", "key_red"),
    ("mv_key_b", "key_yellow"),
    ("mv_key_c", "key_white"),
    ("slot", "glass_dark"),
    ("slot_field", "field"),
    ("slot_bezel", "brass"),
    ("mv_knob", "chrome"),
    ("mv_knob__face", "steel_radial"),
    ("mv_knob__cap", "steel_brushed"),
    ("mv_knob__dot", "lacquer_wine"),
    ("slider", "lacquer_bone"),
    ("slider_knob", "brass"),
    ("mv_fan__card0", "card"),
    ("mv_fan__card1", "card"),
    ("mv_fan__card2", "card"),
    ("mv_fan__card3", "card"),
    ("mv_fan__card4", "card"),
    ("backlight", "backlight"),
    ("led0", "led"),
    ("led1", "led"),
    ("bin_floor", "felt"),
    ("bin_lining", "leather"),
    ("drum_top", "brass"),
    ("drum_top_rim", "steel_brushed"),
    ("drum_bottom", "brass"),
    ("drum_bottom_rim", "steel_brushed"),
    ("mv_lid__print", "card_white"),
    ("mv_lid__photo", "photo"),
    ("mv_lid__label", "tape"),
    ("mv_dial", "rubber_dark"),
    ("mv_dial__label", "lacquer_red"),
    ("mv_marker", "lacquer_cream"),
    ("rail_front", "steel_dark"),
    ("post_left", "chrome"),
    ("post_right", "chrome"),
    ("track_bed", "felt"),
    ("track_light", "backlight"),
    ("ruler", "ivory"),
    ("strip", "film"),
    ("track_marks", "card_white"),
    ("bumper", "lacquer_red"),
    ("mv_slider", "brass"),
    ("mv_slider__needle", "lacquer_wine"),
    ("foot_left", "rubber"),
    ("foot_right", "rubber"),
    ("vase_body", "glaze_print"),
    ("vase_rim", "glaze_rim"),
    ("water", "water"),
    ("vase_handle", "glaze"),
];

pub const CAMERAS: &[(&str, &str)] = &[
    ("front", "50 mm  shift 0.00 0.04"),
    ("hero", "35 mm  f/2.8"),
    ("plate", "ortho  1.62 m"),
];

pub const LIGHTS: &[(&str, &str)] = &[
    ("sun", "5600 K  3.2 lx"),
    ("sky", "clear  turbidity 2.4"),
    ("window", "area  0.8 × 0.6 m"),
];

pub const SMOOTHING: &[&str] = &["flat", "smooth", "by angle"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub object: Option<usize>,
    pub name: String,
    pub children: Vec<usize>,
}

pub fn tree() -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for (index, (name, _)) in OBJECTS.iter().enumerate() {
        match name.split_once("__") {
            Some((parent, _)) => match rows.iter_mut().find(|row| row.name == parent) {
                Some(row) => row.children.push(index),
                None => rows.push(Row {
                    object: None,
                    name: parent.to_string(),
                    children: vec![index],
                }),
            },
            None => rows.push(Row {
                object: Some(index),
                name: name.to_string(),
                children: Vec::new(),
            }),
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn children_hang_under_their_parent() {
        let rows = tree();
        let knob = rows.iter().find(|row| row.name == "mv_knob").unwrap();
        let names: Vec<&str> = knob.children.iter().map(|i| OBJECTS[*i].0).collect();
        assert_eq!(names, ["mv_knob__face", "mv_knob__cap", "mv_knob__dot"]);
        let fan = rows.iter().find(|row| row.name == "mv_fan").unwrap();
        assert_eq!(fan.object, None);
        assert_eq!(fan.children.len(), 5);
        let lid = rows.iter().find(|row| row.name == "mv_lid").unwrap();
        assert_eq!(lid.children.len(), 8);
    }

    #[test]
    fn every_object_appears_once() {
        let rows = tree();
        let count: usize = rows
            .iter()
            .map(|row| row.children.len() + usize::from(row.object.is_some()))
            .sum();
        assert_eq!(count, OBJECTS.len());
    }
}
