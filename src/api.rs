use crate::{
    models::{HealthResponse, MetaPatch, RepoFilters, RepoIdentity, StarSyncEvent},
    openapi,
    service::StarSyncService,
};
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde::Deserialize;
use std::{convert::Infallible, net::SocketAddr, sync::Arc};
use tokio_stream::wrappers::BroadcastStream;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

#[derive(Clone)]
pub struct ApiState {
    service: Arc<StarSyncService>,
}

pub fn router(service: StarSyncService) -> Router {
    let state = ApiState {
        service: Arc::new(service),
    };
    Router::new()
        .route("/health", get(health))
        .route("/repos", get(list_repos))
        .route("/repos/{owner}/{repo}", get(get_repo))
        .route(
            "/repos/{owner}/{repo}/meta",
            get(get_meta).patch(patch_meta).delete(delete_meta),
        )
        .route("/search", get(search_repos))
        .route("/sync", post(sync_stars))
        .route("/enrich/readme", post(enrich_readme))
        .route("/events", get(events))
        .route("/openapi.json", get(openapi_json))
        .route("/openapi.yaml", get(openapi_yaml))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

pub async fn serve(service: StarSyncService) -> anyhow::Result<()> {
    let bind: SocketAddr = service.config().bind.parse()?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("StarSync REST API listening on http://{bind}");
    axum::serve(listener, router(service)).await?;
    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

async fn list_repos(
    State(state): State<ApiState>,
    Query(filters): Query<RepoFilters>,
) -> Result<Json<crate::models::ListResponse<crate::models::RepoView>>, ApiError> {
    Ok(Json(state.service.list_repos(filters)?))
}

async fn search_repos(
    State(state): State<ApiState>,
    Query(filters): Query<RepoFilters>,
) -> Result<Json<crate::models::ListResponse<crate::models::SearchResult>>, ApiError> {
    Ok(Json(state.service.search_repos(filters)?))
}

async fn get_repo(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
) -> Result<Json<crate::models::RepoView>, ApiError> {
    let identity = RepoIdentity::new(owner, repo);
    state
        .service
        .get_repo(&identity)?
        .map(Json)
        .ok_or(ApiError::not_found("repo not found"))
}

async fn get_meta(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
) -> Result<Json<crate::markdown::RepoMetaDocument>, ApiError> {
    Ok(Json(
        state.service.get_meta(&RepoIdentity::new(owner, repo))?,
    ))
}

async fn patch_meta(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
    Json(patch): Json<MetaPatch>,
) -> Result<Json<crate::markdown::RepoMetaDocument>, ApiError> {
    Ok(Json(
        state
            .service
            .patch_meta(&RepoIdentity::new(owner, repo), patch)?,
    ))
}

async fn delete_meta(
    State(state): State<ApiState>,
    Path((owner, repo)): Path<(String, String)>,
) -> Result<Json<crate::markdown::RepoMetaDocument>, ApiError> {
    Ok(Json(
        state.service.delete_meta(&RepoIdentity::new(owner, repo))?,
    ))
}

async fn sync_stars(
    State(state): State<ApiState>,
) -> Result<Json<crate::models::SyncReport>, ApiError> {
    Ok(Json(state.service.sync().await?))
}

#[derive(Debug, Deserialize)]
struct EnrichQuery {
    limit: Option<usize>,
}

async fn enrich_readme(
    State(state): State<ApiState>,
    Query(query): Query<EnrichQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let updated = state.service.enrich_readmes(query.limit).await?;
    Ok(Json(serde_json::json!({ "updated": updated })))
}

async fn events(
    State(state): State<ApiState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let stream =
        BroadcastStream::new(state.service.events().subscribe()).filter_map(|event| async move {
            let event = match event {
                Ok(event) => event,
                Err(_) => return None,
            };
            Some(Ok(Event::default()
                .event(event_name(&event))
                .data(serde_json::to_string(&event).unwrap_or_default())))
        });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn openapi_json() -> Json<serde_json::Value> {
    Json(openapi::openapi_json())
}

async fn openapi_yaml() -> Response {
    match openapi::openapi_yaml() {
        Ok(yaml) => (
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/yaml"),
            )],
            yaml,
        )
            .into_response(),
        Err(error) => ApiError::from(error).into_response(),
    }
}

fn event_name(event: &StarSyncEvent) -> &'static str {
    match event {
        StarSyncEvent::SyncStarted { .. } => "sync_started",
        StarSyncEvent::RemoteAdded { .. } => "remote_added",
        StarSyncEvent::RemoteRemoved { .. } => "remote_removed",
        StarSyncEvent::RemoteUpdated { .. } => "remote_updated",
        StarSyncEvent::MetaChanged { .. } => "meta_changed",
        StarSyncEvent::ReadmeEnriched { .. } => "readme_enriched",
        StarSyncEvent::SyncCompleted { .. } => "sync_completed",
        StarSyncEvent::StorageChanged { .. } => "storage_changed",
        StarSyncEvent::Error { .. } => "error",
    }
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(value: anyhow::Error) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: value.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({
                "error": self.message,
            })),
        )
            .into_response()
    }
}
