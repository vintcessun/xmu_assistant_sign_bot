#!/usr/bin/env pwsh
<#
.SYNOPSIS
    把 build-alinux3.ps1 产出的 `run` 与 `data/lib/` 上传到服务器并重启 systemd 服务。

.DESCRIPTION
    面向 root@vintces.icu 的 `/root/bot` 部署：
      1. 校验本地 `run` 与 `data/lib/`（由 scripts/build-alinux3.ps1 生成）存在；
      2. 以 `.new` 临时名上传（服务照常运行，上传期间零停机）；
      3. 上传成功后才进入停机窗口：停服务 -> 原子 `mv` 就位（旧二进制备份为 `run.bak`）
         -> 整目录替换 `data/lib` -> 启动服务并回显状态。
         上传中断不会破坏现有可用版本；停机只发生在替换这一小段。
    依赖系统自带的 OpenSSH 客户端（`ssh` / `scp`）。首次连接自动接受并固定主机密钥
    （StrictHostKeyChecking=accept-new）。使用密钥或交互式密码认证。

.EXAMPLE
    pwsh scripts/deploy.ps1
    pwsh scripts/deploy.ps1 -IdentityFile ~/.ssh/id_ed25519
    pwsh scripts/deploy.ps1 -HostName 1.2.3.4 -Port 2222 -RemoteBase /opt/bot
#>
param(
    [string]$User = "root",
    [string]$HostName = "vintces.icu",
    [int]$Port = 22,
    [string]$RemoteBase = "/root/bot",
    [string]$Service = "xmu-assistant-bot.service",
    [string]$IdentityFile = "",
    [switch]$NoRestart
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = (Resolve-Path (Join-Path $ScriptDir "..")).Path
$Remote = "$User@$HostName"

# ---- 0. 前置检查：OpenSSH 客户端与本地产物 ----
foreach ($exe in @("ssh", "scp")) {
    if (-not (Get-Command $exe -ErrorAction SilentlyContinue)) {
        throw "未找到 $exe，需要 OpenSSH 客户端；Windows 可在 [设置 - 应用 - 可选功能] 中安装。"
    }
}

$LocalRun = Join-Path $ProjectRoot "run"
$LocalLib = Join-Path $ProjectRoot "data\lib"

if (-not (Test-Path $LocalRun)) {
    throw "本地未找到 run：$LocalRun`n请先运行 scripts/build-alinux3.ps1 生成产物。"
}
if (-not (Test-Path $LocalLib)) {
    throw "本地未找到 data/lib：$LocalLib`n请先运行 scripts/build-alinux3.ps1 生成产物。"
}
$libFiles = Get-ChildItem $LocalLib -File -ErrorAction SilentlyContinue
if ($libFiles.Count -eq 0) {
    throw "data/lib 为空：$LocalLib`n产物不完整，请重新运行 scripts/build-alinux3.ps1。"
}

$runSizeMB = [math]::Round((Get-Item $LocalRun).Length / 1MB, 1)
Write-Host "===== 部署目标 =====" -ForegroundColor Cyan
Write-Host "远端        : ${Remote}:$Port"
Write-Host "远端目录    : $RemoteBase  (run + data/lib)"
Write-Host "服务        : $Service"
Write-Host "本地 run    : $LocalRun  ($runSizeMB MB)"
Write-Host "本地 lib 数 : $($libFiles.Count)"
Write-Host ""

# ---- ssh / scp 公共参数（注意 ssh 用 -p，scp 用 -P）----
$commonOpts = @("-o", "StrictHostKeyChecking=accept-new", "-o", "ConnectTimeout=10")
$sshOpts = $commonOpts + @("-p", "$Port")
$scpOpts = $commonOpts + @("-P", "$Port")
if ($IdentityFile -ne "") {
    $idPath = (Resolve-Path $IdentityFile).Path
    $sshOpts += @("-i", $idPath)
    $scpOpts += @("-i", $idPath)
}

function Invoke-Remote {
    param([string]$Script, [string]$What)
    & ssh @sshOpts $Remote $Script
    if ($LASTEXITCODE -ne 0) { throw "远端步骤失败（$What），exit=$LASTEXITCODE" }
}

# ---- 1. 连通性探测 ----
Write-Host "==> 探测 SSH 连接..." -ForegroundColor DarkGray
Invoke-Remote "echo connected as `$(whoami) on `$(hostname)" "连接测试"

# ---- 2. 准备上传目录（清掉残留的 .new）；不停服务，上传期间保持在线 ----
Write-Host "==> 准备上传目录（不停服务）..." -ForegroundColor DarkGray
$prep = @"
set -e
mkdir -p '$RemoteBase/data'
rm -rf '$RemoteBase/run.new' '$RemoteBase/data/lib.new'
echo '[remote] 上传目录就绪（服务仍在运行，上传期间零停机）'
"@
Invoke-Remote $prep "建目录/清残留"

# ---- 3. 上传（相对路径，避免 Windows 盘符冒号被 scp 当作主机）----
Push-Location $ProjectRoot
try {
    Write-Host "==> 上传 run -> $RemoteBase/run.new ..." -ForegroundColor DarkGray
    & scp @scpOpts "run" "${Remote}:$RemoteBase/run.new"
    if ($LASTEXITCODE -ne 0) { throw "上传 run 失败，exit=$LASTEXITCODE" }

    Write-Host "==> 上传 data/lib -> $RemoteBase/data/lib.new ..." -ForegroundColor DarkGray
    & scp @scpOpts "-r" "data/lib" "${Remote}:$RemoteBase/data/lib.new"
    if ($LASTEXITCODE -ne 0) { throw "上传 data/lib 失败，exit=$LASTEXITCODE" }
}
finally {
    Pop-Location
}

# ---- 4. 关键段（上传已完成）：停服务 -> 替换 -> 启动。停机窗口仅此一小段 ----
if ($NoRestart) {
    Write-Host "==> 就位替换（-NoRestart：不停/不启服务，热替换文件）..." -ForegroundColor DarkGray
    $deploy = @"
set -e
[ -f '$RemoteBase/run' ] && cp -f '$RemoteBase/run' '$RemoteBase/run.bak' || true
mv -f '$RemoteBase/run.new' '$RemoteBase/run'
chmod +x '$RemoteBase/run'
rm -rf '$RemoteBase/data/lib'
mv '$RemoteBase/data/lib.new' '$RemoteBase/data/lib'
echo '[remote] 文件已就位（旧二进制备份为 run.bak）；服务未重启，需手动 systemctl restart $Service 生效'
"@
    Invoke-Remote $deploy "替换文件"
}
else {
    Write-Host "==> 停服务 -> 替换 -> 启动（停机窗口仅此段）..." -ForegroundColor DarkGray
    $deploy = @"
set -e
echo '[remote] 停止服务...'
systemctl stop '$Service' 2>/dev/null || true
[ -f '$RemoteBase/run' ] && cp -f '$RemoteBase/run' '$RemoteBase/run.bak' || true
mv -f '$RemoteBase/run.new' '$RemoteBase/run'
chmod +x '$RemoteBase/run'
rm -rf '$RemoteBase/data/lib'
mv '$RemoteBase/data/lib.new' '$RemoteBase/data/lib'
echo '[remote] 已替换 run 与 data/lib（旧二进制备份为 run.bak）'
echo '[remote] 启动服务...'
systemctl start '$Service'
sleep 1
echo -n '[remote] is-active: '; systemctl is-active '$Service' || true
echo '----- status -----'
systemctl --no-pager --full status '$Service' 2>&1 | head -n 20 || true
"@
    Invoke-Remote $deploy "停服务/替换/启动"
}

Write-Host ""
Write-Host "===== 部署完成 =====" -ForegroundColor Green
Write-Host "如需回滚二进制：ssh $Remote `"systemctl stop $Service && mv -f $RemoteBase/run.bak $RemoteBase/run && systemctl start $Service`""
