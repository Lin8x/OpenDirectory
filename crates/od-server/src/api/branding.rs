//! Company branding: logo, accent color and product name shown in the
//! Admin Center (including the sign-in screen).

use axum::extract::State;
use axum::Json;
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::auth::Admin;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

#[derive(Serialize, Deserialize, Default)]
pub struct Branding {
    /// `data:image/png;base64,...` (PNG, JPEG, WebP or sanitized SVG), or empty.
    #[serde(default)]
    pub logo: String,
    /// `#rrggbb`, or empty for the default.
    #[serde(default)]
    pub accent: String,
    /// Shown instead of "Admin Center" under the logo, e.g. "Contoso IT".
    #[serde(default)]
    pub product_name: String,
    /// Optional support contact shown on the sign-in screen.
    #[serde(default)]
    pub support_contact: String,
}

const MAX_LOGO_BYTES: usize = 512 * 1024;

/// Public: the sign-in screen shows the brand before anyone signs in.
pub async fn get(State(s): State<AppState>) -> ApiResult<Json<Branding>> {
    Ok(Json(match s.db.meta_get("branding")? {
        Some(j) => serde_json::from_str(&j).unwrap_or_default(),
        None => Branding::default(),
    }))
}

pub async fn set(State(s): State<AppState>, admin: Admin, Json(b): Json<Branding>) -> ApiResult<Json<Branding>> {
    if !b.accent.is_empty() && !(b.accent.len() == 7 && b.accent.starts_with('#') && b.accent[1..].chars().all(|c| c.is_ascii_hexdigit())) {
        return Err(ApiError::bad_request("The accent color must look like #3a63f3"));
    }
    if b.product_name.chars().count() > 40 || b.support_contact.chars().count() > 120 {
        return Err(ApiError::bad_request("The name or contact is too long"));
    }
    if !b.logo.is_empty() {
        check_logo(&b.logo)?;
    }
    s.db.meta_set("branding", &serde_json::to_string(&b).map_err(anyhow::Error::from)?)?;
    s.db.audit(admin.name(), "branding.update", "", if b.logo.is_empty() { "default logo" } else { "custom logo" });
    Ok(Json(b))
}

fn check_logo(logo: &str) -> ApiResult<()> {
    let (mime, data) = logo
        .strip_prefix("data:")
        .and_then(|r| r.split_once(";base64,"))
        .ok_or_else(|| ApiError::bad_request("Upload the logo as an image file"))?;
    if !["image/png", "image/jpeg", "image/webp", "image/svg+xml"].contains(&mime) {
        return Err(ApiError::bad_request("Logos can be PNG, JPEG, WebP or SVG"));
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(data).map_err(|_| ApiError::bad_request("The logo couldn't be read"))?;
    if bytes.len() > MAX_LOGO_BYTES {
        return Err(ApiError::bad_request("Logos are limited to 512 KB"));
    }
    if mime == "image/svg+xml" {
        // SVG can carry scripts. The UI shows logos with <img>, which never runs
        // them, but refuse active content anyway so the file is safe everywhere.
        let text = String::from_utf8_lossy(&bytes).to_lowercase();
        let banned =
            ["<script", "javascript:", "<foreignobject", "<iframe", "<embed", "<object", "xlink:href=\"http", "href=\"http", "<!entity"];
        if banned.iter().any(|b| text.contains(b)) || text.split_whitespace().any(|w| w.starts_with("on") && w.contains('=')) {
            return Err(ApiError::bad_request(
                "This SVG contains scripts or external links. Export it again as a plain SVG, or use a PNG.",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn svg(s: &str) -> String {
        format!("data:image/svg+xml;base64,{}", base64::engine::general_purpose::STANDARD.encode(s))
    }
    #[test]
    fn svg_sanitizing() {
        assert!(check_logo(&svg("<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"10\" height=\"10\"/></svg>")).is_ok());
        assert!(check_logo(&svg("<svg><script>alert(1)</script></svg>")).is_err());
        assert!(check_logo(&svg("<svg onload=\"alert(1)\"></svg>")).is_err());
        assert!(check_logo("data:text/html;base64,PGgxPg==").is_err());
    }
}
