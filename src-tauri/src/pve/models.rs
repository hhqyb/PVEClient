use serde::{Deserialize, Serialize};

/// 认证域（PVE 的 realm：pam / pve / ldap / ad / openid）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Realm {
    pub realm: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub default: Option<u8>,
    #[serde(default)]
    pub tfa: Option<String>,
}

/// `POST /access/ticket` 的返回
#[derive(Debug, Clone, Deserialize)]
pub struct TicketResponse {
    pub ticket: String,
    #[serde(rename = "CSRFPreventionToken", default)]
    pub csrf: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
}

/// 已建立的会话（只在 Rust 侧持有 ticket）
#[derive(Debug, Clone)]
pub struct Session {
    pub ticket: String,
    pub csrf: Option<String>,
    pub username: String,
    pub realm: String,
    pub host: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub username: String,
    pub realm: String,
    pub host: String,
    pub version: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerProbe {
    pub version: String,
    pub fingerprint: Option<String>,
    /// 证书指纹与上次记录不一致时的告警文案（不阻断登录，由用户判断）
    pub fingerprint_warning: Option<String>,
    pub realms: Vec<Realm>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionInfo {
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub release: Option<String>,
    /// PVE 会返回 repoid，此处无需使用但保留以完整解析
    #[serde(default)]
    #[allow(dead_code)]
    pub repoid: Option<String>,
}

/// `/cluster/resources?type=vm` 的单项
#[derive(Debug, Clone, Deserialize)]
pub struct ClusterResource {
    /// 形如 "qemu/100"，仅用于反序列化完整性
    #[serde(default)]
    #[allow(dead_code)]
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub vmid: u32,
    #[serde(default)]
    pub name: Option<String>,
    pub node: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub template: Option<u8>,
    #[serde(default)]
    pub pool: Option<String>,
    #[serde(default)]
    pub tags: Option<String>,
    #[serde(default)]
    pub cpu: Option<f64>,
    #[serde(default)]
    pub maxcpu: Option<f64>,
    #[serde(default)]
    pub mem: Option<u64>,
    #[serde(default)]
    pub maxmem: Option<u64>,
    #[serde(default)]
    pub uptime: Option<u64>,
    #[serde(default)]
    pub lock: Option<String>,
    #[serde(default)]
    pub hastate: Option<String>,
}

/// 对外暴露给前端的虚拟机模型
#[derive(Debug, Clone, Serialize)]
pub struct Guest {
    pub vmid: u32,
    pub node: String,
    /// "qemu" | "lxc"
    pub kind: String,
    pub name: String,
    /// "running" | "stopped" | "unknown"
    pub status: String,
    pub pool: Option<String>,
    pub tags: Vec<String>,
    pub cpu: Option<f64>,
    pub maxcpu: Option<f64>,
    pub mem: Option<u64>,
    pub maxmem: Option<u64>,
    pub uptime: Option<u64>,
    pub lock: Option<String>,
    pub hastate: Option<String>,
    /// 该 VM 是否可以用 SPICE 打开（qemu + vga=qxl*）
    pub spice_capable: bool,
    /// 原始 vga 配置，便于前端排查与展示
    pub vga: Option<String>,
}

impl Guest {
    pub fn is_running(&self) -> bool {
        self.status == "running"
    }
}

/// `GET .../{qemu|lxc}/{vmid}/config` 中我们关心的字段
#[derive(Debug, Clone, Default, Deserialize)]
pub struct VmConfig {
    #[serde(default)]
    pub vga: Option<String>,
    /// 名称与 ostype 便于后续扩展（如自动适配分辨率），当前仅用于解析
    #[serde(default)]
    #[allow(dead_code)]
    pub name: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub ostype: Option<String>,
}

impl VmConfig {
    /// PVE 文档：vga=qxl 即同时为该 VM 启用 SPICE；多显示器变体为 qxl2/qxl3/qxl4。
    pub fn supports_spice(&self) -> bool {
        self.vga
            .as_deref()
            .map(|v| {
                let head = v.split(',').next().unwrap_or("").trim();
                head.starts_with("qxl")
            })
            .unwrap_or(false)
    }
}

/// `POST .../vncproxy`
///
/// 注意：实测 PVE 把 `port` 返回成字符串（`"5900"`）而不是数字，
/// 直接声明为 u16 会导致反序列化失败。这里用自定义反序列化兼容两种形式。
#[derive(Debug, Clone, Deserialize)]
pub struct VncProxyResponse {
    pub ticket: String,
    #[serde(deserialize_with = "de_flexible_u16")]
    pub port: u16,
    #[serde(default)]
    pub password: Option<String>,
    /// PVE 同时返回登录用户与证书链，当前无需使用
    #[serde(default)]
    #[allow(dead_code)]
    pub user: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub cert: Option<String>,
}

/// PVE 的部分端点把数字序列化成字符串，这里两种都接受。
fn de_flexible_u16<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        Num(u64),
        Str(String),
    }

    match NumOrStr::deserialize(deserializer)? {
        NumOrStr::Num(n) => u16::try_from(n).map_err(D::Error::custom),
        NumOrStr::Str(s) => s.trim().parse::<u16>().map_err(D::Error::custom),
    }
}

impl VncProxyResponse {
    /// RFB 握手用的密码：新版本返回 password，旧版本需用 ticket。
    pub fn rfb_password(&self) -> String {
        self.password
            .clone()
            .unwrap_or_else(|| self.ticket.clone())
    }
}

/// `POST .../spiceproxy` —— 字段要全量透传给 remote-viewer，故用 map 保存。
#[derive(Debug, Clone, Deserialize)]
pub struct SpiceProxyResponse(pub serde_json::Map<String, serde_json::Value>);

impl SpiceProxyResponse {
    pub fn get_str(&self, key: &str) -> Option<String> {
        match self.0.get(key)? {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }

    pub fn host(&self) -> Option<String> {
        self.get_str("host")
    }

    pub fn tls_port(&self) -> Option<u16> {
        self.get_str("tls-port").and_then(|s| s.parse().ok())
    }

    /// 生成 remote-viewer 可直接读取的 .vv(INI) 内容。
    ///
    /// 关键：spiceproxy 的返回是 additionalProperties 开放的，不同 PVE 版本会多带
    /// `host-subject`、`ca`（完整 PEM 证书链）、`delete-this-file` 等键。
    /// 只挑白名单字段会导致 TLS 校验失败，因此这里全量透传 —— 但要去掉
    /// `delete-this-file`：我们是走 stdin 的，没有临时文件可删，留着会让
    /// 查看器尝试删除不存在的文件。
    pub fn to_vv(&self) -> String {
        let mut out = String::from("[virt-viewer]\n");
        for (key, value) in &self.0 {
            if key == "delete-this-file" {
                continue;
            }
            let v = match value {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => {
                    if *b {
                        "1".to_string()
                    } else {
                        "0".to_string()
                    }
                }
                _ => continue,
            };
            out.push_str(key);
            out.push('=');
            // 换行会截断当前值并伪造出新的配置行，必须替换掉
            let sanitized: String = v
                .chars()
                .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
                .collect();
            out.push_str(&sanitized);
            out.push('\n');
        }
        out
    }
}

/// `/nodes/{node}/tasks/{upid}/status`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatus {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub exitstatus: Option<String>,
    #[serde(default)]
    pub node: Option<String>,
}

/// 桌面协议
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Spice,
    Vnc,
}

/// 前端拿到的控制台句柄
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "protocol", rename_all = "lowercase")]
pub enum ConsoleHandle {
    Spice {
        id: String,
        vmid: u32,
        /// 外部查看器的可执行文件路径
        viewer: String,
        /// 选择该协议的原因（含降级说明）
        reason: String,
    },
    Vnc {
        id: String,
        vmid: u32,
        ws_url: String,
        password: String,
        /// 选择该协议的原因（含降级说明）
        reason: String,
    },
}

/// 外部查看器状态
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum ViewerState {
    Running,
    Exited { code: Option<i32> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qxl_with_memory_arg_supports_spice() {
        // 实测 PVE 返回的正是这种带参数的形式
        let cfg = VmConfig {
            vga: Some("qxl,memory=128".into()),
            ..Default::default()
        };
        assert!(cfg.supports_spice());
    }

    #[test]
    fn qxl_multihead_supports_spice() {
        for vga in ["qxl", "qxl2", "qxl3", "qxl4"] {
            let cfg = VmConfig {
                vga: Some(vga.into()),
                ..Default::default()
            };
            assert!(cfg.supports_spice(), "{vga} 应支持 SPICE");
        }
    }

    #[test]
    fn non_qxl_does_not_support_spice() {
        for vga in ["std", "cirrus", "vmware", "virtio", "virtio-gl", "serial0", ""] {
            let cfg = VmConfig {
                vga: Some(vga.into()),
                ..Default::default()
            };
            assert!(!cfg.supports_spice(), "{vga} 不应支持 SPICE");
        }
        assert!(!VmConfig::default().supports_spice());
    }

    #[test]
    fn vv_passes_through_all_fields_but_delete_marker() {
        // 用实测的 spiceproxy 响应形状构造
        let raw = serde_json::json!({
            "proxy": "http://pve.dddbg.com:3128",
            "host-subject": "OU=PVE Cluster Node,O=Proxmox Virtual Environment,CN=pve.dddbg.com",
            "title": "VM 104 - EDA",
            "host": "pvespiceproxy:abc:104:pve:61000::def",
            "tls-port": 61000,
            "toggle-fullscreen": "Shift+F11",
            "type": "spice",
            "release-cursor": "Ctrl+Alt+R",
            "password": "628b2730b460",
            "secure-attention": "Ctrl+Alt+Ins",
            "delete-this-file": 1
        })
        .as_object()
        .unwrap()
        .clone();

        let vv = SpiceProxyResponse(raw).to_vv();

        assert!(vv.starts_with("[virt-viewer]\n"));
        // 关键字段必须透传，否则 TLS / 连接失败
        assert!(vv.contains("host-subject="), "必须保留 host-subject");
        assert!(vv.contains("tls-port=61000"));
        assert!(vv.contains("proxy=http://pve.dddbg.com:3128"));
        assert!(vv.contains("password=628b2730b460"));
        assert!(vv.contains("toggle-fullscreen=Shift+F11"));
        // 走 stdin 时没有临时文件可删，必须去掉该标记
        assert!(!vv.contains("delete-this-file"));
    }

    #[test]
    fn vv_strips_newlines_to_prevent_ini_injection() {
        let raw = serde_json::json!({
            "host": "h",
            "tls-port": 61000,
            "password": "pw\n[evil]\nkey=value",
        })
        .as_object()
        .unwrap()
        .clone();

        let vv = SpiceProxyResponse(raw).to_vv();
        // 换行被抹平后，注入内容只会留在 password 那一行内，无法形成独立的段或配置行
        let config_lines = vv
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('['))
            .count();
        assert_eq!(config_lines, 3, "只应有 host / tls-port / password 三行");
        assert!(!vv.lines().any(|l| l == "[evil]"), "不得出现独立注入段");
    }

    #[test]
    fn rfb_password_prefers_password_field() {
        let resp = VncProxyResponse {
            ticket: "PW:PVEVNC:xxx".into(),
            port: 5900,
            password: Some("PW".into()),
            user: None,
            cert: None,
        };
        assert_eq!(resp.rfb_password(), "PW");
    }

    #[test]
    fn rfb_password_falls_back_to_ticket() {
        let resp = VncProxyResponse {
            ticket: "PVEVNC:xxx".into(),
            port: 5900,
            password: None,
            user: None,
            cert: None,
        };
        assert_eq!(resp.rfb_password(), "PVEVNC:xxx");
    }
}
