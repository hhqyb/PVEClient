import { create } from "zustand";

import { api, ConsoleError } from "../api/bridge";
import type { ConsoleHandle, Guest, SessionInfo, Settings } from "../api/types";

interface SessionState {
  session: SessionInfo | null;
  guests: Guest[];
  loadingGuests: boolean;
  guestError: ConsoleError | null;

  console: ConsoleHandle | null;
  openingConsole: boolean;

  settings: Settings | null;

  /**
   * 本次登录是否已走过「单台自动连接」。
   * 放在 store 而不是组件 ref：否则用户从控制台点「返回」时组件重挂载，
   * 会立刻又被自动连走，永远回不到列表。
   */
  autoConnectedOnce: boolean;
  markAutoConnected: () => void;

  bootstrap: () => Promise<void>;
  signIn: (info: SessionInfo) => void;
  expire: () => void;
  refreshGuests: () => Promise<Guest[]>;
  openConsole: (guest: Guest, force?: "spice" | "novnc") => Promise<ConsoleHandle>;
  closeConsole: () => Promise<void>;
  signOut: () => Promise<void>;
  updateSettings: (next: Settings) => Promise<void>;
}

export const useSession = create<SessionState>((set, get) => ({
  session: null,
  guests: [],
  loadingGuests: false,
  guestError: null,
  console: null,
  openingConsole: false,
  settings: null,
  autoConnectedOnce: false,

  markAutoConnected: () => set({ autoConnectedOnce: true }),

  bootstrap: async () => {
    const [session, settings] = await Promise.all([
      api.sessionInfo().catch(() => null),
      api.getSettings().catch(() => null),
    ]);
    set({ session, settings });
  },

  signIn: (info) => set({ session: info, autoConnectedOnce: false }),

  // ticket 已失效：只清会话状态，不动 settings（用户仍需看到上次的服务器地址）
  expire: () =>
    set({
      session: null,
      guests: [],
      console: null,
      guestError: null,
      autoConnectedOnce: false,
    }),

  refreshGuests: async () => {
    set({ loadingGuests: true, guestError: null });
    try {
      const guests = await api.listGuests();
      set({ guests, loadingGuests: false });
      return guests;
    } catch (e) {
      const err = e as ConsoleError;
      set({ guestError: err, loadingGuests: false });
      throw err;
    }
  },

  openConsole: async (guest, force) => {
    set({ openingConsole: true });
    try {
      const handle = await api.openConsole({
        vmid: guest.vmid,
        node: guest.node,
        kind: guest.kind,
        forceProtocol: force,
      });
      set({ console: handle, openingConsole: false });
      return handle;
    } catch (e) {
      set({ openingConsole: false });
      throw e;
    }
  },

  closeConsole: async () => {
    const current = get().console;
    set({ console: null });
    if (current) {
      await api.closeConsole(current.id).catch(() => undefined);
    }
  },

  signOut: async () => {
    await get().closeConsole().catch(() => undefined);
    await api.logout().catch(() => undefined);
    set({
      session: null,
      guests: [],
      console: null,
      guestError: null,
      // 重新登录后应重新享受「单台自动连接」
      autoConnectedOnce: false,
    });
  },

  updateSettings: async (next) => {
    const saved = await api.saveSettings(next);
    set({ settings: saved });
  },
}));
