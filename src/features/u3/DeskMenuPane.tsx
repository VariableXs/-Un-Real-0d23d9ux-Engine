/**
 * U3-v7 DeskMenuPane 活体件（AI-U3 · 批次七 · deskmenu 引擎演示面）。
 *
 * 一个 620px 缩比桌面样张：两个图标、右键菜单真弹、F2/单击名/慢双击
 * 三入口真进重命名、非法字符即时抖动、属性对话框真开（勾选即生效）。
 * 全部交互走 deskmenu 引擎纯函数，状态即时可见（三章 100ms 反馈红线）。
 *
 * 词典纪律（十章）：本件两个浮层（右键菜单/属性对话框）出路四条齐全——
 * dictwalk 账本 u3-desk-context / u3-props-dialog / u3-desk-rename 条目。
 */

import React, { useCallback, useRef, useState } from "react";
import { SectionCard } from "../settings/MouseJ1Panels";
import {
  iconContextMenu, propsGeneral, applyAttribute,
  renameIdle, renameEnter, renameSubmit, renameEscape, initialSelection, splitNameExt,
  fullTextReachable, type ItemFacts, type MenuItem, type RenameRt,
} from "./labapi";

/** 样张文件事实源（F264 与演示驱动同源）。 */
const DEMO_FILES: ReadonlyArray<ItemFacts> = [
  {
    name: "项目总结报告最终版.docx", kind: "file", openWith: "文字处理",
    location: "D:\\工作\\2026", sizeBytes: 5242880, createdAt: 1758864000000,
    modifiedAt: 1758950400000, accessedAt: 1758950400000,
    readOnly: false, hidden: false, protectedReason: null, contains: null,
  },
  {
    name: "系统快照.bak", kind: "file", openWith: null,
    location: "D:\\工作\\2026", sizeBytes: 734003200, createdAt: 1758777600000,
    modifiedAt: 1758777600000, accessedAt: 1758950400000,
    readOnly: true, hidden: false, protectedReason: "系统保护备份——只读由备份策略锁定", contains: null,
  },
];

type FloatKind = "none" | "menu" | "props" | "rename";

export function U3DeskMenuSection(): React.ReactElement {
  const [files, setFiles] = useState<ReadonlyArray<ItemFacts>>(DEMO_FILES);
  const [selected, setSelected] = useState<number | null>(null);
  const [float, setFloat] = useState<FloatKind>("none");
  const [menuAt, setMenuAt] = useState<{ x: number; y: number } | null>(null);
  const [rt, setRt] = useState<RenameRt>(renameIdle());
  const [err, setErr] = useState("");
  const [shake, setShake] = useState(false);
  const [msg, setMsg] = useState("右键图标弹菜单 · F2 或连点名字进重命名 · 属性真开");
  const menuRef = useRef<HTMLDivElement | null>(null);
  const nameAreaAt = useRef<{ at: number; idx: number } | null>(null);

  const target = selected !== null ? files[selected] : null;

  /** 三路全文可达实况（F502 判据的活体证据）。 */
  const reach = target
    ? fullTextReachable(target.name, {
        tooltip: () => target.name,                      // Tooltip 路：悬停全文
        rename: () => rt.phase === "editing" ? rt.draft : target.name, // 重命名路：编辑框全文
        properties: () => propsGeneral(target).location, // 属性路：位置字段全文
      })
    : null;

  const closeFloat = useCallback(() => {
    setFloat("none");
    setMenuAt(null);
  }, []);

  /** 名字区单击（判据：单击已选中项的名 → 进重命名；慢双击 700ms 窗同理）。 */
  const onNameClick = useCallback((idx: number) => {
    if (selected !== idx) { setSelected(idx); nameAreaAt.current = null; return; }
    const now = Date.now();
    const prev = nameAreaAt.current;
    const via = prev && prev.idx === idx && now - prev.at < 700 ? "slow-dbl" as const : "name-click" as const;
    nameAreaAt.current = { at: now, idx };
    setRt(renameEnter(renameIdle(), files[idx]!.name, via));
    setErr("");
    setFloat("rename");
  }, [selected, files]);

  /** F2 入口。 */
  const onF2 = useCallback(() => {
    if (selected === null) { setMsg("先点选一个图标——F2 是对选中项的"); return; }
    setRt(renameEnter(renameIdle(), files[selected]!.name, "f2"));
    setErr("");
    setFloat("rename");
  }, [selected, files]);

  /** 菜单动作分派。 */
  const runMenu = useCallback((m: MenuItem) => {
    if (m.disabled || selected === null) return;
    if (m.id === "rename") { closeFloat(); onF2(); return; }
    if (m.id === "properties") { setFloat("props"); return; }
    setMsg(`「${m.label}」触发（演示面记录动作）`);
    closeFloat();
  }, [selected, closeFloat, onF2]);

  /** 提交重命名（判据：成功完成即完成；失败行内红字不弹窗）。 */
  const submitRename = useCallback(() => {
    if (!target || rt.draft === null) return;
    const siblings = files.filter((_, i) => i !== selected).map((f) => f.name);
    const sub = renameSubmit(rt, siblings, !target.protectedReason);
    if (!sub.verdict.ok) {
      setErr(sub.verdict.inlineError);
      if (sub.verdict.shake) { setShake(true); window.setTimeout(() => setShake(false), 300); }
      return;
    }
    setFiles(files.map((f, i) => (i === selected ? { ...f, name: rt.draft } : f)));
    setRt(renameIdle());
    setFloat("none");
    setMsg(`重命名完成 → ${rt.draft}（无动画打扰）`);
  }, [rt, target, files, selected]);

  /** Esc 还原（判据：还原原名退出，不产生副作用）。 */
  const escRename = useCallback(() => {
    const esc = renameEscape(rt);
    setRt(esc);
    setFloat("none");
    setMsg("Esc 还原退出——原名保持");
  }, [rt]);

  const menu = target ? iconContextMenu(target, target.openWith ? [target.openWith, "浏览器"] : ["浏览器"]) : [];
  const sel = target ? initialSelection(target.name) : { start: 0, end: 0 };
  const ext = target ? splitNameExt(target.name).ext : "";

  return (
    <SectionCard title="右键菜单装配区" f="F502/F260/F264·v7">
      <div className="u3-lab-intro">
        桌面三路装配活体：F502 省略的全文三条路都可达（Tooltip/重命名/属性）、F260 行内重命名状态机（三入口/扩展名隔离/非法字符即时抖动/失败行内红字不弹窗）、F264 属性对话框（勾选即生效/受保护只读拒绝取消）。
      </div>

      {/* 缩比桌面样张 */}
      <div
        className="u3-menu-stage"
        role="img" aria-label="桌面右键菜单活体演示"
        onContextMenu={(e) => {
          e.preventDefault();
          if (selected === null) { setMsg("右键落在空白——先选中一个图标（空白菜单另册）"); return; }
          const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
          setMenuAt({ x: e.clientX - rect.left, y: e.clientY - rect.top });
          setFloat("menu");
        }}
        onClick={(e) => {
          // 点外部关闭（十章出路一）：点在浮层外收所有浮层
          if (!(e.target as HTMLElement).closest("[data-u3-float]")) { closeFloat(); }
        }}
        onKeyDown={(e) => {
          if (e.key === "Escape") { closeFloat(); escRename(); }
          if (e.key === "F2") { e.preventDefault(); onF2(); }
        }}
        tabIndex={0}
      >
        {files.map((f, i) => {
          const editing = float === "rename" && selected === i && rt.phase === "editing";
          return (
            <div key={f.name} className={`u3-menu-icon ${selected === i ? "is-selected" : ""}`}>
              <span className="u3-menu-glyph" aria-hidden>{f.kind === "file" ? "📄" : "📁"}</span>
              {editing ? (
                <input
                  data-u3-float="rename"
                  className={`u3-menu-rename ${shake ? "is-shake" : ""}`}
                  value={rt.draft}
                  autoFocus
                  onFocus={(e) => e.currentTarget.setSelectionRange(sel.start, sel.end)}
                  onChange={(e) => setRt({ ...rt, draft: e.target.value })}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") { e.preventDefault(); submitRename(); }
                    if (e.key === "Escape") { e.preventDefault(); escRename(); }
                  }}
                  aria-label="行内重命名"
                />
              ) : (
                <button
                  type="button"
                  className="u3-menu-name"
                  onClick={() => onNameClick(i)}
                  title={f.name /* F502 Tooltip 路全文 */}
                >{f.name}</button>
              )}
              {editing && err && <span className="u3-menu-err" role="alert">{err}</span>}
            </div>
          );
        })}

        {/* 右键菜单（出路四条：点外/Esc/再点/失焦） */}
        {float === "menu" && menuAt && (
          <div
            ref={menuRef}
            data-u3-float="menu"
            className="u3-menu-flyout"
            style={{ left: Math.min(menuAt.x, 430), top: Math.min(menuAt.y, 60) }}
            role="menu"
            onBlur={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node)) closeFloat(); }}
          >
            {menu.map((m) => m.id === "-" ? (
              <div key="sep" className="u3-menu-sep" />
            ) : (
              <button
                key={m.id}
                type="button"
                className="u3-menu-item"
                role="menuitem"
                disabled={m.disabled}
                onClick={() => runMenu(m)}
              >
                {m.label}
                {m.shortcut && <kbd className="u3-menu-kbd">{m.shortcut}</kbd>}
                {m.submenu && <span className="u3-menu-sub">▸ {m.submenu.map((s) => s.label).join("、")}</span>}
              </button>
            ))}
          </div>
        )}

        {/* 属性对话框（模态：出路四条齐全——u3-props-dialog 账本条目） */}
        {float === "props" && target && (
          <PropsDialog
            facts={target}
            onClose={() => { setFloat("none"); setMsg("属性关闭——焦点归触发钮"); }}
            onToggle={(key, v) => {
              const next = applyAttribute(target, key, v);
              const denied = next === target;
              setFiles(files.map((f, i) => (i === selected ? next : f)));
              setMsg(denied ? "受保护文件：取消只读被拒（安全提示区语义）" : `${key === "readOnly" ? "只读" : "隐藏"} → ${v}（勾选即生效）`);
            }}
          />
        )}
      </div>

      {/* 状态行 */}
      <div className="u3-row">
        <div>
          <div className="u3-name"><span className="fno">F502</span>三路全文可达实况</div>
          <div className="u3-desc">Tooltip / 重命名 / 属性三条路都要拿得回全文——缺一路即红</div>
        </div>
        <div className="u3-ctl" style={{ gridColumn: "2 / span 2" }}>
          <span className="u3-stat">选中 <b>{target?.name ?? "（无）"}</b></span>
          <span className="u3-stat">扩展名隔离 <b>{ext || "（无扩展名）"}</b></span>
          {reach && <span className={`u3-badge ${reach.ok ? "ok" : "warn"}`}>{reach.ok ? "✓ 三路全通" : `✗ 缺 ${reach.missing.join("/")}`}</span>}
        </div>
      </div>
      <div className="u3-row">
        <div>
          <div className="u3-name"><span className="fno">F260</span>三入口</div>
          <div className="u3-desc">F2 / 单击已选中项的名 / 慢双击名区（700ms 窗）——快双击是打开语义不进重命名</div>
        </div>
        <div className="u3-ctl" style={{ gridColumn: "2 / span 2" }}>
          <button type="button" className="j1x-btn" onClick={onF2}>F2 重命名</button>
          <span className="u3-stat" role="status">{msg}</span>
        </div>
      </div>
    </SectionCard>
  );
}

/** F264 属性对话框（常规页 + 勾选即生效 + 受保护安全提示区）。 */
function PropsDialog(props: {
  facts: ItemFacts;
  onClose: () => void;
  onToggle: (key: "readOnly" | "hidden", value: boolean) => void;
}): React.ReactElement {
  const g = propsGeneral(props.facts);
  return (
    <div
      data-u3-float="props"
      className="u3-props-mask"
      role="dialog" aria-label="属性对话框"
      onClick={(e) => { if (e.target === e.currentTarget) props.onClose(); }}
      onKeyDown={(e) => { if (e.key === "Escape") props.onClose(); }}
    >
      <div className="u3-props">
        <div className="u3-props-head">
          <span>{props.facts.name} 属性</span>
          <button type="button" className="j1x-btn" onClick={props.onClose} aria-label="关闭">✕</button>
        </div>
        <div className="u3-props-grid">
          <span className="u3-props-k">类型</span><span>{g.type}</span>
          <span className="u3-props-k">打开方式</span><span>{g.openWith}</span>
          <span className="u3-props-k">位置</span><span className="u3-props-path">{g.location}</span>
          <span className="u3-props-k">大小</span><span>{g.size}</span>
          <span className="u3-props-k">占用</span><span>{g.sizeOnDisk}</span>
          <span className="u3-props-k">创建时间</span><span>{new Date(g.times.created).toLocaleString()}</span>
          <span className="u3-props-k">修改时间</span><span>{new Date(g.times.modified).toLocaleString()}</span>
          <span className="u3-props-k">访问时间</span><span>{new Date(g.times.accessed).toLocaleString()}</span>
        </div>
        {g.safeNotice && <div className="u3-props-notice" role="note">🛡 {g.safeNotice}</div>}
        <div className="u3-props-attrs">
          <label>
            <input type="checkbox" checked={g.readOnly} onChange={(e) => props.onToggle("readOnly", e.target.checked)} />
            只读
          </label>
          <label>
            <input type="checkbox" checked={g.hidden} onChange={(e) => props.onToggle("hidden", e.target.checked)} />
            隐藏
          </label>
        </div>
      </div>
    </div>
  );
}
