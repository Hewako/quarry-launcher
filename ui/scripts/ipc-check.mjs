// Проверка связки интерфейса с Rust: каждый invoke("команда", { … }) должен звать
// зарегистрированную команду с теми же аргументами (Tauri превращает maxMb в max_mb).
// Предпросмотр в браузере такие ошибки не ловит — в настоящем приложении ломается кнопка.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const noComments = (s) => s.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/[^\n]*/g, "");
/** Разбить по запятым верхнего уровня (не внутри <…>, {…}, (…), […]). */
const split = (s) => {
  const out = [];
  let depth = 0, cur = "";
  for (const c of s) {
    if ("<{([".includes(c)) depth++;
    if (">})]".includes(c)) depth--;
    if (c === "," && depth === 0) { out.push(cur); cur = ""; } else cur += c;
  }
  out.push(cur);
  return out.map((x) => x.trim()).filter(Boolean);
};
/** Содержимое скобок, открытых на позиции i. */
const group = (s, i) => {
  const open = s[i], close = { "(": ")", "{": "}" }[open];
  for (let j = i, d = 0; j < s.length; j++) {
    if (s[j] === open) d++;
    else if (s[j] === close && --d === 0) return s.slice(i + 1, j);
  }
  return "";
};
const snake = (s) => s.replace(/[A-Z]/g, (c) => "_" + c.toLowerCase());

const rs = noComments(readFileSync("src-tauri/src/lib.rs", "utf8"));
const cmds = new Map();
for (const m of rs.matchAll(/#\[tauri::command[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)\s*(?:<[^>]*>)?\(/g)) {
  const params = split(group(rs, m.index + m[0].length - 1))
    .filter((p) => !/:\s*(tauri::)?(State|AppHandle|Window|WebviewWindow)\b/.test(p))
    .map((p) => p.split(":")[0].replace(/^mut\s+/, "").trim());
  cmds.set(m[1], params);
}
const handler = new Set(split(rs.match(/generate_handler!\[([\s\S]*?)\]/)?.[1] ?? "").map((s) => s.split("::").pop()));
const problems = [];
for (const c of cmds.keys()) if (!handler.has(c)) problems.push(`команда ${c} не зарегистрирована в generate_handler!`);

const walk = (d) => readdirSync(d).flatMap((f) => {
  const p = join(d, f);
  return statSync(p).isDirectory() ? walk(p) : /\.tsx?$/.test(f) && !f.startsWith("preview") ? [p] : [];
});
let calls = 0;
for (const file of walk("src")) {
  const src = noComments(readFileSync(file, "utf8"));
  for (const m of src.matchAll(/invoke(?:<[^(]*?>)?\(\s*"(\w+)"\s*(,\s*)?/g)) {
    calls++;
    const name = m[1];
    const want = cmds.get(name);
    if (!want) { problems.push(`${file}: invoke("${name}") — такой команды нет`); continue; }
    const at = m.index + m[0].length;
    const keys = !m[2] ? [] : src[at] === "{" ? split(group(src, at)).map((s) => snake(s.split(":")[0].trim())) : ["‹не объект›"];
    for (const k of keys) if (!want.includes(k)) problems.push(`${file}: ${name} — лишний аргумент ${k} (ждёт: ${want.join(", ") || "ничего"})`);
    for (const w of want) if (!keys.includes(w)) problems.push(`${file}: ${name} — не передан ${w}`);
  }
}
if (problems.length) {
  console.error(`Связка интерфейса с Rust сломана (${problems.length}):\n  ${problems.join("\n  ")}`);
  process.exit(1);
}
console.log(`Связка интерфейса с Rust в порядке: ${cmds.size} команд, ${calls} вызовов.`);
