use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use quick_xml::se::to_string;

#[derive(Debug, Serialize)]
#[serde(rename = "Error")]
pub struct S3Error {
    #[serde(rename = "Code")]
    pub code: String,
    #[serde(rename = "Message")]
    pub message: String,
    #[serde(rename = "Resource", skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(rename = "RequestId")]
    pub request_id: String,
}

pub enum S3ErrorType {
    NoSuchKey,
    NoSuchBucket,
    AccessDenied,
    InternalError,
    // Add more as needed
}

impl S3ErrorType {
    pub fn to_response(&self, resource: Option<String>) -> Response {
        let (status, code, message) = match self {
            S3ErrorType::NoSuchKey => (
                StatusCode::NOT_FOUND,
                "NoSuchKey",
                "The specified key does not exist.",
            ),
            S3ErrorType::NoSuchBucket => (
                StatusCode::NOT_FOUND,
                "NoSuchBucket",
                "The specified bucket does not exist.",
            ),
            S3ErrorType::AccessDenied => (
                StatusCode::FORBIDDEN,
                "AccessDenied",
                "Access Denied.",
            ),
            S3ErrorType::InternalError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "InternalError",
                "An internal error occurred.",
            ),
        };

        let err = S3Error {
            code: code.to_string(),
            message: message.to_string(),
            resource,
            request_id: uuid::Uuid::new_v4().to_string(),
        };

        let xml = to_string(&err).unwrap_or_default();
        (status, [("Content-Type", "application/xml")], xml).into_response()
    }
}
