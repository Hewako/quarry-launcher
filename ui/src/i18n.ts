// Перевод интерфейса. Исходные строки — русские; t() ищет их перевод в словаре языка.
// Язык читается при запуске (кеш в localStorage, источник — настройки лаунчера); при смене
// языка окно перезагружается, поэтому строки в константах модулей тоже переводятся.
import { EN } from "./i18n.en";
import { CORE_EN } from "./i18n.core.en";
import { coreTranslator } from "./i18n.core";

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

const core = lang === "en" ? coreTranslator({ ...EN, ...CORE_EN }) : (s: string) => s;

/** Русский исходник с подстановками — для того, что хранится (уведомления) и переводится при показе через tc(). */
export function src(ru: string, ...args: unknown[]): string {
  return args.length ? ru.replace(/\{(\d+)\}/g, (_, i) => String(args[Number(i)] ?? "")) : ru;
}

/** Перевести сообщение ядра или сохранённый исходник: ошибку, предупреждение, находку Mod Doctor, уведомление. */
export function tc(ru: string): string;
export function tc(ru: string | undefined): string | undefined;
export function tc(ru?: string) {
  return ru && core(ru);
}
