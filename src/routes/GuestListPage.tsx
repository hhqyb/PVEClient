import { useCallback, useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";

import { api, ConsoleError } from "../api/bridge";
import type { Guest } from "../api/types";
import EmptyState from "../components/EmptyState";
import GuestCard from "../components/GuestCard";
import { useSession } from "../store/session";
import { APP_VERSION } from "../version";

const POLL_INTERVAL_MS = 15_000;

export default function GuestListPage() {
  const navigate = useNavigate();
  const {
    session,
    guests,
    refreshGuests,
    openConsole,
    openingConsole,
    guestError,
    signOut,
    markAutoConnected,
  } = useSession();

  const [busyVmid, setBusyVmid] = useState<number | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const initialLoad = useRef(true);

  const goConnect = useCallback(
    async (guest: Guest) => {
      setActionError(null);
      try {
        // 未运行就先开机，等任务完成再连
        if (guest.status !== "running") {
          setNotice("正在启动虚拟机…");
          const upid = await api.guestPower({
            vmid: guest.vmid,
            node: guest.node,
            kind: guest.kind,
            action: "start",
          });
          await waitForTask(guest.node, upid);
          // 任务 OK 只代表 PVE 接受了操作；用 status/current 探测真正的运行状态，
          // 因为列表数据源（cluster/resources）有缓存延迟，否则会误报「未运行」
          await waitUntilRunning(guest.node, guest.kind, guest.vmid);
          setNotice(null);
        }
        await openConsole(guest);
        navigate("/console", { state: { guest } });
      } catch (e) {
        setNotice(null);
        const err = e as ConsoleError;
        setActionError(err.message);
      } finally {
        // 无论成功与否都同步一次列表，让界面状态与 PVE 真实状态一致
        await refreshGuests().catch(() => undefined);
      }
    },
    [navigate, openConsole, refreshGuests],
  );

  const handlePower = useCallback(
    async (guest: Guest, action: "start" | "shutdown" | "reboot") => {
      setBusyVmid(guest.vmid);
      setActionError(null);
      try {
        const upid = await api.guestPower({
          vmid: guest.vmid,
          node: guest.node,
          kind: guest.kind,
          action,
        });
        const labels = { start: "开机", shutdown: "关机", reboot: "重启" } as const;
        setNotice(`正在${labels[action]} ${guest.name}…`);
        await waitForTask(guest.node, upid);
        setNotice(null);
        await refreshGuests();
      } catch (e) {
        setNotice(null);
        setActionError((e as ConsoleError).message);
      } finally {
        setBusyVmid(null);
      }
    },
    [refreshGuests],
  );

  // 初次加载 + 轮询
  useEffect(() => {
    if (!session) {
      navigate("/login", { replace: true });
      return;
    }

    let cancelled = false;

    const load = async () => {
      try {
        const list = await refreshGuests();
        if (cancelled) return;

        // 只有一台 → 登录后直接连接（需求核心分支）。
        // 用 store 里的标记而非组件 ref，确保用户从控制台返回时不会被再次自动连走。
        if (!useSession.getState().autoConnectedOnce && list.length === 1) {
          markAutoConnected();
          void goConnect(list[0]);
        }
      } catch {
        // 错误已存入 store，由界面呈现
      } finally {
        initialLoad.current = false;
      }
    };

    void load();
    const timer = setInterval(() => {
      // 已进入控制台时不再轮询列表
      if (useSession.getState().console) return;
      void refreshGuests().catch(() => undefined);
    }, POLL_INTERVAL_MS);

    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [session, navigate, refreshGuests, goConnect, markAutoConnected]);

  if (!session) return null;

  const loading = initialLoad.current && guests.length === 0 && !guestError;

  return (
    <div className="page">
      <header className="topbar">
        <span className="topbar-title">我的云桌面</span>
        <span className="topbar-meta">
          {session.username} @ {session.host}
          {session.version ? ` · PVE ${session.version}` : ""}
        </span>
        <span className="app-version">{APP_VERSION}</span>
        <span className="spacer" />
        <button className="btn btn-ghost" onClick={() => void refreshGuests()}>
          刷新
        </button>
        <button className="btn btn-ghost" onClick={() => navigate("/settings")}>
          设置
        </button>
        <button
          className="btn btn-ghost btn-danger"
          onClick={async () => {
            await signOut();
            navigate("/login", { replace: true });
          }}
        >
          退出登录
        </button>
      </header>

      {notice && (
        <div style={{ padding: "12px 20px 0" }}>
          <div className="alert alert-info">{notice}</div>
        </div>
      )}

      {actionError && (
        <div style={{ padding: "12px 20px 0" }}>
          <div className="alert alert-error">{actionError}</div>
        </div>
      )}

      {loading && (
        <div className="center-page">
          <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 14 }}>
            <div className="spinner" />
            <span style={{ color: "var(--text-dim)", fontSize: 13 }}>正在读取可用虚拟机…</span>
          </div>
        </div>
      )}

      {!loading && guestError && (
        <EmptyState
          title="无法读取虚拟机列表"
          description={guestError.message}
          action={
            <button className="btn btn-primary" onClick={() => void refreshGuests()}>
              重试
            </button>
          }
        />
      )}

      {!loading && !guestError && guests.length === 0 && (
        <EmptyState
          title="你还没有可用的虚拟机"
          description="当前账号能访问的虚拟机为空。这通常意味着管理员还没有为你分配权限。"
          hint={
            <>
              需要管理员在 PVE 上授予 <code>VM.Audit</code> 与 <code>VM.Console</code>：
              <br />
              <code>
                pveum acl modify /pool/&lt;池名&gt; --groups &lt;AD组&gt;-&lt;realm&gt;
                --roles PVEVMUser
              </code>
              <br />
              <br />
              另外请确认该 AD / LDAP 用户已存在于 PVE 的用户表中（<code>pveum user add</code> 或通过
              realm sync 同步）。
            </>
          }
          action={
            <button className="btn btn-primary" onClick={() => void refreshGuests()}>
              刷新
            </button>
          }
        />
      )}

      {!loading && !guestError && guests.length > 0 && (
        <div className="guest-grid">
          {guests.map((guest) => (
            <GuestCard
              key={`${guest.node}-${guest.kind}-${guest.vmid}`}
              guest={guest}
              busy={busyVmid === guest.vmid || openingConsole}
              onOpen={(g) => void goConnect(g)}
              onPower={(action) => void handlePower(guest, action)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * 轮询 PVE 任务直到结束。电源操作是异步 worker，
 * 必须等 exitstatus 才能判断真的成功了。
 */
async function waitForTask(
  node: string,
  upid: string,
  timeoutMs = 120_000,
): Promise<void> {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const status = await api.taskStatus(node, upid).catch(() => null);
    if (status && status.status === "stopped") {
      if (status.exitstatus && status.exitstatus !== "OK") {
        throw new ConsoleError({
          code: "TASK_FAILED",
          message: `PVE 任务失败：${status.exitstatus}`,
        });
      }
      return;
    }
    await new Promise((r) => setTimeout(r, 1000));
  }
  throw new ConsoleError({ code: "TASK_TIMEOUT", message: "等待 PVE 任务超时" });
}

/**
 * 等待虚拟机真正进入 running。
 *
 * `qmstart` 任务返回 OK 时，虚拟机可能还在初始化，而列表用的
 * `/cluster/resources` 更是集群状态缓存，会比真实状态晚一拍。
 * 这里直接用 `status/current` 查询真实状态，避免「刚开机却判定未运行」。
 */
async function waitUntilRunning(
  node: string,
  kind: string,
  vmid: number,
  timeoutMs = 90_000,
): Promise<void> {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const running = await api
      .guestIsRunning(vmid, node, kind)
      .catch(() => false);
    if (running) return;
    await new Promise((r) => setTimeout(r, 1000));
  }
  throw new ConsoleError({
    code: "GUEST_NOT_RUNNING",
    message: "虚拟机启动超时，请稍后在列表中刷新确认状态",
  });
}
