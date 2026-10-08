/**
 * AI-14 开放接口组 — 设置页「开放接口」标签。
 * 覆盖：U-37 协议中枢 / U-38 数据开放导出 / U-39 .vxs 资源包 /
 * Z-51 本地事件流 / Z-52 插件清单校验 / Z-53 本地网关 / Z-54 分享信封 /
 * Z-55 数据连接器 / N-30 浏览器伴侣收件箱。
 * 红线：网关/事件流默认关闭（零监听）；导出只读；协议注册仅便携态。
 */
import { useCallback, useEffect, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../i18n";
import { ipc, type Shell } from "../../lib/ipc";
import { validateTheme, type ThemeCheckResult } from "../../lib/themeToken";
import { validatePluginManifest, type PluginCheckResult } from "../../lib/pluginManifest";
import { openEnvelope, sealEnvelope, type ShareKind } from "../../lib/shareFormat";
import { checkEcoCompat, ECO_MATRIX } from "../../lib/ecoMatrix";

function Badge(props: { ok: boolean | null; children: React.ReactNode }): React.ReactElement {
  const cls = props.ok === null ? "oh-badge" : props.ok ? "oh-badge ok" : "oh-badge bad";
  return <span className={cls}>{props.children}</span>;
}

function renderCheckItems(items: { level: string; message: string }[]): string {
  return items.map((i) => `${i.level === "error" ? "✕" : "⚠"} ${i.message}`).join("\n") || "—";
}

export function OpenHubTab(): React.ReactElement {
  const { t } = useI18n();

  const [cfg, setCfg] = useState<Shell.OpenHubConfigView | null>(null);
  const [gw, setGw] = useState<Shell.GatewayStatusView | null>(null);
  const [busy, setBusy] = useState(false);

  const patchCfg = useCallback(
    async (p: Partial<Shell.OpenHubConfigView>): Promise<void> => {
      if (!cfg) return;
      setBusy(true);
      try {
        const next = await ipc.openhubConfigSet({ ...cfg, ...p });
        setCfg(next);
      } finally {
        setBusy(false);
      }
    },
    [cfg],
  );

  useEffect(() => {
    void ipc.openhubConfigGet().then(setCfg).catch(() => setCfg(null));
    void ipc.gatewayStatus().then(setGw).catch(() => setGw(null));
  }, []);

  // ---- U-37 协议中枢 ----
  const [dlUrl, setDlUrl] = useState("variable://scene/apply?name=focus");
  const [dlOut, setDlOut] = useState<string | null>(null);
  const doParse = async (): Promise<void> => {
    try {
      const r = await ipc.deeplinkParse(dlUrl.trim());
      setDlOut(`${r.verb}\n${r.action}\n${r.params.map(([k, v]) => `${k}=${v}`).join("\n")}`);
    } catch (e) {
      setDlOut(t("ohDlErr") + ": " + String(e));
    }
  };

  // ---- U-39 .vxs ----
  const [vxsOut, setVxsOut] = useState<string | null>(null);
  const doVxs = async (): Promise<void> => {
    const p = await openFileDialog({ multiple: false, filters: [{ name: "VXS Pack", extensions: ["vxs"] }] });
    if (typeof p !== "string") return;
    try {
      const r = await ipc.vxsValidate(p);
      setVxsOut(`${t("ohVxsId")}: ${r.id}\n${t("ohVxsFmt")}: ${r.formatOk ? "OK" : "NG"}\n${r.resources.join("\n")}${r.errors.length ? "\n" + r.errors.join("\n") : ""}`);
    } catch (e) {
      setVxsOut(String(e));
    }
  };

  // ---- 数据开放导出 ----
  const [exportOut, setExportOut] = useState<string | null>(null);
  const doExport = async (): Promise<void> => {
    const dir = await openFileDialog({ multiple: false, directory: true });
    if (typeof dir !== "string") return;
    setBusy(true);
    try {
      const r = await ipc.openhubDataExport(dir);
      setExportOut(`${r.outDir}\n${t("ohExportFiles")}: ${r.files} · ${Math.round(r.bytes / 1024)} KB`);
    } catch (e) {
      setExportOut(String(e));
    } finally {
      setBusy(false);
    }
  };

  // ---- Z-51 事件流 ----
  const [tailOut, setTailOut] = useState<string | null>(null);
  const doTail = async (): Promise<void> => {
    const lines = await ipc.openhubStreamTail(20).catch(() => [] as string[]);
    setTailOut(lines.join("\n") || t("ohTailEmpty"));
  };

  // ---- Z-52 插件清单校验（本地，不联网）----
  const [pjText, setPjText] = useState(JSON.stringify({ id: "demo-clock", name: "Demo Clock", version: "1.0.0", engine: ">=1.5 <2", permissions: ["ui.notify"], entry: "main.js" }, null, 2));
  const [pjOut, setPjOut] = useState<PluginCheckResult | null>(null);
  const doPj = (): void => {
    try {
      setPjOut(validatePluginManifest(JSON.parse(pjText)));
    } catch {
      setPjOut({ ok: false, items: [{ level: "error", message: t("ohBadJson") }] });
    }
  };

  // ---- Z-50 主题令牌校验（本地）----
  const [thText, setThText] = useState(JSON.stringify({ name: "示例主题", tokens: { "--v-color-bg": "#0b1122", "--v-color-fg": "#dfe7f5", "--v-color-accent": "#6f8fd8" } }, null, 2));
  const [thOut, setThOut] = useState<ThemeCheckResult | null>(null);
  const doTh = (): void => {
    try {
      setThOut(validateTheme(JSON.parse(thText)));
    } catch {
      setThOut({ ok: false, items: [{ level: "error", message: t("ohBadJson") }] });
    }
  };

  // ---- Z-54 分享信封 ----
  const [envText, setEnvText] = useState("");
  const [envOut, setEnvOut] = useState<string | null>(null);
  const doSeal = (): void => {
    const env = sealEnvelope("keymap" as ShareKind, { "Ctrl+Alt+O": "orchestrator" });
    setEnvText(JSON.stringify(env, null, 2));
    setEnvOut(t("ohSealOk"));
  };
  const doOpen = (): void => {
    const r = openEnvelope(envText, (p) => Object.keys(p as object).length);
    setEnvOut(r.ok ? r.summary : r.error);
  };

  // ---- Z-55 连接器 ----
  const [cPath, setCPath] = useState("");
  const [cKind, setCKind] = useState("json");
  const [cSql, setCSql] = useState("SELECT * FROM t");
  const [cOut, setCOut] = useState<string | null>(null);
  const doQuery = async (): Promise<void> => {
    const r = await ipc.openhubConnectorQuery({ path: cPath, kind: cKind, sql: cKind === "sqlite" ? cSql : null });
    setCOut(r.error ?? [r.columns.join(" | "), ...r.rows.slice(0, 20).map((row) => row.join(" | "))].join("\n"));
  };

  // ---- N-30 伴侣收件箱 ----
  const [inbox, setInbox] = useState<string[]>([]);
  const reloadInbox = useCallback((): void => {
    void ipc.companionInbox().then(setInbox).catch(() => setInbox([]));
  }, []);
  useEffect(reloadInbox, [reloadInbox]);

  return (
    <div className="oh-tab">
      <p className="oh-desc">{t("ohDesc")}</p>

      {/* N-28/Z-53 本地网关 */}
      <section className="oh-group">
        <h4>{t("ohGwTitle")}</h4>
        <label className="oh-row">
          <input type="checkbox" disabled={!cfg || busy} checked={cfg?.gatewayEnabled ?? false} onChange={(e) => void patchCfg({ gatewayEnabled: e.target.checked })} />
          <span>
            <span className="oh-label">{t("ohGwToggle")}</span>
            <span className="oh-hint">{t("ohGwHint")}</span>
          </span>
        </label>
        <div className="oh-row">
          <span>
            <span className="oh-label">{t("ohGwPort")}</span>
            <span className="oh-hint">{gw ? (gw.running ? t("ohGwRunning", { port: String(gw.port) }) : t("ohGwStopped")) : t("ohGwUnknown")}</span>
          </span>
          <input
            className="oh-input"
            style={{ maxWidth: 110 }}
            type="number"
            disabled={!cfg || busy}
            value={cfg?.gatewayPort ?? 47630}
            onChange={(e) => void patchCfg({ gatewayPort: Number(e.target.value) })}
          />
        </div>
        {cfg && (
          <div className="oh-row">
            <span className="oh-mono">{cfg.gatewayToken}</span>
            <button type="button" disabled={busy} onClick={() => void ipc.gatewayTokenRegen().then((tk) => setCfg({ ...cfg, gatewayToken: tk }))}>
              {t("ohGwRegen")}
            </button>
          </div>
        )}
      </section>

      {/* Z-51 本地事件流 */}
      <section className="oh-group">
        <h4>{t("ohStreamTitle")}</h4>
        <label className="oh-row">
          <input type="checkbox" disabled={!cfg || busy} checked={cfg?.streamEnabled ?? false} onChange={(e) => void patchCfg({ streamEnabled: e.target.checked })} />
          <span>
            <span className="oh-label">{t("ohStreamToggle")}</span>
            <span className="oh-hint">{t("ohStreamHint")}</span>
          </span>
        </label>
        <div className="oh-actions">
          <button type="button" onClick={() => void doTail()}>{t("ohStreamTail")}</button>
        </div>
        {tailOut !== null && <pre className="oh-result">{tailOut}</pre>}
      </section>

      {/* U-37 协议中枢 */}
      <section className="oh-group">
        <h4>{t("ohDlTitle")}</h4>
        <p className="oh-hint">{t("ohDlHint")}</p>
        <input className="oh-input oh-mono" value={dlUrl} onChange={(e) => setDlUrl(e.target.value)} />
        <div className="oh-actions">
          <button type="button" onClick={() => void doParse()}>{t("ohDlParse")}</button>
          <button type="button" disabled={busy} onClick={() => void ipc.deeplinkRegister().then(() => setDlOut(t("ohDlRegOk"))).catch((e) => setDlOut(String(e)))}>{t("ohDlRegister")}</button>
          <button type="button" disabled={busy} onClick={() => void ipc.deeplinkUnregister().then(() => setDlOut(t("ohDlUnregOk"))).catch((e) => setDlOut(String(e)))}>{t("ohDlUnregister")}</button>
        </div>
        {dlOut !== null && <pre className="oh-result">{dlOut}</pre>}
      </section>

      {/* U-38 数据开放导出 */}
      <section className="oh-group">
        <h4>{t("ohExportTitle")}</h4>
        <p className="oh-hint">{t("ohExportHint")}</p>
        <div className="oh-actions">
          <button type="button" disabled={busy} onClick={() => void doExport()}>{t("ohExportRun")}</button>
        </div>
        {exportOut !== null && <pre className="oh-result">{exportOut}</pre>}
      </section>

      {/* U-39 .vxs 资源包 */}
      <section className="oh-group">
        <h4>{t("ohVxsTitle")}</h4>
        <p className="oh-hint">{t("ohVxsHint")}</p>
        <div className="oh-actions">
          <button type="button" onClick={() => void doVxs()}>{t("ohVxsPick")}</button>
        </div>
        {vxsOut !== null && <pre className="oh-result">{vxsOut}</pre>}
      </section>

      {/* Z-52 插件清单校验 */}
      <section className="oh-group">
        <h4>{t("ohPjTitle")}</h4>
        <textarea className="oh-input oh-mono" rows={7} value={pjText} onChange={(e) => setPjText(e.target.value)} />
        <div className="oh-actions">
          <button type="button" onClick={doPj}>{t("ohValidate")}</button>
        </div>
        {pjOut && (
          <div className="oh-result">
            <Badge ok={pjOut.ok}>{pjOut.ok ? "PASS" : "FAIL"}</Badge>
            {"\n"}
            {renderCheckItems(pjOut.items)}
          </div>
        )}
      </section>

      {/* Z-50 主题令牌校验 */}
      <section className="oh-group">
        <h4>{t("ohThTitle")}</h4>
        <textarea className="oh-input oh-mono" rows={7} value={thText} onChange={(e) => setThText(e.target.value)} />
        <div className="oh-actions">
          <button type="button" onClick={doTh}>{t("ohValidate")}</button>
        </div>
        {thOut && (
          <div className="oh-result">
            <Badge ok={thOut.ok}>{thOut.ok ? "PASS" : "FAIL"}</Badge>
            {"\n"}
            {renderCheckItems(thOut.items)}
          </div>
        )}
      </section>

      {/* Z-54 分享信封 */}
      <section className="oh-group">
        <h4>{t("ohEnvTitle")}</h4>
        <p className="oh-hint">{t("ohEnvHint")}</p>
        <textarea className="oh-input oh-mono" rows={6} value={envText} onChange={(e) => setEnvText(e.target.value)} />
        <div className="oh-actions">
          <button type="button" onClick={doSeal}>{t("ohEnvSeal")}</button>
          <button type="button" onClick={doOpen}>{t("ohEnvOpen")}</button>
        </div>
        {envOut !== null && <pre className="oh-result">{envOut}</pre>}
      </section>

      {/* Z-56 兼容矩阵（只读展示 + 判定） */}
      <section className="oh-group">
        <h4>{t("ohEcoTitle")}</h4>
        <p className="oh-hint">{t("ohEcoHint")}</p>
        {ECO_MATRIX.map((e) => {
          const v = checkEcoCompat(e.kind, e.formatVersion);
          return (
            <div className="oh-row" key={e.kind}>
              <span className="oh-label">{e.kind} · v{e.formatVersion}</span>
              <Badge ok={v.verdict === "supported"}>{v.message}</Badge>
            </div>
          );
        })}
      </section>

      {/* Z-55 数据连接器 */}
      <section className="oh-group">
        <h4>{t("ohConnTitle")}</h4>
        <p className="oh-hint">{t("ohConnHint")}</p>
        <div className="oh-actions">
          <button type="button" onClick={async () => {
            const p = await openFileDialog({ multiple: false });
            if (typeof p === "string") setCPath(p);
          }}>{t("ohConnPick")}</button>
          <select value={cKind} onChange={(e) => setCKind(e.target.value)}>
            <option value="json">JSON</option>
            <option value="csv">CSV</option>
            <option value="sqlite">SQLite</option>
          </select>
          {cKind === "sqlite" && <input className="oh-input oh-mono" style={{ maxWidth: 260 }} value={cSql} onChange={(e) => setCSql(e.target.value)} />}
          <button type="button" disabled={!cPath} onClick={() => void doQuery()}>{t("ohConnRun")}</button>
        </div>
        {cOut !== null && <pre className="oh-result">{cOut}</pre>}
      </section>

      {/* N-30 浏览器伴侣收件箱 */}
      <section className="oh-group">
        <h4>{t("ohInboxTitle")}</h4>
        <p className="oh-hint">{t("ohInboxHint")}</p>
        <div className="oh-actions">
          <button type="button" onClick={reloadInbox}>{t("ohInboxReload")}</button>
          <button type="button" onClick={() => void ipc.companionInboxClear().then(reloadInbox)}>{t("ohInboxClear")}</button>
        </div>
        {inbox.length === 0 ? (
          <p className="oh-hint">{t("ohInboxEmpty")}</p>
        ) : (
          <pre className="oh-result">{inbox.join("\n")}</pre>
        )}
      </section>
    </div>
  );
}
