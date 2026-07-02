# xmu_secure_link SOCKS5 中间层 (Docker)

一个最小中间层：**Docker 暴露 `127.0.0.1:1080` SOCKS5；走这个 SOCKS5 的流量进入容器后，
按 `xmu_secure_link` 自己下发的分流路由走——校园/ACL 目标经 VPN 网卡出去（隐匿真实 IP），
其余目标直连。不强制把所有连接都塞进 VPN。**

```text
App -> 127.0.0.1:1080 SOCKS5 -> 容器内 microsocks -> 容器内 Linux 路由表
  ├─ 校园/ACL 目标 -> VPN 网卡 (tun) -> xmu_secure_link (OpenVPN3) -> OpenVPN Server
  └─ 其余目标       -> 直连 (eth0)
```

不实现 Clash / PAC / HTTP 代理 / 透明代理 / tun2socks / UDP 转发 / 规则系统。

---

## 与原始计划的差异（研究后修正）

| 计划 | 实际实现 | 原因 |
|------|----------|------|
| 两阶段构建（vcpkg 编译 openvpn3） | **单阶段，直接下载 Release** | 已有 Linux 预编译产物，无需编译 |
| `debian:bookworm-slim` | **`ubuntu:24.04`** | 二进制需要 `GLIBC_2.39`，bookworm 只有 2.36，启动即失败 |
| 只 `COPY *.so` | 下载自包含 tar（binary + 全部 `.so`）并设 `LD_LIBRARY_PATH` | 二进制 RUNPATH=`$ORIGIN` 不传递，`libovpnffi` 的依赖找不到 |
| 从日志 `Remotes:` 解析服务器 IP | **用 `ss` 探测已建立连接的对端** + 路由/日志兜底 | 日志格式未公开，`ss` 更稳且与格式无关 |
| 无条件把默认路由切到 VPN | **完全不切默认路由，走客户端自己的分流路由** | 客户端已为校园/ACL 目标下发 tun 路由；bot 只把 `*.xmu.edu.cn` 发给 SOCKS5，无需强制默认路由，也彻底避免自咬尾巴 |

镜像里额外安装 `libstdc++6`（`libovpnffi.so` 需要），其余原生依赖都在 tar 内。

---

## 前置条件

- Windows + Docker Desktop（WSL2 后端）。WSL2 内核自带 `/dev/net/tun`。
- 已存在数据目录（内含 `session.json`、`device_id`），用于免登录复用会话。
  该目录路径**不写死在 compose 里**，运行前用环境变量 `SECURELINK_DATA_DIR` 传入，
  挂载到容器 `/root/.local/share/mysecurelinkrs`：

  ```powershell
  $env:SECURELINK_DATA_DIR="C:/Users/vintces/Desktop/rust/xmu_assistant_sign_bot/data/securelink"
  ```

  必须设置该变量；WSL 里直接跑 Docker 用 `/mnt/c/...` 形式。

> 会话里的 `refresh_token` 会过期。过期后 `up` 时容器会打印 SSO 登录页并直接退出
> （无 TTY 无法粘贴 callback）。此时用下面的**一次性交互登录**刷新会话即可。

### 一次性交互登录（会话过期时）

```powershell
docker compose -f docker/docker-compose.yml run --rm -it xmu-securelink-socks login
```

会打印一个 SSO URL：浏览器打开、完成登录、把最终 callback URL 粘回终端。看到开始
连接（或 `VPN connected`）后按 `Ctrl-C`——刷新后的会话已写回挂载卷
`data\securelink`。之后正常 `up -d` 即可免登录自动连接。

---

## 启动（在 `xmu_secure_link_docker` 目录下执行）

> **代理注意**：本机通过本地 Clash 代理（`127.0.0.1:7890`）访问外网时，Docker 的
> **构建**网络无法自行解析 DNS，`apt` 和下载 Release 会失败。构建前设置：
>
> ```powershell
> $env:DOCKER_BUILD_PROXY="http://host.docker.internal:7890"
> ```
>
> compose 会把它作为构建期 `http_proxy/https_proxy` 传入（只影响构建，不写进镜像，
> 也不影响运行期）。直连外网的机器留空即可。运行期容器直连 XMU（已验证可达），
> 不需要代理。

前台运行（看日志）：

```powershell
docker compose -f docker/docker-compose.yml up --build
```

后台运行：

```powershell
docker compose -f docker/docker-compose.yml up -d --build
```

查看日志 / 进容器 / 看网卡与路由：

```powershell
docker logs -f xmu-securelink-socks
docker exec -it xmu-securelink-socks bash
docker exec -it xmu-securelink-socks ip addr
docker exec -it xmu-securelink-socks ip route
```

停止：

```powershell
docker compose -f docker/docker-compose.yml down
```

---

## 验收：两个隧道内网探针必须通过 SOCKS5 成功

验收目标不是访问公网，而是证明 SOCKS5 流量确实进了隧道。宿主机 PowerShell：

```powershell
# FTP 探针 121.192.180.236:21 —— 期望看到 "220 ..." FTP banner
curl.exe -v --socks5-hostname 127.0.0.1:1080 telnet://121.192.180.236:21 --max-time 15

# SSH 探针 59.77.5.59:2222 —— 期望看到 "SSH-2.0-..." banner
curl.exe -v --socks5-hostname 127.0.0.1:1080 telnet://59.77.5.59:2222 --max-time 15
```

容器内确认这两个目标确实走 VPN 网卡（而不是 `eth0`）：

```powershell
docker exec -it xmu-securelink-socks ip route get 121.192.180.236
docker exec -it xmu-securelink-socks ip route get 59.77.5.59
```

两条结果的 `dev` 都应是 `tun*`（或 `ovpn*`），且 OpenVPN Server 本身的 `/32`
路由仍走原始 `eth0`。

只有当上面两条 `curl` 都能连上并看到 banner 时，才算跑通。

---

## 排查

- **SOCKS5 能握手但 CONNECT 全部超时（本机其他都正常）**：宿主 `127.0.0.1:1080`
  被别的进程占了——**VS Code 的端口转发（Ports 面板）**、Clash 等代理都爱用 1080。
  外来监听者会把 SOCKS 流量整个劫持走（method 协商照样成功，极具迷惑性）。查：

  ```powershell
  Get-NetTCPConnection -LocalPort 1080 -State Listen | ForEach-Object {
    (Get-Process -Id $_.OwningProcess).ProcessName }
  ```

  若被占：删掉 VS Code Ports 面板里的 1080 转发，或换端口
  `$env:SOCKS_HOST_PORT="11080"` 后重新 `up`（compose 端口映射跟随该变量）。
- **VPN 频繁断线重连（日志 `NETWORK_EOF_ERROR` / `RECONNECTING`）**：服务器主动
  掐会话，典型原因是**同一账号/同一份 session 在别处也连着 VPN**（手机 App、
  另一台电脑、部署在服务器上的 bot），单会话策略下双方每 5 秒互踢。entrypoint
  会在每次重连后 1 秒内自动补路由（自愈），但根治需要停掉另一个客户端或重新
  login 生成新会话。
- **两条 curl 都超时**：SOCKS5 起来了但连接没进 VPN 网卡。
  `docker logs` 看 `[entrypoint]` 输出的 “final route table / probe target routes”，
  确认探针目标 `dev` 是 `tun*`。若是 `eth0`，说明客户端还没为这些目标下发 tun 路由
  （多半是隧道刚起来还没推完路由，或会话过期没连上）——看日志里客户端是否成功建连 /
  出现 “VPN connected”。
- **容器反复重启 / `xmu_secure_link exited early`**：多半是会话过期需要交互登录，
  见上文“前置条件”，重新登录后再挂载。
- **`ip route get` 报错或无 tun**：确认 Docker Desktop WSL2 后端、compose 里的
  `cap_add: NET_ADMIN` 与 `devices: /dev/net/tun` 生效。

---

## 会话文件监听：隔离 + 双向同步

容器不让客户端直接读写挂载卷，而是用**隔离工作副本**区分「谁改了会话文件」，从而
在「外部改动要重载」和「客户端自己刷 token 不该重启」之间干净地分开：

```text
挂载卷 SRC  /root/.local/share/mysecurelinkrs      <- 与宿主共享（真源）
工作副本 WORK /run/securelink-home/.local/share/... <- 客户端实际读写（HOME/XDG 重定向）
```

启动时把 SRC 拷进 WORK 作为种子；之后 entrypoint 每 `WATCH_INTERVAL` 秒（默认 5s）
对两侧 `session.json`+`device_id` 做 sha256 指纹比对：

| 变化侧 | 含义 | 动作 | 是否重启 |
|--------|------|------|----------|
| **WORK 变了** | 客户端内部刷新了 token（写到隔离副本） | WORK → SRC，**向外**反映到宿主 | 否 |
| **SRC 变了** | 外部（宿主 / 另一次登录）改了文件 | SRC → WORK，**向内**反映；打印日志后重载客户端 | 是（重启 `xmu_secure_link`） |

- 外部改动带 2s（`WATCH_DEBOUNCE`）去抖，避免宿主编辑器写一半就触发。
- 同步是脚本自己写的那侧会更新指纹基线，不会被误判为对侧改动（防止来回抖动）。
- 内部刷新只镜像文件、**不重启**，所以客户端例行刷 token 不会造成重启风暴。
- `microsocks` 独立于客户端进程，重载期间保持监听；重启后自动重新钉 XMU /32 路由。
- 客户端异常退出也会被循环发现并自动拉起。
- **路由自愈**：客户端遇到服务器断线会进程内重连（tun 销毁重建，内核清掉 /32
  路由）；watch 循环每个 tick 检查并重新钉路由，`WATCH_INTERVAL=1` 时自愈延迟
  不超过 ~1 秒。
- **优雅停机**：收到 TERM 后先让客户端正常断开 VPN 再退出。若被 SIGKILL 硬杀，
  服务器侧会残留半开会话，之后的连接会被反复踢线——`down` 请给足超时。

可调环境变量（compose `environment` 里覆盖）：`WATCH_INTERVAL`（默认 1s，同时
决定路由自愈速度）、`WATCH_DEBOUNCE`。宿主端口用 `SOCKS_HOST_PORT` 覆盖（默认
1080，被占用时换）。

> `login` 一次性交互模式**不隔离**（`HOME=/root`），直接写挂载卷，保证刷新后的会话
> 落回宿主 `data\securelink`。

---

## 关键保证

1. 宿主机只暴露 `127.0.0.1:1080`。
2. 宿主机路由表不被修改。
3. VPN 网卡只存在于容器内部。
4. `microsocks` 与 `xmu_secure_link` 在同一网络命名空间。
5. `microsocks` 不理解 OpenVPN，只做 SOCKS5。
6. SOCKS5 的 TCP connect 由容器路由表**分流**：校园/ACL 目标进 VPN 网卡，其余直连。
7. 不修改容器默认路由，只用客户端自己下发的分流路由，不存在自咬尾巴问题。
8. 客户端跑在隔离工作副本上：内部刷 token 向外镜像不重启；外部改文件向内同步并重载。
