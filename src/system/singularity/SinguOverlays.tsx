/**
 * SINGULARITY-100 · overlay 工具窗集合（15 面板，ai04 协议自挂载）。
 *
 * 各面板只通过 singu:* 事件总线与行为层对话（零改写既有组件）：
 * - singu-campfire  Q-37/54 临时篝火 + 存储湿度计
 * - singu-pomodoro  Q-44 番茄剧场
 * - singu-ruler     Q-45 屏幕标尺
 * - singu-countdown Q-49 倒计时胶囊
 * - singu-stash     Q-50 收藏抽屉
 * - singu-journal   Q-64/68/69 访问日志 + 权限说明书 + 设备活动史
 * - singu-iceberg   Q-70 数据冰山
 * - singu-eco       Q-71/72 营养标签 + 插件体检
 * - singu-playground Q-73 API 演练场
 * - singu-events    Q-74 事件订阅面板
 * - singu-packview  Q-75 资源包预览
 * - singu-cli       Q-76 CLI 速查
 * - singu-boutique  Q-79/80 主题精品店 + 材质滑块
 * - singu-soundlens Q-84 声音透视镜
 * - singu-quality   Q-93..100 工程质量面板
 */

import { useEffect, useState, type ReactElement } from "react";
import { useI18n } from "../../i18n";
import { dispatchClose, installCloseHandler, mountOnEvent } from "../wallpaper/mount";
import { on } from "./shared";
import { singuJournalList, singuTempClear, singuTempScan } from "./singuIpc";
import { singuT } from "./labels";

// ---------------------------------------------------------------------------
// 通用外壳
// ---------------------------------------------------------------------------

function Panel(props: { feature: string; title: string; children: React.ReactNode }): ReactElement {
  const { lang } = useI18n();
  const close = (): void => dispatchClose(props.feature);
  return (
    <div className="singu-panel" role="dialog" aria-label={props.title}>
      <header className="singu-panel-head">
        <b>{props.title}</b>
        <button className="singu-btn danger" onClick={close} aria-label={singuT("singuH_close", lang)}>
          ×
        </button>
      </header>
      <div className="singu-panel-body">{props.children}</div>
    </div>
  );
}

/** 事件订阅 hook（window singu:* → state）。 */
function useSinguEvent<T>(name: string, initial: T): [T, (v: T) => void] {
  const [v, setV] = useState<T>(initial);
  useEffect(
    () =>
      on(window, name, (e: Event) => {
        setV(((e as CustomEvent).detail ?? initial) as T);
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [name],
  );
  return [v, setV];
}

// ---------------------------------------------------------------------------
// Q-37/54 临时篝火 + 存储湿度计
// ---------------------------------------------------------------------------

function fmtBytes(b: number): string {
  if (b < 1024) return `${b} B`;
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`;
  if (b < 1024 * 1024 * 1024) return `${(b / 1024 / 1024).toFixed(1)} MB`;
  return `${(b / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function CampfirePanel(): ReactElement {
  const { lang } = useI18n();
  const [entries, setEntries] = useState<Array<{ path: string; kind: string; bytes: number; files: number }>>([]);
  const [humidity] = useSinguEvent<{ state: string; usage: number; mount: string }>("singu:humidity", {
    state: "—",
    usage: 0,
    mount: "",
  });
  const [freed, setFreed] = useState(0);
  const load = (): void => {
    void singuTempScan().then((rows) => setEntries(rows ?? []));
  };
  useEffect(load, []);
  const clear = (kind: string): void => {
    void singuTempClear([kind]).then((n) => {
      if (n !== null && n > 0) setFreed(n);
      load();
    });
  };
  const total = entries.reduce((s, e) => s + e.bytes, 0);
  const HUMIDITY_ZH: Record<string, string> = { dry: "干爽", moist: "湿润", damp: "潮湿", tide: "涨潮" };
  return (
    <Panel feature="singu-campfire" title={`CAMPFIRE · ${singuT("Q-37", lang)}`}>
      <div className="singu-camp-humidity">
        <span className={`hum ${humidity.state}`}>{HUMIDITY_ZH[humidity.state] ?? humidity.state}</span>
        <span className="mount">{humidity.mount || "—"}</span>
        <span className="pct">{Math.round(humidity.usage)}%</span>
      </div>
      {entries.length === 0 ? (
        <p className="singu-empty">无临时文件账目（干净）</p>
      ) : (
        <ul className="singu-camp-list">
          {entries.map((e) => (
            <li key={e.path}>
              <span className={`kind ${e.kind}`}>{e.kind}</span>
              <span className="path" title={e.path}>
                {e.path}
              </span>
              <span className="bytes">{fmtBytes(e.bytes)}</span>
              <button className="singu-btn" onClick={() => clear(e.kind)}>
                清理
              </button>
            </li>
          ))}
        </ul>
      )}
      <footer className="singu-panel-foot">
        <span>共 {fmtBytes(total)}</span>
        {freed > 0 ? <span className="freed">已释放 {fmtBytes(freed)}</span> : null}
        <button className="singu-btn" onClick={() => clear("temp")}>
          清理临时
        </button>
        <button className="singu-btn" onClick={() => clear("cache")}>
          清理缓存
        </button>
      </footer>
    </Panel>
  );
}

// ---------------------------------------------------------------------------
// Q-44 番茄剧场
// ---------------------------------------------------------------------------

export function PomodoroPanel(): ReactElement {
  const { lang } = useI18n();
  const [state] = useSinguEvent<{ running: boolean; fruits: number }>("singu:pomodoro-tick", {
    running: false,
    fruits: 0,
  });
  const send = (action: "start" | "pause" | "reset"): void => {
    window.dispatchEvent(new CustomEvent("singu:pomodoro", { detail: { action } }));
  };
  return (
    <Panel feature="singu-pomodoro" title={`POMODORO · ${singuT("Q-44", lang)}`}>
      <div className="singu-pomo-status">
        <span className={`state ${state.running ? "run" : "idle"}`}>{state.running ? "FOCUS" : "PAUSED"}</span>
        <span className="fruits">
          {"●".repeat(Math.min(8, state.fruits))} {state.fruits} 果实
        </span>
      </div>
      <div className="singu-pomo-actions">
        <button className="singu-btn" onClick={() => send("start")}>
          开始专注
        </button>
        <button className="singu-btn" onClick={() => send("pause")}>
          暂停
        </button>
        <button className="singu-btn danger" onClick={() => send("reset")}>
          重置
        </button>
        <button
          className="singu-btn"
          onClick={() => window.dispatchEvent(new CustomEvent("singu:stash", { detail: { open: true } }))}
        >
          休息雨声（抽屉内）
        </button>
      </div>
    </Panel>
  );
}

// ---------------------------------------------------------------------------
// Q-45 屏幕标尺
// ---------------------------------------------------------------------------

export function RulerPanel(): ReactElement {
  const { lang } = useI18n();
  const [mode, setMode] = useState<"ruler" | "rect" | "angle" | "pick">("ruler");
  const [px, setPx] = useState<{ x: number; y: number } | null>(null);
  useEffect(() => {
    const move = (e: MouseEvent): void => setPx({ x: e.clientX, y: e.clientY });
    window.addEventListener("mousemove", move);
    return () => window.removeEventListener("mousemove", move);
  }, []);
  return (
    <Panel feature="singu-ruler" title={`RULER · ${singuT("Q-45", lang)}`}>
      <div className="singu-ruler-modes">
        {(["ruler", "rect", "angle", "pick"] as const).map((m) => (
          <button key={m} className={`singu-btn ${mode === m ? "on" : ""}`} onClick={() => setMode(m)}>
            {m}
          </button>
        ))}
      </div>
      <div className="singu-ruler-read">
        {px ? (
          <span>
            X {px.x} · Y {px.y}
          </span>
        ) : (
          <span>移动鼠标取点</span>
        )}
      </div>
      <div className="singu-ruler-bar">
        {Array.from({ length: 60 }, (_, i) => (
          <i key={i} className={i % 10 === 0 ? "tick major" : "tick"} />
        ))}
      </div>
    </Panel>
  );
}

// ---------------------------------------------------------------------------
// Q-49 倒计时胶囊
// ---------------------------------------------------------------------------

export function CountdownPanel(): ReactElement {
  const { lang } = useI18n();
  const [label, setLabel] = useState("");
  const [minutes, setMinutes] = useState(25);
  const add = (): void => {
    if (!label.trim()) return;
    window.dispatchEvent(new CustomEvent("singu:capsule-add", { detail: { label: label.trim(), minutes } }));
    setLabel("");
  };
  return (
    <Panel feature="singu-countdown" title={`COUNTDOWN · ${singuT("Q-49", lang)}`}>
      <div className="singu-count-form">
        <input
          placeholder="标签（如：泡面）"
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
        />
        <input type="number" min={1} max={600} value={minutes} onChange={(e) => setMinutes(Number(e.target.value) || 1)} />
        <span>min</span>
        <button className="singu-btn" onClick={add}>
          添加胶囊
        </button>
      </div>
      <p className="singu-hint">胶囊常驻任务栏旁；到点前 5/1 分钟与到点各提醒一次。</p>
    </Panel>
  );
}

// ---------------------------------------------------------------------------
// Q-50 收藏抽屉
// ---------------------------------------------------------------------------

export function StashPanel(): ReactElement {
  const { lang } = useI18n();
  const [items, setItems] = useState<Array<{ id: number; kind: string; value: string }>>([]);
  useEffect(() => {
    try {
      const raw = localStorage.getItem("variable.singu.stash");
      if (raw) setItems(JSON.parse(raw) as typeof items);
    } catch {
      /* 空 */
    }
  }, []);
  return (
    <Panel feature="singu-stash" title={`STASH · ${singuT("Q-50", lang)}`}>
      {items.length === 0 ? (
        <p className="singu-empty">抽屉空空（划词工具箱的「存抽屉」可快速存入）</p>
      ) : (
        <ul className="singu-stash-list">
          {items.slice(0, 30).map((it) => (
            <li key={it.id}>
              <i className={`kind ${it.kind}`} />
              <span>{it.value.slice(0, 48)}</span>
            </li>
          ))}
        </ul>
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------------------
// Q-64/68/69 访问日志 + 权限说明书 + 设备活动史
// ---------------------------------------------------------------------------

export function JournalPanel(): ReactElement {
  const { lang } = useI18n();
  const [tab, setTab] = useState<"journal" | "perms" | "devices">("journal");
  const [rows, setRows] = useState<Array<{ ts_ms: number; dir: string; action: string; actor: string }>>([]);
  useEffect(() => {
    void singuJournalList().then((r) => setRows(r ?? []));
  }, [tab]);
  const burns = (): void => {
    window.dispatchEvent(new CustomEvent("singu:journal-clear"));
    setRows([]);
  };
  return (
    <Panel feature="singu-journal" title={`JOURNAL · ${singuT("Q-64", lang)}`}>
      <div className="singu-journal-tabs">
        {(
          [
            ["journal", "访问日志"],
            ["perms", "权限说明书"],
            ["devices", "设备活动史"],
          ] as const
        ).map(([k, label]) => (
          <button key={k} className={`singu-btn ${tab === k ? "on" : ""}`} onClick={() => setTab(k)}>
            {label}
          </button>
        ))}
      </div>
      {tab === "journal" ? (
        <>
          {rows.length === 0 ? (
            <p className="singu-empty">暂无访问记录（仅记录标记目录的环境内访问）</p>
          ) : (
            <ul className="singu-journal-list">
              {rows.slice(0, 50).map((r, i) => (
                <li key={`${r.ts_ms}-${i}`}>
                  <time>{new Date(r.ts_ms).toLocaleTimeString()}</time>
                  <span className="act">{r.action}</span>
                  <span className="dir" title={r.dir}>
                    {r.dir}
                  </span>
                  <span className="actor">{r.actor}</span>
                </li>
              ))}
            </ul>
          )}
          <footer className="singu-panel-foot">
            <button className="singu-btn" onClick={() => window.dispatchEvent(new CustomEvent("singu:journal-export"))}>
              导出
            </button>
            <button className="singu-btn danger" onClick={burns}>
              焚毁日志
            </button>
          </footer>
        </>
      ) : tab === "perms" ? (
        <p className="singu-hint">权限说明书在信任链中心逐应用展开（Q-68）；此处为计数账本总览。</p>
      ) : (
        <>
          <p className="singu-hint">麦克风/摄像头启停历史（30 天滚动）：</p>
          <button
            className="singu-btn"
            onClick={() => window.dispatchEvent(new CustomEvent("singu:device-history-export"))}
          >
            导出诊断包附件
          </button>
        </>
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------------------
// Q-70 数据冰山
// ---------------------------------------------------------------------------

export function IcebergPanel(): ReactElement {
  const { lang } = useI18n();
  const [zones] = useSinguEvent<{ zones: Array<{ zone: string; bytes: number }>; error: string | null }>(
    "singu:iceberg",
    { zones: [], error: null },
  );
  useEffect(() => {
    window.dispatchEvent(new CustomEvent("singu:iceberg-load"));
  }, []);
  const total = zones.zones.reduce((s, z) => s + z.bytes, 0) || 1;
  const drill = (zone: string): void => {
    window.dispatchEvent(new CustomEvent("singu:iceberg-drill", { detail: { zone } }));
  };
  return (
    <Panel feature="singu-iceberg" title={`ICEBERG · ${singuT("Q-70", lang)}`}>
      {zones.error ? (
        <p className="singu-empty">数据构成不可读（{zones.error}）</p>
      ) : zones.zones.length === 0 ? (
        <p className="singu-empty">正在读取数据分区…</p>
      ) : (
        <div className="singu-iceberg-sea">
          <div className="waterline" />
          {zones.zones.map((z) => (
            <button
              key={z.zone}
              className={`singu-berg ${z.zone === "database" ? "above" : "below"}`}
              style={{ width: `${Math.max(12, (z.bytes / total) * 100)}%` }}
              onClick={() => drill(z.zone)}
            >
              <b>{z.zone}</b>
              <span>{fmtBytes(z.bytes)}</span>
            </button>
          ))}
        </div>
      )}
      <p className="singu-hint">水面上 = 用户可见数据；水面下 = 缓存/日志/临时。点击下钻治理。</p>
    </Panel>
  );
}

// ---------------------------------------------------------------------------
// Q-71/72 营养标签 + 插件体检
// ---------------------------------------------------------------------------

export function EcoPanel(): ReactElement {
  const { lang } = useI18n();
  const [report] = useSinguEvent<Array<{ id: string; grade: string; memGrowthPct: number; subs: number; errRate: number }>>(
    "singu:checkup-report",
    [],
  );
  useEffect(() => {
    window.dispatchEvent(new CustomEvent("singu:checkup"));
  }, []);
  return (
    <Panel feature="singu-eco" title={`ECO · ${singuT("Q-72", lang)}`}>
      {report.length === 0 ? (
        <p className="singu-empty">尚无采样数据（体检每 60s 采一次样）</p>
      ) : (
        <table className="singu-table">
          <thead>
            <tr>
              <th>插件</th>
              <th>评分</th>
              <th>内存增幅</th>
              <th>订阅数</th>
              <th>错误率</th>
            </tr>
          </thead>
          <tbody>
            {report.map((r) => (
              <tr key={r.id}>
                <td>{r.id}</td>
                <td>
                  <b className={`grade ${r.grade}`}>{r.grade}</b>
                </td>
                <td>{r.memGrowthPct.toFixed(1)}%</td>
                <td>{r.subs}</td>
                <td>{(r.errRate * 100).toFixed(2)}%</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <p className="singu-hint">营养标签（Q-71）在插件市场安装页展示，未经试用如实标注。</p>
    </Panel>
  );
}

// ---------------------------------------------------------------------------
// Q-73 API 演练场
// ---------------------------------------------------------------------------

export function PlaygroundPanel(): ReactElement {
  const { lang } = useI18n();
  const [method, setMethod] = useState("GET");
  const [path, setPath] = useState("/api/version");
  const [result] = useSinguEvent<{ status: number; ms: number; body: string }>("singu:api-result", {
    status: -1,
    ms: 0,
    body: "",
  });
  const send = (): void => {
    window.dispatchEvent(new CustomEvent("singu:api-try", { detail: { method, path } }));
  };
  return (
    <Panel feature="singu-playground" title={`PLAYGROUND · ${singuT("Q-73", lang)}`}>
      <div className="singu-api-form">
        <select value={method} onChange={(e) => setMethod(e.target.value)}>
          {["GET", "POST", "PUT", "DELETE"].map((m) => (
            <option key={m}>{m}</option>
          ))}
        </select>
        <input value={path} onChange={(e) => setPath(e.target.value)} onKeyDown={(e) => e.key === "Enter" && send()} />
        <button className="singu-btn" onClick={send}>
          发送
        </button>
      </div>
      {result.status >= 0 ? (
        <div className="singu-api-result">
          <header>
            <b className={result.status < 400 ? "ok" : "err"}>{result.status}</b>
            <span>{result.ms}ms</span>
          </header>
          <pre>{result.body}</pre>
        </div>
      ) : (
        <p className="singu-empty">仅能调用本地网关（127.0.0.1:17777）；无外网。</p>
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------------------
// Q-74 事件订阅面板
// ---------------------------------------------------------------------------

export function EventsPanel(): ReactElement {
  const { lang } = useI18n();
  const [preview] = useSinguEvent<Array<{ event: string; ts: number }>>("singu:event-preview", []);
  useEffect(() => {
    const t = setInterval(() => window.dispatchEvent(new CustomEvent("singu:event-preview")), 1000);
    return () => clearInterval(t);
  }, []);
  return (
    <Panel feature="singu-events" title={`EVENTS · ${singuT("Q-74", lang)}`}>
      <p className="singu-hint">本地事件流实时预览（500 条环形缓冲）：</p>
      <ul className="singu-events-list">
        {preview.length === 0 ? (
          <li className="singu-empty">静默中…</li>
        ) : (
          preview.slice(0, 20).map((p, i) => (
            <li key={`${p.ts}-${i}`}>
              <time>{new Date(p.ts).toLocaleTimeString()}</time>
              <code>{p.event}</code>
            </li>
          ))
        )}
      </ul>
    </Panel>
  );
}

// ---------------------------------------------------------------------------
// Q-75 资源包预览
// ---------------------------------------------------------------------------

export function PackViewPanel(): ReactElement {
  const { lang } = useI18n();
  const [path, setPath] = useState("");
  const [report] = useSinguEvent<{ ok: boolean; report?: { total: number; ok: number; failed: string[]; readable: boolean }; error?: string }>(
    "singu:pack-report",
    { ok: false },
  );
  const load = (): void => {
    if (path.trim()) window.dispatchEvent(new CustomEvent("singu:pack-preview", { detail: { path: path.trim() } }));
  };
  return (
    <Panel feature="singu-packview" title={`PACKVIEW · ${singuT("Q-75", lang)}`}>
      <div className="singu-api-form">
        <input placeholder="D:\path\to\pack.vxs" value={path} onChange={(e) => setPath(e.target.value)} />
        <button className="singu-btn" onClick={load}>
          预览
        </button>
      </div>
      {report.ok && report.report ? (
        <div className="singu-pack-report">
          <p>
            条目 {report.report.total} · 完好 {report.report.ok} ·{" "}
            {report.report.failed.length > 0 ? `损坏 ${report.report.failed.length}` : "全部完好"}
          </p>
          {report.report.failed.slice(0, 10).map((f, i) => (
            <p key={i} className="err">
              {f}
            </p>
          ))}
        </div>
      ) : report.error ? (
        <p className="singu-empty err">{report.error}</p>
      ) : (
        <p className="singu-empty">输入 .vxs/.vxa 包路径进行内存流只读预览（零解压到磁盘）。</p>
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------------------
// Q-76 CLI 速查
// ---------------------------------------------------------------------------

export function CliPanel(): ReactElement {
  const { lang } = useI18n();
  const [cheats] = useSinguEvent<Array<{ scene: string; cmds: Array<[string, string]> }>>("singu:cli-list", []);
  const [copied, setCopied] = useState("");
  useEffect(() => {
    window.dispatchEvent(new CustomEvent("singu:cli-list"));
  }, []);
  const copy = (cmd: string): void => {
    void navigator.clipboard.writeText(cmd);
    setCopied(cmd);
    setTimeout(() => setCopied(""), 1500);
  };
  return (
    <Panel feature="singu-cli" title={`CLI · ${singuT("Q-76", lang)}`}>
      {cheats.map((g) => (
        <section key={g.scene} className="singu-cli-group">
          <b>{g.scene}</b>
          {g.cmds.map(([cmd, desc]) => (
            <button key={cmd} className="singu-cli-card" onClick={() => copy(cmd)}>
              <code>{cmd}</code>
              <span>{desc}</span>
              <i>{copied === cmd ? "已复制" : "点击复制"}</i>
            </button>
          ))}
        </section>
      ))}
    </Panel>
  );
}

// ---------------------------------------------------------------------------
// Q-79/80 主题精品店 + 材质滑块
// ---------------------------------------------------------------------------

export function BoutiquePanel(): ReactElement {
  const { lang } = useI18n();
  const [themes] = useSinguEvent<Array<{ id: string; name: string; hue: number; sat: number; light: number }>>(
    "singu:boutique-list",
    [],
  );
  const [mat, setMat] = useState({ fog: 40, refraction: 30, grain: 15, gloss: 55 });
  useEffect(() => {
    window.dispatchEvent(new CustomEvent("singu:boutique-list"));
  }, []);
  const tryOn = (id: string): void => {
    window.dispatchEvent(new CustomEvent("singu:boutique-try", { detail: { id } }));
  };
  const commit = (id: string): void => {
    window.dispatchEvent(new CustomEvent("singu:boutique-commit", { detail: { id } }));
  };
  const applyMat = (key: keyof typeof mat, v: number): void => {
    const next = { ...mat, [key]: v };
    setMat(next);
    window.dispatchEvent(new CustomEvent("singu:material", { detail: next }));
  };
  return (
    <Panel feature="singu-boutique" title={`BOUTIQUE · ${singuT("Q-79", lang)}`}>
      <div className="singu-boutique-grid">
        {themes.map((t) => (
          <div
            key={t.id}
            className="singu-theme-card"
            style={{ background: `hsl(${t.hue} ${t.sat}% ${t.light}%)` }}
            onMouseEnter={() => tryOn(t.id)}
            onMouseLeave={() => window.dispatchEvent(new CustomEvent("singu:boutique-clear"))}
            onClick={() => commit(t.id)}
          >
            <b>{t.name}</b>
            <span>悬停试穿 · 点击固化</span>
          </div>
        ))}
      </div>
      <section className="singu-material">
        <b>{singuT("Q-80", lang)}</b>
        {(
          [
            ["fog", "雾度"],
            ["refraction", "折射"],
            ["grain", "噪点"],
            ["gloss", "光泽"],
          ] as const
        ).map(([key, label]) => (
          <label key={key}>
            <span>{label}</span>
            <input
              type="range"
              min={0}
              max={100}
              value={mat[key]}
              onChange={(e) => applyMat(key, Number(e.target.value))}
            />
          </label>
        ))}
      </section>
    </Panel>
  );
}

// ---------------------------------------------------------------------------
// Q-84 声音透视镜
// ---------------------------------------------------------------------------

export function SoundLensPanel(): ReactElement {
  const { lang } = useI18n();
  const [rows] = useSinguEvent<Array<{ app: string; volume: number; level: number }>>("singu:sound-lens", []);
  return (
    <Panel feature="singu-soundlens" title={`SOUND LENS · ${singuT("Q-84", lang)}`}>
      {rows.length === 0 ? (
        <p className="singu-empty">静默态（无活跃音频流）</p>
      ) : (
        rows.map((r) => (
          <div key={r.app} className="singu-sound-row">
            <b>{r.app}</b>
            <div className="level">
              <i style={{ width: `${Math.round(r.level * 100)}%` }} />
            </div>
            <span>{Math.round(r.level * 100)}%</span>
          </div>
        ))
      )}
      <p className="singu-hint">电平条 30fps 更新；单流闪避在音频面板操作。</p>
    </Panel>
  );
}

// ---------------------------------------------------------------------------
// Q-93..100 工程质量面板（火焰图/长任务/配额/演练/瘦身/降级/更新/心电图）
// ---------------------------------------------------------------------------

export function QualityPanel(): ReactElement {
  const { lang } = useI18n();
  const [flame] = useSinguEvent<Array<{ ts: number; stages: Array<{ name: string; ms: number }> }>>("singu:flame", []);
  const [longtasks] = useSinguEvent<Array<{ ms: number; ts: number; source: string }>>("singu:longtask", []);
  const [quota] = useSinguEvent<Array<{ zone: string; bytes: number; quota: number; warn: boolean }>>("singu:quota", []);
  useEffect(() => {
    window.dispatchEvent(new CustomEvent("singu:flame-load"));
    window.dispatchEvent(new CustomEvent("singu:longtask-load"));
    window.dispatchEvent(new CustomEvent("singu:quota-load"));
  }, []);
  const latest = flame[0];
  const totalMs = latest?.stages.reduce((s, x) => s + x.ms, 0) || 1;
  return (
    <Panel feature="singu-quality" title={`QUALITY · ${singuT("Q-93", lang)} → ${singuT("Q-100", lang)}`}>
      <section>
        <b>{singuT("Q-93", lang)}</b>
        {latest ? (
          <div className="singu-flame">
            {latest.stages.map((s, i) => (
              <span
                key={i}
                title={`${s.name} ${Math.round(s.ms)}ms`}
                style={{ width: `${Math.max(4, (s.ms / totalMs) * 100)}%` }}
              >
                {s.name}
              </span>
            ))}
          </div>
        ) : (
          <p className="singu-empty">无启动埋点数据</p>
        )}
      </section>
      <section>
        <b>{singuT("Q-94", lang)}</b>
        {longtasks.length === 0 ? (
          <p className="singu-empty">近 5 分钟无长任务（&gt;50ms）</p>
        ) : (
          <ul className="singu-task-list">
            {longtasks.slice(0, 10).map((t, i) => (
              <li key={i}>
                <span className="ms">{t.ms}ms</span>
                <span>{new Date(t.ts).toLocaleTimeString()}</span>
              </li>
            ))}
          </ul>
        )}
      </section>
      <section>
        <b>{singuT("Q-95", lang)}</b>
        {quota.length === 0 ? (
          <p className="singu-empty">配额数据读取中…</p>
        ) : (
          quota.map((z) => (
            <div key={z.zone} className={`singu-quota-row ${z.warn ? "warn" : ""}`}>
              <span>{z.zone}</span>
              <span>
                {fmtBytes(z.bytes)} / {fmtBytes(z.quota)}
              </span>
              {z.warn ? <b className="err">超限预警</b> : null}
            </div>
          ))
        )}
      </section>
      <section>
        <b>{singuT("Q-96", lang)} · {singuT("Q-97", lang)} · {singuT("Q-99", lang)} · {singuT("Q-100", lang)}</b>
        <div className="singu-quality-actions">
          <button className="singu-btn" onClick={() => window.dispatchEvent(new CustomEvent("singu:drill", { detail: { kind: "render" } }))}>
            渲染崩溃演练
          </button>
          <button className="singu-btn" onClick={() => window.dispatchEvent(new CustomEvent("singu:diet-load"))}>
            生成瘦身报告
          </button>
          <button className="singu-btn" onClick={() => window.dispatchEvent(new CustomEvent("singu:update-preview"))}>
            更新预览
          </button>
          <button className="singu-btn" onClick={() => window.dispatchEvent(new CustomEvent("singu:ecg-load"))}>
            版点心电图
          </button>
        </div>
      </section>
    </Panel>
  );
}

// ---------------------------------------------------------------------------
// ai04 协议自挂载（模块 import 即激活全部 overlay 监听）
// ---------------------------------------------------------------------------

if (typeof window !== "undefined") {
  const OVERLAYS: Array<[string, () => Promise<{ Overlay: React.ComponentType }>]> = [
    ["singu-campfire", () => Promise.resolve({ Overlay: CampfirePanel })],
    ["singu-pomodoro", () => Promise.resolve({ Overlay: PomodoroPanel })],
    ["singu-ruler", () => Promise.resolve({ Overlay: RulerPanel })],
    ["singu-countdown", () => Promise.resolve({ Overlay: CountdownPanel })],
    ["singu-stash", () => Promise.resolve({ Overlay: StashPanel })],
    ["singu-journal", () => Promise.resolve({ Overlay: JournalPanel })],
    ["singu-iceberg", () => Promise.resolve({ Overlay: IcebergPanel })],
    ["singu-eco", () => Promise.resolve({ Overlay: EcoPanel })],
    ["singu-playground", () => Promise.resolve({ Overlay: PlaygroundPanel })],
    ["singu-events", () => Promise.resolve({ Overlay: EventsPanel })],
    ["singu-packview", () => Promise.resolve({ Overlay: PackViewPanel })],
    ["singu-cli", () => Promise.resolve({ Overlay: CliPanel })],
    ["singu-boutique", () => Promise.resolve({ Overlay: BoutiquePanel })],
    ["singu-soundlens", () => Promise.resolve({ Overlay: SoundLensPanel })],
    ["singu-quality", () => Promise.resolve({ Overlay: QualityPanel })],
  ];
  for (const [feature, loader] of OVERLAYS) {
    void mountOnEvent(window, feature, loader);
    void installCloseHandler(window, feature);
  }
}
