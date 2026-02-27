import time
import shutil
import subprocess
import sys
from pathlib import Path

# --- 配置区 ---
# 你的原始编译输出文件
SOURCE_EXE = Path("target/release/xmu_assistant_bot.exe")
# 实际运行的副本文件
RUN_EXE = Path("run.exe")
# 检测间隔（秒）
CHECK_INTERVAL = 1.0
# 等待编译完全写入的缓冲时间（秒）
WRITE_BUFFER_TIME = 2.0
# --------------

current_process = None
last_mtime = 0


def log(msg: str, color: str = "white"):
    """简单的带颜色日志输出"""
    colors = {
        "green": "\033[92m",
        "yellow": "\033[93m",
        "red": "\033[91m",
        "cyan": "\033[96m",
        "reset": "\033[0m",
    }
    timestamp = time.strftime("%H:%M:%S")
    print(f"{colors.get(color, '')}[{timestamp}] {msg}{colors['reset']}")


def stop_process():
    """停止当前运行的 run.exe"""
    global current_process
    if current_process and current_process.poll() is None:
        log("正在停止旧进程...", "yellow")
        try:
            current_process.terminate()
            current_process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            log("进程停止超时，强制查杀...", "red")
            current_process.kill()
        except Exception as e:
            log(f"停止进程出错: {e}", "red")

    current_process = None


def start_process():
    """复制并启动新进程"""
    global current_process

    stop_process()

    if SOURCE_EXE.exists():
        try:
            log(f"正在复制: {SOURCE_EXE.name} -> {RUN_EXE.name}", "cyan")
            # 确保目录存在
            RUN_EXE.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(SOURCE_EXE, RUN_EXE)
        except Exception as e:
            log(f"复制文件失败: {e}", "red")
            return
    elif RUN_EXE.exists():
        log(f"源文件不存在，将直接使用现有的 {RUN_EXE.name}", "yellow")
    else:
        log(f"错误: {SOURCE_EXE.name} 和 {RUN_EXE.name} 都不存在", "red")
        return

    log("🚀 启动新版本...", "green")
    try:
        current_process = subprocess.Popen([str(RUN_EXE)])
    except Exception as e:
        log(f"启动失败: {e}", "red")


def main():
    global last_mtime

    if not SOURCE_EXE.exists():
        if RUN_EXE.exists():
            log(
                f"警告: 找不到源文件 {SOURCE_EXE}，但发现已存在 {RUN_EXE}，将忽略并继续",
                "yellow",
            )
        else:
            log(f"错误: 找不到源文件 {SOURCE_EXE} 且 {RUN_EXE} 不存在，未编译", "red")
            log("请先运行一次: cargo build --release", "yellow")
            return
    else:
        last_mtime = SOURCE_EXE.stat().st_mtime

    start_process()

    log(f"👀 正在监控 {SOURCE_EXE} 的变化...", "cyan")

    try:
        while True:
            time.sleep(CHECK_INTERVAL)

            # 1. 检测文件变化（热重载）
            if SOURCE_EXE.exists():
                current_mtime = SOURCE_EXE.stat().st_mtime
                if current_mtime != last_mtime:
                    log("检测到编译产物变化！", "yellow")
                    time.sleep(WRITE_BUFFER_TIME)
                    last_mtime = SOURCE_EXE.stat().st_mtime
                    start_process()
                    continue

            # 2. 检测进程是否结束（自动重启）
            if current_process is not None:
                exit_code = current_process.poll()
                if exit_code is not None:
                    log(f"进程已退出，退出码: {exit_code}。正在重启...", "yellow")
                    start_process()

    except KeyboardInterrupt:
        log("\n正在退出监视脚本...", "yellow")
        stop_process()
        sys.exit(0)


if __name__ == "__main__":
    main()
