import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";

import { api } from "../api/bridge";
import type { Settings } from "../api/types";
import { useSession } from "../store/session";
import { APP_VERSION } from "../version";

export default function SettingsPage() {
  const navigate = useNavigate();
  const { settings, updateSettings, session } = useSession();

  const [draft, setDraft] = useState<Settings | null>(settings);
  const [viewerPath, setViewerPath] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!draft && settings) setDraft(settings);
  }, [settings, draft]);

  useEffect(() => {
    api
      .detectViewer()
      .then(setViewerPath)
      .catch(() => setViewerPath(null));
  }, []);

  if (!draft) {
    return (
      <div className="page">
        <header className="topbar">
          <button className="btn btn-ghost" onClick={() => navigate(-1)}>
            ← 返回
          </button>
          <span className="topbar-title">设置</span>
        </header>
      </div>
    );
  }

  const patch = (part: Partial<Settings>) => {
    setDraft({ ...draft, ...part });
    setSaved(false);
  };

  async function handleSave() {
    if (!draft) return;
    setError(null);
    try {
      await updateSettings(draft);
      setSaved(true);
    } catch (e) {
      setError((e as Error).message);
    }
  }

  return (
    <div className="page">
      <header className="topbar">
        <button className="btn btn-ghost" onClick={() => navigate(-1)}>
          ← 返回
        </button>
        <span className="topbar-title">设置</span>
        <span className="spacer" />
        <button className="btn btn-primary" onClick={() => void handleSave()}>
          保存
        </button>
      </header>

      <div className="settings-body">
        {saved && <div className="alert alert-info">设置已保存。协议与证书策略在下次登录时生效。</div>}
        {error && <div className="alert alert-error">{error}</div>}

        <section className="settings-section">
          <h3>服务器</h3>
          <div className="field">
            <label htmlFor="server">地址</label>
            <input
              id="server"
              className="input"
              value={draft.server}
              onChange={(e) => patch({ server: e.target.value })}
              placeholder="https://100.98.129.60:8006"
              spellCheck={false}
            />
          </div>
          <div className="field">
            <label htmlFor="username">用户名</label>
            <input
              id="username"
              className="input"
              value={draft.username}
              onChange={(e) => patch({ username: e.target.value })}
              spellCheck={false}
            />
          </div>
          {session && (
            <p className="settings-note">
              当前会话：{session.username} · PVE {session.version || "未知"}
            </p>
          )}
        </section>

        <section className="settings-section">
          <h3>控制台协议</h3>
          <div className="field">
            <label htmlFor="protocol">首选协议</label>
            <select
              id="protocol"
              className="select"
              value={draft.protocol}
              onChange={(e) =>
                patch({ protocol: e.target.value as Settings["protocol"] })
              }
            >
              <option value="auto">自动（SPICE 优先，不可用时降级）</option>
              <option value="spice">始终尝试 SPICE</option>
              <option value="novnc">始终使用网页控制台</option>
            </select>
          </div>
          <p className="settings-note">
            SPICE 只有虚拟机 <code>vga</code> 配置为 <code>qxl</code> 时才可用，
            且需要客户端安装 remote-viewer。SPICE 依赖
            <code style={{ margin: "0 3px" }}>3128</code> 与
            <code style={{ margin: "0 3px" }}>61000-61999</code> 端口。
          </p>

          <label className="checkbox">
            <input
              type="checkbox"
              checked={draft.spice_kiosk}
              onChange={(e) => patch({ spice_kiosk: e.target.checked })}
            />
            SPICE 以全屏托管（kiosk）模式打开
          </label>

          <div className="field">
            <label htmlFor="proxy">SPICE 代理覆盖（可选）</label>
            <input
              id="proxy"
              className="input"
              value={draft.spice_proxy_override ?? ""}
              onChange={(e) =>
                patch({ spice_proxy_override: e.target.value || null })
              }
              placeholder="100.98.129.60:3128"
              spellCheck={false}
            />
            <p className="settings-note">
              PVE 返回的 proxy 主机名在客户端无法解析时，在这里填写可访问的
              地址。
            </p>
          </div>

          <div className="field">
            <label htmlFor="viewer">remote-viewer 路径</label>
            <input
              id="viewer"
              className="input"
              value={draft.viewer_path ?? ""}
              onChange={(e) => patch({ viewer_path: e.target.value || null })}
              placeholder={viewerPath ?? "自动探测"}
              spellCheck={false}
            />
            <p className="settings-note">
              自动探测结果：
              <span className="mono">{viewerPath ?? "未找到"}</span>
            </p>
          </div>
        </section>

        <section className="settings-section">
          <h3>安全</h3>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={draft.allow_invalid_certs}
              onChange={(e) => patch({ allow_invalid_certs: e.target.checked })}
            />
            允许无效 / 自签证书
          </label>
          <p className="settings-note">
            PVE 默认使用自签证书，内网场景通常需要开启。关闭后连接自签 PVE
            会失败。开启时存在中间人风险，建议在可信网络中使
            用。
          </p>

          <label className="checkbox">
            <input
              type="checkbox"
              checked={draft.remember_password}
              onChange={(e) => patch({ remember_password: e.target.checked })}
            />
            记住密码（保存到系统钥匙串）
          </label>
          <p className="settings-note">
            密码只存储在操作系统凭据管理器（Windows 凭据管理器 / macOS
            钥匙串 / Linux libsecret），不会写入配置文件。
          </p>

          {draft.last_fingerprint && (
            <div className="field">
              <label>上次记录的服务器证书指纹</label>
              <div className="mono">{draft.last_fingerprint}</div>
            </div>
          )}
        </section>

        <section className="settings-section">
          <h3>关于</h3>
          <p className="settings-note">
            PVE 云桌面客户端 · 登录后只显示当前账号有权限的虚拟机。
            认证（AD / LDAP）与权限完全由 Proxmox VE 后端负责，
            客户端不直接连接目录服务。
          </p>
          <div className="settings-meta">
            <span>版本</span>
            <span className="mono">{APP_VERSION}</span>
          </div>
        </section>
      </div>
    </div>
  );
}
