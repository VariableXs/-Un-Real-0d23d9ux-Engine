import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { errMessage, ipc } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { formatBytes } from "../../lib/format";

/**
 * B-28：网络层（设置页「网络」标签）。
 * 环回代理启停 + kill-switch + 白名单规则库（域名确认卡升级：显示发起执行档）
 * + 流量仪表（放行/拒绝/字节数）。直连逃逸如实边界见提示文案。
 */

interface NetStatus {
  proxyRunning: boolean;
  proxyPort: number;
  killSwitch: boolean;
  ruleCount: number;
  bytesRelayed: number;
  connsAllowed: number;
  connsDenied: number;
}
interface NetRule {
  domain: string;
  profile: string;
  grantedAt: number;
}

export function NetworkTab() {
  const { t } = useI18n();
  const [status, setStatus] = useState<NetStatus | null>(null);
  const [rules, setRules] = useState<NetRule[]>([]);
  const [newDomain, setNewDomain] = useState("");

  const refresh = useCallback(async () => {
    try {
      setStatus(await ipc.netStatus());
      setRules(await ipc.netRulesList());
    } catch (e) {
      pushToast("error", t("ntStatusFail"), errMessage(e).message);
    }
  }, [t]);

  useEffect(() => {
    void refresh();
    const id = window.setInterval(() => void refresh(), 3000);
    return () => window.clearInterval(id);
  }, [refresh]);

  const toggleProxy = () =>
    void (async () => {
      try {
        if (status?.proxyRunning) {
          await ipc.netProxyStop();
        } else {
          await ipc.netProxyStart();
        }
        await refresh();
      } catch (e) {
        pushToast("error", t("ntProxyFail"), errMessage(e).message);
      }
    })();

  const toggleKill = () =>
    void (async () => {
      try {
        const r = await ipc.netKillSwitch(!(status?.killSwitch ?? false));
        pushToast(r.on ? "info" : "success", r.on ? t("ntKillOn") : t("ntKillOff"), "");
        await refresh();
      } catch (e) {
        pushToast("error", t("ntProxyFail"), errMessage(e).message);
      }
    })();

  const grant = () =>
    void (async () => {
      if (!newDomain.trim()) return;
      try {
        await ipc.netRuleGrant(newDomain.trim(), "manual");
        setNewDomain("");
        await refresh();
        pushToast("success", t("ntRuleGranted"), "");
      } catch (e) {
        pushToast("error", t("ntRuleFail"), errMessage(e).message);
      }
    })();

  const revoke = (domain: string) =>
    void (async () => {
      try {
        await ipc.netRuleRevoke(domain);
        await refresh();
      } catch (e) {
        pushToast("error", t("ntRuleFail"), errMessage(e).message);
      }
    })();

  return (
    <div className="nt-tab">
      <div className="nt-row">
        <span>{t("ntProxy")}</span>
        <button type="button" className={status?.proxyRunning ? "on" : ""} onClick={toggleProxy}>
          {status?.proxyRunning ? t("ntStop") : t("ntStart")}
        </button>
      </div>
      {status?.proxyRunning && <p className="dim small">127.0.0.1:{status.proxyPort}</p>}

      <div className="nt-row kill">
        <span>{t("ntKillSwitch")}</span>
        <button type="button" className={status?.killSwitch ? "danger on" : ""} onClick={toggleKill}>
          {status?.killSwitch ? t("ntKillDisable") : t("ntKillEnable")}
        </button>
      </div>

      <div className="nt-stats">
        <div>↑↓ {formatBytes(status?.bytesRelayed ?? 0)}</div>
        <div>
          {t("ntAllowed")}: {status?.connsAllowed ?? 0}
        </div>
        <div>
          {t("ntDenied")}: {status?.connsDenied ?? 0}
        </div>
      </div>

      <h4>{t("ntRules")}</h4>
      <div className="nt-add">
        <input
          placeholder={t("ntDomain")}
          value={newDomain}
          onChange={(e) => setNewDomain(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && grant()}
        />
        <button type="button" disabled={!newDomain.trim()} onClick={grant}>
          {t("ntGrant")}
        </button>
      </div>
      {rules.map((r) => (
        <div key={r.domain} className="nt-rule">
          <span>{r.domain}</span>
          <span className="dim small">{r.profile}</span>
          <button type="button" onClick={() => revoke(r.domain)}>
            ✕
          </button>
        </div>
      ))}
      <p className="dim small">{t("ntEscapeNote")}</p>
    </div>
  );
}
