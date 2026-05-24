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

    #[cfg(test)]
    fn new_for_test(base_url: &str) -> Self {
        Self {
            meshdb_client: crate::meshdb::Client::new_with_client(base_url, reqwest::Client::new()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use wiremock::matchers::{method, path, query_param};

    const NODE_UUID: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    const BUILDING_UUID: &str = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
    const INSTALL_UUID: &str = "cccccccc-cccc-cccc-cccc-cccccccccccc";
    const VALID_BIN: i64 = 1234567;
    const INVALID_BIN: i64 = 9999999;

    fn disambiguate_nn_response(node_id: &str) -> serde_json::Value {
        serde_json::json!({
            "resolved_node": {},
            "supporting_data": {
                "exact_match_node": {"id": node_id}
            }
        })
    }

    fn disambiguate_install_response(install_id: &str) -> serde_json::Value {
        serde_json::json!({
            "resolved_node": {},
            "supporting_data": {
                "exact_match_install": {"id": install_id, "install_number": 100}
            }
        })
    }

    fn disambiguate_no_match_response() -> serde_json::Value {
        serde_json::json!({
            "resolved_node": {},
            "supporting_data": {}
        })
    }

    fn node_response(node_id: &str, building_ids: &[&str]) -> serde_json::Value {
        let buildings: Vec<serde_json::Value> = building_ids.iter()
            .map(|id| serde_json::json!({"id": id}))
            .collect();
        serde_json::json!({
            "id": node_id,
            "buildings": buildings,
            "devices": [],
            "installs": [],
            "latitude": 40.7,
            "longitude": -74.0,
            "status": "Active"
        })
    }

    fn building_response(building_id: &str, bin: Option<i64>) -> serde_json::Value {
        serde_json::json!({
            "id": building_id,
            "installs": [],
            "bin": bin,
            "latitude": 40.7,
            "longitude": -74.0,
            "address_truth_sources": []
        })
    }

    fn install_response(install_id: &str, building_id: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "id": install_id,
            "request_date": "2024-01-01T00:00:00Z",
            "install_number": 100,
            "member": {},
            "building": {"id": building_id},
            "status": "Active",
            "roof_access": false,
            "additional_members": []
        })
    }

    async fn mount_disambiguate(server: &MockServer, number: u32, body: serde_json::Value) {
        Mock::given(method("GET"))
            .and(path("/api/v1/disambiguate-number/"))
            .and(query_param("number", number.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }

    async fn mount_node(server: &MockServer, node_id: &str, body: serde_json::Value) {
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/nodes/{}/", node_id)))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }

    async fn mount_building(server: &MockServer, building_id: &str, body: serde_json::Value) {
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/buildings/{}/", building_id)))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }

    async fn mount_install(server: &MockServer, install_id: &str, body: serde_json::Value) {
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/installs/{}/", install_id)))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }

    // --- resolve_nn_or_install_to_bin: NN path ---

    #[tokio::test]
    async fn nn_resolves_to_bin() {
        let server = MockServer::start().await;
        mount_disambiguate(&server, 1234, disambiguate_nn_response(NODE_UUID)).await;
        mount_node(&server, NODE_UUID, node_response(NODE_UUID, &[BUILDING_UUID])).await;
        mount_building(&server, BUILDING_UUID, building_response(BUILDING_UUID, Some(VALID_BIN))).await;

        let provider = ProgenitorMeshDBProvider::new_for_test(&server.uri());
        let result = provider.resolve_nn_or_install_to_bin(1234).await.unwrap();
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["bin"], "1234567");
        assert_eq!(json["source"], "NN");
    }

    #[tokio::test]
    async fn nn_no_buildings_with_valid_bin_returns_data_error() {
        let server = MockServer::start().await;
        mount_disambiguate(&server, 1234, disambiguate_nn_response(NODE_UUID)).await;
        mount_node(&server, NODE_UUID, node_response(NODE_UUID, &[BUILDING_UUID])).await;
        mount_building(&server, BUILDING_UUID, building_response(BUILDING_UUID, None)).await;

        let provider = ProgenitorMeshDBProvider::new_for_test(&server.uri());
        let err = provider.resolve_nn_or_install_to_bin(1234).await.unwrap_err();
        assert!(matches!(err, MeshDBError::DataError(_)));
    }

    #[tokio::test]
    async fn nn_building_invalid_bin_returns_data_error() {
        let server = MockServer::start().await;
        mount_disambiguate(&server, 1234, disambiguate_nn_response(NODE_UUID)).await;
        mount_node(&server, NODE_UUID, node_response(NODE_UUID, &[BUILDING_UUID])).await;
        // INVALID_BIN starts with 9 which is not a valid first digit for BINId
        mount_building(&server, BUILDING_UUID, building_response(BUILDING_UUID, Some(INVALID_BIN))).await;

        let provider = ProgenitorMeshDBProvider::new_for_test(&server.uri());
        let err = provider.resolve_nn_or_install_to_bin(1234).await.unwrap_err();
        assert!(matches!(err, MeshDBError::DataError(_)));
    }

    #[tokio::test]
    async fn nn_multiple_buildings_skips_to_first_with_valid_bin() {
        let second_building = "dddddddd-dddd-dddd-dddd-dddddddddddd";
        let server = MockServer::start().await;
        mount_disambiguate(&server, 1234, disambiguate_nn_response(NODE_UUID)).await;
        mount_node(&server, NODE_UUID, node_response(NODE_UUID, &[BUILDING_UUID, second_building])).await;
        mount_building(&server, BUILDING_UUID, building_response(BUILDING_UUID, None)).await;
        mount_building(&server, second_building, building_response(second_building, Some(2000000))).await;

        let provider = ProgenitorMeshDBProvider::new_for_test(&server.uri());
        let result = provider.resolve_nn_or_install_to_bin(1234).await.unwrap();
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["bin"], "2000000");
        assert_eq!(json["source"], "NN");
    }

    // --- resolve_nn_or_install_to_bin: Install path ---

    #[tokio::test]
    async fn install_resolves_to_bin() {
        let server = MockServer::start().await;
        mount_disambiguate(&server, 100, disambiguate_install_response(INSTALL_UUID)).await;
        mount_install(&server, INSTALL_UUID, install_response(INSTALL_UUID, Some(BUILDING_UUID))).await;
        mount_building(&server, BUILDING_UUID, building_response(BUILDING_UUID, Some(VALID_BIN))).await;

        let provider = ProgenitorMeshDBProvider::new_for_test(&server.uri());
        let result = provider.resolve_nn_or_install_to_bin(100).await.unwrap();
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["bin"], "1234567");
        assert_eq!(json["source"], "Install");
    }

    #[tokio::test]
    async fn install_no_building_id_returns_data_error() {
        let server = MockServer::start().await;
        mount_disambiguate(&server, 100, disambiguate_install_response(INSTALL_UUID)).await;
        mount_install(&server, INSTALL_UUID, install_response(INSTALL_UUID, None)).await;

        let provider = ProgenitorMeshDBProvider::new_for_test(&server.uri());
        let err = provider.resolve_nn_or_install_to_bin(100).await.unwrap_err();
        assert!(matches!(err, MeshDBError::DataError(_)));
    }

    #[tokio::test]
    async fn install_building_no_valid_bin_returns_data_error() {
        let server = MockServer::start().await;
        mount_disambiguate(&server, 100, disambiguate_install_response(INSTALL_UUID)).await;
        mount_install(&server, INSTALL_UUID, install_response(INSTALL_UUID, Some(BUILDING_UUID))).await;
        mount_building(&server, BUILDING_UUID, building_response(BUILDING_UUID, None)).await;

        let provider = ProgenitorMeshDBProvider::new_for_test(&server.uri());
        let err = provider.resolve_nn_or_install_to_bin(100).await.unwrap_err();
        assert!(matches!(err, MeshDBError::DataError(_)));
    }

    // --- resolve_nn_or_install_to_bin: unrecognized ---

    #[tokio::test]
    async fn unrecognized_number_returns_data_error() {
        let server = MockServer::start().await;
        mount_disambiguate(&server, 9999, disambiguate_no_match_response()).await;

        let provider = ProgenitorMeshDBProvider::new_for_test(&server.uri());
        let err = provider.resolve_nn_or_install_to_bin(9999).await.unwrap_err();
        assert!(matches!(err, MeshDBError::DataError(_)));
    }

    // --- resolve_nn_or_install_to_bin: API error ---

    #[tokio::test]
    async fn api_error_returns_api_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/disambiguate-number/"))
            .respond_with(ResponseTemplate::new(500).set_body_json(
                serde_json::json!({"detail": "internal server error"})
            ))
            .mount(&server)
            .await;

        let provider = ProgenitorMeshDBProvider::new_for_test(&server.uri());
        let err = provider.resolve_nn_or_install_to_bin(1234).await.unwrap_err();
        assert!(matches!(err, MeshDBError::ApiError(_)));
    }
}