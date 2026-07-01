# xmu_secure_link_docker

**厦大 SecureLink(OpenVPN3)之上的最小 SOCKS5 中间层。** 容器内跑
[`xmu_secure_link`](https://github.com/XMU-MoYu-Club/xmu_secure_link) 建立 VPN 隧道,
在宿主机 `127.0.0.1:1080` 暴露一个 SOCKS5。走这个 SOCKS5 的流量按 `xmu_secure_link`
自己下发的**分流路由**走——校园/ACL 目标(如 `*.xmu.edu.cn`)经校园 VPN 出口出去(隐匿真实 IP),
其余目标直连。**不强制把所有连接都塞进 VPN**,也不修改宿主机路由表。

```text
App -> 127.0.0.1:1080 SOCKS5 -> 容器内 microsocks -> 容器内路由表
  ├─ 校园/ACL 目标 -> VPN 网卡 (tun) -> xmu_secure_link (OpenVPN3) -> OpenVPN Server
  └─ 其余目标       -> 直连 (eth0)
```

- Docker Hub: `vintcessun/xmu_secure_link_docker`
- 标签:`latest`、`v0.1.0`
- 平台:`linux/amd64`(发行二进制为 x86_64;需要宿主内核提供 `/dev/net/tun`,如 Docker Desktop WSL2 后端)

## 前置:SecureLink 会话数据

免登录复用会话,需要一个含 `session.json` + `device_id` 的数据目录,挂到容器
`/root/.local/share/mysecurelinkrs`。首次或会话过期时用**一次性交互登录**刷新:

```bash
docker run --rm -it \
  -v /path/to/securelink-data:/root/.local/share/mysecurelinkrs \
  vintcessun/xmu_secure_link_docker:latest login
```

会打印 SSO 登录 URL:浏览器打开→完成登录→把最终 callback URL 粘回终端;看到开始连接
(或 `VPN connected`)后按 `Ctrl-C`,刷新后的会话已写回挂载卷。

## 运行

```bash
docker run -d --name xmu-securelink-socks \
  --cap-add NET_ADMIN --device /dev/net/tun \
  -p 127.0.0.1:1080:1080 \
  -v /path/to/securelink-data:/root/.local/share/mysecurelinkrs \
  -e RUST_LOG=info -e SOCKS_PORT=1080 \
  --restart unless-stopped \
  vintcessun/xmu_secure_link_docker:latest
```

- `--cap-add NET_ADMIN` + `--device /dev/net/tun`:让容器内建 VPN 网卡、改**容器自己**的路由表。
- `-p 127.0.0.1:1080:1080`:**只**在宿主 loopback 暴露 SOCKS5;VPN 网卡只存在于容器内。

也可用仓库里的 `docker/docker-compose.yml`(用 `SECURELINK_DATA_DIR` 传数据目录路径)。

## 验收:证明流量确实进了隧道

```bash
# 校园内网探针(需经隧道才可达)——期望看到对应 banner
curl -v --socks5-hostname 127.0.0.1:1080 telnet://121.192.180.236:21 --max-time 15   # FTP: 220 ...
curl -v --socks5-hostname 127.0.0.1:1080 telnet://59.77.5.59:2222   --max-time 15    # SSH: SSH-2.0-...

# 容器内确认目标走 tun 而非 eth0
docker exec -it xmu-securelink-socks ip route get 121.192.180.236
```

## 与调用方配合

调用端(如机器人)只需把 `*.xmu.edu.cn` 的请求走 `socks5h://127.0.0.1:1080`、其余直连即可——
这些校园 IP 正是 `xmu_secure_link` 会路由进隧道的目标,因此从校园出口出去、隐匿真实 IP。

## 许可证 / 来源

- 本镜像与打包脚本以 **GPL-3.0-only** 分发(见仓库 `LICENSE`)。
- 镜像内的 `xmu_secure_link` 二进制来自其官方 Release(GPL)。
- 源:<https://github.com/XMU-MoYu-Club/xmu_secure_link>
