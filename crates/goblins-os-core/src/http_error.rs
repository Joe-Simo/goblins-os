//! Shared error envelope for the core daemon's JSON API.
//!
//! Every handler returns the same `{"text": "..."}` body on failure, so the error
//! shape lives in one place rather than being copy-pasted across modules. A future
//! change to the client error contract is then made once, here.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct ErrorBody {
    pub(crate) text: String,
}

/// Build the canonical `{"text": ...}` error response. Accepts `impl Into<String>`
/// so both static literals and dynamically built `String`s (e.g. validation
/// messages) share one helper.
pub(crate) fn error_response(status: StatusCode, text: impl Into<String>) -> Response {
    (status, Json(ErrorBody { text: text.into() })).into_response()
}
