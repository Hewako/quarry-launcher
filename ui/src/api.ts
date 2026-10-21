// Типизированные обёртки над командами Tauri. Формы данных совпадают с serde в quarry-core.
import { invoke } from "@tauri-apps/api/core";

export type LoaderKind = "vanilla" | "fabric" | "quilt" | "neoforge" | "forge";
export type AuthMode = "microsoft" | "ely_by";
export type Source = "profile" | "pack" | "global" | "default";
export type Outcome = "success" | "crashed" | "killed" | "failed_to_start";
export type Provider = "microsoft" | "elyby";

export interface LaunchOverrides {
  max_memory_mb?: number;
  min_memory_mb?: number;
  java_path?: string;
  jvm_args?: string[];
  resolution?: [number, number];
}

export interface LastLaunch {
  at: number;
  outcome: Outcome;
  exit_code?: number;
  duration_secs: number;
  message?: string;
  log_file?: string;
}

export interface Instance {
  id: string;
  name: string;
  game_version: string;
  loader: LoaderKind;
  loader_version?: string;
  created_at: number;
  group?: string;
  auth_mode: AuthMode;
  account?: string;
  settings: LaunchOverrides;
  pack_settings: LaunchOverrides;
  last_launch?: LastLaunch;
}

export interface Resolved<T> { value: T; source: Source }
export interface ResolvedLaunch {
  max_memory_mb: Resolved<number>;
  min_memory_mb: Resolved<number>;
  java_path: Resolved<string | null>;
  jvm_args: Resolved<string[]>;
  resolution: Resolved<[number, number] | null>;
}

export interface AppInfo {
  version: string;
  dev_launch: boolean;
  data_dir: string;
  microsoft_configured: boolean;
  os: string;
}

export interface VersionEntry { id: string; type: string; releaseTime: string }
export interface LoaderVersion { version: string; stable: boolean }
export interface AccountRef { provider: Provider; uuid: string; name: string; owns_java_edition: boolean; checked_at: number; skin_url?: string; skin_variant?: string }
export interface GlobalSettings { launch: LaunchOverrides; theme: string; streamer_mode: boolean; default_account?: string }
export interface DeviceCode { device_code: string; user_code: string; verification_uri: string; expires_in: number; interval: number; message: string }

export type QuickPlay = { kind: "singleplayer" | "multiplayer" | "realms"; target: string };

export interface Progress { files_total: number; files_done: number; bytes_total: number; bytes_done: number; retries: number }
export interface ExitInfo { code?: number; duration_secs: number; killed: boolean; crashed: boolean; crash_report?: string; log_file: string; tail: string[] }
export type Stage = "metadata" | "libraries" | "assets" | "natives" | "java";

export type LaunchEvent =
  | { type: "stage"; stage: Stage; label: string }
  | { type: "progress"; stage: Stage; progress: Progress }
  | { type: "account"; provider: string; name: string }
  | { type: "java"; label: string; reason: string }
  | { type: "warning"; message: string }
  | { type: "started"; pid?: number; command: string }
  | { type: "log"; line: string }
  | { type: "exited"; info: ExitInfo };

export const accountKey = (a: AccountRef) => `${a.provider}:${a.uuid}`;

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  listInstances: () => invoke<{ instances: Instance[]; broken: { dir: string; reason: string }[]; running: string[] }>("list_instances"),
  createInstance: (name: string, gameVersion: string, loader: LoaderKind, loaderVersion?: string) =>
    invoke<Instance>("create_instance", { name, gameVersion, loader, loaderVersion: loaderVersion ?? null }),
  trashInstance: (id: string) => invoke<string>("trash_instance", { id }),
  updateInstance: (id: string, patch: { name?: string; settings?: LaunchOverrides; authMode?: AuthMode; account?: string | null }) =>
    invoke<Instance>("update_instance", {
      id,
      name: patch.name ?? null,
      settings: patch.settings ?? null,
      authMode: patch.authMode ?? null,
      // "" — аккаунт по умолчанию, null — не менять.
      account: patch.account === undefined ? null : (patch.account ?? ""),
    }),
  launchSettings: (id: string) =>
    invoke<{ resolved: ResolvedLaunch; profile: LaunchOverrides; global: LaunchOverrides; total_memory_mb?: number }>("launch_settings", { id }),
  globalSettings: () => invoke<GlobalSettings>("global_settings"),
  saveGlobalSettings: (settings: GlobalSettings) => invoke<void>("save_global_settings", { settings }),
  listVersions: (snapshots: boolean, old: boolean) =>
    invoke<{ versions: VersionEntry[]; latest_release: string; offline: boolean }>("list_versions", { snapshots, old }),
  versionInfo: (id: string) => invoke<{ java_major: number; java_reason: string }>("version_info", { id }),
  loaderVersions: (loader: LoaderKind, gameVersion: string) => invoke<LoaderVersion[]>("loader_versions", { loader, gameVersion }),
  launch: (id: string, quickPlay?: QuickPlay, fullVerify = false) =>
    invoke<void>("launch_instance", { id, quickPlay: quickPlay ?? null, fullVerify }),
  stop: (id: string) => invoke<boolean>("stop_instance", { id }),
  previewCommand: (id: string) => invoke<string>("preview_command", { id }),
  openFolder: (id: string, sub?: string) => invoke<void>("open_instance_folder", { id, sub: sub ?? null }),
  readLog: (id: string) => invoke<string[]>("read_log_file", { id, file: null }),
  listAccounts: () => invoke<AccountRef[]>("list_accounts"),
  removeAccount: (key: string) => invoke<void>("remove_account", { key }),
  accountAvatar: (key: string) => invoke<string | null>("account_avatar", { key }),
  setDefaultAccount: (key: string) => invoke<void>("set_default_account", { key }),
  microsoftStart: () => invoke<DeviceCode>("microsoft_start"),
  microsoftFinish: (code: DeviceCode) => invoke<AccountRef>("microsoft_finish", { code }),
  elyLogin: () => invoke<AccountRef>("ely_login"),
  elyLoginPassword: (login: string, password: string, totp?: string) =>
    invoke<{ account?: AccountRef; need_totp: boolean }>("ely_login_password", { login, password, totp: totp ?? null }),
  cancelLogin: () => invoke<void>("cancel_login"),
  openLink: (url: string) => invoke<void>("open_link", { url }),
};

export function errText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}

export const loaderName: Record<LoaderKind, string> = {
  vanilla: "Vanilla", fabric: "Fabric", quilt: "Quilt", neoforge: "NeoForge", forge: "Forge",
};

export const sourceName: Record<Source, string> = {
  profile: "профиль", pack: "сборка", global: "глобально", default: "по умолчанию",
};

export function relTime(unix: number): string {
  if (!unix) return "—";
  const d = new Date(unix * 1000);
  const now = new Date();
  const days = Math.floor((new Date(now.toDateString()).getTime() - new Date(d.toDateString()).getTime()) / 86_400_000);
  const hm = d.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
  if (days === 0) return `сегодня, ${hm}`;
  if (days === 1) return `вчера, ${hm}`;
  if (days < 7) return `${days} дн. назад`;
  return d.toLocaleDateString("ru-RU", { day: "2-digit", month: "2-digit" });
}

export function duration(secs: number): string {
  if (secs < 60) return `${secs} с`;
  const m = Math.floor(secs / 60);
  if (m < 60) return `${m} мин`;
  return `${Math.floor(m / 60)} ч ${m % 60} мин`;
}

export function mb(bytes: number): string {
  return (bytes / 1_048_576).toFixed(1).replace(".", ",");
}

/** Статус сборки по последнему настоящему запуску (разбор игрока, п. 4). */
export function statusOf(i: Instance, phase?: "preparing" | "running" | "done" | "failed"): { text: string; cls: "ok" | "warn" | "err" | "" } {
  if (phase === "preparing") return { text: "подготовка…", cls: "warn" };
  if (phase === "running") return { text: "игра запущена", cls: "ok" };
  const l = i.last_launch;
  if (!l) return { text: "ещё не запускался", cls: "warn" };
  switch (l.outcome) {
    case "success": return { text: `запуск успешен · ${relTime(l.at)}`, cls: "ok" };
    case "killed": return { text: `остановлен · ${relTime(l.at)}`, cls: "" };
    case "crashed": return { text: `упал · ${relTime(l.at)}`, cls: "err" };
    case "failed_to_start": return { text: `не запустился · ${relTime(l.at)}`, cls: "err" };
  }
}
