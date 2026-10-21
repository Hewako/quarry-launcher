// Предпросмотр интерфейса в обычном браузере (только для разработки и визуальной проверки):
// IPC Tauri подменяется через официальный @tauri-apps/api/mocks тестовыми данными.
// Файл не входит в сборку приложения (vite build собирает только index.html).
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import React from "react";
import ReactDOM from "react-dom/client";
import type { Instance } from "./api";
import App from "./App";
import "./styles.css";

const now = Math.floor(Date.now() / 1000);
const instances: Instance[] = [
  { id: "tech", name: "Технология+", game_version: "26.3", loader: "fabric", loader_version: "0.19.5", created_at: now - 864000, favorite: true, group: "С друзьями", color: "#3b7dd8", auth_mode: "microsoft", settings: { max_memory_mb: 6144 }, pack_settings: {}, last_launch: { at: now - 86400, outcome: "success", duration_secs: 8040 } },
  { id: "shaders", name: "Шейдеры", game_version: "1.21.11", loader: "quilt", loader_version: "0.29.2", created_at: now - 864000, auth_mode: "microsoft", settings: {}, pack_settings: {}, last_launch: { at: now - 3600, outcome: "crashed", exit_code: -1, duration_secs: 31, message: "игра завершилась с кодом -1" } },
  { id: "vanilla", name: "Ванилла 26.3", game_version: "26.3", loader: "vanilla", created_at: now - 864000, auth_mode: "microsoft", settings: {}, pack_settings: {} },
  { id: "old", name: "Хардкор 1.16.5", game_version: "1.16.5", loader: "vanilla", created_at: now - 864000, auth_mode: "microsoft", settings: {}, pack_settings: {}, last_launch: { at: now - 3 * 86400, outcome: "failed_to_start", duration_secs: 0, message: "Нет аккаунта Microsoft. Войдите на экране «Аккаунты»" } },
];

const params = new URLSearchParams(location.search);
const withAccount = params.get("account") !== "none";

mockIPC((cmd, args) => {
  const a = args as Record<string, unknown>;
  switch (cmd) {
    case "app_info": return { curseforge: true, version: "0.1.0", dev_launch: params.has("dev"), data_dir: "/Users/player/.quarry", microsoft_configured: params.has("ms"), os: "macos" };
    case "list_instances": return { instances, broken: [], running: [] };
    case "update_instance": return { ...instances.find((i) => i.id === a.id)!, ...((a.meta as object) ?? {}) };
    case "resources_list": if (a.kind === "datapack") return { game_format: 61, shader_loader: true, packs: [{ file_name: "Towers.zip", description: "Башни в мире", size: 340_000, is_dir: false, enabled: false, formats: [61, 61], compatible: true }] };
      return a.kind === "resourcepack"
      ? { game_format: 75, shader_loader: true, packs: [
        { file_name: "Faithful 32x.zip", description: "Те же текстуры, в два раза чётче", size: 9_400_000, is_dir: false, enabled: true, formats: [75, 75], compatible: true },
        { file_name: "Fresh Animations.zip", description: "Живые анимации мобов", size: 2_100_000, is_dir: false, enabled: true, formats: [70, 75], compatible: true },
        { file_name: "Old Pack.zip", description: "Для 1.20", size: 800_000, is_dir: false, enabled: false, formats: [15, 15], compatible: false },
      ] }
      : { shader_loader: true, packs: [
        { file_name: "ComplementaryReimagined.zip", description: "", size: 1_200_000, is_dir: false, enabled: true },
        { file_name: "BSL.zip", description: "", size: 900_000, is_dir: false, enabled: false },
      ] };
    case "mods_groups": return { names: ["Оптимизация", "Визуал", "Удобство", "Серверные", "Разработка"], groups: { "sodium.jar": "Оптимизация", "iris.jar": "Визуал", "indium.jar": "Визуал" } };
    case "configs_list": return [{ path: "config/iris.properties", size: 120, user_edited: false }, { path: "config/sodium-options.json", size: 840, user_edited: true }, { path: "options.txt", size: 4200, user_edited: false }];
    case "configs_search": return [{ path: "config/iris.properties", line: 2, text: "shaderPack=BSL.zip" }];
    case "config_read": return "{\n  \"quality\": {\n    \"weather_quality\": \"FAST\"\n  }\n}\n";
    case "config_history": return [{ at: Date.now() - 3600_000, size: 830 }];
    case "config_preview": return { diff: [{ kind: "=", text: "{" }, { kind: "-", text: "  \"fps\": 60" }, { kind: "+", text: "  \"fps\": 144" }, { kind: "=", text: "}" }] };
    case "memory_advice": return Number(a.maxMb) > 12_000 ? ["Отдано 14 из 16 ГБ памяти компьютера — системе и браузеру может не хватить, игра начнёт тормозить"] : [];
    case "options_view": return {
      entries: [["version", "4671"], ["renderDistance", "12"], ["simulationDistance", "8"], ["maxFps", "120"], ["fov", "0.25"], ["gamma", "0.5"], ["guiScale", "0"], ["enableVsync", "true"], ["fullscreen", "false"],
        ["soundCategory_master", "1.0"], ["soundCategory_music", "0.3"], ["key_key.jump", "key.keyboard.space"], ["key_key.drop", "key.keyboard.q"], ["key_key.sprint", "key.keyboard.q"], ["key_key.attack", "key.mouse.left"]],
      conflicts: [{ key: "key.keyboard.q", actions: ["key.drop", "key.sprint"] }],
      presets: [{ name: "Слабый ноутбук", category: "graphics", count: 6 }],
    };
    case "storage_overview": return [{ label: "Профили (миры, моды, настройки)", size: 6_200_000_000, cleanable: false }, { label: "Библиотеки и ресурсы игры (общие для профилей)", size: 1_900_000_000, cleanable: false }, { label: "Временные файлы и кеш", size: 240_000_000, cleanable: true }];
    case "cache_clean": return 240_000_000;
    case "crash_reports": return [{ file: "crash-2026-10-21_09.30.00-client.txt", when: "2026-10-21 09:30:00", size: 18_000, description: "Rendering overlay" }];
    case "support_report_preview": return [{ name: "summary.txt", content: "Отчёт поддержки Quarry\n\nQuarry: 0.1.0\nВерсия игры: 26.3" }, { name: "mods.txt", content: "[вкл] Sodium 0.8.14 (sodium.jar)" }];
    case "first_run_info": return { data_dir: "/Users/player/.quarry", free_bytes: 84_000_000_000, javas: [{ path: "/usr/bin/java", major: 21, version: "21.0.4", arch: "aarch64" }] };
    case "first_run_done": return null;
    case "servers_list": return [{ name: "Сервер с друзьями", ip: "mc.example.org" }, { name: "Локальный", ip: "127.0.0.1:25566" }];
    case "server_ping": return a.address === "mc.example.org"
      ? { version: "Paper 26.3", protocol: 800, online: 7, max: 50, motd: "Добро пожаловать!\nВыживание с друзьями", latency_ms: 38 }
      : Promise.reject("Сервер не ответил: Connection refused");
    case "servers_save": return null;
    case "disk_usage": return { total: 1_480_000_000, parts: [{ label: "Миры", size: 820_000_000 }, { label: "Моды", size: 410_000_000 }, { label: "Ресурспаки и шейдеры", size: 160_000_000 }, { label: "Снимки модов", size: 70_000_000 }, { label: "Скриншоты", size: 20_000_000 }] };
    case "list_accounts": return withAccount ? [{ provider: "microsoft", uuid: "0123", name: "Steve_Builder", owns_java_edition: true, checked_at: now }] : [];
    case "global_settings": return { launch: {}, theme: params.get("theme") ?? "dark", streamer_mode: false, language: "ru", ui_scale: 100, compact: false, first_run_done: !params.has("firstrun"), backup_before_risky_launch: true, backup_after_exit: false, backup_every_days: 0, download_limit_kbps: 0, check_launcher_updates: true };
    case "save_global_settings": return null;
    case "launch_settings": {
      const i = instances.find((x) => x.id === a.id)!;
      return {
        resolved: {
          max_memory_mb: { value: i.settings.max_memory_mb ?? 4096, source: i.settings.max_memory_mb ? "profile" : "default" },
          min_memory_mb: { value: i.settings.max_memory_mb ?? 4096, source: "default" },
          java_path: { value: null, source: "default" }, jvm_args: { value: [], source: "default" }, resolution: { value: null, source: "default" },
        },
        profile: i.settings, global: {}, total_memory_mb: 16384,
      };
    }
    case "list_versions": return { versions: [{ id: "26.3", type: "release", releaseTime: "2026-10-14T10:00:00+00:00" }, { id: "26.2", type: "release", releaseTime: "2026-07-01T10:00:00+00:00" }, { id: "1.21.11", type: "release", releaseTime: "2025-12-09T10:00:00+00:00" }], latest_release: "26.3", offline: false };
    case "version_info": return { java_major: 25, java_reason: "javaVersion.majorVersion = 25 в JSON версии" };
    case "loader_versions": return [{ version: "0.20.1", stable: false }, { version: "0.19.5", stable: true }];
    case "read_log_file": return ["[13:47:02] [main/INFO]: Loading Minecraft 1.21.11 with Quilt Loader 0.29.2", "[13:47:03] [main/WARN]: Mod distanthorizons recommends sodium >=0.7.3", "[13:47:04] [main/INFO]: Game directory: ‹HOME›/.quarry/instances/shaders/game", "[13:47:33] [Render thread/ERROR]: Mixin apply for mod indium failed", "java.lang.RuntimeException: boom", "\tat a.b(C.java:1)"];
    case "launch_instance": {
      const id = a.id as string;
      setTimeout(async () => {
        await emit("launch-event", { id, event: { type: "stage", stage: "assets", label: "Ресурсы игры" } });
        await emit("launch-event", { id, event: { type: "progress", stage: "assets", progress: { files_total: 4231, files_done: 1730, bytes_total: 680_000_000, bytes_done: 270_000_000, retries: 0 } } });
        await emit("launch-event", { id, event: { type: "java", label: "Eclipse Adoptium 25.0.4.1 (aarch64)", reason: "нужна Java 25 (javaVersion.majorVersion = 25 в JSON версии) — найдена Eclipse Adoptium 25.0.4.1 (aarch64)" } });
      }, 50);
      return null;
    }
    case "mods_view": {
      const fm = (id: string, name: string, version: string, side: string) => [{ loader: "fabric", id, name, version, side, dependencies: [], mixins: [] }];
      const files = [
        { file_name: "sodium.jar", rel_path: "mods/sodium.jar", enabled: true, size: 1, sha1: "a", mods: fm("sodium", "Sodium", "0.8.14", "client"), source: { path: "mods/sodium.jar", sha1: "a", source: "modrinth", layer: "base" } },
        { file_name: "indium.jar", rel_path: "mods/indium.jar", enabled: true, size: 1, sha1: "b", mods: fm("indium", "Indium", "1.0.30", "client"), source: { path: "mods/indium.jar", sha1: "b", source: "modrinth", layer: "user" } },
        { file_name: "iris.jar", rel_path: "mods/iris.jar", enabled: true, size: 1, sha1: "c", mods: fm("iris", "Iris Shaders", "1.10.8", "client"), source: { path: "mods/iris.jar", sha1: "c", source: "modrinth", layer: "base" } },
        { file_name: "old.jar.disabled", rel_path: "mods/old.jar.disabled", enabled: false, size: 1, sha1: "d", mods: [], read_error: "в файле нет метаданных мода" },
      ];
      return {
        files,
        report: {
          findings: [{ severity: "blocking", title: "Indium требует sodium <0.6, а стоит 0.8.14", detail: "Версии не подходят друг к другу — загрузчик остановит запуск.", evidence: "fabric.mod.json: зависимость sodium <0.6", files: ["indium.jar"], fixes: [{ kind: "disable", file_name: "indium.jar" }] }],
          mods: [
            { file_name: "sodium.jar", state: "ok", state_label: "конфликтов не найдено", notes: [] },
            { file_name: "indium.jar", state: "blocked", state_label: "блокирует запуск", notes: ["Indium требует sodium <0.6, а стоит 0.8.14"] },
            { file_name: "iris.jar", state: "ok", state_label: "конфликтов не найдено", notes: [] },
            { file_name: "old.jar", state: "disabled", state_label: "выключен", notes: [] },
          ],
          total: 4, ok: 2, warnings: 0, blocked: 1, can_launch: false,
        },
        journal: [{ at: now - 3600, action: "Установлено", what: "Iris Shaders 1.10.8", reason: "выбран вами", snapshot: "s1" }],
      };
    }
    case "snapshots_list": return [
      { id: "s2", reason: "Перед установкой Distant Horizons", created_at: now - 3600, pinned: false, auto: true, mods: 44, configs: 121 },
      { id: "s1", reason: "Чистая сборка 1.8", created_at: now - 9 * 86400, pinned: true, auto: false, mods: 42, configs: 118 },
    ];
    case "snapshot_diff": return { added: ["mods/DistantHorizons-2.3.4.jar"], removed: [], changed: [], options: [["renderDistance", "12", "16"]] };
    case "worlds_list": return [
      { folder: "New World", name: "База у реки", version_name: "26.3", data_version: 5023, game_mode: "выживание", hardcore: false, last_played: now - 3600, size: 21_000_000,
        backups: [{ world: "New World", file: "/b/New World-1.zip", size: 18_000_000, created_at: now - 7200 }, { world: "New World", file: "/b/New World-2.zip", size: 17_500_000, created_at: now - 86400 * 2 }] },
      { folder: "Creative", name: "Творческий", version_name: "26.4-snapshot-2", data_version: 5101, game_mode: "творчество", hardcore: true, last_played: now - 86400 * 9, size: 4_200_000, backups: [] },
    ];
    case "world_restore": return "New World";
    case "import_sources": return { prism: [["/Users/player/Library/Application Support/PrismLauncher/instances/Tech", "Технология+ (1.21.11 · Fabric)"]], official: "/Users/player/Library/Application Support/minecraft" };
    case "mods_check_updates": return { changes: [{ kind: "update", title: "Sodium", old_version: "0.8.12", new_version: "0.8.14", reason: "новая версия", size: 1914874 }], notes: ["Моды, которых нет на Modrinth, проверить на обновления нельзя"] };
    case "catalog_search": return { hits: [{ project_id: "AANobbMI", slug: "sodium", title: "Sodium", description: "Современный движок отрисовки, заметно поднимает FPS", downloads: 250585660, author: "jellysquid3", client_side: "required", categories: [] }], total_hits: 1 };
    default: return null;
  }
}, { shouldMockEvents: true });

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
