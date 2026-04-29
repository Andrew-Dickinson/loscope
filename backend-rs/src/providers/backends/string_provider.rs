
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

#[async_trait]
impl StringProvider for NYCDOBSqliteStringProvider {
    async fn get_string(&self, asset_type: AssetType, identifier: &str) -> Result<String, AssetErr> {
        if asset_type != AssetType::BuildingFootprintWKT {
            return Err(AssetErr::UnsupportedAssetType(
                format!("NYCDOBSqliteStringFetcher only supports BuildingFootprintWKT, got {asset_type}")
            ))
        }

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
                    rusqlite::Error::QueryReturnedNoRows => Err(AssetErr::AssetDownloadError(
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