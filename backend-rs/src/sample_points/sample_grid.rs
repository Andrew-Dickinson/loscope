use crate::building::heightmap::RooftopHeightMap;
use crate::sample_points::point::SamplePoint;


///     Given a heightmap representing a rooftop, generate points which are roughly evenly spaced over the rooftop based
///     on sample_spacing, with extra points at areas of large height change and around the perimeter. For each sample
///     point, we provide a "display" location as well as a "measurement" location which is usually offset upwards
///     by mast_offset
pub fn sample_points_for_rooftop(
    rooftop_height_map: &RooftopHeightMap,
    mast_offset_ft: f64,
    sample_spacing: f64
) -> Vec<SamplePoint> {
    todo!()
}