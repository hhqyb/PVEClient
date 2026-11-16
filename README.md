# PVE 云桌面客户端

搭配 Proxmox VE 的跨平台云桌面瘦客户端。登录后只显示当前账号有权限的虚拟机，
一台时直接连接，多台时列表选择。**协议自动选择：SPICE 优先，noVNC 兜底。**

支持 Windows / macOS / Linux（Tauri v2 + React + noVNC）。

## 它解决什么问题

PVE 自带的 Web UI 面向管理员：功能全但层级深，普通用户每次进桌面都要点四五层。
这个客户端只保留"登录 → 连自己的桌面"这条最短路径：

- 用户认证完全交给 PVE 后端（AD / LDAP / PAM / PVE realm 都支持），客户端**不接触目录服务**
- 权限过滤由 PVE 的 ACL 完成，客户端不做任何权限判断
- 能用 SPICE 就用 SPICE（桌面级画质、音频、USB 重定向），否则自动降级到内嵌网页控制台

## 快速开始

```bash
npm install
npm run tauri dev        # 开发
npm run tauri build      # 打包
```

前置依赖：

| 平台 | 需要 |
|---|---|
| 全部 | Node.js 18+、Rust 1.77+ |
| macOS | Xcode Command Line Tools |
| Windows | MSVC 构建工具、WebView2（Win11 自带） |
| Linux | `libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev` |

## 客户端侧配置

首次启动填 PVE 地址（如 `https://100.98.129.60:8006`），认证域下拉会自动填充。

**「允许自签证书」默认开启** —— PVE 用 IP 访问时几乎不可能有受信证书。
开启后仍会记录服务器证书指纹，指纹变化时给出告警（TOFU）。

## PVE 侧配置（决定用户能看到哪些虚拟机）

客户端看到什么，完全由 PVE 的 ACL 决定。以下按顺序检查：

### 1. 配置认证域

`Datacenter → Permissions → Realms → Add`，类型选 `Active Directory` 或 `LDAP`。

- **AD** 需要填 `Domain`、`Server`、`Bind User`（AD 通常必需）
- **LDAP** 需要填 `Base DN`、`User Attribute Name`（一般是 `uid`）、`Server`
- 配好 Sync Options 后执行同步：

```bash
pveum realm sync <realm>
```

### 2. 确认用户已存在于 PVE 用户表

**这是"认证成功但看不到任何虚拟机"的头号原因。** AD / LDAP 用户必须先在
`/etc/pve/user.cfg` 里有对应条目，才能被授予权限：

```bash
pveum user add youbin@<realm>
# 或靠 realm sync 批量导入
```

### 3. 授予权限

内置角色 `PVEVMUser` 包含 `VM.Audit` + `VM.Console` + `VM.PowerMgmt`，
正好覆盖瘦客户端的全部需求：

```bash
# 推荐：按资源池授权，一个池就是一个人的桌面
pveum acl modify /pool/<池名> --groups <AD组>-<realm> --roles PVEVMUser --propagate 1

# 或者精确到单台
pveum acl modify /vms/104 --groups <AD组>-<realm> --roles PVEVMUser
```

需要的最小权限：

| 权限 | 用途 |
|---|---|
| `VM.Audit` | 在列表中看到这台虚拟机 |
| `VM.Console` | 打开控制台 |
| `VM.PowerMgmt` | 开机 / 关机 / 重启 |

### 4. 让桌面 VM 支持 SPICE

```bash
qm set <vmid> -vga qxl,memory=64
```

`vga=qxl` 是 SPICE 的启用开关（多显示器用 `qxl2` / `qxl3` / `qxl4`）。
Windows 虚拟机还需要在系统内安装 [SPICE guest tools](https://www.spice-space.org/download.html)
（vioserial + qxldod 驱动）才能正常显示与使用剪贴板。

### 5. 放通网络端口

**SPICE 使用的端口与 8006 无关，这是最常见的失败原因：**

| 端口 | 用途 |
|---|---|
| `8006/tcp` | PVE API 与网页控制台（noVNC 只用这一个） |
| `3128/tcp` | spiceproxy 守护进程 |
| `61000-61999/tcp` | 虚拟机的 SPICE 服务端口 |

只放通 8006 时 SPICE 必然失败，客户端会自动降级到 noVNC 并提示原因。

## 协议选择逻辑

```
打开控制台
 ├─ 强制 novnc            → noVNC
 ├─ LXC 容器              → noVNC（容器的 spiceproxy 只是文本终端）
 ├─ vga 不是 qxl          → noVNC（提示需设为 qxl）
 ├─ 未安装 remote-viewer  → noVNC（附安装指引）
 ├─ spiceproxy 失败       → noVNC（附原因）
 ├─ 查看器启动即退出      → noVNC（附退出码与端口提示）
 └─ 成功                  → SPICE（外部窗口）
```

每次降级都会在界面上给出明确原因，不会静默切换。

### 安装 SPICE 客户端

| 平台 | 命令 |
|---|---|
| Linux | `sudo apt install virt-viewer` 或 `dnf install virt-viewer` |
| macOS | `brew install virt-viewer` |
| Windows | 从 [virt-manager.org](https://virt-manager.org/download) 下载 virt-viewer 安装包 |

客户端会自动探测 `remote-viewer`，也可在设置页手动指定路径。

> SPICE 的 TLS 校验由 remote-viewer 自己完成。PVE 用自签证书时，
> 首次连接会弹出一次证书确认框，接受即可 —— 这与客户端的
> 「允许自签证书」设置无关。

## 功能范围

**已实现**

- 登录（含 realm 选择、双因素验证码、记住密码到系统钥匙串）
- 只显示当前账号有权限的虚拟机（含 QEMU 与 LXC，排除模板）
- 一台自动直连 / 多台列表选择 / 零台权限提示
- SPICE 优先 + noVNC 兜底，降级原因可见，可手动切换协议
- 电源操作（开机 / 关机 / 重启）并等待 PVE 任务完成
- 会话续期（ticket 过期前自动用旧 ticket 换新）
- 自签证书支持 + 证书指纹变化告警
- 三平台打包

**不包含**（继续用 PVE 原生 Web UI）

虚拟机创建/删除、快照、存储与网络管理、管理员面板。

## 技术要点

- **凭证不进前端**：ticket 只存在于 Rust 侧，前端拿不到
- **控制台经本地中转**：`vncwebsocket` 除 `vncticket` 外还要求 `PVEAuthCookie`，
  且 webview 无法放宽 TLS 校验，因此由 Rust 在本机起一个只监听 127.0.0.1 的
  WebSocket 中转（随机端口 + 一次性 nonce），前端只连这个本地地址
- **SPICE 密码不落盘**：`.vv` 配置通过 stdin 传给 `remote-viewer -`，不写临时文件
- **SPICE 字段全量透传**：`spiceproxy` 的返回是开放结构，不同版本会多带
  `host-subject`、`ca`（PEM 证书链）等键，只挑白名单字段会导致 TLS 校验失败

## 开发

```bash
npm run build                    # 前端类型检查 + 构建
cd src-tauri && cargo test       # Rust 单元测试
cd src-tauri && cargo clippy     # 静态检查
```

针对真实 PVE 的集成测试（默认跳过）：

```bash
cd src-tauri
PVE_TEST_HOST=https://your.pve:8006 \
PVE_TEST_USER=youbin PVE_TEST_REALM=pam PVE_TEST_PASS=*** \
PVE_TEST_EXPECT_SINGLE=1 \
cargo test --test live_pve -- --ignored --nocapture --test-threads=1
```

这组测试验证的是文档没写清、只能靠真机确认的行为，例如：

- `/access/domains` 匿名可读，而 `/version` 需要认证
- `vncproxy` 的 `port` 返回的是**字符串**而非数字
- `vncwebsocket` **不接受** `websocket=1` 参数（加上会 400）
- `websocket=1` 时 PVE 把 VNC 密码拼在 ticket 前面

## macOS 未签名提示

如果打包产物在 macOS 上提示"已损坏"，执行：

```bash
xattr -dr com.apple.quarantine /Applications/PVE\ Desktop.app
```

或右键 → 打开。
