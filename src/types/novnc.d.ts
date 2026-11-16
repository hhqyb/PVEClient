/**
 * @novnc/novnc 没有随包提供类型声明，且 package.json 的 exports
 * 只暴露包根（"./core/rfb.js"）。这里给出最小可用的类型定义，
 * 只覆盖本项目实际用到的 API，避免用 any 掩盖真实错误。
 */
declare module "@novnc/novnc" {
  export interface RFBCredentials {
    username?: string;
    password?: string;
    target?: string;
  }

  export interface RFBOptions {
    credentials?: RFBCredentials;
    shared?: boolean;
    repeaterID?: string;
    wsProtocols?: string[];
  }

  export default class RFB extends EventTarget {
    constructor(target: HTMLElement, url: string, options?: RFBOptions);

    scaleViewport: boolean;
    resizeSession: boolean;
    clipViewport: boolean;
    focusOnClick: boolean;
    viewOnly: boolean;
    background: string;

    disconnect(): void;
    sendCredentials(credentials: RFBCredentials): void;
    sendCtrlAltDel(): void;
    focus(): void;
    blur(): void;
    machineShutdown(): void;
    machineReboot(): void;
    machineReset(): void;

    clipboardPasteFrom(text: string): void;
  }
}
