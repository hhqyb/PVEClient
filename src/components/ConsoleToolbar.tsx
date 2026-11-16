interface Props {
  title: string;
  subtitle?: string;
  onBack: () => void;
  onSwitchProtocol?: () => void;
  switchLabel?: string;
  extra?: React.ReactNode;
}

export default function ConsoleToolbar({
  title,
  subtitle,
  onBack,
  onSwitchProtocol,
  switchLabel,
  extra,
}: Props) {
  return (
    <header className="topbar">
      <button className="btn btn-ghost" onClick={onBack} title="返回列表">
        ← 返回
      </button>
      <div style={{ minWidth: 0 }}>
        <div className="topbar-title" style={{ lineHeight: 1.3 }}>
          {title}
        </div>
        {subtitle && <div className="topbar-meta">{subtitle}</div>}
      </div>
      <span className="spacer" />
      {extra}
      {onSwitchProtocol && (
        <button className="btn btn-ghost" onClick={onSwitchProtocol}>
          {switchLabel ?? "切换协议"}
        </button>
      )}
    </header>
  );
}
