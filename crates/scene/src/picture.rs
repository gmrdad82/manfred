use crate::Error;
use std::path::Path;

const AMPLIFY: u32 = 8;

pub fn write_png(path: &Path, size: [u32; 2], rgba: &[u8]) -> Result<(), Error> {
    if rgba.len() != (size[0] as usize) * (size[1] as usize) * 4 {
        return Err(Error(format!(
            "{}: {} bytes do not make a {}x{} RGBA image",
            path.display(),
            rgba.len(),
            size[0],
            size[1]
        )));
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| Error(format!("{}: {e}", parent.display())))?;
    }
    let file =
        std::fs::File::create(path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), size[0], size[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder
        .write_header()
        .map_err(|e| Error(format!("{}: {e}", path.display())))?;
    writer
        .write_image_data(rgba)
        .map_err(|e| Error(format!("{}: {e}", path.display())))?;
    writer
        .finish()
        .map_err(|e| Error(format!("{}: {e}", path.display())))
}

pub fn read_png(path: &Path) -> Result<([u32; 2], Vec<u8>), Error> {
    let failed = |e: &dyn std::fmt::Display| Error(format!("{}: {e}", path.display()));
    let file = std::fs::File::open(path).map_err(|e| failed(&e))?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .map_err(|e| failed(&e))?;
    let mut buffer = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or_else(|| failed(&"too large"))?
    ];
    let info = reader.next_frame(&mut buffer).map_err(|e| failed(&e))?;
    if (info.color_type, info.bit_depth) != (png::ColorType::Rgba, png::BitDepth::Eight) {
        return Err(failed(&"not 8-bit RGBA"));
    }
    reader.finish().map_err(|e| failed(&e))?;
    buffer.truncate(info.buffer_size());
    Ok(([info.width, info.height], buffer))
}

pub fn psnr(a: &[u8], b: &[u8]) -> Result<f64, Error> {
    if a.len() != b.len() || !a.len().is_multiple_of(4) {
        return Err(Error(format!(
            "the images differ in size: {} and {} bytes",
            a.len(),
            b.len()
        )));
    }
    let mut total = 0u64;
    let mut count = 0u64;
    for (x, y) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        for k in 0..3 {
            let d = i64::from(x[k]) - i64::from(y[k]);
            total += (d * d) as u64;
        }
        count += 3;
    }
    if total == 0 || count == 0 {
        return Ok(f64::INFINITY);
    }
    let mse = total as f64 / count as f64;
    Ok(10.0 * (255.0 * 255.0 / mse).log10())
}

pub fn difference(a: &[u8], b: &[u8]) -> Vec<u8> {
    a.chunks_exact(4)
        .zip(b.chunks_exact(4))
        .flat_map(|(x, y)| {
            let channel = |k: usize| (u32::from(x[k].abs_diff(y[k])) * AMPLIFY).min(255) as u8;
            [channel(0), channel(1), channel(2), 255]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn psnr_is_infinite_for_equal_images_and_falls_with_error() {
        let a = vec![10u8, 20, 30, 255, 40, 50, 60, 255];
        assert_eq!(psnr(&a, &a).unwrap(), f64::INFINITY);
        let mut b = a.clone();
        b[0] = 11;
        let one = psnr(&a, &b).unwrap();
        let expected = 10.0 * (255.0f64 * 255.0 / (1.0 / 6.0)).log10();
        assert!((one - expected).abs() < 1e-9);
        b[1] = 30;
        assert!(psnr(&a, &b).unwrap() < one);
        let mut alpha = a.clone();
        alpha[3] = 0;
        assert_eq!(psnr(&a, &alpha).unwrap(), f64::INFINITY);
        assert!(psnr(&a, &a[..4]).is_err());
    }

    #[test]
    fn the_difference_is_amplified_and_opaque() {
        let a = [0u8, 100, 200, 0];
        let b = [3u8, 90, 200, 0];
        assert_eq!(difference(&a, &b), vec![24, 80, 0, 255]);
    }

    #[test]
    fn a_png_round_trips() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp/scene");
        let path = dir.join("two.png");
        let rgba = [255u8, 0, 0, 255, 0, 0, 255, 255];
        write_png(&path, [2, 1], &rgba).unwrap();
        let decoder =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buffer).unwrap();
        assert_eq!((info.width, info.height), (2, 1));
        assert_eq!(&buffer[..info.buffer_size()], &rgba);
        assert!(write_png(&path, [3, 1], &rgba).is_err());
        assert_eq!(read_png(&path).unwrap(), ([2, 1], rgba.to_vec()));
        let bytes = std::fs::read(&path).unwrap();
        let cut = dir.join("cut.png");
        std::fs::write(&cut, &bytes[..bytes.len() - 12]).unwrap();
        assert!(read_png(&cut).is_err());
        assert!(read_png(&dir.join("missing.png")).is_err());
    }
}
