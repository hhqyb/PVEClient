use crate::error::{AppError, AppResult};
use crate::pve::client::PveClient;
use crate::pve::models::{SpiceProxyResponse, TaskStatus, VncProxyResponse};

/// 申请 VNC 代理：`POST .../{kind}/{vmid}/vncproxy`（需要 VM.Console）。
///
/// `websocket=1` 让 PVE 走 ws 升级路径；新版本会额外返回 `password`，
/// 该值才是 RFB 握手用的密码，`ticket` 用于 vncwebsocket 的 vncticket 参数。
pub async fn request_vnc(
    client: &PveClient,
    node: &str,
    kind: &str,
    vmid: u32,
) -> AppResult<VncProxyResponse> {
    let path = format!("nodes/{node}/{kind}/{vmid}/vncproxy");
    let form = vec![("websocket".to_string(), "1".to_string())];
    client.post_form(&path, form).await
}

/// 申请 SPICE 配置：`POST .../qemu/{vmid}/spiceproxy`（需要 VM.Console）。
///
/// `proxy_override` 只接受**纯主机名或 IP**：PVE 的参数校验会拒绝 `host:port` 形式，
/// 端口固定由它自己补成 3128。传了端口反而会 400。
pub async fn request_spice(
    client: &PveClient,
    node: &str,
    vmid: u32,
    proxy_override: Option<&str>,
) -> AppResult<SpiceProxyResponse> {
    let path = format!("nodes/{node}/qemu/{vmid}/spiceproxy");
    let form = match proxy_override {
        Some(p) if !p.is_empty() => vec![("proxy".to_string(), p.to_string())],
        _ => Vec::new(),
    };
    let raw: serde_json::Map<String, serde_json::Value> = client.post_form(&path, form).await?;

    let resp = SpiceProxyResponse(raw);

    if resp.host().is_none() || resp.tls_port().is_none() {
        return Err(AppError::SpiceUnreachable(
            "spiceproxy 返回缺少 host / tls-port".into(),
        ));
    }

    Ok(resp)
}

/// 取出 PVE 返回的 proxy 里的主机名，例如 `http://pve.dddbg.com:3128` → `pve.dddbg.com`。
pub fn spice_proxy_host(proxy: &str) -> Option<String> {
    let rest = proxy
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(proxy);
    let host = rest.split('/').next()?.split(':').next()?.trim();
    (!host.is_empty()).then(|| host.to_string())
}

/// 判断主机名能否在本地解析。
///
/// PVE 集群常见配置：它对外通告 `pve.dddbg.com` 这类内部域名，而客户端是用
/// IP 或别的名字（如 Tailscale 的 `pve`）访问的，本地根本解析不到该域名，
/// 于是 SPICE 必然失败。这里先探测，失败则改用用户实际连得上的地址重试。
pub fn resolves(host: &str, port: u16) -> bool {
    use std::net::ToSocketAddrs;
    (host, port).to_socket_addrs().is_ok()
}

/// 读取 VM 当前状态，判断是否运行中（控制台必须先运行）。
pub async fn is_running(
    client: &PveClient,
    node: &str,
    kind: &str,
    vmid: u32,
) -> AppResult<bool> {
    let path = format!("nodes/{node}/{kind}/{vmid}/status/current");
    let value: serde_json::Value = client.get(&path).await?;
    Ok(value
        .get("status")
        .and_then(|s| s.as_str())
        .map(|s| s == "running")
        .unwrap_or(false))
}

/// 电源操作：start / shutdown / reboot / stop
pub async fn power(
    client: &PveClient,
    node: &str,
    kind: &str,
    vmid: u32,
    action: &str,
) -> AppResult<String> {
    let action = match action {
        "start" | "shutdown" | "reboot" | "stop" => action,
        _ => return Err(AppError::Other(format!("不支持的电源操作：{action}"))),
    };
    let path = format!("nodes/{node}/{kind}/{vmid}/status/{action}");
    client.post_empty(&path).await
}

/// 查询任务进度：`GET /nodes/{node}/tasks/{upid}/status`
pub async fn task_status(
    client: &PveClient,
    node: &str,
    upid: &str,
) -> AppResult<TaskStatus> {
    let encoded = urlencoding::encode(upid);
    let path = format!("nodes/{node}/tasks/{encoded}/status");
    client.get(&path).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_proxy_host_from_pve_format() {
        // PVE 实际返回的形状
        assert_eq!(
            spice_proxy_host("http://pve.dddbg.com:3128").as_deref(),
            Some("pve.dddbg.com")
        );
        assert_eq!(
            spice_proxy_host("http://100.98.129.60:3128").as_deref(),
            Some("100.98.129.60")
        );
        // 没有 scheme 或没有端口也要能拆
        assert_eq!(spice_proxy_host("pve:3128").as_deref(), Some("pve"));
        assert_eq!(spice_proxy_host("pve").as_deref(), Some("pve"));
    }

    #[test]
    fn unresolvable_proxy_host_is_detected() {
        // 集群内部域名在本机解析不到，必须能被识别出来，否则 SPICE 会静默失败
        assert!(!resolves("pve.dddbg.com", 3128));
        // 回环地址一定可解析，用来确认探测本身没写反
        assert!(resolves("127.0.0.1", 3128));
    }
}
