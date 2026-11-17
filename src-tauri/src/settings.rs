use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::pve::models::Protocol;

/// 用户可配置项，落盘为 JSON（**不含任何密码**）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 完整服务器地址，如 https://100.98.129.60:8006
    pub server: String,
    pub username: String,
    pub realm: String,
    /// 记住密码：密码本身写入系统钥匙串，这里只存开关
    pub remember_password: bool,
    /// 内网自建 PVE 多为自签证书，默认放宽；可在设置页关闭
    pub allow_invalid_certs: bool,
    /// auto | spice | novnc
    pub protocol: String,
    /// PVE 返回的 proxy 主机名不可达时的替代地址，如 100.98.129.60:3128
    pub spice_proxy_override: Option<String>,
    /// 手动指定的 remote-viewer 路径
    pub viewer_path: Option<PathBuf>,
    /// SPICE 全屏托管（kiosk）
    pub spice_kiosk: bool,
    /// SPICE 以全屏（非 kiosk）打开；与 spice_kiosk 都为 false 时以普通窗口打开
    pub spice_fullscreen: bool,
    /// SPICE 启用 USB 重定向（透传本地 USB 设备到虚拟机）
    pub spice_usb_redirect: bool,
    /// 上次见过的服务器证书指纹，用于变化告警
    pub last_fingerprint: Option<String>,
    /// 已知服务器版本，便于离线展示
    pub server_version: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server: String::new(),
            username: String::new(),
            realm: String::new(),
            remember_password: false,
            allow_invalid_certs: true,
            protocol: "auto".to_string(),
            spice_proxy_override: None,
            viewer_path: None,
            spice_kiosk: true,
            spice_fullscreen: false,
            spice_usb_redirect: true,
            last_fingerprint: None,
            server_version: None,
        }
    }
}

impl Settings {
    pub fn protocol_pref(&self) -> Option<Protocol> {
        match self.protocol.as_str() {
            "spice" => Some(Protocol::Spice),
            "novnc" => Some(Protocol::Vnc),
            _ => None,
        }
    }

    pub fn viewer(&self) -> Option<PathBuf> {
        self.viewer_path.clone().filter(|p| p.is_file())
    }
}

pub struct SettingsStore {
    path: PathBuf,
    inner: Mutex<Settings>,
}

impl SettingsStore {
    pub fn load(dir: PathBuf) -> Self {
        let path = dir.join("settings.json");
        let inner = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .unwrap_or_default();
        Self {
            path,
            inner: Mutex::new(inner),
        }
    }

    pub fn get(&self) -> Settings {
        self.inner
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    pub fn save(&self, next: Settings) -> AppResult<()> {
        let json = serde_json::to_string_pretty(&next)
            .map_err(|e| AppError::Io(format!("序列化设置失败：{e}")))?;

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::Io(format!("创建配置目录失败：{e}")))?;
        }

        // 原子写：先写临时文件再重命名，避免中断留下半截 JSON
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json)
            .map_err(|e| AppError::Io(format!("写入设置失败：{e}")))?;
        std::fs::rename(&tmp, &self.path)
            .map_err(|e| AppError::Io(format!("保存设置失败：{e}")))?;

        *self
            .inner
            .lock()
            .map_err(|_| AppError::Io("设置锁已损坏".into()))? = next;
        Ok(())
    }
}
