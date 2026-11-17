/**
 * 与 Rust 侧 models.rs 一一对应的类型定义。
 * 改动这里时务必同步 `src-tauri/src/pve/models.rs`。
 */

export type Protocol = "spice" | "vnc";

export interface Realm {
  realm: string;
  type: string;
  comment?: string | null;
  default?: number | null;
  tfa?: string | null;
}

export interface ServerProbe {
  version: string;
  fingerprint: string | null;
  /** 证书指纹与上次记录不一致时的告警（不阻断登录） */
  fingerprint_warning: string | null;
  realms: Realm[];
}

export interface SessionInfo {
  username: string;
  realm: string;
  host: string;
  version: string;
  expires_at: number;
}

export interface Guest {
  vmid: number;
  node: string;
  kind: "qemu" | "lxc";
  name: string;
  status: string;
  pool: string | null;
  tags: string[];
  cpu: number | null;
  maxcpu: number | null;
  mem: number | null;
  maxmem: number | null;
  uptime: number | null;
  lock: string | null;
  hastate: string | null;
  /** 是否可用 SPICE 打开（qemu 且 vga=qxl*） */
  spice_capable: boolean;
  /** 原始 vga 配置，用于提示与排障 */
  vga: string | null;
}

export interface TaskStatus {
  status: string;
  exitstatus: string | null;
  node: string | null;
}

export type ConsoleHandle =
  | {
      protocol: "spice";
      id: string;
      vmid: number;
      viewer: string;
      reason: string;
    }
  | {
      protocol: "vnc";
      id: string;
      vmid: number;
      ws_url: string;
      password: string;
      reason: string;
    };

export type ViewerState =
  | { state: "running" }
  | { state: "exited"; code: number | null };

export interface Settings {
  server: string;
  username: string;
  realm: string;
  remember_password: boolean;
  allow_invalid_certs: boolean;
  protocol: "auto" | "spice" | "novnc";
  spice_proxy_override: string | null;
  viewer_path: string | null;
  spice_kiosk: boolean;
  spice_fullscreen: boolean;
  spice_usb_redirect: boolean;
  last_fingerprint: string | null;
  server_version: string | null;
}

/** SPICE 会话的状态栏数据（来自 Rust 侧 spice_status 命令） */
export interface SpiceStatus {
  console_id: string;
  vmid: number;
  guest_name: string;
  node: string;
  protocol: string;
  usb_redirect: boolean;
  fullscreen: boolean;
}

/** Rust 侧 AppError 的序列化形式 */
export interface AppErrorPayload {
  code: string;
  message: string;
  tfa_challenge?: string | null;
}
