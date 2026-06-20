use image::{DynamicImage, RgbaImage};
use photon_rs::PhotonImage;
use photon_rs::{channels, colour_spaces, effects};
use crate::providers::terrain_classification_tile_provider::TerrainClassificationTile;
use crate::types::tiles::TerrainClass;

const WATER_OVERRIDE_COLOR: ColorOverride = &[56, 95, 237, 255];
const VEG_OVERRIDE_COLOR: ColorOverride = &[28, 127, 44, 255];

const TILE_TO_ORTHO_SCALE_FACTOR_EACH_AXIS: usize = 2;
const OVERRIDE_BLEND: f32 = 0.5;

type ColorOverride = &'static[u8; 4];

impl TryFrom<&TerrainClass> for ColorOverride {

    type Error = ();

    fn try_from(value: &TerrainClass) -> Result<Self, Self::Error> {
        match value {
            TerrainClass::Vegetation => Ok(VEG_OVERRIDE_COLOR),
            TerrainClass::Water => Ok(WATER_OVERRIDE_COLOR),
            _ => Err(())
        }
    }
}

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

pub fn colorize_from_classifications(img: DynamicImage, tile: TerrainClassificationTile) -> DynamicImage {
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut raw_pixels = rgba.into_raw();

    assert_eq!((tile.terrain_class().shape()[0] * TILE_TO_ORTHO_SCALE_FACTOR_EACH_AXIS) as u32, width);
    assert_eq!((tile.terrain_class().shape()[1] * TILE_TO_ORTHO_SCALE_FACTOR_EACH_AXIS) as u32, height);

    let ortho_width = width as usize;
    // terrain_class axes are [easting, northing]: axis 0 → image x, axis 1 → image y.
    // Northing increases northward but image y increases downward, so northing must be inverted.
    let northing_max = tile.terrain_class().shape()[1] - 1;
    for ((easting, northing), classification) in tile.terrain_class().indexed_iter() {
        if let Ok(color) = ColorOverride::try_from(classification) {
            for de in 0..TILE_TO_ORTHO_SCALE_FACTOR_EACH_AXIS {
                for dn in 0..TILE_TO_ORTHO_SCALE_FACTOR_EACH_AXIS {
                    let ortho_x = easting * TILE_TO_ORTHO_SCALE_FACTOR_EACH_AXIS + de;
                    let ortho_y = (northing_max - northing) * TILE_TO_ORTHO_SCALE_FACTOR_EACH_AXIS + dn;
                    let byte_offset = (ortho_y * ortho_width + ortho_x) * 4;
                    let px = &mut raw_pixels[byte_offset..byte_offset + 4];
                    for i in 0..3 {
                        px[i] = (color[i] as f32 * OVERRIDE_BLEND + px[i] as f32 * (1.0 - OVERRIDE_BLEND)) as u8;
                    }
                }
            }
        }
    }

    // Rebuild a DynamicImage from the processed RGBA bytes.
    DynamicImage::ImageRgba8(
        RgbaImage::from_raw(width, height, raw_pixels)
            .expect("raw_pixels must never have its length changed"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;
    use ndarray::Array2;
    use crate::providers::terrain_classification_tile_provider::TerrainClassificationTile;
    use crate::types::tiles::{TerrainClass, TileId};

    const TILE_SIDE: usize = 500;
    const ORTHO_SIDE: u32 = (TILE_SIDE * TILE_TO_ORTHO_SCALE_FACTOR_EACH_AXIS) as u32;

    fn sample_tile_id() -> TileId {
        TileId::parse("2235_12").unwrap()
    }

    fn solid_ortho(color: [u8; 4]) -> DynamicImage {
        let pixels: Vec<u8> = color.iter()
            .cloned()
            .cycle()
            .take((ORTHO_SIDE * ORTHO_SIDE * 4) as usize)
            .collect();
        DynamicImage::ImageRgba8(RgbaImage::from_raw(ORTHO_SIDE, ORTHO_SIDE, pixels).unwrap())
    }

    fn uniform_tile(class: TerrainClass) -> TerrainClassificationTile {
        TerrainClassificationTile::new(
            sample_tile_id(),
            Array2::from_elem((TILE_SIDE, TILE_SIDE), class),
        )
    }

    fn pixel_at(img: &DynamicImage, x: u32, y: u32) -> [u8; 4] {
        img.to_rgba8().get_pixel(x, y).0
    }

    fn blend_with_white(color: ColorOverride) -> [u8; 4] {
        let mut out = [255u8; 4];
        for i in 0..3 {
            out[i] = (color[i] as f32 * OVERRIDE_BLEND + 255.0 * (1.0 - OVERRIDE_BLEND)) as u8;
        }
        out
    }

    // --- ColorOverride TryFrom ---

    #[test]
    fn color_override_none_is_err() {
        assert!(ColorOverride::try_from(&TerrainClass::None).is_err());
    }

    #[test]
    fn color_override_building_is_err() {
        assert!(ColorOverride::try_from(&TerrainClass::Building).is_err());
    }

    #[test]
    fn color_override_vegetation_returns_veg_color() {
        assert_eq!(ColorOverride::try_from(&TerrainClass::Vegetation).unwrap(), VEG_OVERRIDE_COLOR);
    }

    #[test]
    fn color_override_water_returns_water_color() {
        assert_eq!(ColorOverride::try_from(&TerrainClass::Water).unwrap(), WATER_OVERRIDE_COLOR);
    }

    // --- colorize_from_classifications ---

    #[test]
    fn colorize_none_leaves_image_unchanged() {
        let white = [255u8, 255, 255, 255];
        let result = colorize_from_classifications(solid_ortho(white), uniform_tile(TerrainClass::None));
        assert_eq!(pixel_at(&result, 0, 0), white);
        assert_eq!(pixel_at(&result, ORTHO_SIDE - 1, ORTHO_SIDE - 1), white);
    }

    #[test]
    fn colorize_vegetation_blends_all_pixels() {
        let white = [255u8, 255, 255, 255];
        let expected = blend_with_white(VEG_OVERRIDE_COLOR);
        let result = colorize_from_classifications(solid_ortho(white), uniform_tile(TerrainClass::Vegetation));
        assert_eq!(pixel_at(&result, 0, 0), expected);
        assert_eq!(pixel_at(&result, ORTHO_SIDE - 1, ORTHO_SIDE - 1), expected);
        assert_eq!(pixel_at(&result, ORTHO_SIDE / 2, ORTHO_SIDE / 2), expected);
    }

    #[test]
    fn colorize_water_blends_all_pixels() {
        let white = [255u8, 255, 255, 255];
        let expected = blend_with_white(WATER_OVERRIDE_COLOR);
        let result = colorize_from_classifications(solid_ortho(white), uniform_tile(TerrainClass::Water));
        assert_eq!(pixel_at(&result, 0, 0), expected);
        assert_eq!(pixel_at(&result, ORTHO_SIDE - 1, ORTHO_SIDE - 1), expected);
    }

    #[test]
    fn colorize_single_vegetation_pixel_maps_to_correct_2x2_ortho_block() {
        // terrain axes are [easting, northing].
        // terrain[(1, TILE_SIDE-1)] = easting=1, northing=max (northernmost row).
        // Northing is inverted onto image y, so northing_max → ortho y=0 (top).
        // Expected: ortho x ∈ {2,3}, y ∈ {0,1}.
        let white = [255u8, 255, 255, 255];
        let mut terrain = Array2::from_elem((TILE_SIDE, TILE_SIDE), TerrainClass::None);
        terrain[(1, TILE_SIDE - 1)] = TerrainClass::Vegetation; // easting=1, northing=max
        let tile = TerrainClassificationTile::new(sample_tile_id(), terrain);
        let result = colorize_from_classifications(solid_ortho(white), tile);

        let veg = blend_with_white(VEG_OVERRIDE_COLOR);
        // Correct 2×2 block: x ∈ {2,3}, y ∈ {0,1} (top of image = northernmost).
        assert_eq!(pixel_at(&result, 2, 0), veg);
        assert_eq!(pixel_at(&result, 3, 0), veg);
        assert_eq!(pixel_at(&result, 2, 1), veg);
        assert_eq!(pixel_at(&result, 3, 1), veg);
        // If northing weren't inverted, these bottom pixels would be lit instead.
        assert_eq!(pixel_at(&result, 2, ORTHO_SIDE - 1), white);
        assert_eq!(pixel_at(&result, 2, ORTHO_SIDE - 2), white);
        // If axes were swapped, these would be lit instead.
        assert_eq!(pixel_at(&result, 0, 2), white);
        assert_eq!(pixel_at(&result, 4, 0), white);
    }
}
