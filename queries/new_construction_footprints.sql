SELECT
    bin,
    base_bbl as tax_lot_bbl,
    the_geom as output_geometry,
    ground_elevation,
    height_roof,
    'new_construction_building_footprint' AS type,
    json_object(
        'bin', bin,
        'bbl', base_bbl,
        'ground_elevation', ground_elevation,
        'height_roof', height_roof,
        'geom_source', geom_source,
        'construction_year', CAST(construction_year AS INTEGER),
        'last_status_type', last_status_type
    ) AS props
FROM building_footprints
WHERE construction_year >= 2021
AND height_roof IS NOT NULL
AND the_geom IS NOT NULL;