import { useState, useCallback } from "react";
import { TitleBar } from "./components/TitleBar";
import { CategorySection } from "./components/CategorySection";
import { InstallDrawer } from "./components/InstallDrawer";
import { AdminWarningDialog } from "./components/AdminWarningDialog";
import { OfflineErrorScreen } from "./components/OfflineErrorScreen";
import { useTheme } from "./hooks/useTheme";
import { useManifest } from "./hooks/useManifest";
import { useInstaller } from "./hooks/useInstaller";
import type { Package } from "./types/manifest";

export default function App() {
  const { theme, setTheme } = useTheme();
  const { manifest, status, installStatus, retry } = useManifest();
  const { phase, progress, result, logs, startInstall, cancelInstall, reset } =
    useInstaller();

  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [showWarning, setShowWarning] = useState(false);
  const [showDrawer, setShowDrawer] = useState(false);

  const togglePackage = useCallback((id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  }, []);

  const selectAll = useCallback((ids: string[]) => {
    setSelected((prev) => {
      const next = new Set(prev);
      ids.forEach((id) => next.add(id));
      return next;
    });
  }, []);

  const deselectAll = useCallback((ids: string[]) => {
    setSelected((prev) => {
      const next = new Set(prev);
      ids.forEach((id) => next.delete(id));
      return next;
    });
  }, []);

  const selectedPackages: Package[] = manifest
    ? manifest.categories
        .flatMap((c) => c.packages)
        .filter((p) => selected.has(p.id))
    : [];

  const totalSizeMb = selectedPackages.reduce((sum, p) => sum + p.size_mb, 0);

  const isInstalling = phase !== "idle" && !["completed", "error", "cancelled"].includes(phase);

  function handleInstallClick() {
    if (selectedPackages.length === 0) return;
    setShowWarning(true);
  }

  function handleConfirmInstall() {
    setShowWarning(false);
    setShowDrawer(true);
    startInstall(selectedPackages);
  }

  function handleDrawerClose() {
    setShowDrawer(false);
    reset();
  }

  if (status === "offline") {
    return (
      <div className={`theme-${theme}`} style={{ height: "100vh", display: "flex", flexDirection: "column" }}>
        <TitleBar theme={theme} onThemeChange={setTheme} />
        <OfflineErrorScreen onRetry={retry} />
      </div>
    );
  }

  return (
    <div
      className={`theme-${theme}`}
      style={{
        height: "100vh",
        display: "flex",
        flexDirection: "column",
        background: "var(--md-surface)",
        position: "relative",
        overflow: "hidden",
      }}
    >
      <TitleBar theme={theme} onThemeChange={setTheme} />

      {/* Main scrollable content */}
      <main
        style={{
          flex: 1,
          overflowY: "auto",
          overflowX: "hidden",
          padding: "8px 0",
        }}
      >
        {status === "loading" && (
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              justifyContent: "center",
              height: "100%",
              gap: "16px",
              color: "var(--md-on-surface-variant)",
            }}
          >
            <span
              className="material-symbols-rounded"
              style={{
                fontSize: "40px",
                animation: "spin 1.5s linear infinite",
              }}
            >
              progress_activity
            </span>
            <p className="body-medium">Loading package list...</p>
            <style>{`@keyframes spin { from{transform:rotate(0deg)} to{transform:rotate(360deg)} }`}</style>
          </div>
        )}

        {status === "loaded" && manifest && (
          <>
            {manifest.categories.map((cat) => (
              <CategorySection
                key={cat.id}
                category={cat}
                selected={selected}
                installStatus={installStatus}
                progress={progress}
                onToggle={togglePackage}
                onSelectAll={selectAll}
                onDeselectAll={deselectAll}
                disabled={isInstalling || showDrawer}
              />
            ))}
          </>
        )}
      </main>

      {/* Footer */}
      {status === "loaded" && !showDrawer && (
        <footer
          style={{
            flexShrink: 0,
            display: "flex",
            alignItems: "center",
            gap: "16px",
            padding: "12px 20px",
            background: "var(--md-surface-container-low)",
            borderTop: "1px solid var(--md-outline-variant)",
          }}
        >
          <div style={{ flex: 1 }}>
            {selected.size > 0 ? (
              <p className="body-medium" style={{ color: "var(--md-on-surface-variant)" }}>
                {selected.size} package{selected.size !== 1 ? "s" : ""} selected
                {totalSizeMb > 0 && (
                  <span style={{ marginLeft: "8px", color: "var(--md-outline)" }}>
                    ~{totalSizeMb >= 1024
                      ? `${(totalSizeMb / 1024).toFixed(1)} GB`
                      : `${totalSizeMb} MB`}
                  </span>
                )}
              </p>
            ) : (
              <p className="body-medium" style={{ color: "var(--md-outline)" }}>
                Select packages to install
              </p>
            )}
          </div>

          {selected.size > 0 && (
            <button
              className="btn-text"
              onClick={() => setSelected(new Set())}
            >
              Clear
            </button>
          )}

          <button
            className="btn-filled"
            disabled={selected.size === 0}
            onClick={handleInstallClick}
          >
            <span className="material-symbols-rounded sz-18">download</span>
            Install
            {selected.size > 0 && ` (${selected.size})`}
          </button>
        </footer>
      )}

      {/* Install drawer */}
      {showDrawer && (
        <InstallDrawer
          packages={selectedPackages}
          phase={phase}
          progress={progress}
          result={result}
          logs={logs}
          onCancel={cancelInstall}
          onClose={handleDrawerClose}
        />
      )}

      {/* Admin warning dialog */}
      {showWarning && (
        <AdminWarningDialog
          packageCount={selectedPackages.length}
          onCancel={() => setShowWarning(false)}
          onContinue={handleConfirmInstall}
        />
      )}
    </div>
  );
}
