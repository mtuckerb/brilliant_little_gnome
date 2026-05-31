// Pre-share validation for Brightspace auth keys.
//
// Before a Brightspace session cookie is ever shared/synced to another
// device, it is probed against a known-good Brightspace auth endpoint
// (`/users/whoami`) to confirm it is live and correct. This mirrors the
// liveness probe already used by `maybe_mark_auth_failure`, but is
// reusable from the sync paths so we never publish a stale/invalid key
// to peers.
//
// Hard rules enforced here:
//   * Only a confirmed-live (`Valid`) key is allowed to proceed. An
//     `Invalid` (expired/wrong) or `Inconclusive` (unreachable/misconfig)
//     result fails closed.
//   * The candidate key is never logged. Probe errors are stripped of
//     their URL (`without_url`) so a configured endpoint that happens to
//     embed a secret can't leak into logs either.

use crate::error::{AppError, Result};
use reqwest::{header, Client, StatusCode};
use std::time::Duration;

/// Outcome of probing a Brightspace session key against the known-good
/// auth endpoint. Carries no key material, so `Debug`/`Display` of this
/// type can never leak the cookie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionValidation {
    /// Endpoint confirmed the session is live. Sharing/sync may proceed.
    Valid,
    /// Endpoint rejected the session (401/403): expired or incorrect.
    Invalid,
    /// Could not confirm liveness (network error, misconfig, or an
    /// unexpected status). Treated as fail-closed by callers.
    Inconclusive,
}

/// Env var that overrides the validation endpoint. Set this when the
/// known-good auth link is environment-specific (so no tenant/account
/// URL is hardcoded), or to point validation at a local stub in tests.
/// When unset, the endpoint is derived from the dynamic Brightspace host.
pub const VALIDATE_URL_ENV: &str = "BRILLIANT_AUTH_VALIDATE_URL";

/// User-facing message when validation actively rejects the key.
/// Intentionally actionable and free of any key material.
pub const SHARE_BLOCKED_INVALID: &str = "Brightspace authentication could not be verified - the saved key looks expired or incorrect, so it was not shared to your other devices. Sign in again, then retry.";

/// User-facing message when validation could not be completed. Also
/// fail-closed: the key is not shared.
pub const SHARE_BLOCKED_INCONCLUSIVE: &str = "Could not reach Brightspace to verify your auth key, so it was not shared to your other devices. Check your connection and try again.";

/// Map a probe outcome (HTTP status, or `None` for a transport failure)
/// to a validation result. Pure and synchronous so it is trivially
/// unit-testable without any network.
pub fn classify_probe(status: Option<StatusCode>) -> SessionValidation {
    match status {
        Some(s) if s.is_success() => SessionValidation::Valid,
        Some(StatusCode::UNAUTHORIZED) | Some(StatusCode::FORBIDDEN) => SessionValidation::Invalid,
        Some(_) => SessionValidation::Inconclusive,
        None => SessionValidation::Inconclusive,
    }
}

/// Resolve the validation endpoint URL. Prefers the `BRILLIANT_AUTH_VALIDATE_URL`
/// override when set to a non-empty value; otherwise derives the standard
/// `/users/whoami` endpoint from the (dynamic, never hardcoded) host.
pub fn validation_probe_url(host: &str) -> Result<String> {
    if let Ok(override_url) = std::env::var(VALIDATE_URL_ENV) {
        let trimmed = override_url.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    let h = host.trim();
    if h.is_empty() {
        return Err(AppError::BadRequest(
            "no Brightspace host configured for auth validation".into(),
        ));
    }
    Ok(format!(
        "https://{}/d2l/api/lp/{}/users/whoami",
        h,
        super::API_VERSION
    ))
}

/// Probe a candidate session key against `probe_url`. The key is sent only
/// in the `Cookie` header and is never logged. Times out so a sync can't
/// hang on a wedged endpoint.
pub async fn probe_session(http: &Client, probe_url: &str, cookie: &str) -> SessionValidation {
    match http
        .get(probe_url)
        .header(header::COOKIE, cookie)
        .timeout(Duration::from_secs(10))
        .send()
        .await
    {
        Ok(resp) => classify_probe(Some(resp.status())),
        Err(e) => {
            // `without_url` strips any (possibly secret-bearing) URL from
            // the error before logging; the cookie is never part of `e`.
            tracing::warn!("auth validation probe failed: {} (key not logged)", e.without_url());
            classify_probe(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn classify_probe_maps_statuses() {
        assert_eq!(classify_probe(Some(StatusCode::OK)), SessionValidation::Valid);
        assert_eq!(classify_probe(Some(StatusCode::NO_CONTENT)), SessionValidation::Valid);
        assert_eq!(classify_probe(Some(StatusCode::UNAUTHORIZED)), SessionValidation::Invalid);
        assert_eq!(classify_probe(Some(StatusCode::FORBIDDEN)), SessionValidation::Invalid);
        assert_eq!(
            classify_probe(Some(StatusCode::INTERNAL_SERVER_ERROR)),
            SessionValidation::Inconclusive
        );
        assert_eq!(classify_probe(Some(StatusCode::BAD_GATEWAY)), SessionValidation::Inconclusive);
        assert_eq!(classify_probe(None), SessionValidation::Inconclusive);
    }

    #[test]
    fn validation_probe_url_override_default_and_empty_host() {
        std::env::set_var(VALIDATE_URL_ENV, "https://stub.invalid/probe");
        assert_eq!(
            validation_probe_url("ignored.example.edu").unwrap(),
            "https://stub.invalid/probe"
        );

        std::env::remove_var(VALIDATE_URL_ENV);
        let url = validation_probe_url("lms.example.edu").unwrap();
        assert!(url.starts_with("https://lms.example.edu/d2l/api/lp/"));
        assert!(url.ends_with("/users/whoami"));
        assert!(validation_probe_url("").is_err());
    }

    #[test]
    fn user_facing_messages_carry_no_key_material() {
        for msg in [SHARE_BLOCKED_INVALID, SHARE_BLOCKED_INCONCLUSIVE] {
            assert!(!msg.contains("d2lSessionVal"));
            assert!(!msg.contains("d2lSecureSessionVal"));
            assert!(!msg.contains('='));
        }
    }

    /// Serve exactly one HTTP/1.1 response with the given status line on a
    /// fresh loopback port and return its URL. No real Brightspace contact.
    async fn spawn_status_server(status_line: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await;
                let resp = format!(
                    "HTTP/1.1 {}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                    status_line
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.flush().await;
            }
        });
        format!("http://{}/users/whoami", addr)
    }

    #[tokio::test]
    async fn probe_session_valid_on_200() {
        let url = spawn_status_server("200 OK").await;
        let http = Client::new();
        assert_eq!(
            probe_session(&http, &url, "d2lSessionVal=x; d2lSecureSessionVal=y").await,
            SessionValidation::Valid
        );
    }

    #[tokio::test]
    async fn probe_session_invalid_on_401() {
        let url = spawn_status_server("401 Unauthorized").await;
        let http = Client::new();
        assert_eq!(probe_session(&http, &url, "k=v").await, SessionValidation::Invalid);
    }

    #[tokio::test]
    async fn probe_session_inconclusive_on_unreachable() {
        let addr = {
            let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
            l.local_addr().unwrap()
        };
        let url = format!("http://{}/users/whoami", addr);
        let http = Client::new();
        assert_eq!(
            probe_session(&http, &url, "k=v").await,
            SessionValidation::Inconclusive
        );
    }
}
