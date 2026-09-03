import { X, AlertTriangle, CheckCircle2, Info } from "lucide-react";
import { dismissToast, useToasts } from "../state/uiStore";

export function ToastHost(): React.ReactElement {
  const toasts = useToasts();
  return (
    <div className="toast-host" role="status" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast ${t.kind}`}>
          <span className="toast-icon">
            {t.kind === "error" ? <AlertTriangle size={15} /> : t.kind === "success" ? <CheckCircle2 size={15} /> : <Info size={15} />}
          </span>
          <div className="toast-text">
            <span>{t.message}</span>
            {t.detail && <small>{t.detail}</small>}
          </div>
          <button type="button" className="icon-btn tiny" aria-label="dismiss" onClick={() => dismissToast(t.id)}>
            <X size={12} />
          </button>
        </div>
      ))}
    </div>
  );
}
