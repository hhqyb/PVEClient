use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::process::Child;
use tokio::sync::Mutex;

use crate::bridge::WsBridge;
use crate::error::{AppError, AppResult};
use crate::pve::models::{ConsoleHandle, Guest, Protocol, ViewerState};
use crate::pve::PveClient;

pub mod spice;
pub mod vnc;

/// SPICE 会话的启动选项，由设置页决定。
#[derive(Debug, Clone, Copy)]
pub struct SpiceOptions {
    /// kiosk 全屏托管（退出即结束会话）
    pub kiosk: bool,
    /// 普通全屏（可用热键切回窗口）
    pub fullscreen: bool,
    /// 是否允许把本地 USB 设备透传给虚拟机
    pub usb_redirect: bool,
}

impl Default for SpiceOptions {
    fn default() -> Self {
        Self {
            kiosk: false,
            fullscreen: false,
            usb_redirect: true,
        }
    }
}

impl SpiceOptions {
    /// 把开关写进 .vv：remote-viewer 只在配置里看到这些键时才改变行为。
    fn apply_to(&self, vv: &str) -> String {
        let mut extra = String::new();
        extra.push_str(&format!(
            "fullscreen={}\n",
            if self.fullscreen || self.kiosk { 1 } else { 0 }
        ));
        extra.push_str(&format!(
            "enable-usbredir={}\n",
            if self.usb_redirect { 1 } else { 0 }
        ));
        format!("{vv}{extra}")
    }
}

///
/// 只保留回收资源所必需的句柄：SPICE 需要子进程，VNC 需要注销中转路由。
enum ActiveConsole {
    Spice {
        child: Child,
        /// 状态栏展示所需的会话信息（进程存活、USB/全屏开关都在这里）
        /// 用 Box 避免与 Vnc 变体体积差异过大
        status: Box<SpiceStatus>,
    },
    Vnc { nonce: String },
}

/// SPICE 会话的状态栏信息。
///
/// SPICE 画面由外部 remote-viewer 进程绘制，我们无法往它的窗口里注入 UI，
/// 因此状态栏由客户端自己的悬浮窗口呈现，数据从这里取。
#[derive(Debug, Clone, serde::Serialize)]
pub struct SpiceStatus {
    pub console_id: String,
    pub vmid: u32,
    pub guest_name: String,
    pub node: String,
    /// 实际使用的协议标签，如 SPICE
    pub protocol: String,
    /// 由 .vv 下发的开关，用于如实反映当前会话能力
    pub usb_redirect: bool,
    pub fullscreen: bool,
}

#[derive(Default)]
struct Registry {
    consoles: HashMap<String, ActiveConsole>,
}

/// 控制台管理器：负责协议选路、降级、以及资源回收。
pub struct ConsoleManager {
    bridge: Arc<WsBridge>,
    registry: Mutex<Registry>,
}

impl ConsoleManager {
    pub fn new(bridge: Arc<WsBridge>) -> Self {
        Self {
            bridge,
            registry: Mutex::new(Registry::default()),
        }
    }

    /// 打开控制台。
    ///
    /// 选路规则（对应方案 §5.2）：
    ///   1. 强制 novnc → 直接走 VNC
    ///   2. 非 qemu 或 vga 不是 qxl → VNC（LXC 的 spiceproxy 只是文本终端，无意义）
    ///   3. 未找到 remote-viewer → 降级 VNC（附原因）
    ///   4. spiceproxy 失败 → 降级 VNC（附原因）
    ///   5. 查看器启动后立刻异常退出 → 降级 VNC（附原因）
    ///
    /// 每次降级都带 `reason`，前端必须展示，避免用户误以为客户端在偷懒。
    pub async fn open(
        &self,
        client: &PveClient,
        guest: &Guest,
        forced: Option<Protocol>,
        proxy_override: Option<&str>,
        options: SpiceOptions,
        viewer_override: Option<&PathBuf>,
    ) -> AppResult<ConsoleHandle> {
        if !guest.is_running() {
            return Err(AppError::GuestNotRunning(format!(
                "{} (VM {}) 当前状态为 {}，请先开机",
                guest.name, guest.vmid, guest.status
            )));
        }

        if forced == Some(Protocol::Vnc) {
            return self.open_vnc(client, guest, "按设置强制使用网页控制台").await;
        }

        let forced_spice = forced == Some(Protocol::Spice);

        // 判据：qemu + vga=qxl*（PVE 文档：选 qxl 即启用 SPICE）
        if guest.kind != "qemu" {
            return self
                .open_vnc(client, guest, "容器只能使用网页控制台")
                .await;
        }
        if !guest.spice_capable && !forced_spice {
            let detail = guest
                .vga
                .as_deref()
                .map(|v| format!("当前 vga={v}"))
                .unwrap_or_else(|| "未配置 vga".to_string());
            return self
                .open_vnc(
                    client,
                    guest,
                    &format!("该虚拟机未启用 SPICE（{detail}，需设为 qxl）"),
                )
                .await;
        }

        let viewer = match viewer_override.cloned().or_else(spice::detect_viewer) {
            Some(v) => v,
            None => {
                let reason = "未检测到 remote-viewer，已降级为网页控制台。\
                    安装方式：Linux `apt install virt-viewer`，macOS `brew install virt-viewer`，\
                    Windows 从 virt-manager.org 下载 virt-viewer";
                return self.open_vnc(client, guest, reason).await;
            }
        };

        let mut spice_cfg = match crate::pve::console::request_spice(
            client,
            &guest.node,
            guest.vmid,
            proxy_override,
        )
        .await
        {
            Ok(cfg) => cfg,
            Err(e) => {
                let reason = format!("获取 SPICE 配置失败，已降级为网页控制台：{e}");
                return self.open_vnc(client, guest, &reason).await;
            }
        };

        // PVE 集群常把 proxy 通告成内部域名（pve.dddbg.com），而用户是用 IP 或
        // 别的名字连进来的，本地解析不到该域名，remote-viewer 必然连不上 3128。
        // 此时改用「用户刚刚实际连通的地址」重试一次。
        if proxy_override.is_none() {
            if let Some(proxy) = spice_cfg.get_str("proxy") {
                let needs_retry = match crate::pve::console::spice_proxy_host(&proxy) {
                    Some(h) => !crate::pve::console::resolves(&h, 3128),
                    None => false,
                };
                if needs_retry {
                    if let Some(fallback) = client.host_for_proxy() {
                        if let Ok(alt) = crate::pve::console::request_spice(
                            client,
                            &guest.node,
                            guest.vmid,
                            Some(&fallback),
                        )
                        .await
                        {
                            spice_cfg = alt;
                        }
                    }
                }
            }
        }

        let vv = options.apply_to(&spice_cfg.to_vv());
        let mut child = match spice::launch(&viewer, &vv, options.kiosk, options.fullscreen).await
        {
            Ok(c) => c,
            Err(e) => {
                let reason = format!("启动外部查看器失败，已降级为网页控制台：{e}");
                return self.open_vnc(client, guest, &reason).await;
            }
        };

        // 查看器若在启动初期就退出（配置错、端口不通），及时降级而不是留个死窗口
        if let Some(code) = spice::detect_early_exit(&mut child, std::time::Duration::from_secs(5)).await
        {
            let reason = format!(
                "外部查看器启动后立即退出（退出码 {code}）。\
                 常见原因：3128 或 61000-61999 端口未放通。已降级为网页控制台"
            );
            return self.open_vnc(client, guest, &reason).await;
        }

        let id = uuid::Uuid::new_v4().to_string();
        let status = SpiceStatus {
            console_id: id.clone(),
            vmid: guest.vmid,
            guest_name: guest.name.clone(),
            node: guest.node.clone(),
            protocol: "SPICE".to_string(),
            usb_redirect: options.usb_redirect,
            fullscreen: options.kiosk || options.fullscreen,
        };
        self.registry
            .lock()
            .await
            .consoles
            .insert(id.clone(), ActiveConsole::Spice { child, status: Box::new(status) });

        Ok(ConsoleHandle::Spice {
            id,
            vmid: guest.vmid,
            viewer: viewer.display().to_string(),
            reason: "已使用 SPICE 外部查看器打开（性能与音频优于网页控制台）".to_string(),
        })
    }

    /// 明确的 noVNC 分支：申请 vncproxy，注册本地中转，返回前端可用地址。
    pub async fn open_vnc(
        &self,
        client: &PveClient,
        guest: &Guest,
        reason: &str,
    ) -> AppResult<ConsoleHandle> {
        let proxy = crate::pve::console::request_vnc(
            client,
            &guest.node,
            &guest.kind,
            guest.vmid,
        )
        .await?;

        let ticket = client.export_ticket().await?;

        let upstream = format!(
            "{}/nodes/{}/{}/{}/vncwebsocket?port={}&vncticket={}",
            client.ws_base_url(),
            guest.node,
            guest.kind,
            guest.vmid,
            proxy.port,
            urlencoding::encode(&proxy.ticket)
        );
        // 注意：不要在 vncwebsocket 上加 websocket=1 —— 该参数只属于 vncproxy，
        // 加到 WS 端点会被 PVE 判为参数校验失败（400）。

        let nonce = random_nonce();
        let ws_url = self
            .bridge
            .register(
                nonce.clone(),
                crate::bridge::BridgeEntry {
                    upstream_url: upstream,
                    cookie: format!("PVEAuthCookie={ticket}"),
                    allow_invalid_certs: client.allow_invalid_certs(),
                },
            )
            .await;

        let id = uuid::Uuid::new_v4().to_string();
        self.registry
            .lock()
            .await
            .consoles
            .insert(id.clone(), ActiveConsole::Vnc { nonce });

        Ok(ConsoleHandle::Vnc {
            id,
            vmid: guest.vmid,
            ws_url,
            password: proxy.rfb_password(),
            reason: reason.to_string(),
        })
    }

    /// 关闭控制台：SPICE 结束外部进程，VNC 注销本地非 binding。
    pub async fn close(&self, id: &str) -> AppResult<()> {
        let entry = self.registry.lock().await.consoles.remove(id);

        match entry {
            Some(ActiveConsole::Spice { mut child, .. }) => {
                // 先礼后兵：先请它自己退出，避免残留窗口
                let _ = child.start_kill();
                let _ = tokio::time::timeout(std::time::Duration::from_secs(2), child.wait()).await;
                Ok(())
            }
            Some(ActiveConsole::Vnc { nonce, .. }) => {
                self.bridge.unregister(&nonce).await;
                Ok(())
            }
            None => Ok(()),
        }
    }

    pub async fn viewer_state(&self, id: &str) -> Option<ViewerState> {
        let mut guard = self.registry.lock().await;
        match guard.consoles.get_mut(id) {
            Some(ActiveConsole::Spice { child, .. }) => match child.try_wait() {
                Ok(Some(status)) => Some(ViewerState::Exited {
                    code: status.code(),
                }),
                Ok(None) => Some(ViewerState::Running),
                Err(_) => Some(ViewerState::Exited { code: None }),
            },
            _ => None,
        }
    }

    /// 当前 SPICE 会话的状态栏数据（供悬浮状态栏展示）。
    ///
    /// 同时返回查看器是否还活着：若用户直接关掉了 remote-viewer 窗口，
    /// 状态栏要能反映出来，而不是一直显示「已连接」。
    pub async fn spice_status(&self) -> Option<SpiceStatus> {
        let mut guard = self.registry.lock().await;
        for entry in guard.consoles.values_mut() {
            if let ActiveConsole::Spice { child, status } = entry {
                // try_wait 顺带回收已退出的子进程，避免僵尸
                let running = matches!(child.try_wait(), Ok(None));
                if !running {
                    return None;
                }
                return Some((**status).clone());
            }
        }
        None
    }

    /// 应用退出前清理所有控制台，避免 PVE 侧 dtach/vncterm 与本地子进程堆积。
    pub async fn close_all(&self) {
        let ids: Vec<String> = self
            .registry
            .lock()
            .await
            .consoles
            .keys()
            .cloned()
            .collect();
        for id in ids {
            let _ = self.close(&id).await;
        }
    }
}

fn random_nonce() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}
