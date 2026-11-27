//! Agent downloads served by the Management Server itself, so devices get
//! the agent from your own server instead of the internet.

use axum::http::header;
use axum::response::IntoResponse;

/// The Android agent, bundled at build time (`./scripts/build-android.sh`).
const ANDROID_APK: &[u8] = include_bytes!("../../assets/opendirectory-agent-android.apk");

/// No sign-in: a phone downloads this before it's enrolled. It contains no
/// secrets; enrolling still needs a token.
pub async fn android_apk() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/vnd.android.package-archive"),
            (header::CONTENT_DISPOSITION, "attachment; filename=\"opendirectory-agent.apk\""),
        ],
        ANDROID_APK,
    )
}
