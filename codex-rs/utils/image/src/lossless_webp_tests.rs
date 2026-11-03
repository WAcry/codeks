// codeks 0014: validate the shipped encoder, byte selection, and lossless boundaries.
use super::*;

#[test]
fn screenshot_conversion_preserves_every_rgba_pixel() {
    let pixels = ImageBuffer::from_fn(1280, 720, |x, y| {
        if x < 240 {
            Rgba([32u8, 40, 52, 255])
        } else if x % 19 < 13 && y % 31 < 3 {
            Rgba([64, 96, 128, 77])
        } else {
            Rgba([240, 248, 252, 0])
        }
    });
    let original = image_bytes(&pixels, ImageFormat::Png);
    let processed = load_for_prompt_bytes_uncached(
        Path::new("screenshot.png"),
        original.clone(),
        PromptImageMode::Original,
    )
    .expect("process screenshot");

    assert_eq!(processed.mime, "image/webp");
    assert!(processed.bytes.len() < original.len());
    assert_eq!(
        image::load_from_memory(&processed.bytes)
            .expect("decode WebP")
            .to_rgba8(),
        pixels
    );
}

#[test]
fn keeps_compact_monochrome_png_when_webp_is_larger() {
    let mut seed = 1u32;
    let data = (0..128 * 128 / 8)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as u8
        })
        .collect::<Vec<_>>();
    let mut original = Vec::new();
    let mut encoder = png::Encoder::new(&mut original, 128, 128);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::One);
    encoder
        .write_header()
        .expect("write PNG header")
        .write_image_data(&data)
        .expect("write monochrome PNG");
    assert_keeps_png(original);
}

#[test]
fn conversion_without_resizing_keeps_rgb_profile_and_orientation() {
    let pixels = ImageBuffer::from_pixel(512, 256, Rgba([32u8, 64, 96, 255]));
    let original = image_bytes_with_metadata(&pixels, ImageFormat::Png, TEST_RGB_ICC_PROFILE);
    let processed = load_for_prompt_bytes_uncached(
        Path::new("metadata.png"),
        original,
        PromptImageMode::Original,
    )
    .expect("convert PNG with metadata");
    assert_eq!(processed.mime, "image/webp");
    let mut decoder = ImageReader::with_format(Cursor::new(&processed.bytes), ImageFormat::WebP)
        .into_decoder()
        .expect("decode WebP metadata");
    assert_eq!(
        decoder.icc_profile().unwrap(),
        Some(TEST_RGB_ICC_PROFILE.to_vec())
    );
    assert_eq!(
        decoder.exif_metadata().unwrap(),
        Some(ROTATE_90_EXIF.to_vec())
    );
    assert_eq!(decoder.orientation().unwrap(), Orientation::Rotate90);
}

#[test]
fn keeps_16_bit_png_without_quantizing() {
    let pixels = ImageBuffer::from_pixel(64, 32, Rgba([12345u16, 23456, 34567, 45678]));
    let mut original = Cursor::new(Vec::new());
    DynamicImage::ImageRgba16(pixels)
        .write_to(&mut original, ImageFormat::Png)
        .expect("encode 16-bit PNG");
    assert_keeps_png(original.into_inner());
}

#[test]
fn keeps_png_animation() {
    let mut original = Vec::new();
    let mut encoder = png::Encoder::new(&mut original, 1, 1);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_animated(2, 0).expect("set PNG animation");
    let mut writer = encoder.write_header().expect("write APNG header");
    writer
        .write_image_data(&[1, 2, 3, 255])
        .expect("write first frame");
    writer
        .write_image_data(&[4, 5, 6, 255])
        .expect("write second frame");
    writer.finish().expect("finish APNG");
    assert_keeps_png(original);
}

#[test]
fn keeps_png_color_metadata_that_cannot_be_carried_to_webp() {
    let pixels = ImageBuffer::from_pixel(64, 32, Rgba([32u8, 64, 96, 255]));
    assert_keeps_png(image_bytes_with_metadata(
        &pixels,
        ImageFormat::Png,
        TEST_CMYK_ICC_PROFILE,
    ));

    let mut original = Vec::new();
    let mut encoder = png::Encoder::new(&mut original, 64, 32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_gamma(png::ScaledFloat::new(0.6));
    encoder
        .write_header()
        .expect("write PNG header")
        .write_image_data(pixels.as_raw())
        .expect("write PNG with gamma");
    assert_keeps_png(original);
}

#[test]
fn keeps_original_png_beyond_webp_dimensions() {
    let pixels = ImageBuffer::from_pixel(16385, 1, Rgba([32u8, 64, 96, 255]));
    assert_keeps_png(image_bytes(&pixels, ImageFormat::Png));
}

fn assert_keeps_png(original: Vec<u8>) {
    let processed = load_for_prompt_bytes_uncached(
        Path::new("original.png"),
        original.clone(),
        PromptImageMode::Original,
    )
    .expect("process PNG");
    assert_eq!(processed.mime, "image/png");
    assert_eq!(processed.bytes.as_ref(), original);
}
