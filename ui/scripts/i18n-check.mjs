// Проверка перевода: каждая строка в t("…") должна быть в словаре i18n.en.ts, а каждое
// русское сообщение ядра (строки Rust с кириллицей) — в i18n.core.en.ts.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const CYR = /[А-Яа-яЁё]/;
const dictKeys = (file) => {
  const map = new Map();
  for (const m of readFileSync(file, "utf8").matchAll(/^\s*("(?:[^"\\]|\\.)*"):\s*("(?:[^"\\]|\\.)*")/gm)) map.set(JSON.parse(m[1]), JSON.parse(m[2]));
  return map;
};
const keys = dictKeys("src/i18n.en.ts");
const missing = new Map();
function walk(dir) {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) walk(p);
    else if (/\.tsx?$/.test(f) && !/^(i18n|preview)/.test(f)) {
      for (const m of readFileSync(p, "utf8").matchAll(/\bt\(\s*("(?:[^"\\]|\\.)*")/g)) {
        const s = JSON.parse(m[1]);
        if (!keys.has(s)) missing.set(s, p);
      }
    }
  }
}
walk("src");

/** Строковые литералы Rust: без комментариев, символов, тестовых модулей и сообщений expect/panic. */
function rustStrings(src) {
  src = src.replace(/\r\n/g, "\n"); // на Windows git отдаёт исходники с CRLF, rustc читает их как LF
  const cut = src.search(/#\[cfg\((all\()?test\b/);
  if (cut >= 0) src = src.slice(0, cut);
  const out = [];
  const push = (s, at) => { if (!/(expect|panic!)\(\s*$/.test(src.slice(Math.max(0, at - 30), at))) out.push(s); };
  for (let i = 0; i < src.length; i++) {
    const c = src[i];
    if (c === "/" && src[i + 1] === "/") { i = src.indexOf("\n", i); if (i < 0) break; continue; }
    if (c === "/" && src[i + 1] === "*") { i = src.indexOf("*/", i + 2) + 1; continue; }
    if (c === "'") {
      if (src[i + 1] === "\\") i = src.indexOf("'", i + 2);
      else if (src[i + 2] === "'") i += 2;
      continue;
    }
    const raw = /^r(#*)"/.exec(src.slice(i, i + 10));
    if (raw && !/\w/.test(src[i - 1] ?? "")) {
      const end = '"' + raw[1];
      const s = i + raw[0].length, e = src.indexOf(end, s);
      push(src.slice(s, e), i); i = e + end.length - 1; continue;
    }
    if (c === '"') {
      let s = "", j = i + 1;
      for (; src[j] !== '"'; j++) {
        if (src[j] !== "\\") { s += src[j]; continue; }
        const n = src[++j];
        if (n === "\n") { while (/\s/.test(src[j + 1])) j++; }
        else if (n === "u") { const k = src.indexOf("}", j); s += String.fromCodePoint(parseInt(src.slice(j + 2, k), 16)); j = k; }
        else s += { n: "\n", t: "\t", r: "\r", 0: "\0" }[n] ?? n;
      }
      push(s, i); i = j;
    }
  }
  return out;
}
/** `{x}`, `{}`, `{n:.1}` → `{0}`, `{1}` по порядку; `{{`/`}}` → скобки. */
const template = (s) => { let n = 0; return s.replace(/\{\{|\}\}|\{[^{}]*\}/g, (m) => (m === "{{" ? "{" : m === "}}" ? "}" : `{${n++}}`)); };
const holes = (s) => [...s.matchAll(/\{(\d+)\}/g)].map((m) => m[1]).sort().join(",");

const core = new Map();
const walkRust = (dir) => {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) { if (f !== "bin") walkRust(p); } // bin — консольная отладочная утилита
    else if (f.endsWith(".rs")) for (const s of rustStrings(readFileSync(p, "utf8"))) if (CYR.test(s)) core.set(template(s), p);
  }
};
walkRust("../core/src");
walkRust("src-tauri/src");
JSON.stringify(JSON.parse(readFileSync("../core/rules/doctor-rules.json", "utf8")), (_, v) => { if (typeof v === "string" && CYR.test(v)) core.set(v, "doctor-rules.json"); return v; });
const coreDict = dictKeys("src/i18n.core.en.ts");
for (const [s, p] of core) if (!coreDict.has(s)) missing.set(s, p);

// Перевод по шаблонам: подстановки, вложенные русские части, свои данные игрока, многострочное.
const { coreTranslator } = await import("../src/i18n.core.ts");
const tr = coreTranslator(Object.fromEntries(coreDict));
for (const [ru, en] of [
  ["Профиль «Мой мир» не найден", "Profile “Мой мир” not found"],
  ["Не удалось разобрать ответ Modrinth: expected value", "Could not parse Modrinth reply: expected value"],
  ["{0} пока не поддерживается — такой инстанс импортировать нельзя".replace("{0}", "Forge"), "Forge is not supported yet — this instance cannot be imported"],
  ["Обновлено\nМоды", "Updated\nMods"],
  ["нужна Java 21 (21+) — найдена 17; Java для Apple Silicon не найдена — используется x64-версия через Rosetta 2", "Java 21 (21+) is needed — found 17; no Java for Apple Silicon found — using the x64 build through Rosetta 2"],
  ["Просто текст игрока", "Просто текст игрока"],
]) if (tr(ru) !== en) missing.set(ru, `i18n.core.ts: перевёл как ${JSON.stringify(tr(ru))}, ждали ${JSON.stringify(en)}`);
for (const [ru, en] of coreDict) if (holes(ru) !== holes(en)) missing.set(ru, `i18n.core.en.ts: в переводе другие {N} — ${JSON.stringify(en)}`);

if (process.argv.includes("--core-list")) { for (const s of core.keys()) console.log(JSON.stringify(s)); process.exit(0); }
if (missing.size) {
  console.error(`Нет перевода (${missing.size}):`);
  for (const [s, p] of missing) console.error(`  ${p}: ${JSON.stringify(s)}`);
  process.exit(1);
}
console.log(`Перевод полный: интерфейс ${keys.size} строк, сообщения ядра ${core.size}.`);
