# 1. 查找正在运行的 run.exe
$targets = Get-CimInstance Win32_Process -Filter "Name='run.exe'" |
Select-Object `
@{n = "PID"; e = { $_.ProcessId } },
Name,
ExecutablePath,
CommandLine,
@{n = "WorkingSetMB"; e = { [math]::Round($_.WorkingSetSize / 1MB, 2) } },
@{n = "PrivateMemoryMB"; e = { [math]::Round($_.PrivatePageCount / 1MB, 2) } }

if (-not $targets) {
    Write-Host "当前没有正在运行的 run.exe 进程。" -ForegroundColor Yellow
    Write-Host "下面开始在常见目录搜索 run.exe 文件位置..."
    
    Get-ChildItem `
        -Path "$env:ProgramFiles", "$env:ProgramFiles(x86)", "$env:LOCALAPPDATA", "$env:APPDATA", "$env:USERPROFILE\Downloads", "$env:USERPROFILE\Desktop", "C:\Windows" `
        -Filter "run.exe" `
        -File `
        -Recurse `
        -Force `
        -ErrorAction SilentlyContinue |
    Select-Object FullName, Length, CreationTime, LastWriteTime |
    Format-Table -AutoSize

    return
}

Write-Host "`n找到正在运行的 run.exe：" -ForegroundColor Green
$targets | Format-Table -AutoSize

# 2. 如果只有一个 run.exe，自动选中；如果多个，让你手动输入 PID
if (($targets | Measure-Object).Count -eq 1) {
    $pidToWatch = $targets.PID
    Write-Host "`n只有一个 run.exe，自动监测 PID: $pidToWatch" -ForegroundColor Green
}
else {
    $pidToWatch = Read-Host "`n有多个 run.exe，请输入要监测的 PID"
}

# 3. 创建日志文件
$log = "run.exe_mem_$pidToWatch.csv"

Write-Host "`n开始监测 PID=$pidToWatch 的内存变化，每 1 秒记录一次。" -ForegroundColor Green
Write-Host "日志保存到：$log"
Write-Host "按 Ctrl + C 停止。`n"

# 4. 循环监测
while ($true) {
    $procInfo = Get-CimInstance Win32_Process -Filter "ProcessId=$pidToWatch" -ErrorAction SilentlyContinue
    $p = Get-Process -Id $pidToWatch -ErrorAction SilentlyContinue

    if (-not $p) {
        Write-Host "进程已经退出，停止监测。" -ForegroundColor Yellow
        break
    }

    $row = [pscustomobject]@{
        Time            = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
        PID             = $pidToWatch
        Name            = $p.ProcessName
        Path            = $procInfo.ExecutablePath
        WorkingSetMB    = [math]::Round($p.WorkingSet64 / 1MB, 2)
        PrivateMemoryMB = [math]::Round($p.PrivateMemorySize64 / 1MB, 2)
        PagedMemoryMB   = [math]::Round($p.PagedMemorySize64 / 1MB, 2)
        VirtualMemoryMB = [math]::Round($p.VirtualMemorySize64 / 1MB, 2)
        CPUSeconds      = $p.CPU
    }

    Clear-Host
    Write-Host "正在监测 run.exe，PID=$pidToWatch，按 Ctrl + C 停止。`n" -ForegroundColor Cyan
    $row | Format-List

    $row | Export-Csv $log -Append -NoTypeInformation -Encoding UTF8

    Start-Sleep -Seconds 1
}