use crate::text::Overlay;
use pfx_post::{BloomKind, Chain, Image, Pass, Vignette, VignetteKind, bayer, color};
use pfx_trace::denoise;
use std::borrow::Cow;
use std::path::Path;

const RLE_LEAST: usize = 8;
const RLE_MOST: usize = 32767;
const RUN_LEAST: usize = 4;
const RUN_MOST: usize = 127;
const LITERAL_MOST: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Exr,
    Hdr,
}

impl Format {
    pub fn of(path: &Path) -> Option<Format> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "png" => Some(Format::Png),
            "exr" => Some(Format::Exr),
            "hdr" => Some(Format::Hdr),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Film {
    pub size: [u32; 2],
    pub color: Vec<[f32; 4]>,
    pub albedo: Vec<[f32; 4]>,
    pub normal: Vec<[f32; 4]>,
    pub denoised: Option<Vec<[f32; 4]>>,
    pub samples: u32,
}

pub fn cut(pixels: &[[f32; 4]], width: u32, origin: [u32; 2], size: [u32; 2]) -> Vec<[f32; 4]> {
    (origin[1]..origin[1] + size[1])
        .flat_map(|y| {
            let at = (y * width + origin[0]) as usize;
            pixels[at..at + size[0] as usize].iter().copied()
        })
        .collect()
}

impl Film {
    pub fn cut(&self, origin: [u32; 2], size: [u32; 2]) -> Film {
        let take = |pixels: &[[f32; 4]]| {
            if pixels.is_empty() {
                Vec::new()
            } else {
                cut(pixels, self.size[0], origin, size)
            }
        };
        Film {
            size,
            color: take(&self.color),
            albedo: take(&self.albedo),
            normal: take(&self.normal),
            denoised: self.denoised.as_deref().map(take),
            samples: self.samples,
        }
    }

    pub fn mean(&self) -> [f64; 3] {
        let mut sum = [0.0f64; 3];
        for pixel in &self.color {
            for (total, value) in sum.iter_mut().zip(pixel) {
                *total += f64::from(*value);
            }
        }
        let count = self.color.len().max(1) as f64;
        sum.map(|total| total / count)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Look<'a> {
    pub chain: &'a Chain,
    pub overlay: Option<&'a Overlay>,
}

impl<'a> From<&'a Chain> for Look<'a> {
    fn from(chain: &'a Chain) -> Look<'a> {
        Look {
            chain,
            overlay: None,
        }
    }
}

impl<'a> From<(&'a Chain, &'a Overlay)> for Look<'a> {
    fn from((chain, overlay): (&'a Chain, &'a Overlay)) -> Look<'a> {
        Look {
            chain,
            overlay: Some(overlay).filter(|overlay| !overlay.is_empty()),
        }
    }
}

pub fn looked<'a>(film: &Film, finish: impl Into<Look<'a>>) -> Result<Vec<u8>, String> {
    looked_with(film, finish, true)
}

pub fn looked_with<'a>(
    film: &Film,
    finish: impl Into<Look<'a>>,
    denoised: bool,
) -> Result<Vec<u8>, String> {
    Ok(finished(film, finish, denoised)?
        .into_iter()
        .flat_map(|pixel| {
            let [r, g, b, _] = pixel.map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8);
            [r, g, b, 255]
        })
        .collect())
}

pub fn developed(film: &Film, denoised: bool) -> Result<Cow<'_, [[f32; 4]]>, String> {
    let [width, height] = film.size;
    match (denoised, &film.denoised) {
        (true, Some(pixels)) => Ok(Cow::Borrowed(pixels)),
        (true, None) => denoise::filter(
            width,
            height,
            &film.color,
            &film.albedo,
            &film.normal,
            film.samples.max(1),
        )
        .map(Cow::Owned),
        (false, _) => Ok(Cow::Borrowed(&film.color)),
    }
}

pub fn finished<'a>(
    film: &Film,
    finish: impl Into<Look<'a>>,
    denoised: bool,
) -> Result<Vec<[f32; 4]>, String> {
    finished_at(film, finish, denoised, [0, 0], film.size)
}

pub fn finished_at<'a>(
    film: &Film,
    finish: impl Into<Look<'a>>,
    denoised: bool,
    origin: [u32; 2],
    full: [u32; 2],
) -> Result<Vec<[f32; 4]>, String> {
    let look = finish.into();
    let [width, height] = film.size;
    let image = Image::from_pixels(width, height, developed(film, denoised)?.into_owned())
        .map_err(|_| format!("the film is not {width}x{height}"))?;
    let whole = origin == [0, 0] && film.size == full;
    let finished = match look.overlay {
        None => {
            let mut chain = look.chain.clone();
            chain.passes.push(Pass::Encode);
            if whole {
                chain.apply(&image, None, None)
            } else {
                framed(&chain, &image, origin, full)
            }
        }
        Some(_) if !whole => return Err("overlay text is placed by the whole frame".into()),
        Some(overlay) => {
            let mut finished = look.chain.apply(&image, None, None);
            overlay.paint(&mut finished.pixels, film.size)?;
            let encode = Chain {
                passes: vec![Pass::Encode],
                ..look.chain.clone()
            };
            encode.apply(&finished, None, None)
        }
    };
    Ok(finished.pixels)
}

fn framed(chain: &Chain, image: &Image, origin: [u32; 2], full: [u32; 2]) -> Image {
    chain.passes.iter().fold(image.clone(), |current, pass| {
        framed_pass(pass, &current, chain, origin, full)
    })
}

fn framed_pass(
    pass: &Pass,
    image: &Image,
    chain: &Chain,
    origin: [u32; 2],
    full: [u32; 2],
) -> Image {
    match pass {
        Pass::Vignette(vignette) => vignette_at(image, vignette, origin, full),
        Pass::Grade(passes) => passes.iter().fold(image.clone(), |current, pass| {
            framed_pass(pass, &current, chain, origin, full)
        }),
        pass => pass.run(image, None, None, chain.frame, chain.seed),
    }
}

fn vignette_at(image: &Image, pass: &Vignette, origin: [u32; 2], full: [u32; 2]) -> Image {
    if pass.strength.abs() < 1e-8 {
        return image.clone();
    }
    let w = full[0].max(1) as f32;
    let h = full[1].max(1) as f32;
    image.map_rgb(|x, y, c| {
        let st_x = ((origin[0] + x) as f32 + 0.5) / w;
        let st_y = ((origin[1] + y) as f32 + 0.5) / h;
        let factor = match pass.kind {
            VignetteKind::Power => {
                let uv_x = st_x * 2.0 - 1.0;
                let uv_y = st_y * 2.0 - 1.0;
                let r = (uv_x * uv_x + (uv_y * pass.aspect).powi(2)).sqrt() / pass.scale.max(1e-4);
                1.0 - pass.strength * color::safe_pow(r.max(0.0), pass.power)
            }
            VignetteKind::Smoothstep => {
                let dx = st_x - 0.5;
                let dy = (st_y - 0.5) * pass.aspect;
                let v = (dx * dx + dy * dy).sqrt();
                1.0 - pass.strength * color::smoothstep(pass.inner, pass.outer, v)
            }
        };
        [c[0] * factor, c[1] * factor, c[2] * factor]
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reach {
    pub pixels: u32,
    pub align: u32,
}

impl Reach {
    pub const NONE: Reach = Reach {
        pixels: 0,
        align: 1,
    };

    fn then(self, next: Reach) -> Reach {
        let gcd = |mut a: u32, mut b: u32| {
            while b != 0 {
                (a, b) = (b, a % b);
            }
            a
        };
        Reach {
            pixels: self.pixels + next.pixels,
            align: self.align / gcd(self.align, next.align) * next.align,
        }
    }
}

pub fn reach(chain: &Chain) -> Reach {
    chain
        .passes
        .iter()
        .fold(Reach::NONE, |reach, pass| reach.then(pass_reach(pass)))
}

fn pass_reach(pass: &Pass) -> Reach {
    match pass {
        Pass::Grade(passes) => passes
            .iter()
            .fold(Reach::NONE, |reach, pass| reach.then(pass_reach(pass))),
        Pass::Bloom(bloom) if !pass.is_local() => {
            let down = bloom.down.max(1);
            Reach {
                pixels: down * (bloom.radius.min(64) + 3),
                align: down,
            }
        }
        _ => Reach::NONE,
    }
}

pub fn crop_refusal<'a>(finish: impl Into<Look<'a>>, full: [u32; 2]) -> Option<String> {
    let look = finish.into();
    if look.overlay.is_some() {
        return Some("the recipe's overlay text is placed by the whole frame".into());
    }
    if let Some(name) = look.chain.passes.iter().find_map(unframed) {
        return Some(format!(
            "the recipe's finish has {name}, which reads past a crop's margin or the pixel's place in the frame"
        ));
    }
    let align = reach(look.chain).align;
    (!full[0].is_multiple_of(align) || !full[1].is_multiple_of(align)).then(|| {
        format!("the recipe's bloom works in {align}x{align} blocks, so the frame's sides must be multiples of {align}")
    })
}

fn unframed(pass: &Pass) -> Option<&'static str> {
    match pass {
        Pass::Grade(passes) => passes.iter().find_map(unframed),
        Pass::Vignette(_) => None,
        Pass::Bloom(bloom) if bloom.kind != BloomKind::Rings => None,
        Pass::Grain(_) | Pass::Dither(_) => Some(pass.name()),
        pass if pass.is_local() => None,
        pass => Some(pass.name()),
    }
}

pub fn dithered(size: [u32; 2], pixels: &[[f32; 4]]) -> Vec<u8> {
    dithered_at(size, [0, 0], pixels)
}

pub fn dithered_at(size: [u32; 2], origin: [u32; 2], pixels: &[[f32; 4]]) -> Vec<u8> {
    let width = size[0].max(1);
    pixels
        .iter()
        .enumerate()
        .flat_map(|(at, pixel)| {
            let (x, y) = (origin[0] + at as u32 % width, origin[1] + at as u32 / width);
            let offset = (bayer(x, y) - 0.5) / 255.0;
            let [r, g, b] = [0, 1, 2].map(|k| {
                ((pixel[k].clamp(0.0, 1.0) + offset).clamp(0.0, 1.0) * 255.0).round() as u8
            });
            [r, g, b, 255]
        })
        .collect()
}

pub fn master(pixels: &[[f32; 4]]) -> Vec<u8> {
    pixels
        .iter()
        .flat_map(|pixel| {
            let [r, g, b] = [0, 1, 2].map(|k| (pixel[k].clamp(0.0, 1.0) * 65535.0).round() as u16);
            [r, g, b, u16::MAX].into_iter().flat_map(u16::to_be_bytes)
        })
        .collect()
}

pub fn write_master(path: &Path, size: [u32; 2], pixels: &[[f32; 4]]) -> Result<(), String> {
    fast_png(
        path,
        size,
        pixels.len(),
        png::BitDepth::Sixteen,
        &master(pixels),
    )
}

pub fn write_delivery(
    path: &Path,
    size: [u32; 2],
    origin: [u32; 2],
    pixels: &[[f32; 4]],
) -> Result<(), String> {
    fast_png(
        path,
        size,
        pixels.len(),
        png::BitDepth::Eight,
        &dithered_at(size, origin, pixels),
    )
}

fn fast_png(
    path: &Path,
    size: [u32; 2],
    pixels: usize,
    depth: png::BitDepth,
    bytes: &[u8],
) -> Result<(), String> {
    if pixels != size[0] as usize * size[1] as usize {
        return Err(format!(
            "{}: {} pixels do not make a {}x{} image",
            path.display(),
            pixels,
            size[0],
            size[1]
        ));
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let failed = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let file = std::fs::File::create(path).map_err(|e| failed(&e))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), size[0], size[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(depth);
    encoder.set_compression(png::Compression::Fast);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder.write_header().map_err(|e| failed(&e))?;
    writer.write_image_data(bytes).map_err(|e| failed(&e))?;
    writer.finish().map_err(|e| failed(&e))
}

pub fn rgbe(rgb: [f32; 3]) -> [u8; 4] {
    let rgb = rgb.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
    let top = rgb[0].max(rgb[1]).max(rgb[2]);
    if !top.is_normal() {
        return [0; 4];
    }
    let exponent = (((top.to_bits() >> 23) & 0xff) as i32 - 126).min(127);
    let scale = 2f32.powi(8 - exponent);
    let [r, g, b] = rgb.map(|v| (v * scale).floor().min(255.0) as u8);
    [r, g, b, (exponent + 128) as u8]
}

pub fn hdr(size: [u32; 2], pixels: &[[f32; 4]]) -> Result<Vec<u8>, String> {
    let [width, height] = sides(size, pixels)?;
    let mut out =
        format!("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nSOFTWARE=manfred\n\n-Y {height} +X {width}\n")
            .into_bytes();
    let rle = (RLE_LEAST..=RLE_MOST).contains(&width);
    let mut channel = vec![0u8; width];
    for row in pixels.chunks_exact(width) {
        let scan: Vec<[u8; 4]> = row.iter().map(|p| rgbe([p[0], p[1], p[2]])).collect();
        if !rle {
            out.extend(scan.iter().flatten());
            continue;
        }
        out.extend([2, 2, (width >> 8) as u8, (width & 0xff) as u8]);
        for k in 0..4 {
            for (byte, pixel) in channel.iter_mut().zip(&scan) {
                *byte = pixel[k];
            }
            runs(&channel, &mut out);
        }
    }
    Ok(out)
}

fn runs(data: &[u8], out: &mut Vec<u8>) {
    let mut at = 0;
    while at < data.len() {
        let mut begin = at;
        let mut run = 0;
        while begin < data.len() {
            run = 1;
            while begin + run < data.len() && run < RUN_MOST && data[begin + run] == data[begin] {
                run += 1;
            }
            if run >= RUN_LEAST {
                break;
            }
            begin += run;
        }
        let found = begin < data.len() && run >= RUN_LEAST;
        let end = if found { begin } else { data.len() };
        while at < end {
            let count = (end - at).min(LITERAL_MOST);
            out.push(count as u8);
            out.extend(&data[at..at + count]);
            at += count;
        }
        if found {
            out.push((LITERAL_MOST + run) as u8);
            out.push(data[begin]);
            at = begin + run;
        }
    }
}

pub fn write_exr(path: &Path, size: [u32; 2], pixels: &[[f32; 4]]) -> Result<(), String> {
    use exr::prelude::*;
    let [width, height] = sides(size, pixels)?;
    let layer = Layer::new(
        (width, height),
        LayerAttributes::named("manfred render"),
        Encoding::FAST_LOSSLESS,
        SpecificChannels::rgba(|at: Vec2<usize>| {
            let [r, g, b, _] = pixels[at.y() * width + at.x()].map(f16::from_f32);
            (r, g, b, f16::ONE)
        }),
    );
    Image::from_layer(layer)
        .write()
        .non_parallel()
        .to_file(path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

pub fn read_exr(path: &Path) -> Result<([u32; 2], Vec<[f32; 4]>), String> {
    type Pixels = ([usize; 2], Vec<[f32; 4]>);
    let image = exr::prelude::read_first_rgba_layer_from_file(
        path,
        |resolution, _| -> Pixels {
            (
                [resolution.width(), resolution.height()],
                vec![[0.0; 4]; resolution.width() * resolution.height()],
            )
        },
        |pixels: &mut Pixels, at: exr::prelude::Vec2<usize>, (r, g, b, a): (f32, f32, f32, f32)| {
            let width = pixels.0[0];
            pixels.1[at.y() * width + at.x()] = [r, g, b, a];
        },
    )
    .map_err(|e| format!("{}: {e}", path.display()))?;
    let ([width, height], pixels) = image.layer_data.channel_data.pixels;
    Ok(([width as u32, height as u32], pixels))
}

fn sides(size: [u32; 2], pixels: &[[f32; 4]]) -> Result<[usize; 2], String> {
    let [width, height] = size.map(|side| side as usize);
    if pixels.len() != width * height || width == 0 || height == 0 {
        return Err(format!(
            "{} pixels are not a {width}x{height} picture",
            pixels.len()
        ));
    }
    Ok([width, height])
}

pub fn write_linear(path: &Path, size: [u32; 2], pixels: &[[f32; 4]]) -> Result<Format, String> {
    let format = Format::of(path)
        .filter(|format| *format != Format::Png)
        .ok_or_else(|| format!("{}: linear pixels go to .exr or .hdr", path.display()))?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    match format {
        Format::Exr => write_exr(path, size, pixels)?,
        _ => std::fs::write(path, hdr(size, pixels)?)
            .map_err(|e| format!("{}: {e}", path.display()))?,
    }
    Ok(format)
}

pub fn write<'a>(path: &Path, film: &Film, finish: impl Into<Look<'a>>) -> Result<Format, String> {
    write_with(path, film, finish, true)
}

pub fn write_with<'a>(
    path: &Path,
    film: &Film,
    finish: impl Into<Look<'a>>,
    denoised: bool,
) -> Result<Format, String> {
    let format = Format::of(path).ok_or_else(|| {
        format!(
            "{}: the out file ends in .png, .exr or .hdr",
            path.display()
        )
    })?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    match format {
        Format::Png => {
            let rgba = looked_with(film, finish, denoised)?;
            manfred_scene::write_png(path, film.size, &rgba).map_err(|e| e.0)?;
        }
        Format::Exr => write_exr(path, film.size, &film.color)?,
        Format::Hdr => std::fs::write(path, hdr(film.size, &film.color)?)
            .map_err(|e| format!("{}: {e}", path.display()))?,
    }
    Ok(format)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pfx_load::Sky;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp/render-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn known(width: usize, height: usize) -> Vec<[f32; 4]> {
        (0..width * height)
            .map(|n| {
                let k = (n % 7) as i32 - 3;
                let level = 2f32.powi(k);
                match n % 4 {
                    0 => [level, level * 0.5, level * 0.25, 1.0],
                    1 => [0.0, 0.0, 0.0, 1.0],
                    2 => [level * 0.75, level, 0.0, 1.0],
                    _ => [0.125, 0.125, 0.125, 1.0],
                }
            })
            .collect()
    }

    #[test]
    fn the_master_is_16_bit_big_endian_and_the_delivery_dithers_to_the_same_mean() {
        assert_eq!(
            master(&[[1.0, 0.5, -1.0, 0.3]]),
            [255, 255, 128, 0, 0, 0, 255, 255]
        );
        let level = 100.4 / 255.0;
        let flat = vec![[level, level, level, 1.0]; 64];
        let bytes = dithered([8, 8], &flat);
        let mean = bytes.chunks(4).map(|p| f64::from(p[0])).sum::<f64>() / 64.0;
        assert!((mean - 100.4).abs() < 0.05, "{mean}");
        assert!(
            bytes
                .chunks(4)
                .all(|p| p[3] == 255 && (100..=101).contains(&p[0]))
        );
        let path = scratch("master.png");
        write_master(&path, [8, 8], &flat).unwrap();
        let reader =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()))
                .read_info()
                .unwrap();
        assert_eq!(reader.info().bit_depth, png::BitDepth::Sixteen);
        assert!(write_master(&path, [9, 8], &flat).is_err());
    }

    #[test]
    fn rgbe_holds_powers_of_two_exactly_and_others_within_a_step() {
        assert_eq!(rgbe([1.0, 0.5, 0.25]), [128, 64, 32, 129]);
        assert_eq!(rgbe([0.0, 0.0, 0.0]), [0; 4]);
        assert_eq!(rgbe([f32::NAN, -1.0, 0.0]), [0; 4]);
        assert_eq!(rgbe([f32::INFINITY, 0.0, 0.0]), [0; 4]);
        let pixel = rgbe([1000.0, 3.5, 0.001]);
        let decoded = Sky::parse(&hdr([1, 1], &[[1000.0, 3.5, 0.001, 1.0]]).unwrap())
            .unwrap()
            .texels[0];
        assert_eq!(pixel[3], 128 + 10);
        assert!((decoded[0] - 1000.0).abs() <= 1000.0 / 128.0, "{decoded:?}");
        assert!((decoded[1] - 3.5).abs() <= 1000.0 / 128.0, "{decoded:?}");
    }

    #[test]
    fn an_hdr_reads_back_through_the_engines_sky_reader_flat_and_run_length() {
        for (width, height) in [(5, 3), (64, 4), (300, 2)] {
            let pixels = known(width, height);
            let bytes = hdr([width as u32, height as u32], &pixels).unwrap();
            let sky = Sky::parse(&bytes).unwrap();
            assert_eq!((sky.width, sky.height), (width as u32, height as u32));
            assert_eq!(sky.texels, pixels, "{width}x{height}");
        }
        let flat = vec![[0.5, 0.5, 0.5, 1.0]; 64 * 2];
        let packed = hdr([64, 2], &flat).unwrap();
        assert!(packed.len() < 64 * 2 * 4, "{}", packed.len());
        assert_eq!(Sky::parse(&packed).unwrap().texels, flat);
        assert!(hdr([4, 4], &flat).is_err());
    }

    #[test]
    fn literal_and_run_packets_split_where_the_format_allows() {
        let mut data = vec![7u8; 300];
        data.extend((0..200).map(|n| n as u8));
        data.extend([1, 1, 1, 2, 2, 2, 2]);
        let mut out = Vec::new();
        runs(&data, &mut out);
        let mut back = Vec::new();
        let mut at = 0;
        while at < out.len() {
            let count = out[at] as usize;
            assert!(count > 0);
            if count > LITERAL_MOST {
                back.extend(std::iter::repeat_n(out[at + 1], count - LITERAL_MOST));
                at += 2;
            } else {
                back.extend(&out[at + 1..at + 1 + count]);
                at += 1 + count;
            }
        }
        assert_eq!(back, data);
    }

    #[test]
    fn an_exr_round_trips_known_pixels_as_half_floats() {
        let pixels = known(37, 5);
        let path = scratch("known.exr");
        write_exr(&path, [37, 5], &pixels).unwrap();
        let (size, back) = read_exr(&path).unwrap();
        assert_eq!(size, [37, 5]);
        assert_eq!(back, pixels);
        let odd = vec![[1.0 / 3.0, 1000.25, 0.1, 1.0]];
        write_exr(&path, [1, 1], &odd).unwrap();
        let (_, back) = read_exr(&path).unwrap();
        for (got, want) in back[0].iter().zip(odd[0]) {
            assert!((got - want).abs() <= want.abs() / 1024.0, "{got} {want}");
        }
        let first = std::fs::read(&path).unwrap();
        write_exr(&path, [1, 1], &odd).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), first);
        assert!(write_exr(&path, [2, 2], &odd).is_err());
    }

    #[test]
    fn formats_follow_the_extension() {
        assert_eq!(Format::of(Path::new("a/b.png")), Some(Format::Png));
        assert_eq!(Format::of(Path::new("b.EXR")), Some(Format::Exr));
        assert_eq!(Format::of(Path::new("sky.hdr")), Some(Format::Hdr));
        assert_eq!(Format::of(Path::new("sky.jpg")), None);
        assert_eq!(Format::of(Path::new("sky")), None);
    }

    #[test]
    fn the_png_goes_through_the_look_and_srgb() {
        let size = [4, 2];
        let film = Film {
            size,
            color: vec![[0.18, 0.18, 0.18, 1.0]; 8],
            albedo: vec![[0.5, 0.5, 0.5, 1.0]; 8],
            normal: vec![[0.0, 0.0, 1.0, 1.0]; 8],
            denoised: None,
            samples: 16,
        };
        let plain = looked(&film, &Chain::new()).unwrap();
        assert_eq!(plain.len(), 32);
        assert!(
            plain.chunks_exact(4).all(|p| p == [118, 118, 118, 255]),
            "{plain:?}"
        );
        let look = graded_finish();
        let graded = looked(&film, &look).unwrap();
        assert_ne!(graded, plain);
        assert_eq!(graded, looked(&film, &look).unwrap());
        let path = scratch("look.png");
        assert_eq!(write(&path, &film, &look).unwrap(), Format::Png);
        assert!(write(&scratch("look.tga"), &film, &look).is_err());
    }

    fn graded_finish() -> Chain {
        let recipe = manfred_recipe::parse("manfred = 1\nname = \"t\"\n").unwrap();
        manfred_scene::Lighting::of(&recipe).unwrap().finish
    }

    fn bloomed_finish() -> Chain {
        let recipe = manfred_recipe::parse(
            "manfred = 1\nname = \"t\"\n[look]\ngrade = { vignette = 0.3, saturation = 1.1, warmth = 0.05, bloom = 0.5, haze = 0.0 }\n",
        )
        .unwrap();
        manfred_scene::Lighting::of(&recipe).unwrap().finish
    }

    #[test]
    fn a_crop_finished_in_its_margin_is_the_whole_frames_finish_there() {
        let full = [96, 64];
        let pixels: Vec<[f32; 4]> = (0..full[0] * full[1])
            .map(|n| {
                let (x, y) = (n % full[0], n / full[0]);
                let hot = if (x / 7 + y / 5) % 9 == 0 { 6.0 } else { 0.2 };
                [
                    hot * (1.0 + (x % 3) as f32 * 0.1),
                    hot * 0.8,
                    hot * (y % 4) as f32 * 0.3,
                    1.0,
                ]
            })
            .collect();
        let film = |size, color: Vec<[f32; 4]>| Film {
            size,
            albedo: color.clone(),
            normal: vec![[0.0, 0.0, 1.0, 1.0]; color.len()],
            color,
            denoised: None,
            samples: 1,
        };
        let finish = bloomed_finish();
        assert!(crop_refusal(&finish, full).is_none());
        assert!(crop_refusal(&finish, [95, 64]).is_some());
        let Reach {
            pixels: margin,
            align,
        } = reach(&finish);
        assert_eq!(align, 2);
        let whole = finished(&film(full, pixels.clone()), &finish, false).unwrap();
        let rect = [41, 23, 9, 7];
        let origin = [(rect[0] - margin) / align * align, 0];
        let size = [full[0] - origin[0], rect[1] + rect[3] + margin];
        let inner = finished_at(
            &film(size, cut(&pixels, full[0], origin, size)),
            &finish,
            false,
            origin,
            full,
        )
        .unwrap();
        let at = [rect[0] - origin[0], rect[1] - origin[1]];
        let got = cut(&inner, size[0], at, [rect[2], rect[3]]);
        let want = cut(&whole, full[0], [rect[0], rect[1]], [rect[2], rect[3]]);
        for (got, want) in got.iter().zip(&want) {
            for k in 0..3 {
                assert!((got[k] - want[k]).abs() < 1e-5, "{got:?} {want:?}");
            }
        }
        let bare = cut(&pixels, full[0], [rect[0], rect[1]], [rect[2], rect[3]]);
        let unframed = finished(&film([rect[2], rect[3]], bare), &finish, false).unwrap();
        assert!(
            unframed
                .iter()
                .zip(&want)
                .any(|(a, b)| (a[0] - b[0]).abs() > 1e-3)
        );
    }

    fn standard_finish(exposure: f64) -> Chain {
        let text = format!(
            "manfred = 1\nname = \"t\"\n[look]\nview = \"standard\"\nexposure = {exposure:?}\n"
        );
        let recipe = manfred_recipe::parse(&text).unwrap();
        manfred_scene::Lighting::of(&recipe).unwrap().finish
    }

    fn levels(finish: &Chain, linear: [f32; 4]) -> Vec<u8> {
        let film = Film {
            size: [4, 1],
            color: linear.map(|v| [v, v, v, 1.0]).to_vec(),
            albedo: vec![[0.5, 0.5, 0.5, 1.0]; 4],
            normal: vec![[0.0, 0.0, 1.0, 1.0]; 4],
            denoised: None,
            samples: 1,
        };
        looked_with(&film, finish, false)
            .unwrap()
            .chunks_exact(4)
            .map(|p| {
                assert_eq!(p[0], p[1]);
                assert_eq!(p[3], 255);
                p[0]
            })
            .collect()
    }

    #[test]
    fn the_standard_finish_is_exposure_clamp_and_the_srgb_curve() {
        let finish = standard_finish(1.0);
        assert_eq!(finish.passes.len(), 1);
        assert!(matches!(finish.passes[0], Pass::Exposure(e) if e == 1.0));
        assert_eq!(levels(&finish, [0.0, 0.5, 1.0, 2.0]), [0, 188, 255, 255]);
        assert_eq!(levels(&finish, [0.25, 0.25, 0.25, 0.25]), [137; 4]);
        let doubled = standard_finish(2.0);
        assert_eq!(levels(&doubled, [0.0, 0.25, 0.5, 1.0]), [0, 188, 255, 255]);
        let halved = standard_finish(0.5);
        assert_eq!(levels(&halved, [0.0, 1.0, 2.0, 4.0]), [0, 188, 255, 255]);
    }

    #[test]
    fn the_standard_finish_leaves_the_graded_tone_curve_out() {
        let graded = graded_finish();
        let samples = [0.0, 0.5, 1.0, 2.0];
        assert_ne!(
            levels(&graded, samples),
            levels(&standard_finish(1.0), samples)
        );
    }

    #[test]
    fn a_film_goes_through_raw_when_the_denoiser_is_off() {
        let film = Film {
            size: [2, 1],
            color: vec![[0.5, 0.5, 0.5, 1.0], [0.0, 0.0, 0.0, 1.0]],
            albedo: vec![[0.5, 0.5, 0.5, 1.0]; 2],
            normal: vec![[0.0, 0.0, 1.0, 1.0]; 2],
            denoised: None,
            samples: 4,
        };
        let raw = looked_with(&film, &Chain::new(), false).unwrap();
        assert_eq!(raw, [188, 188, 188, 255, 0, 0, 0, 255]);
        let path = scratch("raw.png");
        assert_eq!(
            write_with(&path, &film, &Chain::new(), false).unwrap(),
            Format::Png
        );
    }
}
