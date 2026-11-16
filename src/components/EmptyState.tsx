interface Props {
  title: string;
  description: React.ReactNode;
  hint?: React.ReactNode;
  action?: React.ReactNode;
}

export default function EmptyState({ title, description, hint, action }: Props) {
  return (
    <div className="center-page">
      <div className="empty-state">
        <h2>{title}</h2>
        <p>{description}</p>
        {hint && <div className="empty-hint">{hint}</div>}
        {action}
      </div>
    </div>
  );
}
