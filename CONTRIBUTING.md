# Contributing to Android18

Thanks for considering it! This gets you from clone to running app in a
few minutes.

## Setup

- **macOS** — the desktop app renders through Metal
- **Rust** — install [rustup](https://rustup.rs); the pinned 1.95 toolchain
  (`rust-toolchain.toml`) installs itself on first `cargo` call
- **Xcode Command Line Tools** — `xcode-select --install`

For the Android side (optional): Android Studio / the Android SDK, and a
device or emulator for `android-service/`.

## Run & develop

```bash
make run    # watch-build-run loop; starts in demo mode (no phone needed)
make test   # Rust workspace tests + Android unit tests
```

## The one hard rule

`crates/core` (**android18-core**) stays **GPUI-free and UI-free** — std
and domain logic only, fully unit-tested. All device I/O flows through the
ports in `crates/core/src/port.rs` (`DeviceBackend`, `SearchProvider`);
adapters (`MockDevice`, the HTTP transport) implement them. The app crate
may depend on the core; never the other way around.

## Before you open a PR

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

All three green, always. New modules ship with unit tests, no `unwrap()` on
user input — errors come back as values.

Commits follow [Conventional Commits](https://www.conventionalcommits.org)
(`feat:`, `fix:`, `refactor:` …), mirroring the existing history.

## Where things live

- `crates/app/src/ui/` — one module per surface (browser, dashboard,
  terminal, …)
- `crates/app/src/theme.rs` — every color token; the single color home
- `docs/ARCHITECTURE.md` — the full system picture
- `docs/DESIGN.md` — the UI spec the app implements

Questions or ideas? Open an issue first — happy to scope it with you.
