import { createContext, useCallback, useContext, useState, type ReactNode } from "react";

type ToastKind = "is-info" | "is-success" | "is-warning" | "is-danger";

interface Toast {
  id: number;
  message: string;
  kind: ToastKind;
}

interface ToastContextValue {
  show: (message: string, kind?: ToastKind, duration?: number) => void;
}

const Ctx = createContext<ToastContextValue>({ show: () => {} });

let nextId = 1;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);

  const show = useCallback((message: string, kind: ToastKind = "is-info", duration = 5000) => {
    const id = nextId++;
    setToasts((t) => [...t, { id, message, kind }]);
    if (duration > 0) {
      setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), duration);
    }
  }, []);

  return (
    <Ctx.Provider value={{ show }}>
      {children}
      <div id="toast-container">
        {toasts.map((t) => (
          <div key={t.id} className={`notification toast ${t.kind} is-active`}>
            <button className="delete" onClick={() => setToasts((cur) => cur.filter((x) => x.id !== t.id))}></button>
            <span dangerouslySetInnerHTML={{ __html: t.message }} />
          </div>
        ))}
      </div>
    </Ctx.Provider>
  );
}

export function useToast() {
  return useContext(Ctx);
}
