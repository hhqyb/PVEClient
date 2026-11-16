import { useCallback, useEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";

import { api, ConsoleError } from "../api/bridge";
import type { Guest } from "../api/types";
import ConsoleToolbar from "../components/ConsoleToolbar";
import SpicePanel from "../components/SpicePanel";
import VncViewport from "../components/VncViewport";
import { useSession } from "../store/session";

export default function ConsolePage() {
  const navigate = useNavigate();
  const location = useLocation();
  const { console: handle, openConsole, closeConsole, guests } = useSession();

  const [error, setError] = useState<string | null>(null);
  const [switching, setSwitching] = useState(false);

  // 从列表页带过来；直接刷新页面时退回 store 里找
  const initialGuest = (location.state as { guest?: Guest } | null)?.guest;
  const guest =
    initialGuest ?? guests.find((g) => g.vmid === handle?.vmid) ?? null;

  const connectedOnce = useRef(false);

  // 直接进入 /console（例如页面刷新）时补一次打开动作
  useEffect(() => {
    if (handle || !guest || connectedOnce.current) return;
    connectedOnce.current = true;
    void openConsole(guest).catch((e) => setError((e as ConsoleError).message));
  }, [handle, guest, openConsole]);

  const switchProtocol = useCallback(
    async (target: "spice" | "novnc") => {
      if (!guest) return;
      setSwitching(true);
      setError(null);
      try {
        await closeConsole();
        await openConsole(guest, target);
      } catch (e) {
        setError((e as ConsoleError).message);
      } finally {
        setSwitching(false);
      }
    },
    [guest, closeConsole, openConsole],
  );

  const reopen = useCallback(async () => {
    if (!guest) return;
    setError(null);
    try {
      await closeConsole();
      await openConsole(guest, "spice");
    } catch (e) {
      setError((e as ConsoleError).message);
    }
  }, [guest, closeConsole, openConsole]);

  const goBack = useCallback(async () => {
    await closeConsole().catch(() => undefined);
    navigate("/guests");
  }, [closeConsole, navigate]);

  // 确保离开控制台时回收外部进程（避免 remote-viewer 变僵尸、PVE 侧会话堆积）：
  // - 路由跳转：goBack / switchProtocol 已显式关闭
  // - 窗口关闭：beforeunload
  //
  // 注意：这里**不能**在组件卸载时关闭控制台。React 在开发模式下用 StrictMode
  // 故意执行「挂载 → 卸载 → 重新挂载」，若卸载即关闭，刚打开的控制台会被立刻拆掉：
  // SPICE 表现为窗口闪一下就消失，noVNC 表现为本地中转 nonce 已注销而连不上（404）。
  // 控制台句柄保存在 store 里，跨路由与重挂载都有效，由 goBack / signOut 负责收尾。
  useEffect(() => {
    const cleanup = () => {
      const current = useSession.getState().console;
      if (current) void api.closeConsole(current.id).catch(() => undefined);
    };
    window.addEventListener("beforeunload", cleanup);
    return () => window.removeEventListener("beforeunload", cleanup);
  }, []);

  if (!guest) {
    return (
      <div className="page">
        <ConsoleToolbar title="控制台" onBack={() => navigate("/guests")} />
        <div className="center-page">
          <div className="empty-state">
            <h2>没有可显示的控制台</h2>
            <p>请从列表中选择一台虚拟机。</p>
            <button className="btn btn-primary" onClick={() => navigate("/guests")}>
              返回列表
            </button>
          </div>
        </div>
      </div>
    );
  }

  const title = `${guest.name} · VMID ${guest.vmid}`;
  const subtitle = `${guest.node} · ${guest.kind === "qemu" ? "QEMU" : "LXC"}`;

  return (
    <div className="page">
      <ConsoleToolbar
        title={title}
        subtitle={subtitle}
        onBack={() => void goBack()}
        onSwitchProtocol={
          handle
            ? () =>
                void switchProtocol(handle.protocol === "spice" ? "novnc" : "spice")
            : undefined
        }
        switchLabel={
          handle?.protocol === "spice" ? "改用网页控制台" : "尝试 SPICE"
        }
        extra={
          handle && (
            <span
              className={`badge ${handle.protocol === "spice" ? "badge-spice" : "badge-vnc"}`}
            >
              {handle.protocol === "spice" ? "SPICE" : "VNC"}
            </span>
          )
        }
      />

      {error && (
        <div style={{ padding: "12px 20px 0" }}>
          <div className="alert alert-error">{error}</div>
        </div>
      )}

      {switching && (
        <div style={{ padding: "12px 20px 0" }}>
          <div className="alert alert-info">正在切换控制台协议…</div>
        </div>
      )}

      {!handle && !error && (
        <div className="console-body">
          <div className="console-overlay">
            <div className="console-panel">
              <div className="spinner" />
              <h2>正在准备控制台…</h2>
              <p>优先尝试 SPICE，不可用时自动降级为网页控制台</p>
            </div>
          </div>
        </div>
      )}

      {handle?.protocol === "spice" && (
        <SpicePanel
          consoleId={handle.id}
          viewer={handle.viewer}
          reason={handle.reason}
          onReopen={() => void reopen()}
          onSwitchToVnc={() => void switchProtocol("novnc")}
        />
      )}

      {handle?.protocol === "vnc" && (
        <>
          {handle.reason && (
            <div style={{ padding: "12px 20px 0" }}>
              <div className="alert alert-info">{handle.reason}</div>
            </div>
          )}
          <VncViewport
            wsUrl={handle.ws_url}
            password={handle.password}
            onDisconnected={(detail) => setError(detail)}
          />
        </>
      )}
    </div>
  );
}
