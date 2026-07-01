param(
    [string]$BinaryName = "xmu_assistant_bot",
    [string]$ImageName = "xmu-assistant-bot-alinux3",
    [switch]$NoCache,
    [switch]$SkipImageBuild,
    [string]$Proxy = "http://host.docker.internal:7890",
    [switch]$Clean
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRootPath = (Resolve-Path (Join-Path $ScriptDir "..")).Path
$DockerfilePath = Join-Path $ScriptDir "Dockerfile.alinux3"
$InnerScriptPath = Join-Path $ScriptDir ".build_alinux3_inner.sh"

if (!(Test-Path $DockerfilePath)) {
    throw "Dockerfile not found: $DockerfilePath"
}

Write-Host "===== Project root ====="
Write-Host $ProjectRootPath

Write-Host ""
Write-Host "===== Dockerfile ====="
Write-Host $DockerfilePath

Write-Host ""
Write-Host "===== Binary name ====="
Write-Host $BinaryName

Write-Host ""
Write-Host "===== Docker check ====="
docker version
if ($LASTEXITCODE -ne 0) {
    throw "docker is not available"
}

# =========================
# 1. build docker image
# =========================
if (-not $SkipImageBuild) {
    Write-Host ""
    Write-Host "===== Building Docker image: $ImageName ====="

    $buildArgs = @(
        "build",
        "--progress=plain",
        "-f", $DockerfilePath,
        "-t", $ImageName
    )

    if ($Proxy -ne "") {
        $buildArgs += @(
            "--build-arg", "HTTP_PROXY=$Proxy",
            "--build-arg", "HTTPS_PROXY=$Proxy",
            "--build-arg", "ALL_PROXY=$Proxy",
            "--build-arg", "NO_PROXY=localhost,127.0.0.1,::1"
        )
    }

    if ($NoCache) {
        $buildArgs += "--no-cache"
    }

    $buildArgs += $ProjectRootPath

    docker @buildArgs

    if ($LASTEXITCODE -ne 0) {
        throw "docker build failed"
    }
}

# =========================
# 2. write inner bash script
# =========================
$CleanValue = if ($Clean) { "1" } else { "0" }

$InnerContent = @'
#!/usr/bin/env bash
set -euo pipefail

cd /work

BIN_NAME="${BIN_NAME:-xmu_assistant_bot}"
CLEAN="${CLEAN:-0}"

echo "===== Container env ====="
cat /etc/os-release || true
echo
uname -m
echo
ldd --version | head -n 1 || true
echo

echo "===== Toolchain ====="
command -v rustc || true
command -v cargo || true
command -v clang || true
command -v clang++ || true
command -v pkg-config || true
command -v cmake || true
command -v mold || true
command -v ld.lld || true
echo

rustc --version || true
cargo --version || true
clang --version | head -n 1 || true
pkg-config --version || true
echo

echo "===== Workdir ====="
pwd
ls -la

if [ "$CLEAN" = "1" ]; then
  echo
  echo "===== cargo clean ====="
  cargo clean
fi

echo
echo "===== Configure build env ====="

# 对齐 GitHub Actions：OpenSSL 使用系统包，静态链接 OpenSSL。
export OPENSSL_STATIC=1
export OPENSSL_NO_VENDOR=1
export OPENSSL_DIR=/usr
export OPENSSL_LIB_DIR=/usr/lib64
export OPENSSL_INCLUDE_DIR=/usr/include

# 对齐 GitHub Actions：native C/C++ 依赖走 clang。
# 这也绕开 aws-lc-sys 对 alinux3 gcc 的 memcmp bug 检测。
export CC=clang
export CXX=clang++
export AR=ar

export CC_x86_64_unknown_linux_gnu=clang
export CXX_x86_64_unknown_linux_gnu=clang++

export AWS_LC_SYS_CC=clang
export AWS_LC_SYS_CXX=clang++
export AWS_LC_SYS_CC_x86_64_unknown_linux_gnu=clang
export AWS_LC_SYS_CXX_x86_64_unknown_linux_gnu=clang++

# CMake probe 也显式走 clang。
export CMAKE_C_COMPILER=clang
export CMAKE_CXX_COMPILER=clang++

# pkg-config：按系统 devel 包查找 OpenCV / FFmpeg / OpenSSL。
# Dockerfile 里应该安装 opencv-devel、ffmpeg-devel、openssl-devel、clang-devel。
export PKG_CONFIG_PATH="/usr/local/lib64/pkgconfig:/usr/local/lib/pkgconfig:/usr/lib64/pkgconfig:/usr/share/pkgconfig:${PKG_CONFIG_PATH:-}"
export PKG_CONFIG_ALLOW_SYSTEM_LIBS=1
export PKG_CONFIG_ALLOW_SYSTEM_CFLAGS=1

# ffmpeg-sys-next 兼容：避免寻找新版本 FFmpeg 里已经废弃/移除的 avfft。
export FFMPEG_NO_AVFFT=1

# OpenCV 默认动态链接，最后由本脚本复制 .so 到 ./data/lib。
export OPENCV_DYNAMIC=1

# bindgen / opencv-rust / clang-sys 查找 libclang。
LIBCLANG_SO="$(find /usr/lib64 /usr/lib -name 'libclang.so*' 2>/dev/null | head -n 1 || true)"
if [ -n "$LIBCLANG_SO" ]; then
  export LIBCLANG_PATH="$(dirname "$LIBCLANG_SO")"
fi

# OpenCV 头文件常见位置。
export BINDGEN_EXTRA_CLANG_ARGS="-I/usr/include -I/usr/include/opencv4 ${BINDGEN_EXTRA_CLANG_ARGS:-}"

# CMake 查找 OpenCVConfig.cmake 的常见路径。
export CMAKE_PREFIX_PATH="/usr:/usr/lib64/cmake:/usr/share/cmake:${CMAKE_PREFIX_PATH:-}"

# run 在项目根目录，所以 $ORIGIN/data/lib 正好对应 ./data/lib。
# 链接器对齐 GitHub Actions：优先 mold，其次 lld，最后普通 clang 链接。
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=clang

if command -v mold >/dev/null 2>&1; then
  export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-fuse-ld=mold -C link-arg=-Wl,-rpath,\$ORIGIN/data/lib"
elif command -v ld.lld >/dev/null 2>&1; then
  export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-fuse-ld=lld -C link-arg=-Wl,-rpath,\$ORIGIN/data/lib"
else
  export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,-rpath,\$ORIGIN/data/lib"
fi

export LD_LIBRARY_PATH="/usr/local/lib64:/usr/local/lib:/usr/lib64:/work/data/lib:/work/lib:/work/libs:/work/vendor/lib:/work/native/lib:/work/target/release:/work/target/release/deps:${LD_LIBRARY_PATH:-}"
export LIBRARY_PATH="/usr/local/lib64:/usr/local/lib:/usr/lib64:/work/data/lib:/work/lib:/work/libs:/work/vendor/lib:/work/native/lib:/work/target/release:/work/target/release/deps:${LIBRARY_PATH:-}"

export CPATH="/usr/local/include:/usr/local/include/opencv4:${CPATH:-}"
export OPENCV_PACKAGE_NAME=opencv4

echo "===== Verify OpenCV ====="
pkg-config --modversion opencv4
pkg-config --libs opencv4
find /usr/local -name 'libopencv_wechat_qrcode.so*' -print

echo "BIN_NAME=$BIN_NAME"
echo "OPENSSL_STATIC=$OPENSSL_STATIC"
echo "OPENSSL_NO_VENDOR=$OPENSSL_NO_VENDOR"
echo "OPENSSL_DIR=$OPENSSL_DIR"
echo "OPENSSL_LIB_DIR=$OPENSSL_LIB_DIR"
echo "OPENSSL_INCLUDE_DIR=$OPENSSL_INCLUDE_DIR"
echo "CC=$CC"
echo "CXX=$CXX"
echo "CMAKE_C_COMPILER=$CMAKE_C_COMPILER"
echo "CMAKE_CXX_COMPILER=$CMAKE_CXX_COMPILER"
echo "PKG_CONFIG_PATH=$PKG_CONFIG_PATH"
echo "LIBCLANG_PATH=${LIBCLANG_PATH:-}"
echo "BINDGEN_EXTRA_CLANG_ARGS=$BINDGEN_EXTRA_CLANG_ARGS"
echo "FFMPEG_NO_AVFFT=$FFMPEG_NO_AVFFT"
echo "OPENCV_DYNAMIC=$OPENCV_DYNAMIC"
echo "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=$CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER"
echo "RUSTFLAGS=$RUSTFLAGS"

export PKG_CONFIG_PATH="/usr/local/lib64/pkgconfig:/usr/local/lib/pkgconfig:/usr/lib64/pkgconfig:/usr/share/pkgconfig:${PKG_CONFIG_PATH:-}"
export LD_LIBRARY_PATH="/usr/local/lib64:/usr/local/lib:/usr/lib64:/work/data/lib:/work/lib:/work/libs:/work/vendor/lib:/work/native/lib:/work/target/release:/work/target/release/deps:${LD_LIBRARY_PATH:-}"
export LIBRARY_PATH="/usr/local/lib64:/usr/local/lib:/usr/lib64:/work/data/lib:/work/lib:/work/libs:/work/vendor/lib:/work/native/lib:/work/target/release:/work/target/release/deps:${LIBRARY_PATH:-}"
export CPATH="/usr/local/include:/usr/local/include/opencv4:${CPATH:-}"

export OPENCV_PACKAGE_NAME=opencv4

pkg-config --modversion opencv4
pkg-config --libs opencv4
find /usr/local -name 'libopencv_wechat_qrcode.so*' -print

echo
echo "===== Native dependency check: OpenCV ====="

if pkg-config --exists opencv4; then
  export OPENCV_PKGCONFIG_NAME=opencv4
  export OPENCV_PACKAGE_NAME=opencv4
  pkg-config --modversion opencv4
  pkg-config --libs --cflags opencv4
elif pkg-config --exists opencv; then
  export OPENCV_PKGCONFIG_NAME=opencv
  export OPENCV_PACKAGE_NAME=opencv
  pkg-config --modversion opencv
  pkg-config --libs --cflags opencv
else
  echo "ERROR: neither opencv4.pc nor opencv.pc was found."
  echo
  echo "Current PKG_CONFIG_PATH:"
  echo "$PKG_CONFIG_PATH" | tr ':' '\n'
  echo
  echo "Try these inside Dockerfile:"
  echo "  dnf install -y opencv-devel"
  echo
  echo "Debug commands:"
  echo "  dnf provides '*/opencv4.pc'"
  echo "  dnf provides '*/opencv.pc'"
  echo "  dnf provides '*/OpenCVConfig.cmake'"
  exit 1
fi

echo "OPENCV_PKGCONFIG_NAME=$OPENCV_PKGCONFIG_NAME"
echo "OPENCV_PACKAGE_NAME=$OPENCV_PACKAGE_NAME"

echo
echo "===== Native dependency check: FFmpeg ====="

pkg-config --modversion libavutil
pkg-config --modversion libavcodec
pkg-config --modversion libavformat
pkg-config --modversion libswscale
pkg-config --modversion libswresample

pkg-config --libs --cflags libavutil libavcodec libavformat libavdevice libavfilter libswresample libswscale

echo
echo "===== Cargo build ====="

cargo build --release --bin "$BIN_NAME"

SRC_BIN="target/release/$BIN_NAME"

if [ ! -f "$SRC_BIN" ]; then
  echo
  echo "ERROR: binary not found: $SRC_BIN"
  echo "Available files in target/release:"
  find target/release -maxdepth 1 -type f -printf "%f\n" 2>/dev/null | sort || true
  exit 1
fi

echo
echo "===== Prepare output on mounted /work ====="

rm -f ./run
mkdir -p ./data/lib
rm -rf ./data/lib/*

cp -v "$SRC_BIN" ./run
chmod +x ./run

# 强制写入运行时 rpath。
patchelf --set-rpath '$ORIGIN/data/lib' ./run

echo
echo "===== Initial ldd ====="
LD_LIBRARY_PATH="$LD_LIBRARY_PATH" ldd ./run | tee ./data/ldd.before-package.txt

if grep -q "not found" ./data/ldd.before-package.txt; then
  echo
  echo "ERROR: some libraries are not found before packaging."
  echo "Put missing .so files under one of:"
  echo "  ./data/lib"
  echo "  ./lib"
  echo "  ./libs"
  echo "  ./vendor/lib"
  echo "  ./native/lib"
  exit 1
fi

is_glibc_family() {
  local base="$1"

  case "$base" in
    libc.so*|ld-linux*.so*|libpthread.so*|libdl.so*|libm.so*|librt.so*|libresolv.so*|libutil.so*|libnss_*.so*|libanl.so*|libcrypt.so*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

copy_one() {
  local src="$1"
  local base
  base="$(basename "$src")"

  if is_glibc_family "$base"; then
    return 0
  fi

  if [ ! -f "$src" ]; then
    return 0
  fi

  if [ ! -e "./data/lib/$base" ]; then
    echo "copy $src -> ./data/lib/$base"
    cp -Lv "$src" "./data/lib/$base"
    copy_deps "$src" || true
  fi
}

copy_deps() {
  local file="$1"

  LD_LIBRARY_PATH="$LD_LIBRARY_PATH" ldd "$file" 2>/dev/null | awk '
    $2 == "=>" && $3 ~ /^\// { print $3 }
    $1 ~ /^\// { print $1 }
  ' | sort -u | while read -r lib; do
    copy_one "$lib"
  done
}

copy_pkgconfig_libs() {
  local pc_name="$1"

  pkg-config --libs "$pc_name" 2>/dev/null | tr ' ' '\n' | while read -r item; do
    case "$item" in
      -L*)
        echo "${item#-L}" >> /tmp/pkg_lib_dirs.txt
        ;;
    esac
  done

  pkg-config --libs "$pc_name" 2>/dev/null | tr ' ' '\n' | while read -r item; do
    case "$item" in
      -l*)
        local libname
        libname="${item#-l}"

        if [ -f /tmp/pkg_lib_dirs.txt ]; then
          while read -r dir; do
            [ -d "$dir" ] || continue

            for candidate in "$dir/lib${libname}.so" "$dir/lib${libname}.so."*; do
              if [ -e "$candidate" ]; then
                copy_one "$candidate"
                break
              fi
            done
          done < /tmp/pkg_lib_dirs.txt
        fi
        ;;
    esac
  done

  rm -f /tmp/pkg_lib_dirs.txt
}

echo
echo "===== Copy non-glibc shared libraries from ldd ====="
copy_deps ./run

echo
echo "===== Copy OpenCV / FFmpeg pkg-config shared libraries ====="
copy_pkgconfig_libs "$OPENCV_PKGCONFIG_NAME" || true

for pc in libavutil libavcodec libavformat libavdevice libavfilter libswresample libswscale; do
  copy_pkgconfig_libs "$pc" || true
done

echo
echo "===== Copy deps of packaged libraries ====="
find ./data/lib -maxdepth 1 -type f -name '*.so*' -print | while read -r so; do
  copy_deps "$so" || true
done

echo
echo "===== Packaged libs ====="
find ./data/lib -maxdepth 1 -type f -printf "%f\n" | sort || true

echo
echo "===== RUNPATH / NEEDED ====="
readelf -d ./run | grep -E 'RUNPATH|RPATH|NEEDED' || true

echo
echo "===== Final ldd with ./data/lib ====="
LD_LIBRARY_PATH="/work/data/lib:$LD_LIBRARY_PATH" ldd ./run | tee ./data/ldd.after-package.txt

if grep -q "not found" ./data/ldd.after-package.txt; then
  echo
  echo "ERROR: some libraries are still not found after packaging."
  exit 1
fi

echo
echo "===== Required GLIBC versions ====="
strings ./run 2>/dev/null | grep -E 'GLIBC_[0-9]' | sort -V | uniq | tail -n 20 || true

echo
echo "===== Required GLIBCXX versions ====="
strings ./run ./data/lib/libstdc++.so.6 2>/dev/null | grep -E 'GLIBCXX_[0-9]' | sort -V | uniq | tail -n 20 || true

echo
echo "===== Output in container ====="
ls -lh ./run
echo
find ./data/lib -maxdepth 1 -type f -printf "%p\n" | sort || true

echo
echo "OK: built /work/run and copied non-glibc libs to /work/data/lib"
'@

# 写成 UTF-8 无 BOM + LF，避免 bash 读 Windows CRLF/BOM 出问题
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$InnerContent = $InnerContent -replace "`r`n", "`n"
[System.IO.File]::WriteAllText($InnerScriptPath, $InnerContent, $Utf8NoBom)

Write-Host ""
Write-Host "===== Inner script ====="
Write-Host $InnerScriptPath

# =========================
# 3. run build inside container
# =========================
Write-Host ""
Write-Host "===== Running build in container ====="

$mountArg = "type=bind,source=$ProjectRootPath,target=/work"

$runArgs = @(
    "run",
    "--rm",
    "-e", "BIN_NAME=$BinaryName",
    "-e", "CLEAN=$CleanValue"
)

if ($Proxy -ne "") {
    $runArgs += @(
        "-e", "HTTP_PROXY=$Proxy",
        "-e", "HTTPS_PROXY=$Proxy",
        "-e", "ALL_PROXY=$Proxy",
        "-e", "http_proxy=$Proxy",
        "-e", "https_proxy=$Proxy",
        "-e", "all_proxy=$Proxy",
        "-e", "NO_PROXY=localhost,127.0.0.1,::1,host.docker.internal",
        "-e", "no_proxy=localhost,127.0.0.1,::1,host.docker.internal"
    )
}

$runArgs += @(
    "--mount", $mountArg,
    "-w", "/work",
    $ImageName,
    "bash", "/work/scripts/.build_alinux3_inner.sh"
)

Write-Host ("docker " + ($runArgs -join " "))

docker @runArgs

if ($LASTEXITCODE -ne 0) {
    throw "docker run build failed"
}

# =========================
# 4. host-side validation
# =========================
$RunPath = Join-Path $ProjectRootPath "run"
$LibDir = Join-Path $ProjectRootPath "data\lib"

Write-Host ""
Write-Host "===== Host-side check ====="

if (!(Test-Path $RunPath)) {
    throw "run was not created on host: $RunPath"
}

if (!(Test-Path $LibDir)) {
    throw "data/lib was not created on host: $LibDir"
}

$LibFiles = Get-ChildItem $LibDir -File -ErrorAction SilentlyContinue

Write-Host "run => $RunPath"
Write-Host "libs => $LibDir"
Write-Host "lib count => $($LibFiles.Count)"

if ($LibFiles.Count -eq 0) {
    Write-Host "WARNING: data/lib is empty. This is only OK if the binary has no non-glibc dynamic deps."
}

Write-Host ""
Write-Host "===== BUILD COMPLETE ====="
Write-Host "Upload these to server:"
Write-Host "  run"
Write-Host "  data/lib/"