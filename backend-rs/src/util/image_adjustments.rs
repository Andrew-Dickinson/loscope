use image::{DynamicImage, RgbaImage};
use photon_rs::PhotonImage;
use photon_rs::{channels, colour_spaces, effects};

/// Apply a fixed set of photo adjustments to an ortho image before serving it.
///
/// Adjustments (in order):
/// - Exposure    : +10 brightness
/// - Contrast    : +20
/// - Highlights  : lighten HSL by 0.18
/// - Saturation  : saturate HSL by 0.15
/// - Temperature : +15 red, -10 blue (warmer)
pub fn apply_photo_adjustments(img: DynamicImage) -> DynamicImage {
    // photon_rs operates on raw RGBA bytes, so convert via RgbaImage.
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let raw_pixels = rgba.into_raw();

    let mut photon_img = PhotonImage::new(raw_pixels, width, height);

    effects::inc_brightness(&mut photon_img, 10);
    effects::adjust_contrast(&mut photon_img, 20.0);
    colour_spaces::lighten_hsl(&mut photon_img, 0.0);
    colour_spaces::saturate_hsl(&mut photon_img, 0.15);
    channels::alter_red_channel(&mut photon_img, 15);
    channels::alter_blue_channel(&mut photon_img, -10);

    // Rebuild a DynamicImage from the processed RGBA bytes.
    let processed = photon_img.get_raw_pixels();
    DynamicImage::ImageRgba8(
        RgbaImage::from_raw(width, height, processed)
            .expect("photon_rs output dimensions must match input"),
    )
}
