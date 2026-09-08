/** N-09 世界时钟：多时区（Intl），时区列表本地持久化。 */
import { useState } from "react";
import { Plus, X } from "lucide-react";
import { useLaneLang, LABELS } from "../labels";
import { useRefresh } from "./common";

const ZONES_LS_KEY = "variable:widgets:clock:v1";
const DEFAULT_ZONES = ["Asia/Shanghai", "UTC", "America/New_York"];

function loadZones(): string[] {
  try {
    const raw = localStorage.getItem(ZONES_LS_KEY);
    const arr = raw ? (JSON.parse(raw) as string[]) : null;
    return arr && Array.isArray(arr) && arr.length > 0 ? arr : DEFAULT_ZONES;
  } catch {
    return DEFAULT_ZONES;
  }
}

function fmtZone(tz: string, lang: string): string {
  try {
    return new Intl.DateTimeFormat(lang === "en" ? "en-US" : "zh-CN", {
      timeZone: tz, hour: "2-digit", minute: "2-digit", hour12: false,
    }).format(new Date());
  } catch {
    return "--:--";
  }
}

export default function WorldClock({ paused }: { paused: boolean }): React.ReactElement {
  const lang = useLaneLang();
  const t = LABELS[lang];
  const [zones, setZones] = useState<string[]>(loadZones);
  const [, tick] = useState(0);
  const [adding, setAdding] = useState(false);

  useRefresh(() => tick((n) => n + 1), 30_000, paused);

  const persist = (zs: string[]): void => {
    setZones(zs);
    try {
      localStorage.setItem(ZONES_LS_KEY, JSON.stringify(zs));
    } catch {
      /* ignore */
    }
  };

  return (
    <div className="wgt-clock">
      {zones.slice(0, 4).map((z) => (
        <div key={z} className="wgt-clock-row">
          <span className="wgt-clock-zone" title={z}>{z.split("/").pop()?.replace("_", " ")}</span>
          <strong>{fmtZone(z, lang)}</strong>
          {zones.length > 1 && (
            <button type="button" className="wgt-x" aria-label="remove" onClick={() => persist(zones.filter((x) => x !== z))}>
              <X size={11} />
            </button>
          )}
        </div>
      ))}
      {adding ? (
        <input
          className="wgt-input"
          autoFocus
          placeholder={t.addClock}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              const v = (e.target as HTMLInputElement).value.trim();
              if (v) persist([...zones, v]);
              setAdding(false);
            }
            if (e.key === "Escape") setAdding(false);
            e.stopPropagation();
          }}
        />
      ) : (
        zones.length < 4 && (
          <button type="button" className="wgt-add" onClick={() => setAdding(true)}>
            <Plus size={12} />
          </button>
        )
      )}
    </div>
  );
}