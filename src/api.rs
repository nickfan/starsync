use crate::{
    models::{
        EventSubscriptionCreate, EventSubscriptionPatch, HealthResponse, MetaPatch, RepoFilters,
        RepoIdentity,
    },
    openapi,
    service::StarSyncService,
};
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Redirect, Response,
    },
    routing::{get, patch, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde::Deserialize;
use std::{
    convert::Infallible,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::Arc,
};
use tokio_stream::wrappers::BroadcastStream;
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

#[derive(Clone)]
pub struct ApiState {
    service: Arc<StarSyncService>,
}

pub fn router(service: StarSyncService) -> Router {
    api_router(service)
}

fn api_router(service: StarSyncService) -> Router {
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
        .route("/events/recent", get(recent_events))
        .route(
            "/event-subscriptions",
            get(list_event_subscriptions).post(create_event_subscription),
        )
        .route(
            "/event-subscriptions/{id}",
            patch(update_event_subscription).delete(delete_event_subscription),
        )
        .route("/openapi.json", get(openapi_json))
        .route("/openapi.yaml", get(openapi_yaml))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

fn router_with_ui(service: StarSyncService, ui_dir: Option<PathBuf>) -> Router {
    let app = api_router(service);
    let Some(ui_dir) = ui_dir else {
        return app;
    };
    let index = ui_dir.join("index.html");
    let ui_service = ServeDir::new(&ui_dir)
        .append_index_html_on_directories(true)
        .fallback(ServeFile::new(index));
    app.route("/", get(ui_home)).nest_service("/ui", ui_service)
}

pub async fn serve(service: StarSyncService) -> anyhow::Result<()> {
    let config = service.config().clone();
    let ui_dir = if config.ui_enabled {
        let status = crate::ui::prepare_ui(&config)?;
        if status.overwritten {
            if let Some(backup_dir) = &status.backup_dir {
                println!(
                    "StarSync Web UI refreshed at {} after backing up old UI to {}",
                    status.dir.display(),
                    backup_dir.display()
                );
                tracing::info!(
                    ui_dir = %status.dir.display(),
                    backup_dir = %backup_dir.display(),
                    "StarSync Web UI refreshed with backup"
                );
            } else {
                println!(
                    "StarSync Web UI refreshed at {} without backup",
                    status.dir.display()
                );
                tracing::info!(
                    ui_dir = %status.dir.display(),
                    "StarSync Web UI refreshed without backup"
                );
            }
        } else if status.extracted {
            println!("StarSync Web UI extracted to {}", status.dir.display());
            tracing::info!(ui_dir = %status.dir.display(), "StarSync Web UI extracted");
        }
        Some(status.dir)
    } else {
        None
    };

    let bind: SocketAddr = config.bind.parse()?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    println!("StarSync REST API listening on {}", local_url(bind, ""));
    tracing::info!("StarSync REST API listening on http://{bind}");
    if ui_dir.is_some() {
        println!("StarSync Web UI available at {}", local_url(bind, "/ui/"));
    }
    axum::serve(listener, router_with_ui(service, ui_dir)).await?;
    Ok(())
}

async fn ui_home() -> Redirect {
    Redirect::temporary("/ui/")
}

fn local_url(bind: SocketAddr, path: &str) -> String {
    let host = match bind.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => "127.0.0.1".to_string(),
        IpAddr::V6(ip) if ip.is_unspecified() => "[::1]".to_string(),
        IpAddr::V6(ip) => format!("[{ip}]"),
        IpAddr::V4(ip) => ip.to_string(),
    };
    format!("http://{host}:{}{}", bind.port(), path)
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
                .event(event.name.clone())
                .data(serde_json::to_string(&event).unwrap_or_default())))
        });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(Debug, Deserialize)]
struct RecentEventsQuery {
    limit: Option<usize>,
}

async fn recent_events(
    State(state): State<ApiState>,
    Query(query): Query<RecentEventsQuery>,
) -> Result<Json<Vec<crate::models::EventEnvelope>>, ApiError> {
    Ok(Json(
        state.service.recent_events(query.limit.unwrap_or(50))?,
    ))
}

async fn list_event_subscriptions(
    State(state): State<ApiState>,
) -> Json<Vec<crate::models::EventSubscriptionView>> {
    Json(state.service.list_event_subscriptions())
}

async fn create_event_subscription(
    State(state): State<ApiState>,
    Json(create): Json<EventSubscriptionCreate>,
) -> Result<Json<crate::models::EventSubscriptionView>, ApiError> {
    Ok(Json(state.service.create_event_subscription(create)?))
}

async fn update_event_subscription(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(patch): Json<EventSubscriptionPatch>,
) -> Result<Json<crate::models::EventSubscriptionView>, ApiError> {
    Ok(Json(
        state
            .service
            .patch_event_subscription(&id, patch)
            .map_err(event_subscription_error)?,
    ))
}

async fn delete_event_subscription(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<crate::models::EventSubscriptionView>, ApiError> {
    Ok(Json(
        state
            .service
            .delete_event_subscription(&id)
            .map_err(event_subscription_error)?,
    ))
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

fn event_subscription_error(error: anyhow::Error) -> ApiError {
    let message = error.to_string();
    if message.contains("event subscription not found") {
        ApiError::not_found(message)
    } else {
        ApiError::from(anyhow::anyhow!(message))
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
