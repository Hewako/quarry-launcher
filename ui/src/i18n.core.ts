// Перевод готовых сообщений ядра по шаблонам: «Профиль «{0}» не найден» ловит любое имя
// профиля, а вложенные русские части (причины, подсказки) переводятся тем же способом.
// Без импортов — его напрямую проверяет scripts/i18n-check.mjs.
const CYR = /[А-Яа-яЁё]/;
const HOLE = /\{\d+\}/g;
/** Хвост, который ядро дописывает к другому сообщению: «; …», « — …», « (…)». */
const TAIL = /^[\s;,(—]/;
const esc = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

export function coreTranslator(dict: Record<string, string>): (ru: string) => string {
  // Сначала самые длинные шаблоны: «{0} пока не поддерживается — …» раньше «{0} пока не поддерживается».
  const rules = Object.entries(dict)
    .filter(([k]) => /\{\d+\}/.test(k) || TAIL.test(k))
    .sort(([a], [b]) => b.replace(HOLE, "").length - a.replace(HOLE, "").length)
    .map(([k, v]) => {
      const tail = TAIL.test(k);
      const body = k.split(HOLE).map(esc).join("([\\s\\S]*?)");
      return [new RegExp(`^${tail ? "([\\s\\S]+?)" : ""}${body}$`), v, tail] as const;
    });
  const cache = new Map<string, string>();
  const tr = (ru: string): string => {
    if (!CYR.test(ru)) return ru;
    let out = cache.get(ru) ?? dict[ru];
    if (out === undefined) {
      for (const [re, en, tail] of rules) {
        const m = re.exec(ru);
        if (!m) continue;
        const args = m.slice(tail ? 2 : 1);
        out = (tail ? tr(m[1]) : "") + en.replace(HOLE, (h) => tr(args[Number(h.slice(1, -1))] ?? ""));
        break;
      }
    }
    out ??= ru.includes("\n") ? ru.split("\n").map(tr).join("\n") : ru;
    if (cache.size > 5000) cache.clear();
    cache.set(ru, out);
    return out;
  };
  return tr;
}
