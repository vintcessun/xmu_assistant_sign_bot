# agent-browser

Agent 调试用的浏览器工具：通过 Chrome 的 CDP 调试口，在**已登录**的 tronclass
标签页上下文里直接发 API 请求（自动带该窗口 cookie）。用于探测 TronClass 二维码
签到机制——尤其是「一枚 `data` 生成后多久内提交还算数」的**接受窗口**，以及验证
「教师采集 data → 学生提交」这条**无需扫码**链路是否成立。

全程走 CDP，独立于项目主程序与浏览器扩展；**只对自有测试租户（tronclass.com.tw）
的自有账号做实验，用完即停，绝不碰真实 XMU 点名。**

## 依赖

- [uv](https://docs.astral.sh/uv/)（Python 包管理）。脚本用 PEP 723 内联声明依赖，
  `uv run` 首次会自动装 `websocket-client`，无需手动建 venv。
- Chrome（路径写死在 `launch.py`，如不同请改 `CHROME`）。

## 用法

```bash
# 1) 起两个独立 CDP 窗口，分别手动登录教师 / 学生账号
uv run agent-browser/launch.py --role teacher   # 端口 9333
uv run agent-browser/launch.py --role student   # 端口 9334

# 2) 确认各窗口登录态与选课（学生须与教师有共同课）
uv run agent-browser/probe.py whoami --port 9333
uv run agent-browser/probe.py whoami --port 9334

# 3) 量 token 形状 + 轮换速度（教师侧，需先 create 拿 rid）
uv run agent-browser/probe.py create --port 9333 --course 54803
uv run agent-browser/probe.py qr --port 9333 --course 54803 --rid <RID> --n 8
uv run agent-browser/probe.py stop --port 9333 --rid <RID>

# 4) 一把梭：全自动量接受窗口（建点名→不同延迟提交→首次成功即停→清理）
uv run agent-browser/probe.py window --tport 9333 --sport 9334 --course 54803
```

## 端口约定

| 角色 | CDP 端口 | profile 目录（gitignore） |
|------|---------|--------------------------|
| teacher | 9333 | `chrome-profile-teacher/` |
| student | 9334 | `chrome-profile-student/` |

## 安全边界

- profile 目录含登录 cookie，已 `.gitignore`，不入库。
- 写操作（create/start/submit/stop）仅用于自有测试课的 `__probe` 点名，跑完即 stop。
- 不对 lnt.xmu.edu.cn 的真实点名做提交（有真实考勤后果，需另行明确授权且有活点名）。
