use std::cell::RefCell;
use std::sync::{Arc, OnceLock};
use eproj::{Coordinate3, Projector, SpatialReferenceIdentifier};
use crate::types::coords::{GPSCoords2, GPSCoords3, NYSCoords2, NYSCoords3};

/////////  THREADING STUFF

// Utils to create only a single CoordinateConverter per thread, in a safe manner

static WRAPPER_INIT: OnceLock<Box<dyn Fn() -> CoordinateConverter + Send + Sync>> = OnceLock::new();

thread_local! {
    static LOCAL: RefCell<CoordinateConverter> = RefCell::new(
        WRAPPER_INIT.get().expect("call init_coord_converter_factory first")()
    );
}

pub fn init_coord_converter_factory(f: impl Fn() -> CoordinateConverter + Send + Sync + 'static) {
    WRAPPER_INIT.set(Box::new(f)).ok().expect("factory already set");
}

pub fn with_coord_converter<F, R>(f: F) -> R
where
    F: FnOnce(&CoordinateConverter) -> R,
{
    LOCAL.with(|w| f(&w.borrow()))
}

/////////  END THREADING STUFF

const METERS_PER_FOOT: f64 = 0.3048;

pub struct CoordinateConverter {
    nys_to_gps: Projector,
    gps_to_nys: Projector,
}

impl CoordinateConverter {
    pub fn new() -> Self {
        // Safety: the unwrap() calls below will panic if the EPSG specifiers are invalid, or
        // if no transforms are available to between them, but since we hard code the EPSG
        // specifiers, this should easily be detectable via testing, and shouldn't be propagated
        // to callers (since this coordinate transform is essential to the functioning
        // of the application and there's nothing callers can do to fix these issues)

        Self {
            nys_to_gps: Projector::new(
                SpatialReferenceIdentifier::Epsg6539,
                SpatialReferenceIdentifier::Epsg9754
            ).unwrap(),
            gps_to_nys: Projector::new(
                SpatialReferenceIdentifier::Epsg9754,
                SpatialReferenceIdentifier::Epsg6539
            ).unwrap(),
        }
    }

    pub fn to_nys_plane2(&self, gps_coords: &GPSCoords2) -> NYSCoords2 {
        NYSCoords2::from3(&self.to_nys_plane3(&GPSCoords3::from2(gps_coords, 0.0)))
    }

    pub fn to_nys_plane3(&self, gps_coords: &GPSCoords3) -> NYSCoords3 {
        let res = self.gps_to_nys
            .convert(Coordinate3::new(*gps_coords.lon(), *gps_coords.lat(), *gps_coords.alt_m() / METERS_PER_FOOT))
            .unwrap();
        // .or_else(|err| Err(CoordinateErr(format!("Error converting {gps_coords:?} to NYS plane {err}"))))?;

        NYSCoords3::new(res.x(), res.y(), res.z())
    }

    pub fn to_gps2(&self, nys_coords: &NYSCoords2) -> GPSCoords2 {
        GPSCoords2::from3(&self.to_gps3(&NYSCoords3::from2(nys_coords, 0.0)))
    }

    pub fn to_gps3(&self, nys_coords: &NYSCoords3) -> GPSCoords3 {
        let res = self.nys_to_gps
            .convert(Coordinate3::new(*nys_coords.easting(), *nys_coords.northing(), *nys_coords.alt_usft()))
            .unwrap();
        // .or_else(|err| Err(CoordinateErr(format!("Error converting {nys_coords:?} to GPS coords {err}"))))?;

        GPSCoords3::new(res.y(), res.x(), res.z() * METERS_PER_FOOT)
    }
}

pub struct ThreadLocalCoordConverter {
    init: Arc<dyn Fn() -> CoordinateConverter + Send + Sync>,
}

impl ThreadLocalCoordConverter {
    pub fn new(init: impl Fn() -> CoordinateConverter + Send + Sync + 'static) -> Self {
        Self { init: Arc::new(init) }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use approx::assert_abs_diff_eq;
    use super::*;

    #[test]
    fn convert_nys_to_gps_simple_roundtrip() {
        let converter = CoordinateConverter::new();
        let nys = NYSCoords3::new(1039748.806, 176148.995, 329.337);

        let gps = converter.to_gps3(&nys);
        assert_abs_diff_eq!(*gps.lat(), 40.650, epsilon = 0.000001);
        assert_abs_diff_eq!(*gps.lon(), -73.800, epsilon = 0.000001);
        assert_abs_diff_eq!(*gps.alt_m(), 100.0, epsilon = 0.001);
        assert_abs_diff_eq!(converter.to_nys_plane3(&converter.to_gps3(&nys)), nys, epsilon = 0.001);
    }
    #[test]
    fn convert_nys_to_gps_simple_roundtrip_no_z() {
        let converter = CoordinateConverter::new();
        let nys = NYSCoords2::new(1039748.806, 176148.995);

        let gps = converter.to_gps2(&nys);
        assert_abs_diff_eq!(*gps.lat(), 40.650, epsilon = 0.000001);
        assert_abs_diff_eq!(*gps.lon(), -73.800, epsilon = 0.000001);
        assert_abs_diff_eq!(converter.to_nys_plane2(&converter.to_gps2(&nys)), nys, epsilon = 0.001);
    }

    #[test]
    fn convert_gps_to_nys_simple_roundtrip() {
        let converter = CoordinateConverter::new();
        let gps = GPSCoords3::new(40.7850341, -73.9633981, 55.0);

        let nys = converter.to_nys_plane3(&gps);
        // TODO: Tighten up this epsilon by validating that our conversion is actually high precision
        assert_abs_diff_eq!(nys, NYSCoords3::new(994386.443, 225285.050, 181.446), epsilon = 1.0);

        let new_gps = converter.to_gps3(&converter.to_nys_plane3(&gps));
        assert_abs_diff_eq!(*new_gps.lat(), *gps.lat(), epsilon = 0.0000001);
        assert_abs_diff_eq!(*new_gps.lon(), *gps.lon(), epsilon = 0.0000001);
        assert_abs_diff_eq!(*new_gps.alt_m(), *gps.alt_m(), epsilon = 0.001);
    }

    #[test]
    fn exactly_one_converter_per_thread() {
        init_coord_converter_factory(|| CoordinateConverter::new());

        let handle = thread::spawn(|| {
                let addr1 = with_coord_converter(|converter| std::ptr::addr_of!(converter) as usize);
                let addr2 = with_coord_converter(|converter| std::ptr::addr_of!(converter) as usize);
                assert_eq!(addr1, addr2);
                addr1
            }
        );

        let handle2 = thread::spawn(|| {
            let addr3 = with_coord_converter(|converter| std::ptr::addr_of!(converter) as usize);
            let addr4 = with_coord_converter(|converter| std::ptr::addr_of!(converter) as usize);
            let addr5 = with_coord_converter(|converter| std::ptr::addr_of!(converter) as usize);
            let addr6 = with_coord_converter(|converter| std::ptr::addr_of!(converter) as usize);
            assert_eq!(addr3, addr4);
            assert_eq!(addr3, addr5);
            assert_eq!(addr3, addr6);
            addr3
        }
        );

        let addr1 = handle.join().unwrap();
        let addr3 = handle2.join().unwrap();
        assert_ne!(addr1, addr3);
    }
}