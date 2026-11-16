interface Props {
  running: boolean;
}

export default function StatusDot({ running }: Props) {
  return (
    <span
      className={`status-dot ${running ? "status-running" : "status-stopped"}`}
      aria-label={running ? "运行中" : "已停止"}
      title={running ? "运行中" : "已停止"}
    />
  );
}
