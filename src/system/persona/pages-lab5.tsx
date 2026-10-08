/**
 * E 域页组⑩ · 深化实验室五（批次八收尾批）：布线/对拍/搜索导航面板。
 */
import { useMemo, useState } from "react";
import { Card, Row, PButton, Notice } from "./ui";
import { STATE_PAGES } from "./state-blocks";
import { STATE_WIRING, auditStateWiring, resolvePhase } from "./state-wiring";
import { crosscheckFormats, scanSources, SCAN_RULES } from "./crosscheck-guard";
import { searchIndex, groupHits, auditNavTree, CROSS_LINKS, type SearchEntry } from "./search-nav";
import { initialAutosave, markDirty, dueSave, completeSave, needsCloseConfirmation, draftSnapshot, AUTOSAVE_FORCE_MS } from "./crosscheck-guard";

const STATE_PAGES_ALL: readonly string[] = STATE_PAGES;

// ---------- 状态布线面板 ----------

export function StateWiringCard(): React.ReactNode {
  const audit = useMemo(() => auditStateWiring(), []);
  const [pageIdx, setPageIdx] = useState(0);
  const w = STATE_WIRING[pageIdx]!;
  const phase = resolvePhase(w, { isEmpty: pageIdx % 3 === 1, isError: pageIdx % 3 === 2 });
  return (
    <Card title="状态布线表（二十页状态块的布线真源）">
      <Row label={`布线 ${audit.wired} 页`} sub={audit.ok ? "机检全绿：每页有空态触发/错误触发/确认触发点（破坏性操作必有确认）" : audit.issues.join("；")}>
        <span style={{ color: audit.ok ? "var(--p-success)" : "var(--p-danger)" }}>{audit.ok ? "全覆盖" : "有缺口"}</span>
      </Row>
      <Row label={`样例页 ${w.page}`} sub={`空态条件: ${w.emptyWhen} · 错误条件: ${w.errorWhen}`}>
        <span style={{ display: "flex", gap: 4 }}>
          <PButton onClick={() => setPageIdx((pageIdx + 1) % STATE_WIRING.length)}>下一页</PButton>
        </span>
      </Row>
      <Row label={`当前解析相位: ${phase}`} sub="normal/empty/error 三相位——页面层按此渲染对应状态块（数据驱动不散写 if）">
        <span style={{ color: phase === "error" ? "var(--p-danger)" : phase === "empty" ? "var(--p-warn)" : "var(--p-success)" }}>{phase}</span>
      </Row>
      <Row label="确认触发点" sub={w.confirmTriggers.join(" · ")}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- 对拍与扫描面板 ----------

export function CrosscheckCard(): React.ReactNode {
  const demo = useMemo(() => {
    // format 对拍：真实引擎生产样本（verdict-report / usage-model / interaction-ledger 的导出）。
    const samples = [
      { producer: "verdict-report.ts", formatValue: "vx-verdict-evidence" },
      { producer: "usage-model.ts", formatValue: "vx-usage-ranking" },
      { producer: "interaction-ledger.ts", formatValue: "vx-interaction-ledger" },
      { producer: "asset-package.ts", formatValue: "vxtheme-assets" },
    ];
    const cc = crosscheckFormats(samples);
    // 词典扫描：喂一段含违例的样本源码（确定性正则扫描）。
    const scan = scanSources({
      "pages-demo.tsx": [
        "<button>确定</button>",
        "<span>暂无数据</span>",
        "<Notice>错误码: 0x80070005</Notice>",
        "<button>还原出厂配色</button>",
      ].join("\n"),
    });
    return { cc, scan, rules: SCAN_RULES.length };
  }, []);
  return (
    <Card title="交叉对拍与词典扫描（十/十四章机检）">
      <Row label={`format 对拍: 命中 ${demo.cc.matched.length} · 未登记 ${demo.cc.unregistered.length} · 未抽样 ${demo.cc.unsampled.length}`} sub={demo.cc.ok ? "生产格式的 format 字段全部在注册表（一处一事实对拍通过）" : demo.cc.unregistered.map((u) => u.formatValue).join(",")}>
        <span style={{ color: demo.cc.ok ? "var(--p-success)" : "var(--p-danger)" }}>{demo.cc.ok ? "对拍通过" : "有未登记格式"}</span>
      </Row>
      <Row label={`词典扫描: 规则 ${demo.rules} 条 · 样例违例 ${demo.scan.findings.length}`} sub={demo.scan.findings.map((f) => `${f.ruleId}@L${f.line}`).join(" · ") + "（确定钮/裸空态/裸错误码——字面量级确定性扫描）"}>
        <span style={{ color: demo.scan.findings.length > 0 ? "var(--p-warn)" : "var(--p-success)" }}>{demo.scan.findings.length} 处</span>
      </Row>
    </Card>
  );
}

// ---------- 搜索与导航面板 ----------

export function SearchNavCard(): React.ReactNode {
  const [q, setQ] = useState("");
  const INDEX: SearchEntry[] = useMemo(() => [
    { id: "tokens", page: "tokens", title: "令牌表", keywords: ["颜色", "配色", "color"], pinyinInitials: "lhb", shortcut: "无" },
    { id: "wallpaper", page: "wallpaper", title: "壁纸轮换", keywords: ["每日一换", "壁纸"], pinyinInitials: "bzl" },
    { id: "shortcuts", page: "shortcuts", title: "快捷键", keywords: ["键位", "重录"], pinyinInitials: "kjj" },
    { id: "archive", page: "archive", title: "档案导出", keywords: ["备份", "迁移"], pinyinInitials: "dad" },
    { id: "verdict", page: "verdict", title: "域总检", keywords: ["验收", "三步"], pinyinInitials: "yzj", shortcut: "Ctrl+Alt+V" },
  ], []);
  const hits = useMemo(() => searchIndex(INDEX, q, ["tokens"]), [INDEX, q]);
  const grouped = useMemo(() => groupHits(hits), [hits]);
  const nav = useMemo(() => auditNavTree(STATE_PAGES_ALL), []);
  return (
    <Card title="全域搜索与导航树（十一章可发现性）">
      <Row label="搜索（标题/关键词/拼音首字母 + 最近使用置顶）" sub={q ? hits.map((h) => `${h.entry.title}(${h.via}+${h.score})`).join(" · ") || "无命中" : "输入 'lhb'（令牌首字母）或 'color' 试探"}>
        <input value={q} onChange={(e) => setQ(e.target.value)} style={{ width: 140, fontSize: 11 }} aria-label="全域搜索演示" />
      </Row>
      {q && hits.length > 0 ? (
        <Row label="结果分组（按域结构化——不是平铺清单）" sub={[...grouped.keys()].map((p) => `${p}×${grouped.get(p)!.length}`).join(" · ")}>
          <span />
        </Row>
      ) : null}
      <Row label="导航树（二十页五组）" sub={nav.orphanPages.length === 0 ? `孤儿页 0——每页都有入口 · 面包屑 ${nav.breadcrumbs.size} 条` : `孤儿页: ${nav.orphanPages.join(",")}`}>
        <span style={{ color: nav.orphanPages.length === 0 ? "var(--p-success)" : "var(--p-danger)" }}>{nav.orphanPages.length === 0 ? "全覆盖" : "有孤儿"}</span>
      </Row>
      <Row label="页间跳转（用错带回正路——九章引导结构面）" sub={Object.entries(CROSS_LINKS).map(([k, v]) => `${k}→${v.join("/")}`).join(" · ")}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- 自动保存守护面板 ----------

export function AutosaveCard(): React.ReactNode {
  const demo = useMemo(() => {
    let s = initialAutosave();
    s = markDirty(s, 0);
    const debounce = dueSave(s, 3000);
    const beat = dueSave(s, 6000);
    const forced = dueSave(s, AUTOSAVE_FORCE_MS + 1);
    const saved = completeSave(s, true, 7000);
    const failed = completeSave(s, false, 7000);
    const draft = draftSnapshot(s, 1000);
    return {
      debounce: debounce.note,
      beat: beat.note,
      forced: forced.note,
      savedClose: needsCloseConfirmation(saved),
      failedClose: needsCloseConfirmation(failed),
      failed: failed.phase,
      draft: draft?.note ?? "无",
    };
  }, []);
  return (
    <Card title="自动保存守护（十二章防丢失）">
      <Row label="双节拍" sub={`3s: ${demo.debounce} · 6s: ${demo.beat} · 2min+: ${demo.forced}`}>
        <span />
      </Row>
      <Row label="关窗拦截（有未保存内容 = 提示，不静默丢）" sub={`已保存态拦截 ${demo.savedClose} · 失败态拦截 ${demo.failedClose}（${demo.failed}——三要素提示挂起）`}>
        <span />
      </Row>
      <Row label="崩溃草稿镜像" sub={demo.draft}>
        <span />
      </Row>
      <Notice tone="info">用户的劳动成果比代码神圣：脏超 2min 强制保存 + 关窗拦截 + 崩溃草稿——三层防丢失全部数据可验证。</Notice>
    </Card>
  );
}
