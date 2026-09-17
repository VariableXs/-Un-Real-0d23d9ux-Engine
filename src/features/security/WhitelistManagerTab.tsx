/**
 * 任务 36（AI-V）：白名单管理 UI（设置页新标签）。
 *
 * 视觉：沿用既有设置页 .w11-legacy / .st-* 版式（Win11 风格逐像素对照既有 Tab）。
 * 逻辑：规则增删改查 + 即时生效（下发命令 + 状态回读）+ 变更审计自身留痕。
 * 降级：内核通道未接入时走内存镜像 + localStorage，并显示「本地预览态」角标，绝不静默成功。
 */
import { useEffect, useRef, useState } from "react";
import "./security.css";
import { useI18n } from "../../i18n";
import { pushToast } from "../../state/uiStore";
import { EmptyState } from "../../components/EmptyState";
import { getDefaultBridge } from "./whitelistBridge";
import type { WhitelistError, WhitelistRule } from "./whitelistTypes";

function errKey(code: WhitelistError): string {
  switch (code) {
    case "NOT_ABSOLUTE":
      return "wlErrAbs";
    case "BAD_CHAR":
      return "wlErrBadChar";
    case "RULES_FULL":
      return "wlErrFull";
    case "KERNEL_UNREACHABLE":
      return "wlErrKernel";
    default:
      return "wlErrUnknown";
  }
}

export function WhitelistManagerTab(): React.ReactElement {
  const { t } = useI18n();
  const bridge = getDefaultBridge();
  const [, force] = useState(0);
  const [prefix, setPrefix] = useState("");
  const [read, setRead] = useState(true);
  const [write, setWrite] = useState(false);
  const [busy, setBusy] = useState(false);
  const prefixRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    let alive = true;
    if (!bridge.isInitialized()) {
      void bridge.init().then(() => alive && force((n) => n + 1));
    }
    const off = bridge.onChange(() => alive && force((n) => n + 1));
    return () => {
      alive = false;
      off();
    };
  }, [bridge]);

  const rules = bridge.getRules();
  const local = bridge.isLocalPreview();

  const add = async (): Promise<void> => {
    if (busy) return;
    setBusy(true);
    try {
      const r = await bridge.addRule({ prefix, read, write });
      if (!r.ok) {
        const msg = t(errKey(r.code));
        pushToast("error", t("wlTitle"), msg);
      } else {
        pushToast("success", t("wlRuleAdded"), `${read ? "r" : ""}${write ? "w" : ""} ${prefix}`);
        setPrefix("");
      }
    } finally {
      setBusy(false);
    }
  };

  const remove = async (index: number): Promise<void> => {
    if (busy) return;
    setBusy(true);
    try {
      const r = await bridge.removeRule(index);
      if (!r.ok) pushToast("error", t("wlTitle"), t(errKey(r.code)));
      else pushToast("success", t("wlRuleRemoved"), "");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="wl-tab">
      <p className="dim small">{t("wlDesc")}</p>

      {local && (
        <div className="wl-badge" role="status" data-testid="wl-badge">
          ⚠ {t("wlBadgeLocal")}
        </div>
      )}

      <h4 className="st-sec-title">{t("wlAddTitle")}</h4>
      <label className="st-field">
        {t("wlFieldPrefix")}
        <input
          value={prefix}
          placeholder={t("wlFieldPrefixPh")}
          data-testid="wl-prefix-input"
          ref={prefixRef}
          onChange={(e) => setPrefix(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void add();
          }}
        />
      </label>
      <p className="dim small">{t("wlWildcardHint")}</p>
      <div className="st-actions">
        <label className="st-field-inline small">
          <input type="checkbox" checked={read} onChange={(e) => setRead(e.target.checked)} />
          {t("wlChkRead")}
        </label>
        <label className="st-field-inline small">
          <input type="checkbox" checked={write} onChange={(e) => setWrite(e.target.checked)} />
          {t("wlChkWrite")}
        </label>
        <button type="button" disabled={busy} onClick={() => void add()} data-testid="wl-add-btn">
          {t("wlBtnAdd")}
        </button>
      </div>

      <h4 className="st-sec-title">{t("wlRulesTitle")}</h4>
      {rules.length === 0 ? (
        <EmptyState
          icon="lock"
          title={t("wlEmptyTitle")}
          description={t("wlEmptyDesc")}
          actionLabel={t("wlBtnAdd")}
          onAction={() => {
            prefixRef.current?.focus();
          }}
        />
      ) : (
        <div className="wl-rules" data-testid="wl-rules-list">
          <div className="wl-rules-head">
            <span>{t("wlColPrefix")}</span>
            <span>{t("wlColRead")}</span>
            <span>{t("wlColWrite")}</span>
            <span>{t("wlColActions")}</span>
          </div>
          {rules.map((r: WhitelistRule) => (
            <div className="wl-rule-row" key={r.index}>
              <span className="wl-prefix mono">
                {r.prefix}
                {r.wildcard ? "/*" : ""}
              </span>
              <span>{r.read ? "✓" : "—"}</span>
              <span>{r.write ? "✓" : "—"}</span>
              <span>
                <button
                  type="button"
                  className="btn ghost"
                  disabled={busy}
                  onClick={() => void remove(r.index)}
                >
                  {t("wlBtnRemove")}
                </button>
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
