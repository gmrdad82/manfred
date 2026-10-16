use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const ICO_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
const ICNS_CHUNKS: [(&[u8; 4], u32); 9] = [
    (b"ic04", 16),
    (b"ic05", 32),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic09", 512),
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic13", 256),
    (b"ic14", 512),
];
const USAGE: &str = "usage: icons [--from <icons dir>] [--ico <file.ico>] [--icns <file.icns>]";

fn load(dir: &Path, size: u32) -> Result<Vec<u8>, String> {
    let path = dir.join(format!("manfred-{size}.png"));
    let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (width, height) = png_size(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    if width != size || height != size {
        return Err(format!(
            "{}: is {width}x{height}, not {size}x{size}",
            path.display()
        ));
    }
    Ok(bytes)
}

fn png_size(bytes: &[u8]) -> Result<(u32, u32), String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let reader = decoder.read_info().map_err(|e| e.to_string())?;
    let info = reader.info();
    Ok((info.width, info.height))
}

fn pack_ico(dir: &Path) -> Result<Vec<u8>, String> {
    let images = ICO_SIZES
        .iter()
        .map(|&size| load(dir, size).map(|bytes| (size, bytes)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, bytes) in &images {
        let side = if *size >= 256 { 0 } else { *size as u8 };
        out.push(side);
        out.push(side);
        out.push(0);
        out.push(0);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += bytes.len() as u32;
    }
    for (_, bytes) in &images {
        out.extend_from_slice(bytes);
    }
    Ok(out)
}

fn pack_icns(dir: &Path) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    for (kind, size) in ICNS_CHUNKS {
        let bytes = load(dir, size)?;
        body.extend_from_slice(kind);
        body.extend_from_slice(&(bytes.len() as u32 + 8).to_be_bytes());
        body.extend_from_slice(&bytes);
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"icns");
    out.extend_from_slice(&(body.len() as u32 + 8).to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn run() -> Result<(), String> {
    let mut from = PathBuf::from("assets/marks/icons");
    let mut ico = PathBuf::from("packaging/windows/manfred.ico");
    let mut icns = PathBuf::from("packaging/macos/manfred.icns");
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let slot = match flag.as_str() {
            "--from" => &mut from,
            "--ico" => &mut ico,
            "--icns" => &mut icns,
            _ => return Err(USAGE.to_string()),
        };
        *slot = PathBuf::from(args.next().ok_or_else(|| USAGE.to_string())?);
    }
    write(&ico, &pack_ico(&from)?)?;
    write(&icns, &pack_icns(&from)?)?;
    println!("icons: wrote {} and {}", ico.display(), icns.display());
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("icons: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn u16le(b: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([b[at], b[at + 1]])
    }

    fn u32le(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }

    fn u32be(b: &[u8], at: usize) -> u32 {
        u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }

    fn decodes(bytes: &[u8], size: u32) {
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .expect("valid png header");
        let mut buf = vec![0; reader.output_buffer_size().expect("sized")];
        let frame = reader.next_frame(&mut buf).expect("valid png data");
        assert_eq!((frame.width, frame.height), (size, size));
    }

    #[test]
    fn ico_directory_and_payloads_read_back() {
        let ico = fs::read(root().join("packaging/windows/manfred.ico")).expect("committed ico");
        assert_eq!(u16le(&ico, 0), 0);
        assert_eq!(u16le(&ico, 2), 1);
        let count = u16le(&ico, 4) as usize;
        assert_eq!(count, ICO_SIZES.len());
        for (i, size) in ICO_SIZES.iter().enumerate() {
            let at = 6 + 16 * i;
            let side = if *size == 256 { 0 } else { *size as u8 };
            assert_eq!((ico[at], ico[at + 1]), (side, side));
            let len = u32le(&ico, at + 8) as usize;
            let offset = u32le(&ico, at + 12) as usize;
            decodes(&ico[offset..offset + len], *size);
        }
        let last = 6 + 16 * (count - 1);
        assert_eq!(
            u32le(&ico, last + 8) as usize + u32le(&ico, last + 12) as usize,
            ico.len()
        );
    }

    #[test]
    fn icns_chunks_and_payloads_read_back() {
        let icns = fs::read(root().join("packaging/macos/manfred.icns")).expect("committed icns");
        assert_eq!(&icns[0..4], b"icns");
        assert_eq!(u32be(&icns, 4) as usize, icns.len());
        let mut at = 8;
        let mut seen = Vec::new();
        while at < icns.len() {
            let kind = &icns[at..at + 4];
            let len = u32be(&icns, at + 4) as usize;
            let (_, size) = ICNS_CHUNKS
                .iter()
                .find(|(k, _)| k.as_slice() == kind)
                .expect("known chunk");
            decodes(&icns[at + 8..at + len], *size);
            seen.push(kind.to_vec());
            at += len;
        }
        assert_eq!(at, icns.len());
        let want: Vec<Vec<u8>> = ICNS_CHUNKS.iter().map(|(k, _)| k.to_vec()).collect();
        assert_eq!(seen, want);
    }

    #[test]
    fn packing_is_reproducible() {
        let dir = root().join("assets/marks/icons");
        let ico = fs::read(root().join("packaging/windows/manfred.ico")).expect("ico");
        let icns = fs::read(root().join("packaging/macos/manfred.icns")).expect("icns");
        assert_eq!(pack_ico(&dir).expect("ico"), ico);
        assert_eq!(pack_icns(&dir).expect("icns"), icns);
    }
}
