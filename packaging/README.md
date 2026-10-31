# Rustcord desktop packaging

Rustcord is a fork of the native Rust Discord client Serein. MIT/Apache-2.0 licenses,
third-party notices and upstream attribution remain part of every package.

| Platform | Installed application | Launcher |
| --- | --- | --- |
| Linux | Rustcord desktop entry | `rustcord` |
| Windows | Rustcord Start Menu shortcut and installer | `rustcord.exe` |
| macOS | `Rustcord.app` | `Contents/MacOS/rustcord` |

The Cargo package remains `serein`; the executable target is `rustcord` and is the
package's default run target. Build with `cargo build --locked --release -p serein`.
Use `cargo xtask package` on the target OS for its existing native packaging path.
Linux also supports `--format dir`, `rpm`, `arch` and `appimage`.

For compatibility, the application ID `cz.viceverse.serein`, OS credential namespace,
icon asset filenames and native Linux package names remain stable. They are internal
identifiers, not the displayed product name. Existing credentials and desktop integration
are not migrated or deleted by this rename. The preserved application ID means this fork
should not be assumed to run alongside upstream as an independent account profile.

Source packaging paths are adapted for Linux, Windows and macOS. Only checks actually
reported in the pull request have been run; this does not certify all architectures,
OS versions, media devices or Discord protocol compatibility. Unsigned local builds are
not signed/notarized releases. Fork release artifacts have not been published.

Upstream URLs remain attributed to Serein. Downloading an upstream release still yields
Serein; it does not contain changes from this fork. The in-app updater and AppImage update metadata accept only the fork release repository;
Flatpak references target the fork hosting path. Until fork releases/repositories are published,
checks report that no release is available. No hosting or release publication is performed here.
