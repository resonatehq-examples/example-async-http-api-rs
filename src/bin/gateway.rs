use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use resonate::prelude::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Deserialize)]
struct BeginQuery {
    id: Option<String>,
}

#[derive(Deserialize)]
struct WaitQuery {
    id: Option<String>,
}

#[derive(Clone)]
struct AppState {
    resonate: Arc<Resonate>,
}

#[tokio::main]
async fn main() {
    let resonate = Resonate::new(ResonateConfig {
        url: Some("http://localhost:8001".into()),
        group: Some("gateway".into()),
        ..Default::default()
    });

    let state = AppState {
        resonate: Arc::new(resonate),
    };

    let app = Router::new()
        .route("/begin", post(begin))
        .route("/wait", get(wait))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:5001")
        .await
        .expect("failed to bind 127.0.0.1:5001");
    println!("gateway listening on http://127.0.0.1:5001");
    axum::serve(listener, app)
        .await
        .expect("axum server crashed");
}

/// Start (or reconnect to) a durable execution and return immediately.
///
/// The gateway uses `rpc(...).spawn()` to dispatch work to a worker without
/// awaiting completion. Resonate deduplicates by promise id: calling `/begin`
/// twice with the same `id` reconnects to the in-flight (or already completed)
/// execution rather than starting a new one.
async fn begin(
    State(state): State<AppState>,
    Query(q): Query<BeginQuery>,
    body: Option<Json<Value>>,
) -> impl IntoResponse {
    let id = q.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let data = match body {
        Some(Json(v)) if !v.is_null() => v,
        _ => json!({ "foo": "bar" }),
    };

    match state
        .resonate
        .rpc::<_, Value>(&id, "foo", data)
        .target("poll://any@worker")
        .spawn()
        .await
    {
        Ok(handle) => (
            StatusCode::OK,
            Json(json!({
                "promise": handle.id,
                "status": "pending",
                "wait": format!("/wait?id={}", id),
            })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("failed_to_begin: {err}") })),
        )
            .into_response(),
    }
}

/// Poll the status of a durable execution by id without blocking the gateway.
///
/// `resonate.get(id)` returns a handle to the existing promise. `handle.done()`
/// is non-blocking — it returns whether the promise has completed yet. Clients
/// call `/wait` repeatedly until status becomes `resolved` or `rejected`.
async fn wait(State(state): State<AppState>, Query(q): Query<WaitQuery>) -> impl IntoResponse {
    let id = match q.id {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "id query parameter is required" })),
            )
                .into_response();
        }
    };

    let handle = match state.resonate.get::<Value>(&id).await {
        Ok(h) => h,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("{id} not found") })),
            )
                .into_response();
        }
    };

    let done = handle.done().await.unwrap_or(false);
    if !done {
        return (
            StatusCode::OK,
            Json(json!({
                "status": "pending",
                "promise_id": id,
                "message": "Processing in progress",
            })),
        )
            .into_response();
    }

    match handle.result().await {
        Ok(result) => (
            StatusCode::OK,
            Json(json!({
                "status": "resolved",
                "promise_id": id,
                "result": result,
            })),
        )
            .into_response(),
        Err(rejection) => (
            StatusCode::OK,
            Json(json!({
                "status": "rejected",
                "promise_id": id,
                "error": rejection.to_string(),
            })),
        )
            .into_response(),
    }
}
