# IngameIME Rust Core

用于 Windows 输入法集成的 Rust 核心库。

## 架构

本仓库分为两层：

- 核心库（`ingameime_core`）：
  - Windows 后端：TSF + IMM32 回退
  - 输入上下文生命周期与候选词/预编辑/输入模式回调
  - 不硬编码 Java 包名或类名
- 可选 JNI 适配层（`jni-adapter` feature）：
  - 通过 `JNI_OnLoad + RegisterNatives` 注册 native 方法
  - 将 Java 回调对象桥接为 Rust 核心回调

## 后端选择

- `api = 0`：优先 TSF，失败后回退 IMM32
- `api = 1`：仅使用 IMM32

## 构建

仅核心库：

```powershell
cargo build
```

构建 JNI 适配层：

```powershell
cargo build --features jni-adapter
cargo build --release --features jni-adapter
```

Linux 交叉编译（Zig）：

```powershell
cargo install cargo-zigbuild --locked
cargo zigbuild --release --target x86_64-unknown-linux-gnu --no-default-features
```

Linux 后端 feature（用于后续实现）：

```powershell
cargo build --features x11
cargo build --features wayland
```

## CI

- Windows 构建（`ingameime_core.dll` + `.pdb`）
- 使用 Zig 的 Linux 交叉编译（`libingameime_core.so`）
- 推送 tag 时会自动上传 release 产物。

## JNI 绑定类（启用 `jni-adapter` 时）

JNI 注册目标类按以下顺序解析：

1. Java 系统属性：`ingameime.jni.bind_class`
2. 编译期环境变量：`INGAMEIME_JNI_BIND_CLASS`

类名可用以下格式：`com.example.YourLibrary` 或 `com/example/YourLibrary`。
