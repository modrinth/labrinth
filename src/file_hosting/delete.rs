use crate::file_hosting::{AuthorizationData, FileHostingError};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DeleteFileData {
    pub file_id: String,
    pub file_name: String,
}

#[cfg(feature = "backblaze")]
pub async fn delete_file_version(
    authorization_data: AuthorizationData,
    file_id: String,
    file_name: String,
) -> Result<DeleteFileData, FileHostingError> {
    Ok(reqwest::Client::new()
        .post(
            &format!(
                "{}/b2api/v2/b2_delete_file_version",
                authorization_data.api_url
            )
            .to_string(),
        )
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(
            reqwest::header::AUTHORIZATION,
            authorization_data.authorization_token,
        )
        .body(
            serde_json::json!({
                "fileName": file_name,
                "fileId": file_id
            })
            .to_string(),
        )
        .send()
        .await?
        .json()
        .await?)
}

#[cfg(not(feature = "backblaze"))]
pub async fn delete_file_version(
    _authorization_data: AuthorizationData,
    file_id: String,
    file_name: String,
) -> Result<DeleteFileData, FileHostingError> {
    let path = std::path::Path::new(&dotenv::var("MOCK_FILE_PATH").unwrap())
        .join(file_name.replace("../", ""));
    std::fs::remove_file(path)?;
    Ok(DeleteFileData { file_id, file_name })
}
