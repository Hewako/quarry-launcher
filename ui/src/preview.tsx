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
  { id: "tech", name: "Технология+", game_version: "26.3", loader: "fabric", loader_version: "0.19.5", created_at: now - 864000, auth_mode: "microsoft", settings: { max_memory_mb: 6144 }, pack_settings: {}, last_launch: { at: now - 86400, outcome: "success", duration_secs: 8040 } },
  { id: "shaders", name: "Шейдеры", game_version: "1.21.11", loader: "quilt", loader_version: "0.29.2", created_at: now - 864000, auth_mode: "microsoft", settings: {}, pack_settings: {}, last_launch: { at: now - 3600, outcome: "crashed", exit_code: -1, duration_secs: 31, message: "игра завершилась с кодом -1" } },
  { id: "vanilla", name: "Ванилла 26.3", game_version: "26.3", loader: "vanilla", created_at: now - 864000, auth_mode: "microsoft", settings: {}, pack_settings: {} },
  { id: "old", name: "Хардкор 1.16.5", game_version: "1.16.5", loader: "vanilla", created_at: now - 864000, auth_mode: "microsoft", settings: {}, pack_settings: {}, last_launch: { at: now - 3 * 86400, outcome: "failed_to_start", duration_secs: 0, message: "Нет аккаунта Microsoft. Войдите на экране «Аккаунты»" } },
];

const params = new URLSearchParams(location.search);
const withAccount = params.get("account") !== "none";

mockIPC((cmd, args) => {
  const a = args as Record<string, unknown>;
  switch (cmd) {
    case "app_info": return { version: "0.1.0", dev_launch: params.has("dev"), data_dir: "/Users/player/.quarry", microsoft_configured: params.has("ms"), os: "macos" };
    case "list_instances": return { instances, broken: [], running: [] };
    case "list_accounts": return withAccount ? [{ provider: "microsoft", uuid: "0123", name: "Steve_Builder", owns_java_edition: true, checked_at: now }] : [];
    case "global_settings": return { launch: {}, theme: params.get("theme") ?? "dark", streamer_mode: false };
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
    default: return null;
  }
}, { shouldMockEvents: true });

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
