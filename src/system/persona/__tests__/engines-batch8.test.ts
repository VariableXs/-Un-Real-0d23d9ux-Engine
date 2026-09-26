import { describe, expect, it } from "vitest";
import { MESSAGE_CATALOG, MESSAGE_PAGES, auditCopy, triadComplete, pageMessages } from "../message-catalog";
import { sealEnvelope, unsealEnvelope, planAtomicWrite, MigrationChain, quotaVerdict, cleanupCandidates } from "../persistence";
import { sceneTree, resolveTokens, renderFrame, dirtyRects, SCENE_SIZE } from "../preview-render";
import { defaultTokenTable } from "../tokens";

// ---------- message-catalog ----------

describe("message-catalog · 空态/错误态/确认态目录（十二查 13 深化）", () => {
  it("目录规模：20 页 × 双语，键集严格一致（漏翻=键集差异=检出）", () => {
    expect(MESSAGE_PAGES).toHaveLength(20);
    expect(Object.keys(MESSAGE_CATALOG.en).sort()).toEqual(Object.keys(MESSAGE_CATALOG.zh).sort());
  });

  it("每个槽位非空（空态不是空白——设计资源纪律）", () => {
    for (const lang of ["zh", "en"] as const) {
      for (const [page, m] of Object.entries(MESSAGE_CATALOG[lang])) {
        const fields = [m.empty.title, m.empty.guide, m.empty.action, m.error.what, m.error.why, m.error.next, m.confirm.what, m.confirm.consequence, m.confirm.confirm, m.confirm.cancel];
        for (const f of fields) expect(f.trim().length, `${lang}/${page}`).toBeGreaterThan(0);
      }
    }
  });

  it("三要素完整性运行期复验（类型门禁之外的双保险）", () => {
    const m = pageMessages("tokens", "zh")!;
    expect(triadComplete(m.error)).toBe(true);
    expect(triadComplete({ what: "a", why: "", next: "c" })).toBe(false);
  });

  it("全目录文案审计：禁词表/标点混用/空字段全扫", () => {
    const audit = auditCopy();
    expect(audit.checked).toBe(400); // 20 页 × 2 语言 × 10 槽位。
    expect(audit.issues).toEqual([]);
    expect(audit.clean).toBe(true);
  });

  it("未知页/未知语言显性返回 null（不静默给错词）", () => {
    expect(pageMessages("bogus", "zh")).toBeNull();
    expect(pageMessages("tokens", "fr" as never)).toBeNull();
  });
});

// ---------- persistence ----------

describe("persistence · 版本化存储模型（F161 深化）", () => {
  it("信封封拆：合法通过、payload 篡改被校验和检出", () => {
    const env = sealEnvelope("persona/tokens", 1, { accent: "#6e7fd4", list: [1, 2, 3] }, 1000);
    const ok = unsealEnvelope<{ accent: string }>(JSON.parse(JSON.stringify(env)));
    expect(ok.ok).toBe(true);
    const tampered = JSON.parse(JSON.stringify(env));
    tampered.payload.accent = "#ff0000";
    const bad = unsealEnvelope(tampered);
    expect(bad.ok).toBe(false);
    if (!bad.ok) expect(bad.reason).toContain("校验和不符");
  });

  it("stable stringify 键序无关：同内容不同键序 = 同校验和", () => {
    const a = sealEnvelope("s", 1, { a: 1, b: 2 }, 0).checksum;
    const b = sealEnvelope("s", 1, { b: 2, a: 1 }, 0).checksum;
    expect(a).toBe(b);
  });

  it("非信封/缺字段显性拒绝（零静默）", () => {
    expect(unsealEnvelope(null).ok).toBe(false);
    expect(unsealEnvelope("junk").ok).toBe(false);
    expect(unsealEnvelope({ slot: "s" }).ok).toBe(false);
  });

  it("原子写计划：四阶段 + 断电恢复表（永不半空的执行细则）", () => {
    const plan = planAtomicWrite("persona/icons", 1, { packId: "p" }, 0);
    expect(plan.phases.map((p) => p.phase)).toEqual(["write-tmp", "verify", "commit", "done"]);
    expect(plan.recovery.map((r) => r.crashedAt)).toEqual(["write-tmp", "verify", "commit", "done"]);
    expect(plan.recovery.find((r) => r.crashedAt === "commit")!.do).toBe("promote-tmp");
    expect(plan.recovery.find((r) => r.crashedAt === "verify")!.do).toBe("delete-tmp");
  });

  it("迁移链：逐版本小步（跳版注册拒绝）、两段迁移可回放、异常保留原始输入语义", () => {
    const chain = new MigrationChain<Record<string, unknown>>();
    chain.register(0, 1, (o) => ({ ...(o as Record<string, unknown>), v1: true }), "加 v1 标记");
    chain.register(1, 2, (o) => ({ ...(o as Record<string, unknown>), v2: true }), "加 v2 标记");
    expect(() => chain.register(0, 2, (o) => o as Record<string, unknown>, "跳版")).toThrow();
    const r = chain.migrate({ theme: "dark" } as Record<string, unknown>, 0, 2);
    expect(r.ok).toBe(true);
    if (r.ok) {
      expect(r.data.v1).toBe(true);
      expect(r.data.v2).toBe(true);
      expect(r.applied).toHaveLength(2);
    }
    const dead = chain.migrate({}, 2, 5);
    expect(dead.ok).toBe(false);
    if (!dead.ok) expect(dead.reason).toContain("无 2→3 迁移链");
  });

  it("配额与清理候选：超限判定、top3、钉选保护（可清≠自动清）", () => {
    const usages = [
      { slot: "a", bytes: 5000, updatedAt: 1000 },
      { slot: "b", bytes: 50000, updatedAt: 2000 },
      { slot: "c", bytes: 500, updatedAt: 3000 },
      { slot: "d", bytes: 20000, updatedAt: 9000 },
    ];
    const q = quotaVerdict(usages, 100_000);
    expect(q.within).toBe(true);
    expect(q.usedBytes).toBe(75_500);
    expect(q.top3.map((u) => u.slot)).toEqual(["b", "d", "a"]);
    const stale = cleanupCandidates(usages, 10_000, 5_000, new Set(["d"]));
    expect(stale.map((u) => u.slot).sort()).toEqual(["a", "b", "c"]); // d 钉选保护；a/b/c 均超 5s 龄。
  });
});

// ---------- preview-render ----------

describe("preview-render · 场景几何与脏区（F152 深化）", () => {
  it("三场景确定性：同场景两次渲染逐位一致、几何在界内", () => {
    for (const kind of ["desktop", "explorer", "settings"] as const) {
      const a = renderFrame(kind, null);
      const b = renderFrame(kind, null);
      expect(JSON.stringify(a)).toBe(JSON.stringify(b));
      for (const n of a) {
        expect(n.rect.x).toBeGreaterThanOrEqual(0);
        expect(n.rect.x + n.rect.w).toBeLessThanOrEqual(SCENE_SIZE.width);
      }
    }
  });

  it("场景内容差异：desktop 有图标、explorer 有侧栏、settings 有面板", () => {
    expect(sceneTree("desktop").some((n) => n.role === "icon")).toBe(true);
    expect(sceneTree("explorer").some((n) => n.id === "side")).toBe(true);
    expect(sceneTree("settings").some((n) => n.id === "row-btn")).toBe(true);
  });

  it("令牌解析：表值优先、缺键走出厂物理值（预览永不白屏）", () => {
    const table = defaultTokenTable();
    const s = resolveTokens(table, ["--p-accent"]);
    expect(s.background).toBe(table.colors["--p-accent"]);
    const fallback = resolveTokens(null, ["--p-bg-canvas"]);
    expect(fallback.background).toBe("#14141c");
    const missing = resolveTokens(null, ["--p-nonexistent"]);
    expect(missing.background).toBe("transparent");
  });

  it("脏区：改 accent 只有依赖 accent 的节点脏；还原图零脏区", () => {
    const base = defaultTokenTable();
    const changed = JSON.parse(JSON.stringify(base));
    changed.colors["--p-accent"] = "#ff8800";
    const prev = renderFrame("desktop", base);
    const d = dirtyRects(prev, renderFrame("desktop", changed));
    expect(d.dirty.length).toBeGreaterThan(0);
    expect(d.changedIds).toContain("icon-1"); // 图标吃 accent。
    expect(d.changedIds).not.toContain("win-title"); // 标题栏不吃 accent。
    // 还原：零脏区（放弃零残留的结构保证）。
    expect(dirtyRects(prev, renderFrame("desktop", base)).dirty).toHaveLength(0);
  });

  it("干净区占比 ∈ (0,1]，零变化 = 1", () => {
    const base = defaultTokenTable();
    const f = renderFrame("desktop", base);
    expect(dirtyRects(f, f).cleanRatio).toBe(1);
    const changed = JSON.parse(JSON.stringify(base));
    changed.colors["--p-accent"] = "#ff8800";
    const r = dirtyRects(f, renderFrame("desktop", changed));
    expect(r.cleanRatio).toBeGreaterThan(0);
    expect(r.cleanRatio).toBeLessThan(1);
  });
});
