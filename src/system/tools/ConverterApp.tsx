import { useCallback, useEffect, useMemo, useState } from "react";
import { ArrowLeftRight, Copy, Plus, RefreshCw, X } from "lucide-react";
import { useI18n, formatDateTime } from "../../i18n";
import type { Lang } from "../../i18n/dictionaries";
import { ipc } from "../../lib/ipc";
import { requestNetConsent } from "../../lib/netGuard";
import { pushToast } from "../../state/uiStore";
import {
  CONV_CATS,
  CONVERT_TABLE,
  convert,
  convertRate,
  formatConvResult,
  radixConvert,
  type ConvCatId,
} from "../../lib/convert";
import "../../styles/ai08-convert.css";

/**
 * Z-26 换算中心（VWM 虚拟窗口应用，AI-08 基础工具组）：
 * - 单位换算：8 大类，双向即时换算 + 交换
 * - 进制转换：二 / 八 / 十 / 十六互转（程序员模式，u64 值域，超限如实报错）
 * - 汇率换算：手动离线汇率表（可编辑，rates.json 持久化），
 *   「联网刷新」先经 requestNetConsent 征得同意，同意后 ipc.httpFetch
 *   open.er-api.com 并明示更新时间；拒绝 / 失败静默保持离线表
 * 红线：不做时区转换、不做公式记忆、不自动联网（仅用户显式点击并同意）。
 */

/** 本地三语词条（键与 dictionaries.ts 计划键一致，缺失时兜底显示）。 */
const CV_LABELS: Record<string, Record<Lang, string>> = {
  cvTitle: { zh: "换算中心", "zh-TW": "換算中心", en: "Converter Hub" },
  cvTabUnit: { zh: "单位换算", "zh-TW": "單位換算", en: "Units" },
  cvTabRadix: { zh: "进制转换", "zh-TW": "進制轉換", en: "Radix" },
  cvTabRate: { zh: "汇率换算", "zh-TW": "匯率換算", en: "Exchange" },
  cvSwap: { zh: "交换", "zh-TW": "交換", en: "Swap" },
  cvCopy: { zh: "复制", "zh-TW": "複製", en: "Copy" },
  cvCopied: { zh: "已复制", "zh-TW": "已複製", en: "Copied" },
  cvCopyFail: { zh: "复制失败", "zh-TW": "複製失敗", en: "Copy failed" },
  cvResult: { zh: "结果", "zh-TW": "結果", en: "Result" },
  cvCategory: { zh: "类别", "zh-TW": "類別", en: "Category" },
  cvFromVal: { zh: "源数值", "zh-TW": "來源數值", en: "From value" },
  cvToVal: { zh: "目标数值", "zh-TW": "目標數值", en: "To value" },
  cvFromUnit: { zh: "源单位", "zh-TW": "來源單位", en: "From unit" },
  cvToUnit: { zh: "目标单位", "zh-TW": "目標單位", en: "To unit" },
  cvInvalid: { zh: "无效输入", "zh-TW": "無效輸入", en: "Invalid input" },
  cvRadixHint: {
    zh: "值域 u64（0 – 18446744073709551615），超出如实报错",
    "zh-TW": "值域 u64（0 – 18446744073709551615），超出如實報錯",
    en: "u64 range (0 – 18446744073709551615), out-of-range errors shown as-is",
  },
  cvAmount: { zh: "金额", "zh-TW": "金額", en: "Amount" },
  cvAddCur: { zh: "添加货币", "zh-TW": "新增貨幣", en: "Add currency" },
  cvDelRow: { zh: "删除该行", "zh-TW": "刪除該行", en: "Remove row" },
  cvRateHint: {
    zh: "汇率 = 1 USD 可兑换的该货币数量（基准 USD）",
    "zh-TW": "匯率 = 1 USD 可兌換的該貨幣數量（基準 USD）",
    en: "Rate = amount per 1 USD (base USD)",
  },
  cvRefresh: { zh: "联网刷新", "zh-TW": "聯網刷新", en: "Refresh online" },
  cvFetching: { zh: "获取中…", "zh-TW": "獲取中…", en: "Fetching…" },
  cvUpdated: { zh: "更新时间", "zh-TW": "更新時間", en: "Updated" },
  cvUpdatedOffline: {
    zh: "离线近似表（可手动编辑）",
    "zh-TW": "離線近似表（可手動編輯）",
    en: "Offline approximations (editable)",
  },
  cvNetPurpose: { zh: "更新换算中心的离线汇率表", "zh-TW": "更新換算中心的離線匯率表", en: "Refresh exchange rate table" },
  cvRefreshOk: { zh: "汇率已更新", "zh-TW": "匯率已更新", en: "Rates updated" },
};

type ConvTab = "unit" | "radix" | "rate";

const TABS: { id: ConvTab; key: string }[] = [
  { id: "unit", key: "cvTabUnit" },
  { id: "radix", key: "cvTabRadix" },
  { id: "rate", key: "cvTabRate" },
];

/** 汇率表落盘文件名（tools/rates.json，经 ipc.toolDataWrite）。 */
const RATES_FILE = "rates.json";
/** 联网刷新数据源（出站前必须经 requestNetConsent 同意）。 */
const ER_API = "https://open.er-api.com/v6/latest/USD";

/** 默认离线近似汇率（静态，仅供无网络 / 未刷新时使用）。 */
const DEFAULT_RATES: [string, string][] = [
  ["USD", "1"],
  ["CNY", "7.24"],
  ["EUR", "0.92"],
  ["JPY", "151.5"],
  ["GBP", "0.79"],
  ["HKD", "7.81"],
  ["TWD", "32.1"],
  ["KRW", "1330"],
  ["SGD", "1.34"],
  ["AUD", "1.52"],
  ["CAD", "1.36"],
  ["CHF", "0.9"],
  ["THB", "36.4"],
  ["MYR", "4.72"],
  ["RUB", "92.5"],
  ["INR", "83.3"],
];

const BASES: { base: number; label: string }[] = [
  { base: 2, label: "BIN" },
  { base: 8, label: "OCT" },
  { base: 10, label: "DEC" },
  { base: 16, label: "HEX" },
];

/** 纯换算 + 格式化（输入非法 / 单位非法 → 空串，由 UI 显示占位）。 */
function safeConv(raw: string, catId: ConvCatId, from: string, to: string): string {
  if (raw.trim() === "") return "";
  const v = Number(raw);
  if (Number.isNaN(v)) return "";
  try {
    return formatConvResult(convert(v, catId, from, to));
  } catch {
    return "";
  }
}

/** rows → 有效汇率 map（货币代码大写归一，剔除空/非法行）。 */
function rowsToRates(rows: [string, string][]): Record<string, number> {
  const m: Record<string, number> = {};
  for (const [cur, rate] of rows) {
    const v = Number(rate);
    const c = cur.trim().toUpperCase();
    if (c && Number.isFinite(v) && v > 0) m[c] = v;
  }
  return m;
}

export function ConverterApp(_props: { winId: string }): React.ReactElement {
  const { lang, t } = useI18n();
  const tt = useCallback((key: string) => CV_LABELS[key]?.[lang] ?? t(key), [lang, t]);

  const [tab, setTab] = useState<ConvTab>("unit");

  // ---------- 单位换算 ----------
  const [cat, setCat] = useState<ConvCatId>("length");
  const [fromU, setFromU] = useState("km");
  const [toU, setToU] = useState("m");
  const [fromVal, setFromVal] = useState("1");
  const [toVal, setToVal] = useState<string>(() => safeConv("1", "length", "km", "m"));

  // ---------- 进制转换 ----------
  const [activeBase, setActiveBase] = useState(10);
  const [baseInput, setBaseInput] = useState("255");

  // ---------- 汇率换算 ----------
  const [rows, setRows] = useState<[string, string][]>(() => DEFAULT_RATES.map((r) => [...r] as [string, string]));
  const [updatedAt, setUpdatedAt] = useState<number | null>(null);
  const [fetching, setFetching] = useState(false);
  const [amount, setAmount] = useState("100");
  const [rateFrom, setRateFrom] = useState("USD");
  const [rateTo, setRateTo] = useState("CNY");

  const rates = useMemo(() => rowsToRates(rows), [rows]);

  // 汇率表加载（tools/rates.json；损坏 / 缺失静默用默认表）
  useEffect(() => {
    let disposed = false;
    ipc
      .toolDataRead(RATES_FILE)
      .then((raw) => {
        if (disposed || !raw) return;
        try {
          const data = JSON.parse(raw) as { updatedAt?: unknown; rates?: unknown };
          if (data.rates && typeof data.rates === "object") {
            const entries = Object.entries(data.rates as Record<string, unknown>).filter(
              ([, v]) => typeof v === "number" && (v as number) > 0,
            );
            if (entries.length > 0) {
              setRows(entries.map(([c, v]) => [c, String(v)] as [string, string]));
            }
          }
          if (typeof data.updatedAt === "number") setUpdatedAt(data.updatedAt);
        } catch {
          /* 损坏数据静默忽略，保持默认离线表 */
        }
      })
      .catch(() => {});
    return () => {
      disposed = true;
    };
  }, []);

  /** 汇率表落盘（失败静默，不打扰编辑）。 */
  const persistRates = (nextRows: [string, string][], ts: number | null): void => {
    try {
      void ipc
        .toolDataWrite(RATES_FILE, JSON.stringify({ updatedAt: ts, rates: rowsToRates(nextRows) }))
        .catch(() => {});
    } catch {
      /* 非 Tauri 环境（无 invoke）静默跳过 */
    }
  };

  const copyText = (s: string): void => {
    if (!s) return;
    void navigator.clipboard
      .writeText(s)
      .then(() => pushToast("success", tt("cvTitle"), `${s} ${tt("cvCopied")}`))
      .catch(() => pushToast("error", tt("cvTitle"), tt("cvCopyFail")));
  };

  // ----- 单位换算：双向即时 -----
  const onFromVal = (s: string): void => {
    setFromVal(s);
    setToVal(safeConv(s, cat, fromU, toU));
  };
  const onToVal = (s: string): void => {
    setToVal(s);
    setFromVal(safeConv(s, cat, toU, fromU));
  };
  const onCatChange = (c: ConvCatId): void => {
    const us = CONVERT_TABLE[c].units;
    const nf = us[0]!.id;
    const nt = us[1]!.id;
    setCat(c);
    setFromU(nf);
    setToU(nt);
    setToVal(safeConv(fromVal, c, nf, nt));
  };
  const onFromU = (u: string): void => {
    setFromU(u);
    setToVal(safeConv(fromVal, cat, u, toU));
  };
  const onToU = (u: string): void => {
    setToU(u);
    setToVal(safeConv(fromVal, cat, fromU, u));
  };
  const swapUnits = (): void => {
    const nf = toU;
    const nt = fromU;
    const nv = toVal;
    setFromU(nf);
    setToU(nt);
    setFromVal(nv);
    setToVal(safeConv(nv, cat, nf, nt));
  };

  // ----- 进制：错误信息 + 各进制展示值 -----
  const radixErr = useMemo(() => {
    try {
      radixConvert(baseInput, activeBase, 10);
      return "";
    } catch (e) {
      return e instanceof Error ? e.message : "错误";
    }
  }, [baseInput, activeBase]);

  const radixValueOf = (base: number): string => {
    if (radixErr) return "";
    try {
      return radixConvert(baseInput, activeBase, base);
    } catch {
      return "";
    }
  };

  // 货币下拉选项（仅有效行）；当前选中项被删时回落到第一项
  const curList = useMemo(() => Object.keys(rates), [rates]);
  const effFrom = rates[rateFrom] !== undefined ? rateFrom : (curList[0] ?? "USD");
  const effTo = rates[rateTo] !== undefined ? rateTo : (curList[1] ?? curList[0] ?? "USD");

  // ----- 汇率：结果（使用校正后的 effFrom/effTo，货币被删行后自动回落） -----
  const rateResult = useMemo(() => {
    if (amount.trim() === "") return "";
    const v = Number(amount);
    if (Number.isNaN(v)) return "";
    const rTo = rates[effTo];
    if (typeof rTo !== "number" || !(rTo > 0)) return "";
    try {
      return formatConvResult(convertRate(v, effFrom, rates) * rTo);
    } catch {
      return "";
    }
  }, [amount, effFrom, effTo, rates]);

  /** 联网刷新：先征得同意，失败 / 拒绝静默保持离线表（红线）。 */
  const refreshRates = async (): Promise<void> => {
    if (fetching) return;
    setFetching(true);
    try {
      const decision = await requestNetConsent(ER_API, tt("cvNetPurpose"));
      if (decision !== "once" && decision !== "always") return; // 拒绝：静默保持离线表
      const raw = await ipc.httpFetch(ER_API);
      const data = JSON.parse(raw) as { rates?: Record<string, number> };
      if (!data.rates || typeof data.rates.USD !== "number") throw new Error("响应缺少 rates");
      // 只更新表内已有货币（保留用户手添行；接口未返回的保持原值）
      const nextRows = rows.map(([c, r]) => {
        const v = data.rates?.[c.trim().toUpperCase()];
        return typeof v === "number" && v > 0 ? ([c.trim().toUpperCase(), String(v)] as [string, string]) : ([c, r] as [string, string]);
      });
      setRows(nextRows);
      const now = Date.now();
      setUpdatedAt(now);
      persistRates(nextRows, now);
      pushToast("success", tt("cvTitle"), tt("cvRefreshOk"));
    } catch {
      /* 失败：静默保持离线表 */
    } finally {
      setFetching(false);
    }
  };

  const unitLabel = (id: string, zh: string, en: string): string => (lang === "en" ? `${id} · ${en}` : `${id} · ${zh}`);

  return (
    <div className="cv-app" role="application" aria-label={tt("cvTitle")}>
      <div className="cv-tabs" role="tablist">
        {TABS.map((m) => (
          <button
            key={m.id}
            type="button"
            role="tab"
            aria-selected={tab === m.id}
            className={`cv-tab${tab === m.id ? " active" : ""}`}
            onClick={() => setTab(m.id)}
          >
            {tt(m.key)}
          </button>
        ))}
      </div>

      {tab === "unit" && (
        <div className="cv-panel">
          <div className="cv-unit-cat-row">
            <span className="dim small">{tt("cvCategory")}</span>
            <select
              className="text-input"
              value={cat}
              onChange={(e) => onCatChange(e.target.value as ConvCatId)}
              aria-label={tt("cvCategory")}
            >
              {CONV_CATS.map((c) => (
                <option key={c} value={c}>
                  {lang === "en" ? CONVERT_TABLE[c].en : CONVERT_TABLE[c].zh}
                </option>
              ))}
            </select>
          </div>

          <div className="cv-unit-grid">
            <div className="cv-unit-side">
              <input
                className="text-input cv-unit-val"
                value={fromVal}
                onChange={(e) => onFromVal(e.target.value)}
                aria-label={tt("cvFromVal")}
                inputMode="decimal"
              />
              <select
                className="text-input"
                value={fromU}
                onChange={(e) => onFromU(e.target.value)}
                aria-label={tt("cvFromUnit")}
              >
                {CONVERT_TABLE[cat].units.map((u) => (
                  <option key={u.id} value={u.id}>
                    {unitLabel(u.id, u.zh, u.en)}
                  </option>
                ))}
              </select>
            </div>
            <button
              type="button"
              className="btn ghost tiny cv-swap"
              onClick={swapUnits}
              aria-label={tt("cvSwap")}
              title={tt("cvSwap")}
            >
              <ArrowLeftRight size={14} />
            </button>
            <div className="cv-unit-side">
              <input
                className="text-input cv-unit-val"
                value={toVal}
                onChange={(e) => onToVal(e.target.value)}
                aria-label={tt("cvToVal")}
                inputMode="decimal"
              />
              <select
                className="text-input"
                value={toU}
                onChange={(e) => onToU(e.target.value)}
                aria-label={tt("cvToUnit")}
              >
                {CONVERT_TABLE[cat].units.map((u) => (
                  <option key={u.id} value={u.id}>
                    {unitLabel(u.id, u.zh, u.en)}
                  </option>
                ))}
              </select>
            </div>
          </div>

          <div className="cv-unit-result">
            <span className="dim small">{tt("cvResult")}</span>
            <span className="cv-unit-result-num" aria-live="polite">
              {toVal || "—"}
            </span>
            <button type="button" className="btn ghost tiny" onClick={() => copyText(toVal)} disabled={!toVal}>
              <Copy size={12} /> {tt("cvCopy")}
            </button>
          </div>
          {!toVal && fromVal.trim() !== "" && <p className="cv-radix-error">{tt("cvInvalid")}</p>}
        </div>
      )}

      {tab === "radix" && (
        <div className="cv-panel">
          <div className="cv-radix-rows">
            {BASES.map((b) => {
              const isEditing = activeBase === b.base;
              const shown = isEditing ? baseInput : radixValueOf(b.base);
              return (
                <div className="cv-radix-row" key={b.base}>
                  <label className="cv-radix-label" htmlFor={`cv-radix-${b.base}`}>
                    {b.label}
                  </label>
                  <input
                    id={`cv-radix-${b.base}`}
                    className={`text-input cv-radix-input${isEditing && radixErr ? " invalid" : ""}`}
                    value={shown}
                    onChange={(e) => {
                      setActiveBase(b.base);
                      setBaseInput(e.target.value);
                    }}
                    onFocus={() => setActiveBase(b.base)}
                    spellCheck={false}
                    aria-label={b.label}
                  />
                  <button
                    type="button"
                    className="btn ghost tiny"
                    onClick={() => copyText(radixValueOf(b.base))}
                    disabled={!!radixErr}
                    aria-label={`${tt("cvCopy")} ${b.label}`}
                  >
                    <Copy size={12} />
                  </button>
                </div>
              );
            })}
          </div>
          {radixErr && <p className="cv-radix-error" role="alert">{radixErr}</p>}
          <p className="dim small">{tt("cvRadixHint")}</p>
        </div>
      )}

      {tab === "rate" && (
        <div className="cv-panel">
          <div className="cv-rate-head">
            <button
              type="button"
              className="btn primary tiny"
              onClick={() => void refreshRates()}
              disabled={fetching}
            >
              <RefreshCw size={12} /> {fetching ? tt("cvFetching") : tt("cvRefresh")}
            </button>
            <span className="dim small cv-rate-meta">
              {updatedAt
                ? `${tt("cvUpdated")}: ${formatDateTime(updatedAt, lang)}`
                : tt("cvUpdatedOffline")}
            </span>
          </div>

          <div className="cv-rate-conv">
            <input
              className="text-input cv-rate-amount"
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
              aria-label={tt("cvAmount")}
              inputMode="decimal"
            />
            <select
              className="text-input"
              value={effFrom}
              onChange={(e) => setRateFrom(e.target.value)}
              aria-label={tt("cvFromVal")}
            >
              {curList.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
            <select
              className="text-input"
              value={effTo}
              onChange={(e) => setRateTo(e.target.value)}
              aria-label={tt("cvToVal")}
            >
              {curList.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
          </div>

          <div className="cv-rate-result">
            <span className="cv-rate-result-num" aria-live="polite">
              {rateResult || "—"}
            </span>
            <button type="button" className="btn ghost tiny" onClick={() => copyText(rateResult)} disabled={!rateResult}>
              <Copy size={12} /> {tt("cvCopy")}
            </button>
          </div>

          <p className="dim small">{tt("cvRateHint")}</p>
          <div className="cv-rate-table" role="table" aria-label={tt("cvTabRate")}>
            {rows.map(([cur, rate], i) => (
              <div className="cv-rate-row" key={i}>
                <input
                  className="text-input cv-rate-cur"
                  value={cur}
                  onChange={(e) => {
                    const next = rows.map((r, j) => (j === i ? ([e.target.value, r[1]] as [string, string]) : r));
                    setRows(next);
                    persistRates(next, updatedAt);
                  }}
                  aria-label={`currency ${i + 1}`}
                  spellCheck={false}
                />
                <input
                  className="text-input cv-rate-val"
                  value={rate}
                  onChange={(e) => {
                    const next = rows.map((r, j) => (j === i ? ([r[0], e.target.value] as [string, string]) : r));
                    setRows(next);
                    persistRates(next, updatedAt);
                  }}
                  aria-label={`rate ${i + 1}`}
                  inputMode="decimal"
                />
                <button
                  type="button"
                  className="icon-btn tiny"
                  onClick={() => {
                    const next = rows.filter((_, j) => j !== i);
                    setRows(next);
                    persistRates(next, updatedAt);
                  }}
                  aria-label={tt("cvDelRow")}
                  title={tt("cvDelRow")}
                >
                  <X size={12} />
                </button>
              </div>
            ))}
          </div>
          <button
            type="button"
            className="btn ghost tiny"
            onClick={() => {
              const next = [...rows, ["", ""] as [string, string]];
              setRows(next);
            }}
          >
            <Plus size={12} /> {tt("cvAddCur")}
          </button>
        </div>
      )}
    </div>
  );
}
