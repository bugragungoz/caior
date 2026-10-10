import { useRef, useEffect } from "react";
import type { Package, PackageProgress } from "../types/manifest";
import type { InstallerPhase } from "../hooks/useInstaller";
import type { InstallResult } from "../types/manifest";

interface InstallDrawerProps {
  packages: Package[];
  phase: InstallerPhase;
  progress: Record<string, PackageProgress>;
  result: InstallResult | null;
  logs: string[];
  onCancel: () => void;
  onClose: () => void;
}

function PhaseIcon({ phase }: { phase: InstallerPhase }) {
  if (phase === "completed")
    return (
      <span className="material-symbols-rounded filled" style={{ color: "var(--md-primary)", fontSize: "22px" }}>
        check_circle
      </span>
    );
  if (phase === "error" || phase === "cancelled")
    return (
      <span className="material-symbols-rounded filled" style={{ color: "var(--md-error)", fontSize: "22px" }}>
        cancel
      </span>
    );
  return (
    <span
      className="material-symbols-rounded"
      style={{
        color: "var(--md-primary)",
        fontSize: "22px",
        animation: "spin 1.5s linear infinite",
      }}
    >
      progress_activity
    </span>
  );
}

export function InstallDrawer({
  packages,
  phase,
  progress,
  result,
  logs,
  onCancel,
  onClose,
}: InstallDrawerProps) {
  const logRef = useRef<HTMLDivElement>(null);
  const done = phase === "completed" || phase === "error" || phase === "cancelled";

  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [logs]);

  const stageLabel = {
    idle: "Preparing...",
    downloading: "Downloading packages...",
    installing: "Installing packages...",
    completed: `Done — ${result?.success_count ?? 0} installed, ${result?.error_count ?? 0} failed`,
    error: "Installation encountered errors",
    cancelled: "Installation cancelled",
  }[phase];

  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        top: "48px",
        background: "var(--md-surface-container-low)",
        display: "flex",
        flexDirection: "column",
        zIndex: 50,
      }}
      className="fade-in"
    >
      {/* Header */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: "12px",
          padding: "16px 20px 12px",
          borderBottom: "1px solid var(--md-outline-variant)",
          flexShrink: 0,
        }}
      >
        <PhaseIcon phase={phase} />
        <p
          style={{
            flex: 1,
            fontSize: "16px",
            fontWeight: 500,
            color: "var(--md-on-surface)",
          }}
        >
          {stageLabel}
        </p>
        {done ? (
          <button className="btn-tonal" onClick={onClose}>
            Close
          </button>
        ) : (
          <button className="btn-outlined" onClick={onCancel}>
            Cancel
          </button>
        )}
      </div>

      {/* Package progress list */}
      <div
        style={{
          overflowY: "auto",
          padding: "12px 16px",
          display: "flex",
          flexDirection: "column",
          gap: "6px",
          flex: "0 0 auto",
          maxHeight: "240px",
        }}
      >
        {packages.map((pkg) => {
          const p = progress[pkg.id];
          const stage = p?.stage ?? "idle";
          const pct = p?.progress ?? 0;

          const stageColor =
            stage === "done"
              ? "var(--md-primary)"
              : stage === "error"
              ? "var(--md-error)"
              : "var(--md-on-surface)";

          const stageText =
            stage === "idle" || stage === undefined
              ? "Waiting..."
              : stage === "downloading"
              ? `Downloading ${pct}%`
              : stage === "downloaded"
              ? "Downloaded"
              : stage === "queued"
              ? "Waiting for UAC..."
              : stage === "installing"
              ? `Installing ${pct}%`
              : stage === "done"
              ? "Installed"
              : stage === "error"
              ? `Error: ${p?.message ?? ""}`
              : stage === "skipped"
              ? "Skipped"
              : stage;

          const showBar =
            stage === "downloading" ||
            stage === "installing" ||
            stage === "done";

          return (
            <div
              key={pkg.id}
              style={{
                background: "var(--md-surface-container)",
                borderRadius: "10px",
                padding: "10px 14px",
              }}
            >
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: "8px",
                  marginBottom: showBar ? "6px" : 0,
                }}
              >
                <p
                  className="label-large"
                  style={{ flex: 1, color: "var(--md-on-surface)" }}
                >
                  {pkg.name}
                </p>
                <p className="label-small" style={{ color: stageColor }}>
                  {stageText}
                </p>
              </div>
              {showBar && (
                <div className="progress-track">
                  <div
                    className="progress-fill"
                    style={{
                      width: `${stage === "done" ? 100 : pct}%`,
                      background: "var(--md-primary)",
                    }}
                  />
                </div>
              )}
            </div>
          );
        })}
      </div>

      <div className="divider" style={{ margin: "0 16px" }} />

      {/* Log output */}
      <div
        ref={logRef}
        style={{
          flex: 1,
          overflowY: "auto",
          padding: "12px 20px",
          fontFamily: '"Roboto Mono", "Consolas", monospace',
          fontSize: "12px",
          lineHeight: "18px",
          color: "var(--md-on-surface-variant)",
          background: "var(--md-surface-container-lowest)",
          margin: "8px 16px 16px",
          borderRadius: "8px",
        }}
      >
        {logs.length === 0 ? (
          <p style={{ color: "var(--md-outline)" }}>Waiting for output...</p>
        ) : (
          logs.map((line, i) => <div key={i}>{line}</div>)
        )}
      </div>

      <style>{`
        @keyframes spin {
          from { transform: rotate(0deg); }
          to { transform: rotate(360deg); }
        }
      `}</style>
    </div>
  );
}
