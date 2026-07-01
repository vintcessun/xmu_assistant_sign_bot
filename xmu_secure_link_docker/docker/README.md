# xmu_secure_link SOCKS5 中间层 (Docker)

一个最小中间层：**Docker 暴露 `127.0.0.1:1080` SOCKS5，所有走这个 SOCKS5 的 TCP
流量进入容器，经容器内 `xmu_secure_link` 建立的 VPN 网卡出去。**

```text
App
  -> 127.0.0.1:1080 SOCKS5
  -> 容器内 microsocks
  -> 容器内 Linux 路由表
  -> 容器内 VPN 网卡 (tun)
  -> xmu_secure_link (OpenVPN3 Client)
  -> OpenVPN Server
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
| 无条件把默认路由切到 VPN | **只有成功钉住服务器 IP 后才切** | 否则可能切断隧道自身的上行（自咬尾巴） |

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

- **两条 curl 都超时**：SOCKS5 起来了但连接没进 VPN 网卡。
  `docker logs` 看 `[entrypoint]` 输出的 “final route table / probe target routes”，
  确认探针目标 `dev` 是 `tun*`。若是 `eth0`，说明客户端没为内网目标下发路由，
  且默认路由未切到 VPN——检查是否打印了 `WARN: could not identify the OpenVPN
  server IP`（服务器 IP 未识别 → 出于安全没切默认路由）。
- **容器反复重启 / `xmu_secure_link exited early`**：多半是会话过期需要交互登录，
  见上文“前置条件”，重新登录后再挂载。
- **`ip route get` 报错或无 tun**：确认 Docker Desktop WSL2 后端、compose 里的
  `cap_add: NET_ADMIN` 与 `devices: /dev/net/tun` 生效。

---

## 关键保证

1. 宿主机只暴露 `127.0.0.1:1080`。
2. 宿主机路由表不被修改。
3. VPN 网卡只存在于容器内部。
4. `microsocks` 与 `xmu_secure_link` 在同一网络命名空间。
5. `microsocks` 不理解 OpenVPN，只做 SOCKS5。
6. SOCKS5 的 TCP connect 由容器路由表送进 VPN 网卡。
7. OpenVPN Server 的连接被钉在原始 `eth0`，默认路由切换不会自咬尾巴。
