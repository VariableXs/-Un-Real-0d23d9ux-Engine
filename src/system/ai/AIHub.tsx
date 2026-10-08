import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Download, Plus, ShieldCheck, TerminalSquare, Trash2, UserRound } from "lucide-react";
import { Modal } from "../../components/Modal";
import { errMessage, ipc, type AiIdentityView, type AiProgress, type AiToolStatus, type AiVerifyRow } from "../../lib/ipc";
import { requestNetConsent } from "../../lib/netGuard";
import { useI18n } from "../../i18n";
import { pushToast, uiStore, useUi } from "../../state/uiStore";
import { launchThirdApp } from "../launcher/thirdApps";

/**
 * AI Hub（批次B-9/B-10，BLUEPRINT 3.6 / 10.1，桌面窗口模态）：
 * - 三态卡片：未安装 / 未登录 / 已登录（登录态为容器配置标记推断，不读凭据本体）
 * - Node 运行时与 CLI 安装：出站（nodejs.org / registry.npmjs.org）逐域弹窗授权，
 *   进度经 ai://progress 事件流回传（真实字节计数）
 * - 身份库（Vault 2.0）：Token 只存保险箱加密区；保险箱未锁时如实提示
 * - 唤起 = 把工具+身份写入终端执行档后走既有 embed 通道打开便携 Windows Terminal
 */

export function AIHub(): React.ReactElement | null {
  const { t } = useI18n();
  const open = useUi((s) => s.aiHubOpen);
  const [tools, setTools] = useState<AiToolStatus[]>([]);
  const [identities, setIdentities] = useState<AiIdentityView[] | null>(null);
  const [progress, setProgress] = useState<AiProgress | null>(null);
  const [verify, setVerify] = useState<AiVerifyRow[] | null>(null);
  const [form, setForm] = useState<{ tool: string; label: string; token: string; note: string } | null>(null);
  const [busy, setBusy] = useState(false);

  async function reload(): Promise<void> {
    try {
      setTools(await ipc.aiToolStatus());
    } catch (e) {
      pushToast("error", t("aiTitle"), errMessage(e).message);
    }
    try {
      setIdentities(await ipc.identityList());
    } catch {
      setIdentities(null); // 保险箱未解锁等 → 如实提示，不假装空库
    }
  }

  useEffect(() => {
    if (open) void reload();
  }, [open]);

  // B-8：安装进度事件流（真实字节计数；done/error 后刷新状态）
  useEffect(() => {
    const un = listen<AiProgress>("ai://progress", (e) => {
      const p = e.payload;
      setProgress(p.phase === "done" || p.phase === "error" ? null : p);
      if (p.phase === "done") {
        pushToast("success", p.tool, p.message);
        void reload();
      } else if (p.phase === "error") {
        pushToast("error", p.tool, p.message);
      }
    });
    return () => {
      void un.then((f) => f()).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function installNode(): Promise<void> {
    const d = await requestNetConsent("https://nodejs.org", t("aiInstallNode"));
    if (d === "deny") return;
    try {
      await ipc.aiInstallNode();
    } catch (e) {
      pushToast("error", t("aiInstallNode"), errMessage(e).message);
    }
  }

  async function installTool(toolId: string): Promise<void> {
    const d = await requestNetConsent("https://registry.npmjs.org", t("aiInstallTool"));
    if (d === "deny") return;
    try {
      await ipc.aiInstallTool(toolId);
    } catch (e) {
      pushToast("error", t("aiInstallTool"), errMessage(e).message);
    }
  }

  async function openTerminal(toolId: string, identityId?: string): Promise<void> {
    setBusy(true);
    try {
      const termId = await ipc.aiLaunch(toolId, identityId);
      await reload();
      await launchThirdApp(termId, "Variable Terminal");
      uiStore.setState({ aiHubOpen: false });
    } catch (e) {
      pushToast("error", t("aiTitle"), errMessage(e).message);
    } finally {
      setBusy(false);
    }
  }

  async function addIdentity(): Promise<void> {
    if (!form) return;
    setBusy(true);
    try {
      await ipc.identityAdd(form.tool, form.label, form.token, form.note);
      setForm(null);
      await reload();
    } catch (e) {
      pushToast("error", t("aiIdentities"), errMessage(e).message);
    } finally {
      setBusy(false);
    }
  }

  async function removeIdentity(id: string): Promise<void> {
    try {
      await ipc.identityRemove(id);
      await reload();
    } catch (e) {
      pushToast("error", t("aiIdentities"), errMessage(e).message);
    }
  }

  async function doVerify(): Promise<void> {
    setBusy(true);
    try {
      setVerify(await ipc.aiVerify());
    } catch (e) {
      pushToast("error", t("aiVerify"), errMessage(e).message);
    } finally {
      setBusy(false);
    }
  }

  if (!open) return null;

  const nodeReady = tools.length > 0 && tools.every((x) => x.nodeInstalled);

  return (
    <Modal open title={t("aiTitle")} onClose={() => uiStore.setState({ aiHubOpen: false })} width={720}>
      <>
        <p className="dim small">{t("aiHint")}</p>

        {!nodeReady && (
          <div className="row gap8" style={{ margin: "8px 0" }}>
            <button type="button" className="btn primary" disabled={busy || !!progress} onClick={() => void installNode()}>
              <Download size={13} /> {progress ? `${t("aiNodeInstalling")} ${progress.message}` : t("aiInstallNode")}
            </button>
          </div>
        )}

        <div className="ai-card-grid">
          {tools.map((tool) => {
            const toolIdentities = (identities ?? []).filter((i) => i.tool === tool.id);
            return (
              <div key={tool.id} className="ai-card">
                <div className="row gap8">
                  <strong className="ellipsis">{tool.name}</strong>
                  <span className="flex-1" />
                  <span className={`ai-state ${tool.installed ? (tool.loggedIn ? "ok" : "warn") : "off"}`}>
                    {!tool.installed
                      ? t("aiStateNotInstalled")
                      : tool.loggedIn
                        ? t("aiStateLoggedIn")
                        : t("aiStateNotLoggedIn")}
                  </span>
                </div>
                <p className="dim small">
                  {tool.installed && tool.lastActivityMs
                    ? `${t("aiLastActivity")} ${new Date(tool.lastActivityMs).toLocaleString()}`
                    : tool.installed
                      ? t("aiNoActivity")
                      : tool.npmPackage}
                </p>
                <div className="row gap8">
                  {!tool.installed ? (
                    <button type="button" className="btn ghost" disabled={busy || !nodeReady || !!progress} onClick={() => void installTool(tool.id)}>
                      <Download size={13} /> {t("aiInstallTool")}
                    </button>
                  ) : (
                    <button type="button" className="btn primary" disabled={busy} onClick={() => void openTerminal(tool.id)}>
                      <TerminalSquare size={13} /> {tool.loggedIn ? t("aiOpenTerminal") : t("aiLoginTerminal")}
                    </button>
                  )}
                  <button type="button" className="btn ghost" disabled={busy || !tool.installed} onClick={() => setForm({ tool: tool.id, label: "", token: "", note: "" })}>
                    <Plus size={13} /> {t("aiIdentityAdd")}
                  </button>
                </div>
                {toolIdentities.length > 0 && (
                  <div className="ai-identities">
                    {toolIdentities.map((i) => (
                      <div key={i.id} className="backup-row">
                        <UserRound size={12} />
                        <span className="small">@{i.label}</span>
                        <span className="dim small">…{i.tokenTail}</span>
                        <span className="flex-1" />
                        <button type="button" className="icon-btn tiny" title={t("aiSwitch")} onClick={() => void openTerminal(tool.id, i.id)}>▶</button>
                        <button type="button" className="icon-btn tiny danger-hover" title={t("aiDelete")} onClick={() => void removeIdentity(i.id)}><Trash2 size={12} /></button>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
          {tools.length === 0 && <p className="dim small">…</p>}
        </div>

        {identities === null && <p className="dim small" style={{ marginTop: 8 }}>🔒 {t("aiVaultLocked")}</p>}

        {form && (
          <div className="ai-form">
            <h4>{t("aiIdentityAdd")} — {tools.find((x) => x.id === form.tool)?.name ?? form.tool}</h4>
            <div className="row gap8">
              <input className="small" style={{ width: 120 }} placeholder={t("aiIdentityLabel")} value={form.label} onChange={(e) => setForm({ ...form, label: e.target.value })} />
              <input className="small flex-1" type="password" placeholder={t("aiIdentityToken")} value={form.token} onChange={(e) => setForm({ ...form, token: e.target.value })} />
            </div>
            <div className="row gap8" style={{ marginTop: 6 }}>
              <input className="small flex-1" placeholder={t("aiIdentityNote")} value={form.note} onChange={(e) => setForm({ ...form, note: e.target.value })} />
              <button type="button" className="btn primary" disabled={busy || !form.label || !form.token} onClick={() => void addIdentity()}>{t("aiSave")}</button>
              <button type="button" className="btn ghost" onClick={() => setForm(null)}>{t("aiCancel")}</button>
            </div>
          </div>
        )}

        <div className="row gap8" style={{ marginTop: 12 }}>
          <button type="button" className="btn ghost" disabled={busy} onClick={() => void doVerify()}>
            <ShieldCheck size={13} /> {t("aiVerify")}
          </button>
        </div>
        {verify && (
          <div className="backup-list" style={{ marginTop: 8 }}>
            {verify.map((r) => (
              <div key={r.id} className="backup-row">
                <span className="small">{r.name}</span>
                <span className="flex-1" />
                {r.hostResidue.length > 0 ? (
                  <span className="small" style={{ color: "var(--danger, #e5484d)" }}>
                    {t("aiVerifyResidue").replace("{n}", String(r.hostResidue.length))}
                  </span>
                ) : (
                  <span className="dim small">{t("aiVerifyClean")}</span>
                )}
              </div>
            ))}
          </div>
        )}
      </>
    </Modal>
  );
}
