//! 针对真实 PVE 实例的集成测试。
//!
//! 默认不运行（需要真实环境与凭据）。手动执行：
//!
//! ```bash
//! PVE_TEST_HOST=https://100.98.129.60:8006 \
//! PVE_TEST_USER=youbin PVE_TEST_REALM=pam PVE_TEST_PASS=xxx \
//! PVE_TEST_EXPECT_SINGLE=1 \
//!   cargo test --test live_pve -- --ignored --nocapture
//! ```
//!
//! 这些测试的价值在于：PVE 的端点可用性、返回字段、WS 握手细节都与文档有出入，
//! 只有对真实实例跑过才算验证（例如 /version 需要认证、vncwebsocket 不接受
//! websocket=1 参数）。

use std::time::Duration;

use pve_client_lib::pve::client::{normalize_server, PveClient};
use pve_client_lib::pve::{console, resources};

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

struct Fixture {
    client: PveClient,
    user: String,
}

async fn setup() -> Option<Fixture> {
    let host = env("PVE_TEST_HOST")?;
    let user = env("PVE_TEST_USER")?;
    let realm = env("PVE_TEST_REALM")?;
    let pass = env("PVE_TEST_PASS")?;

    let (base, hostname) = normalize_server(&host).expect("地址应合法");
    let client = PveClient::new(base, hostname, true).expect("客户端应可创建");
    client
        .login(&user, &realm, &pass, None)
        .await
        .expect("登录应成功");

    Some(Fixture { client, user })
}

#[tokio::test]
#[ignore = "需要真实 PVE 环境"]
async fn realms_are_readable_before_login() {
    let host = match env("PVE_TEST_HOST") {
        Some(h) => h,
        None => return,
    };
    let (base, hostname) = normalize_server(&host).unwrap();
    let client = PveClient::new(base, hostname, true).unwrap();

    // 这是登录页能填 realm 下拉的前提
    let realms: Vec<pve_client_lib::pve::models::Realm> =
        client.get_anonymous("access/domains").await.unwrap();
    assert!(!realms.is_empty(), "至少应有 pam / pve 两个内置域");
    println!(
        "realm 列表：{:?}",
        realms.iter().map(|r| &r.realm).collect::<Vec<_>>()
    );

    // 反向验证：/version 需要认证，匿名访问必须被拒
    let anon_version = client
        .get_anonymous::<serde_json::Value>("version")
        .await;
    assert!(anon_version.is_err(), "/version 匿名访问应被拒绝");
}

#[tokio::test]
#[ignore = "需要真实 PVE 环境"]
async fn login_and_list_guests_respects_permissions() {
    let Some(fx) = setup().await else { return };

    let guests = resources::list_guests(&fx.client).await.unwrap();
    println!("用户 {} 可见 {} 台虚拟机", fx.user, guests.len());
    for g in &guests {
        println!(
            "  {} {} ({}) {} vga={:?} spice={}",
            g.vmid, g.name, g.kind, g.status, g.vga, g.spice_capable
        );
    }

    if env("PVE_TEST_EXPECT_SINGLE").as_deref() == Some("1") {
        assert_eq!(guests.len(), 1, "该账号应只看到 1 台虚拟机");
    }

    // 认证确实生效了（版本号只在登录后可取）
    println!("PVE 版本 = {}", fx.client.version());
}

#[tokio::test]
#[ignore = "需要真实 PVE 环境"]
async fn vnc_console_credentials_shape() {
    let Some(fx) = setup().await else { return };
    let guests = resources::list_guests(&fx.client).await.unwrap();
    let Some(guest) = guests.iter().find(|g| g.is_running()) else {
        println!("没有运行中的虚拟机，跳过");
        return;
    };

    let proxy = console::request_vnc(&fx.client, &guest.node, &guest.kind, guest.vmid)
        .await
        .unwrap();

    println!("vncproxy port={} ticket_len={}", proxy.port, proxy.ticket.len());
    println!("rfb password = {:?}", proxy.rfb_password());

    assert!(proxy.port >= 5900 && proxy.port <= 5999, "端口应在 5900-5999");
    // websocket=1 时 PVE 会把密码拼在 ticket 前面
    if let Some(pw) = &proxy.password {
        assert!(
            proxy.ticket.starts_with(&format!("{pw}:")),
            "ticket 应以 password + ':' 开头"
        );
    }
}

#[tokio::test]
#[ignore = "需要真实 PVE 环境"]
async fn spice_config_is_usable() {
    let Some(fx) = setup().await else { return };
    let guests = resources::list_guests(&fx.client).await.unwrap();
    let Some(guest) = guests.iter().find(|g| g.is_running() && g.spice_capable) else {
        println!("没有运行中且支持 SPICE 的虚拟机，跳过");
        return;
    };

    let cfg = console::request_spice(&fx.client, &guest.node, guest.vmid, None)
        .await
        .unwrap();

    let vv = cfg.to_vv();
    println!("生成的 .vv 配置：\n{vv}");

    assert!(cfg.host().is_some());
    assert!(cfg.tls_port().is_some());
    assert!(vv.contains("[virt-viewer]"));
    assert!(!vv.contains("delete-this-file"), "走 stdin 时不该保留删除标记");
}

#[tokio::test]
#[ignore = "需要真实 PVE 环境"]
async fn vnc_websocket_upgrades() {
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let Some(fx) = setup().await else { return };
    let guests = resources::list_guests(&fx.client).await.unwrap();
    let Some(guest) = guests.iter().find(|g| g.is_running()) else {
        println!("没有运行中的虚拟机，跳过");
        return;
    };

    let proxy = console::request_vnc(&fx.client, &guest.node, &guest.kind, guest.vmid)
        .await
        .unwrap();
    let ticket = fx.client.export_ticket().await.unwrap();

    let url = format!(
        "{}/nodes/{}/{}/{}/vncwebsocket?port={}&vncticket={}",
        fx.client.ws_base_url(),
        guest.node,
        guest.kind,
        guest.vmid,
        proxy.port,
        urlencoding::encode(&proxy.ticket)
    );

    let mut req = url.as_str().into_client_request().unwrap();
    req.headers_mut()
        .insert("Cookie", format!("PVEAuthCookie={ticket}").parse().unwrap());

    let connector = tokio_tungstenite::Connector::Rustls(std::sync::Arc::new(
        pve_client_lib::tls::ws_client_config(true).unwrap(),
    ));

    let (mut ws, resp) = tokio_tungstenite::connect_async_tls_with_config(
        req,
        None,
        false,
        Some(connector),
    )
    .await
    .expect("WS 应升级成功（101）");

    println!("握手成功，HTTP 状态 = {}", resp.status());
    assert_eq!(resp.status().as_u16(), 101);

    // 给 VNC 服务器一点时间推初始握手数据
    if let Ok(Some(Ok(msg))) =
        tokio::time::timeout(Duration::from_secs(5), ws.next()).await
    {
        println!("收到首帧：{} bytes", msg.len());
    }

    let _ = ws.close(None).await;
}
