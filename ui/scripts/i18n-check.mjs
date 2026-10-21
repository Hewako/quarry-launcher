// Проверка перевода: каждая строка в t("…") должна быть в словаре i18n.en.ts.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const dict = readFileSync("src/i18n.en.ts", "utf8");
const keys = new Set([...dict.matchAll(/^\s*("(?:[^"\\]|\\.)*"):/gm)].map((m) => JSON.parse(m[1])));
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
if (missing.size) {
  console.error(`Нет перевода (${missing.size}):`);
  for (const [s, p] of missing) console.error(`  ${p}: ${JSON.stringify(s)}`);
  process.exit(1);
}
console.log(`Перевод полный: ${keys.size} строк.`);
