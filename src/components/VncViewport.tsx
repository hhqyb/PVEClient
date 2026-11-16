import { useEffect, useRef, useState } from "react";
import RFB from "@novnc/novnc";

interface Props {
  wsUrl: string;
  password: string;
  onDisconnected?: (detail: string) => void;
}

type Status = "connecting" | "connected" | "disconnected" | "error";

/**
 * noVNC 视口。
 *
 * 注意：连接目标是 Rust 侧的本地 ws 中转（127.0.0.1），不是 PVE 本身。
 * 这样可以绕开 webview 无法携带跨域 PVEAuthCookie、以及自签证书的问题，
 * 凭证也不会进入前端。
 */
export default function VncViewport({ wsUrl, password, onDisconnected }: Props) {
  const hostRef = useRef<HTMLDivElement>(null);
  const rfbRef = useRef<RFB | null>(null);
  /** 复用判定用的键：地址或密码变了才需要重建连接 */
  const keyRef = useRef<string | null>(null);
  /** 挂起的拆除定时器，允许紧随其后的重挂载取消它 */
  const pendingCloseRef = useRef<number | null>(null);
  const [status, setStatus] = useState<Status>("connecting");
  const [detail, setDetail] = useState("");

  /**
   * 回调放进 ref：父组件通常传内联箭头函数，每次渲染都是新引用。
   * 若把它放进下面 effect 的依赖数组，任何一次重渲染都会断开并重建连接，
   * 而 PVE 的 vncticket 是一次性的，重连必然失败（表现为连接立刻中断）。
   */
  const onDisconnectedRef = useRef(onDisconnected);
  onDisconnectedRef.current = onDisconnected;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    // StrictMode 会「挂载 → 卸载 → 重新挂载」。卸载时若立即断开会废掉一次性
    // vncticket，所以这里把拆除推迟一拍，重挂载时取消即可继续复用同一条连接。
    if (pendingCloseRef.current !== null) {
      window.clearTimeout(pendingCloseRef.current);
      pendingCloseRef.current = null;
    }

    const key = `${wsUrl}|${password}`;
    const reusable = rfbRef.current !== null && keyRef.current === key;

    let rfb = rfbRef.current;
    if (!reusable) {
      if (rfb) {
        try {
          rfb.disconnect();
        } catch {
          // 旧连接断开失败不影响新建
        }
      }

      setStatus("connecting");
      setDetail("");

      rfb = new RFB(host, wsUrl, {
        credentials: { password },
        wsProtocols: ["binary"],
      });
      rfbRef.current = rfb;
      keyRef.current = key;

      const instance = rfb;

      // 事件只认当前实例：被替换掉的旧实例回调一律忽略
      const isCurrent = () => rfbRef.current === instance;

      // 跟随窗口尺寸自适应，避免出现滚动条
      instance.scaleViewport = true;
      instance.resizeSession = true;
      instance.clipViewport = false;
      instance.focusOnClick = true;

      instance.addEventListener("connect", () => {
        if (!isCurrent()) return;
        setStatus("connected");
        setDetail("");
      });

      instance.addEventListener("disconnect", (e: Event) => {
        if (!isCurrent()) return;
        const ev = e as CustomEvent<{ clean: boolean }>;
        setStatus("disconnected");
        setDetail(ev.detail?.clean ? "会话已正常结束" : "连接意外中断");
        onDisconnectedRef.current?.(ev.detail?.clean ? "会话已正常结束" : "连接意外中断");
      });

      instance.addEventListener("securityfailure", (e: Event) => {
        if (!isCurrent()) return;
        const ev = e as CustomEvent<{ status?: number; reason?: string }>;
        const reason = ev.detail?.reason || "VNC 认证失败";
        setStatus("error");
        setDetail(reason);
      });

      instance.addEventListener("credentialsrequired", (e: Event) => {
        const ev = e as CustomEvent<{ types: string[] }>;
        if (ev.detail?.types?.includes("password")) {
          instance.sendCredentials({ password });
        }
      });
    }

    const current = rfb;
    if (!current) return;
    return () => {
      pendingCloseRef.current = window.setTimeout(() => {
        pendingCloseRef.current = null;
        try {
          current.disconnect();
        } catch {
          // 断开阶段的异常无需上报
        }
        if (rfbRef.current === current) {
          rfbRef.current = null;
          keyRef.current = null;
        }
      }, 150);
    };
  }, [wsUrl, password]);

  return (
    <div className="console-body">
      <div ref={hostRef} className="vnc-canvas" />

      {status !== "connected" && (
        <div className="console-overlay">
          <div className="console-panel">
            {status === "connecting" ? (
              <>
                <div className="spinner" />
                <h2>正在建立控制台连接…</h2>
                <p>通过本地安全通道中转，凭证不会暴露给前端页面</p>
              </>
            ) : (
              <>
                <h2>{status === "error" ? "控制台认证失败" : "控制台已断开"}</h2>
                <p>{detail}</p>
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
