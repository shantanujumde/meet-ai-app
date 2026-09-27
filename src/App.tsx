/**
 * The app shell.
 *
 * This is scaffold, not Phase 2. It proves the window opens and that the
 * design-system tokens are reaching the webview. The meeting list, live
 * transcript, notes pane and onboarding all land in Phase 2 (SPEC §5).
 */
export function App() {
  return (
    <main
      style={{
        height: "100%",
        display: "grid",
        placeItems: "center",
        padding: "2rem",
        background: "var(--surface-canvas, #131316)",
        color: "var(--text-primary, #f7f7f8)",
      }}
    >
      <div style={{ maxWidth: "28rem", textAlign: "center" }}>
        <h1 style={{ fontSize: "1.375rem", fontWeight: 600, margin: "0 0 0.5rem" }}>meet-ai</h1>
        <p style={{ margin: 0, opacity: 0.7, lineHeight: 1.5 }}>
          The app shell is wired up and running. Recording, transcripts and notes arrive in the next
          phase.
        </p>
      </div>
    </main>
  );
}
