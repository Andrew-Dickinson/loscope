from dataclasses import dataclass, field

import numpy as np

OBSTRUCTION_TYPE_BUILDING = "building_footprint_since_2021"

OBSTRUCTION_TYPE_NEW_CONSTRUCTION_2021 = "completed_construction_since_2021"

OBSTRUCTION_TYPE_PERMITS_ISSUED_2025 = "permits_issued_since_2025"
OBSTRUCTION_TYPE_PERMITS_ISSUED_2020 = "permits_issued_since_2020"
OBSTRUCTION_TYPE_PERMITS_ISSUED_2015 = "permits_issued_since_2015"

OBSTRUCTION_TYPE_PLANS_APPROVED_2025 = "plans_approved_since_2025"
OBSTRUCTION_TYPE_PLANS_APPROVED_2020 = "plans_approved_since_2020"
OBSTRUCTION_TYPE_PLANS_APPROVED_2015 = "plans_approved_since_2015"

OBSTRUCTION_TYPE_PLANS_FILED_2025 = "plans_filed_since_2025"
OBSTRUCTION_TYPE_PLANS_FILED_2020 = "plans_filed_since_2020"
OBSTRUCTION_TYPE_PLANS_FILED_2015 = "plans_filed_since_2015"

OBSTRUCTION_TYPE_MANUAL_ANNOTATION = "manual_annotation"

@dataclass
class Obstruction:
    obstruction_id: str      # UUID string
    obstruction_type: str    # one of the OBSTRUCTION_TYPE_* constants
    attributes: dict         # arbitrary key-value properties
    x_offset: int            # SW corner easting (NYS usft)
    y_offset: int            # SW corner northing (NYS usft)
    raster: np.ndarray       # uint16, shape (W, H), axes [easting_local, northing_local]
    tile_ids: list = field(default_factory=list)  # canonical tile IDs the footprint intersects
