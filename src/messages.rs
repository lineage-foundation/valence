// messages.rs
//
// REST handlers for the `/messages` mailbox API. Every route is authed via
// the `AuthedAddress` extractor (see `auth.rs`); the authenticated caller's
// address is always the mailbox key — a sender POSTs into a recipient's
// mailbox, a reader GETs/DELETEs their own.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::AuthedAddress;
use crate::store::{KvStore, StoreError};

#[derive(Clone)]
pub struct MessagesState {
    pub store: Arc<dyn KvStore + Send + Sync>,
    pub ttl_secs: u64,
}

pub fn router(state: MessagesState) -> Router {
    Router::new()
        .route("/messages", post(create_message).get(get_all_messages).delete(delete_all_messages))
        .route("/messages/:id", get(get_message).delete(delete_message))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
pub struct CreateMessageRequest {
    pub id: String,
    pub data: Value,
}

#[derive(Debug, Serialize)]
pub struct CreateMessageResponse {
    pub id: String,
}

#[derive(Debug, Serialize)]
pub struct GetMessageResponse {
    pub id: String,
    pub data: Value,
}

/// Errors surfaced by the `/messages` handlers, rendered as a JSON body of
/// the shape `{ "error": "..." }`.
#[derive(Debug)]
pub enum MessagesError {
    Store(StoreError),
    NotFound,
}

impl From<StoreError> for MessagesError {
    fn from(e: StoreError) -> Self {
        MessagesError::Store(e)
    }
}

impl IntoResponse for MessagesError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            MessagesError::Store(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            MessagesError::NotFound => (StatusCode::NOT_FOUND, "message not found".to_string()),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

async fn create_message(
    State(state): State<MessagesState>,
    AuthedAddress(mailbox): AuthedAddress,
    Json(req): Json<CreateMessageRequest>,
) -> Result<(StatusCode, Json<CreateMessageResponse>), MessagesError> {
    state
        .store
        .set(&mailbox, &req.id, &req.data, state.ttl_secs)
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(CreateMessageResponse { id: req.id }),
    ))
}

async fn get_all_messages(
    State(state): State<MessagesState>,
    AuthedAddress(mailbox): AuthedAddress,
) -> Result<Json<BTreeMap<String, Value>>, MessagesError> {
    let all = state.store.get_all(&mailbox).await?;
    Ok(Json(all))
}

async fn get_message(
    State(state): State<MessagesState>,
    AuthedAddress(mailbox): AuthedAddress,
    Path(id): Path<String>,
) -> Result<Json<GetMessageResponse>, MessagesError> {
    let data = state
        .store
        .get_one(&mailbox, &id)
        .await?
        .ok_or(MessagesError::NotFound)?;

    Ok(Json(GetMessageResponse { id, data }))
}

async fn delete_message(
    State(state): State<MessagesState>,
    AuthedAddress(mailbox): AuthedAddress,
    Path(id): Path<String>,
) -> Result<StatusCode, MessagesError> {
    state.store.delete_one(&mailbox, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_all_messages(
    State(state): State<MessagesState>,
    AuthedAddress(mailbox): AuthedAddress,
) -> Result<StatusCode, MessagesError> {
    state.store.delete_all(&mailbox).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemStore;
    use axum::body::Body;
    use axum::http::Request;
    use serde_json::Value as Json_;
    use tower::ServiceExt;
    use tw_chain::crypto::sign_ed25519::{gen_keypair, sign_detached};
    use tw_chain::utils::transaction_utils::construct_address;

    fn test_router() -> Router {
        let state = MessagesState {
            store: Arc::new(MemStore::new()),
            ttl_secs: 60,
        };
        router(state)
    }

    /// Builds a fresh keypair/address and returns the address plus the three
    /// auth headers a caller must send to act as that address's mailbox
    /// owner (matches the scheme in `auth.rs`: sign the address bytes).
    fn signed_headers() -> (String, String, String, String) {
        let (pk, sk) = gen_keypair();
        let address = construct_address(&pk);
        let sig = sign_detached(address.as_bytes(), &sk);
        (
            address.clone(),
            address,
            hex::encode(pk.as_ref()),
            hex::encode(sig.as_ref()),
        )
    }

    fn authed_request(
        method: &str,
        uri: &str,
        address: &str,
        public_key: &str,
        signature: &str,
        body: Body,
    ) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("address", address)
            .header("public_key", public_key)
            .header("signature", signature)
            .header("content-type", "application/json")
            .body(body)
            .unwrap()
    }

    #[tokio::test]
    async fn post_then_get_returns_it() {
        let app = test_router();
        let (_owner, address, pk, sig) = signed_headers();

        let post_body = serde_json::to_vec(&json!({ "id": "msg1", "data": { "hello": "world" } }))
            .unwrap();
        let post_req = authed_request(
            "POST",
            "/messages",
            &address,
            &pk,
            &sig,
            Body::from(post_body),
        );
        let post_resp = app.clone().oneshot(post_req).await.unwrap();
        assert_eq!(post_resp.status(), StatusCode::CREATED);

        let get_req = authed_request("GET", "/messages/msg1", &address, &pk, &sig, Body::empty());
        let get_resp = app.clone().oneshot(get_req).await.unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Json_ = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body["id"], "msg1");
        assert_eq!(body["data"], json!({ "hello": "world" }));
    }

    #[tokio::test]
    async fn get_missing_id_returns_404() {
        let app = test_router();
        let (_owner, address, pk, sig) = signed_headers();

        let get_req = authed_request(
            "GET",
            "/messages/does-not-exist",
            &address,
            &pk,
            &sig,
            Body::empty(),
        );
        let get_resp = app.oneshot(get_req).await.unwrap();
        assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_one_removes_it() {
        let app = test_router();
        let (_owner, address, pk, sig) = signed_headers();

        let post_body = serde_json::to_vec(&json!({ "id": "msg1", "data": 1 })).unwrap();
        let post_req = authed_request(
            "POST",
            "/messages",
            &address,
            &pk,
            &sig,
            Body::from(post_body),
        );
        app.clone().oneshot(post_req).await.unwrap();

        let del_req = authed_request("DELETE", "/messages/msg1", &address, &pk, &sig, Body::empty());
        let del_resp = app.clone().oneshot(del_req).await.unwrap();
        assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);

        let get_req = authed_request("GET", "/messages/msg1", &address, &pk, &sig, Body::empty());
        let get_resp = app.oneshot(get_req).await.unwrap();
        assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_all_clears_mailbox() {
        let app = test_router();
        let (_owner, address, pk, sig) = signed_headers();

        for (id, data) in [("msg1", json!(1)), ("msg2", json!(2))] {
            let post_body = serde_json::to_vec(&json!({ "id": id, "data": data })).unwrap();
            let post_req = authed_request(
                "POST",
                "/messages",
                &address,
                &pk,
                &sig,
                Body::from(post_body),
            );
            app.clone().oneshot(post_req).await.unwrap();
        }

        let del_req = authed_request("DELETE", "/messages", &address, &pk, &sig, Body::empty());
        let del_resp = app.clone().oneshot(del_req).await.unwrap();
        assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);

        let get_req = authed_request("GET", "/messages", &address, &pk, &sig, Body::empty());
        let get_resp = app.oneshot(get_req).await.unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Json_ = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body, json!({}));
    }

    #[tokio::test]
    async fn get_all_shape_has_id_keyed_entries() {
        let app = test_router();
        let (_owner, address, pk, sig) = signed_headers();

        for (id, data) in [("msg1", json!({"a": 1})), ("msg2", json!({"b": 2}))] {
            let post_body = serde_json::to_vec(&json!({ "id": id, "data": data })).unwrap();
            let post_req = authed_request(
                "POST",
                "/messages",
                &address,
                &pk,
                &sig,
                Body::from(post_body),
            );
            app.clone().oneshot(post_req).await.unwrap();
        }

        let get_req = authed_request("GET", "/messages", &address, &pk, &sig, Body::empty());
        let get_resp = app.oneshot(get_req).await.unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Json_ = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(
            body,
            json!({ "msg1": {"a": 1}, "msg2": {"b": 2} })
        );
    }

    #[tokio::test]
    async fn missing_auth_headers_is_unauthorized() {
        let app = test_router();
        let req = Request::builder()
            .method("GET")
            .uri("/messages")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
