use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, StatusCode, HeaderMap},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::fs::{self, File};
use tokio::io::{self, AsyncSeekExt, AsyncReadExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;
use urlencoding::decode;
use crate::state::AppState;
use crate::cache::CachedStat;
use crate::error::S3ErrorType;
use futures_util::StreamExt;

pub async fn get_object(
    Path((bucket, key)): Path<(String, String)>,
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Response {
    let key = match decode(&key) {
        Ok(k) => k.into_owned(),
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    let storage = match state.storage_map.get(&bucket) {
        Some(s) => s,
        None => return S3ErrorType::NoSuchBucket.to_response(Some(bucket)),
    };

    let path = storage.join(key.trim_start_matches('/'));
    let mut file = match File::open(&path).await {
        Ok(f) => f,
        Err(_) => return S3ErrorType::NoSuchKey.to_response(Some(key)),
    };

    let metadata = match file.metadata().await {
        Ok(m) => m,
        Err(_) => return S3ErrorType::InternalError.to_response(None),
    };

    let size = metadata.len();
    let mod_time: DateTime<Utc> = metadata.modified().unwrap_or(SystemTime::now()).into();
    let etag = format!("\"{:x}-{:x}\"", mod_time.timestamp_nanos_opt().unwrap_or(0), size);

    // Handle Range Header
    if let Some(range_header) = headers.get(header::RANGE).and_then(|h| h.to_str().ok()) {
        if let Some(range) = parse_range(range_header, size) {
            let (start, end) = range;
            let range_size = end - start + 1;
            
            if file.seek(io::SeekFrom::Start(start)).await.is_ok() {
                let stream = ReaderStream::new(file.take(range_size));
                return Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(header::CONTENT_TYPE, "application/octet-stream")
                    .header(header::CONTENT_LENGTH, range_size)
                    .header(header::CONTENT_RANGE, format!("bytes {}-{}/{}", start, end, size))
                    .header(header::ETAG, etag)
                    .header("Last-Modified", mod_time.to_rfc2822())
                    .body(Body::from_stream(stream))
                    .unwrap();
            }
        }
    }

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, size)
        .header(header::ETAG, etag)
        .header("Last-Modified", mod_time.to_rfc2822())
        .body(body)
        .unwrap()
}

pub async fn head_object(
    Path((bucket, key)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let key = match decode(&key) {
        Ok(k) => k.into_owned(),
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    let cache_key = format!("{}/{}", bucket, key);
    if let Some(cached) = state.cache.get(&cache_key) {
        return Response::builder()
            .header(header::CONTENT_LENGTH, cached.size)
            .header(header::ETAG, &cached.etag)
            .header("Last-Modified", cached.mod_time.to_rfc2822())
            .body(Body::empty())
            .unwrap();
    }

    let storage = match state.storage_map.get(&bucket) {
        Some(s) => s,
        None => return S3ErrorType::NoSuchBucket.to_response(Some(bucket)),
    };

    let path = storage.join(key.trim_start_matches('/'));
    let metadata = match fs::metadata(&path).await {
        Ok(m) => m,
        Err(_) => return S3ErrorType::NoSuchKey.to_response(Some(key)),
    };

    if metadata.is_dir() {
         return S3ErrorType::NoSuchKey.to_response(Some(key));
    }

    let size = metadata.len();
    let mod_time: DateTime<Utc> = metadata.modified().unwrap_or(SystemTime::now()).into();
    let etag = format!("\"{:x}-{:x}\"", mod_time.timestamp_nanos_opt().unwrap_or(0), size);

    state.cache.insert(cache_key, CachedStat {
        size,
        mod_time,
        etag: etag.clone(),
    });

    Response::builder()
        .header(header::CONTENT_LENGTH, size)
        .header(header::ETAG, etag)
        .header("Last-Modified", mod_time.to_rfc2822())
        .body(Body::empty())
        .unwrap()
}

pub async fn put_object(
    Path((bucket, key)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    body: Body,
) -> Response {
    let key = match decode(&key) {
        Ok(k) => k.into_owned(),
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    let storage = match state.storage_map.get(&bucket) {
        Some(s) => s,
        None => return S3ErrorType::NoSuchBucket.to_response(Some(bucket)),
    };

    let path = storage.join(key.trim_start_matches('/'));
    
    // Create parent directories
    if let Some(parent) = path.parent() {
        if let Err(_) = fs::create_dir_all(parent).await {
            return S3ErrorType::InternalError.to_response(None);
        }
    }

    let mut file = match File::create(&path).await {
        Ok(f) => f,
        Err(_) => return S3ErrorType::InternalError.to_response(None),
    };

    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(data) => {
                if let Err(_) = file.write_all(&data).await {
                    return S3ErrorType::InternalError.to_response(None);
                }
            }
            Err(_) => return S3ErrorType::InternalError.to_response(None),
        }
    }

    // Invalidate cache
    state.cache.remove(&format!("{}/{}", bucket, key));

    StatusCode::OK.into_response()
}

pub async fn delete_object(
    Path((bucket, key)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let key = match decode(&key) {
        Ok(k) => k.into_owned(),
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    let storage = match state.storage_map.get(&bucket) {
        Some(s) => s,
        None => return S3ErrorType::NoSuchBucket.to_response(Some(bucket)),
    };

    let path = storage.join(key.trim_start_matches('/'));
    if let Err(_) = fs::remove_file(&path).await {
        // S3 returns 204 even if file doesn't exist during DELETE
        return StatusCode::NO_CONTENT.into_response();
    }

    state.cache.remove(&format!("{}/{}", bucket, key));
    StatusCode::NO_CONTENT.into_response()
}

fn parse_range(range_header: &str, file_size: u64) -> Option<(u64, u64)> {
    if !range_header.starts_with("bytes=") { return None; }
    let range_str = &range_header[6..];
    let parts: Vec<&str> = range_str.split('-').collect();
    if parts.len() != 2 { return None; }

    let start = parts[0].parse::<u64>().ok()?;
    let end = if parts[1].is_empty() {
        file_size - 1
    } else {
        parts[1].parse::<u64>().ok()?
    };

    if start <= end && end < file_size {
        Some((start, end))
    } else {
        None
    }
}
