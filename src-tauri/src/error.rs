use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("无法连接到服务器：{0}")]
    Network(String),

    #[error("TLS 证书校验失败：{0}")]
    Tls(String),

    #[error("服务器地址无效：{0}")]
    InvalidUrl(String),

    #[error("服务器返回了非预期的响应：{0}")]
    BadResponse(String),

    #[error("用户名或密码错误")]
    InvalidCredentials,

    #[error("认证域（realm）不存在或不可用：{0}")]
    InvalidRealm(String),

    #[error("需要双因素认证")]
    NeedTfa { challenge: String },

    #[error("权限不足：{0}")]
    Forbidden(String),

    #[error("会话已过期，请重新登录")]
    SessionExpired,

    #[error("虚拟机未运行：{0}")]
    GuestNotRunning(String),

    #[error("外部查看器异常：{0}")]
    ViewerFailed(String),

    #[error("SPICE 连接失败（请确认 3128 与 61000-61999 端口已放通）：{0}")]
    SpiceUnreachable(String),

    #[error("未登录")]
    NotAuthenticated,

    #[error("文件存储错误：{0}")]
    Io(String),

    #[error("系统钥匙串错误：{0}")]
    Keyring(String),

    #[error("{0}")]
    Other(String),
}

impl AppError {
    /// 稳定的机器可读错误码，前端据此决定提示与降级行为。
    pub fn code(&self) -> &'static str {
        match self {
            Self::Network(_) => "NETWORK",
            Self::Tls(_) => "TLS",
            Self::InvalidUrl(_) => "INVALID_URL",
            Self::BadResponse(_) => "BAD_RESPONSE",
            Self::InvalidCredentials => "INVALID_CREDENTIALS",
            Self::InvalidRealm(_) => "INVALID_REALM",
            Self::NeedTfa { .. } => "NEED_TFA",
            Self::Forbidden(_) => "FORBIDDEN",
            Self::SessionExpired => "SESSION_EXPIRED",
            Self::GuestNotRunning(_) => "GUEST_NOT_RUNNING",
            Self::ViewerFailed(_) => "VIEWER_FAILED",
            Self::SpiceUnreachable(_) => "SPICE_UNREACHABLE",
            Self::NotAuthenticated => "NOT_AUTHENTICATED",
            Self::Io(_) => "IO",
            Self::Keyring(_) => "KEYRING",
            Self::Other(_) => "OTHER",
        }
    }

}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: String,
    tfa_challenge: Option<&'a str>,
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let body = ErrorBody {
            code: self.code(),
            message: self.to_string(),
            tfa_challenge: match self {
                Self::NeedTfa { challenge } => Some(challenge.as_str()),
                _ => None,
            },
        };
        body.serialize(serializer)
    }
}

pub type AppResult<T> = Result<T, AppError>;

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::BadResponse(e.to_string())
    }
}
