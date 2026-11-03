// codeks 0014: preserve pixels and supported metadata while reducing PNG payloads.
use std::io::Cursor;
use std::path::Path;

use image::DynamicImage;
use image::ImageError;
use image::ImageFormat;
use image::error::DecodingError;

use super::ImageMetadata;
use super::ImageProcessingError;
use super::encode_image;

pub(super) fn prefer_smaller(
    path: &Path,
    image: &DynamicImage,
    original: Vec<u8>,
    metadata: ImageMetadata,
) -> Result<(Vec<u8>, ImageFormat), ImageProcessingError> {
    let reader = png::Decoder::new(Cursor::new(&original))
        .read_info()
        .map_err(|source| {
            ImageProcessingError::decode_error(
                path,
                ImageError::Decoding(DecodingError::new(ImageFormat::Png.into(), source)),
            )
        })?;
    let info = reader.info();
    // WebP stores 8-bit pixels with dimensions up to 16384. Keep animation and
    // color metadata that the shared encoder cannot carry without conversion.
    if info.bit_depth == png::BitDepth::Sixteen
        || info.width > 16384
        || info.height > 16384
        || info.animation_control.is_some()
        || (info.icc_profile.is_some() && metadata.icc_profile.is_none())
        || (info.icc_profile.is_none()
            && info.srgb.is_none()
            && (info.source_gamma.is_some() || info.source_chromaticities.is_some()))
    {
        return Ok((original, ImageFormat::Png));
    }
    let (webp, format) = encode_image(image, ImageFormat::WebP, metadata)?;
    if webp.len() < original.len() {
        Ok((webp, format))
    } else {
        Ok((original, ImageFormat::Png))
    }
}
