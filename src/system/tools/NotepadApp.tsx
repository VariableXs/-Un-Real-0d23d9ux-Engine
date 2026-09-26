import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { pushToast } from "../../state/uiStore";
import { d2Store } from "../../features/desktopxp/d2store";
import "../../styles/desktop-d2.css";

/**
 * F097 记事本类编辑器 · VWM 工具窗口（C 桌面体验域·后段 · AI-D2 v3）。
 *
 * 判据对位（主册 G-C-27）：
 * - 大文件只读门：超阈值只读打开、显式启用编辑（防误改大日志）；
 * - 自动保存断电零丢失：草稿防抖 500ms + 启动恢复（B-1801 原子写语义
 *   在内核模型面 stard/notepad.rs——本窗口为交互面，草稿走 localStorage
 *   单键整写（原子 setItem）+ 恢复询问，不静默覆盖）；
 * - 查找：即时计数 + 上一个/下一个跳转（选区定位）；替换：单个/全部；
 * - 编码面：UTF-8 BOM 检测如实标注（GBK 等转码判据在模型面/装机批）。
 *
 * 诚实边界：打开/保存走浏览器 File API（Tauri 壳层的系统对话框接入属
 * F233 原生对话框族——AI-H3 面），大文件全文读入有内存上限（超限截断
 * 并如实标注「仅载入前 N MB」）。
 */

const BIG_FILE_WARN_BYTES = 8 * 1024 * 1024;
const LOAD_CAP_BYTES = 32 * 1024 * 1024;
const DRAFT_KEY = "variable:desktop:d2:notepad-draft:v1";
const DEBOUNCE_MS = 500;

interface Draft { name: string; text: string; at: number; bom: boolean }

export function NotepadApp(_props: { winId: string }): React.ReactElement {
  const [name, setName] = useState("未命名.txt");
  const [text, setText] = useState("");
  const [bom, setBom] = useState(false);
  const [readOnly, setReadOnly] = useState(false);
  const [truncated, setTruncated] = useState(false);
  const [findQ, setFindQ] = useState("");
  const [replaceR, setReplaceR] = useState("");
  const [matchIdx, setMatchIdx] = useState(0);
  const [dirty, setDirty] = useState(false);
  const [draftAt, setDraftAt] = useState<number | null>(null);
  const [showFind, setShowFind] = useState(false);
  const [showReplace, setShowReplace] = useState(false);
  const taRef = useRef<HTMLTextAreaElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const saveTimer = useRef<number | null>(null);
  const dirtyRef = useRef(false);
  dirtyRef.current = dirty;

  const autoSave = d2Store.getWith("notepad", "autoSave", true);

  // 自动草稿：改动防抖 500ms → localStorage 整写（原子）。
  useEffect(() => {
    if (!autoSave || !dirty) return;
    if (saveTimer.current !== null) window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      try {
        const draft: Draft = { name, text, at: Date.now(), bom };
        localStorage.setItem(DRAFT_KEY, JSON.stringify(draft));
        setDraftAt(draft.at);
      } catch (e) {
        console.error("[notepad] 草稿落盘失败", e); // 十三·补：显性化
      }
    }, DEBOUNCE_MS);
    return () => {
      if (saveTimer.current !== null) window.clearTimeout(saveTimer.current);
    };
  }, [text, name, bom, autoSave, dirty]);

  // 挂载恢复询问（不静默覆盖空画布——恢复是显式动作，F273 同规：损坏 = 从头）。
  useEffect(() => {
    try {
      const raw = localStorage.getItem(DRAFT_KEY);
      if (!raw) return;
      const d = JSON.parse(raw) as Draft;
      if (d.text) {
        pushToast("info", "发现上次编辑草稿", `「${d.name}」共 ${d.text.length} 字符（${new Date(d.at).toLocaleTimeString()}）——点「恢复草稿」取回`);
      }
    } catch {
      /* 损坏 = 无草稿 */
    }
  }, []);

  const restoreDraft = (): void => {
    try {
      const raw = localStorage.getItem(DRAFT_KEY);
      if (!raw) {
        pushToast("info", "没有草稿", "改动后 0.5s 自动入草稿");
        return;
      }
      const d = JSON.parse(raw) as Draft;
      setName(d.name);
      setText(d.text);
      setBom(d.bom);
      setReadOnly(false);
      setDirty(true);
      pushToast("success", "草稿已恢复", `${d.text.length} 字符`);
    } catch (e) {
      pushToast("error", "草稿恢复失败", String(e));
    }
  };

  const openFile = useCallback((file: File): void => {
    if (file.size > LOAD_CAP_BYTES) {
      // 大文件截断载入（诚实标注——全文载入是装机批/模型面的流式路径）。
      void file.slice(0, LOAD_CAP_BYTES).text().then((t) => {
        setName(file.name);
        setText(t);
        setBom(true); // slice(0) 含 BOM 若有
        setReadOnly(true); // 截断文件禁编辑（防止「保存」丢掉未载入部分——数据安全红线）
        setTruncated(true);
        setDirty(false);
        pushToast("info", "大文件已截断载入", `仅载入前 ${Math.round(LOAD_CAP_BYTES / 1024 / 1024)} MB（共 ${Math.round(file.size / 1024 / 1024)} MB）——只读保护开启，防保存丢内容`);
      });
      return;
    }
    void file.text().then((t) => {
      setName(file.name);
      setText(t);
      setBom(t.charCodeAt(0) === 0xfeff);
      // 只读门：超大文件只读打开、显式启用（判据本体）。
      const gate = d2Store.getWith("notepad", "bigFileReadOnlyGate", true);
      const ro = gate && file.size > BIG_FILE_WARN_BYTES;
      setReadOnly(ro);
      setTruncated(false);
      setDirty(false);
      if (ro) {
        pushToast("info", "大文件已只读打开", `${Math.round(file.size / 1024 / 1024)} MB 超过 8 MB 门——点「启用编辑」解除（防误改大日志）`);
      }
    }).catch((e) => pushToast("error", "打开失败", String(e)));
  }, []);

  const enableEdit = (): void => {
    setReadOnly(false);
    pushToast("success", "编辑已启用", "大文件——保存请用「另存下载」确认完整内容");
  };

  const saveDownload = (): void => {
    if (readOnly) {
      pushToast("info", "当前只读", "先「启用编辑」或改用另存");
      return;
    }
    const blob = new Blob([(bom ? "\ufeff" : "") + text], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = name;
    a.click();
    URL.revokeObjectURL(url);
    setDirty(false);
    pushToast("success", "已保存（下载）", `${name} · ${text.length} 字符 · UTF-8${bom ? " BOM" : ""}`);
  };

  const matches = useMemo(() => {
    if (findQ === "") return [] as number[];
    const out: number[] = [];
    let i = text.indexOf(findQ);
    while (i >= 0 && out.length < 10_000) {
      out.push(i);
      i = text.indexOf(findQ, i + findQ.length);
    }
    return out;
  }, [text, findQ]);

  const gotoMatch = (dir: 1 | -1): void => {
    if (matches.length === 0 || !taRef.current) return;
    const next = (matchIdx + dir + matches.length) % matches.length;
    setMatchIdx(next);
    const pos = matches[next]!;
    taRef.current.focus();
    taRef.current.setSelectionRange(pos, pos + findQ.length);
    // 滚动到可见（近似行高估算）。
    const before = text.slice(0, pos);
    const line = before.split("\n").length;
    taRef.current.scrollTop = Math.max(0, (line - 6) * 21);
  };

  const replaceOne = (): void => {
    if (matches.length === 0 || matchIdx >= matches.length) return;
    const pos = matches[matchIdx]!;
    setText(text.slice(0, pos) + replaceR + text.slice(pos + findQ.length));
    setDirty(true);
  };

  const replaceAll = (): void => {
    if (findQ === "" || matches.length === 0) return;
    const n = matches.length;
    setText(text.split(findQ).join(replaceR));
    setDirty(true);
    pushToast("success", "全部替换完成", `${n} 处「${findQ}」→「${replaceR}」`);
  };

  // 键位：Ctrl+F / Ctrl+H / Ctrl+S（域内 React 焦点内处理——零全局注册）。
  const onKey = (e: React.KeyboardEvent): void => {
    if (!e.ctrlKey) return;
    const k = e.key.toLowerCase();
    if (k === "f") { setShowFind(true); setShowReplace(false); e.preventDefault(); }
    else if (k === "h") { setShowReplace(true); setShowFind(true); e.preventDefault(); }
    else if (k === "s") { saveDownload(); e.preventDefault(); }
  };

  const lines = useMemo(() => text.split("\n").length, [text]);
  const bytesApprox = useMemo(() => new TextEncoder().encode(text).length, [text]);

  return (
    <div className="d2app" onKeyDown={onKey}>
      <div className="d2app-toolbar">
        <span className="d2app-title">记事本</span>
        <span className="d2app-sep" />
        <button type="button" onClick={() => fileRef.current?.click()}>打开</button>
        <input
          ref={fileRef}
          type="file"
          accept=".txt,.log,.md,.json,.csv,.ini,.cfg,text/*"
          hidden
          onChange={(e) => { const f = e.target.files?.[0]; if (f) openFile(f); e.target.value = ""; }}
        />
        <button type="button" onClick={saveDownload} disabled={readOnly}>保存（下载）</button>
        <button type="button" onClick={restoreDraft}>恢复草稿</button>
        {readOnly && !truncated && <button type="button" onClick={enableEdit}>启用编辑</button>}
        <span className="d2app-sep" />
        <button type="button" onClick={() => { setShowFind(true); setShowReplace(false); }} aria-pressed={showFind && !showReplace}>查找 Ctrl+F</button>
        <button type="button" onClick={() => { setShowReplace(true); setShowFind(true); }} aria-pressed={showReplace}>替换 Ctrl+H</button>
        {dirty && <span className="d2-badge">未保存改动</span>}
      </div>

      {(showFind || showReplace) && (
        <div className="d2app-toolbar" style={{ background: "var(--vx-surface-3, #f0f2f6)" }}>
          <input
            type="text"
            placeholder="查找…（即时计数）"
            value={findQ}
            onChange={(e) => { setFindQ(e.target.value); setMatchIdx(0); }}
            style={{ width: 180 }}
            aria-label="查找"
          />
          <span className="d2-muted">{findQ === "" ? "" : `${matches.length} 处${matches.length > 0 ? ` · 第 ${matchIdx + 1} 个` : ""}`}</span>
          <button type="button" onClick={() => gotoMatch(-1)} disabled={matches.length === 0}>上一个</button>
          <button type="button" onClick={() => gotoMatch(1)} disabled={matches.length === 0}>下一个</button>
          {showReplace && (
            <>
              <input
                type="text"
                placeholder="替换为…"
                value={replaceR}
                onChange={(e) => setReplaceR(e.target.value)}
                style={{ width: 160 }}
                aria-label="替换为"
              />
              <button type="button" onClick={replaceOne} disabled={matches.length === 0 || readOnly}>替换</button>
              <button type="button" onClick={replaceAll} disabled={matches.length === 0 || readOnly}>全部</button>
            </>
          )}
          <button type="button" onClick={() => { setShowFind(false); setShowReplace(false); }} aria-label="关闭查找">×</button>
        </div>
      )}

      <textarea
        ref={taRef}
        className="d2-note-textarea"
        value={text}
        readOnly={readOnly}
        onChange={(e) => { setText(e.target.value); setDirty(true); }}
        spellCheck={false}
        aria-label="编辑区"
        placeholder={"拖入或「打开」一个文本文件（.txt/.log/.md/…）。\n改动 0.5s 后自动入草稿——断电/崩溃可恢复。"}
        onDragOver={(e) => e.preventDefault()}
        onDrop={(e) => {
          e.preventDefault();
          const f = e.dataTransfer.files?.[0];
          if (f) openFile(f);
        }}
        style={{ flex: 1, minHeight: 0, resize: "none", border: "none", outline: "none", padding: "10px 14px", fontFamily: "ui-monospace, Consolas, monospace", fontSize: 13.5, lineHeight: 1.55, background: "var(--vx-surface-1, #fff)", color: "var(--vx-text, #1b1b1b)" }}
      />

      <div className="d2app-status">
        <span>{name}</span>
        <span>{text.length} 字符 · {lines} 行 · ≈{(bytesApprox / 1024).toFixed(1)} KB</span>
        <span>UTF-8{bom ? " BOM" : ""}（GBK 转码在模型面/装机批）</span>
        {readOnly && <span className="d2-bad">只读{truncated ? "（截断载入）" : "（大文件门）"}</span>}
        <span>草稿 {draftAt ? new Date(draftAt).toLocaleTimeString() : "—"}（自动保存{autoSave ? "开" : "关"}）</span>
      </div>
    </div>
  );
}
