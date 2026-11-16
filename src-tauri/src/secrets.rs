use crate::error::{AppError, AppResult};

const SERVICE: &str = "dev.pveclient.desktop";

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
