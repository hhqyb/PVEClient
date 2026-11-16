// noVNC 的 VNC 通道全部由 bridge.rs 的本地 WS 中转实现，
// 此模块仅保留占位以分离「协议实现」与「协议选路」的职责边界。
//
// VNC 与 SPICE 的关键差异：
// - VNC 可内嵌 webview（noVNC），SPICE 只能调外部程序
// - VNC 只需 8006 一个端口，SPICE 还需要 3128 + 61000+
// - VNC 的 RFB 密码取自 vncproxy 的 password，vncticket 用于 WS 握手
