# OpenDirectory Agent for Android

A small Android app that enrolls a phone or tablet with your Management Server, using the same protocol as the desktop Endpoint Agent.

## What it does

- Reports inventory: model, Android version and security patch level, storage, network addresses.
- Reports security status: whether a screen lock is set, and whether storage is encrypted.
- Shows the **Managed-device notice** GPO setting and records when the user acknowledges it.
- **Help desk chat** with IT, and **Message user** notifications from the Admin Center.
- Works off the office network through the **Internet Gateway**, with a device certificate whose key stays in the Android Keystore.

It doesn't enforce settings, run scripts or open remote sessions. Enforcing settings needs Android Enterprise (Device Owner), which isn't included yet.

## Why it's light

- About 48 KB (release build). No AndroidX, Compose, Google Play services or third-party libraries.
- No background service, open connection or wake lock. Android's JobScheduler runs a check-in about every 15 minutes, only when there's a network and the battery isn't low, batched with other apps' work.
- Inventory is only sent when it changes (or once a day). The app polls more often only while its Help desk screen is open.

## Build

Needs JDK 17 and the Android SDK (platform 35).

```bash
./gradlew assembleRelease
```

The APK is in `app/build/outputs/apk/release/`. It's signed with the debug key; configure your own signing config to publish it.

## Enroll a device

In the Admin Center, open **Enrollment**, create a token and pick the **Android** tab. Enter the server address, token and TLS fingerprint in the app and tap **Enroll**.
