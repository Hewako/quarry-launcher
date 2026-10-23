// Список сторонних библиотек, которые попадают в программу, с текстами их лицензий
// (MIT, Apache и другие требуют прикладывать текст). Пишет ui/src-tauri/THIRD-PARTY-NOTICES.txt,
// который кладётся в установщики. Запуск перед релизом: `npm run licenses` (в ui/).
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ui = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const root = resolve(ui, "..");
const LICENSE_FILE = /^(licen[cs]e|copying|notice|unlicense)/i;

/** Тексты лицензий из папки пакета (LICENSE, LICENSE-MIT, COPYING, NOTICE…). */
function texts(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((f) => LICENSE_FILE.test(f))
    .sort()
    .map((f) => readFileSync(join(dir, f), "utf8").trim())
    .filter(Boolean);
}

// Rust: обычные и build-зависимости программы (без dev), без своих пакетов.
function rust() {
  const meta = JSON.parse(execFileSync("cargo", ["metadata", "--format-version", "1", "--locked", "--offline"], { cwd: root, maxBuffer: 1 << 28 }).toString());
  const byId = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const own = new Set(meta.workspace_members);
  const seen = new Set();
  const queue = [...own];
  while (queue.length) {
    const id = queue.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const d of nodes.get(id)?.deps ?? []) {
      if (d.dep_kinds.some((k) => k.kind !== "dev")) queue.push(d.pkg);
    }
  }
  return [...seen].filter((id) => !own.has(id)).map((id) => {
    const p = byId.get(id);
    return { name: p.name, version: p.version, license: p.license ?? (p.license_file ? `см. ${p.license_file}` : "?"), url: p.repository ?? p.homepage ?? "", texts: texts(dirname(p.manifest_path)) };
  });
}

// npm: зависимости интерфейса из package.json (не devDependencies) и всё, что они тянут.
function npm() {
  const out = new Map();
  const visit = (name, from) => {
    // Как ищет Node: node_modules рядом, потом выше по папкам — но не выше ui/.
    let dir = "";
    for (let d = from; d.startsWith(ui) && !dir; d = dirname(d)) {
      if (existsSync(join(d, "node_modules", name, "package.json"))) dir = join(d, "node_modules", name);
    }
    if (!dir) return;
    const p = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
    const key = `${p.name}@${p.version}`;
    if (out.has(key)) return;
    const license = typeof p.license === "string" ? p.license : p.license?.type ?? "?";
    const url = typeof p.repository === "string" ? p.repository : p.repository?.url ?? p.homepage ?? "";
    out.set(key, { name: p.name, version: p.version, license, url, texts: texts(dir) });
    for (const d of Object.keys(p.dependencies ?? {})) visit(d, dir);
  };
  const pkg = JSON.parse(readFileSync(join(ui, "package.json"), "utf8"));
  for (const d of Object.keys(pkg.dependencies ?? {})) visit(d, ui);
  return [...out.values()];
}

const all = [...rust(), ...npm()].sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));
const missing = all.filter((p) => p.texts.length === 0);
let s = "Axel' — сторонние компоненты и их лицензии\nAxel' — third-party components and their licenses\n\n";
s += "Axel' распространяется по GPL-3.0-or-later (файл LICENSE в репозитории https://github.com/Hewako/quarry-launcher).\n";
s += `В программу входят следующие сторонние библиотеки (${all.length}). Ниже — их лицензии и тексты лицензий.\n\n`;
for (const p of all) s += `- ${p.name} ${p.version} — ${p.license}${p.url ? ` — ${p.url}` : ""}\n`;
s += "\n";
// Одинаковые тексты (например, стандартный Apache-2.0) печатаются один раз, со списком пакетов.
const groups = new Map();
for (const p of all) for (const t of p.texts) {
  const g = groups.get(t) ?? [];
  g.push(`${p.name} ${p.version}`);
  groups.set(t, g);
}
for (const [t, names] of groups) s += `${"=".repeat(78)}\n${names.join(", ")}\n${"-".repeat(78)}\n${t}\n\n`;
if (missing.length) {
  s += `${"=".repeat(78)}\nБез файла лицензии в пакете — действует лицензия из списка выше; её стандартный текст приведён выше у других пакетов:\n`;
  for (const p of missing) s += `- ${p.name} ${p.version} — ${p.license}\n`;
}
writeFileSync(join(ui, "src-tauri", "THIRD-PARTY-NOTICES.txt"), s);
console.log(`Сторонних компонентов: ${all.length} (Rust и npm); без файла лицензии: ${missing.length}; уникальных текстов: ${groups.size}.`);
