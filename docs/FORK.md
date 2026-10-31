# surf1k/Serein

This repository is a GitHub fork of [ViceVerse-cz/Serein](https://github.com/ViceVerse-cz/Serein).
The starting upstream revision is `1b2f8942` on `main` (31 October 2026). Upstream history,
MIT/Apache-2.0 licenses and third-party notices are retained.

## Application

Serein is a Rust desktop application rendered with egui/wgpu. It connects directly to
Discord REST and Gateway services for existing accounts. It does not require a separate
chat server or a new messaging account. A temporary native webview is used for sign-in;
the chat interface is native.

The fork defaults new preferences to English. Existing explicitly saved language settings,
including System, are preserved. Discord message content and server names remain as written
by their authors. English documentation and error text do not imply translation of Discord content.

## Build

Follow [platform requirements](platform-support.md), then run:

```sh
git clone --branch fork/english-default https://github.com/surf1k/Serein.git
cd Serein
cargo build --locked --release -p serein
cargo run --locked --release -p serein
```

Use `cargo run --locked -p serein --features demo -- --demo` for an offline synthetic session.
This is sample data, not a live Discord account. Do not put session tokens or passwords into
GitHub issues, logs or commits. Authentication and keyring behavior are described in
[authentication](authentication.md) and [storage policy](storage-policy.md).

## Compatibility and verification

This fork inherits upstream's implemented features and limitations. Consult the
[feature matrix](../README.md#feature-matrix) and [Discord compatibility notes](discord-compatibility.md).
Full parity with the official Discord client is not certified. No live account login,
messages, microphone capture or calls were tested during creation of this fork.

The local Fedora host lacks GTK4, WebKitGTK 6.0 and audio development packages required
for a native build. Build/check outcomes are recorded in the fork's pull request.
Upstream download links remain labeled as upstream: no fork-specific installer or release
has been produced yet. CI artifacts, when successful, are unsigned test builds.

Upstream issue links, extension catalogs, package repositories and in-app updater remain
upstream-owned. Updates may install upstream builds. They have not been redirected to an empty
fork release endpoint. This fork is unofficial and is not endorsed by Discord.
