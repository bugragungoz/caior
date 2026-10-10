export interface RegistryCheck {
  hive: string;
  key: string;
  value: string | null;
  min_dword: number | null;
}

export interface Package {
  id: string;
  name: string;
  version: string;
  url: string;
  drive_url: string | null;
  args: string;
  installer_type: string;
  size_mb: number;
  recommended: boolean;
  legacy: boolean;
  registry_check: RegistryCheck | null;
}

export interface Category {
  id: string;
  label: string;
  icon: string;
  packages: Package[];
}

export interface Manifest {
  version: string;
  categories: Category[];
}

export type InstallStatus =
  | "installed"
  | "not_installed"
  | "unknown"
  | "checking";

export type PackageStage =
  | "idle"
  | "downloading"
  | "downloaded"
  | "queued"
  | "installing"
  | "done"
  | "error"
  | "skipped";

export interface PackageProgress {
  packageId: string;
  stage: PackageStage;
  progress: number;
  message: string;
}

export interface InstallResult {
  stage: "completed" | "cancelled" | "error";
  success_count: number;
  error_count: number;
  errors: Array<{ package_id: string; message: string }>;
}

export type ThemeName = "violet" | "ocean" | "ember" | "forest";
