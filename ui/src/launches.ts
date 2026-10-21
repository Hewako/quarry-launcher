// Состояние запусков по профилям. Источник — события Rust `launch-event` / `launch-finished`,
// без таймеров и догадок: кнопка «Играть» разблокируется только по событию завершения.
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import type { ExitInfo, LaunchEvent, Progress } from "./api";

export interface LaunchState {
  phase: "preparing" | "running" | "done" | "failed";
  stageLabel: string;
  progress?: Progress;
  account?: string;
  javaLabel?: string;
  javaReason?: string;
  warnings: string[];
  logs: string[];
  command?: string;
  exit?: ExitInfo;
  error?: string;
  startedAt: number;
}

const MAX_LOG_LINES = 5000;
type Listener = () => void;

class LaunchStore {
  states = new Map<string, LaunchState>();
  private listeners = new Set<Listener>();
  private started = false;

  subscribe(l: Listener) {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  }
  private notify() {
    this.listeners.forEach((l) => l());
  }

  begin(id: string) {
    this.states.set(id, { phase: "preparing", stageLabel: "Подготовка", warnings: [], logs: [], startedAt: Date.now() });
    this.notify();
  }

  private update(id: string, f: (s: LaunchState) => LaunchState) {
    const cur = this.states.get(id) ?? { phase: "preparing", stageLabel: "Подготовка", warnings: [], logs: [], startedAt: Date.now() };
    this.states.set(id, f(cur));
  }

  async start() {
    if (this.started) return;
    this.started = true;
    let pending = false;
    const schedule = () => {
      if (pending) return;
      pending = true;
      // Логи приходят сотнями строк в секунду — перерисовка не чаще 10 раз в секунду.
      setTimeout(() => { pending = false; this.notify(); }, 100);
    };
    await listen<{ id: string; event: LaunchEvent }>("launch-event", ({ payload }) => {
      const { id, event: e } = payload;
      this.update(id, (s) => {
        switch (e.type) {
          case "stage": return { ...s, stageLabel: e.label, progress: undefined };
          case "progress": return { ...s, progress: e.progress };
          case "account": return { ...s, account: `${e.name} (${e.provider})` };
          case "java": return { ...s, javaLabel: e.label, javaReason: e.reason };
          case "warning": return { ...s, warnings: [...s.warnings, e.message] };
          case "started": return { ...s, phase: "running", stageLabel: "Игра запущена", command: e.command, progress: undefined };
          case "log": {
            const logs = s.logs.length >= MAX_LOG_LINES ? s.logs.slice(-MAX_LOG_LINES + 1) : s.logs.slice();
            logs.push(e.line);
            return { ...s, logs };
          }
          case "exited": return { ...s, exit: e.info, phase: "done", stageLabel: e.info.crashed ? "Игра упала" : "Игра закрыта" };
        }
      });
      schedule();
    });
    await listen<{ id: string; error?: string }>("launch-finished", ({ payload }) => {
      this.update(payload.id, (s) =>
        payload.error ? { ...s, phase: "failed", error: payload.error } : { ...s, phase: "done" },
      );
      this.notify();
    });
  }
}

export const launches = new LaunchStore();

export function useLaunches(): Map<string, LaunchState> {
  const [, force] = useState(0);
  useEffect(() => {
    const off = launches.subscribe(() => force((n) => n + 1));
    return () => { off(); };
  }, []);
  return launches.states;
}

export const isActive = (s?: LaunchState) => !!s && (s.phase === "preparing" || s.phase === "running");
