use std::path::PathBuf;
use std::process::ExitCode;

use manfred_build::Registry;
use manfred_scene::Error;

use super::{pair, refuse, seconds, size};

#[derive(clap::Args, Debug, PartialEq)]
pub(super) struct Args {
    #[arg(value_name = "recipe", help = "The recipe to draw")]
    recipe: PathBuf,
    #[arg(
        long,
        value_name = "name",
        help = "The recipe's camera to look through"
    )]
    camera: String,
    #[arg(long, value_name = "WxH", value_parser = size, help = "The still's size in pixels")]
    size: [u32; 2],
    #[arg(long, value_name = "png", help = "The PNG to write")]
    out: PathBuf,
    #[arg(
        long,
        value_name = "name",
        help = "Draw this variant instead of the recipe"
    )]
    variant: Option<String>,
    #[arg(
        long,
        value_name = "key=value",
        value_parser = pair,
        help = "Replace a value the recipe has; repeatable"
    )]
    set: Vec<String>,
    #[arg(
        long,
        value_name = "S",
        value_parser = seconds,
        help = "Pose the recipe's [timeline] at this many seconds"
    )]
    time: Option<f64>,
    #[command(flatten)]
    pick: super::Picks,
}

fn still(args: &Args, registry: &Registry) -> Result<Vec<u8>, Error> {
    let (recipe, built) = manfred_scene::load_at(
        &args.recipe,
        args.variant.as_deref(),
        &args.set,
        &args.pick.pick(),
        registry,
        args.time,
    )?;
    let gpu = manfred_scene::headless()?;
    manfred_scene::shoot(&gpu, &recipe, &built.parts, &args.camera, args.size)
}

pub fn run(args: &Args, registry: &Registry) -> ExitCode {
    let shot = still(args, registry)
        .and_then(|rgba| manfred_scene::write_png(&args.out, args.size, &rgba));
    match shot {
        Ok(()) => {
            println!("{}", args.out.display());
            ExitCode::SUCCESS
        }
        Err(e) => refuse("shot", &e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(words: &[&str]) -> Result<Args, String> {
        match super::super::parsed(&[&["shot"], words].concat())? {
            super::super::Verb::Shot(args) => Ok(args),
            _ => Err("not shot".into()),
        }
    }

    #[test]
    fn every_flag_is_read_and_the_required_ones_are_required() {
        let args = line(&[
            "table.toml",
            "--camera",
            "main",
            "--size",
            "320x200",
            "--out",
            "tmp/a.png",
            "--variant",
            "bare",
            "--set",
            "sky.intensity=0.4",
            "--time",
            "2",
        ])
        .unwrap();
        assert_eq!(
            args,
            Args {
                recipe: "table.toml".into(),
                camera: "main".into(),
                size: [320, 200],
                out: "tmp/a.png".into(),
                variant: Some("bare".into()),
                set: vec!["sky.intensity=0.4".into()],
                time: Some(2.0),
                pick: Default::default(),
            }
        );
        for words in [
            &["table.toml", "--camera", "main", "--size", "1x1"][..],
            &["a.toml", "b.toml"][..],
            &["a.toml", "--out", "a", "--out", "b"][..],
            &["a.toml", "--camera"][..],
            &["a.toml", "--nope", "x"][..],
        ] {
            assert!(line(words).is_err(), "{words:?}");
        }
    }

    #[test]
    fn a_recipe_it_cannot_read_exits_1() {
        let args = line(&[
            "missing.toml",
            "--camera",
            "main",
            "--size",
            "8x8",
            "--out",
            "tmp/never.png",
        ])
        .unwrap();
        let code = run(&args, &Registry::new());
        assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::FAILURE));
    }
}
