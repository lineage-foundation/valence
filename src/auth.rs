// auth.rs
//
// Request authentication for authed routes.
//
// Clients send three headers on every authed request:
//   - `address`    — caller's address (hex string); this is also the
//                     mailbox key.
//   - `public_key` — caller's ed25519 public key, hex-encoded.
//   - `signature`  — hex-encoded ed25519 detached signature over the raw
//                     UTF-8 bytes of the `address` string (i.e. the signed
//                     message is `address.as_bytes()`, not hex-decoded and
//                     not hashed).
//
// A request is accepted iff `signature` is a valid detached signature of
// `address.as_bytes()` under `public_key` — i.e. the caller proves control of
// `public_key` and signed the target address. This is intentionally
// verify-only, with NO `address == construct_address(public_key)` binding: the
// relay lets a sender drop an encrypted payload addressed to a counterparty
// (in the JS SDK, `make2WayPayment` signs the *recipient's* address with the
// *sender's* key), so a binding would reject every send. Confidentiality comes
// from the payload being E2E-encrypted for the recipient, not from mailbox
// access control. Must match the AIBlock/Lineage JS SDK, which signs this way.
// (Future hardening option: bind address==construct_address only on GET/DELETE,
// where the SDK signs the caller's own address.)

use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use tw_chain::crypto::sign_ed25519::{verify_detached, PublicKey, Signature};

const HEADER_ADDRESS: &str = "address";
const HEADER_PUBLIC_KEY: &str = "public_key";
const HEADER_SIGNATURE: &str = "signature";

/// An address that has passed signature verification and ownership binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedAddress(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    MissingHeader(&'static str),
    InvalidHeaderEncoding(&'static str),
    InvalidHex(&'static str),
    InvalidPublicKeyLength,
    InvalidSignatureLength,
    SignatureVerificationFailed,
}

impl AuthError {
    fn message(&self) -> String {
        match self {
            AuthError::MissingHeader(h) => format!("missing '{h}' header"),
            AuthError::InvalidHeaderEncoding(h) => format!("header '{h}' is not valid UTF-8"),
            AuthError::InvalidHex(h) => format!("header '{h}' is not valid hex"),
            AuthError::InvalidPublicKeyLength => "public_key has an invalid length".to_string(),
            AuthError::InvalidSignatureLength => "signature has an invalid length".to_string(),
            AuthError::SignatureVerificationFailed => {
                "signature verification failed".to_string()
            }
        }
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let body = Json(json!({ "error": self.message() }));
        (StatusCode::UNAUTHORIZED, body).into_response()
    }
}

/// Pure verifier: takes the three raw header strings and returns the
/// verified address on success. No axum types involved, so it's directly
/// unit-testable.
pub fn verify_request(
    address: &str,
    public_key_hex: &str,
    signature_hex: &str,
) -> Result<VerifiedAddress, AuthError> {
    let public_key_bytes =
        hex::decode(public_key_hex).map_err(|_| AuthError::InvalidHex(HEADER_PUBLIC_KEY))?;
    let signature_bytes =
        hex::decode(signature_hex).map_err(|_| AuthError::InvalidHex(HEADER_SIGNATURE))?;

    let public_key =
        PublicKey::from_slice(&public_key_bytes).ok_or(AuthError::InvalidPublicKeyLength)?;
    let signature =
        Signature::from_slice(&signature_bytes).ok_or(AuthError::InvalidSignatureLength)?;

    if !verify_detached(&signature, address.as_bytes(), &public_key) {
        return Err(AuthError::SignatureVerificationFailed);
    }

    Ok(VerifiedAddress(address.to_string()))
}

/// Axum extractor: reads the `address` / `public_key` / `signature`
/// headers, runs [`verify_request`], and rejects with 401 on failure.
/// Handlers that need the verified caller just take `AuthedAddress` as an
/// argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthedAddress(pub String);

#[async_trait]
impl<S> FromRequestParts<S> for AuthedAddress
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let address = header_str(parts, HEADER_ADDRESS)?;
        let public_key = header_str(parts, HEADER_PUBLIC_KEY)?;
        let signature = header_str(parts, HEADER_SIGNATURE)?;

        verify_request(address, public_key, signature).map(|v| AuthedAddress(v.0))
    }
}

fn header_str<'a>(parts: &'a Parts, name: &'static str) -> Result<&'a str, AuthError> {
    parts
        .headers
        .get(name)
        .ok_or(AuthError::MissingHeader(name))?
        .to_str()
        .map_err(|_| AuthError::InvalidHeaderEncoding(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tw_chain::crypto::sign_ed25519::{gen_keypair, sign_detached};
    use tw_chain::utils::transaction_utils::construct_address;

    // Real vector taken from the JS SDK test set: this public key must
    // derive this exact address, or request auth is not compatible with
    // clients signed against the SDK.
    const KNOWN_PUBLIC_KEY: &str =
        "5e6d463ec66d7999769fa4de56f690dfb62e685b97032f5926b0cb6c93ba83c6";
    const KNOWN_ADDRESS: &str =
        "cf0067d6c42463b2c1e4236e9669df546c74b16c0e2ef37114549b2944e05b7c";

    #[test]
    fn address_derivation_matches_known_sdk_vector() {
        let pk_bytes = hex::decode(KNOWN_PUBLIC_KEY).expect("valid hex");
        let pk = PublicKey::from_slice(&pk_bytes).expect("valid pubkey length");
        assert_eq!(construct_address(&pk), KNOWN_ADDRESS);
    }

    #[test]
    fn accepts_a_valid_signed_request() {
        let (pk, sk) = gen_keypair();
        let address = construct_address(&pk);
        let sig = sign_detached(address.as_bytes(), &sk);

        let result = verify_request(
            &address,
            &hex::encode(pk.as_ref()),
            &hex::encode(sig.as_ref()),
        );

        assert_eq!(result, Ok(VerifiedAddress(address)));
    }

    #[test]
    fn rejects_signature_over_different_bytes() {
        let (pk, sk) = gen_keypair();
        let address = construct_address(&pk);
        // Signed over a different message than the claimed address.
        let sig = sign_detached(b"not-the-address", &sk);

        let result = verify_request(
            &address,
            &hex::encode(pk.as_ref()),
            &hex::encode(sig.as_ref()),
        );

        assert_eq!(result, Err(AuthError::SignatureVerificationFailed));
    }

    #[test]
    fn accepts_valid_signature_over_a_non_owned_address() {
        // The relay is verify-only: a sender signs a *recipient's* address with
        // their own key to drop an encrypted payload into that mailbox (this is
        // exactly what the SDK's make2WayPayment does). As long as the signature
        // over the target address is valid under the presented public key, it is
        // accepted — there is no address==construct_address(public_key) binding.
        let (pk, sk) = gen_keypair();
        let (other_pk, _other_sk) = gen_keypair();
        let recipient_address = construct_address(&other_pk); // not owned by `pk`
        let sig = sign_detached(recipient_address.as_bytes(), &sk);

        let result = verify_request(
            &recipient_address,
            &hex::encode(pk.as_ref()),
            &hex::encode(sig.as_ref()),
        );

        assert_eq!(result, Ok(VerifiedAddress(recipient_address)));
    }

    #[test]
    fn rejects_malformed_hex_header() {
        let (pk, sk) = gen_keypair();
        let address = construct_address(&pk);
        let sig = sign_detached(address.as_bytes(), &sk);

        let result = verify_request(&address, "not-hex!!", &hex::encode(sig.as_ref()));

        assert_eq!(result, Err(AuthError::InvalidHex(HEADER_PUBLIC_KEY)));
    }
}
