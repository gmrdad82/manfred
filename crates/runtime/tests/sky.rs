use manfred_runtime::sky;

const WIDTH: usize = 128;

fn header() -> Vec<u8> {
    format!("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y 1 +X {WIDTH}\n").into_bytes()
}

fn pixel(x: usize) -> [u8; 4] {
    [10 + x as u8, 200 - x as u8, 64, 128 + (x % 3) as u8]
}

#[test]
fn an_rle_row_with_a_full_128_literal_decodes() {
    let mut rle = header();
    rle.extend_from_slice(&[2, 2, 0, WIDTH as u8]);
    for channel in 0..4 {
        rle.push(128);
        rle.extend((0..WIDTH).map(|x| pixel(x)[channel]));
    }
    let mut flat = header();
    flat.extend((0..WIDTH).flat_map(pixel));
    let decoded = sky::decode(&rle).unwrap();
    assert_eq!((decoded.width, decoded.height), (WIDTH as u32, 1));
    assert_eq!(decoded, sky::decode(&flat).unwrap());
    assert_ne!(decoded.texels[0], decoded.texels[WIDTH - 1]);
}

#[test]
fn a_zero_count_is_the_engines_own_error() {
    let mut rle = header();
    rle.extend_from_slice(&[2, 2, 0, WIDTH as u8, 0]);
    assert_eq!(sky::decode(&rle).unwrap_err(), "bad hdr run");
}
