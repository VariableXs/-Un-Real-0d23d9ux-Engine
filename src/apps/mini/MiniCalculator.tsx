import { useState } from "react";
import { useI18n } from "../../i18n";
import { calcEval, fmtResult } from "../../lib/calc";

/**
 * U-18 迷你应用：迷你计算器。
 * - 四则 + 括号求值：直接复用 lib/calc 的 calcEval（递归下降求值器，
 *   传 ×/÷ 字面量即可，tokenizer 内建映射到 * /）——不自造轮子
 * - 历史条：本窗口内的求值记录（会话态，不持久化），点击回填表达式
 * - 错误如实展示（calcEval 抛错 = 表达式非法）
 */

interface HistoryEntry {
  expr: string;
  result: string;
}

const KEYS: ReadonlyArray<{ k: string; cls?: string; ins?: string }> = [
  { k: "C", cls: "fn" },
  { k: "(", cls: "fn" },
  { k: ")", cls: "fn" },
  { k: "⌫", cls: "fn" },
  { k: "7" }, { k: "8" }, { k: "9" }, { k: "÷", cls: "op" },
  { k: "4" }, { k: "5" }, { k: "6" }, { k: "×", cls: "op" },
  { k: "1" }, { k: "2" }, { k: "3" }, { k: "−", cls: "op", ins: "-" },
  { k: "0" }, { k: "." }, { k: "+", cls: "op" }, { k: "=", cls: "eq" },
];

const HISTORY_CAP = 8;

export function MiniCalculator(): React.ReactElement {
  const { t } = useI18n();
  const [expr, setExpr] = useState("");
  const [result, setResult] = useState<string | null>(null);
  const [error, setError] = useState(false);
  const [history, setHistory] = useState<HistoryEntry[]>([]);

  const evaluate = (): void => {
    const e = expr.trim();
    if (!e) return;
    try {
      const v = calcEval(e);
      const r = fmtResult(v);
      setResult(r);
      setError(false);
      setHistory((h) => [{ expr: e, result: r }, ...h].slice(0, HISTORY_CAP));
    } catch {
      setResult(null);
      setError(true);
    }
  };

  const press = (key: string, ins?: string): void => {
    if (key === "=") {
      evaluate();
      return;
    }
    if (key === "C") {
      setExpr("");
      setResult(null);
      setError(false);
      return;
    }
    if (key === "⌫") {
      setExpr((s) => s.slice(0, -1));
      setError(false);
      return;
    }
    setError(false);
    setExpr((s) => s + (ins ?? key));
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>): void => {
    if (e.key === "Enter") {
      e.preventDefault();
      evaluate();
    }
  };

  return (
    <div className="mini-calc">
      <input
        className="text-input mini-calc-expr"
        type="text"
        value={expr}
        placeholder={t("miniCalcPlaceholder")}
        aria-label={t("miniCalculatorTitle")}
        spellCheck={false}
        onChange={(e) => {
          setExpr(e.target.value);
          setError(false);
        }}
        onKeyDown={onKeyDown}
      />
      <div className={`mini-calc-result${error ? " err" : ""}`}>
        {error ? t("miniCalcError") : result !== null ? `= ${result}` : ""}
      </div>

      <div className="mini-calc-pad">
        {KEYS.map((b) => (
          <button
            key={b.k}
            type="button"
            className={`mini-calc-key${b.cls ? ` ${b.cls}` : ""}`}
            onClick={() => press(b.k, b.ins)}
          >
            {b.k}
          </button>
        ))}
      </div>

      {history.length > 0 && (
        <div className="mini-calc-history" aria-label={t("miniCalcHistory")}>
          {history.map((h, i) => (
            <button
              key={`${i}-${h.expr}`}
              type="button"
              className="mini-calc-hitem"
              title={h.expr}
              onClick={() => {
                setExpr(h.expr);
                setResult(h.result);
                setError(false);
              }}
            >
              <span className="ellipsis">{h.expr}</span>
              <span className="mini-calc-hres">= {h.result}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
