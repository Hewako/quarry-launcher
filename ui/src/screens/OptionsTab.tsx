// Вкладка «Настройки игры»: options.txt без ручной правки — графика, звук, управление,
// конфликты клавиш, пресеты, импорт и экспорт раскладки.
import { open, save } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useState } from "react";
import { errText, gameOptions, type Instance, type OptCategory, type OptionsView } from "../api";
import { Modal, useDialog } from "../ui/Dialog";

type Num = { key: string; label: string; min: number; max: number; step: number; show: (v: number) => string };
const GRAPHICS: Num[] = [
  { key: "renderDistance", label: "Дальность прорисовки", min: 2, max: 32, step: 1, show: (v) => `${v} чанков` },
  { key: "simulationDistance", label: "Дальность симуляции", min: 5, max: 32, step: 1, show: (v) => `${v} чанков` },
  { key: "maxFps", label: "Ограничение FPS", min: 10, max: 260, step: 10, show: (v) => (v >= 260 ? "без ограничения" : `${v}`) },
  { key: "fov", label: "Поле зрения", min: -1, max: 1, step: 0.025, show: (v) => `${Math.round(70 + v * 40)}°` },
  { key: "gamma", label: "Яркость", min: 0, max: 1, step: 0.05, show: (v) => `${Math.round(v * 100)}%` },
  { key: "guiScale", label: "Размер интерфейса", min: 0, max: 6, step: 1, show: (v) => (v === 0 ? "авто" : `${v}`) },
];
const TOGGLES: [string, string][] = [["enableVsync", "Вертикальная синхронизация"], ["fullscreen", "Полный экран"], ["entityShadows", "Тени существ"]];
const SOUND_NAMES: Record<string, string> = {
  master: "Общая громкость", music: "Музыка", record: "Пластинки", weather: "Погода", block: "Блоки", hostile: "Враждебные",
  neutral: "Дружелюбные", player: "Игроки", ambient: "Окружение", voice: "Голос", ui: "Интерфейс",
};

/** KeyboardEvent.code / кнопка мыши → название клавиши Minecraft. */
function mcKey(e: KeyboardEvent | MouseEvent): string | null {
  if (e instanceof MouseEvent) return ["key.mouse.left", "key.mouse.middle", "key.mouse.right", "key.mouse.4", "key.mouse.5"][e.button] ?? null;
  const c = e.code;
  const special: Record<string, string> = {
    Space: "space", Tab: "tab", Enter: "enter", Backspace: "backspace", CapsLock: "caps.lock", Escape: "escape",
    ShiftLeft: "left.shift", ShiftRight: "right.shift", ControlLeft: "left.control", ControlRight: "right.control",
    AltLeft: "left.alt", AltRight: "right.alt", MetaLeft: "left.win", MetaRight: "right.win",
    ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right", Backquote: "grave.accent",
    Minus: "minus", Equal: "equal", BracketLeft: "left.bracket", BracketRight: "right.bracket", Semicolon: "semicolon",
    Quote: "apostrophe", Comma: "comma", Period: "period", Slash: "slash", Backslash: "backslash",
  };
  if (special[c]) return `key.keyboard.${special[c]}`;
  if (/^Key[A-Z]$/.test(c)) return `key.keyboard.${c.slice(3).toLowerCase()}`;
  if (/^Digit\d$/.test(c)) return `key.keyboard.${c.slice(5)}`;
  if (/^F\d{1,2}$/.test(c)) return `key.keyboard.${c.toLowerCase()}`;
  if (/^Numpad\d$/.test(c)) return `key.keyboard.keypad.${c.slice(6)}`;
  return null;
}
const keyLabel = (k: string) => {
  if (k.endsWith(".unknown")) return "не назначена";
  const m: Record<string, string> = { "key.mouse.left": "ЛКМ", "key.mouse.right": "ПКМ", "key.mouse.middle": "Колесо" };
  return m[k] ?? k.replace(/^key\.(keyboard|mouse)\./, "").replace(/\./g, " ").toUpperCase();
};

export function OptionsTab({ inst, running }: { inst: Instance; running: boolean }) {
  const dialog = useDialog();
  const [view, setView] = useState<OptionsView | null>(null);
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [capture, setCapture] = useState<string | null>(null);
  const [naming, setNaming] = useState<OptCategory | null>(null);
  const [presetName, setPresetName] = useState("");
  const [filter, setFilter] = useState("");

  const load = useCallback(() => { gameOptions.view(inst.id).then((v) => { setView(v); setDraft({}); }).catch(() => setView(null)); }, [inst.id]);
  useEffect(load, [load]);
  const values = useMemo(() => ({ ...Object.fromEntries(view?.entries ?? []), ...draft }), [view, draft]);
  const set = (k: string, v: string) => setDraft((d) => ({ ...d, [k]: v }));

  // Назначение клавиши: следующее нажатие или клик мышью.
  useEffect(() => {
    if (!capture) return;
    const onKey = (e: KeyboardEvent) => { e.preventDefault(); const k = e.code === "Escape" ? "key.keyboard.unknown" : mcKey(e); if (k) { set(capture, k); setCapture(null); } };
    const onMouse = (e: MouseEvent) => { const k = mcKey(e); if (k) { e.preventDefault(); set(capture, k); setCapture(null); } };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("mousedown", onMouse, true);
    return () => { window.removeEventListener("keydown", onKey, true); window.removeEventListener("mousedown", onMouse, true); };
  }, [capture]);

  if (!view) return <p className="muted"><span className="spinner" /> Загружаем…</p>;
  if (view.entries.length === 0) return <div className="card empty"><p>Файла настроек игры ещё нет — он появится после первого запуска этого профиля.</p></div>;

  const act = async (f: () => Promise<unknown>, ok?: string) => {
    try { await f(); load(); if (ok) await dialog.alert("Готово", ok); } catch (e) { await dialog.alert("Не получилось", errText(e)); }
  };
  const dirty = Object.keys(draft).length > 0;
  const keys = Object.entries(values).filter(([k]) => k.startsWith("key_")).sort(([a], [b]) => a.localeCompare(b));
  const conflictKeys = new Set(Object.entries(keys.reduce<Record<string, number>>((m, [, v]) => (v.endsWith(".unknown") ? m : { ...m, [v]: (m[v] ?? 0) + 1 }), {})).filter(([, n]) => n > 1).map(([k]) => k));
  const sounds = Object.keys(values).filter((k) => k.startsWith("soundCategory_"));
  const presets = (c: OptCategory) => view.presets.filter((p) => p.category === c);
  const PresetBar = ({ c }: { c: OptCategory }) => (
    <div className="row small" style={{ marginBottom: 8 }}>
      {presets(c).length > 0 && (
        <select className="field auto mini" aria-label="Применить пресет" value="" disabled={running || dirty}
          onChange={(e) => e.target.value && act(() => gameOptions.applyPreset(inst.id, e.target.value), "Пресет применён. Перед этим сохранён снимок — вернуть можно на вкладке «Снимки и миры».")}>
          <option value="">Применить пресет…</option>
          {presets(c).map((p) => <option key={p.name} value={p.name}>{p.name}</option>)}
        </select>
      )}
      <button className="btn link small" disabled={dirty} onClick={() => { setPresetName(""); setNaming(c); }}>Сохранить как пресет…</button>
    </div>
  );

  return (
    <>
      {running && <div className="alert warn" style={{ marginBottom: 12 }}><span>Игра запущена: при выходе она перезапишет настройки, поэтому сохранить можно после закрытия.</span></div>}
      {view.conflicts.length > 0 && (
        <div className="alert warn" style={{ marginBottom: 12 }}>
          <div className="h">Одна клавиша на несколько действий: {view.conflicts.length}</div>
          {view.conflicts.map((c) => <span key={c.key} className="small">{keyLabel(c.key)} — {c.actions.map((a) => a.replace(/^key\./, "")).join(", ")}</span>)}
        </div>
      )}
      <div className="row" style={{ marginBottom: 12 }}>
        <button className="btn pri" disabled={!dirty || running} onClick={() => act(() => gameOptions.set(inst.id, draft))}>Сохранить изменения</button>
        <button className="btn" disabled={!dirty} onClick={() => setDraft({})}>Отменить</button>
        <span className="muted small">Перед сохранением делается снимок — вернуть можно на вкладке «Снимки и миры».</span>
      </div>

      <h4>Графика</h4>
      <PresetBar c="graphics" />
      <div className="card">
        {GRAPHICS.filter((g) => values[g.key] !== undefined).map((g) => {
          const v = Number(values[g.key]);
          return (
            <div className="setting" key={g.key}>
              <div className="lbl">{g.label}</div>
              <input type="range" min={g.min} max={g.max} step={g.step} value={v} aria-label={g.label} onChange={(e) => set(g.key, e.target.value)} />
              <span className="muted small">{g.show(v)}</span>
            </div>
          );
        })}
        {TOGGLES.filter(([k]) => values[k] !== undefined).map(([k, l]) => (
          <div className="setting" key={k}>
            <div className="lbl">{l}</div>
            <label className="row"><input type="checkbox" checked={values[k] === "true"} onChange={(e) => set(k, String(e.target.checked))} /> включено</label>
            <span />
          </div>
        ))}
      </div>

      {sounds.length > 0 && (
        <>
          <h4>Звук</h4>
          <PresetBar c="sound" />
          <div className="card">
            {sounds.map((k) => {
              const name = k.replace("soundCategory_", "");
              const v = Number(values[k]);
              return (
                <div className="setting" key={k}>
                  <div className="lbl">{SOUND_NAMES[name] ?? name}</div>
                  <input type="range" min={0} max={1} step={0.05} value={v} aria-label={SOUND_NAMES[name] ?? name} onChange={(e) => set(k, e.target.value)} />
                  <span className="muted small">{Math.round(v * 100)}%</span>
                </div>
              );
            })}
          </div>
        </>
      )}

      <h4>Управление</h4>
      <PresetBar c="controls" />
      <div className="row small" style={{ marginBottom: 8 }}>
        <button className="btn link small" onClick={async () => {
          const path = await save({ title: "Экспорт раскладки", defaultPath: `раскладка-${inst.name}.txt` }).catch(() => null);
          if (path) act(() => gameOptions.exportLayout(inst.id, path), "Раскладка сохранена в файл.");
        }}>Экспорт раскладки…</button>
        <button className="btn link small" disabled={running || dirty} onClick={async () => {
          const path = await open({ title: "Импорт раскладки", filters: [{ name: "Раскладка", extensions: ["txt"] }] }).catch(() => null);
          if (typeof path === "string") act(async () => { const n = await gameOptions.importLayout(inst.id, path); await dialog.alert("Раскладка загружена", `Назначено клавиш: ${n}. Перед этим сохранён снимок.`); });
        }}>Импорт раскладки…</button>
      </div>
      <div className="card" style={{ padding: 0 }}>
        <table className="servers">
          <tbody>
            {keys.map(([k, v]) => (
              <tr key={k} className={conflictKeys.has(v) ? "conflict" : ""}>
                <td className="small">{k.replace(/^key_key\./, "").replace(/^key_/, "")}</td>
                <td style={{ textAlign: "right" }}>
                  {conflictKeys.has(v) && <span className="badge warn" style={{ marginRight: 8 }}>конфликт</span>}
                  <button className={`btn ${capture === k ? "pri" : ""}`} onClick={() => setCapture(capture === k ? null : k)}>
                    {capture === k ? "Нажмите клавишу… (Esc — снять)" : keyLabel(v)}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <details style={{ marginTop: 16 }}>
        <summary>Все параметры options.txt ({view.entries.length})</summary>
        <input className="field" type="search" placeholder="Найти параметр…" value={filter} onChange={(e) => setFilter(e.target.value)} style={{ margin: "8px 0" }} />
        <div className="card" style={{ padding: 0, maxHeight: 360, overflow: "auto" }}>
          <table className="servers">
            <tbody>
              {Object.entries(values).filter(([k]) => !filter || k.toLowerCase().includes(filter.toLowerCase())).map(([k, v]) => (
                <tr key={k}>
                  <td className="mono small">{k}</td>
                  <td><input className="field mono mini" aria-label={k} value={v} onChange={(e) => set(k, e.target.value)} /></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </details>

      {naming && (
        <Modal title="Сохранить пресет" onClose={() => setNaming(null)}>
          <p className="small muted">Пресет хранит {naming === "graphics" ? "настройки графики" : naming === "sound" ? "громкость" : "управление"} этого профиля — его можно применить к любому профилю.</p>
          <input className="field" maxLength={48} placeholder="Например: Слабый ноутбук" value={presetName} onChange={(e) => setPresetName(e.target.value)} data-autofocus />
          <div className="acts">
            <button className="btn" onClick={() => setNaming(null)}>Отмена</button>
            <button className="btn pri" disabled={!presetName.trim()} onClick={() => { const c = naming; setNaming(null); act(() => gameOptions.savePreset(inst.id, presetName, c), "Пресет сохранён."); }}>Сохранить</button>
          </div>
        </Modal>
      )}
    </>
  );
}
