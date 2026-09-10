use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

/// Failures surfaced by the explorer.
#[derive(Debug, Error)]
pub enum ExplorerError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("database error")]
    Database(#[from] sqlx::Error),
    #[error("database migration error")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("node RPC transport error")]
    RpcTransport(#[from] reqwest::Error),
    #[error("node RPC {code}: {message}")]
    Rpc { code: i64, message: String },
    #[error("invalid node response: {0}")]
    InvalidNodeResponse(String),
    #[error("invalid AuxPoW proof: {0}")]
    InvalidAuxPow(String),
    #[error("resource not found")]
    NotFound,
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("service is not ready: {0}")]
    NotReady(String),
}

#[derive(Debug, Serialize)]
struct ProblemDetails {
    #[serde(rename = "type")]
    problem_type: &'static str,
    title: &'static str,
    status: u16,
    detail: String,
    instance: String,
}

impl IntoResponse for ExplorerError {
    fn into_response(self) -> Response {
        let retryable_database_error = match &self {
            Self::Database(error) => is_retryable_database_error(error),
            _ => false,
        };
        let (status, problem_type, title, detail) = match self {
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "https://wcashexplorer.com/problems/not-found",
                "Not found",
                "The requested explorer resource does not exist.".to_owned(),
            ),
            Self::InvalidRequest(detail) => (
                StatusCode::BAD_REQUEST,
                "https://wcashexplorer.com/problems/invalid-request",
                "Invalid request",
                detail,
            ),
            Self::NotReady(detail) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "https://wcashexplorer.com/problems/not-ready",
                "Service not ready",
                detail,
            ),
            Self::Rpc { message, .. }
            | Self::InvalidNodeResponse(message)
            | Self::InvalidAuxPow(message) => (
                StatusCode::BAD_GATEWAY,
                "https://wcashexplorer.com/problems/upstream-data",
                "Upstream data error",
                message,
            ),
            Self::Config(message) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "https://wcashexplorer.com/problems/configuration",
                "Configuration error",
                message,
            ),
            Self::Database(_) if retryable_database_error => (
                StatusCode::SERVICE_UNAVAILABLE,
                "https://wcashexplorer.com/problems/not-ready",
                "Service not ready",
                "The explorer database is temporarily busy; retry the request.".to_owned(),
            ),
            Self::Database(_) | Self::Migration(_) | Self::RpcTransport(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "https://wcashexplorer.com/problems/internal",
                "Internal service error",
                "The explorer could not complete this request.".to_owned(),
            ),
        };
        let body = ProblemDetails {
            problem_type,
            title,
            status: status.as_u16(),
            detail,
            instance: format!("urn:uuid:{}", Uuid::new_v4()),
        };
        let mut response = (
            status,
            [(http::header::CONTENT_TYPE, "application/problem+json")],
            Json(body),
        )
            .into_response();
        if retryable_database_error {
            response.headers_mut().insert(
                http::header::RETRY_AFTER,
                "2".parse().expect("valid header"),
            );
        }
        response
    }
}

fn is_retryable_database_error(error: &sqlx::Error) -> bool {
    if matches!(
        error,
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed | sqlx::Error::WorkerCrashed
    ) {
        return true;
    }
    error
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .is_some_and(|code| matches!(code.as_ref(), "55P03" | "57014" | "53300" | "57P03"))
}

/// Explorer result alias.
pub type Result<T> = std::result::Result<T, ExplorerError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_database_failures_are_retryable_without_internal_details() {
        let response = ExplorerError::Database(sqlx::Error::PoolTimedOut).into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.headers().get(http::header::RETRY_AFTER),
            Some(&http::HeaderValue::from_static("2"))
        );
    }
}
