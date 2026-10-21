// Режим стримера в интерфейсе: ник, пути и адреса серверов скрываются на экране.
import { useSyncExternalStore } from "react";
import { t } from "./i18n";

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

export const nick = (name: string | undefined, hide: boolean) => (hide && name ? t("Игрок") : name);
export const path = (p: string | undefined, hide: boolean) => (hide && p ? t("‹путь скрыт›") : p);
export const address = (ip: string, hide: boolean) => (hide ? t("‹адрес скрыт›") : ip);
