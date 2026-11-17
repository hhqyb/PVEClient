import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";

import { api, ConsoleError } from "../api/bridge";
import type { Realm, Settings } from "../api/types";
import { useSession } from "../store/session";
import { APP_VERSION } from "../version";

type Phase = "idle" | "probing" | "ready" | "submitting";

export default function LoginPage() {
  const navigate = useNavigate();
  const { settings, signIn } = useSession();

  const [server, setServer] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [realm, setRealm] = useState("");
  const [remember, setRemember] = useState(false);
  const [allowInvalid, setAllowInvalid] = useState(true);

  const [realms, setRealms] = useState<Realm[]>([]);
  const [version, setVersion] = useState("");
  const [phase, setPhase] = useState<Phase>("idle");
  const [error, setError] = useState<ConsoleError | null>(null);
  const [otp, setOtp] = useState("");
  const [needTfa, setNeedTfa] = useState(false);
  const [hint, setHint] = useState("");
  const [certWarning, setCertWarning] = useState<string | null>(null);

  // 用已保存的设置预填，但绝不预填密码（密码只从钥匙串按需读取）
  useEffect(() => {
    if (!settings) return;
    const server = settings.server;
    const savedRealm = settings.realm;
    // 用户名与认证域分开显示：保存的 username 带 @realm，输入框只放纯用户名，
    // 认证域由下拉框决定（PVE 不接受裸用户名，两者都是必填）
    const [userOnly] = settings.username.split("@");
    setServer((prev) => prev || server);
    setUsername((prev) => prev || userOnly);
    setRealm((prev) => prev || savedRealm);
    setRemember(settings.remember_password);
    setAllowInvalid(settings.allow_invalid_certs);

    // 自动探活：认证域下拉框需要 realm 列表，否则用户面对空列表无从选择
    if (server.trim()) {
      void probe(server.trim(), settings.allow_invalid_certs, savedRealm);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings]);

  useEffect(() => {
    if (!settings?.remember_password) return;
    let cancelled = false;
    api
      .loadSavedPassword()
      .then((pw) => {
        if (!cancelled && pw) setPassword(pw);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [settings?.remember_password, settings?.username]);

  async function probe(target: string, allowInvalidCerts: boolean, keepRealm?: string) {
    setError(null);
    setCertWarning(null);
    setPhase("probing");
    try {
      const result = await api.probeServer(target, allowInvalidCerts);
      setRealms(result.realms);
      setVersion(result.version);
      if (result.fingerprint_warning) {
        setCertWarning(result.fingerprint_warning);
      }

      // 尽量保留用户/设置里已有的选择，其次选 PVE 标记的 default 域
      setRealm((prev) => {
        const wanted = keepRealm || prev;
        if (wanted && result.realms.some((r) => r.realm === wanted)) {
          return wanted;
        }
        const preferred = result.realms.find((r) => (r.default ?? 0) === 1);
        if (preferred) return preferred.realm;
        if (result.realms.length === 1) return result.realms[0].realm;
        return wanted || "";
      });

      setPhase("ready");
      setHint(
        result.realms.length === 0
          ? "该服务器没有可用的认证域，请先在 PVE 上配置 AD / LDAP realm"
          : `已连接 Proxmox VE ${result.version}，共 ${result.realms.length} 个认证域`,
      );
    } catch (e) {
      setPhase("idle");
      setError(e as ConsoleError);
    }
  }

  async function handleProbe(e?: React.FormEvent) {
    e?.preventDefault();
    if (!server.trim()) {
      setError(new ConsoleError({ code: "INVALID_URL", message: "请填写服务器地址" }));
      return;
    }
    setHint("");
    await probe(server.trim(), allowInvalid, realm);
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setPhase("submitting");
    try {
      const info = await api.login({
        url: server.trim(),
        username: username.trim(),
        realm,
        password,
        remember,
        otp: needTfa && otp.trim() ? otp.trim() : undefined,
        allowInvalidCerts: allowInvalid,
      });
      signIn(info);
      navigate("/guests", { replace: true });
    } catch (e) {
      const err = e as ConsoleError;
      if (err.code === "NEED_TFA") {
        setNeedTfa(true);
        setHint("该账号启用了双因素认证，请输入验证码后再次提交");
      } else {
        setError(err);
      }
      setPhase("ready");
    }
  }

  const busy = phase === "probing" || phase === "submitting";
  const probed = phase === "ready" || phase === "submitting";

  return (
    <div className="center-page">
      <form className="login-card" onSubmit={handleSubmit}>
        <div>
          <h1 className="login-title">PVE 云桌面</h1>
          <p className="login-subtitle">
            使用你的 AD / LDAP 账号登录，只显示你有权限的虚拟机
          </p>
        </div>

        <div className="field">
          <label htmlFor="server">服务器地址</label>
          <input
            id="server"
            className="input"
            placeholder="https://100.98.129.60:8006"
            value={server}
            onChange={(e) => setServer(e.target.value)}
            onBlur={() => {
              if (server.trim() && !probed && !busy) void handleProbe();
            }}
            disabled={busy}
            autoComplete="url"
            spellCheck={false}
          />
        </div>

        <div className="field">
          <label htmlFor="realm">认证域</label>
          {/* 认证域是 PVE 认证的必填项（PVE 不接受裸用户名），因此始终显示。
              探测成功时用下拉框避免拼错；探测不到时退回文本框，保证仍能手动登录。 */}
          {realms.length > 0 ? (
            <select
              id="realm"
              className="select"
              value={realm}
              onChange={(e) => setRealm(e.target.value)}
              disabled={busy}
            >
              {realms.map((r) => (
                <option key={r.realm} value={r.realm}>
                  {r.realm} ({r.type})
                  {r.comment ? ` — ${r.comment}` : ""}
                </option>
              ))}
            </select>
          ) : (
            <input
              id="realm"
              className="input"
              placeholder="pam"
              value={realm}
              onChange={(e) => setRealm(e.target.value)}
              disabled={busy}
              spellCheck={false}
            />
          )}
          <p className="field-hint">
            {realms.length > 0
              ? "AD / LDAP / PAM 账号选各自的域，登录时会自动补全为"
              : "未能读取认证域列表，可直接填写（常见值：pam、pve 或你的 AD 域）。登录用户名会自动补全为"}
            <code style={{ margin: "0 3px" }}>
              {username.trim() || "用户名"}@{realm || "域"}
            </code>
          </p>
        </div>

        <div className="field">
          <label htmlFor="username">用户名</label>
          <input
            id="username"
            className="input"
            placeholder="youbin"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            disabled={busy}
            autoComplete="username"
            spellCheck={false}
          />
          <p className="field-hint">
            只填用户名部分，认证域由上方下拉框决定
          </p>
        </div>

        <div className="field">
          <label htmlFor="password">密码</label>
          <input
            id="password"
            className="input"
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            disabled={busy}
            autoComplete="current-password"
          />
        </div>

        {needTfa && (
          <div className="field">
            <label htmlFor="otp">双因素验证码</label>
            <input
              id="otp"
              className="input"
              inputMode="numeric"
              placeholder="6 位验证码"
              value={otp}
              onChange={(e) => setOtp(e.target.value)}
              disabled={busy}
              autoComplete="one-time-code"
            />
          </div>
        )}

        <label className="checkbox">
          <input
            type="checkbox"
            checked={remember}
            onChange={(e) => setRemember(e.target.checked)}
            disabled={busy}
          />
          记住密码（存入系统钥匙串）
        </label>

        <label className="checkbox">
          <input
            type="checkbox"
            checked={allowInvalid}
            onChange={(e) => setAllowInvalid(e.target.checked)}
            disabled={busy}
          />
          允许无效 / 自签证书
        </label>

        {!allowInvalid && (
          <div className="alert alert-warn">
            PVE 默认使用自签证书。关闭此项后若无法连接，请先为 PVE 配置受信任的证书。
          </div>
        )}

        {hint && !error && <div className="alert alert-info">{hint}</div>}

        {certWarning && (
          <div className="alert alert-warn">{certWarning}</div>
        )}

        {error && (
          <div className="alert alert-error">
            <div>{error.message}</div>
          </div>
        )}

        <div style={{ display: "flex", gap: 10 }}>
          <button
            type="button"
            className="btn"
            onClick={() => void handleProbe()}
            disabled={busy || !server.trim()}
          >
            {phase === "probing" ? "连接中…" : "测试连接"}
          </button>
          <button
            type="submit"
            className="btn btn-primary"
            style={{ flex: 1 }}
            disabled={busy || !username.trim() || !server.trim()}
          >
            {phase === "submitting" ? "登录中…" : "登录"}
          </button>
        </div>

        {version && (
          <div className="topbar-meta" style={{ textAlign: "center" }}>
            Proxmox VE {version}
          </div>
        )}

        <div className="login-version">{APP_VERSION}</div>
      </form>
    </div>
  );
}

export type { Settings };
