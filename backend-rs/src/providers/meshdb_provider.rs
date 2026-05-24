use std::sync::Arc;
use std::time::Duration;
use derive_new::new;
use image::DynamicImage;
use uuid::Uuid;
use crate::building::bin_id::BINId;
use crate::meshdb::types::ErrorResponse;
use crate::providers::backends::fs_cache::AssetProvider;
use crate::types::errors::{AssetErr, MeshDBError};
use crate::types::meshdb::{MeshdbBINSource, NumberLookupResponse};
use crate::types::tiles::TileId;

const MESHDB_BASE_URL: &str = "https://db.nycmesh.net";


pub struct ProgenitorMeshDBProvider  {
    meshdb_client: crate::meshdb::Client,
}

impl ProgenitorMeshDBProvider {
    pub fn new(meshdb_api_token: String) -> Self {
        let authorization_header = format!("Token {}", meshdb_api_token);

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            authorization_header.parse().unwrap(),
        );

        let client = reqwest::ClientBuilder::new()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(15))
            .default_headers(headers)
            .build()
            .unwrap();

        Self {
            meshdb_client: crate::meshdb::Client { baseurl: MESHDB_BASE_URL.to_string(), client }
        }
    }

    async fn get_bin_from_building_id(&self, building_id: &Uuid) -> Result<Option<BINId>, progenitor_client::Error> {
        let building_response = self.meshdb_client
            .api_v1_buildings_retrieve(&building_id).await?.into_inner();
        if let Some(bin_int) = building_response.bin {
            Ok(BINId::from_int(bin_int).ok())
        } else {
            Ok(None)
        }
    }

    pub async fn resolve_nn_or_install_to_bin(&self, number: u32) -> Result<NumberLookupResponse, MeshDBError> {
        let lookup_response = self.meshdb_client
            .api_v1_disambiguate_number_retrieve(number.into()).await?.into_inner();

        if let Some(node_id) = lookup_response
            .supporting_data
            .exact_match_node.and_then(|n| n.id) {
            let node_response = self.meshdb_client.api_v1_nodes_retrieve(&node_id).await?.into_inner();
            let building_ids: Vec<Uuid> = node_response.buildings.iter()
                .filter_map(|b| b.id).collect();
            for building_id in building_ids {
                if let Some(bin) = self.get_bin_from_building_id(&building_id).await? {
                    return Ok(NumberLookupResponse::new(bin, MeshdbBINSource::NN))
                }
            }

            Err(MeshDBError::DataError(
                format!("No buildings with valid BINs found for node: {}", node_id)
            ))
        } else if let Some(install_id) = lookup_response.supporting_data.exact_match_install.map(|inst| inst.id) {
            let install_response = self.meshdb_client.api_v1_installs_retrieve(&install_id).await?.into_inner();
            if let Some(building_id) = install_response.building.id {
                if let Some(bin) = self.get_bin_from_building_id(&building_id).await? {
                    return Ok(NumberLookupResponse::new(bin, MeshdbBINSource::Install))
                }
            }

            Err(MeshDBError::DataError(
                format!("No buildings with valid BINs found for install: {}", install_id)
            ))
        } else {
            Err(MeshDBError::DataError(format!("{} is not a recognized NN or install number", number)))
        }
    }
}