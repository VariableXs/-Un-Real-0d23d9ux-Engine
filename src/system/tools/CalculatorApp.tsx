import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Delete } from "lucide-react";
import { useI18n } from "../../i18n";
import type { Lang } from "../../i18n/dictionaries";
import {
  UNIT_TABLE,
  calcEval,
  fmtResult,
  progEval,
  progFormat,
  progParse,
  unitConvert,
  type CalcAngle,
  type ProgBase,
  type ProgOp,
  type UnitCategory,
} from "../../lib/calc";

/**
 * F-2.1 计算器（VWM 虚拟窗口应用）：
 * 标准 / 科学 / 程序员 / 单位换算四模式 + 历史记录（本窗口实例内保留）+ 键盘全支持。
 * 计算全部走引擎本地（src/lib/calc.ts，零依赖、零网络）。
 */

type CalcMode = "std" | "sci" | "prog" | "conv";

const MODES: { id: CalcMode; key: string }[] = [
  { id: "std", key: "calcModeStd" },
  { id: "sci", key: "calcModeSci" },
  { id: "prog", key: "calcModeProg" },
  { id: "conv", key: "calcModeConv" },
];

const MODE_LABELS: Record<string, Record<Lang, string>> = {
  calcModeStd: { zh: "标准", "zh-TW": "標準", en: "Standard" },
  calcModeSci: { zh: "科学", "zh-TW": "科學", en: "Scientific" },
  calcModeProg: { zh: "程序员", "zh-TW": "程式設計", en: "Programmer" },
  calcModeConv: { zh: "换算", "zh-TW": "換算", en: "Convert" },
};

interface HistoryItem {
  expr: string;
  result: string;
}

/** 按钮定义：label 显示文本；ins 插入/动作；wide 跨两列。 */
interface KeyDef {
  label: string;
  ins?: string;
  act?: "eq" | "clear" | "back";
  cls?: string;
  wide?: boolean;
}

const STD_KEYS: KeyDef[] = [
  { label: "C", act: "clear", cls: "calc-key-fn" },
  { label: "( )", ins: "(", cls: "calc-key-fn" },
  { label: "%", ins: "%", cls: "calc-key-fn" },
  { label: "÷", ins: "/", cls: "calc-key-op" },
  { label: "7", ins: "7" }, { label: "8", ins: "8" }, { label: "9", ins: "9" },
  { label: "×", ins: "*", cls: "calc-key-op" },
  { label: "4", ins: "4" }, { label: "5", ins: "5" }, { label: "6", ins: "6" },
  { label: "−", ins: "-", cls: "calc-key-op" },
  { label: "1", ins: "1" }, { label: "2", ins: "2" }, { label: "3", ins: "3" },
  { label: "+", ins: "+", cls: "calc-key-op" },
  { label: "0", ins: "0", wide: true }, { label: ".", ins: "." },
  { label: "=", act: "eq", cls: "calc-key-eq" },
];

const SCI_KEYS: KeyDef[] = [
  { label: "sin", ins: "sin(" }, { label: "cos", ins: "cos(" }, { label: "tan", ins: "tan(" },
  { label: "ln", ins: "ln(" }, { label: "log", ins: "log(" }, { label: "√", ins: "sqrt(" },
  { label: "π", ins: "pi" }, { label: "e", ins: "e" }, { label: "^", ins: "^" }, { label: "!", ins: "!" },
];

const PROG_OPS: { label: string; op: ProgOp }[] = [
  { label: "AND", op: "and" }, { label: "OR", op: "or" }, { label: "XOR", op: "xor" }, { label: "NOT", op: "not" },
  { label: "≪", op: "shl" }, { label: "≫", op: "shr" }, { label: "MOD", op: "mod" },
];

export function CalculatorApp(): React.ReactElement {
  const { lang } = useI18n();
  const tt = useCallback((key: string) => MODE_LABELS[key]?.[lang] ?? key, [lang]);
  const [mode, setMode] = useState<CalcMode>("std");
  const [expr, setExpr] = useState("");
  const [result, setResult] = useState("");
  const [angle, setAngle] = useState<CalcAngle>("deg");
  const [history, setHistory] = useState<HistoryItem[]>([]);

  // 程序员模式状态
  const [base, setBase] = useState<ProgBase>(10);
  const [acc, setAcc] = useState<bigint | null>(null);
  const [pendingOp, setPendingOp] = useState<ProgOp | null>(null);

  // 换算模式状态
  const [cat, setCat] = useState<UnitCategory>("length");
  const [fromU, setFromU] = useState("km");
  const [toU, setToU] = useState("m");
  const [convVal, setConvVal] = useState("1");

  const displayRef = useRef<HTMLInputElement>(null);

  const pushHistory = useCallback((e: string, r: string) => {
    setHistory((h) => [{ expr: e, result: r }, ...h].slice(0, 50));
  }, []);

  const doEquals = useCallback(() => {
    if (!expr.trim()) return;
    try {
      const v = calcEval(expr, angle);
      const r = fmtResult(v);
      setResult(r);
      pushHistory(expr, r);
      setExpr(r);
    } catch (err) {
      setResult(err instanceof Error ? err.message : "错误");
    }
  }, [expr, angle, pushHistory]);

  const press = useCallback(
    (k: KeyDef) => {
      if (k.act === "clear") {
        setExpr("");
        setResult("");
        return;
      }
      if (k.act === "eq") {
        doEquals();
        return;
      }
      if (k.ins !== undefined) setExpr((e) => e + k.ins);
    },
    [doEquals],
  );

  // 键盘支持（挂载在本窗口组件，VWM 聚焦时自然接收）
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (mode !== "std" && mode !== "sci") return;
      if (/^[0-9a-z+*/().!%^-]$/i.test(e.key)) {
        setExpr((s) => s + e.key);
        e.preventDefault();
      } else if (e.key === "Enter") {
        doEquals();
        e.preventDefault();
      } else if (e.key === "Backspace") {
        setExpr((s) => s.slice(0, -1));
        e.preventDefault();
      } else if (e.key === "Escape") {
        setExpr("");
        setResult("");
        e.preventDefault();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [mode, doEquals]);

  // 程序员模式：输入当前进制的数字串
  const [progInput, setProgInput] = useState("0");
  const progDigits = base === 10 ? "0123456789" : base === 16 ? "0123456789ABCDEF" : base === 8 ? "01234567" : "01";
  const applyProgOp = (op: ProgOp) => {
    try {
      const cur = progParse(progInput, base);
      if (op === "not") {
        setProgInput(progFormat(progEval(cur, "not")).dec);
        return;
      }
      if (pendingOp !== null && acc !== null) {
        const merged = progEval(acc, pendingOp, cur);
        setAcc(merged);
        setProgInput(progFormat(merged).dec);
      } else {
        setAcc(cur);
      }
      setPendingOp(op);
    } catch (err) {
      setProgInput(err instanceof Error ? err.message : "错误");
    }
  };
  const progEquals = () => {
    if (pendingOp === null || acc === null) return;
    try {
      const cur = progParse(progInput, base);
      const v = progEval(acc, pendingOp, cur);
      setProgInput(progFormat(v).dec);
      setPendingOp(null);
      setAcc(null);
    } catch (err) {
      setProgInput(err instanceof Error ? err.message : "错误");
    }
  };

  const convResult = useMemo(() => {
    try {
      const v = Number(convVal);
      if (Number.isNaN(v)) return "—";
      return fmtResult(unitConvert(v, cat, fromU, toU));
    } catch {
      return "—";
    }
  }, [convVal, cat, fromU, toU]);

  const switchCat = (c: UnitCategory) => {
    setCat(c);
    const us = UNIT_TABLE[c].units;
    setFromU(us[0]!.id);
    setToU(us[1]!.id);
  };

  return (
    <div className="calc-app" role="application" aria-label={tt("calcModeStd")}>
      <div className="calc-tabs" role="tablist">
        {MODES.map((m) => (
          <button
            key={m.id}
            role="tab"
            aria-selected={mode === m.id}
            className={`calc-tab${mode === m.id ? " active" : ""}`}
            onClick={() => setMode(m.id)}
          >
            {tt(m.key)}
          </button>
        ))}
      </div>

      {(mode === "std" || mode === "sci") && (
        <div className="calc-main">
          <div className="calc-left">
            <div className="calc-display">
              <input
                ref={displayRef}
                className="calc-expr text-input"
                value={expr}
                onChange={(e) => setExpr(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") doEquals();
                }}
                aria-label="表达式"
                placeholder="0"
              />
              <div className="calc-result" aria-live="polite">{result}</div>
            </div>
            {mode === "sci" && (
              <div className="calc-sci-row">
                <button
                  type="button"
                  className="btn ghost tiny"
                  onClick={() => setAngle((a) => (a === "deg" ? "rad" : "deg"))}
                  aria-label="角度制切换"
                >
                  {angle === "deg" ? "DEG" : "RAD"}
                </button>
                {SCI_KEYS.map((k) => (
                  <button key={k.label} type="button" className="calc-key calc-key-fn" onClick={() => press(k)}>
                    {k.label}
                  </button>
                ))}
              </div>
            )}
            <div className="calc-pad">
              {STD_KEYS.map((k, i) => (
                <button
                  key={`${k.label}-${i}`}
                  type="button"
                  className={`calc-key${k.cls ? ` ${k.cls}` : ""}${k.wide ? " calc-key-wide" : ""}`}
                  onClick={() => press(k)}
                >
                  {k.label}
                </button>
              ))}
              <button type="button" className="calc-key calc-key-fn" onClick={() => setExpr((e) => e.slice(0, -1))} aria-label="退格">
                <Delete size={14} />
              </button>
            </div>
          </div>
          <aside className="calc-history" aria-label="历史记录">
            <div className="calc-hist-head">
              <span className="small dim">{lang === "en" ? "History" : "历史记录"}</span>
              {history.length > 0 && (
                <button type="button" className="icon-btn tiny" onClick={() => setHistory([])} aria-label="清空历史">
                  ✕
                </button>
              )}
            </div>
            {history.length === 0 ? (
              <p className="dim small">{lang === "en" ? "No calculations yet" : "暂无计算记录"}</p>
            ) : (
              <ul className="calc-hist-list">
                {history.map((h, i) => (
                  <li key={i}>
                    <button
                      type="button"
                      className="calc-hist-item"
                      title={h.expr}
                      onClick={() => setExpr(h.result)}
                    >
                      <span className="dim small ellipsis">{h.expr}</span>
                      <span className="calc-hist-res">{h.result}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </aside>
        </div>
      )}

      {mode === "prog" && (
        <div className="calc-prog">
          <div className="row gap8" role="radiogroup" aria-label="进制">
            {([16, 10, 8, 2] as ProgBase[]).map((b) => (
              <button
                key={b}
                type="button"
                className={`btn ghost tiny${base === b ? " active" : ""}`}
                aria-pressed={base === b}
                onClick={() => {
                  // 切换进制：当前值换底
                  try {
                    const v = progParse(progInput, base);
                    setBase(b);
                    setProgInput(b === 10 ? progFormat(v).dec : v < 0n ? `-${(-v).toString(b)}` : v.toString(b));
                  } catch {
                    setBase(b);
                    setProgInput("0");
                  }
                }}
              >
                {b === 2 ? "BIN" : b === 8 ? "OCT" : b === 10 ? "DEC" : "HEX"}
              </button>
            ))}
          </div>
          <div className="calc-prog-bases small dim">
            {(() => {
              try {
                const f = progFormat(progParse(progInput, base));
                return (
                  <>
                    <span>HEX {f.hex}</span>
                    <span>DEC {f.dec}</span>
                    <span>OCT {f.oct}</span>
                    <span>BIN {f.bin}</span>
                  </>
                );
              } catch {
                return <span>—</span>;
              }
            })()}
          </div>
          <input
            className="calc-expr text-input"
            value={progInput}
            onChange={(e) => setProgInput(e.target.value)}
            aria-label="程序员输入"
          />
          <div className="calc-pad">
            {progDigits.split("").map((d) => (
              <button key={d} type="button" className="calc-key" onClick={() => setProgInput((s) => (s === "0" ? d : s + d))}>
                {d}
              </button>
            ))}
            {PROG_OPS.map((o) => (
              <button key={o.op} type="button" className="calc-key calc-key-op" onClick={() => applyProgOp(o.op)}>
                {o.label}
              </button>
            ))}
            <button type="button" className="calc-key calc-key-fn" onClick={() => setProgInput((s) => s.slice(0, -1) || "0")}>
              ⌫
            </button>
            <button type="button" className="calc-key calc-key-fn" onClick={() => setProgInput("0")}>
              C
            </button>
            <button type="button" className="calc-key calc-key-eq" onClick={progEquals}>
              =
            </button>
          </div>
          {pendingOp !== null && <p className="small dim">待运算：{PROG_OPS.find((o) => o.op === pendingOp)?.label}</p>}
        </div>
      )}

      {mode === "conv" && (
        <div className="calc-conv">
          <div className="row gap8" role="radiogroup" aria-label="类别">
            {(Object.keys(UNIT_TABLE) as UnitCategory[]).map((c) => (
              <button
                key={c}
                type="button"
                className={`btn ghost tiny${cat === c ? " active" : ""}`}
                aria-pressed={cat === c}
                onClick={() => switchCat(c)}
              >
                {UNIT_TABLE[c].label}
              </button>
            ))}
          </div>
          <div className="calc-conv-grid">
            <select className="text-input" value={fromU} onChange={(e) => setFromU(e.target.value)} aria-label="源单位">
              {UNIT_TABLE[cat].units.map((u) => (
                <option key={u.id} value={u.id}>{u.id}</option>
              ))}
            </select>
            <input className="text-input" value={convVal} onChange={(e) => setConvVal(e.target.value)} aria-label="源数值" />
            <select className="text-input" value={toU} onChange={(e) => setToU(e.target.value)} aria-label="目标单位">
              {UNIT_TABLE[cat].units.map((u) => (
                <option key={u.id} value={u.id}>{u.id}</option>
              ))}
            </select>
            <div className="calc-conv-result" aria-live="polite">{convResult}</div>
          </div>
        </div>
      )}
    </div>
  );
}
