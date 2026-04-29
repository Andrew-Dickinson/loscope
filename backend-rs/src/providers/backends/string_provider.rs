
use tokio_rusqlite::{rusqlite, Connection};
use crate::providers::backends::asset_fetcher::AssetType;
use crate::types::errors::AssetErr;

#[async_trait]
pub trait StringProvider {
    // Get an asset string from a database backend. The logic for what `identifier` is, and
    // how it's used in a query is determined by the value of AssetType
    async fn get_string(&self, asset_type: AssetType, identifier: &str) -> Result<String, AssetErr>;
}

pub struct NYCDOBSqliteStringProvider {
    db_connection: Connection
}

impl NYCDOBSqliteStringProvider {
    pub async fn new(db_file_path: &str) -> Result<Self, tokio_rusqlite::Error> {
        let db_connection = Connection::open(db_file_path).await?;
        db_connection.call(
            |conn| conn.execute("PRAGMA query_only=ON", ())
        ).await?;
        Ok(NYCDOBSqliteStringProvider { db_connection })
    }
}

#[cfg(test)]
impl NYCDOBSqliteStringProvider {
    fn with_connection(conn: Connection) -> Self {
        NYCDOBSqliteStringProvider { db_connection: conn }
    }
}

#[async_trait]
impl StringProvider for NYCDOBSqliteStringProvider {
    async fn get_string(&self, asset_type: AssetType, identifier: &str) -> Result<String, AssetErr> {
        if asset_type != AssetType::BuildingFootprintWKT {
            return Err(AssetErr::UnsupportedAssetType(
                format!("NYCDOBSqliteStringFetcher only supports BuildingFootprintWKT, got {asset_type}")
            ))
        }

        // The below code is all super BuildingFootprintWKT specific, but we Err-ed above in
        // all other cases, so this is fine for now. If we add other types here this should
        // definitely at least go in a helper

        // BINs are small <10B and inexpensive to clone
        let bin_id = String::from(identifier);

        self.db_connection.call(
            move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT the_geom FROM building_footprints WHERE bin = ?"
                ).or_else(|err| Err(AssetErr::AssetDownloadError(
                    format!("Error preparing footprint query for {bin_id:?}: {err}")
                )))?;

                stmt.query_one(
                    rusqlite::params![bin_id.as_str()],
                    |row| -> Result<String, rusqlite::Error> { row.get(0) }
                ).or_else(|err| match err {
                    rusqlite::Error::InvalidColumnType(_, _, _)
                    | rusqlite::Error::InvalidColumnIndex(_)
                    | rusqlite::Error::InvalidColumnName(_) => Err(AssetErr::AssetContentError(
                        format!("Invalid footprint database content for {bin_id:?}: {err}")
                    )),
                    rusqlite::Error::QueryReturnedNoRows => Err(AssetErr::AssetNotFound(
                        format!("{bin_id:?} not found in database")
                    )),
                    rusqlite::Error::QueryReturnedMoreThanOneRow =>
                        Err(AssetErr::AssetContentError(
                            format!("Found more than one entry for {bin_id:?}: {err}")
                        )),
                    _ => Err(AssetErr::AssetDownloadError(
                        format!("Error querying footprint for {bin_id:?}: {err}"))
                    )
                })
            }
        ).await.or_else(|err| match err {
            tokio_rusqlite::Error::Error(e) => Err(e),
            _ => Err(AssetErr::AssetDownloadError(
                format!("Error utilizing db_connection while getting footprint for {identifier:?}: {err}")
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_WKT: &str = "POLYGON((0 0, 1 0, 1 1, 0 1, 0 0))";

    async fn make_provider(rows: &[(&str, &str)]) -> NYCDOBSqliteStringProvider {
        let conn = Connection::open(":memory:").await.unwrap();
        conn.call(|c| {
            c.execute_batch(
                "CREATE TABLE building_footprints (bin TEXT, the_geom TEXT)"
            )
        }).await.unwrap();

        for (bin, geom) in rows {
            let bin = bin.to_string();
            let geom = geom.to_string();
            conn.call(move |c| {
                c.execute(
                    "INSERT INTO building_footprints (bin, the_geom) VALUES (?1, ?2)",
                    rusqlite::params![bin, geom],
                )
            }).await.unwrap();
        }

        NYCDOBSqliteStringProvider::with_connection(conn)
    }

    #[tokio::test]
    async fn returns_wkt_for_known_bin() {
        let provider = make_provider(&[("1000001", SAMPLE_WKT)]).await;
        let result = provider.get_string(AssetType::BuildingFootprintWKT, "1000001").await;
        assert_eq!(result.unwrap(), SAMPLE_WKT);
    }

    #[tokio::test]
    async fn returns_not_found_for_missing_bin() {
        let provider = make_provider(&[]).await;
        let result = provider.get_string(AssetType::BuildingFootprintWKT, "1000001").await;
        assert!(matches!(result, Err(AssetErr::AssetNotFound(_))));
    }

    #[tokio::test]
    async fn returns_content_error_for_duplicate_bin() {
        let provider = make_provider(&[
            ("1000001", SAMPLE_WKT),
            ("1000001", "POLYGON((2 2, 3 2, 3 3, 2 3, 2 2))"),
        ]).await;
        let result = provider.get_string(AssetType::BuildingFootprintWKT, "1000001").await;
        assert!(matches!(result, Err(AssetErr::AssetContentError(_))));
    }

    #[tokio::test]
    async fn returns_unsupported_type_error_for_non_footprint_asset() {
        let provider = make_provider(&[]).await;
        let result = provider.get_string(AssetType::OrthoImage, "1000001").await;
        assert!(matches!(result, Err(AssetErr::UnsupportedAssetType(_))));
    }

    #[tokio::test]
    async fn does_not_return_row_for_different_bin() {
        let provider = make_provider(&[("2000001", SAMPLE_WKT)]).await;
        let result = provider.get_string(AssetType::BuildingFootprintWKT, "1000001").await;
        assert!(matches!(result, Err(AssetErr::AssetNotFound(_))));
    }
}