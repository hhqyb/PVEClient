import { useCallback, useEffect, useRef, useState } from "react";

import { api } from "../api/bridge";
import type { SpiceStatus } from "../api/types";

const POLL_MS = 2000;
/** 鼠标在顶部热区停留多久才展开（毫秒） */
const HOVER_DELAY_MS = 2000;
/** 热区高度：贴住屏幕上沿的一条窄带，默认完全隐形 */
const HOTZONE_PX = 24;

interface Props {
  /** 切换会话的全屏/窗口模式（重开查看器） */
  onToggleFullscreen: () => void;
  /** 重新打开以应用 USB 开关变化 */
  onReopen: () => void;
  usbEnabled: boolean;
  onToggleUsb: () => void;
}

/**
 * SPICE 悬浮状态栏。
 *
 * 为什么是独立的一层而不是窗口内的条：
 * SPICE 画面由外部进程 remote-viewer 绘制，我们无法往它的窗口里注入 UI，
 * 因此状态栏只能作为客户端自己窗口里的一条悬浮层存在。
 *
 * 交互（按需求）：贴在屏幕正上方，默认隐藏；鼠标在顶部窄带停留 2 秒后
 * 向下滑出；移开鼠标自动收起。展开状态下保持显示，便于点击其中的开关。
 */
export default function SpiceStatusBar({
  onToggleFullscreen,
  onReopen,
  usbEnabled,
  onToggleUsb,
}: Props) {
  const [status, setStatus] = useState<SpiceStatus | null>(null);
  const [open, setOpen] = useState(false);
  const [hovering, setHovering] = useState(false);
  const hoverTimer = useRef<number | null>(null);

  // 轮询会话状态：查看器被用户直接关掉时，状态栏应随之消失
  useEffect(() => {
    let cancelled = false;

    const poll = async () => {
      try {
        const next = await api.spiceStatus();
        if (!cancelled) setStatus(next);
      } catch {
        // 会话切换瞬间可能查不到，下一轮会恢复
      }
    };

    void poll();
    const timer = setInterval(poll, POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, []);

  const clearHoverTimer = useCallback(() => {
    if (hoverTimer.current !== null) {
      window.clearTimeout(hoverTimer.current);
      hoverTimer.current = null;
    }
  }, []);

  // 进入热区开始计时，满 2 秒才展开；提前离开则取消，避免误触
  const enterHotzone = useCallback(() => {
    if (open || hoverTimer.current !== null) return;
    hoverTimer.current = window.setTimeout(() => {
      hoverTimer.current = null;
      setOpen(true);
    }, HOVER_DELAY_MS);
  }, [open]);

  const leaveHotzone = useCallback(() => {
    // 只有当鼠标既不在热区也不在面板上时才取消计时
    if (!hovering) clearHoverTimer();
  }, [hovering, clearHoverTimer]);

  useEffect(() => clearHoverTimer, [clearHoverTimer]);

  // 会话结束时收起，避免留下一个空条
  useEffect(() => {
    if (!status && open) setOpen(false);
  }, [status, open]);

  if (!status) return null;

  return (
    <div className={`spice-bar-layer ${open ? "is-open" : ""}`}>
      {/* 顶部热区：透明窄带，鼠标停留 2 秒触发展开 */}
      <div
        className="spice-bar-hotzone"
        style={{ height: HOTZONE_PX }}
        onMouseEnter={enterHotzone}
        onMouseLeave={leaveHotzone}
        aria-hidden="true"
      >
        {/* 收起时给一个极淡的视觉提示，否则用户永远不知道这里有东西 */}
        <span className="spice-bar-hint" />
      </div>

      <div
        className="spice-bar"
        onMouseEnter={() => setHovering(true)}
        onMouseLeave={() => setHovering(false)}
        role="status"
        aria-live="polite"
      >
        <div className="spice-bar-inner">
          <div className="spice-bar-group">
            <span className="spice-bar-dot" aria-hidden="true" />
            <span className="spice-bar-name">{status.guest_name}</span>
            <span className="spice-bar-meta">
              VMID {status.vmid} · {status.node}
            </span>
          </div>

          <span className="spice-bar-sep" aria-hidden="true" />

          <div className="spice-bar-group">
            <span className="spice-bar-label">网络</span>
            <span className="spice-bar-value spice-bar-ok">已连接</span>
            <span className="spice-bar-meta">{status.protocol}</span>
          </div>

          <span className="spice-bar-sep" aria-hidden="true" />

          <button
            type="button"
            className="spice-bar-btn"
            onClick={onToggleUsb}
            title="USB 透传需要重开会话才生效"
          >
            <span className="spice-bar-label">USB 透传</span>
            <span
              className={`spice-bar-value ${usbEnabled ? "spice-bar-ok" : "spice-bar-off"}`}
            >
              {usbEnabled ? "已启用" : "已关闭"}
            </span>
          </button>

          <span className="spice-bar-sep" aria-hidden="true" />

          <button type="button" className="spice-bar-btn" onClick={onToggleFullscreen}>
            <span className="spice-bar-label">显示</span>
            <span className="spice-bar-value">
              {status.fullscreen ? "全屏" : "窗口"}
            </span>
          </button>

          <span className="spice-bar-sep" aria-hidden="true" />

          <button type="button" className="spice-bar-btn" onClick={onReopen}>
            重新连接
          </button>
        </div>
      </div>
    </div>
  );
}
