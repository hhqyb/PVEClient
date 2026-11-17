pub mod console;
mod error;
mod secrets;
mod settings;
// 以下模块供集成测试直接驱动（PVE 的端点细节只能靠真机验证）
pub mod bridge;
pub mod pve;
pub mod tls;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{Manager, State};
use tokio::sync::RwLock;

use bridge::WsBridge;
use console::ConsoleManager;
use error::{AppError, AppResult};
use pve::models::{ConsoleHandle, Guest, Protocol, Realm, ServerProbe, SessionInfo, TaskStatus, ViewerState};
use pve::PveClient;
use settings::{Settings, SettingsStore};

/// 应用全局状态。
///
/// 设计要点：`client` 用 RwLock 包一层是因为重新登录会替换整个客户端
/// （服务器地址、证书策略都可能变）；控制台管理器则跨会话复用，
/// 以便退出登录时清理残留的外部查看器。
struct AppState {
    settings: SettingsStore,
    client: RwLock<Option<Arc<PveClient>>>,
    consoles: ConsoleManager,
}

impl AppState {
    async fn client(&self) -> AppResult<Arc<PveClient>> {
        self.client
            .read()
            .await
            .clone()
            .ok_or(AppError::NotAuthenticated)
    }

    async fn set_client(&self, client: Option<Arc<PveClient>>) {
        *self.client.write().await = client;
    }
}

fn err<T>(e: AppError) -> Result<T, AppError> {
    Err(e)
}

// ───────────────────────────── 探活 / 登录 ─────────────────────────────

#[tauri::command]
async fn probe_server(
    state: State<'_, AppState>,
    url: String,
    allow_invalid_certs: Option<bool>,
) -> Result<ServerProbe, AppError> {
    let mut settings = state.settings.get();
    let allow_invalid_certs = allow_invalid_certs.unwrap_or(settings.allow_invalid_certs);

    let (base, host) = pve::client::normalize_server(&url)?;
    let client = PveClient::new(base.clone(), host.clone(), allow_invalid_certs)?;

    let (version, realms) = pve::resources::probe(&client).await?;
    // 探活阶段拿不到版本号：PVE 的 /version 需要认证，这里用 realm 数量作为连通凭据
    let version = if version.is_empty() {
        format!("已连接（{} 个认证域）", realms.len())
    } else {
        version
    };

    // TOFU：记录服务器证书指纹，变化时告警（不阻断，由用户判断）
    let port = reqwest::Url::parse(&base)
        .ok()
        .and_then(|u| u.port_or_known_default())
        .unwrap_or(8006);
    let fingerprint = tls::fetch_server_cert_fingerprint(&host, port).await;
    let fingerprint_warning = match (&fingerprint, &settings.last_fingerprint) {
        (Some(current), Some(previous)) if current != previous => Some(format!(
            "服务器证书指纹与上次记录不一致（旧 {previous} → 新 {current}）。\
             若你刚更换过 PVE 证书属正常；否则请与管理员确认，可能存在中间人攻击。"
        )),
        _ => None,
    };

    if let Some(fp) = &fingerprint {
        settings.last_fingerprint = Some(fp.clone());
        settings.server_version = Some(version.clone());
        let _ = state.settings.save(settings);
    }

    // 探活成功后暂存客户端，供后续登录复用（同一连接池 / cookie）
    state.set_client(Some(Arc::new(client))).await;

    Ok(ServerProbe {
        version,
        fingerprint,
        fingerprint_warning,
        realms,
    })
}

#[tauri::command]
async fn list_realms(state: State<'_, AppState>) -> Result<Vec<Realm>, AppError> {
    let client = state.client().await?;
    client.get_anonymous("access/domains").await
}

#[tauri::command]
// Tauri 命令的参数必须是扁平的具名参数（前端按字段名传），因此参数较多
#[allow(clippy::too_many_arguments)]
async fn login(
    state: State<'_, AppState>,
    url: Option<String>,
    username: String,
    realm: String,
    password: String,
    remember: Option<bool>,
    otp: Option<String>,
    allow_invalid_certs: Option<bool>,
) -> Result<SessionInfo, AppError> {
    let settings = state.settings.get();
    let allow_invalid_certs = allow_invalid_certs.unwrap_or(settings.allow_invalid_certs);

    // 如果没有经过 probe，就地建一个客户端
    let client = match (state.client().await, url.clone()) {
        (Ok(existing), None) => existing,
        (_, Some(url)) => {
            let (base, host) = pve::client::normalize_server(&url)?;
            Arc::new(PveClient::new(base, host, allow_invalid_certs)?)
        }
        (Err(_), None) => {
            return err(AppError::InvalidUrl("请先填写服务器地址".into()));
        }
    };

    client
        .login(&username, &realm, &password, otp.as_deref())
        .await?;

    state.set_client(Some(client.clone())).await;

    let info = client
        .session_info()
        .await
        .ok_or(AppError::NotAuthenticated)?;

    // 记住密码：只进钥匙串；不记住则清掉旧条目。
    // 键用与 load_saved_password 完全相同的来源（settings.server），保证读写一致。
    let remember = remember.unwrap_or(settings.remember_password);
    let server = client.base_url().trim_end_matches("/api2/json").to_string();
    let account = secrets::account_key(&server, &info.username, &info.realm);
    if remember {
        if let Err(e) = secrets::save_password(&account, &password) {
            tracing::warn!("保存密码到钥匙串失败：{e}");
        }
    } else {
        let _ = secrets::delete_password(&account);
    }

    // 持久化非敏感信息
    let mut next = settings;
    next.server = server;
    next.username = info.username.clone();
    next.realm = info.realm.clone();
    next.allow_invalid_certs = allow_invalid_certs;
    next.remember_password = remember;
    next.server_version = Some(info.version.clone());
    let _ = state.settings.save(next);

    Ok(info)
}

#[tauri::command]
async fn logout(state: State<'_, AppState>) -> Result<(), AppError> {
    if let Ok(client) = state.client().await {
        client.logout().await;
    }
    state.set_client(None).await;
    state.consoles.close_all().await;
    Ok(())
}

#[tauri::command]
async fn session_info(state: State<'_, AppState>) -> Result<Option<SessionInfo>, AppError> {
    match state.client().await {
        Ok(client) => Ok(client.session_info().await),
        Err(_) => Ok(None),
    }
}

#[tauri::command]
async fn load_saved_password(state: State<'_, AppState>) -> Result<Option<String>, AppError> {
    let settings = state.settings.get();
    if !settings.remember_password {
        return Ok(None);
    }
    // 必须与 login 保存时用同一个键，否则读不到（该 bug 曾让「记住密码」全部失效）
    let account = secrets::account_key(&settings.server, &settings.username, &settings.realm);
    secrets::load_password(&account)
}

// ───────────────────────────── 资源 / 电源 ─────────────────────────────

#[tauri::command]
async fn list_guests(state: State<'_, AppState>) -> Result<Vec<Guest>, AppError> {
    let client = state.client().await?;
    pve::resources::list_guests(&client).await
}

#[tauri::command]
async fn guest_power(
    state: State<'_, AppState>,
    vmid: u32,
    node: String,
    kind: String,
    action: String,
) -> Result<String, AppError> {
    let client = state.client().await?;
    pve::console::power(&client, &node, &kind, vmid, &action).await
}
#[tauri::command]
async fn task_status(
    state: State<'_, AppState>,
    node: String,
    upid: String,
) -> Result<TaskStatus, AppError> {
    let client = state.client().await?;
    pve::console::task_status(&client, &node, &upid).await
}

#[tauri::command]
async fn guest_is_running(
    state: State<'_, AppState>,
    vmid: u32,
    node: String,
    kind: String,
) -> Result<bool, AppError> {
    let client = state.client().await?;
    pve::console::is_running(&client, &node, &kind, vmid).await
}

// ───────────────────────────── 控制台 ─────────────────────────────

#[tauri::command]
async fn open_console(
    state: State<'_, AppState>,
    vmid: u32,
    node: String,
    kind: String,
    force_protocol: Option<String>,
) -> Result<ConsoleHandle, AppError> {
    let client = state.client().await?;
    let settings = state.settings.get();

    // 用最新配置重新判定，避免列表缓存过期（用户可能刚改了 vga）
    let mut guests = pve::resources::list_guests(&client).await?;
    let mut guest = guests
        .drain(..)
        .find(|g| g.vmid == vmid && g.node == node && g.kind == kind)
        .ok_or_else(|| {
            AppError::Forbidden(format!("未找到 VM {vmid}，或当前账号没有访问权限"))
        })?;

    // `/cluster/resources` 的 status 是集群状态缓存，开机任务刚结束时它可能仍报
    // stopped（实测：任务 OK 后 status/current 已是 running，而 resources 还是旧值）。
    // 直接以 status/current 为准，否则「开机并连接」必然误报「虚拟机未运行」。
    if let Ok(running) =
        pve::console::is_running(&client, &guest.node, &guest.kind, guest.vmid).await
    {
        guest.status = if running { "running" } else { "stopped" }.to_string();
    }

    let forced = match force_protocol.as_deref() {
        Some("spice") => Some(Protocol::Spice),
        Some("novnc") | Some("vnc") => Some(Protocol::Vnc),
        _ => settings.protocol_pref(),
    };

    let options = console::SpiceOptions {
        kiosk: settings.spice_kiosk,
        fullscreen: settings.spice_fullscreen,
        usb_redirect: settings.spice_usb_redirect,
    };

    state
        .consoles
        .open(
            &client,
            &guest,
            forced,
            settings.spice_proxy_override.as_deref(),
            options,
            settings.viewer().as_ref(),
        )
        .await
}

/// SPICE 状态栏数据：当前会话信息与查看器存活状态。
#[tauri::command]
async fn spice_status(
    state: State<'_, AppState>,
) -> Result<Option<console::SpiceStatus>, AppError> {
    Ok(state.consoles.spice_status().await)
}

#[tauri::command]
async fn close_console(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    state.consoles.close(&id).await
}

#[tauri::command]
async fn viewer_status(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<ViewerState>, AppError> {
    Ok(state.consoles.viewer_state(&id).await)
}

#[tauri::command]
async fn detect_viewer() -> Result<Option<String>, AppError> {
    Ok(console::spice::detect_viewer().map(|p| p.display().to_string()))
}

// ───────────────────────────── 设置 ─────────────────────────────

#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<Settings, AppError> {
    Ok(state.settings.get())
}

#[tauri::command]
async fn save_settings(
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, AppError> {
    // 隔离：任何情况下都不允许密码混进设置文件
    state.settings.save(settings.clone())?;

    // 证书策略或代理覆盖变化后，需要重建客户端才能生效。
    // 但不打断现有会话：下一次登录时自然使用新策略。
    Ok(state.settings.get())
}

// ───────────────────────────── 启动 ─────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pve_client_lib=info,warn".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| PathBuf::from("."));

            let settings = SettingsStore::load(dir);

            // 本地 WS 中转在应用生命周期内常驻，端口由系统分配
            let bridge = tauri::async_runtime::block_on(WsBridge::start())
                .expect("无法启动本地控制台中转服务");

            let state = AppState {
                settings,
                client: RwLock::new(None),
                consoles: ConsoleManager::new(Arc::new(bridge)),
            };

            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            probe_server,
            list_realms,
            login,
            logout,
            session_info,
            load_saved_password,
            list_guests,
            guest_power,
            task_status,
            guest_is_running,
            open_console,
            close_console,
            spice_status,
            viewer_status,
            detect_viewer,
            get_settings,
            save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}
