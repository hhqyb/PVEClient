import { useEffect, useState } from "react";

import { api } from "../api/bridge";
import type { ViewerState } from "../api/types";

interface Props {
  consoleId: string;
  viewer: string;
  reason: string;
  onReopen: () => void;
  onSwitchToVnc: () => void;
}

const POLL_MS = 2000;

/**
 * SPICE 分支：SPICE 无法内嵌，只能调外部 viewer。
 * 因此这里要清楚表达三件事：用哪个程序打开、是否还在运行、如何回退。
 */
export default function SpicePanel({
  consoleId,
  viewer,
  reason,
  onReopen,
  onSwitchToVnc,
}: Props) {
  const [state, setState] = useState<ViewerState>({ state: "running" });

  useEffect(() => {
    let cancelled = false;

    const poll = async () => {
      try {
        const next = await api.viewerStatus(consoleId);
        if (!cancelled && next) setState(next);
      } catch {
        // viewer 进程可能刚好被回收，下一次轮询会刷新状态
      }
    };

    void poll();
    const timer = setInterval(poll, POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [consoleId]);

  const exited = state.state === "exited";

  return (
    <div className="console-body">
      <div className="console-overlay">
        <div className="console-panel">
          {exited ? (
            <>
              <h2>外部查看器已退出</h2>
              <p>
                {state.state === "exited" && state.code !== null
                  ? `退出码 ${state.code}。`
                  : ""}
                如果是首次连接失败，请检查防火墙是否放通
                <code style={{ margin: "0 4px" }}>3128</code> 与
                <code style={{ margin: "0 4px" }}>61000-61999</code>
                端口。
              </p>
            </>
          ) : (
            <>
              <div className="spinner" />
              <h2>已用外部查看器打开</h2>
              <p>
                SPICE 会话在独立的窗口中运行（音频、剪贴板、USB 重定向都在那边生效）。
                关闭该窗口即结束会话。
              </p>
            </>
          )}

          <div className="alert alert-info" style={{ textAlign: "left" }}>
            {reason}
          </div>

          <p className="mono" style={{ color: "var(--text-faint)" }}>
            {viewer}
          </p>

          <div style={{ display: "flex", gap: 10, flexWrap: "wrap", justifyContent: "center" }}>
            <button className="btn btn-primary" onClick={onReopen}>
              重新打开
            </button>
            <button className="btn" onClick={onSwitchToVnc}>
              改用网页控制台
            </button>
          </div>

          <p style={{ fontSize: 12, color: "var(--text-faint)" }}>
            需要 install：Linux <code>apt install virt-viewer</code> · macOS{" "}
            <code>brew install virt-viewer</code> · Windows 从 virt-manager.org 下载
          </p>
        </div>
      </div>
    </div>
  );
}
