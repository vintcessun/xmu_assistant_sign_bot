#!/usr/bin/env pwsh
<#
.SYNOPSIS
    探测合适的 MSVC / Clang 工具链并执行 cargo build。

.DESCRIPTION
    本项目依赖 `opencv` crate，其绑定生成阶段使用 libclang 解析 OpenCV 与 MSVC 头文件。
    当系统中的 Clang 版本低于新版 MSVC STL 所要求的版本时，会触发：
        error STL1000: Unexpected compiler version, expected Clang XX or newer.
    本脚本会：
      1. 通过 vswhere 定位 Visual Studio，并导入其 MSVC 编译环境（cl.exe / INCLUDE / LIB）。
      2. 探测可用的 Clang/libclang，挑选版本最高的一个并设置 LIBCLANG_PATH。
      3. 若 Clang 与 MSVC STL 版本不匹配，自动注入官方逃生宏
         `_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH`（通过 OPENCV_CLANG_ARGS）以放行解析。
      4. 执行 `cargo build`（默认 --release）。

.PARAMETER Profile
    构建配置：release（默认）或 debug。

.PARAMETER CargoArgs
    透传给 cargo 的其余参数，例如 `-- --features xxx`。

.EXAMPLE
    pwsh -File scripts/build.ps1
    pwsh -File scripts/build.ps1 -Profile debug
    pwsh -File scripts/build.ps1 -- -vv
#>
[CmdletBinding()]
param(
    [ValidateSet('release', 'debug')]
    [string]$Profile = 'release',

    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CargoArgs
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot

function Write-Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
function Write-Info($msg) { Write-Host "    $msg" -ForegroundColor DarkGray }
function Write-Warn2($msg) { Write-Host "[!] $msg" -ForegroundColor Yellow }

# ---------------------------------------------------------------------------
# 1. 探测 Visual Studio (MSVC)  —— 只探测、不修改环境
# ---------------------------------------------------------------------------
# 说明：Rust 的 `cc` crate 会通过注册表 / vswhere 自动定位 MSVC（cl.exe、INCLUDE、LIB），
#       无需手动执行 vcvars64.bat。强行导入 vcvars 反而可能打乱 PATH/INCLUDE 顺序，
#       让 ffmpeg-sys-next 之类的依赖误用到 msys64/MinGW 头文件而编译失败。
#       因此这里仅做“探测并提示”，把 MSVC 的选取交给 cc 自动完成。
function Probe-Msvc {
    Write-Step "探测 MSVC 工具链"
    if (Get-Command cl.exe -ErrorAction SilentlyContinue) {
        Write-Info "cl.exe 已在 PATH 中（将沿用当前 Developer 环境）。"
        return
    }
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $installPath = & $vswhere -latest -products * `
            -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
            -property installationPath 2>$null
        if (-not $installPath) {
            $installPath = & $vswhere -latest -products * -property installationPath 2>$null
        }
        if ($installPath) {
            Write-Info "已检测到 Visual Studio: $installPath"
            Write-Info "cc crate 将自动选用其 MSVC 工具链（无需 vcvars）。"
            return
        }
    }
    Write-Warn2 "未检测到 Visual Studio MSVC 工具链。请安装 'Desktop development with C++' 工作负载。"
}

# ---------------------------------------------------------------------------
# 2. 探测最合适的 Clang / libclang
# ---------------------------------------------------------------------------
function Find-BestClang {
    $candidates = New-Object System.Collections.Generic.List[string]

    # a) 已有的 LIBCLANG_PATH
    if ($env:LIBCLANG_PATH) {
        foreach ($exe in @('clang.exe', '..\bin\clang.exe')) {
            $p = Join-Path $env:LIBCLANG_PATH $exe
            if (Test-Path $p) { $candidates.Add((Resolve-Path $p).Path) }
        }
    }
    # b) PATH 上的 clang
    $onPath = Get-Command clang.exe -ErrorAction SilentlyContinue
    if ($onPath) { $candidates.Add($onPath.Source) }
    # c) 常见安装目录
    foreach ($d in @(
            'C:\Program Files\LLVM\bin',
            'C:\Program Files (x86)\LLVM\bin',
            'D:\Software\LLVM\bin',
            "$env:ProgramFiles\LLVM\bin")) {
        $p = Join-Path $d 'clang.exe'
        if (Test-Path $p) { $candidates.Add($p) }
    }

    $best = $null
    foreach ($clang in ($candidates | Select-Object -Unique)) {
        try {
            $verLine = (& $clang --version 2>$null | Select-Object -First 1)
        } catch { continue }
        if ($verLine -match 'version\s+(\d+)\.(\d+)\.(\d+)') {
            $major = [int]$matches[1]
            $ver = [version]("{0}.{1}.{2}" -f $matches[1], $matches[2], $matches[3])
            $binDir = Split-Path -Parent $clang
            if ((-not $best) -or ($ver -gt $best.Version)) {
                $best = [pscustomobject]@{
                    Exe     = $clang
                    BinDir  = $binDir
                    Major   = $major
                    Version = $ver
                    Line    = $verLine
                }
            }
        }
    }
    return $best
}

# ---------------------------------------------------------------------------
# 主流程
# ---------------------------------------------------------------------------
Push-Location $RepoRoot
try {
    Probe-Msvc

    Write-Step "探测 Clang / libclang"
    $clang = Find-BestClang
    if (-not $clang) {
        Write-Warn2 "未找到任何 Clang 安装。opencv 绑定生成需要 libclang，请安装 LLVM。"
        Write-Warn2 "下载：https://github.com/llvm/llvm-project/releases"
        exit 1
    }
    Write-Info $clang.Line
    Write-Info "libclang 目录: $($clang.BinDir)"
    $env:LIBCLANG_PATH = $clang.BinDir

    # opencv 解析阶段需要的额外 clang 参数（保留用户已有设置）
    $extra = '-D_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH'
    if ($clang.Major -lt 20) {
        Write-Warn2 "Clang $($clang.Version) 低于新版 MSVC STL 期望的版本(>=20)，注入兼容宏以放行 libclang 解析。"
    }
    if ($env:OPENCV_CLANG_ARGS) {
        if ($env:OPENCV_CLANG_ARGS -notmatch '_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH') {
            $env:OPENCV_CLANG_ARGS = "$($env:OPENCV_CLANG_ARGS) $extra"
        }
    } else {
        $env:OPENCV_CLANG_ARGS = $extra
    }
    Write-Info "OPENCV_CLANG_ARGS = $($env:OPENCV_CLANG_ARGS)"

    # -----------------------------------------------------------------------
    # 执行 cargo build
    # -----------------------------------------------------------------------
    $buildArgs = @('build')
    if ($Profile -eq 'release') { $buildArgs += '--release' }
    if ($CargoArgs) { $buildArgs += $CargoArgs }

    Write-Step "cargo $($buildArgs -join ' ')"
    & cargo @buildArgs
    $code = $LASTEXITCODE
    if ($code -ne 0) {
        Write-Warn2 "构建失败，退出码 $code"
        exit $code
    }
    Write-Step "构建成功 ✓"
}
finally {
    Pop-Location
}
