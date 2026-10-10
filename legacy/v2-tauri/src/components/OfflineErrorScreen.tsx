import { getCurrentWindow } from "@tauri-apps/api/window";

interface OfflineErrorScreenProps {
  onRetry: () => void;
}

const appWindow = getCurrentWindow();

export function OfflineErrorScreen({ onRetry }: OfflineErrorScreenProps) {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        height: "100%",
        gap: "24px",
        padding: "40px",
        background: "var(--md-surface)",
      }}
    >
      <div
        style={{
          width: "72px",
          height: "72px",
          borderRadius: "50%",
          background: "var(--md-error-container)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <span
          className="material-symbols-rounded"
          style={{ fontSize: "36px", color: "var(--md-error)" }}
        >
          wifi_off
        </span>
      </div>

      <div style={{ textAlign: "center", maxWidth: "380px" }}>
        <p
          style={{
            fontSize: "22px",
            fontWeight: 400,
            color: "var(--md-on-surface)",
            marginBottom: "8px",
          }}
        >
          No Internet Connection
        </p>
        <p
          style={{
            fontSize: "14px",
            color: "var(--md-on-surface-variant)",
            lineHeight: "20px",
          }}
        >
          croxz runtimes requires an active internet connection to download the
          package list and installers.
        </p>
      </div>

      <div style={{ display: "flex", gap: "12px" }}>
        <button className="btn-filled" onClick={onRetry}>
          <span className="material-symbols-rounded sz-18">refresh</span>
          Retry
        </button>
        <button
          className="btn-outlined"
          onClick={() => appWindow.close()}
        >
          Exit
        </button>
      </div>
    </div>
  );
}
