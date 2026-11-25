//! Dev-only API explorer: GET /dev/api (a page to call any route by hand) and GET /dev/api/routes (every route
//! in `router()`, read from this crate's lib.rs so the list never drifts). main.rs mounts it only when
//! API_EXPLORER=1, so a deployed server doesn't show it.

use axum::response::Html;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

pub fn router() -> Router {
    Router::new().route("/dev/api", get(page)).route("/dev/api/routes", get(routes))
}

async fn page() -> Html<&'static str> {
    Html(include_str!("explorer.html"))
}

async fn routes() -> Json<Value> {
    Json(json!(parse(include_str!("lib.rs"))))
}

const METHODS: [&str; 5] = ["get", "post", "put", "patch", "delete"];

/// Every `.route("path", method(module::handler)...)` in the source, as {method, path, handler}.
fn parse(src: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for chunk in src.split(".route(").skip(1) {
        let Some(path) = chunk.split('"').nth(1) else { continue };
        // The call ends at the next route; trailing layers and the end of router() hold no handlers.
        let call = chunk.split(".layer(middleware").next().unwrap_or(chunk);
        for m in METHODS {
            for part in call.split(&format!("{m}(")).skip(1) {
                let handler: String = part.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':').collect();
                // A handler is `module::name`, or an inline closure (health).
                if handler.contains("::") || part.starts_with("||") {
                    out.push(json!({ "method": m.to_uppercase(), "path": path, "handler": if handler.is_empty() { "inline".into() } else { handler } }));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_every_route() {
        let r = super::parse(include_str!("lib.rs"));
        let has = |m: &str, p: &str| r.iter().any(|x| x["method"] == m && x["path"] == p);
        assert!(has("POST", "/v1/attendance/regularisations") && has("GET", "/v1/attendance/regularisations"));
        assert!(has("POST", "/v1/employee-imports") && has("GET", "/v1/employee-imports")); // multi-line route
        assert!(has("GET", "/health") && has("POST", "/v1/auth/admin/sign-in"));
        assert!(!r.iter().any(|x| x["path"].as_str().unwrap_or_default().is_empty()));
        assert!(r.len() > 150, "{}", r.len());
    }
}
