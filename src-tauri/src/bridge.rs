use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::error::{AppError, AppResult};
use crate::tls;

/// 一条待建立的控制台通道。nonce 是唯一的访问凭据，
/// 前端只拿到 `ws://127.0.0.1:<port>/vnc/<nonce>`，没有任何 PVE 凭证。
#[derive(Clone)]
pub struct BridgeEntry {
    pub upstream_url: String,
    pub cookie: String,
    pub allow_invalid_certs: bool,
}

#[derive(Clone, Default)]
struct BridgeState {
    entries: Arc<Mutex<std::collections::HashMap<String, BridgeEntry>>>,
}

/// 本地 WS 中转服务。
///
/// 为什么必须有它：
/// 1. `vncwebsocket` 除了 vncticket 还要求 `PVEAuthCookie`，webview 里的跨域
///    请求带不上这个 cookie；
/// 2. PVE 默认自签证书，webview 无法按需放宽 TLS 校验；
/// 3. 凭证不进前端，只暴露一个随机 nonce 的本地地址。
pub struct WsBridge {
    port: u16,
    state: BridgeState,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl WsBridge {
    pub async fn start() -> AppResult<Self> {
        let state = BridgeState::default();
        let app = Router::new()
            .route("/vnc/:nonce", get(handle_upgrade))
            .with_state(state.clone());

        // 端口 0 让系统分配空闲端口，避免固定端口冲突
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|e| AppError::Io(format!("无法绑定本地端口：{e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| AppError::Io(e.to_string()))?
            .port();

        let (tx, rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            let server = axum::serve(listener, app).with_graceful_shutdown(async {
                let _ = rx.await;
            });
            if let Err(e) = server.await {
                tracing::error!("本地 WS 服务退出：{e}");
            }
        });

        Ok(Self {
            port,
            state,
            shutdown: Some(tx),
        })
    }

    /// 注册一条通道，返回前端可用的本地 ws 地址。
    pub async fn register(&self, nonce: String, entry: BridgeEntry) -> String {
        self.state.entries.lock().await.insert(nonce.clone(), entry);
        format!("ws://127.0.0.1:{}/vnc/{}", self.port, nonce)
    }

    pub async fn unregister(&self, nonce: &str) {
        self.state.entries.lock().await.remove(nonce);
    }
}

impl Drop for WsBridge {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

async fn handle_upgrade(
    ws: WebSocketUpgrade,
    Path(nonce): Path<String>,
    State(state): State<BridgeState>,
) -> impl IntoResponse {
    let entry = state.entries.lock().await.get(&nonce).cloned();

    match entry {
        Some(entry) => ws
            // 与 noVNC 及 PVE 两侧保持一致：协商 `binary` 子协议
            .protocols(["binary"])
            .on_upgrade(move |socket| async move {
                if let Err(e) = proxy(socket, entry).await {
                    tracing::warn!("控制台中转结束：{e}");
                }
            })
            .into_response(),
        None => (
            axum::http::StatusCode::NOT_FOUND,
            "control channel not found",
        )
            .into_response(),
    }
}

/// 双向透传：浏览器 ←→ 本地 ws ←→ PVE wss
async fn proxy(client_ws: WebSocket, entry: BridgeEntry) -> AppResult<()> {
    let mut request = entry
        .upstream_url
        .as_str()
        .into_client_request()
        .map_err(|e| AppError::Other(format!("上游 WS 请求无效：{e}")))?;

    request.headers_mut().insert(
        "Cookie",
        entry
            .cookie
            .parse()
            .map_err(|_| AppError::Other("Cookie 头无效".into()))?,
    );

    // PVE 的 vncwebsocket 会协商 `binary` 子协议；noVNC 侧也请求了它。
    // 中间层必须一并透传，否则上游可能拒绝升级。
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        "binary"
            .parse()
            .map_err(|_| AppError::Other("WS 子协议头无效".into()))?,
    );

    let connector = tokio_tungstenite::Connector::Rustls(Arc::new(
        tls::ws_client_config(entry.allow_invalid_certs)?,
    ));

    let (upstream, _resp) =
        tokio_tungstenite::connect_async_tls_with_config(request, None, false, Some(connector))
            .await
            .map_err(|e| {
                let text = e.to_string();
                if text.contains("certificate") || text.contains("Certificate") {
                    AppError::Tls(format!(
                        "上游 WS 的 TLS 证书不受信任（如需连接自签证书的 PVE，请在设置中允许无效证书）：{text}"
                    ))
                } else {
                    AppError::SpiceUnreachable(format!("上游 WS 连接失败：{text}"))
                }
            })?;

    let (mut client_tx, mut client_rx) = client_ws.split();
    let (mut up_tx, mut up_rx) = upstream.split();

    let client_to_upstream = async {
        while let Some(msg) = client_rx.next().await {
            match msg {
                Ok(Message::Binary(data)) => {
                    if up_tx
                        .send(tokio_tungstenite::tungstenite::Message::Binary(data))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(Message::Text(text)) => {
                    if up_tx
                        .send(tokio_tungstenite::tungstenite::Message::Text(text))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(Message::Ping(p)) => {
                    let _ = up_tx
                        .send(tokio_tungstenite::tungstenite::Message::Ping(p))
                        .await;
                }
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
        let _ = up_tx.close().await;
    };

    let upstream_to_client = async {
        while let Some(msg) = up_rx.next().await {
            match msg {
                Ok(tokio_tungstenite::tungstenite::Message::Binary(data)) => {
                    if client_tx.send(Message::Binary(data)).await.is_err() {
                        break;
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                    if client_tx.send(Message::Text(text)).await.is_err() {
                        break;
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Ping(p)) => {
                    let _ = client_tx.send(Message::Ping(p)).await;
                }
                Ok(tokio_tungstenite::tungstenite::Message::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
        let _ = client_tx.close().await;
    };

    tokio::select! {
        _ = client_to_upstream => {},
        _ = upstream_to_client => {},
    }

    Ok(())
}
