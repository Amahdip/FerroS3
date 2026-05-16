mod config;
mod cache;
mod state;
mod error;
mod handlers;
mod auth;

use axum::{
    routing::get,
    middleware,
    Router,
};
use dashmap::DashMap;
use std::{
    collections::HashMap,
    path::{PathBuf},
    sync::Arc,
};
use tokio::fs;

use crate::config::Config;
use crate::state::AppState;
use crate::handlers::object::{get_object, head_object, put_object, delete_object};
use crate::handlers::list::list_objects;
use crate::handlers::bucket::{head_bucket, list_buckets};
use crate::handlers::admin::generate_presigned_url;
use crate::auth::auth_middleware;

#[tokio::main]
async fn main() {
    let config_path = "config.yaml";
    let config_str = fs::read_to_string(config_path).await.expect("Failed to read config.yaml");
    let config: Config = serde_yaml::from_str(&config_str).expect("Failed to parse config.yaml");

    let mut storage_map = HashMap::new();
    for b in &config.buckets {
        storage_map.insert(b.name.clone(), PathBuf::from(&b.storage));
    }

    let state = Arc::new(AppState {
        config: config.clone(),
        cache: DashMap::with_capacity(config.cache_size),
        storage_map,
    });

    let app = Router::new()
        .route("/", get(list_buckets))
        .route("/_admin/presign", axum::routing::post(generate_presigned_url))
        .route("/:bucket/", get(list_objects).head(head_bucket))
        .route("/:bucket/*key", get(get_object).head(head_object).put(put_object).delete(delete_object))
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware))
        .with_state(state);

    let addr = format!("{}:{}", config.endpoint, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("Rust S3 Proxy listening on http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Request, StatusCode};
    use tower::util::ServiceExt;
    use crate::config::{BucketConfig, AuthConfig};
    use axum::body::Body;

    fn setup_test_state() -> Arc<AppState> {
        let config = Config {
            port: 8080,
            endpoint: "0.0.0.0".to_string(),
            verbose: false,
            cache_size: 10,
            auth: Some(AuthConfig {
                access_key: "test_key".to_string(),
                secret_key: "test_secret".to_string(),
            }),
            buckets: vec![BucketConfig {
                name: "test_bucket".to_string(),
                storage: "./test_data".to_string(),
            }],
        };

        let mut storage_map = HashMap::new();
        storage_map.insert("test_bucket".to_string(), PathBuf::from("./test_data"));

        Arc::new(AppState {
            config,
            cache: DashMap::new(),
            storage_map,
        })
    }

    #[tokio::test]
    async fn test_auth_failure() {
        let state = setup_test_state();
        let app = Router::new()
            .route("/:bucket/", get(list_objects))
            .layer(middleware::from_fn_with_state(state.clone(), auth_middleware))
            .with_state(state);

        let response = app
            .oneshot(Request::builder().uri("/test_bucket/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_auth_success() {
        let state = setup_test_state();
        let app = Router::new()
            .route("/:bucket/", get(list_objects))
            .layer(middleware::from_fn_with_state(state.clone(), auth_middleware))
            .with_state(state);

        // Create test directory if not exists
        fs::create_dir_all("./test_data").await.unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/test_bucket/")
                    .header("Authorization", "test_key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        
        // Clean up
        let _ = fs::remove_dir_all("./test_data").await;
    }

    #[tokio::test]
    async fn test_put_and_delete_object() {
        let state = setup_test_state();
        let app = Router::new()
            .route("/:bucket/*key", get(get_object).put(put_object).delete(delete_object))
            .with_state(state);

        fs::create_dir_all("./test_data").await.unwrap();

        // 1. Put Object
        let response = app.clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/test_bucket/new_file.txt")
                    .body(Body::from("hello world"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Verify file exists
        assert!(fs::metadata("./test_data/new_file.txt").await.is_ok());

        // 2. Delete Object
        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/test_bucket/new_file.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Verify file is gone
        assert!(fs::metadata("./test_data/new_file.txt").await.is_err());

        // Clean up
        let _ = fs::remove_dir_all("./test_data").await;
    }
}
