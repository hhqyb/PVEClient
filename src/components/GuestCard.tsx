import type { Guest } from "../api/types";
import StatusDot from "./StatusDot";

interface Props {
  guest: Guest;
  onOpen: (guest: Guest) => void;
  onPower: (action: "start" | "shutdown" | "reboot") => void;
  busy: boolean;
}

function formatMem(bytes: number | null): string {
  if (!bytes) return "—";
  const gb = bytes / 1024 / 1024 / 1024;
  return gb >= 1 ? `${gb.toFixed(1)} GB` : `${Math.round(bytes / 1024 / 1024)} MB`;
}

function formatCpu(cpu: number | null, maxcpu: number | null): string {
  if (cpu === null || cpu === undefined) return "—";
  const pct = (cpu * 100).toFixed(0);
  return maxcpu ? `${pct}% / ${maxcpu} vCPU` : `${pct}%`;
}

export default function GuestCard({ guest, onOpen, onPower, busy }: Props) {
  const running = guest.status === "running";

  return (
    <div
      className="guest-card"
      role="button"
      tabIndex={0}
      onClick={() => onOpen(guest)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onOpen(guest);
        }
      }}
    >
      <div className="guest-head">
        <StatusDot running={running} />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div className="guest-name">{guest.name}</div>
          <div className="guest-id">
            VMID {guest.vmid} · {guest.node}
            {guest.pool ? ` · ${guest.pool}` : ""}
          </div>
        </div>
      </div>

      <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
        <span className="badge badge-type">
          {guest.kind === "qemu" ? "QEMU" : "LXC"}
        </span>
        {guest.spice_capable ? (
          <span className="badge badge-spice" title={`vga=${guest.vga ?? "qxl"}`}>
            SPICE
          </span>
        ) : (
          <span className="badge badge-vnc" title={guest.vga ? `vga=${guest.vga}` : "未配置 vga"}>
            VNC
          </span>
        )}
        {guest.lock && <span className="badge badge-vnc">{guest.lock}</span>}
        {guest.hastate && <span className="badge badge-vnc">HA {guest.hastate}</span>}
        {guest.tags.map((tag) => (
          <span key={tag} className="badge badge-vnc">
            {tag}
          </span>
        ))}
      </div>

      {running && (
        <div className="guest-stats">
          <span>CPU {formatCpu(guest.cpu, guest.maxcpu)}</span>
          <span>内存 {formatMem(guest.mem)}</span>
        </div>
      )}

      <div
        className="guest-actions"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.stopPropagation()}
      >
        <button
          className="btn btn-ghost"
          style={{ flex: 1 }}
          disabled={busy}
          onClick={() => onOpen(guest)}
        >
          {running ? "连接" : "开机并连接"}
        </button>
        {running ? (
          <>
            <button
              className="btn btn-ghost"
              disabled={busy}
              onClick={() => onPower("reboot")}
              title="重启虚拟机"
            >
              重启
            </button>
            <button
              className="btn btn-ghost btn-danger"
              disabled={busy}
              onClick={() => onPower("shutdown")}
              title="正常关机"
            >
              关机
            </button>
          </>
        ) : (
          <button
            className="btn btn-ghost"
            disabled={busy}
            onClick={() => onPower("start")}
            title="开机"
          >
            开机
          </button>
        )}
      </div>
    </div>
  );
}
