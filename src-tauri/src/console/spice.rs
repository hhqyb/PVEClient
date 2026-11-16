use std::path::PathBuf;

use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};

use crate::error::{AppError, AppResult};

/// 三端定位 remote-viewer / virt-viewer。
///
/// 不依赖 PATH 单一来源：macOS 上 Homebrew 的安装位置不稳定，Windows 上
/// 安装器会注册 `VirtViewer.vvfile` 文件关联，这些都要兜住。
pub fn detect_viewer() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("PVE_CLIENT_VIEWER") {
        let p = PathBuf::from(explicit);
        if p.is_file() {
            return Some(p);
        }
    }

    for candidate in platform_candidates() {
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    if let Some(found) = find_in_path(&executable_names()) {
        return Some(found);
    }

    #[cfg(target_os = "windows")]
    if let Some(found) = windows_registry_lookup() {
        return Some(found);
    }

    None
}

fn executable_names() -> Vec<&'static str> {
    #[cfg(target_os = "windows")]
    {
        vec!["remote-viewer.exe", "virt-viewer.exe"]
    }
    #[cfg(not(target_os = "windows"))]
    {
        vec!["remote-viewer", "virt-viewer"]
    }
}

fn platform_candidates() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        vec![
            PathBuf::from("/opt/homebrew/bin/remote-viewer"),
            PathBuf::from("/opt/homebrew/bin/virt-viewer"),
            PathBuf::from("/usr/local/bin/remote-viewer"),
            PathBuf::from("/usr/local/bin/virt-viewer"),
            PathBuf::from("/Applications/virt-viewer.app/Contents/MacOS/virt-viewer"),
            PathBuf::from("/Applications/VirtViewer.app/Contents/MacOS/virt-viewer"),
        ]
    }
    #[cfg(target_os = "linux")]
    {
        vec![
            PathBuf::from("/usr/bin/remote-viewer"),
            PathBuf::from("/usr/local/bin/remote-viewer"),
            PathBuf::from("/usr/bin/virt-viewer"),
            PathBuf::from("/snap/bin/remote-viewer"),
            PathBuf::from("/var/lib/flatpak/exports/bin/org.virt_manager.virt-viewer"),
        ]
    }
    #[cfg(target_os = "windows")]
    {
        let mut out = Vec::new();
        for base in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
            if let Ok(dir) = std::env::var(base) {
                let root = PathBuf::from(dir).join("VirtViewer");
                // 安装目录通常带版本号，例如 VirtViewer v11.0
                if let Ok(entries) = std::fs::read_dir(&root) {
                    for entry in entries.flatten() {
                        out.push(entry.path().join("bin").join("remote-viewer.exe"));
                        out.push(entry.path().join("bin").join("virt-viewer.exe"));
                    }
                }
                out.push(root.join("bin").join("remote-viewer.exe"));
            }
        }
        out
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Vec::new()
    }
}

fn find_in_path(names: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn windows_registry_lookup() -> Option<PathBuf> {
    use std::process::Command as StdCommand;

    // `ftype VirtViewer.vvfile` 会输出形如
    //   VirtViewer.vvfile="C:\Program Files\VirtViewer v11.0\bin\remote-viewer.exe" "%1"
    let output = StdCommand::new("cmd")
        .args(["/C", "ftype", "VirtViewer.vvfile"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let (_, right) = text.split_once('=')?;
    let exe = right.split('"').nth(1).or_else(|| right.split_whitespace().next())?;
    let p = PathBuf::from(exe.trim());
    p.is_file().then_some(p)
}

/// 启动 remote-viewer，通过 stdin 传入 .vv 配置。
///
/// 用 `-` 让 remote-viewer 从标准输入读配置，**密码不落盘**；
/// `spiceproxy` 的返回字段全部透传，避免漏掉 `host-subject` 导致 TLS 校验失败。
pub async fn launch(
    viewer: &PathBuf,
    vv: &str,
    kiosk: bool,
) -> AppResult<Child> {
    let mut cmd = Command::new(viewer);

    if kiosk {
        cmd.arg("--kiosk").arg("--kiosk-quit").arg("on-disconnect");
    } else {
        cmd.arg("--full-screen");
    }
    cmd.arg("-");
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::ViewerFailed(format!("无法启动 {}：{e}", viewer.display())))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(vv.as_bytes())
            .await
            .map_err(|e| AppError::ViewerFailed(format!("写入查看器配置失败：{e}")))?;
        stdin
            .flush()
            .await
            .map_err(|e| AppError::ViewerFailed(format!("刷新查看器输入失败：{e}")))?;
        // 关闭 stdin，让 remote-viewer 知道配置已发送完毕
        drop(stdin);
    }

    Ok(child)
}

/// 判断查看器是否在启动后立刻异常退出（配置错误 / 端口不可达的早期信号）。
pub async fn detect_early_exit(child: &mut Child, wait: std::time::Duration) -> Option<i32> {
    match tokio::time::timeout(wait, child.wait()).await {
        Ok(Ok(status)) => status.code(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_are_absolute_paths() {
        for p in platform_candidates() {
            assert!(p.is_absolute(), "{p:?} 应为绝对路径");
        }
    }
}
