import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Manifest, InstallStatus, RegistryCheck } from "../types/manifest";

const MANIFEST_URL =
  "https://raw.githubusercontent.com/bugragungoz/croxz_aio_runtimes/main/packages/manifest.json";

export type ManifestLoadStatus = "loading" | "loaded" | "offline" | "error";

export function useManifest() {
  const [manifest, setManifest] = useState<Manifest | null>(null);
  const [status, setStatus] = useState<ManifestLoadStatus>("loading");
  const [installStatus, setInstallStatus] = useState<
    Record<string, InstallStatus>
  >({});

  const fetchManifest = useCallback(async () => {
    setStatus("loading");
    try {
      const resp = await fetch(MANIFEST_URL);
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      const data: Manifest = await resp.json();
      setManifest(data);
      setStatus("loaded");
      checkRegistry(data);
    } catch {
      setStatus("offline");
    }
  }, []);

  const checkRegistry = useCallback(async (data: Manifest) => {
    const allPackages = data.categories.flatMap((c) => c.packages);
    const checks: Array<RegistryCheck | null> = allPackages.map(
      (p) => p.registry_check
    );

    // Initialize all as checking
    const initial: Record<string, InstallStatus> = {};
    allPackages.forEach((p) => {
      initial[p.id] = p.registry_check ? "checking" : "unknown";
    });
    setInstallStatus(initial);

    try {
      const results: Array<boolean | null> = await invoke(
        "check_registry_batch",
        { checks }
      );
      const updated: Record<string, InstallStatus> = {};
      allPackages.forEach((p, i) => {
        const result = results[i];
        updated[p.id] =
          result === null
            ? "unknown"
            : result
            ? "installed"
            : "not_installed";
      });
      setInstallStatus(updated);
    } catch {
      const fallback: Record<string, InstallStatus> = {};
      allPackages.forEach((p) => (fallback[p.id] = "unknown"));
      setInstallStatus(fallback);
    }
  }, []);

  useEffect(() => {
    fetchManifest();
  }, [fetchManifest]);

  return { manifest, status, installStatus, retry: fetchManifest };
}
