// Центр уведомлений: события лаунчера копятся здесь вместо всплывающих окон.
import { useSyncExternalStore } from "react";

export interface Note { id: number; at: number; kind: "info" | "warn" | "err"; title: string; body?: string; profile?: string }

const KEY = "quarry.notes";
// Лента сохраняется между запусками (только у этого пользователя, в localStorage).
let notes: Note[] = (() => { try { return JSON.parse(localStorage.getItem(KEY) ?? "[]") as Note[]; } catch { return []; } })();
let unread = 0;
let seq = notes.reduce((m, n) => Math.max(m, n.id), 0);
const listeners = new Set<() => void>();
const emit = () => {
  try { localStorage.setItem(KEY, JSON.stringify(notes)); } catch { /* не страшно */ }
  listeners.forEach((l) => l());
};

export function pushNote(n: Omit<Note, "id" | "at">) {
  notes = [{ ...n, id: ++seq, at: Date.now() }, ...notes].slice(0, 50);
  unread += 1;
  emit();
}
export function markRead() { if (unread) { unread = 0; emit(); } }
export function clearNotes() { notes = []; unread = 0; emit(); }

let snapshot = { notes, unread };
export function useNotes(): { notes: Note[]; unread: number } {
  return useSyncExternalStore(
    (l) => { listeners.add(l); return () => listeners.delete(l); },
    () => (snapshot.notes === notes && snapshot.unread === unread ? snapshot : (snapshot = { notes, unread })),
  );
}
