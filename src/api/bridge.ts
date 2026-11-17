import { invoke } from "@tauri-apps/api/core";

import type {
  AppErrorPayload,
  ConsoleHandle,
  Guest,
  Realm,
  ServerProbe,
  SessionInfo,
  Settings,
  SpiceStatus,
  TaskStatus,
  ViewerState,
} from "./types";

/**
 * Tauri 的 invoke 会把 Rust 的 AppError 序列化成 {code, message}。
 * 这里统一包成可判别类型的 ConsoleError，前端据此决定提示与降级。
 */
export class ConsoleError extends Error {
  readonly code: string;
  readonly tfaChallenge?: string;

  constructor(payload: AppErrorPayload) {
    super(payload.message);
    this.name = "ConsoleError";
    this.code = payload.code;
    this.tfaChallenge = payload.tfa_challenge ?? undefined;
  }
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (raw) {
    if (raw && typeof raw === "object" && "code" in raw && "message" in raw) {
      const payload = raw as AppErrorPayload;
      // ticket 在服务端已失效：通知全局退回登录页，避免每个调用点各写一遍
      if (payload.code === "SESSION_EXPIRED") {
        window.dispatchEvent(new CustomEvent("pve-session-expired"));
      }
      throw new ConsoleError(payload);
    }
    throw new ConsoleError({
      code: "UNKNOWN",
      message: typeof raw === "string" ? raw : "未知错误",
    });
  }
}

export const api = {
  probeServer: (url: string, allowInvalidCerts?: boolean) =>
    call<ServerProbe>("probe_server", { url, allowInvalidCerts }),

  listRealms: () => call<Realm[]>("list_realms"),

  login: (params: {
    url?: string;
    username: string;
    realm: string;
    password: string;
    remember?: boolean;
    otp?: string;
    allowInvalidCerts?: boolean;
  }) => call<SessionInfo>("login", params),

  logout: () => call<void>("logout"),

  sessionInfo: () => call<SessionInfo | null>("session_info"),

  loadSavedPassword: () => call<string | null>("load_saved_password"),

  listGuests: () => call<Guest[]>("list_guests"),

  guestPower: (params: {
    vmid: number;
    node: string;
    kind: string;
    action: "start" | "shutdown" | "reboot" | "stop";
  }) => call<string>("guest_power", params),

  taskStatus: (node: string, upid: string) =>
    call<TaskStatus>("task_status", { node, upid }),

  guestIsRunning: (vmid: number, node: string, kind: string) =>
    call<boolean>("guest_is_running", { vmid, node, kind }),

  openConsole: (params: {
    vmid: number;
    node: string;
    kind: string;
    forceProtocol?: "spice" | "novnc";
  }) => call<ConsoleHandle>("open_console", params),

  closeConsole: (id: string) => call<void>("close_console", { id }),

  spiceStatus: () => call<SpiceStatus | null>("spice_status"),

  viewerStatus: (id: string) => call<ViewerState | null>("viewer_status", { id }),

  detectViewer: () => call<string | null>("detect_viewer"),

  getSettings: () => call<Settings>("get_settings"),

  saveSettings: (settings: Settings) => call<Settings>("save_settings", { settings }),
};
