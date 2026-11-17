use crate::error::{AppError, AppResult};

const SERVICE: &str = "dev.pveclient.desktop";

/// 钥匙串条目的账号键。
///
/// **保存与读取必须用同一个函数**：早先保存时用 `user@host@realm`、读取时用
/// `user@server_url@realm`，两者永远不相等，导致「记住密码」在所有平台上
/// 都静默失效（把密码写进去了，却读不出来）。
///
/// 键里包含服务器（规范化后不含协议与路径）与认证域，从而支持多套 PVE 环境并存。
pub fn account_key(server: &str, username: &str, realm: &str) -> String {
    // 只保留 host[:port]：https://pve:8006/api2/json → pve:8006
    let host = server
        .trim()
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(server.trim())
        .trim_end_matches('/')
        .split('/')
        .next()
        .unwrap_or("")
        .to_string();

    // 用户名去掉可能已带上的 @realm，避免键里重复出现认证域
    let user = username.trim();
    let user = match user.rsplit_once('@') {
        Some((u, _)) if !u.is_empty() => u,
        _ => user,
    };

    format!("{user}@{host}@{realm}")
}

fn entry(account: &str) -> AppResult<keyring::Entry> {
    keyring::Entry::new(SERVICE, account).map_err(|e| AppError::Keyring(e.to_string()))
}

/// 密码只进系统钥匙串（Windows 凭据管理器 / macOS Keychain / libsecret），
/// 绝不写入 settings.json。
pub fn save_password(account: &str, password: &str) -> AppResult<()> {
    entry(account)?
        .set_password(password)
        .map_err(|e| AppError::Keyring(e.to_string()))
}

pub fn load_password(account: &str) -> AppResult<Option<String>> {
    match entry(account)?.get_password() {
        Ok(pw) => Ok(Some(pw)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Keyring(e.to_string())),
    }
}

pub fn delete_password(account: &str) -> AppResult<()> {
    match entry(account)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Keyring(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_key_is_stable_across_save_and_load_inputs() {
        // 保存与读取现在都传 settings.server（完整 URL），必须得到同一个键。
        // 这正是曾经失效的原因：保存用 host（pve），读取用 URL（https://pve:8006）。
        assert_eq!(
            account_key("https://pve:8006", "root@pam", "pam"),
            account_key("https://pve:8006", "root@pam", "pam")
        );
        // 带不带协议、带不带 API 路径都要归一到同一个键
        assert_eq!(
            account_key("https://pve:8006/api2/json", "root@pam", "pam"),
            account_key("https://pve:8006", "root@pam", "pam")
        );
        assert_eq!(account_key("https://pve:8006", "root@pam", "pam"), "root@pve:8006@pam");
        // 用户名里的 @realm 不能重复出现在键里
        assert_eq!(
            account_key("https://pve:8006", "root", "pam"),
            "root@pve:8006@pam"
        );
    }

    #[test]
    fn account_key_keeps_servers_apart() {
        // 不同服务器或认证域不能串用同一条密码
        assert_ne!(
            account_key("https://a:8006", "root@pam", "pam"),
            account_key("https://b:8006", "root@pam", "pam")
        );
        assert_ne!(
            account_key("https://pve:8006", "root@pam", "pam"),
            account_key("https://pve:8006", "root@pam", "pve")
        );
    }
}
