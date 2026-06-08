use std::path::Path;

use anyhow::{bail, Result};

use super::arcgis;

pub struct PlanimetricLayer {
    pub name: &'static str,
    pub slug: &'static str,
    pub url: &'static str,
}

pub const PLANIMETRIC_LAYERS: &[PlanimetricLayer] = &[
    PlanimetricLayer {
        name: "Boardwalk",
        slug: "boardwalk",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/BOARDWALK_2022/FeatureServer/0",
    },
    PlanimetricLayer {
        name: "Cooling Towers",
        slug: "cooling-towers",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Cooling_Towers_2022/FeatureServer/3",
    },
    PlanimetricLayer {
        name: "Curb",
        slug: "curb",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Curb_2022/FeatureServer/4",
    },
    PlanimetricLayer {
        name: "Curb Cut",
        slug: "curb-cut",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Curb_Cut_2022/FeatureServer/5",
    },
    PlanimetricLayer {
        name: "Elevation",
        slug: "elevation",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Elevation_2022/FeatureServer/6",
    },
    PlanimetricLayer {
        name: "Hydro Structure",
        slug: "hydro-structure",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Hydro_Structure_2022/FeatureServer/7",
    },
    PlanimetricLayer {
        name: "Hydrography",
        slug: "hydrography",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Hydrography_2022/FeatureServer/8",
    },
    PlanimetricLayer {
        name: "Median",
        slug: "median",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Median_2022/FeatureServer/9",
    },
    PlanimetricLayer {
        name: "Miscellaneous Structure Polygon",
        slug: "misc-structure-poly",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Miscellaneous_Structure_Polygon_2022/FeatureServer/10",
    },
    PlanimetricLayer {
        name: "Open Space (No Park)",
        slug: "open-space-no-park",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Open_Space_No_Park_2022/FeatureServer/11",
    },
    PlanimetricLayer {
        name: "Park",
        slug: "park",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Park_2022/FeatureServer/12",
    },
    PlanimetricLayer {
        name: "Parking Lot",
        slug: "parking-lot",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Parking_Lot_2022/FeatureServer/13",
    },
    PlanimetricLayer {
        name: "Pavement Edge",
        slug: "pavement-edge",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Pavement_Edge_2022/FeatureServer/14",
    },
    PlanimetricLayer {
        name: "Pavement Edge Carto",
        slug: "pavement-edge-carto",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Pavement_Edge_Carto_2022/FeatureServer/15",
    },
    PlanimetricLayer {
        name: "Plaza",
        slug: "plaza",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Plaza_2022/FeatureServer/16",
    },
    PlanimetricLayer {
        name: "Railroad",
        slug: "railroad",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Railroad_2022/FeatureServer/17",
    },
    PlanimetricLayer {
        name: "Railroad Structure",
        slug: "railroad-structure",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Railroad_Structure_2022/FeatureServer/18",
    },
    PlanimetricLayer {
        name: "Retaining Wall",
        slug: "retaining-wall",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Retaining_Wall_2022/FeatureServer/19",
    },
    PlanimetricLayer {
        name: "Roadbed",
        slug: "roadbed",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Roadbed_2022/FeatureServer/20",
    },
    PlanimetricLayer {
        name: "Shoreline",
        slug: "shoreline",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Shoreline_2022/FeatureServer/21",
    },
    PlanimetricLayer {
        name: "Sidewalk",
        slug: "sidewalk",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Sidewalk_2022/FeatureServer/22",
    },
    PlanimetricLayer {
        name: "Sidewalk Line",
        slug: "sidewalk-line",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Sidewalk_Line_2022/FeatureServer/23",
    },
    PlanimetricLayer {
        name: "Swimming Pool",
        slug: "swimming-pool",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Swimming_Pool_2022/FeatureServer/24",
    },
    PlanimetricLayer {
        name: "Transport Structure",
        slug: "transport-structure",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Transport_Structure_2022/FeatureServer/25",
    },
    PlanimetricLayer {
        name: "Under Construction / Unknown",
        slug: "under-construction-unknown",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Under_Construction_Unknown_2022/FeatureServer/26",
    },
    PlanimetricLayer {
        name: "Water Tank",
        slug: "water-tank",
        url: "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/Water_Tank_2022/FeatureServer/27",
    },
];

pub fn find_layer(slug: &str) -> Option<&'static PlanimetricLayer> {
    PLANIMETRIC_LAYERS.iter().find(|l| l.slug == slug)
}

fn download_layer(layer: &PlanimetricLayer, out_dir: &Path, chunk_size: usize) -> Result<()> {
    let out_path = out_dir.join(format!("planimetrics-{}.csv", layer.slug));
    println!("Downloading {} → {}", layer.name, out_path.display());
    arcgis::download(layer.url, &out_path, "1=1", chunk_size)
}

pub fn download_all(out_dir: &Path, chunk_size: usize) -> Result<()> {
    std::fs::create_dir_all(out_dir)?;
    for layer in PLANIMETRIC_LAYERS {
        download_layer(layer, out_dir, chunk_size)?;
    }
    println!("All planimetric layers downloaded.");
    Ok(())
}

pub fn download_one(slug: &str, out_dir: &Path, chunk_size: usize) -> Result<()> {
    std::fs::create_dir_all(out_dir)?;
    match find_layer(slug) {
        Some(layer) => download_layer(layer, out_dir, chunk_size),
        None => {
            let known: Vec<&str> = PLANIMETRIC_LAYERS.iter().map(|l| l.slug).collect();
            bail!(
                "Unknown planimetric layer {:?}. Known slugs:\n  {}",
                slug,
                known.join("\n  ")
            )
        }
    }
}