// Перевод интерфейса. Исходные строки — русские; t() ищет их перевод в словаре языка.
// Язык читается при запуске (кеш в localStorage, источник — настройки лаунчера); при смене
// языка окно перезагружается, поэтому строки в константах модулей тоже переводятся.
import { EN } from "./i18n.en";

const KEY = "quarry.lang";

function initial(): string {
  try { return localStorage.getItem(KEY) ?? "ru"; } catch { return "ru"; }
}

let lang = initial();

export const currentLang = () => lang;

/** Сменить язык: запомнить и перезагрузить окно. */
export function switchLang(next: string) {
  if (next === lang) return;
  try { localStorage.setItem(KEY, next); } catch { /* не страшно */ }
  location.reload();
}

/** Синхронизация с настройками лаунчера (если кеш пуст или устарел). */
export function syncLang(fromSettings: string) {
  if (fromSettings && fromSettings !== lang) switchLang(fromSettings);
}

/** Перевести строку. `{0}`, `{1}` … заменяются аргументами. Нет перевода — русский текст. */
export function t(ru: string, ...args: unknown[]): string {
  const s = lang === "en" ? EN[ru] ?? ru : ru;
  return args.length ? s.replace(/\{(\d+)\}/g, (_, i) => String(args[Number(i)] ?? "")) : s;
}
