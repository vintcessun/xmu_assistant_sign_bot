# scripts

构建辅助脚本：自动探测合适的 MSVC / Clang 工具链，并完成 `cargo build`。

## 背景

本项目依赖 [`opencv`](https://crates.io/crates/opencv) crate。它在**绑定生成阶段**会用 `libclang`
解析 OpenCV 与 MSVC 的 C++ 头文件。当系统安装的 **Clang 版本低于新版 MSVC STL 要求的版本**时，
头文件解析会失败并报错：

```
error STL1000: Unexpected compiler version, expected Clang 20 or newer.
```

（例如：Visual Studio 18 / MSVC 14.51 的 STL 要求 Clang ≥ 20，而本机仅有 Clang 19。）

## 解决方案

`build.ps1` 会：

1. 通过 `vswhere` **探测**（不修改环境）Visual Studio MSVC 工具链——MSVC 的实际选取交给
   Rust 的 `cc` crate 自动完成（它会通过注册表/vswhere 找到 `cl.exe` 及对应的 `INCLUDE`/`LIB`）。
   > 注意：脚本**故意不执行 `vcvars64.bat`**。强行导入 vcvars 会打乱 `PATH`/`INCLUDE` 顺序，
   > 导致 `ffmpeg-sys-next` 等依赖误用 msys64/MinGW 头文件而编译失败。
2. 在 `LIBCLANG_PATH`、`PATH`、常见安装目录中探测可用的 Clang，挑选**版本最高**的一个，
   并设置 `LIBCLANG_PATH`；
3. 当 Clang 与 MSVC STL 版本不匹配时，自动通过 `OPENCV_CLANG_ARGS` 注入 MSVC 官方逃生宏
   `_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH`，放行 libclang 解析
   （真正编译绑定时使用 `cl.exe`，不受该版本检查影响）；
4. 执行 `cargo build`（默认 `--release`）。

## 用法

PowerShell：

```powershell
# release 构建（默认）
pwsh -File scripts/build.ps1

# debug 构建
pwsh -File scripts/build.ps1 -Profile debug

# 透传额外 cargo 参数
pwsh -File scripts/build.ps1 -- -vv
```

cmd / 双击：

```bat
scripts\build.bat
scripts\build.bat debug
```

## 彻底修复（可选）

若希望去掉兼容宏，安装 **Clang ≥ MSVC STL 要求的版本**即可：
<https://github.com/llvm/llvm-project/releases>。安装后脚本会自动选用更高版本的 Clang。
