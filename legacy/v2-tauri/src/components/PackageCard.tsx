import type { Package, InstallStatus, PackageProgress } from "../types/manifest";

interface PackageCardProps {
  pkg: Package;
  selected: boolean;
  installStatus: InstallStatus;
  progress: PackageProgress | null;
  onToggle: (id: string) => void;
  disabled: boolean;
}

function StatusBadge({
  pkg,
  installStatus,
}: {
  pkg: Package;
  installStatus: InstallStatus;
}) {
  if (!pkg.url && !pkg.drive_url) {
    return (
      <span className="badge badge-no-url">
        <span className="material-symbols-rounded sz-18">cloud_off</span>
        Drive Required
      </span>
    );
  }
  if (installStatus === "installed") {
    return (
      <span className="badge badge-installed">
        <span className="material-symbols-rounded sz-18 filled">check_circle</span>
        Installed
      </span>
    );
  }
  if (installStatus === "checking") {
    return (
      <span className="badge badge-legacy">
        <span className="material-symbols-rounded sz-18">pending</span>
        Checking
      </span>
    );
  }
  if (pkg.recommended) {
    return (
      <span className="badge badge-recommended">
        <span className="material-symbols-rounded sz-18 filled">star</span>
        Recommended
      </span>
    );
  }
  if (pkg.legacy) {
    return <span className="badge badge-legacy">Legacy</span>;
  }
  return null;
}

function ProgressRow({ progress }: { progress: PackageProgress }) {
  const stageLabel: Record<string, string> = {
    downloading: `Downloading... ${progress.progress}%`,
    downloaded: "Downloaded",
    queued: "Waiting for UAC...",
    installing: `Installing... ${progress.progress}%`,
    done: "Installed",
    error: `Error: ${progress.message || "Unknown error"}`,
    skipped: "No URL — skipped",
  };

  const isError = progress.stage === "error";
  const isDone = progress.stage === "done";

  return (
    <div style={{ paddingLeft: "26px", paddingTop: "4px" }}>
      <div className="progress-track" style={{ marginBottom: "3px" }}>
        <div
          className="progress-fill"
          style={{
            width: `${isDone ? 100 : progress.progress}%`,
            background: isError ? "var(--md-error)" : "var(--md-primary)",
          }}
        />
      </div>
      <p
        className="label-small"
        style={{ color: isError ? "var(--md-error)" : "var(--md-on-surface-variant)" }}
      >
        {stageLabel[progress.stage] ?? progress.stage}
      </p>
    </div>
  );
}

export function PackageCard({
  pkg,
  selected,
  installStatus,
  progress,
  onToggle,
  disabled,
}: PackageCardProps) {
  const hasUrl = !!(pkg.url || pkg.drive_url);
  const isActive =
    progress &&
    progress.stage !== "idle" &&
    progress.stage !== "done" &&
    progress.stage !== "skipped";

  return (
    <div
      style={{
        padding: "10px 16px",
        borderRadius: "12px",
        background: selected
          ? "color-mix(in srgb, var(--md-primary) 8%, var(--md-surface-container))"
          : "var(--md-surface-container)",
        cursor: hasUrl && !disabled ? "pointer" : "default",
        transition: "background 0.15s",
        border: selected
          ? "1px solid color-mix(in srgb, var(--md-primary) 30%, transparent)"
          : "1px solid transparent",
      }}
      onClick={() => hasUrl && !disabled && onToggle(pkg.id)}
    >
      <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
        <input
          type="checkbox"
          className="md-checkbox"
          checked={selected}
          onChange={() => hasUrl && !disabled && onToggle(pkg.id)}
          disabled={!hasUrl || disabled}
          onClick={(e) => e.stopPropagation()}
        />
        <div style={{ flex: 1, minWidth: 0 }}>
          <p
            className="label-large"
            style={{
              color: "var(--md-on-surface)",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {pkg.name}
          </p>
          <p className="body-small" style={{ color: "var(--md-on-surface-variant)" }}>
            v{pkg.version} — {pkg.size_mb} MB
          </p>
        </div>
        <StatusBadge pkg={pkg} installStatus={installStatus} />
      </div>

      {isActive && progress && <ProgressRow progress={progress} />}

      {progress?.stage === "done" && (
        <div style={{ paddingLeft: "26px", paddingTop: "4px" }}>
          <p className="label-small" style={{ color: "var(--md-primary)" }}>
            Installed successfully
          </p>
        </div>
      )}
    </div>
  );
}
