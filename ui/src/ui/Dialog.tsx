// Собственные диалоги вместо confirm()/alert(): с фокусом, Esc и понятными кнопками.
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { t } from "../i18n";

interface ConfirmOptions {
  title: string;
  body: ReactNode;
  confirm: string;
  cancel?: string;
  danger?: boolean;
}

interface DialogApi {
  confirm: (o: ConfirmOptions) => Promise<boolean>;
  alert: (title: string, body: ReactNode) => Promise<void>;
}

const Ctx = createContext<DialogApi | null>(null);

export function useDialog(): DialogApi {
  const v = useContext(Ctx);
  if (!v) throw new Error(t("useDialog вне DialogProvider"));
  return v;
}

export function Modal({ title, children, onClose, wide }: { title: string; children: ReactNode; onClose: () => void; wide?: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    const el = ref.current?.querySelector<HTMLElement>("[data-autofocus], input, select, button.pri, button");
    el?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      // Tab не выходит за пределы окна.
      if (e.key === "Tab" && ref.current) {
        const items = [...ref.current.querySelectorAll<HTMLElement>("button, [href], input, select, textarea, [tabindex]:not([tabindex='-1'])")].filter((x) => !x.hasAttribute("disabled"));
        if (items.length === 0) return;
        const first = items[0], last = items[items.length - 1];
        if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
        else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => { window.removeEventListener("keydown", onKey); prev?.focus(); };
  }, [onClose]);
  return (
    <div className="overlay" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="dialog" role="dialog" aria-modal="true" aria-label={title} ref={ref} style={wide ? { width: "min(720px, calc(100vw - 40px))" } : undefined}>
        <h3>{title}</h3>
        {children}
      </div>
    </div>
  );
}

type Pending =
  | { kind: "confirm"; o: ConfirmOptions; resolve: (v: boolean) => void }
  | { kind: "alert"; title: string; body: ReactNode; resolve: () => void };

export function DialogProvider({ children }: { children: ReactNode }) {
  const [pending, setPending] = useState<Pending | null>(null);
  const confirm = useCallback((o: ConfirmOptions) => new Promise<boolean>((resolve) => setPending({ kind: "confirm", o, resolve })), []);
  const alert = useCallback((title: string, body: ReactNode) => new Promise<void>((resolve) => setPending({ kind: "alert", title, body, resolve })), []);
  const api = useMemo(() => ({ confirm, alert }), [confirm, alert]);
  const close = (v: boolean) => {
    if (!pending) return;
    if (pending.kind === "confirm") pending.resolve(v); else pending.resolve();
    setPending(null);
  };
  return (
    <Ctx.Provider value={api}>
      {children}
      {pending && (
        <Modal title={pending.kind === "confirm" ? pending.o.title : pending.title} onClose={() => close(false)}>
          <div style={{ userSelect: "text" }}>{pending.kind === "confirm" ? pending.o.body : pending.body}</div>
          <div className="acts">
            {pending.kind === "confirm" ? (
              <>
                <button className="btn" onClick={() => close(false)}>{pending.o.cancel ?? t("Отмена")}</button>
                <button className={pending.o.danger ? "btn danger" : "btn pri"} data-autofocus onClick={() => close(true)}>{pending.o.confirm}</button>
              </>
            ) : (
              <button className="btn pri" data-autofocus onClick={() => close(true)}>{t("Понятно")}</button>
            )}
          </div>
        </Modal>
      )}
    </Ctx.Provider>
  );
}
