use std::{
    error::Error,
    fmt,
    sync::{
        Arc, atomic::Ordering,
    },
};

use axum::{
    Router, extract::{
         State, ws::{ Message, WebSocket, WebSocketUpgrade }
    }, http::status::StatusCode, response::{ Html, IntoResponse, Response }, routing::get,
};
use thiserror::Error;
use serde_json::json;

use crate::AppState;

static INDEX_HTML: &str = include_str!("../frontend/index.html");
static STYLE_CSS: &str = include_str!("../frontend/style.css");
static APP_JS: &str = include_str!("../frontend/app.js");

pub fn build_router(
    state: Arc<AppState>
) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/style.css", get(style))
        .route("/app.js", get(app_js))
        .route("/ws", get(ws_upgrade))
        .with_state(state)
}

#[derive(Debug)]
pub enum ApiError {
    StatusOnly(StatusCode),
    StatusWithMessage(StatusCode, String)
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ApiError::StatusOnly(status) => {
                let res = status.canonical_reason().map_or_else(|| format!("HTTP Error: {status}"), |reason| reason.to_owned() );
                f.write_str(&res)
            },
            ApiError::StatusWithMessage(status, msg) => {
                let res = status.canonical_reason().map_or_else(|| format!("HTTP Error: {status}"), |reason| reason.to_owned() );
                f.write_str(&res)?;
                f.write_str("\n")?;
                f.write_str(msg)
            }
        }
    }
}

impl Error for ApiError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            ApiError::StatusOnly(status) => {
                let res = status.canonical_reason().map_or_else(|| format!("HTTP Error: {status}"), |reason| reason.to_owned() );
                (status, res).into_response()
            },
            ApiError::StatusWithMessage(status, msg) => (status, msg).into_response()
        }
    }
}

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Systemd D-Bus error: {0}")]
    Dbus(#[from] zbus::Error),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("API error: {0}")]
    Api(#[from] ApiError),

    #[error("Unknown error")]
    Unknown
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if let AppError::Api(aer) = self {
            return aer.into_response();
        }

        eprintln!("Internal Error: {}", self);

        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Something went wrong: {}", self),
        )
            .into_response()
    }
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn style() -> impl IntoResponse {
    (
        [("content-type", "text/css; charset=utf-8")],
        STYLE_CSS
    )
}

async fn app_js() -> impl IntoResponse {
    (
        [("content-type", "application/javascript; charset=utf-8")],
        APP_JS
    )
}

async fn ws_upgrade(
    State(state): State<Arc<AppState>>,
    ws: WebSocketUpgrade
) -> Result<impl IntoResponse, AppError> {
    if !state.pairing_allowed.load(Ordering::SeqCst) {
        return Err(AppError::Api(ApiError::StatusOnly(StatusCode::SERVICE_UNAVAILABLE)));
    }

    Ok(ws.on_upgrade(move |socket| handle_websocket(socket, state)))
}

async fn handle_websocket(
    mut socket: WebSocket,
    state: Arc<AppState>
) {
    if let Some(Ok(Message::Text(text))) = socket.recv().await {
        if text.trim() != state.pin.as_str() {
            let _ = socket.close().await;
            return;
        }
    } else {
        let _ = socket.close().await;
        return;
    }

    let _ = socket
        .send(Message::Text(json!({ "type": "pin-ok" }).to_string()))
        .await;

    let _ = state.webrtc.handle_signaling(socket, state.pairing_allowed.clone()).await;
}