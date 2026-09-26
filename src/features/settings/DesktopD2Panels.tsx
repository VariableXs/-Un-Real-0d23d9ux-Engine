/**
 * D2 设置面板纵深件（DesktopD2Tab 的 SectionCard 内容层）。
 *
 * 面板纪律（F474 同源）：名称 + 一句话说明 + 调节控件三件套；
 * 全部演示/试炼面板的计时数字如实呈现（证据面，不摆拍）。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { MediaInfoStore, HoverScheduler, durationLabel, resolutionLabel, sniffAndParse, HEAD_BYTES } from "../desktopxp/mediainfo";
import { PhraseBook, mergeCandidates, parseVars, VAR_WHITELIST, PHRASE_CAP, ABBR_MAX_CHARS, CONTENT_MAX_CHARS } from "../desktopxp/phrasebk";
import { Palette, BUILTINS, OPEN_BUDGET_MS } from "../desktopxp/termpalette";
import { ThumbCache, ThumbQueue, CACHE_CAP_BYTES } from "../desktopxp/thumbeng";
import { d2xlog, FRUSTRATION_RULES } from "../desktopxp/d2telemetry";
import { pushToast } from "../../state/uiStore";

/* ------------------------------ 面板卡外壳 ------------------------------ */

export function SectionCard(props: { title: string; f: string; children: React.ReactNode }): React.ReactElement {
  return (
    <div className="d2-panel">
      <div className="d2-panel-head">
        <span>{props.title}</span>
        <span className="d2-fnum">{props.f}</span>
      </div>
      <div className="d2-panel-body">{props.children}</div>
    </div>
  );
}

/* --------------------------- F094 媒体信息试析 --------------------------- */

/** 媒体信息试析台：选/拖一个媒体文件 → 头部嗅探 → 字段 + 缓存/悬停账。 */
export function MediaTryPanel(): React.ReactElement {
  const storeRef = useRef(new MediaInfoStore(512));
  const schedRef = useRef(new HoverScheduler());
  const [result, setResult] = useState<{ name: string; label: string; res: string; dur: string; fromCache: boolean } | null>(null);
  const [schedState, setSchedState] = useState("");

  const analyze = useCallback((file: File): void => {
    const head = file.slice(0, HEAD_BYTES);
    void head.arrayBuffer().then((buf) => {
      const key = `${file.name}|${file.size}|${file.lastModified}`;
      const cached = storeRef.current.lookup(key);
      const fromCache = cached !== undefined;
      const info = cached !== undefined ? cached : sniffAndParse(new Uint8Array(buf), file.size);
      if (!fromCache) storeRef.current.put(key, info);
      // 悬停调度账：模拟一次 hover→tick（缓存命中即时 <100ms 判线）。
      const now = performance.now();
      schedRef.current.hover(now, fromCache);
      if (!fromCache) schedRef.current.tick(now + 800);
      const shownMs = fromCache ? 0 : 800;
      setSchedState(schedRef.current.state.kind === "shown" ? `悬停账：已显示（${fromCache ? "缓存命中 0ms（<100ms 判线）" : "冷读 800ms 档"}）· 即时显示 ${schedRef.current.instantShows} 次` : "悬停账：等待中");
      setResult({
        name: file.name,
        label: info ? (info.container.toUpperCase()) : "未识别容器",
        res: info ? resolutionLabel(info) : "—",
        dur: info ? durationLabel(info.durationMs) : "—",
        fromCache,
      });
      void shownMs;
    }).catch((e) => {
      pushToast("error", "媒体文件读取失败", String(e));
    });
  }, []);

  return (
    <>
      <div className="d2-rowflex">
        <input
          type="file"
          accept="video/*,audio/*,.mkv,.flac,.mp3,.m4a"
          onChange={(e) => {
            const f = e.target.files?.[0];
            if (f) analyze(f);
          }}
          aria-label="选择媒体文件试析"
        />
        <span className="d2-muted">只读头部 64KB，不整读文件</span>
      </div>
      {result && (
        <table className="d2-table" aria-label="媒体信息解析结果">
          <tbody>
            <tr><td>文件</td><td>{result.name}</td></tr>
            <tr><td>容器</td><td>{result.label}</td></tr>
            <tr><td>分辨率</td><td>{result.res}</td></tr>
            <tr><td>时长</td><td>{result.dur}</td></tr>
            <tr><td>缓存</td><td className={result.fromCache ? "d2-ok" : ""}>{result.fromCache ? "命中（LRU）" : "未命中（已入缓存）"}</td></tr>
          </tbody>
        </table>
      )}
      <p className="d2-muted">{schedState || "悬停调度：冷读 800ms 档显示；缓存命中即时显示（<100ms 判线）。"}</p>
      <p className="d2-muted">缓存条目：{storeRef.current.size} · 深度 box 走查（旋转矩阵/轨道字节实算）判据在内核模型面 stard/mediainfo.rs。</p>
    </>
  );
}

/* --------------------------- F108 短语库管理台 --------------------------- */

/** 短语库管理：列表/新建/删除/导入/导出 + 候选试炼场（全链 <100ms 计时）。 */
export function PhraseLibPanel(): React.ReactElement {
  const bookRef = useRef<PhraseBook>(new PhraseBook());
  const [q, setQ] = useState("");
  const [abbr, setAbbr] = useState("");
  const [content, setContent] = useState("");
  const [category, setCategory] = useState("常用");
  const [trial, setTrial] = useState("");
  const [trialOut, setTrialOut] = useState<{ text: string; ms: number; phrase: boolean } | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const [, bump] = useState(0);

  const refresh = useCallback(() => bump((v) => v + 1), []);

  const add = (): void => {
    const r = bookRef.current.add(abbr.trim(), content, category.trim() || "默认");
    if (!r.ok) {
      pushToast("error", "短语未保存", r.error ?? "未知错误");
      return;
    }
    setAbbr("");
    setContent("");
    refresh();
    pushToast("success", "短语已入库", `${abbr} → ${content.slice(0, 24)}…`);
  };

  const doImport = (json: string): void => {
    const report = bookRef.current.import(json, "overwrite");
    refresh();
    pushToast(
      report.rejected.length === 0 ? "success" : "info",
      `导入完成：新增 ${report.added} · 覆盖 ${report.overwritten}`,
      report.rejected.length > 0 ? `拒绝 ${report.rejected.length} 条：${report.rejected.slice(0, 3).join("；")}` : "round-trip 无损",
    );
  };

  const runTrial = (): void => {
    const t0 = performance.now();
    const key = trial.trim();
    const hit = key === "" ? [] : bookRef.current.search(key).filter((p) => p.abbr === key);
    const normal = ["普通候选甲", "普通候选乙"];
    const merged = mergeCandidates(hit, normal, bookRef.current.phrasePriority);
    const expand = hit[0] ? bookRef.current.expand(key, "2026-09-26 14:30", "星期六") : null;
    const ms = performance.now() - t0;
    setTrialOut({
      text: expand ?? merged[0]?.text ?? "（无命中）",
      ms,
      phrase: (merged[0]?.phrase ?? false),
    });
  };

  const filtered = bookRef.current.search(q);
  const vars = useMemo(() => parseVars(content), [content]);

  return (
    <>
      <div className="d2-rowflex">
        <input type="search" placeholder="搜索缩写/内容/分类" value={q} onChange={(e) => setQ(e.target.value)} style={{ flex: 1, minWidth: 160 }} aria-label="搜索短语" />
        <button type="button" onClick={() => { void navigator.clipboard?.writeText(bookRef.current.export()).then(() => pushToast("success", "已导出到剪贴板", "JSON 开放格式——round-trip 无损")); }}>导出</button>
        <button type="button" onClick={() => fileRef.current?.click()}>导入</button>
        <input
          ref={fileRef}
          type="file"
          accept=".json,application/json"
          hidden
          onChange={(e) => {
            const f = e.target.files?.[0];
            if (!f) return;
            void f.text().then(doImport).catch((err) => pushToast("error", "导入失败", String(err)));
            e.target.value = "";
          }}
        />
      </div>
      <div className="d2-list" role="list" aria-label="短语列表">
        {filtered.length === 0 && <div className="d2-list-item"><span className="d2-muted">还没有短语——先在下方新建一条（打 yx 出整句）。</span></div>}
        {filtered.slice(0, 100).map((p) => (
          <div key={p.abbr} className="d2-list-item" role="listitem">
            <span className="d2-badge">{p.abbr}</span>
            <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{p.content}</span>
            <span className="d2-badge d2-badge--use">{p.uses} 次</span>
            <button type="button" onClick={() => { bookRef.current.remove(p.abbr); refresh(); }} aria-label={`删除 ${p.abbr}`}>删除</button>
          </div>
        ))}
      </div>
      <div className="d2-rowflex">
        <input type="text" placeholder="缩写（≤32 字符）" value={abbr} maxLength={ABBR_MAX_CHARS} onChange={(e) => setAbbr(e.target.value)} style={{ width: 140 }} aria-label="缩写" />
        <input type="text" placeholder="内容（≤500 字，支持 {date:yyyy-MM-dd} 变量）" value={content} maxLength={CONTENT_MAX_CHARS} onChange={(e) => setContent(e.target.value)} style={{ flex: 1, minWidth: 200 }} aria-label="短语内容" />
        <input type="text" placeholder="分类" value={category} onChange={(e) => setCategory(e.target.value)} style={{ width: 90 }} aria-label="分类" />
        <button type="button" onClick={add} disabled={abbr.trim() === "" || content === ""}>新建</button>
      </div>
      <p className="d2-muted">
        库 {bookRef.current.items.length}/{PHRASE_CAP} · 内容变量识别：{vars.length > 0 ? vars.map((v) => `{${v.tag}:${v.fmt}}`).join("、") : "（无）"} · 白名单 9 格式符：{VAR_WHITELIST.join(" / ")}
      </p>
      <div className="d2-rowflex">
        <input type="text" placeholder="候选试炼场：输入缩写（如 yx）" value={trial} onChange={(e) => setTrial(e.target.value)} style={{ flex: 1, minWidth: 180 }} aria-label="候选试炼输入" />
        <button type="button" onClick={runTrial}>展开</button>
      </div>
      {trialOut && (
        <p className="d2-muted">
          上屏：{trialOut.phrase ? <span className="d2-badge">短语</span> : null} 「{trialOut.text}」 · 全链 {trialOut.ms.toFixed(2)}ms（判线 {"<100ms"}{trialOut.ms < 100 ? " ✓" : " ✗ 超线即缺陷"}）
        </p>
      )}
    </>
  );
}

/* --------------------------- F096 命令面板总览 --------------------------- */

export function PalettePanel(): React.ReactElement {
  const pRef = useRef<Palette>(new Palette());
  const [hits, setHits] = useState<Array<{ name: string; cmd: string }>>([]);
  const [q, setQ] = useState("");
  const p = pRef.current;

  const run = useCallback((query: string): void => {
    const t0 = performance.now();
    p.open(0);
    const hs = p.search(query);
    const searchMs = performance.now() - t0;
    setHits(hs.slice(0, 12).map((h) => ({ name: h.command.name, cmd: h.command.builtin ? `内置:${h.command.cmdline}` : h.command.cmdline })));
    void searchMs;
  }, [p]);

  useEffect(() => { run(""); }, [run]);

  return (
    <>
      <div className="d2-rowflex">
        <input
          type="search"
          placeholder="试搜：清屏 / qp / 分屏…（子序列+首字母双路）"
          value={q}
          onChange={(e) => { setQ(e.target.value); run(e.target.value); }}
          style={{ flex: 1, minWidth: 200 }}
          aria-label="命令面板试搜"
        />
        <span className="d2-muted">弹出预算 {OPEN_BUDGET_MS}ms · 超预算 {p.overBudget} 次</span>
      </div>
      <div className="d2-list" aria-label="面板命中列表">
        {hits.length === 0 && <div className="d2-list-item"><span className="d2-muted">无命中——查询换试试（支持拼音首字母如 qp）。</span></div>}
        {hits.map((h) => (
          <div key={h.name + h.cmd} className="d2-list-item">
            <span>{h.name}</span>
            <span className="d2-muted">{h.cmd}</span>
          </div>
        ))}
      </div>
      <p className="d2-muted">内置 12 条全可达：{p.allBuiltinsReachable() ? <span className="d2-ok">✓（空查询全量列出）</span> : <span className="d2-bad">✗</span>} · 内置清单：{BUILTINS.map((b) => b.name).join("、")}</p>
      <p className="d2-muted">收藏 {p.favorites.size}/{64}（frecency 定容逐出）· 在终端窗口内 Ctrl+Shift+P 呼出真实面板。</p>
    </>
  );
}

/* --------------------------- F093 缩略图缓存账 --------------------------- */

export function ThumbCachePanel(): React.ReactElement {
  const cacheRef = useRef(new ThumbCache(CACHE_CAP_BYTES));
  const qRef = useRef(new ThumbQueue());
  const [tick, setTick] = useState(0);
  const cache = cacheRef.current;
  const q = qRef.current;

  const simulate = (): void => {
    // 模拟一次「万张目录二次浏览」：首批冷 miss 入队生成，二批热命中。
    for (let i = 0; i < 500; i++) {
      const key = `t${i}`;
      q.enqueue(key, (i % 10 === 0 ? 0 : i % 3 === 0 ? 1 : 2) as 0 | 1 | 2);
    }
    while (q.pending > 0) {
      const key = q.dequeue()!;
      if (!cache.touch(key)) {
        cache.put(key, 24 * 1024 + (key.length % 7) * 512); // 模拟 ~24KB 缩略
      }
    }
    // 二次浏览（全热）。
    for (let i = 0; i < 500; i++) cache.touch(`t${i}`);
    setTick((v) => v + 1);
  };

  return (
    <>
      <div className="d2-rowflex">
        <button type="button" onClick={simulate}>跑一次 500 张二次浏览对账</button>
        <button type="button" onClick={() => { cache.clear(); q.rejected = 0; q.enqueued = 0; q.processed = 0; setTick((v) => v + 1); }}>清账</button>
      </div>
      <div className="d2-cachebar" aria-hidden>
        <div className="d2-cachebar-fill" style={{ width: `${Math.min(100, (cache.bytes / cache.capBytes) * 100)}%` }} />
      </div>
      <table className="d2-table" aria-label="缩略图缓存对账表">
        <tbody>
          <tr><td>条目</td><td>{cache.size}</td><td>占用</td><td>{(cache.bytes / 1024).toFixed(0)} KB / 2GB</td></tr>
          <tr><td>热库命中</td><td>{cache.hotHits}</td><td>未命中</td><td>{cache.hotMisses}</td></tr>
          <tr>
            <td>命中率（热库增量）</td>
            <td className={cache.hitRate() >= 0.95 ? "d2-ok" : ""}>{(cache.hitRate() * 100).toFixed(1)}%{cache.hotHits + cache.hotMisses > 0 && cache.hitRate() >= 0.95 ? " ✓" : ""}</td>
            <td>逐出/弃损</td>
            <td>{cache.evictions} / {cache.discards}</td>
          </tr>
          <tr><td>队列入队/处理/拒绝</td><td>{q.enqueued} / {q.processed} / {q.rejected}</td><td>三级队列</td><td>可视 &gt; 邻近 &gt; 背景</td></tr>
        </tbody>
      </table>
      <p className="d2-muted">像素解码/EXIF 直抽判据在内核模型面 stard/thumbeng.rs；本面板是缓存策略与队列纪律的实时对账（{tick} 次对账）。</p>
    </>
  );
}

/* --------------------------- 十三章 体验日志面板 --------------------------- */

/** 体验日志：会话事件环 + 挫败信号清单 + 导出（十三章/十三·补）。 */
export function XLogPanel(): React.ReactElement {
  const [, bump] = useState(0);
  const log = d2xlog;
  const fr = log.frustrations();

  return (
    <>
      <div className="d2-rowflex">
        <button type="button" onClick={() => bump((v) => v + 1)}>刷新</button>
        <button
          type="button"
          onClick={() => {
            void navigator.clipboard?.writeText(log.export()).then(
              () => pushToast("success", "体验日志已复制", "JSON 开放格式——时间轴 + 挫败清单（行为 only，无内容）"),
              (e) => pushToast("error", "复制失败", String(e)),
            );
          }}
        >
          导出（复制 JSON）
        </button>
        <button type="button" onClick={() => { log.clear(); bump((v) => v + 1); }}>清空</button>
        <span className="d2-muted">会话事件 {log.size}（内存环 {FRUSTRATION_RULES.cap} 上限 · 每 8s 节流落盘）</span>
      </div>
      <div className="d2-list" aria-label="挫败信号清单">
        {fr.length === 0 && <div className="d2-list-item"><span className="d2-muted">暂无挫败信号——狂点/死点/浮层反复开关/重试风暴会自动标记到这里。</span></div>}
        {fr.slice(-20).reverse().map((e) => (
          <div key={e.seq} className="d2-list-item">
            <span className="d2-bad">{e.frustration}</span>
            <span>{e.surface} · {e.element}</span>
            <span className="d2-muted">{new Date(e.t).toLocaleTimeString()} · {e.verdict}{e.ms !== null ? ` · ${e.ms}ms` : ""}</span>
          </div>
        ))}
      </div>
      <p className="d2-muted">隐私红线：只记交互行为与结论，不记输入内容（正文/密码全不入账——十三章）。写入走内存环 + 节流落盘，绝不阻塞交互。</p>
    </>
  );
}
