#!/usr/bin/env sh
# Build the Android agent and bundle it into the Management Server, which
# serves it to phones at /api/v1/agent/downloads/android-agent.apk.
# Needs JDK 17 and the Android SDK (platform 35).
set -e
root="$(cd "$(dirname -- "$0")/.." && pwd)"
cd "$root/apps/android-agent"
./gradlew -q assembleRelease
cp app/build/outputs/apk/release/app-release.apk "$root/crates/od-server/assets/opendirectory-agent-android.apk"
echo "Bundled $(wc -c < "$root/crates/od-server/assets/opendirectory-agent-android.apk") bytes into crates/od-server/assets/. Rebuild the server to ship it."
