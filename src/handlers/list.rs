use axum::{
    body::Body,
    extract::{Path, State, Query},
    http::{header},
    response::Response,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::SystemTime;
use crate::state::AppState;
use crate::error::S3ErrorType;
use utoipa::{IntoParams, ToSchema};

#[derive(Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "kebab-case")]
pub struct ListObjectsParams {
    /// Only return object keys that start with this prefix.
    pub prefix: Option<String>,
    /// Use a delimiter to group common prefixes.
    pub delimiter: Option<String>,
    /// Compatibility marker for legacy list requests.
    pub marker: Option<String>,
    /// Maximum number of keys to return.
    pub max_keys: Option<usize>,
    /// Use `2` to request the simplified ListObjectsV2-compatible mode.
    #[serde(rename = "list-type")]
    pub list_type: Option<u8>,
    /// Compatibility token accepted by the simplified V2 mode.
    pub continuation_token: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename = "ListBucketResult")]
pub struct ListBucketResult {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Prefix")]
    pub prefix: String,
    #[serde(rename = "Marker", skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    #[serde(rename = "NextMarker", skip_serializing_if = "Option::is_none")]
    pub next_marker: Option<String>,
    #[serde(rename = "MaxKeys")]
    pub max_keys: usize,
    #[serde(rename = "Delimiter", skip_serializing_if = "Option::is_none")]
    pub delimiter: Option<String>,
    #[serde(rename = "IsTruncated")]
    pub is_truncated: bool,
    #[serde(rename = "Contents")]
    pub contents: Vec<ObjectContent>,
    #[serde(rename = "CommonPrefixes", skip_serializing_if = "Vec::is_empty")]
    pub common_prefixes: Vec<CommonPrefix>,
    // V2 fields
    #[serde(rename = "KeyCount", skip_serializing_if = "Option::is_none")]
    pub key_count: Option<usize>,
    #[serde(rename = "ContinuationToken", skip_serializing_if = "Option::is_none")]
    pub continuation_token: Option<String>,
    #[serde(rename = "NextContinuationToken", skip_serializing_if = "Option::is_none")]
    pub next_continuation_token: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ObjectContent {
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "LastModified")]
    pub last_modified: String,
    #[serde(rename = "ETag")]
    pub etag: String,
    #[serde(rename = "Size")]
    pub size: u64,
    #[serde(rename = "StorageClass")]
    pub storage_class: String,
}

#[derive(Serialize, ToSchema)]
pub struct CommonPrefix {
    #[serde(rename = "Prefix")]
    pub prefix: String,
}

pub async fn list_objects(
    Path(bucket): Path<String>,
    Query(params): Query<ListObjectsParams>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let storage = match state.storage_map.get(&bucket) {
        Some(s) => s,
        None => return S3ErrorType::NoSuchBucket.to_response(Some(bucket)),
    };

    let prefix = params.prefix.unwrap_or_default();
    let delimiter = params.delimiter;
    let max_keys = params.max_keys.unwrap_or(1000);
    
    let mut contents = Vec::new();
    let mut common_prefixes = std::collections::BTreeSet::new();
    
    let mut walker = walkdir::WalkDir::new(storage).into_iter();
    let mut is_truncated = false;

    while let Some(Ok(entry)) = walker.next() {
        if entry.file_type().is_dir() {
            continue;
        }

        let rel_path = entry.path().strip_prefix(storage).unwrap();
        let key = rel_path.to_string_lossy().replace("\\", "/");
        
        if !key.starts_with(&prefix) {
            continue;
        }

        // Handle Delimiter
        if let Some(ref d) = delimiter {
            let relative_to_prefix = &key[prefix.len()..];
            if let Some(idx) = relative_to_prefix.find(d) {
                let common_prefix = format!("{}{}{}", prefix, &relative_to_prefix[..idx], d);
                if !common_prefixes.contains(&common_prefix) {
                    if contents.len() + common_prefixes.len() >= max_keys {
                        is_truncated = true;
                        break;
                    }
                    common_prefixes.insert(common_prefix);
                }
                continue;
            }
        }

        if contents.len() + common_prefixes.len() >= max_keys {
            is_truncated = true;
            break;
        }

        let metadata = entry.metadata().unwrap();
        let mod_time: DateTime<Utc> = metadata.modified().unwrap_or(SystemTime::now()).into();
        let etag = format!("\"{:x}-{:x}\"", mod_time.timestamp_nanos_opt().unwrap_or(0), metadata.len());

        contents.push(ObjectContent {
            key,
            last_modified: mod_time.to_rfc3339(),
            etag,
            size: metadata.len(),
            storage_class: "STANDARD".to_string(),
        });
    }

    let key_count = contents.len() + common_prefixes.len();

    let result = ListBucketResult {
        xmlns: "http://s3.amazonaws.com/doc/2006-03-01/".to_string(),
        name: bucket,
        prefix,
        marker: params.marker,
        next_marker: None, // Intentional: true pagination to be implemented later
        max_keys,
        delimiter,
        is_truncated,
        contents,
        common_prefixes: common_prefixes.into_iter().map(|p| CommonPrefix { prefix: p }).collect(),
        key_count: if params.list_type == Some(2) { Some(key_count) } else { None },
        continuation_token: params.continuation_token,
        next_continuation_token: None, // Intentional: true pagination to be implemented later
    };

    let xml = quick_xml::se::to_string(&result).unwrap();
    Response::builder()
        .header(header::CONTENT_TYPE, "application/xml")
        .body(Body::from(xml))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::collections::HashMap;
    use dashmap::DashMap;
    use tokio::fs;
    use axum::body::to_bytes;
    use crate::config::{Config, BucketConfig};

    async fn setup_test_state(bucket_name: &str, storage_path: &str) -> Arc<AppState> {
        let mut storage_map = HashMap::new();
        storage_map.insert(bucket_name.to_string(), PathBuf::from(storage_path));
        
        let config = Config {
            port: 8080,
            endpoint: "0.0.0.0".to_string(),
            verbose: false,
            cache_size: 10,
            auth: None,
            buckets: vec![BucketConfig { name: bucket_name.to_string(), storage: storage_path.to_string() }],
        };

        Arc::new(AppState {
            config,
            cache: DashMap::new(),
            storage_map,
        })
    }

    async fn create_test_files(base: &str, files: &[&str]) {
        for f in files {
            let path = PathBuf::from(base).join(f);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await.unwrap();
            }
            fs::write(&path, "data").await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_list_objects_truncation_and_keycount() {
        let storage = "./test_list_data";
        let bucket = "test_bucket";
        let _ = fs::remove_dir_all(storage).await;
        fs::create_dir_all(storage).await.unwrap();

        create_test_files(storage, &["a.txt", "b.txt", "c.txt"]).await;

        let state = setup_test_state(bucket, storage).await;

        // Test max_keys = 2 (Should be truncated)
        let params = ListObjectsParams {
            prefix: None,
            delimiter: None,
            marker: None,
            max_keys: Some(2),
            list_type: Some(2),
            continuation_token: None,
        };
        let response = list_objects(Path(bucket.to_string()), Query(params), State(state.clone())).await;
        let (_, body) = response.into_parts();
        let xml = String::from_utf8(to_bytes(body, usize::MAX).await.unwrap().to_vec()).unwrap();

        assert!(xml.contains("<IsTruncated>true</IsTruncated>"));
        assert!(xml.contains("<KeyCount>2</KeyCount>"));

        // Test max_keys = 3 (Exactly matching total files, Should NOT be truncated)
        let params_exact = ListObjectsParams {
            prefix: None,
            delimiter: None,
            marker: None,
            max_keys: Some(3),
            list_type: Some(2),
            continuation_token: None,
        };
        let response_exact = list_objects(Path(bucket.to_string()), Query(params_exact), State(state.clone())).await;
        let (_, body_exact) = response_exact.into_parts();
        let xml_exact = String::from_utf8(to_bytes(body_exact, usize::MAX).await.unwrap().to_vec()).unwrap();

        assert!(xml_exact.contains("<IsTruncated>false</IsTruncated>"));
        assert!(xml_exact.contains("<KeyCount>3</KeyCount>"));

        let _ = fs::remove_dir_all(storage).await;
    }

    #[tokio::test]
    async fn test_list_objects_delimiter() {
        let storage = "./test_list_data_delim";
        let bucket = "test_bucket";
        let _ = fs::remove_dir_all(storage).await;
        fs::create_dir_all(storage).await.unwrap();

        create_test_files(storage, &["folder1/a.txt", "folder1/b.txt", "folder2/c.txt", "root.txt"]).await;

        let state = setup_test_state(bucket, storage).await;

        let params = ListObjectsParams {
            prefix: None,
            delimiter: Some("/".to_string()),
            marker: None,
            max_keys: Some(10),
            list_type: Some(2),
            continuation_token: None,
        };
        
        let response = list_objects(Path(bucket.to_string()), Query(params), State(state.clone())).await;
        let (_, body) = response.into_parts();
        let xml = String::from_utf8(to_bytes(body, usize::MAX).await.unwrap().to_vec()).unwrap();

        assert!(xml.contains("<Key>root.txt</Key>"));
        assert!(xml.contains("<Prefix>folder1/</Prefix>"));
        assert!(xml.contains("<Prefix>folder2/</Prefix>"));
        assert!(xml.contains("<KeyCount>3</KeyCount>"));
        assert!(xml.contains("<IsTruncated>false</IsTruncated>"));

        let _ = fs::remove_dir_all(storage).await;
    }
}
