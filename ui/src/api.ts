// Типизированные обёртки над командами Tauri. Формы данных совпадают с serde в quarry-core.
import { invoke } from "@tauri-apps/api/core";
import { currentLang, t } from "./i18n";

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
  game_args?: string[];
  env?: Record<string, string>;
}

export interface LastLaunch {
  prepare_secs?: number;
  peak_ram_mb?: number;
  cpu_percent?: number;
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
  description?: string;
  color?: string;
  favorite?: boolean;
  archived?: boolean;
  auth_mode: AuthMode;
  account?: string;
  settings: LaunchOverrides;
  pack_settings: LaunchOverrides;
  last_launch?: LastLaunch;
  external_game_dir?: string;
  pack?: { name: string; version: string; project_id?: string; version_id?: string; channel?: Channel; pinned?: boolean };
  hooks?: Hooks;
}
export interface Hooks { pre?: string; post_exit?: string; post_crash?: string; wrapper?: string; approved?: string }

export interface ProfileMeta { group?: string; description?: string; color?: string; favorite: boolean; archived: boolean }
export interface CloneParts { mods: boolean; configs: boolean; resources: boolean; worlds: boolean }
export const metaOf = (i: Instance): ProfileMeta => ({ group: i.group, description: i.description, color: i.color, favorite: !!i.favorite, archived: !!i.archived });
/** Что можно перенести между профилями (совпадает с ядром, instance::ops::SHARABLE). */
export const SHARABLE: [string, string][] = [
  ["options.txt", t("Настройки игры (управление, графика, звук)")],
  ["servers.dat", t("Список серверов")],
  ["resourcepacks", t("Ресурспаки")],
  ["shaderpacks", t("Шейдеры")],
  ["config", t("Настройки модов")],
  ["saves", t("Миры (копией, свои миры не заменяются)")],
  ["screenshots", t("Скриншоты")],
];

export interface Server { name: string; ip: string; icon?: string; accept_textures?: boolean }
export interface ServerStatus { version: string; protocol: number; online: number; max: number; motd: string; latency_ms: number; favicon?: string }

export interface PackInfo { file_name: string; description: string; size: number; is_dir: boolean; enabled: boolean; formats?: [number, number]; compatible?: boolean; icon?: string }
export type ResKind = "resourcepack" | "shader" | "datapack";

export interface ConfigFile { path: string; size: number; user_edited: boolean }
export interface DiffLine { kind: "=" | "+" | "-"; text: string }
/** «5 модов» с правильным склонением (ru) или «5 mods» (en). */
export function modsLabel(n: number): string {
  if (currentLang() === "en") return `${n} ${n === 1 ? "mod" : "mods"}`;
  const d = n % 10, h = n % 100;
  return `${n} ${d === 1 && h !== 11 ? "мод" : d >= 2 && d <= 4 && (h < 12 || h > 14) ? "мода" : "модов"}`;
}

export const configs = {
  list: (id: string) => invoke<ConfigFile[]>("configs_list", { id }),
  search: (id: string, query: string) => invoke<{ path: string; line: number; text: string }[]>("configs_search", { id, query }),
  read: (id: string, path: string) => invoke<string>("config_read", { id, path }),
  preview: (id: string, path: string, text: string) => invoke<{ diff: DiffLine[]; error?: string }>("config_preview", { id, path, text }),
  save: (id: string, path: string, text: string) => invoke<void>("config_save", { id, path, text }),
  history: (id: string, path: string) => invoke<{ at: number; size: number }[]>("config_history", { id, path }),
  historyRead: (id: string, path: string, at: number) => invoke<string>("config_history_read", { id, path, at }),
};

export type OptCategory = "graphics" | "sound" | "controls";
export interface OptionsView { entries: [string, string][]; conflicts: { key: string; actions: string[] }[]; presets: { name: string; category: OptCategory; count: number }[] }
export const gameOptions = {
  view: (id: string) => invoke<OptionsView>("options_view", { id }),
  set: (id: string, changes: Record<string, string>) => invoke<void>("options_set", { id, changes }),
  savePreset: (id: string, name: string, category: OptCategory) => invoke<{ name: string }>("options_preset_save", { id, name, category }),
  applyPreset: (id: string, name: string) => invoke<void>("options_preset_apply", { id, name }),
  exportLayout: (id: string, path: string) => invoke<void>("options_layout_export", { id, path }),
  importLayout: (id: string, path: string) => invoke<number>("options_layout_import", { id, path }),
};

export const diagnostics = {
  crashReports: (id: string) => invoke<{ file: string; when: string; size: number; description: string }[]>("crash_reports", { id }),
  readCrash: (id: string, file: string) => invoke<string>("crash_report_read", { id, file }),
  reportPreview: (id: string) => invoke<{ name: string; content: string }[]>("support_report_preview", { id }),
  reportSave: (id: string, path: string) => invoke<void>("support_report_save", { id, path }),
  storage: () => invoke<{ label: string; size: number; cleanable: boolean }[]>("storage_overview"),
  cleanCache: () => invoke<number>("cache_clean"),
};

export interface PackVersion { path?: string; version_id: string; version_number: string; changelog?: string }
export interface PackUpdatePlan { from: string; to: string; add: string[]; update: string[]; remove: string[]; kept_mods: string[]; kept_configs: string[]; optional: string[]; game_change?: string }

export interface Bisect { suspects: string[]; disabled: string[]; half: string[]; round: number; snapshot: string }

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
  curseforge: boolean;
  free_bytes?: number;
  data_dir: string;
  microsoft_configured: boolean;
  os: string;
}

export interface VersionEntry { id: string; type: string; releaseTime: string }
export interface LoaderVersion { version: string; stable: boolean }
export interface AccountRef { provider: Provider; uuid: string; name: string; owns_java_edition: boolean; checked_at: number; skin_url?: string; skin_variant?: string }
export interface GlobalSettings {
  launch: LaunchOverrides; theme: string; streamer_mode: boolean; default_account?: string;
  language: string; accent?: string; ui_scale: number; compact: boolean; font: string; first_run_done: boolean;
  backup_before_risky_launch: boolean; backup_after_exit: boolean; backup_every_days: number; download_limit_kbps: number; check_launcher_updates: boolean;
}
export interface WorldBackup { world: string; file: string; size: number; created_at: number }
export interface WorldInfo {
  folder: string; name: string; version_name?: string; data_version?: number; game_mode: string; hardcore: boolean;
  last_played: number; size: number; backups: WorldBackup[];
}
export interface DeviceCode { device_code: string; user_code: string; verification_uri: string; expires_in: number; interval: number; message: string }

export type QuickPlay = { kind: "singleplayer" | "multiplayer" | "realms"; target: string };

export interface Progress { files_total: number; files_done: number; bytes_total: number; bytes_done: number; retries: number }
export interface ExitInfo { code?: number; duration_secs: number; killed: boolean; crashed: boolean; crash_report?: string; log_file: string; tail: string[]; analysis?: CrashAnalysis }
export type Stage = "metadata" | "libraries" | "assets" | "natives" | "java" | "loader";

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
  listInstances: () => invoke<{ instances: Instance[]; broken: { dir: string; reason: string }[]; running: string[]; mods: Record<string, number> }>("list_instances"),
  createInstance: (name: string, gameVersion: string, loader: LoaderKind, loaderVersion?: string) =>
    invoke<Instance>("create_instance", { name, gameVersion, loader, loaderVersion: loaderVersion ?? null }),
  trashInstance: (id: string) => invoke<string>("trash_instance", { id }),
  updateInstance: (id: string, patch: { name?: string; settings?: LaunchOverrides; authMode?: AuthMode; account?: string | null; meta?: ProfileMeta; hooks?: Hooks }) =>
    invoke<Instance>("update_instance", {
      id,
      name: patch.name ?? null,
      settings: patch.settings ?? null,
      authMode: patch.authMode ?? null,
      // "" — аккаунт по умолчанию, null — не менять.
      account: patch.account === undefined ? null : (patch.account ?? ""),
      meta: patch.meta ?? null,
      hooks: patch.hooks ?? null,
    }),
  cloneInstance: (id: string, name: string, parts: CloneParts) => invoke<Instance>("clone_instance", { id, name, parts }),
  diskUsage: (id: string) => invoke<{ parts: { label: string; size: number }[]; total: number }>("disk_usage", { id }),
  copyBetween: (from: string, to: string, items: string[]) => invoke<void>("copy_between", { from, to, items }),
  moveGameDir: (id: string, target: string | null) => invoke<string>("move_game_dir", { id, target }),
  resources: (id: string, kind: ResKind, world?: string) => invoke<{ packs: PackInfo[]; game_format?: number; shader_loader: boolean }>("resources_list", { id, kind, world: world ?? null }),
  setResourcePacks: (id: string, enabled: string[]) => invoke<void>("resources_set_enabled", { id, enabled }),
  setShader: (id: string, shader: string | null) => invoke<void>("shader_set", { id, shader }),
  removeResource: (id: string, kind: ResKind, file: string, world?: string) => invoke<void>("resource_remove", { id, kind, file, world: world ?? null }),
  installResource: (id: string, project: string, kind: ResKind, world?: string) => invoke<string>("resource_install", { id, project, kind, world: world ?? null }),
  servers: (id: string) => invoke<Server[]>("servers_list", { id }),
  saveServers: (id: string, servers: Server[]) => invoke<void>("servers_save", { id, servers }),
  pingServer: (address: string) => invoke<ServerStatus>("server_ping", { address }),
  firstRunInfo: () => invoke<{ data_dir: string; free_bytes?: number; javas: { path: string; major: number; version: string; arch: string }[] }>("first_run_info"),
  firstRunDone: (language: string) => invoke<void>("first_run_done", { language }),
  launcherUpdate: () => invoke<{ version: string; url: string; notes: string } | null>("launcher_update"),
  pauseDownloads: (paused: boolean) => invoke<void>("downloads_pause", { paused }),
  memoryAdvice: (maxMb: number, packMb?: number) => invoke<string[]>("memory_advice", { maxMb, packMb: packMb ?? null }),
  launchSettings: (id: string) =>
    invoke<{ resolved: ResolvedLaunch; profile: LaunchOverrides; global: LaunchOverrides; total_memory_mb?: number }>("launch_settings", { id }),
  globalSettings: () => invoke<GlobalSettings>("global_settings"),
  saveGlobalSettings: (settings: GlobalSettings) => invoke<void>("save_global_settings", { settings }),
  listVersions: (snapshots: boolean, old: boolean) =>
    invoke<{ versions: VersionEntry[]; latest_release: string; offline: boolean }>("list_versions", { snapshots, old }),
  versionInfo: (id: string) => invoke<{ java_major: number; java_reason: string }>("version_info", { id }),
  loaderVersions: (loader: LoaderKind, gameVersion: string) => invoke<LoaderVersion[]>("loader_versions", { loader, gameVersion }),
  launch: (id: string, quickPlay?: QuickPlay, fullVerify = false, safeMode = false) =>
    invoke<void>("launch_instance", { id, quickPlay: quickPlay ?? null, fullVerify, safeMode }),
  stop: (id: string) => invoke<boolean>("stop_instance", { id }),
  previewCommand: (id: string) => invoke<string>("preview_command", { id }),
  openFolder: (id: string, sub?: string) => invoke<void>("open_instance_folder", { id, sub: sub ?? null }),
  readLog: (id: string) => invoke<string[]>("read_log_file", { id, file: null }),
  listAccounts: () => invoke<AccountRef[]>("list_accounts"),
  removeAccount: (key: string) => invoke<void>("remove_account", { key }),
  accountAvatar: (key: string) => invoke<string | null>("account_avatar", { key }),
  accountSkin: (key: string) => invoke<string | null>("account_skin", { key }),
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
  profile: t("профиль"), pack: t("сборка"), global: t("глобально"), default: t("по умолчанию"),
};

export function relTime(unix: number): string {
  if (!unix) return "—";
  const d = new Date(unix * 1000);
  const now = new Date();
  const days = Math.floor((new Date(now.toDateString()).getTime() - new Date(d.toDateString()).getTime()) / 86_400_000);
  const hm = d.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
  if (days === 0) return t("сегодня, {0}", hm);
  if (days === 1) return t("вчера, {0}", hm);
  if (days < 7) return t("{0} дн. назад", days);
  return d.toLocaleDateString("ru-RU", { day: "2-digit", month: "2-digit" });
}

export function duration(secs: number): string {
  if (secs < 60) return t("{0} с", secs);
  const m = Math.floor(secs / 60);
  if (m < 60) return t("{0} мин", m);
  return t("{0} ч {1} мин", Math.floor(m / 60), m % 60);
}

export function mb(bytes: number): string {
  return (bytes / 1_048_576).toFixed(1).replace(".", ",");
}

/** Статус сборки по последнему настоящему запуску (разбор игрока, п. 4). */
export function statusOf(i: Instance, phase?: "preparing" | "running" | "done" | "failed"): { text: string; cls: "ok" | "warn" | "err" | "" } {
  if (phase === "preparing") return { text: t("подготовка…"), cls: "warn" };
  if (phase === "running") return { text: t("игра запущена"), cls: "ok" };
  const l = i.last_launch;
  if (!l) return { text: t("ещё не запускался"), cls: "warn" };
  switch (l.outcome) {
    case "success": return { text: t("запуск успешен · {0}", relTime(l.at)), cls: "ok" };
    case "killed": return { text: t("остановлен · {0}", relTime(l.at)), cls: "" };
    case "crashed": return { text: t("упал · {0}", relTime(l.at)), cls: "err" };
    case "failed_to_start": return { text: t("не запустился · {0}", relTime(l.at)), cls: "err" };
  }
}

// ---------- Моды, каталог, снимки, импорт ----------

export type Side = "client" | "server" | "both" | "unknown";
export interface ModMeta { loader: string; id: string; name: string; version: string; side: Side; dependencies: { id: string; range: string; kind: string }[]; mixins: string[] }
export interface LockEntry { path: string; sha1: string; source: "modrinth" | "curseforge" | "import" | "manual"; project_id?: string; version_number?: string; title?: string; layer: "base" | "user"; pinned?: boolean }
export interface ModFile { file_name: string; rel_path: string; enabled: boolean; size: number; sha1: string; mods: ModMeta[]; read_error?: string; source?: LockEntry; world_content: boolean }
export type ModState = "ok" | "warning" | "blocked" | "disabled" | "unknown";
export type Fix = { kind: "install"; mod_id: string } | { kind: "disable"; file_name: string } | { kind: "keep_one"; file_names: string[] };
export interface Finding { severity: "blocking" | "warning"; title: string; detail: string; evidence: string; files: string[]; fixes: Fix[] }
export interface DoctorReport { findings: Finding[]; mods: { file_name: string; state: ModState; state_label: string; notes: string[] }[]; total: number; ok: number; warnings: number; blocked: number; can_launch: boolean }
export interface JournalEntry { at: number; action: string; what: string; reason: string; snapshot?: string }
export interface Change { kind: "add" | "update" | "remove"; title: string; file_name?: string; old_file?: string; old_version?: string; new_version?: string; reason: string; size: number; version?: unknown }
export interface Plan { changes: Change[]; notes: string[] }
export interface SnapshotInfo { id: string; reason: string; created_at: number; pinned: boolean; auto: boolean; mods: number; configs: number }
export interface SnapshotDiff { added: string[]; removed: string[]; changed: string[]; options: [string, string, string][] }
export interface SearchHit { project_id: string; slug: string; title: string; description: string; downloads: number; icon_url?: string; author: string; client_side: string; categories: string[] }
export type Channel = "auto" | "release" | "beta" | "alpha";
export type CatalogSource = "modrinth" | "curseforge";
export type ProjectKind = "mod" | "resourcepack" | "shader" | "datapack";
export interface ImportResult { instance: Instance; manual: { project_id: number; file_id: number; url: string }[]; notes: string[] }
export type CrashAction = { kind: "disable_mod"; file_name: string } | { kind: "increase_memory" } | { kind: "update_drivers" } | { kind: "open_mod_doctor" } | { kind: "safe_launch" };
export interface CrashAnalysis { title: string; explanation: string; confidence: "high" | "medium" | "low"; confidence_label: string; evidence: string[]; suspects: string[]; actions: CrashAction[] }

export const mods = {
  view: (id: string) => invoke<{ files: ModFile[]; report: DoctorReport; journal: JournalEntry[] }>("mods_view", { id }),
  toggle: (id: string, file: string, enabled: boolean) => invoke<void>("mods_toggle", { id, file, enabled }),
  remove: (id: string, file: string) => invoke<SnapshotInfo>("mods_remove", { id, file }),
  pin: (id: string, file: string, pinned: boolean) => invoke<void>("mods_pin", { id, file, pinned }),
  addFiles: (id: string, paths: string[]) => invoke<{ file_name: string; title: string; known: boolean }[]>("mods_add_files", { id, paths }),
  search: (id: string, query: string, kind: ProjectKind, offset = 0, category = "", source: CatalogSource = "modrinth") =>
    invoke<{ hits: SearchHit[]; total_hits: number }>("catalog_search", { id, query, kind, offset, category: category || null, source }),
  bisectStatus: (id: string) => invoke<Bisect | null>("bisect_status", { id }),
  bisectStep: (id: string, action: "start" | "answer" | "cancel", crashed?: boolean) =>
    invoke<{ kind: "next"; state: Bisect } | { kind: "found"; file: string } | null>("bisect_step", { id, action, crashed: crashed ?? null }),
  groups: (id: string) => invoke<{ names: string[]; groups: Record<string, string> }>("mods_groups", { id }),
  setGroup: (id: string, file: string, group: string | null) => invoke<void>("mods_set_group", { id, file, group }),
  toggleGroup: (id: string, group: string, enabled: boolean) => invoke<string[]>("mods_toggle_group", { id, group, enabled }),
  autoGroups: (id: string) => invoke<number>("mods_auto_groups", { id }),
  planInstall: (id: string, project: string, channel: Channel = "auto", source: CatalogSource = "modrinth") =>
    invoke<Plan>("mods_plan_install", { id, project, channel, source }),
  checkUpdates: (id: string) => invoke<Plan>("mods_check_updates", { id }),
  apply: (id: string, plan: Plan, reason: string) => invoke<SnapshotInfo>("mods_apply", { id, plan, reason }),
};

export const snapshots = {
  list: (id: string) => invoke<SnapshotInfo[]>("snapshots_list", { id }),
  create: (id: string, reason: string) => invoke<SnapshotInfo>("snapshot_create", { id, reason }),
  diff: (id: string, snapshot: string) => invoke<SnapshotDiff>("snapshot_diff", { id, snapshot }),
  restore: (id: string, snapshot: string) => invoke<SnapshotInfo>("snapshot_restore", { id, snapshot }),
  pin: (id: string, snapshot: string, pinned: boolean) => invoke<void>("snapshot_pin", { id, snapshot, pinned }),
  worlds: (id: string) => invoke<WorldInfo[]>("worlds_list", { id }),
  backupWorld: (id: string, world: string) => invoke<WorldBackup>("world_backup", { id, world }),
  restoreWorld: (id: string, file: string) => invoke<string>("world_restore", { id, file }),
};

export const importer = {
  sources: () => invoke<{ prism: [string, string][]; official?: string }>("import_sources"),
  path: (path: string, skipOptional: string[] = [], projectId?: string, versionId?: string) =>
    invoke<ImportResult>("import_path", { path, skipOptional, projectId: projectId ?? null, versionId: versionId ?? null }),
  searchPacks: (query: string) => invoke<{ hits: SearchHit[]; total_hits: number }>("modpack_search", { query, offset: 0 }),
  download: (projectId: string, versionId?: string, channel?: Channel) =>
    invoke<PackVersion>("pack_download", { projectId, versionId: versionId ?? null, channel: channel ?? null }),
  check: (id: string) => invoke<PackVersion | null>("pack_check", { id }),
  settings: (id: string, channel: Channel, pinned: boolean) => invoke<Instance>("pack_settings", { id, channel, pinned }),
  preview: (path: string) => invoke<{ name: string; version: string; game: string; loader: string; optional: string[] }>("mrpack_preview", { path }),
  updatePlan: (id: string, path: string) => invoke<PackUpdatePlan>("pack_update_plan", { id, path }),
  exportMrpack: (id: string, path: string, name: string, version: string) =>
    invoke<{ linked: number; bundled: string[]; report_path: string }>("export_mrpack", { id, path, name, version }),
  updateApply: (id: string, path: string, skipOptional: string[], versionId?: string) =>
    invoke<SnapshotInfo>("pack_update_apply", { id, path, skipOptional, versionId: versionId ?? null }),
  link: (path: string, name: string, gameVersion: string, loader: LoaderKind) => invoke<ImportResult>("link_minecraft", { path, name, gameVersion, loader }),
};

export const sizeText = (b: number) => (b > 1_048_576 ? t("{0} МБ", mb(b)) : t("{0} КБ", Math.max(1, Math.round(b / 1024))));
