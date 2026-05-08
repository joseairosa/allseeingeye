interface StatusbarProps {
  resultCount: number;
}

export function Statusbar({ resultCount }: StatusbarProps) {
  return (
    <footer className="statusbar">
      <span>{resultCount} components</span>
      <span>scan completed 18s ago</span>
      <span>watchers: 4 tools</span>
      <span>privacy: local only</span>
    </footer>
  );
}
