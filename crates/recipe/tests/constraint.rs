use manfred_recipe::{
    ConstraintKind, CopyOf, Owner, Problem, Recipe, apply_set, apply_variant, parse, validate,
};

const HEAD: &str = r#"
manfred = 1
name = "chase"

[timeline]
fps = 30
frames = 30

[[route]]
name = "main"
points = [[0.0, 0.0, 0.0], [0.0, 0.0, 40.0]]

[[object]]
name = "hero"
kind = "box"
size = [1.8, 1.2, 4.4]

[[object.constraint]]
type = "follow_path"
route = "main"
lane = 1.5
speed = [[0.0, 10.0]]
"#;

fn recipe(tail: &str) -> Recipe {
    let text = format!("{HEAD}{tail}");
    parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"))
}

fn problems(r: &Recipe) -> Vec<String> {
    validate(r).iter().map(Problem::to_string).collect()
}

const RIDER: &str = r#"
[[object]]
name = "drone"
kind = "box"
size = [0.4, 0.1, 0.4]

[[object.constraint]]
name = "ride"
type = "copy"
target = "hero"
offset = [0.0, 2.5, -1.0]

[[object.constraint]]
type = "look_at"
target = "hero.body"

[variant.high]
set = { "object.drone.constraint.ride.offset" = [0.0, 6.0, 0.0] }
"#;

#[test]
fn constraints_read_in_order_and_set_and_variants_reach_them_by_position_or_name() {
    let r = recipe(RIDER);
    assert!(validate(&r).is_empty(), "{:?}", problems(&r));
    let all = r.constrained();
    assert_eq!(all.len(), 2);
    assert_eq!(all[1].owner, Owner::Object("drone".into()));
    assert_eq!(all[1].list[0].path, "object[drone].constraint[ride]");
    assert_eq!(all[1].list[1].path, "object[drone].constraint[1]");
    assert_eq!(
        all[1].list[0].kind,
        ConstraintKind::Copy(CopyOf {
            target: "hero".into(),
            offset: [0.0, 2.5, -1.0],
            rotate: [0.0; 3],
            translation: true,
            rotation: true,
            scale: false,
        })
    );
    assert_eq!(r.constraint_order(&all).unwrap(), [0, 1]);
    let set = apply_set(
        &r,
        &[
            "object.hero.constraint.0.lane=-2.0".into(),
            "object.drone.constraint.ride.offset=[0, 3, 0]".into(),
            "object.drone.constraint.1.target=hero".into(),
        ],
    )
    .unwrap();
    let all = set.constrained();
    let ConstraintKind::FollowPath(f) = &all[0].list[0].kind else {
        panic!("a follow_path stays one");
    };
    assert_eq!(f.lane, -2.0);
    let ConstraintKind::Copy(copy) = &all[1].list[0].kind else {
        panic!("a copy stays one");
    };
    assert_eq!(copy.offset, [0.0, 3.0, 0.0]);
    let ConstraintKind::LookAt(look) = &all[1].list[1].kind else {
        panic!("a look_at stays one");
    };
    assert_eq!(look.target, "hero");
    let high = apply_variant(&r, "high").unwrap();
    let ConstraintKind::Copy(copy) = &high.constrained()[1].list[0].kind else {
        panic!("a copy stays one");
    };
    assert_eq!(copy.offset, [0.0, 6.0, 0.0]);
}

#[test]
fn a_cycle_an_unknown_target_and_a_missing_timeline_are_refused_naming_the_entry() {
    let cycle = recipe(
        "\n[[object]]\nname = \"a\"\nkind = \"box\"\nsize = [1, 1, 1]\n\n[[object.constraint]]\ntype = \"look_at\"\ntarget = \"b\"\n\n[[object]]\nname = \"b\"\nkind = \"box\"\nsize = [1, 1, 1]\n\n[[object.constraint]]\ntype = \"copy\"\ntarget = \"a\"\n",
    );
    let found = problems(&cycle);
    assert!(
        found
            .iter()
            .any(|p| p.contains("object[b].constraint[0].target")
                && p.contains("cycle")
                && p.contains("`a` looks at `b`")
                && p.contains("`b` copies `a`")),
        "{found:?}"
    );
    let unknown = recipe(
        "\n[[camera]]\nname = \"eye\"\nfov_y_deg = 40\n\n[[camera.constraint]]\ntype = \"look_at\"\ntarget = \"ghost\"\n",
    );
    let found = problems(&unknown);
    assert!(
        found
            .iter()
            .any(|p| p.contains("camera[eye].constraint[0].target") && p.contains("`ghost`")),
        "{found:?}"
    );
    let still = parse(
        "manfred = 1\nname = \"s\"\n\n[[object]]\nname = \"a\"\nkind = \"box\"\nsize = [1, 1, 1]\n\n[[light]]\nname = \"lamp\"\nposition = [0, 3, 0]\nintensity = 5\n\n[[light.constraint]]\ntype = \"look_at\"\ntarget = \"a\"\n",
    )
    .unwrap();
    let found = problems(&still);
    assert!(
        found
            .iter()
            .any(|p| p.contains("light[lamp].constraint[0].type") && p.contains("[timeline]")),
        "{found:?}"
    );
}
