//! Local HTTP API for the background service.
//!
//! Endpoints (all bound to 127.0.0.1):
//!
//! - `GET  /health`                 — liveness check
//! - `GET  /whoami`                 — current identity (from cached connection)
//! - `GET  /secrets`                — list secret metadata
//! - `GET  /secrets/:env`           — reveal a single secret value
//! - `POST /secrets`                — create / update a secret
//! - `DELETE /secrets/:env`         — delete a secret
//! - `GET  /files`                  — list files / folders
//! - `GET  /sync`                   — current local sync selection
//! - `POST /sync`                   — add / remove items from the selection

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::bindings::*;

use crate::auth::conn::ConnState;
use crate::store::sync::SyncSelection;

pub type SharedConn = Option<Arc<ConnState>>;

pub fn router(conn: SharedConn) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/whoami", get(whoami))
        .route("/secrets", get(list_secrets).post(create_secret))
        .route(
            "/secrets/:env",
            get(reveal_secret_route).delete(delete_secret_route),
        )
        .route("/files", get(list_files))
        .route("/sync", get(sync_status).post(sync_toggle))
        .with_state(AppState { conn })
}

#[derive(Clone)]
struct AppState {
    conn: SharedConn,
}

#[derive(Serialize)]
struct HealthResponse {
    ok: bool,
    service: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        service: "spacenix",
    })
}

#[derive(Serialize)]
struct WhoamiResponse {
    identity: Option<String>,
    signed_in: bool,
}

async fn whoami(State(state): State<AppState>) -> Json<WhoamiResponse> {
    let identity = state
        .conn
        .as_ref()
        .and_then(|c| c.identity())
        .map(|i| i.to_hex().to_string());
    let signed_in = identity.is_some();
    Json(WhoamiResponse {
        identity,
        signed_in,
    })
}

#[derive(Serialize)]
struct SecretDto {
    id: u64,
    env: String,
    device_ids: Vec<String>,
    permissions: Vec<String>,
}

async fn list_secrets(
    State(state): State<AppState>,
) -> Result<Json<Vec<SecretDto>>, axum::http::StatusCode> {
    let Some(conn) = state.conn.as_ref() else {
        return Err(axum::http::StatusCode::SERVICE_UNAVAILABLE);
    };
    let rows: Vec<SecretDto> = conn
        .conn
        .db()
        .my_secrets()
        .iter()
        .map(|s| SecretDto {
            id: s.id,
            env: s.env,
            device_ids: s.device_ids,
            permissions: s.permissions,
        })
        .collect();
    Ok(Json(rows))
}

#[derive(Serialize)]
struct SecretValueDto {
    env: String,
    value: String,
}

async fn reveal_secret_route(
    State(state): State<AppState>,
    Path(env): Path<String>,
) -> Result<Json<SecretValueDto>, axum::http::StatusCode> {
    let Some(conn) = state.conn.as_ref() else {
        return Err(axum::http::StatusCode::SERVICE_UNAVAILABLE);
    };
    let id = conn
        .conn
        .db()
        .my_secrets()
        .iter()
        .find(|s| s.env == env)
        .map(|s| s.id)
        .ok_or(axum::http::StatusCode::NOT_FOUND)?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    conn.conn
        .procedures()
        .reveal_secret_then(id, move |_ctx, res| {
            let _ = tx.send(res);
        });
    let res = rx
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let value = res
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|err| {
            tracing::error!(?err, "reveal_secret rejected");
            axum::http::StatusCode::BAD_REQUEST
        })?
        .ok_or(axum::http::StatusCode::NOT_FOUND)?;
    Ok(Json(SecretValueDto {
        env: value.env,
        value: value.value,
    }))
}

#[derive(Deserialize)]
struct CreateSecretBody {
    env: String,
    value: String,
    #[serde(default)]
    device_ids: Vec<String>,
    #[serde(default)]
    permissions: Vec<String>,
}

async fn create_secret(
    State(state): State<AppState>,
    Json(body): Json<CreateSecretBody>,
) -> Result<Json<SecretDto>, axum::http::StatusCode> {
    let Some(conn) = state.conn.as_ref() else {
        return Err(axum::http::StatusCode::SERVICE_UNAVAILABLE);
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    conn.conn
        .reducers()
        .set_secret_then(
            body.env.clone(),
            body.value,
            body.device_ids.clone(),
            body.permissions.clone(),
            move |_ctx, res| {
                let _ = tx.send(res);
            },
        )
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let res = rx
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    res.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|err| {
            tracing::error!(?err, "set_secret rejected");
            axum::http::StatusCode::BAD_REQUEST
        })?;
    let row = conn
        .conn
        .db()
        .my_secrets()
        .iter()
        .find(|s| s.env == body.env)
        .ok_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(SecretDto {
        id: row.id,
        env: row.env,
        device_ids: row.device_ids,
        permissions: row.permissions,
    }))
}

async fn delete_secret_route(
    State(state): State<AppState>,
    Path(env): Path<String>,
) -> Result<axum::http::StatusCode, axum::http::StatusCode> {
    let Some(conn) = state.conn.as_ref() else {
        return Err(axum::http::StatusCode::SERVICE_UNAVAILABLE);
    };
    let id = conn
        .conn
        .db()
        .my_secrets()
        .iter()
        .find(|s| s.env == env)
        .map(|s| s.id)
        .ok_or(axum::http::StatusCode::NOT_FOUND)?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    conn.conn
        .reducers()
        .delete_secret_then(id, move |_ctx, res| {
            let _ = tx.send(res);
        })
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    let res = rx
        .await
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    res.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|err| {
            tracing::error!(?err, "delete_secret rejected");
            axum::http::StatusCode::BAD_REQUEST
        })?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct FileDto {
    #[allow(dead_code)]
    id: u64,
    name: String,
    path: Option<String>,
    size_bytes: u64,
    is_directory: bool,
    content_type: Option<String>,
    updated_at_micros: i64,
}

async fn list_files(
    State(state): State<AppState>,
) -> Result<Json<Vec<FileDto>>, axum::http::StatusCode> {
    let Some(conn) = state.conn.as_ref() else {
        return Err(axum::http::StatusCode::SERVICE_UNAVAILABLE);
    };
    let rows: Vec<FileDto> = conn
        .conn
        .db()
        .my_files()
        .iter()
        .map(|f| FileDto {
            id: f.id,
            name: f.name,
            path: f.tree_path,
            size_bytes: f.size_bytes,
            is_directory: f.is_directory,
            content_type: f.content_type,
            updated_at_micros: f.updated_at.to_micros_since_unix_epoch(),
        })
        .collect();
    Ok(Json(rows))
}

async fn sync_status() -> Result<Json<SyncSelection>, axum::http::StatusCode> {
    // The service doesn't track which config it was started with; the CLI
    // reads from disk directly. We return an empty selection here and let
    // the CLI append to it via the service.
    Ok(Json(SyncSelection::default()))
}

#[derive(Deserialize)]
struct SyncToggleBody {
    #[allow(dead_code)]
    id: u64,
}

async fn sync_toggle(Json(_body): Json<SyncToggleBody>) -> axum::http::StatusCode {
    // The actual selection file is owned by the user; the service can be
    // queried but mutations go through the CLI which writes sync.toml.
    axum::http::StatusCode::NOT_IMPLEMENTED
}
