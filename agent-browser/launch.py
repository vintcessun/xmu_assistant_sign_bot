# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///
"""起一个带 CDP 调试口的独立 Chrome 窗口，供 probe.py 驱动。

每个角色一个独立 user-data-dir（互不干扰、cookie 不打架），一个固定端口：
  teacher -> 端口 9333，profile chrome-profile-teacher
  student -> 端口 9334，profile chrome-profile-student

窗口起来后是全新 profile（没有你平时的登录/扩展），请在里面手动登录
tronclass.com.tw 的对应账号。登录一次后 cookie 存在该 profile，下次不必重登。

用法：
  uv run agent-browser/launch.py --role teacher
  uv run agent-browser/launch.py --role student
  uv run agent-browser/launch.py --port 9335 --profile chrome-profile-x --url https://www.tronclass.com.tw/login
"""
import argparse
import os
import subprocess
import sys

CHROME = r"C:\Program Files\Google\Chrome\Application\chrome.exe"
HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_URL = "https://www.tronclass.com.tw/login"

ROLES = {
    "teacher": (9333, "chrome-profile-teacher"),
    "student": (9334, "chrome-profile-student"),
}


def main():
    p = argparse.ArgumentParser(description="起带 CDP 的 Chrome 窗口")
    p.add_argument("--role", choices=list(ROLES))
    p.add_argument("--port", type=int)
    p.add_argument("--profile")
    p.add_argument("--url", default=DEFAULT_URL)
    a = p.parse_args()

    if a.role:
        port, profile = ROLES[a.role]
        port = a.port or port
        profile = a.profile or profile
    else:
        if not a.port or not a.profile:
            p.error("要么给 --role，要么同时给 --port 和 --profile")
        port, profile = a.port, a.profile

    if not os.path.isfile(CHROME):
        sys.exit(f"找不到 Chrome：{CHROME}")

    profile_dir = os.path.join(HERE, profile)
    os.makedirs(profile_dir, exist_ok=True)

    cmd = [
        CHROME,
        f"--remote-debugging-port={port}",
        "--remote-allow-origins=*",
        f"--user-data-dir={profile_dir}",
        "--no-first-run",
        "--no-default-browser-check",
        "--new-window",
        a.url,
    ]
    subprocess.Popen(cmd, close_fds=True)
    print(f"已启动 Chrome：角色={a.role or profile}  端口={port}  profile={profile_dir}")
    print(f"CDP: http://127.0.0.1:{port}/json")
    print("请在弹出的窗口里手动登录 tronclass.com.tw 对应账号，登录后即可用 probe.py。")


if __name__ == "__main__":
    main()
