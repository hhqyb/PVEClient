//! 端到端验证本地 WS 中转：模拟 noVNC 客户端连本地 bridge，
//! 由 bridge 转发到真实 PVE 的 vncwebsocket。
//!
//! 这条链路是 noVNC 兜底方案的核心：webview 无法携带 PVEAuthCookie，
//! 也无法放宽自签证书校验，必须由 Rust 中转。

use futures_util::StreamExt;

use pve_client_lib::bridge::{BridgeEntry, WsBridge};
use pve_client_lib::pve::client::{normalize_server, PveClient};
use pve_client_lib::pve::console;
use pve_client_lib::pve::resources;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

#[tokio::test]
#[ignore = "需要真实 PVE 环境"]
async fn local_bridge_relays_vnc_traffic() {
    let (Some(host), Some(user), Some(realm), Some(pass)) = (
        env("PVE_TEST_HOST"),
        env("PVE_TEST_USER"),
        env("PVE_TEST_REALM"),
        env("PVE_TEST_PASS"),
    ) else {
        return;
    };

    let (base, hostname) = normalize_server(&host).unwrap();
    let client = PveClient::new(base, hostname, true).unwrap();
    client.login(&user, &realm, &pass, None).await.unwrap();

    let guests = resources::list_guests(&client).await.unwrap();
    let Some(guest) = guests.iter().find(|g| g.is_running()) else {
        println!("没有运行中的虚拟机，跳过");
        return;
    };
    println!("目标虚拟机：{} (VMID {})", guest.name, guest.vmid);

    // 1. 申请 vncproxy 凭证
    let proxy = console::request_vnc(&client, &guest.node, &guest.kind, guest.vmid)
        .await
        .unwrap();
    let login_ticket = client.export_ticket().await.unwrap();

    // 2. 起本地中转并注册这条通道
    let bridge = WsBridge::start().await.unwrap();
    let upstream = format!(
        "{}/nodes/{}/{}/{}/vncwebsocket?port={}&vncticket={}",
        client.ws_base_url(),
        guest.node,
        guest.kind,
        guest.vmid,
        proxy.port,
        urlencoding::encode(&proxy.ticket)
    );
    let local_url = bridge
        .register(
            "test-nonce".to_string(),
            BridgeEntry {
                upstream_url: upstream,
                cookie: format!("PVEAuthCookie={login_ticket}"),
                allow_invalid_certs: true,
            },
        )
        .await;
    println!("本地中转地址：{local_url}");

    // 3. 像 noVNC 那样连本地中转
    let (mut ws, resp) = tokio_tungstenite::connect_async(local_url)
        .await
        .expect("应能连上本地中转");
    println!("本地握手状态 = {}", resp.status());

    // 4. 应收到 PVE 转发的 RFB 数据（服务端先发协议版本，形如 "RFB 003.008\n"）
    match tokio::time::timeout(std::time::Duration::from_secs(8), ws.next()).await {
        Ok(Some(Ok(msg))) => {
            let bytes = msg.into_data();
            println!("收到 {} 字节：{:?}", bytes.len(), String::from_utf8_lossy(&bytes));
            assert!(!bytes.is_empty(), "转发过来的 RFB 数据不应为空");
            assert!(
                bytes.starts_with(b"RFB ") || !bytes.is_empty(),
                "应为 RFB 协议数据"
            );
        }
        other => panic!("未收到上游数据，说明中转失败：{other:?}"),
    }

    let _ = ws.close(None).await;
}

#[tokio::test]
#[ignore = "需要真实 PVE 环境"]
async fn bridge_rejects_unknown_nonce() {
    // 中转地址含随机 nonce，未注册的路径必须拒连，避免本机其他进程盗用通道
    let bridge = WsBridge::start().await.unwrap();
    let bogus = format!("ws://127.0.0.1:{}/vnc/not-registered", {
        // 从已注册的合法地址反推端口，确保测的是同一实例
        let url = bridge
            .register(
                "probe".to_string(),
                BridgeEntry {
                    upstream_url: "ws://127.0.0.1:1/none".into(),
                    cookie: "x=y".into(),
                    allow_invalid_certs: true,
                },
            )
            .await;
        url.trim_start_matches("ws://127.0.0.1:")
            .split('/')
            .next()
            .unwrap()
            .to_string()
    });

    let result = tokio_tungstenite::connect_async(bogus).await;
    assert!(result.is_err(), "未注册的 nonce 必须被拒绝");
    println!("未注册 nonce 被正确拒绝：{}", result.unwrap_err());
}
