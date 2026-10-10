import { useState, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Package,
  PackageProgress,
  InstallResult,
  PackageStage,
} from "../types/manifest";

export type InstallerPhase =
  | "idle"
  | "downloading"
  | "installing"
  | "completed"
  | "error"
  | "cancelled";

export function useInstaller() {
  const [phase, setPhase] = useState<InstallerPhase>("idle");
  const [progress, setProgress] = useState<Record<string, PackageProgress>>({});
  const [result, setResult] = useState<InstallResult | null>(null);
  const [logs, setLogs] = useState<string[]>([]);
  const unlistenRef = useRef<UnlistenFn[]>([]);

  const appendLog = (msg: string) =>
    setLogs((prev) => [...prev, `${new Date().toLocaleTimeString()} ${msg}`]);

  const startInstall = useCallback(async (packages: Package[]) => {
    setPhase("downloading");
    setResult(null);
    setLogs([]);

    // Initialize progress for all packages
    const initial: Record<string, PackageProgress> = {};
    packages.forEach((p) => {
      initial[p.id] = {
        packageId: p.id,
        stage: "idle",
        progress: 0,
        message: "",
      };
    });
    setProgress(initial);

    // Set up event listeners
    const unlisten1 = await listen<{
      package_id: string;
      stage: PackageStage;
      progress: number;
      message: string;
    }>("install_progress", (event) => {
      const { package_id, stage, progress: pct, message } = event.payload;

      setProgress((prev) => ({
        ...prev,
        [package_id]: { packageId: package_id, stage, progress: pct, message },
      }));

      if (
        stage === "downloading" &&
        pct === 0 &&
        message.includes("Downloading")
      ) {
        appendLog(message || `Downloading ${package_id}...`);
      } else if (stage === "downloaded") {
        appendLog(`Downloaded: ${package_id}`);
        setPhase("installing");
      } else if (stage === "installing") {
        appendLog(`Installing: ${package_id}...`);
      } else if (stage === "done") {
        appendLog(`Installed: ${package_id}`);
      } else if (stage === "error") {
        appendLog(`Error: ${package_id} — ${message}`);
      } else if (stage === "skipped") {
        appendLog(`Skipped: ${package_id} (no URL)`);
      }
    });

    const unlisten2 = await listen<InstallResult>("install_result", (event) => {
      const res = event.payload;
      setResult(res);
      setPhase(res.stage === "completed" ? "completed" : res.stage === "cancelled" ? "cancelled" : "error");
      appendLog(
        `Done — ${res.success_count} succeeded, ${res.error_count} failed`
      );
    });

    unlistenRef.current = [unlisten1, unlisten2];

    try {
      const payload = packages.map((p) => ({
        id: p.id,
        name: p.name,
        url: p.url ?? "",
        drive_url: p.drive_url ?? null,
        installer_type: p.installer_type,
        args: p.args ?? "",
      }));

      await invoke("start_installation", { packages: payload });
    } catch (err) {
      appendLog(`Fatal error: ${err}`);
      setPhase("error");
    }

    unlistenRef.current.forEach((fn) => fn());
    unlistenRef.current = [];
  }, []);

  const cancelInstall = useCallback(async () => {
    await invoke("cancel_installation");
  }, []);

  const reset = useCallback(() => {
    setPhase("idle");
    setProgress({});
    setResult(null);
    setLogs([]);
  }, []);

  return { phase, progress, result, logs, startInstall, cancelInstall, reset };
}
