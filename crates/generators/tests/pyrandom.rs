use manfred_generators::pyrandom::Random;
use serde_json::Value;

fn recorded() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/pyrandom.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn float(v: &Value) -> f64 {
    f64::from_bits(v.as_u64().unwrap())
}

#[test]
fn the_port_matches_cpythons_random_call_for_call() {
    let data = recorded();
    let seeds = data["seeds"].as_array().unwrap();
    assert_eq!(seeds.len(), 5);
    assert_eq!(seeds[0]["seed"], 14);
    let mut checked = 0;
    for record in seeds {
        let seed = record["seed"].as_i64().unwrap();
        let mut r = Random::new(seed);
        for call in record["calls"].as_array().unwrap() {
            let (method, args, want) = (call[0].as_str().unwrap(), &call[1], &call[2]);
            match method {
                "random" => assert_eq!(r.random().to_bits(), want.as_u64().unwrap(), "seed {seed}"),
                "uniform" => {
                    let got = r.uniform(float(&args[0]), float(&args[1]));
                    assert_eq!(got.to_bits(), want.as_u64().unwrap(), "seed {seed}");
                }
                "randrange" => {
                    let got = r.randrange(args[0].as_i64().unwrap(), args[1].as_i64().unwrap());
                    assert_eq!(got.unwrap(), want.as_i64().unwrap(), "seed {seed}");
                }
                other => panic!("unknown method {other}"),
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 5 * 40 * 7);
}

#[test]
fn an_empty_range_is_refused() {
    let mut r = Random::new(14);
    assert_eq!(
        r.randrange(3, 3).unwrap_err(),
        "empty range in randrange(3, 3)"
    );
}

#[test]
fn the_same_seed_gives_the_same_stream() {
    let (mut a, mut b) = (Random::new(14), Random::new(14));
    for _ in 0..2000 {
        assert_eq!(a.next_u32(), b.next_u32());
    }
    assert_ne!(Random::new(14).next_u32(), Random::new(15).next_u32());
}
