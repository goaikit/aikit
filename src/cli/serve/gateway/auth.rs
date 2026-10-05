//! Operator-issued host/session grants. Tokens never authorize legacy endpoints.
use axum::http::{HeaderMap, Method};
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    token: String,
    scopes: Vec<String>,
    #[serde(default)]
    sessions: Option<Vec<String>>,
}
#[derive(Clone, Default)]
pub struct Access {
    grants: Vec<Grant>,
    origins: Vec<String>,
}
impl Access {
    /// The generic command endpoint must not bypass the dedicated response scope.
    pub async fn command_authorized(
        &self,
        owner: Option<&str>,
        req: axum::http::Request<axum::body::Body>,
    ) -> Result<axum::http::Request<axum::body::Body>, axum::http::StatusCode> {
        let path = req.uri().path();
        let gateway =
            path.starts_with("/api/v1/gateway/sessions/") || path.starts_with("/gateway/sessions/");
        if req.method() != Method::POST || !gateway || !path.ends_with("/commands") {
            return Ok(req);
        }
        let response_path = format!("{}/requests/_/response", path.trim_end_matches("/commands"));
        let (parts, body) = req.into_parts();
        let bytes = axum::body::to_bytes(body, 1024 * 1024)
            .await
            .map_err(|_| axum::http::StatusCode::PAYLOAD_TOO_LARGE)?;
        let command = serde_json::from_slice::<serde_json::Value>(&bytes).ok();
        if command
            .as_ref()
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str())
            == Some("respond")
            && !self.authorized(owner, &parts.headers, &Method::POST, &response_path)
        {
            return Err(axum::http::StatusCode::UNAUTHORIZED);
        }
        Ok(axum::http::Request::from_parts(
            parts,
            axum::body::Body::from(bytes),
        ))
    }
    pub fn origin_allowed(&self, headers: &HeaderMap, path: &str) -> bool {
        let gateway = path == "/api/v1/gateway"
            || path.starts_with("/api/v1/gateway/")
            || path == "/gateway"
            || path.starts_with("/gateway/");
        gateway
            && headers
                .get("origin")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|o| self.origins.iter().any(|allowed| allowed == o))
    }
    pub fn load() -> anyhow::Result<Self> {
        let grants = match std::env::var_os("AIKIT_GATEWAY_TOKENS_FILE") {
            Some(path) => serde_json::from_slice::<Vec<Grant>>(&std::fs::read(path)?)?,
            None => vec![],
        };
        for grant in &grants {
            anyhow::ensure!(
                grant.token.len() >= 32,
                "gateway tokens must contain at least 32 characters"
            );
            anyhow::ensure!(
                grant
                    .scopes
                    .iter()
                    .all(|s| matches!(s.as_str(), "read" | "execute" | "respond")),
                "invalid gateway token scope"
            );
        }
        let origins = match std::env::var("AIKIT_GATEWAY_ORIGINS") {
            Ok(value) => serde_json::from_str(&value)?,
            Err(_) => vec![],
        };
        Ok(Self { grants, origins })
    }
    pub fn authorized(
        &self,
        owner: Option<&str>,
        headers: &HeaderMap,
        method: &Method,
        path: &str,
    ) -> bool {
        let suffix = path
            .strip_prefix("/api/v1/gateway")
            .or_else(|| path.strip_prefix("/gateway"))
            .filter(|s| s.is_empty() || s.starts_with('/'));
        if suffix.is_some() {
            if let Some(origin) = headers.get("origin") {
                if !origin
                    .to_str()
                    .is_ok_and(|o| self.origins.iter().any(|allowed| allowed == o))
                {
                    return false;
                }
            }
        }
        let token = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        if owner.is_none() {
            return token.is_none();
        }
        if token.zip(owner).is_some_and(|(a, b)| constant_eq(a, b)) {
            return true;
        }
        let Some(suffix) = suffix else { return false };
        let Some(token) = token else { return false };
        self.grants.iter().any(|g| {
            if !constant_eq(token, &g.token) {
                return false;
            }
            let parts: Vec<_> = suffix.trim_matches('/').split('/').collect();
            if let Some(sessions) = &g.sessions {
                if !(parts.len() >= 2
                    && parts[0] == "sessions"
                    && sessions.iter().any(|id| id == parts[1]))
                {
                    return false;
                }
            }
            let scope = if *method == Method::GET {
                "read"
            } else if *method == Method::POST
                && parts.len() == 5
                && parts[0] == "sessions"
                && parts[2] == "requests"
                && parts[4] == "response"
            {
                "respond"
            } else if *method == Method::POST {
                "execute"
            } else {
                return false;
            };
            g.scopes.iter().any(|s| s == scope)
        })
    }
}
fn constant_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grants_cannot_escape_session_or_scope() {
        let token = "x".repeat(32);
        let access = Access {
            grants: vec![Grant {
                token: token.clone(),
                scopes: vec!["read".into(), "respond".into()],
                sessions: Some(vec!["s1".into()]),
            }],
            origins: vec![],
        };
        let mut h = HeaderMap::new();
        h.insert("authorization", format!("Bearer {token}").parse().unwrap());
        assert!(access.authorized(
            Some("owner"),
            &h,
            &Method::GET,
            "/api/v1/gateway/sessions/s1"
        ));
        assert!(!access.authorized(
            Some("owner"),
            &h,
            &Method::GET,
            "/api/v1/gateway/sessions/s2"
        ));
        assert!(!access.authorized(
            Some("owner"),
            &h,
            &Method::POST,
            "/api/v1/gateway/sessions/s1/commands"
        ));
        assert!(access.authorized(
            Some("owner"),
            &h,
            &Method::POST,
            "/api/v1/gateway/sessions/s1/requests/r1/response"
        ));
        assert!(!access.authorized(Some("owner"), &h, &Method::POST, "/api/v1/messages"));
        h.insert("origin", "https://untrusted.example".parse().unwrap());
        assert!(!access.authorized(
            Some("owner"),
            &h,
            &Method::GET,
            "/api/v1/gateway/sessions/s1"
        ));
    }
}
