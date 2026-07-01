# scripts

`cargo.ps1`：**自动配置 opencv 绑定生成所需的工具链环境变量，然后把参数原样转发给 `cargo`** 的包装器。
Windows / Linux / macOS 通用（需 PowerShell 7+ / `pwsh`）。

## 背景

本项目依赖 [`opencv`](https://crates.io/crates/opencv) crate。它在**绑定生成阶段**会用 `libclang`
解析头文件。若 `LIBCLANG_PATH` 没配好会找不到 libclang；在 Windows 上，当系统 Clang 版本低于
新版 MSVC STL 要求时还会报：

```
error STL1000: Unexpected compiler version, expected Clang 20 or newer.
```

## `cargo.ps1` 做了什么

1. **探测 libclang 并设置 `LIBCLANG_PATH`**：
   - Windows：在 `LIBCLANG_PATH`/`PATH`/常见安装目录中挑**版本最高**的 `clang.exe`；
   - Linux/macOS：用 `llvm-config --libdir` 或常见目录找 `libclang.so*` / `libclang.dylib`。
2. **仅 Windows** 通过 `OPENCV_CLANG_ARGS` 注入官方逃生宏
   `_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH`，放行 Clang 与 MSVC STL 的版本检查
   （非 Windows 无 MSVC，不注入）。MSVC 本身的选取交给 Rust 的 `cc` crate 自动完成（不执行 vcvars）。
3. **把收到的全部参数原样转发给 `cargo`**（用 `Application` 类型定位真正的 cargo，避免同名递归）。

## 用法

任意 `cargo` 子命令都可以，环境变量会自动配好：

```bash
# Windows (PowerShell)
pwsh scripts/cargo.ps1 build --release
pwsh scripts/cargo.ps1 check --message-format=short

# Linux / macOS
pwsh scripts/cargo.ps1 build --release
# 或 chmod +x 后（脚本首行有 pwsh shebang）
./scripts/cargo.ps1 test -- --nocapture
```

## 依赖

- **PowerShell 7+**（`pwsh`）。Linux/macOS 安装：<https://learn.microsoft.com/powershell/scripting/install/installing-powershell>
- **LLVM/Clang**（提供 libclang）。Windows 用 LLVM 官方安装包；Debian/Ubuntu：`apt install libclang-dev clang`。
- 想在 Windows 去掉兼容宏，装 **Clang ≥ 新版 MSVC STL 要求的版本**即可：<https://github.com/llvm/llvm-project/releases>
