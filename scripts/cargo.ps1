#!/usr/bin/env pwsh
<#
.SYNOPSIS
    自动配置 opencv 绑定生成所需的工具链环境变量，然后把所有参数原样转发给 cargo。
    跨平台：Windows 与 Linux/macOS 均可（需 PowerShell 7+ / pwsh）。

.DESCRIPTION
    本项目的 `opencv` crate 在绑定生成阶段用 libclang 解析头文件。本脚本会：
      1. 探测 libclang 并设置 `LIBCLANG_PATH`（Windows 挑版本最高的 clang.exe；Linux/macOS 找 libclang.so/.dylib）；
      2. **仅 Windows**：通过 `OPENCV_CLANG_ARGS` 注入官方逃生宏
         `_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH`（放行 Clang 低于新版 MSVC STL 的版本检查；
         非 Windows 无 MSVC，不注入）；
      3. 把本脚本收到的**全部参数原样转发给 cargo**。
    Windows 的 MSVC 选取交给 Rust 的 cc crate 自动完成（无需手动 vcvars）。

.EXAMPLE
    # Windows
    pwsh scripts/cargo.ps1 build --release
    # Linux（已 chmod +x 时）
    ./scripts/cargo.ps1 check --message-format=short
    pwsh scripts/cargo.ps1 test -- --nocapture
#>

$ErrorActionPreference = 'Stop'

# PowerShell 7 提供 $IsWindows；Windows PowerShell 5.1 无此变量（此时一定是 Windows）。
$onWindows = (-not (Test-Path variable:IsWindows)) -or $IsWindows

function Find-BestClangWindows {
    $candidates = New-Object System.Collections.Generic.List[string]
    if ($env:LIBCLANG_PATH) {
        foreach ($exe in @('clang.exe', '..\bin\clang.exe')) {
            $p = Join-Path $env:LIBCLANG_PATH $exe
            if (Test-Path $p) { $candidates.Add((Resolve-Path $p).Path) }
        }
    }
    $onPath = Get-Command clang.exe -ErrorAction SilentlyContinue
    if ($onPath) { $candidates.Add($onPath.Source) }
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
        try { $verLine = (& $clang --version 2>$null | Select-Object -First 1) } catch { continue }
        if ($verLine -match 'version\s+(\d+)\.(\d+)\.(\d+)') {
            $ver = [version]("{0}.{1}.{2}" -f $matches[1], $matches[2], $matches[3])
            $binDir = Split-Path -Parent $clang
            if ((-not $best) -or ($ver -gt $best.Version)) {
                $best = [pscustomobject]@{ BinDir = $binDir; Version = $ver }
            }
        }
    }
    return $best
}

# 返回包含 libclang 共享库的目录（Linux: libclang.so*；macOS: libclang.dylib）。
function Find-LibclangUnix {
    $pattern = if ($IsMacOS) { 'libclang*.dylib' } else { 'libclang.so*' }

    # a) llvm-config --libdir
    $llvmConfig = Get-Command llvm-config -ErrorAction SilentlyContinue
    if (-not $llvmConfig) { $llvmConfig = Get-Command 'llvm-config-*' -ErrorAction SilentlyContinue | Select-Object -First 1 }
    if ($llvmConfig) {
        try {
            $libdir = (& $llvmConfig.Source --libdir 2>$null | Out-String).Trim()
            if ($libdir -and (Test-Path $libdir) -and
                (Get-ChildItem -LiteralPath $libdir -Filter $pattern -ErrorAction SilentlyContinue)) {
                return $libdir
            }
        } catch {}
    }

    # b) 常见目录（含各版本 llvm-* 的 lib 目录）
    $dirs = @(
        '/usr/lib/x86_64-linux-gnu', '/usr/lib/aarch64-linux-gnu',
        '/usr/lib64', '/usr/lib', '/usr/local/lib', '/lib',
        '/opt/homebrew/opt/llvm/lib', '/usr/local/opt/llvm/lib'
    )
    foreach ($base in @('/usr/lib', '/usr/lib64', '/usr/local')) {
        Get-ChildItem -LiteralPath $base -Directory -Filter 'llvm-*' -ErrorAction SilentlyContinue |
            ForEach-Object { $dirs += (Join-Path $_.FullName 'lib') }
    }
    foreach ($d in $dirs) {
        if ((Test-Path $d) -and
            (Get-ChildItem -LiteralPath $d -Filter $pattern -ErrorAction SilentlyContinue)) {
            return $d
        }
    }
    return $null
}

# 1. 配置 libclang
if ($onWindows) {
    $clang = Find-BestClangWindows
    if ($clang) {
        $env:LIBCLANG_PATH = $clang.BinDir
        Write-Host "==> LIBCLANG_PATH = $($clang.BinDir)  (clang $($clang.Version))" -ForegroundColor DarkGray
    } else {
        Write-Host "[!] 未找到 Clang/libclang，opencv 绑定生成可能失败。安装 LLVM：https://github.com/llvm/llvm-project/releases" -ForegroundColor Yellow
    }

    # 2. 仅 Windows：注入 MSVC STL 版本不匹配的官方逃生宏（保留用户已有设置并补齐）
    $escape = '-D_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH'
    if ($env:OPENCV_CLANG_ARGS) {
        if ($env:OPENCV_CLANG_ARGS -notmatch '_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH') {
            $env:OPENCV_CLANG_ARGS = "$($env:OPENCV_CLANG_ARGS) $escape"
        }
    } else {
        $env:OPENCV_CLANG_ARGS = $escape
    }
} else {
    # Linux/macOS：clang-sys 一般能自动定位 libclang；这里补一层探测以防未装到默认路径。
    if (-not $env:LIBCLANG_PATH) {
        $libdir = Find-LibclangUnix
        if ($libdir) {
            $env:LIBCLANG_PATH = $libdir
            Write-Host "==> LIBCLANG_PATH = $libdir" -ForegroundColor DarkGray
        } else {
            Write-Host "[!] 未找到 libclang，若 opencv 绑定失败请安装（Debian/Ubuntu: apt install libclang-dev clang）。" -ForegroundColor Yellow
        }
    }
    # 非 Windows 无 MSVC，不注入 _ALLOW_COMPILER_AND_STL_VERSION_MISMATCH。
}

# 3. 转发所有参数给真正的 cargo，避免递归解析到 scripts/cargo.cmd / cargo.ps1
function Resolve-RealCargo {
    $scriptDir = Split-Path -Parent $PSCommandPath
    $scriptDirFull = [System.IO.Path]::GetFullPath($scriptDir).TrimEnd('\', '/')

    # 用户显式指定时优先使用
    if ($env:REAL_CARGO -and (Test-Path $env:REAL_CARGO)) {
        return (Resolve-Path $env:REAL_CARGO).Path
    }

    if ($onWindows) {
        # 关键：Windows 下直接找 cargo.exe，不找 cargo，避免匹配到 cargo.cmd
        $commands = Get-Command cargo.exe -CommandType Application -All -ErrorAction SilentlyContinue
    } else {
        $commands = Get-Command cargo -CommandType Application -All -ErrorAction SilentlyContinue
    }

    foreach ($cmd in $commands) {
        $source = $cmd.Source
        if (-not $source) { continue }

        $full = [System.IO.Path]::GetFullPath($source)

        # 排除 scripts 目录里的包装器，防止递归
        if ($full.StartsWith($scriptDirFull, [System.StringComparison]::OrdinalIgnoreCase)) {
            continue
        }

        return $full
    }

    return $null
}

$cargoExe = Resolve-RealCargo
if (-not $cargoExe) {
    Write-Host "[!] 未找到真正的 cargo，请确认 Rust 工具链已安装且在 PATH。" -ForegroundColor Red
    Write-Host "    也可以手动设置 REAL_CARGO，例如：" -ForegroundColor Yellow
    Write-Host '    $env:REAL_CARGO = "C:\Users\vintces\.cargo\bin\cargo.exe"' -ForegroundColor Yellow
    exit 1
}

Write-Host "==> cargo = $cargoExe" -ForegroundColor DarkGray

& $cargoExe @args
exit $LASTEXITCODE