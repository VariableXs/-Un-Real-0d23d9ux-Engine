import { useCallback, useEffect, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../i18n";
import { errMessage, ipc } from "../../lib/ipc";
import { saveSetting } from "../../lib/settings";
import { pushToast } from "../../state/uiStore";

/**
 * B-29：安全分析工作台（设置页「安全」标签）——静态优先、只读、零出站。
 * PE 解析（节熵/导入/可疑 API/签名）+ 入口反汇编（iced-x86 纯 Rust）+
 * Sandbox 探测与 .wsb 生成 + 报告导出（Markdown，不含样本字节）。
 */

interface SectionInfo {
  name: string;
  rawSize: number;
  entropy: number;
  suspiciousEntropy: boolean;
}
interface ImportDll {
  dll: string;
  functions: number;
}
interface PeAnalysis {
  isPe: boolean;
  machine: string;
  entryRva: number;
  sections: SectionInfo[];
  imports: ImportDll[];
  signed: boolean;
  suspiciousHits: string[];
  blake3: string;
  stringsTop: string[];
  sampleNote: string;
}
interface DisasmLine {
  rva: number;
  bytesHex: string;
  text: string;
}

export function SecurityTab() {
  const { t } = useI18n();
  const [analysis, setAnalysis] = useState<PeAnalysis | null>(null);
  const [path, setPath] = useState<string>("");
  const [disasmLines, setDisasmLines] = useState<DisasmLine[] | null>(null);
  const [sandbox, setSandbox] = useState<{ available: boolean; detail: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [shield, setShield] = useState(false);
  const [tagged, setTagged] = useState<number | null>(null);

  // S-1：初始态从 Rust 取（真实开关，含托盘已切换的情况）
  useEffect(() => {
    ipc
      .shieldGet()
      .then(setShield)
      .catch(() => {});
  }, []);

  const toggleShield = () =>
    void (async () => {
      const next = !shield;
      try {
        const st = await ipc.shieldSet(next);
        setShield(st.on);
        setTagged(st.tagged);
        void saveSetting("privacyShield", st.on);
        pushToast("info", st.on ? t("shieldOn") : t("shieldOff"), `${t("shieldTagged")}: ${st.tagged}`);
      } catch (e) {
        pushToast("error", t("shieldFail"), errMessage(e).message);
      }
    })();

  const analyze = () =>
    void (async () => {
      const f = await openFileDialog({
        multiple: false,
        filters: [{ name: "Executable", extensions: ["exe", "dll", "sys"] }],
      });
      if (typeof f !== "string") return;
      setPath(f);
      setBusy(true);
      try {
        setAnalysis(await ipc.peAnalyze(f));
      } catch (e) {
        pushToast("error", t("secAnalyzeFail"), errMessage(e).message);
      } finally {
        setBusy(false);
      }
    })();

  const runDisasm = useCallback(() => {
    if (!path) return;
    setBusy(true);
    ipc
      .disasmEntry(path, 40)
      .then(setDisasmLines)
      .catch((e) => pushToast("error", t("secDisasmFail"), errMessage(e).message))
      .finally(() => setBusy(false));
  }, [path, t]);

  const probe = () =>
    void (async () => {
      try {
        setSandbox(await ipc.sandboxProbe());
      } catch (e) {
        pushToast("error", t("secSandboxFail"), errMessage(e).message);
      }
    })();

  const exportReport = () =>
    void (async () => {
      if (!path) return;
      const out = await saveFileDialog({
        defaultPath: "security-report.md",
        filters: [{ name: "Markdown", extensions: ["md"] }],
      });
      if (typeof out !== "string") return;
      try {
        await ipc.securityReportExport(path, out);
        pushToast("success", t("secReportDone"), out);
      } catch (e) {
        pushToast("error", t("secReportFail"), errMessage(e).message);
      }
    })();

  const preset = () =>
    void (async () => {
      try {
        await ipc.securityEnvPreset();
        pushToast("info", t("secPresetDone"), t("secPresetDetail"));
      } catch (e) {
        pushToast("error", t("secPresetFail"), errMessage(e).message);
      }
    })();


  return (
    <div className="sec-tab">
      <div className="sec-shield">
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <strong>🛡 {t("shieldTitle")}</strong>
          <button type="button" onClick={toggleShield}>
            {shield ? "ON" : "OFF"}
          </button>
          {tagged !== null && <span className="dim small">{t("shieldTagged")}: {tagged}</span>}
        </div>
        <p className="dim small">{t("shieldDesc")}</p>
        <p className="small" style={{ color: "var(--warn, #e6b455)" }}>⚠ {t("shieldBoundary")}</p>
      </div>
      <p className="dim small">{t("secHint")}</p>
      <div className="sec-actions">
        <button type="button" disabled={busy} onClick={analyze}>
          {t("secAnalyze")}
        </button>
        <button type="button" disabled={busy || !analysis} onClick={() => void runDisasm()}>
          {t("secDisasm")}
        </button>
        <button type="button" onClick={probe}>
          {t("secSandboxProbe")}
        </button>
        <button type="button" disabled={!analysis} onClick={exportReport}>
          {t("secExport")}
        </button>
        <button type="button" onClick={preset}>
          {t("secPreset")}
        </button>
      </div>

      {sandbox && (
        <p className={sandbox.available ? "dim small" : "small"}>
          {sandbox.available ? "🟢" : "🟡"} {sandbox.detail}
        </p>
      )}

      {analysis && (
        <div className="sec-report">
          <div>
            {analysis.isPe ? `PE · ${analysis.machine}` : t("secNotPe")} ·{" "}
            {analysis.signed ? t("secSigned") : t("secUnsigned")}
          </div>
          <div className="dim small ellipsis">BLAKE3: {analysis.blake3}</div>
          <div>
            {t("secPacked")}:
            {analysis.sections.filter((s) => s.suspiciousEntropy).length === 0
              ? ` ${t("secNone")}`
              : ""}
          </div>
          {analysis.sections
            .filter((s) => s.suspiciousEntropy)
            .map((s) => (
              <div key={s.name}>
                ⚠ {s.name} — 熵 {s.entropy}
              </div>
            ))}
          <div>
            {t("secSuspicious")}:{" "}
            {analysis.suspiciousHits.length === 0 ? t("secNone") : analysis.suspiciousHits.join(", ")}
          </div>
          <div>
            {t("secImports")}: {analysis.imports.length} DLL
          </div>
        </div>
      )}

      {disasmLines && disasmLines.length > 0 && (
        <pre className="sec-disasm">
          {disasmLines.map((d) => (
            <div key={d.rva}>
              {d.rva.toString(16).padStart(8, "0")}  {d.bytesHex.padEnd(20)}  {d.text}
            </div>
          ))}
        </pre>
      )}
    </div>
  );
}
