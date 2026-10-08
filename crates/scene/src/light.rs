use crate::Error;
use crate::memory::Memory;
use manfred_recipe::{CAP_LUMINANCE, CAP_MEAN, CAPPED, Recipe, Room, SkyAmbient, SkyKind, View};
use pfx_core::daylight::{Authored, Daylight};
use pfx_core::sky::AnalyticSky;
use pfx_live::effects::RoomHaze;
use pfx_live::frame::Sun;
use pfx_live::lights::{LocalLight, MAX_LIGHTS};
use pfx_live::sky::SkySource;
use pfx_load::{RoomLamp, RoomSky, Sky};
use pfx_post::{Bloom, Chain, Pass, Saturation, Tone, Vignette, Warmth};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

const TURBIDITY: f32 = 3.0;
const GROUND: [f32; 3] = [0.2, 0.2, 0.2];
const DAYLIGHT_RADIUS_DEG: f32 = 0.27;
const MIX_FILL: f64 = 0.25;
const LIGHT_FLOOR: f64 = 0.01;
const REFERENCE_HOUR: f64 = 16.0;
const SKY_FILL: f64 = 1.6;
const HAZE_LO: [f32; 3] = [-1.0, -0.02, -0.7];
const HAZE_HI: [f32; 3] = [1.0, 0.95, 0.95];
const HAZE_PHASE: f32 = 0.35;
const HAZE_BACK: f32 = 0.7;
const AUTHORED: Authored = Authored {
    toward: [0.450_789_6, 0.621_087_9, 0.641_122_9],
    colour: [1.0, 0.86, 0.66],
    sky_fill: SKY_FILL,
};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    Authored {
        toward: [f64; 3],
        colour: [f64; 3],
        irradiance: f64,
        radius_deg: f64,
    },
    Daylight(Daylight),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Hourly {
    Fixed,
    Daylight,
    Authored,
}

#[derive(Clone, Debug)]
pub(crate) enum Backdrop {
    Analytic { intensity: f32 },
    Texels { sky: Sky, hourly: Hourly },
}

#[derive(Clone, Debug)]
pub struct Lighting {
    source: Source,
    backdrop: Arc<Backdrop>,
    shade: BTreeMap<u32, [f64; 3]>,
    pub lights: Vec<LocalLight>,
    pub finish: Chain,
    pub haze: Option<RoomHaze>,
}

#[derive(Clone, Copy)]
pub struct Shine {
    pub sun: Sun,
    pub hour: f64,
    pub radius_deg: f32,
    ambient: f64,
}

#[derive(Clone)]
pub struct Lit {
    pub sky: SkySource,
    pub hour: f32,
}

impl Lighting {
    pub fn of(recipe: &Recipe) -> Result<Lighting, Error> {
        Lighting::kept(recipe, &Memory::default())
    }

    pub fn kept(recipe: &Recipe, memory: &Memory) -> Result<Lighting, Error> {
        let source = match (&recipe.sun, &recipe.daylight) {
            (Some(sun), _) => Source::Authored {
                toward: sun.toward,
                colour: sun.color,
                irradiance: sun.irradiance,
                radius_deg: sun.radius_deg,
            },
            (None, Some(daylight)) => Source::Daylight(Daylight {
                hour: daylight.hour,
                day: f64::from(daylight.day),
                latitude: daylight.latitude,
                heading: daylight.heading,
            }),
            (None, None) => Source::Daylight(reference(REFERENCE_HOUR)),
        };
        let backdrop = match &recipe.sky {
            None => Arc::new(Backdrop::Analytic { intensity: 1.0 }),
            Some(sky) => {
                let file = hdr_file(recipe, sky);
                memory.backdrop(&recipe.base, sky, file.as_deref(), || backdrop(recipe, sky))?
            }
        };
        Ok(Lighting {
            source,
            backdrop,
            shade: recipe
                .sky
                .as_ref()
                .map(|sky| sky.shade.clone())
                .unwrap_or_default(),
            lights: lights(recipe)?,
            finish: finish(recipe),
            haze: haze(recipe),
        })
    }

    pub fn shine(&self, hour: Option<f64>) -> Shine {
        let (sun, ambient, hour, radius_deg) = match self.source {
            Source::Authored {
                toward,
                colour,
                irradiance,
                radius_deg,
            } => match hour {
                None => (
                    Sun {
                        direction: unit(toward),
                        colour: colour.map(|v| v as f32),
                        intensity: irradiance as f32,
                    },
                    1.0,
                    REFERENCE_HOUR,
                    radius_deg,
                ),
                Some(hour) => {
                    let light = reference(hour).authored(
                        &Authored {
                            toward,
                            colour,
                            sky_fill: SKY_FILL,
                        },
                        REFERENCE_HOUR,
                    );
                    (
                        Sun {
                            direction: unit(light.toward),
                            colour: light.colour.map(|v| v as f32),
                            intensity: (irradiance * light.intensity) as f32,
                        },
                        light.ambient,
                        hour,
                        radius_deg,
                    )
                }
            },
            Source::Daylight(daylight) => {
                let daylight = Daylight {
                    hour: hour.unwrap_or(daylight.hour),
                    ..daylight
                };
                (
                    Sun::from_daylight(daylight, REFERENCE_HOUR),
                    daylight.light(REFERENCE_HOUR).ambient,
                    daylight.hour,
                    f64::from(DAYLIGHT_RADIUS_DEG),
                )
            }
        };
        Shine {
            sun,
            hour,
            radius_deg: radius_deg as f32,
            ambient,
        }
    }

    pub fn at(&self, hour: Option<f64>) -> Lit {
        let Shine {
            sun, hour, ambient, ..
        } = self.shine(hour);
        let tint = shade_at(&self.shade, hour);
        let sky = match &*self.backdrop {
            Backdrop::Analytic { intensity } => SkySource::Analytic(AnalyticSky {
                sun: sun.direction,
                sun_colour: sun.colour,
                sun_intensity: sun.intensity,
                ambient: ambient as f32 * intensity,
                turbidity: TURBIDITY,
                ground_albedo: GROUND,
            }),
            Backdrop::Texels { sky, hourly } => {
                let scale = match hourly {
                    Hourly::Fixed => 1.0,
                    Hourly::Daylight => ambient,
                    Hourly::Authored => match self.source {
                        Source::Authored { .. } => ambient,
                        Source::Daylight(daylight) => {
                            Daylight { hour, ..daylight }
                                .authored(&AUTHORED, REFERENCE_HOUR)
                                .ambient
                        }
                    },
                };
                let [r, g, b] = tint.map(|channel| (channel * scale) as f32);
                SkySource::Hdr(Sky {
                    width: sky.width,
                    height: sky.height,
                    texels: sky
                        .texels
                        .iter()
                        .map(|texel| [texel[0] * r, texel[1] * g, texel[2] * b, texel[3]])
                        .collect(),
                })
            }
        };
        Lit {
            sky,
            hour: hour as f32,
        }
    }
}

type Ask = (u64, Option<f64>);
type Answer = (u64, Option<f64>, Lit);

pub struct Hours {
    lighting: Arc<Lighting>,
    asks: Option<Sender<Ask>>,
    answers: Option<Receiver<Answer>>,
    asked: u64,
    answered: u64,
}

impl Hours {
    pub fn new(lighting: Arc<Lighting>) -> Hours {
        Hours {
            lighting,
            asks: None,
            answers: None,
            asked: 0,
            answered: 0,
        }
    }

    pub fn ask(&mut self, hour: Option<f64>) {
        if self.asks.is_none() {
            let (ask, asks) = mpsc::channel();
            let (answer, answers) = mpsc::channel();
            let lighting = self.lighting.clone();
            std::thread::spawn(move || serve(&lighting, &asks, &answer));
            self.asks = Some(ask);
            self.answers = Some(answers);
        }
        self.asked += 1;
        let sent = self
            .asks
            .as_ref()
            .is_some_and(|asks| asks.send((self.asked, hour)).is_ok());
        if !sent {
            self.lost();
        }
    }

    pub fn busy(&self) -> bool {
        self.answered < self.asked
    }

    pub fn take(&mut self) -> Option<(Option<f64>, Lit)> {
        let answers = self.answers.as_ref()?;
        match answers.try_recv() {
            Ok(first) => {
                let (id, hour, lit) = newest(first, answers);
                self.answered = id;
                Some((hour, lit))
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.lost();
                None
            }
        }
    }

    #[cfg(test)]
    pub fn wait(&mut self) -> Option<(Option<f64>, Lit)> {
        let mut last = None;
        while self.busy() {
            let Some(answer) = self.answers.as_ref().and_then(|a| a.recv().ok()) else {
                self.lost();
                break;
            };
            self.answered = answer.0;
            last = Some((answer.1, answer.2));
        }
        last
    }

    fn lost(&mut self) {
        self.asks = None;
        self.answers = None;
        self.answered = self.asked;
    }
}

fn serve(lighting: &Lighting, asks: &Receiver<Ask>, answers: &Sender<Answer>) {
    while let Ok(first) = asks.recv() {
        let (id, hour) = newest(first, asks);
        if answers.send((id, hour, lighting.at(hour))).is_err() {
            return;
        }
    }
}

fn newest<T>(mut value: T, rest: &Receiver<T>) -> T {
    while let Ok(next) = rest.try_recv() {
        value = next;
    }
    value
}

fn reference(hour: f64) -> Daylight {
    Daylight {
        hour,
        day: Daylight::DAY,
        latitude: Daylight::LATITUDE,
        heading: Daylight::HEADING,
    }
}

fn backdrop(recipe: &Recipe, sky: &manfred_recipe::Sky) -> Result<Backdrop, Error> {
    let (texels, hourly) = match sky.kind {
        SkyKind::Analytic => {
            return Ok(Backdrop::Analytic {
                intensity: sky.intensity as f32,
            });
        }
        SkyKind::Room => (
            room_sky(&room(sky.room.as_ref())?),
            match sky.ambient {
                Some(SkyAmbient::Authored) => Hourly::Authored,
                None => Hourly::Fixed,
            },
        ),
        SkyKind::Hdr => (hdr(recipe, sky)?, Hourly::Daylight),
        SkyKind::Mix => {
            let daylight = hdr(recipe, sky)?;
            let room = room(sky.room.as_ref())?;
            (
                mixed(daylight, &room, sky.fill.unwrap_or(MIX_FILL) as f32),
                Hourly::Daylight,
            )
        }
    };
    Ok(Backdrop::Texels {
        sky: texels
            .rotated(-(sky.rotation_deg as f32).to_radians())
            .exposed(sky.intensity as f32),
        hourly,
    })
}

fn hdr_file(recipe: &Recipe, sky: &manfred_recipe::Sky) -> Option<PathBuf> {
    if !matches!(sky.kind, SkyKind::Hdr | SkyKind::Mix) {
        return None;
    }
    let path = sky.path.as_ref()?;
    Some(match &recipe.base {
        Some(base) => base.join(path),
        None => Path::new(path).to_path_buf(),
    })
}

fn hdr(recipe: &Recipe, sky: &manfred_recipe::Sky) -> Result<Sky, Error> {
    let Some(file) = hdr_file(recipe, sky) else {
        return Err(Error("sky.path: an HDR sky needs a path".into()));
    };
    let bytes =
        std::fs::read(&file).map_err(|e| Error(format!("sky.path: {}: {e}", file.display())))?;
    let parsed =
        Sky::parse(&bytes).map_err(|e| Error(format!("sky.path: {}: {e}", file.display())))?;
    let balance = sky.balance.map(|b| b.map(|v| v as f32));
    match sky.prepare.as_deref() {
        None => Ok(match balance {
            Some(target) => parsed.balanced_to(target),
            None => parsed,
        }),
        Some(CAPPED) => Ok(capped(parsed, balance)),
        Some(other) => Err(Error(format!(
            "sky.prepare: there is no preparation named \"{other}\", expected \"{CAPPED}\""
        ))),
    }
}

pub fn room(room: Option<&Room>) -> Result<RoomSky, Error> {
    let Some(room) = room else {
        return Err(Error(
            "sky.room: a lamp room sky needs a [sky.room] table".into(),
        ));
    };
    let built = RoomSky {
        width: room.width,
        floor: room.floor.map(|v| v as f32),
        wall: room.wall.map(|v| v as f32),
        ceiling: room.ceiling.map(|v| v as f32),
        lights: room
            .lights
            .iter()
            .map(|lamp| RoomLamp {
                name: lamp.name.clone(),
                az: lamp.az as f32,
                el: lamp.el as f32,
                power: lamp.power as f32,
                color: lamp.color.map(|v| v as f32),
                width: lamp.width as f32,
                height: lamp.height as f32,
                soft: lamp.soft as f32,
                slats: lamp.slats as f32,
                open: lamp.open as f32,
            })
            .collect(),
    };
    built.check().map_err(|e| Error(format!("sky.room: {e}")))?;
    Ok(built)
}

fn room_sky(room: &RoomSky) -> Sky {
    Sky::room(room)
}

fn capped(sky: Sky, balance: Option<[f32; 3]>) -> Sky {
    let sky = sky.capped_luminance(CAP_LUMINANCE as f32);
    match balance {
        Some(target) => sky.balanced_to(target),
        None => sky,
    }
    .scaled_to_mean_luminance(CAP_MEAN as f32)
}

fn mixed(mut daylight: Sky, room: &RoomSky, fill: f32) -> Sky {
    let bare = RoomSky {
        width: daylight.width,
        floor: [0.0; 3],
        wall: [0.0; 3],
        ceiling: [0.0; 3],
        lights: room.lights.clone(),
    };
    let lamps = room_sky(&bare);
    if lamps.width != daylight.width || lamps.height != daylight.height {
        return daylight;
    }
    let have = lamps.mean_luminance();
    if have <= 1e-9 {
        return daylight;
    }
    let gain = fill * daylight.mean_luminance() / have;
    for (texel, lamp) in daylight.texels.iter_mut().zip(&lamps.texels) {
        for k in 0..3 {
            texel[k] += lamp[k] * gain;
        }
    }
    daylight
}

fn lights(recipe: &Recipe) -> Result<Vec<LocalLight>, Error> {
    if recipe.lights.len() > MAX_LIGHTS {
        return Err(Error(format!(
            "light: the engine draws {MAX_LIGHTS} local lights at most, the recipe has {}",
            recipe.lights.len()
        )));
    }
    Ok(recipe
        .lights
        .iter()
        .map(|light| LocalLight {
            position: light.position.map(|v| v as f32),
            colour: light.color.map(|v| v as f32),
            intensity: light.intensity as f32,
            radius: light.radius as f32,
            range: reach(light.range, light.intensity) as f32,
            shadow: light.shadow,
        })
        .collect())
}

pub fn reach(range: f64, intensity: f64) -> f64 {
    if range > 0.0 {
        range
    } else {
        (intensity.max(0.0) / LIGHT_FLOOR).sqrt()
    }
}

fn finish(recipe: &Recipe) -> Chain {
    let Some(look) = &recipe.look else {
        return graded(1.0, 0.0, 0.0, 1.0, 0.0);
    };
    if look.view == View::Standard {
        return Chain {
            passes: vec![Pass::Exposure(look.exposure as f32)],
            ..Chain::new()
        };
    }
    let grade = &look.grade;
    graded(
        look.exposure as f32,
        grade.bloom as f32,
        grade.vignette as f32,
        grade.saturation as f32,
        grade.warmth as f32,
    )
}

fn graded(exposure: f32, bloom: f32, vignette: f32, saturation: f32, warmth: f32) -> Chain {
    Chain {
        passes: vec![
            Pass::Exposure(exposure),
            Pass::Bloom(Bloom::pyramid(bloom, 0.9, 0.7, 0.6, 0.15, 12, 50.0, 2)),
            Pass::Vignette(Vignette::power(vignette, 2.4, 0.8, 1.28)),
            Pass::Saturation(Saturation::rec709(saturation)),
            Pass::Warmth(Warmth::linear(warmth)),
            Pass::Tone(Tone::aces()),
        ],
        ..Chain::new()
    }
}

fn haze(recipe: &Recipe) -> Option<RoomHaze> {
    let look = recipe.look.as_ref()?;
    if look.view == View::Standard {
        return None;
    }
    let amount = look.grade.haze;
    (amount > 0.0).then(|| room_haze(amount as f32))
}

pub fn room_haze(amount: f32) -> RoomHaze {
    RoomHaze {
        phase: HAZE_PHASE,
        back: HAZE_BACK,
        ..RoomHaze::from_amount(HAZE_LO, HAZE_HI, amount)
    }
}

pub fn shade_at(shade: &BTreeMap<u32, [f64; 3]>, hour: f64) -> [f64; 3] {
    let mut below = None;
    let mut above = None;
    for (&at, &tint) in shade {
        let at = f64::from(at);
        if at <= hour {
            below = Some((at, tint));
        }
        if at >= hour && above.is_none() {
            above = Some((at, tint));
        }
    }
    match (below, above) {
        (None, None) => [1.0; 3],
        (Some((_, tint)), None) | (None, Some((_, tint))) => tint,
        (Some((a, low)), Some((b, high))) => {
            let t = if b > a { (hour - a) / (b - a) } else { 0.0 };
            std::array::from_fn(|k| low[k] + (high[k] - low[k]) * t)
        }
    }
}

fn unit(v: [f64; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
    v.map(|x| (x / length) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use manfred_recipe::{parse, validate};
    use std::path::PathBuf;

    fn recipe(text: &str) -> Recipe {
        parse(&format!("manfred = 1\nname = \"t\"\n{text}")).unwrap()
    }

    #[test]
    fn an_authored_sun_is_used_as_written_until_the_hour_moves() {
        let r = recipe(
            "[sun]\nmodel = \"authored\"\ntoward = [-0.45, 0.82, 0.35]\ncolor = [1.0, 0.9, 0.78]\nirradiance = 2.2\nradius_deg = 0.8\n",
        );
        let lighting = Lighting::of(&r).unwrap();
        let lit = lighting.shine(None);
        let length = (0.45f64 * 0.45 + 0.82 * 0.82 + 0.35 * 0.35).sqrt();
        assert!((lit.sun.direction[1] - (0.82 / length) as f32).abs() < 1e-6);
        assert_eq!(lit.sun.colour, [1.0, 0.9, 0.78]);
        assert!((lit.sun.intensity - 2.2).abs() < 1e-6);
        assert_eq!(lit.radius_deg, 0.8);
        let reference = lighting.shine(Some(REFERENCE_HOUR));
        for k in 0..3 {
            assert!((reference.sun.direction[k] - lit.sun.direction[k]).abs() < 1e-5);
        }
        assert!((reference.sun.intensity - 2.2).abs() < 1e-4);
        let evening = lighting.shine(Some(19.0));
        assert!(evening.sun.direction[1] < lit.sun.direction[1]);
        assert!(evening.sun.intensity < lit.sun.intensity);
        assert!(matches!(
            lighting.at(Some(19.0)).sky,
            SkySource::Analytic(_)
        ));
    }

    #[test]
    fn daylight_follows_the_hour() {
        let r = recipe("[sun]\nmodel = \"daylight\"\nhour = 9.0\nlatitude = 45.0\nheading = 0.0\n");
        let lighting = Lighting::of(&r).unwrap();
        let morning = lighting.shine(None);
        let noon = lighting.shine(Some(12.0));
        assert_eq!(morning.hour, 9.0);
        assert!(noon.sun.direction[1] > morning.sun.direction[1]);
    }

    #[test]
    fn more_than_four_lights_are_refused_and_range_zero_reaches() {
        let one = "[[light]]\nname = \"l{}\"\nposition = [0.0, 1.0, 0.0]\nintensity = 4.0\n";
        let five: String = (0..5).map(|i| one.replace("{}", &i.to_string())).collect();
        let error = Lighting::of(&recipe(&five)).unwrap_err();
        assert!(error.0.contains("4 local lights at most"), "{error}");
        let four: String = (0..4).map(|i| one.replace("{}", &i.to_string())).collect();
        let lighting = Lighting::of(&recipe(&four)).unwrap();
        assert_eq!(lighting.lights.len(), 4);
        assert!((lighting.lights[0].range - 20.0).abs() < 1e-4);
        assert!(lighting.lights[0].lit());
    }

    #[test]
    fn the_look_becomes_the_finish_chain() {
        let plain = Lighting::of(&recipe("")).unwrap();
        assert_eq!(plain.finish.passes.len(), 6);
        assert!(matches!(plain.finish.passes[0], Pass::Exposure(e) if e == 1.0));
        let graded = Lighting::of(&recipe(
            "[look]\nexposure = 0.8\ngrade = { vignette = 0.2, saturation = 1.1, warmth = 0.1, bloom = 0.05, haze = 0.0 }\n",
        ))
        .unwrap();
        assert!(matches!(graded.finish.passes[0], Pass::Exposure(e) if (e - 0.8).abs() < 1e-6));
        assert!(matches!(graded.finish.passes[5], Pass::Tone(_)));
    }

    #[test]
    fn the_standard_view_is_exposure_alone_and_the_graded_view_is_the_default() {
        let standard =
            Lighting::of(&recipe("[look]\nview = \"standard\"\nexposure = 0.8\n")).unwrap();
        assert_eq!(standard.finish.passes.len(), 1);
        assert!(matches!(standard.finish.passes[0], Pass::Exposure(e) if (e - 0.8).abs() < 1e-6));
        assert!(standard.haze.is_none());
        let graded = Lighting::of(&recipe("[look]\nview = \"graded\"\nexposure = 0.8\n")).unwrap();
        let bare = Lighting::of(&recipe("[look]\nexposure = 0.8\n")).unwrap();
        assert_eq!(format!("{:?}", graded.finish), format!("{:?}", bare.finish));
        assert_eq!(graded.finish.passes.len(), 6);
    }

    #[test]
    fn the_shade_interpolates_between_hours() {
        let shade = BTreeMap::from([(6, [0.6, 0.6, 0.7]), (12, [1.0, 1.0, 1.0])]);
        assert_eq!(shade_at(&shade, 9.0), [0.8, 0.8, 0.85]);
        assert_eq!(shade_at(&shade, 3.0), [0.6, 0.6, 0.7]);
        assert_eq!(shade_at(&shade, 20.0), [1.0, 1.0, 1.0]);
        assert_eq!(shade_at(&BTreeMap::new(), 9.0), [1.0, 1.0, 1.0]);
    }

    const ROOM: &str = "[sky.room]\nwidth = 64\nfloor = [0.22, 0.135, 0.068]\nwall = [0.19, 0.13, 0.07]\nceiling = [0.28, 0.23, 0.16]\n[[sky.room.lights]]\nname = \"window\"\naz = -64.0\nel = 14.0\nwidth = 37.5\nheight = 29.0\npower = 10.5\ncolor = [1.0, 0.86, 0.66]\nsoft = 0.05\nslats = 6.0\nopen = 0.7\n";

    #[test]
    fn a_room_sky_is_the_engines_room_turned_and_exposed() {
        let r = recipe(&format!(
            "[sun]\nmodel = \"authored\"\ntoward = [0.45, 0.62, 0.64]\nirradiance = 4.5\n[sky]\nkind = \"room\"\nrotation_deg = 30.0\nintensity = 1.5\n{ROOM}"
        ));
        let SkySource::Hdr(sky) = Lighting::of(&r).unwrap().at(None).sky else {
            panic!("a room sky is texels");
        };
        let room = room(r.sky.as_ref().unwrap().room.as_ref()).unwrap();
        assert_eq!(room.lights[0].slats, 6.0);
        let expected = room_sky(&room).rotated(-30.0_f32.to_radians()).exposed(1.5);
        assert_eq!((sky.width, sky.height), (64, 32));
        assert!(
            sky.texels == expected.texels,
            "the room differs from the engine's"
        );
        let brightest = sky.texels.iter().map(|t| t[0]).fold(0.0_f32, f32::max);
        assert!(brightest > 5.0, "{brightest}");
    }

    fn texels(lit: Lit) -> Vec<[f32; 4]> {
        match lit.sky {
            SkySource::Hdr(sky) => sky.texels,
            SkySource::Analytic(_) => panic!("a room sky is texels"),
        }
    }

    #[test]
    fn a_room_takes_the_authored_suns_hourly_ambient_only_when_asked() {
        let sun = "[sun]\nmodel = \"authored\"\ntoward = [0.45, 0.62, 0.64]\ncolor = [1.0, 0.86, 0.66]\nirradiance = 4.5\n";
        let fixed = Lighting::of(&recipe(&format!(
            "{sun}[sky]\nkind = \"room\"\nintensity = 1.5\n{ROOM}"
        )))
        .unwrap();
        let hourly = Lighting::of(&recipe(&format!(
            "{sun}[sky]\nkind = \"room\"\nintensity = 1.5\nambient = \"authored\"\n{ROOM}"
        )))
        .unwrap();
        let room = room_sky(
            &room(
                recipe(&format!("[sky]\nkind = \"room\"\n{ROOM}"))
                    .sky
                    .unwrap()
                    .room
                    .as_ref(),
            )
            .unwrap(),
        )
        .rotated(0.0)
        .exposed(1.5);
        assert!(texels(fixed.at(Some(9.0))) == room.texels);
        assert!(texels(fixed.at(Some(19.0))) == room.texels);
        assert!(texels(hourly.at(None)) == room.texels);
        let authored = Authored {
            toward: [0.45, 0.62, 0.64],
            colour: [1.0, 0.86, 0.66],
            sky_fill: SKY_FILL,
        };
        for hour in [7.0, 12.0, 19.0] {
            let ambient = reference(hour).authored(&authored, REFERENCE_HOUR).ambient;
            assert!((ambient - 1.0).abs() > 1e-3, "{hour}: {ambient}");
            let expected = room.clone().exposed(ambient as f32);
            assert!(
                texels(hourly.at(Some(hour))) == expected.texels,
                "the room at {hour} is not the engine's room exposed by the authored ambient"
            );
        }
    }

    #[test]
    fn the_authored_ambient_without_a_sun_uses_manfreds_authored_sun() {
        let hourly = Lighting::of(&recipe(&format!(
            "[sun]\nmodel = \"daylight\"\nhour = 10.0\nlatitude = 45.0\nheading = 0.0\n[sky]\nkind = \"room\"\nambient = \"authored\"\n{ROOM}"
        )))
        .unwrap();
        let room = room_sky(
            &room(
                recipe(&format!("[sky]\nkind = \"room\"\n{ROOM}"))
                    .sky
                    .unwrap()
                    .room
                    .as_ref(),
            )
            .unwrap(),
        )
        .rotated(0.0)
        .exposed(1.0);
        let Source::Daylight(daylight) = hourly.source else {
            panic!("a recipe without a sun lights by daylight");
        };
        assert_eq!(daylight.latitude, 45.0);
        let ambient = Daylight {
            hour: 18.0,
            ..daylight
        }
        .authored(&AUTHORED, REFERENCE_HOUR)
        .ambient;
        assert!(texels(hourly.at(Some(18.0))) == room.exposed(ambient as f32).texels);
    }

    #[test]
    fn the_hour_worker_answers_only_the_newest_request() {
        let lighting = Lighting::of(&recipe(
            "[sun]\nmodel = \"daylight\"\nhour = 9.0\nlatitude = 45.0\nheading = 0.0\n",
        ))
        .unwrap();
        let (ask, asks) = mpsc::channel();
        let (answer, answers) = mpsc::channel();
        for (id, hour) in [(1, 6.0), (2, 9.5), (3, 13.0), (4, 17.25)] {
            ask.send((id, Some(hour))).unwrap();
        }
        drop(ask);
        serve(&lighting, &asks, &answer);
        drop(answer);
        let answered: Vec<(u64, Option<f64>, f32)> = answers
            .iter()
            .map(|(id, hour, lit)| (id, hour, lit.hour))
            .collect();
        assert_eq!(answered, [(4, Some(17.25), 17.25)]);
    }

    #[test]
    fn a_drag_of_hours_settles_on_the_last_one() {
        let lighting = Arc::new(
            Lighting::of(&recipe(
                "[sun]\nmodel = \"daylight\"\nhour = 9.0\nlatitude = 45.0\nheading = 0.0\n",
            ))
            .unwrap(),
        );
        let mut hours = Hours::new(lighting);
        assert!(!hours.busy());
        assert!(hours.take().is_none());
        for step in 0..=40 {
            hours.ask(Some(6.0 + f64::from(step) * 0.25));
        }
        assert!(hours.busy());
        let (hour, lit) = hours.wait().unwrap();
        assert_eq!(hour, Some(16.0));
        assert_eq!(lit.hour, 16.0);
        assert!(!hours.busy());
        assert!(hours.take().is_none());
    }

    #[test]
    fn a_room_outside_the_engines_limits_is_an_error() {
        let r = recipe("[sky]\nkind = \"room\"\n[sky.room]\nwidth = 9000\n");
        let error = Lighting::of(&r).unwrap_err();
        assert!(
            error.0.starts_with("sky.room: room width is 9000"),
            "{error}"
        );
    }

    #[test]
    fn a_mix_adds_the_lamps_at_their_fill_of_the_daylight() {
        let r = recipe(&format!("[sky]\nkind = \"room\"\n{ROOM}"));
        let room = room(r.sky.as_ref().unwrap().room.as_ref()).unwrap();
        let daylight = Sky {
            width: 64,
            height: 32,
            texels: vec![[0.3, 0.3, 0.3, 1.0]; 64 * 32],
        };
        let mean = daylight.mean_luminance();
        let mixed = mixed(daylight.clone(), &room, 0.25);
        assert!((mixed.mean_luminance() - 1.25 * mean).abs() < 1e-4);
        assert!(mixed.texels.iter().all(|t| t[0] >= 0.3));
        let narrow = Sky {
            width: 8,
            height: 2,
            texels: vec![[0.3, 0.3, 0.3, 1.0]; 16],
        };
        assert_eq!(mixed_texels(&narrow, &room), narrow.texels);
    }

    fn mixed_texels(sky: &Sky, room: &RoomSky) -> Vec<[f32; 4]> {
        mixed(sky.clone(), room, 0.25).texels
    }

    #[test]
    fn haze_above_zero_sets_the_room_haze() {
        let none = Lighting::of(&recipe("[look]\ngrade = { haze = 0.0 }\n")).unwrap();
        assert_eq!(none.haze, None);
        let plain = Lighting::of(&recipe("")).unwrap();
        assert_eq!(plain.haze, None);
        let hazy = Lighting::of(&recipe("[look]\ngrade = { haze = 0.3 }\n")).unwrap();
        assert_eq!(hazy.haze, Some(room_haze(0.3)));
    }

    #[test]
    fn a_missing_hdr_file_is_an_error() {
        let r = recipe(
            "[sky]\nkind = \"hdr\"\npath = \"nowhere.hdr\"\nsource = \"test\"\n[sky.prepare]\ncap = 60.0\nmean = 0.3\n",
        );
        let error = Lighting::of(&r).unwrap_err();
        assert!(error.0.starts_with("sky.path: nowhere.hdr"), "{error}");
    }

    const TABLE: &str = include_str!("../../../tests/golden/table/table.toml");
    const SIZE: [u32; 2] = [800, 500];

    fn table(sky: &str, look: &str) -> Recipe {
        let cut = TABLE.find("[sky]").unwrap();
        parse(&format!("{}{sky}\n{look}", &TABLE[..cut])).unwrap()
    }

    fn window_hdr() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/sky/window.hdr")
    }

    #[test]
    fn an_hdr_sky_is_read_capped_and_balanced() {
        let path = window_hdr().display().to_string();
        let plain = recipe(&format!(
            "[sky]\nkind = \"hdr\"\npath = {path:?}\nsource = \"manfred\"\n"
        ));
        let capped = recipe(&format!(
            "[sky]\nkind = \"hdr\"\npath = {path:?}\nsource = \"manfred\"\n[sky.prepare]\ncap = 60.0\nbalance = [1.0, 0.92, 0.8]\nmean = 0.3\n"
        ));
        let plain = texels(Lighting::of(&plain).unwrap().at(None));
        let capped = texels(Lighting::of(&capped).unwrap().at(None));
        assert_eq!(plain.len(), 128 * 64);
        let brightest = |t: &[[f32; 4]]| t.iter().map(|p| p[0]).fold(0.0_f32, f32::max);
        assert!(brightest(&plain) > 30.0, "{}", brightest(&plain));
        assert!(brightest(&capped) < brightest(&plain));
        let parsed = Sky::parse(&std::fs::read(window_hdr()).unwrap()).unwrap();
        assert!(
            capped
                == parsed
                    .capped_luminance(60.0)
                    .balanced_to([1.0, 0.92, 0.8])
                    .scaled_to_mean_luminance(0.3)
                    .texels
        );
    }

    fn shot(recipe: &Recipe, name: &str) -> Vec<u8> {
        let built = manfred_build::build(recipe, Path::new(".")).unwrap();
        let gpu = crate::headless().unwrap();
        let rgba = crate::shoot(&gpu, &built.recipe(recipe), &built.parts, "main", SIZE).unwrap();
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(name);
        crate::write_png(&out, SIZE, &rgba).unwrap();
        pfx::adapter::turn();
        rgba
    }

    fn mean(rgba: &[u8]) -> f64 {
        rgba.iter().map(|&v| f64::from(v)).sum::<f64>() / rgba.len() as f64
    }

    fn apart(a: &[u8], b: &[u8]) -> f64 {
        assert_eq!(a.len(), b.len());
        a.iter()
            .zip(b)
            .map(|(&a, &b)| f64::from(a.abs_diff(b)))
            .sum::<f64>()
            / a.len() as f64
    }

    #[test]
    #[ignore = "GPU: pgpu run --class interactive --as manfred -- cargo test -- --ignored"]
    fn the_table_renders_under_a_lamp_room() {
        let room = table(
            &format!("[sky]\nkind = \"room\"\nrotation_deg = 169.4\nintensity = 4.2\n\n{ROOM}"),
            "[look]\nexposure = 1.0\n",
        );
        assert!(validate(&room).is_empty(), "{:?}", validate(&room));
        let rgba = shot(&room, "sky-table-room.png");
        let level = mean(&rgba);
        assert!(level > 20.0 && level < 235.0, "mean {level}");
    }

    fn evening(recipe: &Recipe, name: &str) -> Vec<u8> {
        let built = manfred_build::build(recipe, Path::new(".")).unwrap();
        let drawn = built.recipe(recipe);
        let mut stage =
            crate::Stage::with_parts(crate::headless().unwrap(), &drawn, &built.parts, SIZE)
                .unwrap();
        stage.set_hour(Some(19.0)).unwrap();
        let eye = crate::Eye::from_camera(
            crate::camera(recipe, "main").unwrap(),
            f64::from(SIZE[0]) / f64::from(SIZE[1]),
        );
        let target = stage
            .gpu()
            .offscreen(SIZE[0], SIZE[1], stage.output_format())
            .unwrap();
        let mut turns = pfx::adapter::TurnState::default();
        for frame in 0..crate::SETTLE_FRAMES {
            let measured = stage
                .render_measured(&eye, frame as f32 / crate::FRAME_RATE, &target.view)
                .unwrap();
            pfx::adapter::turn_soon(&mut turns, measured.unwrap_or(crate::UNMEASURED_MS));
        }
        let rgba = stage.gpu().readback_rgba8(&target).unwrap();
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(name);
        crate::write_png(&out, SIZE, &rgba).unwrap();
        pfx::adapter::turn();
        rgba
    }

    #[test]
    #[ignore = "GPU: pgpu run --class interactive --as manfred -- cargo test -- --ignored"]
    fn the_table_renders_under_the_lamp_room_with_the_authored_hourly_ambient() {
        let sky =
            format!("[sky]\nkind = \"room\"\nrotation_deg = 169.4\nintensity = 4.2\n\n{ROOM}");
        let fixed = table(&sky, "[look]\nexposure = 1.0\n");
        let hourly = table(
            &sky.replace(
                "intensity = 4.2\n",
                "intensity = 4.2\nambient = \"authored\"\n",
            ),
            "[look]\nexposure = 1.0\n",
        );
        assert!(validate(&hourly).is_empty(), "{:?}", validate(&hourly));
        assert_eq!(
            hourly.sky.as_ref().unwrap().ambient,
            Some(SkyAmbient::Authored)
        );
        let fixed = evening(&fixed, "sky-table-room-19h.png");
        let hourly = evening(&hourly, "sky-table-room-ambient-19h.png");
        let (fixed, hourly) = (mean(&fixed), mean(&hourly));
        println!(
            "the table at 19:00 under the lamp room: mean {fixed:.2}, with the authored ambient {hourly:.2}"
        );
        assert!(hourly > 5.0 && hourly < 235.0, "mean {hourly}");
        assert!((fixed - hourly).abs() > 0.5, "{fixed} against {hourly}");
    }

    #[test]
    #[ignore = "GPU: pgpu run --class interactive --as manfred -- cargo test -- --ignored"]
    fn the_table_renders_under_the_window_hdr_capped_and_balanced() {
        let sky = format!(
            "[sky]\nkind = \"hdr\"\npath = {:?}\nsource = \"Manfred's own test sky\"\nrotation_deg = 169.4\nintensity = 4.2\n[sky.prepare]\ncap = 60.0\nbalance = [1.0, 0.92, 0.8]\nmean = 0.3\n",
            window_hdr().display().to_string(),
        );
        let recipe = table(&sky, "[look]\nexposure = 1.0\n");
        let rgba = shot(&recipe, "sky-table-hdr.png");
        let level = mean(&rgba);
        assert!(level > 20.0 && level < 235.0, "mean {level}");
    }

    #[test]
    #[ignore = "GPU: pgpu run --class interactive --as manfred -- cargo test -- --ignored"]
    fn the_table_renders_with_haze() {
        let sky = "[sky]\nkind = \"analytic\"\nintensity = 0.9\n";
        let clear = shot(
            &table(sky, "[look]\nexposure = 1.0\n"),
            "sky-table-clear.png",
        );
        let hazy = table(sky, "[look]\nexposure = 1.0\ngrade = { haze = 0.3 }\n");
        assert_eq!(Lighting::of(&hazy).unwrap().haze, Some(room_haze(0.3)));
        let hazy = shot(&hazy, "sky-table-haze.png");
        let gap = apart(&clear, &hazy);
        println!("the table with haze 0.3: mean difference {gap:.2}");
        assert!(gap > 1.0, "{gap}");
    }
}
