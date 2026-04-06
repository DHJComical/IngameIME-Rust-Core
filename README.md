# IngameIME Rust Core

Rust core for Windows IME integration.

## Architecture

This crate is split into:

- Core library (`ingameime_core`):
  - Windows backends: TSF + IMM32 fallback
  - Context lifecycle and candidate/preedit/input-mode callbacks
  - No hardcoded Java package/class dependency
- Optional JNI adapter (`jni-adapter` feature):
  - Registers native methods via `JNI_OnLoad + RegisterNatives`
  - Bridges Java callback objects to core Rust callbacks

## Backend Selection

- `api = 0`: TSF first, fallback to IMM32 on failure
- `api = 1`: IMM32 only

## Build

Core only:

```powershell
cargo build
```

JNI adapter build:

```powershell
cargo build --features jni-adapter
cargo build --release --features jni-adapter
```

Linux cross-build (Zig):

```powershell
cargo install cargo-zigbuild --locked
cargo zigbuild --release --target x86_64-unknown-linux-gnu --no-default-features
```

Linux backend feature flags (for upcoming implementation):

```powershell
cargo build --features x11
cargo build --features wayland
```

## CI

- Windows build (`ingameime_core.dll` + `.pdb`)
- Linux cross-build via Zig (`libingameime_core.so`)
- On tag push, release artifacts are uploaded automatically.

## JNI Bind Class (when `jni-adapter` enabled)

JNI registration target class is resolved in this order:

1. Java system property: `ingameime.jni.bind_class`
2. Compile-time env var: `INGAMEIME_JNI_BIND_CLASS`

Accepted format: `com.example.YourLibrary` or `com/example/YourLibrary`.
