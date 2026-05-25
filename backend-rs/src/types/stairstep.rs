use crate::types::coords::NYSCoords2;
use crate::types::tiles::{SUBGRID_TILE_SIDE_LENGTH_USFT, TileId};
use arrayvec::ArrayVec;
use derive_getters::Getters;
use derive_new::new;
use futures_util::StreamExt;
use ndarray::{Array1, Array2, s};
use rocket::serde::{Deserialize, Serialize};
use std::iter::repeat_n;
use std::mem::MaybeUninit;
use typed_floats::tf64::PositiveFinite;
use wincode::config::Config;
use wincode::io::{Reader, Writer};
use wincode::{ReadError, SchemaRead, SchemaWrite};

/// Maps a StairStepGrid element type to a wincode-serializable wire form.
///
/// This exists because ndarray element types (like `PositiveFinite`) may not
/// implement wincode's foreign traits directly due to the orphan rule.
pub(crate) trait WincodeGridElem: Copy + Sized + 'static {
    type Wire: Clone + 'static;
    fn into_wire(self) -> Self::Wire;
    fn from_wire(wire: Self::Wire) -> Self;
}

impl WincodeGridElem for PositiveFinite {
    type Wire = f64;
    fn into_wire(self) -> f64 {
        self.into()
    }
    fn from_wire(w: f64) -> Self {
        PositiveFinite::try_from(w).expect("deserialized f64 is not a valid PositiveFinite")
    }
}

/// Sparse Array2 representation, which uses an x-offset for each row in values to shift that row
/// in the positive-x direction. The contents of row i in values are only valid up to widths[i]
#[derive(new, Serialize, Deserialize, Getters)]
pub struct StairStepGrid<T> {
    values: Array2<T>,
    widths: Array1<usize>,
    offsets: Array1<usize>,
    base_offset: NYSCoords2,
}

impl<T> StairStepGrid<T>
where
    T: Ord,
{
    pub fn max(&self) -> Option<&T> {
        assert_eq!(self.values.nrows(), self.widths.len());
        self.values
            .rows()
            .into_iter()
            .zip(self.widths.iter())
            .flat_map(|(row, &width)| row.into_iter().take(width))
            .max()
    }

    pub fn max_in_tile(&self, tile_id: TileId) -> Option<&T> {
        self.rasterize_in_tile_iter(tile_id).flatten().max()
    }
}

impl<T> StairStepGrid<T> {
    pub fn is_empty(&self) -> bool {
        !self.widths.iter().any(|&w| w > 0)
    }

    pub fn merge<U, V: Default, F: Fn(&T, &U, (usize, usize)) -> V>(
        &self,
        other: &StairStepGrid<U>,
        merge_fn: F,
    ) -> StairStepGrid<V> {
        let mut output: StairStepGrid<V> = StairStepGrid {
            values: Array2::default((self.values.shape()[0], self.values.shape()[1])),
            widths: self.widths.clone(),
            offsets: self.offsets.clone(),
            base_offset: self.base_offset.clone(),
        };

        for i in 0..self.widths().len() {
            let self_row = self.values().row(i);
            let other_row = other.values().row(i);

            let width = self.widths()[i];
            assert_eq!(other.widths()[i], width);

            let offset_y = i;
            let offset_x = self.offsets()[i];
            assert_eq!(other.offsets()[i], offset_x);

            self_row.iter().zip(other_row.iter()).enumerate().for_each(
                |(j, (self_val, other_val))| {
                    output.values[[i, j]] = merge_fn(self_val, other_val, (offset_x, offset_y));
                },
            )
        }

        output
    }
}

impl<T> StairStepGrid<T> {
    pub fn rasterize_in_tile(&self, tile_id: TileId) -> Array2<Option<&T>> {
        // Safety: rasterize_in_tile_iter is guaranteed to return a vec of the right size, so
        // this unwrap should never panic
        Array2::<Option<&T>>::from_shape_vec(
            (
                SUBGRID_TILE_SIDE_LENGTH_USFT.into(),
                SUBGRID_TILE_SIDE_LENGTH_USFT.into(),
            ),
            self.rasterize_in_tile_iter(tile_id).collect(),
        )
        .unwrap()
        .reversed_axes()
    }

    fn rasterize_in_tile_iter(&self, tile_id: TileId) -> impl Iterator<Item = Option<&T>> + '_ {
        const TILE_SIDE: usize = SUBGRID_TILE_SIDE_LENGTH_USFT as usize;

        let step_base_offset = &self.base_offset;
        let tile_base_offset = tile_id.get_sw_corner();

        let step_base_offset = (
            step_base_offset.easting().floor() as usize,
            step_base_offset.northing().floor() as usize,
        );
        let tile_base_offset = (
            tile_base_offset.easting().floor() as usize,
            tile_base_offset.northing().floor() as usize,
        );

        (0..TILE_SIDE).flat_map(move |tile_i| {
            let mut row = ArrayVec::<Option<&T>, TILE_SIDE>::new();

            let step_i = ((tile_i + tile_base_offset.1) as isize) - (step_base_offset.1 as isize);

            let Some(step_i) = usize::try_from(step_i)
                .ok()
                .filter(|step_i| *step_i < self.widths.len())
            else {
                row.extend(repeat_n(None, TILE_SIDE));
                return row;
            };

            let width = self.widths[step_i];
            let global_step_row_start = step_base_offset.0 + self.offsets[step_i];
            let global_step_row_end = global_step_row_start + width;

            let global_overlap_start = global_step_row_start.max(tile_base_offset.0);
            let global_overlap_end = global_step_row_end.min(tile_base_offset.0 + TILE_SIDE);
            if global_overlap_start >= global_overlap_end {
                row.extend(repeat_n(None, TILE_SIDE));
                return row;
            }

            // Safety: strict_sub() won't panic here because as constructed above,
            // global_overlap_end > global_overlap_start >= global_step_row_start &&
            // global_overlap_end > global_overlap_start >= tile_base_offset.0
            let step_j_start = global_overlap_start.strict_sub(global_step_row_start);
            let step_j_end = global_overlap_end.strict_sub(global_step_row_start);
            let tile_j_start = global_overlap_start.strict_sub(tile_base_offset.0);
            let tile_j_end = global_overlap_end.strict_sub(tile_base_offset.0);
            let tile_columns_after_overlap = TILE_SIDE.strict_sub(tile_j_end);

            row.extend(repeat_n(None, tile_j_start));
            row.extend(
                self.values
                    .slice(s![step_i, step_j_start..step_j_end])
                    .into_iter()
                    .map(Some),
            );
            row.extend(repeat_n(None, tile_columns_after_overlap));
            row
        })
    }
}

unsafe impl<C: Config, T> SchemaWrite<C> for StairStepGrid<T>
where
    T: WincodeGridElem,
    T::Wire: SchemaWrite<C, Src = T::Wire>,
    Vec<T::Wire>: SchemaWrite<C, Src = Vec<T::Wire>>,
{
    type Src = StairStepGrid<T>;

    fn size_of(src: &Self::Src) -> wincode::WriteResult<usize> {
        let nrows = src.values.nrows();
        let ncols = src.values.ncols();
        let wire_widths: Vec<usize> = src.widths.to_vec();
        let wire_offsets: Vec<usize> = src.offsets.to_vec();
        let wire_values: Vec<T::Wire> = src.values.iter().copied().map(T::into_wire).collect();
        Ok(<usize as SchemaWrite<C>>::size_of(&nrows)?
            + <usize as SchemaWrite<C>>::size_of(&ncols)?
            + <Vec<usize> as SchemaWrite<C>>::size_of(&wire_widths)?
            + <Vec<usize> as SchemaWrite<C>>::size_of(&wire_offsets)?
            + <NYSCoords2 as SchemaWrite<C>>::size_of(src.base_offset())?
            + <Vec<T::Wire> as SchemaWrite<C>>::size_of(&wire_values)?)
    }

    fn write(mut writer: impl Writer, src: &Self::Src) -> wincode::WriteResult<()> {
        let nrows = src.values.nrows();
        let ncols = src.values.ncols();
        <usize as SchemaWrite<C>>::write(writer.by_ref(), &nrows)?;
        <usize as SchemaWrite<C>>::write(writer.by_ref(), &ncols)?;
        let wire_widths: Vec<usize> = src.widths.to_vec();
        <Vec<usize> as SchemaWrite<C>>::write(writer.by_ref(), &wire_widths)?;
        let wire_offsets: Vec<usize> = src.offsets.to_vec();
        <Vec<usize> as SchemaWrite<C>>::write(writer.by_ref(), &wire_offsets)?;
        <NYSCoords2 as SchemaWrite<C>>::write(writer.by_ref(), src.base_offset())?;
        let wire_values: Vec<T::Wire> = src.values.iter().copied().map(T::into_wire).collect();
        <Vec<T::Wire> as SchemaWrite<C>>::write(writer, &wire_values)?;
        Ok(())
    }
}

unsafe impl<'de, C: Config, T> SchemaRead<'de, C> for StairStepGrid<T>
where
    T: WincodeGridElem,
    T::Wire: SchemaRead<'de, C, Dst = T::Wire>,
    Vec<T::Wire>: SchemaRead<'de, C, Dst = Vec<T::Wire>>,
{
    type Dst = StairStepGrid<T>;

    fn read(
        mut reader: impl Reader<'de>,
        dst: &mut MaybeUninit<Self::Dst>,
    ) -> wincode::ReadResult<()> {
        let nrows = <usize as SchemaRead<'de, C>>::get(reader.by_ref())?;
        let ncols = <usize as SchemaRead<'de, C>>::get(reader.by_ref())?;
        let wire_widths = <Vec<usize> as SchemaRead<'de, C>>::get(reader.by_ref())?;
        let wire_offsets = <Vec<usize> as SchemaRead<'de, C>>::get(reader.by_ref())?;
        let base_offset = <NYSCoords2 as SchemaRead<'de, C>>::get(reader.by_ref())?;
        let wire_values = <Vec<T::Wire> as SchemaRead<'de, C>>::get(reader)?;

        let values_data: Vec<T> = wire_values.into_iter().map(T::from_wire).collect();
        let values = Array2::from_shape_vec((nrows, ncols), values_data)
            .map_err(|_| ReadError::InvalidValue("array element count does not match shape"))?;
        let widths = Array1::from_vec(wire_widths);
        let offsets = Array1::from_vec(wire_offsets);

        dst.write(StairStepGrid {
            values,
            widths,
            offsets,
            base_offset,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::StairStepGrid;
    use crate::types::coords::NYSCoords2;
    use crate::types::tiles::TileId;
    use ndarray::{Array1, Array2, array};

    fn make_grid<T: Clone>(
        values: Array2<T>,
        widths: Array1<usize>,
        offsets: Array1<usize>,
    ) -> StairStepGrid<T> {
        StairStepGrid::new(values, widths, offsets, NYSCoords2::new(0.0, 0.0))
    }

    fn make_grid_at<T: Clone>(
        values: Array2<T>,
        widths: Array1<usize>,
        offsets: Array1<usize>,
        base: (f64, f64),
    ) -> StairStepGrid<T> {
        StairStepGrid::new(values, widths, offsets, NYSCoords2::new(base.0, base.1))
    }

    // "500300_00" → SW corner (500000, 300000), NE (500500, 300500)
    fn tile() -> TileId {
        TileId::parse("500300_00").unwrap()
    }
    fn tile_sw() -> (f64, f64) {
        (500_000.0, 300_000.0)
    }

    // --- rasterize_in_tile ---

    #[test]
    fn rasterize_writes_values_at_tile_origin() {
        let (e, n) = tile_sw();
        let values = Array2::from_shape_vec((1, 3), vec![7u8, 8, 9]).unwrap();
        let grid = make_grid_at(values, array![3], array![0], (e, n));
        let out = grid.rasterize_in_tile(tile());
        assert_eq!(*out[[0, 0]].unwrap(), 7);
        assert_eq!(*out[[1, 0]].unwrap(), 8);
        assert_eq!(*out[[2, 0]].unwrap(), 9);
        assert_eq!(out[[3, 0]], None); // beyond width → untouched
    }

    #[test]
    fn rasterize_multiple_rows() {
        let (e, n) = tile_sw();
        let values = Array2::from_shape_vec((2, 2), vec![10u8, 11, 20, 21]).unwrap();
        let grid = make_grid_at(values, array![2, 2], array![0, 0], (e, n));
        let out = grid.rasterize_in_tile(tile());
        assert_eq!((*out[[0, 0]].unwrap(), *out[[1, 0]].unwrap()), (10, 11)); // row 0 → tile_y=0
        assert_eq!((*out[[0, 1]].unwrap(), *out[[1, 1]].unwrap()), (20, 21)); // row 1 → tile_y=1
    }

    #[test]
    fn rasterize_x_offset_places_values_correctly() {
        let (e, n) = tile_sw();
        // offset=5 → data starts 5 usft east of zone base
        let values = Array2::from_shape_vec((1, 1), vec![42u8]).unwrap();
        let grid = make_grid_at(values, array![1], array![5], (e, n));
        let out = grid.rasterize_in_tile(tile());
        assert_eq!(*out[[5, 0]].unwrap(), 42);
        assert_eq!(out[[4, 0]], None);
        assert_eq!(out[[6, 0]], None);
    }

    #[test]
    fn rasterize_zero_width_rows_are_skipped() {
        let (e, n) = tile_sw();
        let values = Array2::from_shape_vec((3, 2), vec![9u8; 6]).unwrap();
        // Only middle row has non-zero width
        let grid = make_grid_at(values, array![0, 2, 0], array![0, 0, 0], (e, n));
        let out = grid.rasterize_in_tile(tile());
        assert_eq!(out[[0, 0]], None); // row 0 skipped
        assert_eq!(*out[[0, 1]].unwrap(), 9); // row 1 written
        assert_eq!(out[[0, 2]], None); // row 2 skipped
    }

    #[test]
    fn rasterize_zone_south_of_tile_returns_none() {
        // Zone base is far south — i_start will exceed i_end, loop never executes
        let values = Array2::from_shape_vec((1, 3), vec![99u8, 99, 99]).unwrap();
        let grid = make_grid_at(values, array![3], array![0], (500_000.0, 200_000.0));
        let out = grid.rasterize_in_tile(tile());
        assert!(out.iter().all(|&v| v == None));
    }

    #[test]
    fn rasterize_zone_north_of_tile_returns_none() {
        // Zone base is north of the tile — i_end will be negative, early return
        let values = Array2::from_shape_vec((1, 3), vec![99u8, 99, 99]).unwrap();
        let grid = make_grid_at(values, array![3], array![0], (500_000.0, 301_000.0));
        let out = grid.rasterize_in_tile(tile());
        assert!(out.iter().all(|&v| v == None));
    }

    #[test]
    fn rasterize_zone_east_of_tile_returns_none() {
        // Zone data is entirely east of the tile's NE easting (500500)
        let values = Array2::from_shape_vec((1, 1), vec![99u8]).unwrap();
        let grid = make_grid_at(values, array![1], array![0], (501_000.0, 300_000.0));
        let out = grid.rasterize_in_tile(tile());
        assert!(out.iter().all(|&v| v == None));
    }

    #[test]
    fn rasterize_partial_x_overlap_from_west() {
        // Zone starts 100 usft west of tile; 200-wide data → first 100 usft are clipped
        let (_, n) = tile_sw();
        let data: Vec<u8> = (0..200).map(|i| i as u8).collect();
        let values = Array2::from_shape_vec((1, 200), data).unwrap();
        let grid = make_grid_at(values, array![200], array![0], (499_900.0, n));
        let out = grid.rasterize_in_tile(tile());
        // j_start = 500000-499900 = 100, so out[0..100, 0] = values[100..200]
        assert_eq!(*out[[0, 0]].unwrap(), 100);
        assert_eq!(*out[[99, 0]].unwrap(), 199);
        assert_eq!(out[[100, 0]], None); // beyond the 200-wide zone data
    }

    #[test]
    fn rasterize_output_is_always_tile_sized() {
        let (e, n) = tile_sw();
        let values = Array2::from_shape_vec((1, 1), vec![1u8]).unwrap();
        let grid = make_grid_at(values, array![1], array![0], (e, n));
        let out = grid.rasterize_in_tile(tile());
        assert_eq!(out.shape(), &[500, 500]);
    }

    // --- is_empty ---

    #[test]
    fn is_empty_all_zero_widths() {
        let grid = make_grid(
            Array2::<i32>::zeros((3, 4)),
            array![0, 0, 0],
            array![0, 0, 0],
        );
        assert!(grid.is_empty());
    }

    #[test]
    fn is_empty_one_nonzero_width() {
        let grid = make_grid(
            Array2::<i32>::zeros((3, 4)),
            array![0, 2, 0],
            array![0, 0, 0],
        );
        assert!(!grid.is_empty());
    }

    // --- max ---

    #[test]
    fn max_returns_none_when_empty() {
        let grid = make_grid(Array2::<i32>::zeros((2, 3)), array![0, 0], array![0, 0]);
        assert_eq!(grid.max(), None);
    }

    #[test]
    fn max_ignores_cells_beyond_width() {
        // Row 0 has width 2; columns 2+ should be ignored even if they contain large values.
        let values = Array2::from_shape_vec((1, 4), vec![1, 3, 999, 999]).unwrap();
        let grid = make_grid(values, array![2], array![0]);
        assert_eq!(grid.max(), Some(&3));
    }

    #[test]
    fn max_across_multiple_rows() {
        let values = Array2::from_shape_vec((2, 3), vec![1, 2, 3, 4, 5, 6]).unwrap();
        // Row 0 valid up to width 2 (values 1,2), row 1 valid up to width 3 (values 4,5,6)
        let grid = make_grid(values, array![2, 3], array![0, 0]);
        assert_eq!(grid.max(), Some(&6));
    }

    // --- max_in_tile ---

    #[test]
    fn max_in_tile_single_overlapping_row() {
        let (e, n) = tile_sw();
        let values = Array2::from_shape_vec((1, 3), vec![5i32, 10, 3]).unwrap();
        let grid = make_grid_at(values, array![3], array![0], (e, n));
        assert_eq!(*grid.max_in_tile(tile()).unwrap(), 10);
    }

    #[test]
    fn max_in_tile_multiple_rows_returns_overall_max() {
        let (e, n) = tile_sw();
        let values = Array2::from_shape_vec((3, 3), vec![1i32, 2, 3, 7, 9, 4, 5, 6, 8]).unwrap();
        let grid = make_grid_at(values, array![3, 3, 3], array![0, 0, 0], (e, n));
        assert_eq!(*grid.max_in_tile(tile()).unwrap(), 9);
    }

    #[test]
    fn max_in_tile_zero_width_rows_skipped() {
        let (e, n) = tile_sw();
        // Rows 0 and 2 have width 0; only row 1 (value 7) is valid
        let values = Array2::from_shape_vec((3, 1), vec![99i32, 7, 99]).unwrap();
        let grid = make_grid_at(values, array![0, 1, 0], array![0, 0, 0], (e, n));
        assert_eq!(*grid.max_in_tile(tile()).unwrap(), 7);
    }

    #[test]
    fn max_in_tile_all_zero_widths_returns_none() {
        let (e, n) = tile_sw();
        let values = Array2::from_shape_vec((2, 2), vec![9i32; 4]).unwrap();
        let grid = make_grid_at(values, array![0, 0], array![0, 0], (e, n));
        assert_eq!(grid.max_in_tile(tile()), None);
    }

    #[test]
    fn max_in_tile_zone_south_of_tile_returns_none() {
        // Zone base northing 200_000, tile starts at 300_000 → i_start exceeds rows
        let values = Array2::from_shape_vec((1, 3), vec![9i32; 3]).unwrap();
        let grid = make_grid_at(values, array![3], array![0], (500_000.0, 200_000.0));
        assert_eq!(grid.max_in_tile(tile()), None);
    }

    #[test]
    fn max_in_tile_zone_north_of_tile_returns_none() {
        // Zone base northing 301_000 is above tile top (300_500) → i_end negative
        let values = Array2::from_shape_vec((1, 3), vec![9i32; 3]).unwrap();
        let grid = make_grid_at(values, array![3], array![0], (500_000.0, 301_000.0));
        assert_eq!(grid.max_in_tile(tile()), None);
    }

    #[test]
    fn max_in_tile_data_entirely_west_of_tile_returns_none() {
        // Zone base easting 499_000, 3-wide row → data spans [499_000, 499_003), tile starts 500_000
        let (_, n) = tile_sw();
        let values = Array2::from_shape_vec((1, 3), vec![9i32; 3]).unwrap();
        let grid = make_grid_at(values, array![3], array![0], (499_000.0, n));
        assert_eq!(grid.max_in_tile(tile()), None);
    }

    #[test]
    fn max_in_tile_clips_to_tile_east_boundary() {
        // 600-wide row starting at tile SW; cell 499 (value 99) is inside, cell 500 (value 100) is outside
        let (e, n) = tile_sw();
        let mut data = vec![0i32; 600];
        data[499] = 99;
        data[500] = 100;
        let values = Array2::from_shape_vec((1, 600), data).unwrap();
        let grid = make_grid_at(values, array![600], array![0], (e, n));
        assert_eq!(*grid.max_in_tile(tile()).unwrap(), 99);
    }

    #[test]
    fn max_in_tile_x_offset_shifts_data_into_tile() {
        // Zone base at (499_900, tile_n); offset 100 → data at absolute easting 499_900+100=500_000
        let (_, n) = tile_sw();
        let values = Array2::from_shape_vec((1, 1), vec![42i32]).unwrap();
        let grid = make_grid_at(values, array![1], array![100], (499_900.0, n));
        assert_eq!(*grid.max_in_tile(tile()).unwrap(), 42);
    }

    // --- merge ---

    #[test]
    fn merge_applies_fn_to_each_cell() {
        let a = Array2::from_shape_vec((2, 2), vec![1, 2, 3, 4]).unwrap();
        let b = Array2::from_shape_vec((2, 2), vec![10, 20, 30, 40]).unwrap();
        let g1 = make_grid(a, array![2, 2], array![0, 0]);
        let g2 = make_grid(b, array![2, 2], array![0, 0]);

        let merged = g1.merge(&g2, |x, y, _| x + y);

        assert_eq!(merged.values()[[0, 0]], 11);
        assert_eq!(merged.values()[[0, 1]], 22);
        assert_eq!(merged.values()[[1, 0]], 33);
        assert_eq!(merged.values()[[1, 1]], 44);
    }

    #[test]
    fn merge_passes_correct_offset_coords() {
        use std::cell::Cell;
        let a = Array2::from_shape_vec((1, 1), vec![0]).unwrap();
        let b = Array2::from_shape_vec((1, 1), vec![0]).unwrap();
        let g1 = make_grid(a, array![1], array![7]);
        let g2 = make_grid(b, array![1], array![7]);

        let captured = Cell::new((0usize, 0usize));
        g1.merge(&g2, |_, _, coords| {
            captured.set(coords);
            0
        });

        // offset_x comes from offsets[0]=7, offset_y is row index 0
        assert_eq!(captured.get(), (7, 0));
    }

    #[test]
    fn merge_preserves_widths_and_offsets() {
        let a = Array2::<i32>::zeros((2, 3));
        let b = Array2::<i32>::zeros((2, 3));
        let g1 = make_grid(a, array![1, 2], array![3, 4]);
        let g2 = make_grid(b, array![1, 2], array![3, 4]);

        let merged = g1.merge(&g2, |x, y, _| x + y);

        assert_eq!(merged.widths(), g1.widths());
        assert_eq!(merged.offsets(), g1.offsets());
    }
}
