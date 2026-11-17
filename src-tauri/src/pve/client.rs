use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{HeaderMap, HeaderValue, COOKIE};
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use tokio::sync::RwLock;

use crate::error::{AppError, AppResult};
use crate::pve::models::{Session, TicketResponse};

pub const TICKET_LIFETIME_SECS: i64 = 2 * 60 * 60;
/// 提前量：剩余不足 5 分钟就续期，避免请求途中过期。
const RENEW_AHEAD_SECS: i64 = 5 * 60;

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 规范化用户输入的服务器地址，返回 (base_url, host)。
///
/// 接受 `100.98.129.60`、`https://100.98.129.60:8006`、带尾斜杠或带 /api2/json 的形式。
pub fn normalize_server(raw: &str) -> AppResult<(String, String)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidUrl("地址不能为空".into()));
    }

    let with_scheme = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };

    let mut parsed = reqwest::Url::parse(&with_scheme)
        .map_err(|e| AppError::InvalidUrl(format!("{with_scheme} — {e}")))?;

    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err(AppError::InvalidUrl(format!(
            "不支持的协议：{}",
            parsed.scheme()
        )));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| AppError::InvalidUrl("缺少主机名".into()))?
        .to_string();

    // 去掉用户可能粘贴进来的 API 路径
    let path = parsed.path().trim_end_matches('/').to_string();
    let path = path
        .strip_suffix("/api2/json")
        .map(|s| s.to_string())
        .unwrap_or(path);
    parsed.set_path(&path);
    parsed.set_query(None);
    parsed.set_fragment(None);

    let base = parsed.as_str().trim_end_matches('/').to_string();
    Ok((base, host))
}

/// 把 `user@realm` 拆成两段。
///
/// PVE 的 `/access/ticket` 会把 `username` 按 `user@realm` 解析，若再同时带 `realm`
/// 参数就会拼成 `user@realm@realm` 从而认证失败。这里先拆开，保证只有一处携带认证域。
fn split_user_realm(input: &str) -> (String, Option<String>) {
    let trimmed = input.trim();
    match trimmed.rsplit_once('@') {
        Some((user, realm)) if !user.is_empty() && !realm.is_empty() => {
            (user.to_string(), Some(realm.to_string()))
        }
        _ => (trimmed.to_string(), None),
    }
}

/// 内网自建场景下 PVE 几乎必然是自签证书。默认放宽校验，但每次连接都记录指纹供 TOFU 比对。
pub fn build_http_client(allow_invalid_certs: bool) -> AppResult<Client> {
    let mut builder = Client::builder()
        .cookie_store(true)
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .user_agent(concat!("PVEClient/", env!("CARGO_PKG_VERSION")));

    builder = if allow_invalid_certs {
        builder.danger_accept_invalid_certs(true).danger_accept_invalid_hostnames(true)
    } else {
        builder
    };

    builder
        .build()
        .map_err(|e| AppError::Network(format!("无法初始化 HTTP 客户端：{e}")))
}

/// PVE REST 客户端。
///
/// 设计要点：
/// - ticket 只存在 Rust 侧，绝不下发前端
/// - 每次请求前按需续期（用旧 ticket 当 password，不需要明文密码）
/// - 统一解包 `{"data": ...}`，把 PVE 的 `errors`/401/403 映射成 AppError
pub struct PveClient {
    http: Client,
    base: String,
    host: String,
    allow_invalid_certs: bool,
    session: Arc<RwLock<Option<Session>>>,
    version: Arc<RwLock<String>>,
}

impl PveClient {
    pub fn new(base: String, host: String, allow_invalid_certs: bool) -> AppResult<Self> {
        Ok(Self {
            http: build_http_client(allow_invalid_certs)?,
            base,
            host,
            allow_invalid_certs,
            session: Arc::new(RwLock::new(None)),
            version: Arc::new(RwLock::new(String::new())),
        })
    }

    pub fn allow_invalid_certs(&self) -> bool {
        self.allow_invalid_certs
    }

    pub fn base_url(&self) -> String {
        format!("{}/api2/json", self.base)
    }

    /// 用户实际连通的服务器主机名，用于纠正 PVE 通告的不可达 spiceproxy 域名。
    pub fn host_for_proxy(&self) -> Option<String> {
        (!self.host.trim().is_empty()).then(|| self.host.clone())
    }

    pub fn ws_base_url(&self) -> String {
        if self.base.starts_with("https://") {
            format!("wss://{}/api2/json", self.base.trim_start_matches("https://"))
        } else {
            format!("ws://{}/api2/json", self.base.trim_start_matches("http://"))
        }
    }

    pub fn set_version(&self, v: String) {
        if let Ok(mut slot) = self.version.try_write() {
            *slot = v;
        }
    }

    pub fn version(&self) -> String {
        self.version.try_read().map(|v| v.clone()).unwrap_or_default()
    }

    pub async fn session_info(&self) -> Option<crate::pve::models::SessionInfo> {
        let guard = self.session.read().await;
        let s = guard.as_ref()?;
        Some(crate::pve::models::SessionInfo {
            username: s.username.clone(),
            realm: s.realm.clone(),
            host: s.host.clone(),
            version: self.version(),
            expires_at: s.expires_at,
        })
    }

    async fn current_ticket(&self) -> Option<(String, Option<String>)> {
        let guard = self.session.read().await;
        guard
            .as_ref()
            .map(|s| (s.ticket.clone(), s.csrf.clone()))
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api2/json/{}", self.base, path.trim_start_matches('/'))
    }

    /// 把 PVE 的错误响应翻译成有意义的 AppError。
    fn map_error(status: StatusCode, body: &str) -> AppError {
        let trimmed = body.trim();

        // PVE 参数错误形如 {"errors":{"password":"..."},"message":"..."}
        let parsed = serde_json::from_str::<serde_json::Value>(trimmed).ok();

        let detail = parsed
            .as_ref()
            .and_then(|v| {
                if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
                    return Some(msg.to_string());
                }
                let errs = v.get("errors")?.as_object()?;
                let joined = errs
                    .iter()
                    .map(|(k, val)| format!("{k}: {}", val.as_str().unwrap_or("")))
                    .collect::<Vec<_>>()
                    .join("; ");
                Some(joined)
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| trimmed.to_string());

        match status {
            // 带票请求的 401 已在 request() 里先行处理成 SessionExpired，
            // 这里剩下的 401 只来自登录或匿名探活：PVE 用 401 + {"data":null}
            // 表示凭据被拒，必须报“用户名或密码错误”，不能说成会话过期。
            StatusCode::UNAUTHORIZED => AppError::InvalidCredentials,
            StatusCode::FORBIDDEN => AppError::Forbidden(detail),
            StatusCode::BAD_REQUEST => {
                if detail.contains("no such realm") {
                    AppError::InvalidRealm(detail)
                } else {
                    AppError::InvalidCredentials
                }
            }
            _ => AppError::BadResponse(format!("HTTP {} — {}", status.as_u16(), detail)),
        }
    }

    fn classify_transport_error(e: &reqwest::Error) -> AppError {
        let text = e.to_string();
        if e.is_timeout() {
            return AppError::Network("连接超时".into());
        }
        if e.is_connect() {
            if text.contains("certificate") || text.contains("tls") || text.contains("SSL") {
                return AppError::Tls(text);
            }
            return AppError::Network(format!("无法建立连接：{text}"));
        }
        if text.contains("certificate") || text.contains("UnknownIssuer") {
            return AppError::Tls(text);
        }
        AppError::Network(text)
    }

    /// 未认证的 GET（用于探活与 realm 列表，PVE 允许匿名访问）
    pub async fn get_anonymous<T: DeserializeOwned>(&self, path: &str) -> AppResult<T> {
        let resp = self
            .http
            .get(self.url(path))
            .send()
            .await
            .map_err(|e| Self::classify_transport_error(&e))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| Self::classify_transport_error(&e))?;

        if !status.is_success() {
            return Err(Self::map_error(status, &body));
        }
        unwrap_data(&body)
    }

    /// 已认证请求。写操作自动附加 CSRF 头。
    pub async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        form: Option<Vec<(String, String)>>,
    ) -> AppResult<T> {
        self.ensure_fresh_ticket().await?;
        let (ticket, csrf) = self
            .current_ticket()
            .await
            .ok_or(AppError::NotAuthenticated)?;

        let url = self.url(path);

        let send = |ticket: String, csrf: Option<String>| {
            let mut req = self
                .http
                .request(method.clone(), &url)
                .header(COOKIE, format!("PVEAuthCookie={ticket}"));

            if method != Method::GET {
                if let Some(token) = &csrf {
                    if let Ok(v) = HeaderValue::from_str(token) {
                        let mut headers = HeaderMap::new();
                        headers.insert("CSRFPreventionToken", v);
                        req = req.headers(headers);
                    }
                }
            }

            if let Some(pairs) = &form {
                req = req.form(pairs);
            }
            req.send()
        };

        let resp = send(ticket.clone(), csrf.clone())
            .await
            .map_err(|e| Self::classify_transport_error(&e))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| Self::classify_transport_error(&e))?;

        if status == StatusCode::UNAUTHORIZED {
            // 服务端判定 ticket 失效：清掉会话，由上层引导重新登录
            let mut guard = self.session.write().await;
            *guard = None;
            return Err(AppError::SessionExpired);
        }

        if !status.is_success() {
            return Err(Self::map_error(status, &body));
        }

        unwrap_data(&body)
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> AppResult<T> {
        self.request(Method::GET, path, None).await
    }

    pub async fn post_form<T: DeserializeOwned>(
        &self,
        path: &str,
        form: Vec<(String, String)>,
    ) -> AppResult<T> {
        self.request(Method::POST, path, Some(form)).await
    }

    pub async fn post_empty<T: DeserializeOwned>(&self, path: &str) -> AppResult<T> {
        self.request(Method::POST, path, Some(Vec::new())).await
    }

    /// 登录：POST /access/ticket
    ///
    /// PVE 的实际行为（实测）：
    /// - `username=root` 且不带 realm → 401
    /// - `username=root@pam` 且不带 realm → 200
    /// - `username=root` + `realm=pam` → 200
    /// - `username=root@pam` + `realm=pam` → 401（被拼成 root@pam@pam）
    ///
    /// 也就是说认证域必须**恰好出现一次**：要么写在用户名里，要么单独传 realm。
    /// 这里统一取「用户名里的 realm」与「显式 realm」，取到的那一个只发一次，
    /// 并保证 session.username 始终是完整的 `user@realm`（renew 依赖它）。
    pub async fn login(
        &self,
        username: &str,
        realm: &str,
        password: &str,
        otp: Option<&str>,
    ) -> AppResult<Session> {
        let (user_part, realm_from_user) = split_user_realm(username);
        // 下拉框是用户的显式选择，优先于用户名里可能残留的 @realm 后缀；
        // 只有在界面没有认证域可选时才退回用户名自带的那一个。
        let picked = realm.trim();
        let realm = if !picked.is_empty() {
            picked.to_string()
        } else {
            realm_from_user.unwrap_or_default()
        };

        if user_part.is_empty() {
            return Err(AppError::InvalidCredentials);
        }
        // 没有认证域时 PVE 一定拒绝，直接给出可执行的提示，不浪费一次往返
        if realm.is_empty() {
            return Err(AppError::InvalidRealm(
                "请选择认证域（如 pam / pve / 你的 AD 域）".into(),
            ));
        }

        let mut form = vec![("username".to_string(), user_part.clone())];
        form.push(("realm".to_string(), realm.clone()));
        form.push(("password".to_string(), password.to_string()));
        if let Some(code) = otp {
            form.push(("otp".to_string(), code.to_string()));
        }

        let resp = self
            .http
            .post(self.url("access/ticket"))
            .form(&form)
            .send()
            .await
            .map_err(|e| Self::classify_transport_error(&e))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| Self::classify_transport_error(&e))?;

        if !status.is_success() {
            return Err(Self::map_error(status, &body));
        }

        // 双因素：PVE 会返回需要挑战的提示而不是 ticket
        let value: serde_json::Value = serde_json::from_str(&body)?;
        if let Some(challenge) = value
            .get("data")
            .and_then(|d| d.get("NeedTFA"))
            .and_then(|v| v.as_str())
        {
            return Err(AppError::NeedTfa {
                challenge: challenge.to_string(),
            });
        }

        let data = value
            .get("data")
            .ok_or_else(|| AppError::BadResponse("响应缺少 data 字段".into()))?;
        let parsed: TicketResponse = serde_json::from_value(data.clone())?;

        let session = Session {
            ticket: parsed.ticket,
            csrf: parsed.csrf,
            // 始终存成完整的 user@realm：renew() 依赖它拆回用户名与认证域
            username: parsed
                .username
                .unwrap_or_else(|| format!("{user_part}@{realm}")),
            realm: realm.clone(),
            host: self.host.clone(),
            expires_at: now_secs() + TICKET_LIFETIME_SECS,
        };

        let mut guard = self.session.write().await;
        *guard = Some(session.clone());
        drop(guard);

        // `/version` 需要认证，只能在登录成功后取；顺带缓存以便界面展示
        if self.version().is_empty() {
            if let Ok(v) = self.get::<crate::pve::models::VersionInfo>("version").await {
                if let Some(ver) = v.version.or(v.release) {
                    self.set_version(ver);
                }
            }
        }

        Ok(session)
    }

    /// 续期：把旧 ticket 当作 password 重新换一张新 ticket。
    pub async fn renew(&self) -> AppResult<()> {
        let (ticket, _) = self
            .current_ticket()
            .await
            .ok_or(AppError::NotAuthenticated)?;
        let username = {
            let guard = self.session.read().await;
            guard.as_ref().map(|s| s.username.clone()).unwrap_or_default()
        };
        let (user_part, realm_part) = match username.rsplit_once('@') {
            Some((u, r)) => (u.to_string(), r.to_string()),
            None => (username.clone(), String::new()),
        };

        let mut form = vec![
            ("username".to_string(), user_part),
            ("password".to_string(), ticket),
        ];
        if !realm_part.is_empty() {
            form.push(("realm".to_string(), realm_part));
        }

        let resp = self
            .http
            .post(self.url("access/ticket"))
            .form(&form)
            .send()
            .await
            .map_err(|e| Self::classify_transport_error(&e))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| Self::classify_transport_error(&e))?;

        if !status.is_success() {
            let mut guard = self.session.write().await;
            *guard = None;
            return Err(AppError::SessionExpired);
        }

        let parsed: TicketResponse = unwrap_data::<TicketResponse>(&body)?;
        let mut guard = self.session.write().await;
        if let Some(s) = guard.as_mut() {
            s.ticket = parsed.ticket;
            if parsed.csrf.is_some() {
                s.csrf = parsed.csrf;
            }
            s.expires_at = now_secs() + TICKET_LIFETIME_SECS;
        }
        Ok(())
    }

    async fn ensure_fresh_ticket(&self) -> AppResult<()> {
        let needs_renew = {
            let guard = self.session.read().await;
            match guard.as_ref() {
                None => return Err(AppError::NotAuthenticated),
                Some(s) => s.expires_at - now_secs() < RENEW_AHEAD_SECS,
            }
        };
        if needs_renew {
            self.renew().await?;
        }
        Ok(())
    }

    pub async fn logout(&self) {
        let mut guard = self.session.write().await;
        *guard = None;
    }

    /// 取当前 ticket，供 noVNC 代理在 WS 握手时注入 Cookie。
    pub async fn export_ticket(&self) -> AppResult<String> {
        self.ensure_fresh_ticket().await?;
        self.current_ticket()
            .await
            .map(|(t, _)| t)
            .ok_or(AppError::NotAuthenticated)
    }
}

/// PVE 所有成功响应都包一层 `{"data": ...}`。
fn unwrap_data<T: DeserializeOwned>(body: &str) -> AppResult<T> {
    if body.trim().is_empty() {
        // 部分端点（如 status/start）成功时 data 为 null
        return serde_json::from_str::<Option<T>>("null")
            .ok()
            .flatten()
            .ok_or_else(|| AppError::BadResponse("空响应".into()));
    }

    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| AppError::BadResponse(e.to_string()))?;

    let data = value
        .get("data")
        .cloned()
        .ok_or_else(|| AppError::BadResponse("响应缺少 data 字段".into()))?;

    serde_json::from_value(data).map_err(|e| AppError::BadResponse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_bare_host() {
        let (base, host) = normalize_server("100.98.129.60").unwrap();
        assert_eq!(base, "https://100.98.129.60");
        assert_eq!(host, "100.98.129.60");
    }

    #[test]
    fn normalizes_full_url_with_port() {
        let (base, host) = normalize_server("https://100.98.129.60:8006/").unwrap();
        assert_eq!(base, "https://100.98.129.60:8006");
        assert_eq!(host, "100.98.129.60");
    }

    #[test]
    fn strips_api_suffix() {
        let (base, _) = normalize_server("https://pve.local:8006/api2/json").unwrap();
        assert_eq!(base, "https://pve.local:8006");
    }

    #[test]
    fn splits_realm_off_the_username() {
        // 界面把已记住的 root@pam 填进用户名框，同时又选中 pam 下的场景：
        // 不拆开就会发出 username=root@pam + realm=pam，被 PVE 拼成 root@pam@pam
        assert_eq!(
            split_user_realm("root@pam"),
            ("root".to_string(), Some("pam".to_string()))
        );
        assert_eq!(
            split_user_realm("youbin@ad.local"),
            ("youbin".to_string(), Some("ad.local".to_string()))
        );
        // 没有 realm 后缀时保持原样，由 realm 参数单独决定
        assert_eq!(split_user_realm("root"), ("root".to_string(), None));
        // 首尾空白来自输入框，必须去掉
        assert_eq!(
            split_user_realm("  root@pam  "),
            ("root".to_string(), Some("pam".to_string()))
        );
        // 残缺形式不应产生空用户名或空 realm
        assert_eq!(split_user_realm("root@"), ("root@".to_string(), None));
        assert_eq!(split_user_realm("@pam"), ("@pam".to_string(), None));
    }

    #[test]
    fn login_failure_is_invalid_credentials_not_session_expired() {
        // 实测：密码错误时 PVE 返回 401 + {"data":null}，必须翻译成凭据错误，
        // 否则用户输错密码会被提示“会话已过期”，完全误导
        for body in [r#"{"data":null}"#, "", r#"{"message":"invalid credentials"}"#] {
            let err = PveClient::map_error(StatusCode::UNAUTHORIZED, body);
            assert_eq!(
                err.code(),
                "INVALID_CREDENTIALS",
                "body={body} 应被判为凭据错误"
            );
        }
    }
}
