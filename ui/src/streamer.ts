// Режим стримера в интерфейсе: ник, пути и адреса серверов скрываются на экране.
import { useSyncExternalStore } from "react";

let on = false;
const listeners = new Set<() => void>();

export function setStreamer(v: boolean) {
  if (on === v) return;
  on = v;
  listeners.forEach((l) => l());
}

export function useStreamer(): boolean {
  return useSyncExternalStore((l) => { listeners.add(l); return () => listeners.delete(l); }, () => on);
}

export const nick = (name: string | undefined, hide: boolean) => (hide && name ? "Игрок" : name);
export const path = (p: string | undefined, hide: boolean) => (hide && p ? "‹путь скрыт›" : p);
export const address = (ip: string, hide: boolean) => (hide ? "‹адрес скрыт›" : ip);
