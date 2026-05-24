use std::collections::HashMap;
use geo::{Euclidean, InterpolatableLine, Length};
use crate::building::heightmap::RooftopHeightMap;
use crate::sample_points::point::SamplePoint;
use crate::types::coords::NYSCoords3;

// Sample points on a regular grid, centred in each spacing×spacing cell.
// Returns (base_pts, cliff_pts). Cliff points share (easting, northing) with a base point
// but sit at intermediate heights where a neighboring cell is significantly taller.
fn sample_grid(hm: &RooftopHeightMap, spacing: usize) -> (Vec<NYSCoords3>, Vec<NYSCoords3>) {
    assert!(spacing > 0);

    let heightmap = hm.heightmap();
    let mask = hm.mask();
    let (w, h) = heightmap.dim();  // (easting_extent, northing_extent)
    let sw_e = *hm.sw_offset().easting();
    let sw_n = *hm.sw_offset().northing();

    let cliff_trigger_in = (spacing * 12) as f64;
    let cliff_step_in = (spacing * 6) as f64;

    let half = spacing / 2;
    let mut base_pts = Vec::new();
    let mut cliff_pts = Vec::new();

    for ei in (half..w).step_by(spacing) {
        for ni in (half..h).step_by(spacing) {
            if !mask[[ei, ni]] {
                continue;
            }

            let h_in = heightmap[[ei, ni]] as f64;
            let e = sw_e + ei as f64 + 0.5;
            let n = sw_n + ni as f64 + 0.5;
            base_pts.push(NYSCoords3::new(e, n, h_in / 12.0));

            // Find the tallest 4-connected neighbor on the sample grid
            let max_nbr_h = [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .filter_map(|(de, dn)| {
                    let ne = ei as i64 + de * spacing as i64;
                    let nn = ni as i64 + dn * spacing as i64;
                    if ne >= 0 && ne < w as i64 && nn >= 0 && nn < h as i64 {
                        Some(heightmap[[ne as usize, nn as usize]] as f64)
                    } else {
                        None
                    }
                })
                .fold(f64::NEG_INFINITY, f64::max);

            if max_nbr_h - h_in > cliff_trigger_in {
                let n_extra = ((max_nbr_h - h_in) / cliff_step_in) as usize;
                for k in 1..=n_extra + 1 {
                    cliff_pts.push(NYSCoords3::new(e, n, (h_in + k as f64 * cliff_step_in) / 12.0));
                }
            }
        }
    }

    (base_pts, cliff_pts)
}

// Sample points at spacing-foot intervals along the polygon exterior and any interior rings.
// Z is looked up from the heightmap. If a point lands outside the mask, the nearest
// valid ±1-pixel neighbour is tried; points with no valid neighbour are skipped.
fn sample_perimeter(hm: &RooftopHeightMap, spacing: f64) -> Vec<NYSCoords3> {
    assert!(spacing > 0.0);

    let heightmap = hm.heightmap();
    let mask = hm.mask();
    let (w, h) = heightmap.dim();
    let sw_e = *hm.sw_offset().easting();
    let sw_n = *hm.sw_offset().northing();
    let poly = hm.poly_nys();

    let rings: Vec<_> = std::iter::once(poly.exterior())
        .chain(poly.interiors())
        .collect();

    let mut pts = Vec::new();

    for ring in rings {
        let length = Euclidean.length(ring);
        if length == 0.0 {
            continue;
        }

        let mut d = 0.0_f64;
        while d < length {
            let Some(pt) = ring.point_at_ratio_from_start(&Euclidean, d / length) else {
                d += spacing;
                continue;
            };

            let x = pt.x();
            let y = pt.y();

            let mut ei = ((x - sw_e).floor() as i64).clamp(0, w as i64 - 1) as usize;
            let mut ni = ((y - sw_n).floor() as i64).clamp(0, h as i64 - 1) as usize;

            if !mask[[ei, ni]] {
                'search: for dei in -1i64..=1 {
                    for dni in -1i64..=1 {
                        if dei == 0 && dni == 0 {
                            continue;
                        }
                        let ne = ei as i64 + dei;
                        let nn = ni as i64 + dni;
                        if ne >= 0 && ne < w as i64 && nn >= 0 && nn < h as i64
                            && mask[[ne as usize, nn as usize]]
                        {
                            ei = ne as usize;
                            ni = nn as usize;
                            break 'search;
                        }
                    }
                }
                if !mask[[ei, ni]] {
                    d += spacing;
                    continue;
                }
            }

            pts.push(NYSCoords3::new(x, y, heightmap[[ei, ni]] as f64 / 12.0));
            d += spacing;
        }
    }

    pts
}

// Combine grid and perimeter points. Base and cliff points whose XY is within
// cull_radius of any perimeter point are removed, then the result is
// perimeter + surviving base + surviving cliff.
fn cull_and_combine(
    base: Vec<NYSCoords3>,
    cliff: Vec<NYSCoords3>,
    perim: Vec<NYSCoords3>,
    cull_radius: f64,
) -> Vec<NYSCoords3> {
    let r_sq = cull_radius * cull_radius;

    let (surviving_base, surviving_cliff) = {
        let near_perim = |pt: &NYSCoords3| {
            perim.iter().any(|p| {
                let de = pt.easting() - p.easting();
                let dn = pt.northing() - p.northing();
                de * de + dn * dn < r_sq
            })
        };
        (
            base.into_iter().filter(|pt| !near_perim(pt)).collect::<Vec<_>>(),
            cliff.into_iter().filter(|pt| !near_perim(pt)).collect::<Vec<_>>(),
        )
    };

    perim.into_iter()
        .chain(surviving_base)
        .chain(surviving_cliff)
        .collect()
}

// Split points into display and measurement positions. For each unique (easting, northing),
// only the highest point has its measurement position shifted up by offset.
fn apply_mast_offset(pts: Vec<NYSCoords3>, offset: f64) -> (Vec<NYSCoords3>, Vec<NYSCoords3>) {
    let mut measurement_pts = pts;
    let display_pts = measurement_pts.clone();

    if offset != 0.0 && !measurement_pts.is_empty() {
        let mut top_idx: HashMap<(u64, u64), usize> = HashMap::new();
        for (i, pt) in measurement_pts.iter().enumerate() {
            let key = (pt.easting().to_bits(), pt.northing().to_bits());
            top_idx
                .entry(key)
                .and_modify(|best| {
                    if measurement_pts[i].alt_usft() > measurement_pts[*best].alt_usft() {
                        *best = i;
                    }
                })
                .or_insert(i);
        }
        for idx in top_idx.values() {
            let pt = &measurement_pts[*idx];
            measurement_pts[*idx] = NYSCoords3::new(
                *pt.easting(),
                *pt.northing(),
                *pt.alt_usft() + offset,
            );
        }
    }

    (display_pts, measurement_pts)
}

/// Given a heightmap representing a rooftop, generate points which are roughly evenly spaced over the rooftop based
/// on sample_spacing, with extra points at areas of large height change and around the perimeter. For each sample
/// point, we provide a "display" location as well as a "measurement" location which is usually offset upwards
/// by mast_offset
pub fn sample_points_for_rooftop(
    rooftop_height_map: &RooftopHeightMap,
    mast_offset_ft: f64,
    sample_spacing: f64,
) -> Vec<SamplePoint> {
    let spacing_px = sample_spacing.floor() as usize;
    assert!(spacing_px > 0);

    let (base_pts, cliff_pts) = sample_grid(rooftop_height_map, spacing_px);
    let perim_pts = sample_perimeter(rooftop_height_map, sample_spacing);
    let all_pts = cull_and_combine(base_pts, cliff_pts, perim_pts, sample_spacing / 2.0);
    let (display_pts, measurement_pts) = apply_mast_offset(all_pts, mast_offset_ft);

    let sw_nys = NYSCoords3::from2(rooftop_height_map.sw_offset(), 0.0);

    display_pts
        .iter()
        .zip(measurement_pts.iter())
        .map(|(dp, mp)| SamplePoint::new(
            mp.encoded_from_base(&sw_nys),
            dp.encoded_from_base(&sw_nys),
        ))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use geo::polygon;
    use ndarray::Array2;
    use crate::building::bin_id::BINId;
    use crate::building::heightmap::{RooftopHeightMap};
    use crate::types::coords::NYSCoords2;

    const SW_E: f64 = 500000.0;
    const SW_N: f64 = 300000.0;

    fn make_hm(heights: Array2<u16>, mask: Array2<bool>) -> RooftopHeightMap {
        let (w, h) = heights.dim();
        let poly = polygon![
            (x: SW_E,              y: SW_N),
            (x: SW_E + w as f64,  y: SW_N),
            (x: SW_E + w as f64,  y: SW_N + h as f64),
            (x: SW_E,              y: SW_N + h as f64),
            (x: SW_E,              y: SW_N),
        ];
        RooftopHeightMap::new(
            BINId::parse("1000001").unwrap(),
            NYSCoords2::new(SW_E, SW_N),
            heights,
            mask,
            poly,
        )
    }

    fn flat_hm(w: usize, h: usize, height_in: u16) -> RooftopHeightMap {
        make_hm(
            Array2::from_elem((w, h), height_in),
            Array2::from_elem((w, h), true),
        )
    }

    fn pt(e: f64, n: f64, z: f64) -> NYSCoords3 { NYSCoords3::new(e, n, z) }

    // --- sample_grid ---

    #[test]
    fn sample_grid_places_points_at_cell_centers() {
        // 20×20 flat heightmap at 120 in (10 ft), spacing = 10
        // Grid pixels: ei=5, ei=15; ni=5, ni=15 → 4 points
        let hm = flat_hm(20, 20, 120);
        let (base, cliff) = sample_grid(&hm, 10);

        assert_eq!(base.len(), 4);
        assert!(cliff.is_empty());

        let eastings: Vec<f64> = base.iter().map(|p| *p.easting()).collect();
        let northings: Vec<f64> = base.iter().map(|p| *p.northing()).collect();
        assert!(eastings.contains(&(SW_E + 5.5)));
        assert!(eastings.contains(&(SW_E + 15.5)));
        assert!(northings.contains(&(SW_N + 5.5)));
        assert!(northings.contains(&(SW_N + 15.5)));

        for p in &base {
            assert_abs_diff_eq!(*p.alt_usft(), 10.0, epsilon = 1e-9);
        }
    }

    #[test]
    fn sample_grid_skips_masked_pixels() {
        let heights = Array2::from_elem((20, 20), 120u16);
        let mut mask = Array2::from_elem((20, 20), true);
        // Mask out the NE quadrant of sample pixels: (ei=15, ni=15)
        for ei in 10..20 {
            for ni in 10..20 {
                mask[[ei, ni]] = false;
            }
        }
        let hm = make_hm(heights, mask);
        let (base, _) = sample_grid(&hm, 10);
        assert_eq!(base.len(), 3);
    }

    #[test]
    fn sample_grid_no_cliffs_for_flat_surface() {
        let hm = flat_hm(20, 20, 240);
        let (_, cliff) = sample_grid(&hm, 10);
        assert!(cliff.is_empty());
    }

    #[test]
    fn sample_grid_cliff_points_for_height_step() {
        // West half at 120 in (10 ft), east half at 360 in (30 ft)
        // spacing=10: cliff_trigger=120 in, cliff_step=60 in
        // West sample (ei=5) has neighbour at (ei=15) with h=360, diff=240 > 120 → cliff
        // n_extra = floor(240/60) = 4, points at k=1..5 → z = 15,20,25,30,35 ft
        let heights = Array2::from_shape_fn((20, 20), |(ei, _ni)| {
            if ei < 10 { 120u16 } else { 360u16 }
        });
        let hm = make_hm(heights, Array2::from_elem((20, 20), true));
        let (_, cliff) = sample_grid(&hm, 10);

        // Two west-side sample points, each generating 5 cliff points
        assert_eq!(cliff.len(), 10);

        let cliff_at_first: Vec<f64> = cliff
            .iter()
            .filter(|p| {
                (*p.easting() - (SW_E + 5.5)).abs() < 1e-9
                    && (*p.northing() - (SW_N + 5.5)).abs() < 1e-9
            })
            .map(|p| *p.alt_usft())
            .collect();
        assert_eq!(cliff_at_first.len(), 5);
        for &expected_z in &[15.0, 20.0, 25.0, 30.0, 35.0] {
            assert!(cliff_at_first.iter().any(|&z| (z - expected_z).abs() < 1e-9));
        }
    }

    // --- apply_mast_offset ---

    #[test]
    fn apply_mast_offset_zero_offset_unchanged() {
        let pts = vec![pt(0.0, 0.0, 10.0)];
        let (display, meas) = apply_mast_offset(pts, 0.0);
        assert_abs_diff_eq!(*display[0].alt_usft(), 10.0, epsilon = 1e-9);
        assert_abs_diff_eq!(*meas[0].alt_usft(), 10.0, epsilon = 1e-9);
    }

    #[test]
    fn apply_mast_offset_shifts_highest_at_each_xy() {
        let pts = vec![
            pt(100.0, 200.0, 10.0), // lower at (100, 200)
            pt(100.0, 200.0, 15.0), // highest at (100, 200)
            pt(200.0, 200.0, 12.0), // only point at (200, 200)
        ];
        let (display, meas) = apply_mast_offset(pts, 5.0);

        assert_abs_diff_eq!(*display[0].alt_usft(), 10.0, epsilon = 1e-9);
        assert_abs_diff_eq!(*display[1].alt_usft(), 15.0, epsilon = 1e-9);
        assert_abs_diff_eq!(*display[2].alt_usft(), 12.0, epsilon = 1e-9);

        assert_abs_diff_eq!(*meas[0].alt_usft(), 10.0, epsilon = 1e-9); // not highest
        assert_abs_diff_eq!(*meas[1].alt_usft(), 20.0, epsilon = 1e-9); // highest at (100,200) → +5
        assert_abs_diff_eq!(*meas[2].alt_usft(), 17.0, epsilon = 1e-9); // only → +5
    }

    #[test]
    fn apply_mast_offset_display_pts_always_unchanged() {
        let pts = vec![pt(0.0, 0.0, 8.0)];
        let (display, _) = apply_mast_offset(pts, 10.0);
        assert_abs_diff_eq!(*display[0].alt_usft(), 8.0, epsilon = 1e-9);
    }

    // --- cull_and_combine ---

    #[test]
    fn cull_and_combine_removes_points_within_radius() {
        let base = vec![
            pt(0.0, 0.0, 10.0),      // at perim → culled
            pt(100.0, 100.0, 10.0),  // far → kept
        ];
        let cliff = vec![pt(1.0, 0.0, 20.0)]; // within radius → culled
        let perim = vec![pt(0.0, 0.0, 10.0)];

        let result = cull_and_combine(base, cliff, perim, 5.0);

        // perim (1) + surviving base (1, at 100,100) + surviving cliff (0)
        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|p| (*p.easting() - 100.0).abs() < 1e-9));
        assert!(!result.iter().any(|p| *p.easting() == 1.0)); // cliff point removed
    }

    #[test]
    fn cull_and_combine_empty_perimeter_keeps_all() {
        let base = vec![pt(0.0, 0.0, 10.0)];
        let cliff = vec![pt(0.0, 0.0, 20.0)];
        let result = cull_and_combine(base, cliff, vec![], 5.0);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn cull_and_combine_perimeter_points_come_first() {
        let base = vec![pt(50.0, 50.0, 10.0)];
        let perim = vec![pt(0.0, 0.0, 10.0)];
        let result = cull_and_combine(base, vec![], perim, 1.0);
        assert_eq!(result.len(), 2);
        assert_abs_diff_eq!(*result[0].easting(), 0.0, epsilon = 1e-9); // perim first
        assert_abs_diff_eq!(*result[1].easting(), 50.0, epsilon = 1e-9);
    }

    // --- sample_perimeter ---

    #[test]
    fn sample_perimeter_returns_points_on_ring() {
        // 20×20 heightmap → polygon perimeter = 80 ft, spacing = 10 → 8 points
        let hm = flat_hm(20, 20, 120);
        let pts = sample_perimeter(&hm, 10.0);
        assert_eq!(pts.len(), 8);
        for p in &pts {
            assert_abs_diff_eq!(*p.alt_usft(), 10.0, epsilon = 1e-9);
        }
    }

    #[test]
    fn sample_perimeter_empty_for_degenerate_ring() {
        let heights = Array2::from_elem((1, 1), 120u16);
        let mask = Array2::from_elem((1, 1), true);
        let poly = polygon![(x: SW_E, y: SW_N), (x: SW_E, y: SW_N), (x: SW_E, y: SW_N)];
        let hm = RooftopHeightMap::new(
            BINId::parse("1000001").unwrap(),
            NYSCoords2::new(SW_E, SW_N),
            heights,
            mask,
            poly,
        );
        let pts = sample_perimeter(&hm, 10.0);
        assert!(pts.is_empty());
    }

    // --- sample_points_for_rooftop (integration) ---

    #[test]
    fn sample_points_for_rooftop_flat_roof_no_cliffs() {
        let hm = flat_hm(20, 20, 120); // 10 ft flat
        let pts = sample_points_for_rooftop(&hm, 2.0, 10.0);
        assert!(!pts.is_empty());
    }
}
