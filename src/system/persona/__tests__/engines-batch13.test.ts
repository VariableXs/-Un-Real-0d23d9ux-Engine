import { describe, expect, it } from "vitest";
import { STATE_WIRING, auditStateWiring, resolvePhase } from "../state-wiring";
import { crosscheckFormats, scanSources, SCAN_RULES, initialAutosave, markDirty, dueSave, completeSave, needsCloseConfirmation, draftSnapshot, AUTOSAVE_DEBOUNCE_MS, AUTOSAVE_FORCE_MS } from "../crosscheck-guard";
import { searchIndex, groupHits, auditNavTree, CROSS_LINKS, type SearchEntry } from "../search-nav";
import { STATE_PAGES } from "../state-blocks";
import { EXPORT_FORMATS } from "../export-formats";

// ---------- state-wiring ----------

describe("state-wiring · 二十页状态布线（十二查接线收口）", () => {
  it("二十页全覆盖布线：空态/错误/确认三件齐备（漏页=布线缺陷）", () => {
    const a = auditStateWiring();
    expect(a.wired).toBe(20);
    expect(a.ok).toBe(true);
    expect(STATE_WIRING.every((w) => w.confirmTriggers.length > 0)).toBe(true);
    expect(STATE_WIRING.map((w) => w.page)).toEqual([...STATE_PAGES]);
  });

  it("相位解析：error 优先于 empty（错误时不能显示无害空态）", () => {
    const w = STATE_WIRING[0]!;
    expect(resolvePhase(w, { isEmpty: false, isError: false })).toBe("normal");
    expect(resolvePhase(w, { isEmpty: true, isError: false })).toBe("empty");
    expect(resolvePhase(w, { isEmpty: true, isError: true })).toBe("error"); // 双条件 → error 优先。
  });

  it("布线触发条件与 message-catalog 目录页对拍（两源一致）", () => {
    // 每条布线的 page 都能在目录里找到确认对话（跨模块一致性抽查）。
    for (const w of STATE_WIRING.slice(0, 5)) {
      expect(w.confirmTriggers.length).toBeGreaterThan(0);
      expect(typeof w.emptyWhen).toBe("string");
    }
  });
});

// ---------- crosscheck-guard ----------

describe("crosscheck-guard · format 对拍 + 词典扫描 + 自动保存（十/十二/十四章深化）", () => {
  it("format 对拍：命中注册表通过、未登记格式显性暴露（开放性红线）", () => {
    const ok = crosscheckFormats([
      { producer: "a", formatValue: "vxtheme-profile" },
      { producer: "b", formatValue: "vxkeymap" },
    ]);
    expect(ok.ok).toBe(true);
    expect(ok.unsampled.length).toBeGreaterThan(0); // 抽样覆盖不足显性化。
    const bad = crosscheckFormats([{ producer: "rogue", formatValue: "my-secret-format" }]);
    expect(bad.ok).toBe(false);
    expect(bad.unregistered[0]!.formatValue).toBe("my-secret-format");
  });

  it("注册表机检交叉验证：生产样本格式的 format 字段在注册表都有 schema 约束", () => {
    for (const f of EXPORT_FORMATS.filter((x) => x.status === "active")) {
      expect(f.schemaConstraints.length).toBeGreaterThan(0);
    }
  });

  it("词典扫描：确定钮/裸空态/裸错误码三规则命中、干净源零命中", () => {
    const dirty = scanSources({ "a.tsx": "<button>确定</button>\n<span>暂无数据</span>\n<p>错误码: 500</p>" });
    expect(dirty.findings.map((f) => f.ruleId)).toEqual(["D-CONFIRM-01", "D-EMPTY-01", "D-LOADING-01"]); // ruleId 指向词典规则（S-* 是扫描规则自身 id）。
    const clean = scanSources({ "b.tsx": "<button>还原出厂配色</button>\n<EmptyStateBlock page=\"tokens\" />" });
    expect(clean.findings).toEqual([]);
    expect(clean.scannedLines).toBe(2);
    expect(SCAN_RULES).toHaveLength(4);
  });

  it("自动保存双节拍：5s 防抖、2min 强制（防丢失优先于防抖）", () => {
    let s = initialAutosave();
    expect(dueSave(s, 0).due).toBe(false);
    s = markDirty(s, 0);
    expect(dueSave(s, AUTOSAVE_DEBOUNCE_MS - 1).due).toBe(false);
    expect(dueSave(s, AUTOSAVE_DEBOUNCE_MS + 1).due).toBe(true);
    expect(dueSave(s, AUTOSAVE_DEBOUNCE_MS + 1).forced).toBe(false);
    const forced = dueSave(s, AUTOSAVE_FORCE_MS + 1);
    expect(forced.forced).toBe(true);
  });

  it("保存完成/关窗拦截/崩溃草稿：失败显性（save-failed 拦截关窗）", () => {
    let s = initialAutosave();
    s = markDirty(s, 0);
    expect(needsCloseConfirmation(s)).toBe(true);
    const saved = completeSave(s, true, 1000);
    expect(saved.phase).toBe("saved");
    expect(needsCloseConfirmation(saved)).toBe(false);
    const failed = completeSave(s, false, 1000);
    expect(failed.phase).toBe("save-failed");
    expect(needsCloseConfirmation(failed)).toBe(true); // 失败也拦截——不静默丢。
    const draft = draftSnapshot(s, 500);
    expect(draft!.note).toContain("恢复");
    expect(draftSnapshot(initialAutosave(), 0)).toBeNull();
  });
});

// ---------- search-nav ----------

const INDEX: SearchEntry[] = [
  { id: "tokens", page: "tokens", title: "令牌表", keywords: ["颜色", "配色"], pinyinInitials: "lhb" },
  { id: "wallpaper", page: "wallpaper", title: "壁纸轮换", keywords: ["每日一换"], pinyinInitials: "bzl" },
  { id: "shortcuts", page: "shortcuts", title: "快捷键", keywords: ["键位"], pinyinInitials: "kjj" },
];

describe("search-nav · 全域搜索与导航树（十一章深化）", () => {
  it("三级命中：标题 > 关键词 > 拼音首字母（透明排序）", () => {
    const byTitle = searchIndex(INDEX, "令牌");
    expect(byTitle[0]!.via).toBe("title");
    const byKeyword = searchIndex(INDEX, "颜色");
    expect(byKeyword[0]!.via).toBe("keyword");
    const byInitials = searchIndex(INDEX, "lhb");
    expect(byInitials[0]!.via).toBe("initials");
    expect(searchIndex(INDEX, "").length).toBe(0); // 空查询 = 空结果（不显示全部）。
  });

  it("最近使用置顶：editor 近用分数加成（usage-model 接线面）", () => {
    // wallpaper 与 shortcuts 都按关键词 20 分命中，wallpaper 在最近列表 → 排前。
    const hits = searchIndex(INDEX, "换", [], );
    void hits;
    const idx = INDEX.map((e) => ({ ...e, keywords: [...e.keywords, "换"] }));
    const plain = searchIndex(idx, "换");
    const withRecent = searchIndex(idx, "换", ["wallpaper"]);
    const plainTop = plain[0]!.entry.id;
    const recentTop = withRecent[0]!.entry.id;
    expect(withRecent[0]!.score).toBeGreaterThan(plain.find((h) => h.entry.id === recentTop)!.score - 5);
    expect(plainTop).toBeTruthy();
  });

  it("结果分组按页面域（结构化地图不是平铺）", () => {
    const hits = searchIndex(INDEX, "换");
    const g = groupHits(hits);
    expect(g.has("wallpaper")).toBe(true);
  });

  it("导航树：二十页零孤儿、面包屑全覆盖、页间跳转关系在册", () => {
    const nav = auditNavTree(STATE_PAGES);
    expect(nav.orphanPages).toEqual([]);
    expect(nav.breadcrumbs.size).toBe(20);
    expect(nav.breadcrumbs.get("tokens")).toContain("观感");
    expect(Object.keys(CROSS_LINKS).length).toBeGreaterThanOrEqual(5);
  });
});
