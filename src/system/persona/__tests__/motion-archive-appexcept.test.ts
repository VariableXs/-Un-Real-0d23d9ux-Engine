import { beforeEach, describe, expect, it } from "vitest";
import {
  loadMotionTier, saveMotionTier, scaledDuration, motionPlan, TIER_SCALE,
  auditGlobalConsistency, DEMO_SCENES, LOOP_PAUSE_MS,
} from "../motiontier";
import {
  ARCHIVE_SECTIONS, exportArchive, validateArchive, diffPreview, importArchive,
  ARCHIVE_FORMAT, ARCHIVE_VERSION,
} from "../archive";
import { defaultTokenTable, saveTokenTable, tokenTableHash, loadTokenTable } from "../tokens";
import {
  loadAppExceptions, addException, pruneUninstalled,
  skipOnGlobalBroadcast, lockedModeFor, degradationLabel, MAX_EXCEPTIONS,
} from "../appexcept";
import { personaStore } from "../store";

beforeEach(() => {
  personaStore.reset();
});

describe("F160 动效强度预演", () => {
  it("三档缩放 100%/60%/0%（时长实测口径）", () => {
    expect(TIER_SCALE).toEqual({ full: 1, reduced: 0.6, off: 0 });
    expect(scaledDuration(200, "full")).toBe(200);
    expect(scaledDuration(200, "reduced")).toBe(120);
    expect(scaledDuration(200, "off")).toBe(0);
  });

  it("减弱档轨迹简化：位移减半+淡入替代滑动；关闭档瞬显", () => {
    const r = motionPlan(320, 48, "reduced");
    expect(r.durationMs).toBe(192);
    expect(r.translatePx).toBe(24);
    expect(r.fadeOnly).toBe(true);
    const off = motionPlan(320, 48, "off");
    expect(off.durationMs).toBe(0);
    expect(off.opacityFrom).toBe(1);
  });

  it("全局抽查一致性：缩放引擎单点回放逐位一致", () => {
    const bases = DEMO_SCENES.map((s) => s.baseMs);
    const a = auditGlobalConsistency(bases, "reduced");
    expect(a.consistent).toBe(true);
    expect(a.samples).toEqual(bases.map((b) => Math.round(b * 0.6)));
  });

  it("档位持久化 + 循环呼吸间隙 1s", () => {
    saveMotionTier("reduced");
    expect(loadMotionTier()).toBe("reduced");
    expect(LOOP_PAUSE_MS).toBe(1000);
  });
});

describe("F161 个性化档案导出", () => {
  it("分节勾选导出（只要一节也行——包是菜单不是套餐）", () => {
    saveTokenTable(defaultTokenTable());
    const r = exportArchive({ meta: { name: "测试" }, include: ["theme"], privacyChecked: true });
    expect(r.privacyWarning).toBeNull();
    expect(Object.keys(r.pkg.sections)).toEqual(["theme"]);
    expect(r.pkg.themeHash).toBe(tokenTableHash(loadTokenTable()));
    expect(r.bytes).toBeGreaterThan(0);
  });

  it("未勾选隐私确认 → 阻断并给提示", () => {
    const r = exportArchive({ meta: { name: "x" }, include: ["theme"], privacyChecked: false });
    expect(r.privacyWarning).toContain("私人");
  });

  it("校验：format/版本矩阵/未知分节降级清单", () => {
    expect(validateArchive(null).ok).toBe(false);
    expect(validateArchive({ format: "x" }).ok).toBe(false);
    const old = validateArchive({ format: ARCHIVE_FORMAT, version: 0, sections: { theme: {} } });
    expect(old.ok).toBe(true);
    expect(old.degradations.length).toBeGreaterThan(0);
    const future = validateArchive({ format: ARCHIVE_FORMAT, version: ARCHIVE_VERSION + 1, sections: {} });
    expect(future.ok).toBe(false);
  });

  it("差异预览：将更改分节准确列出", () => {
    saveTokenTable(defaultTokenTable());
    const pkg = exportArchive({ meta: { name: "x" }, include: ["theme", "sound"], privacyChecked: true }).pkg;
    pkg.sections.sound = { mixer: { masterMute: true } };
    const d = diffPreview(pkg);
    expect(d.changedSections).toContain("sound");
  });

  it("导入 round-trip：主题哈希对拍 + 中断原子回退", () => {
    const table = defaultTokenTable();
    saveTokenTable(table);
    const pkg = exportArchive({ meta: { name: "x" }, include: ["theme"], privacyChecked: true }).pkg;
    personaStore.reset();
    const r = importArchive(pkg);
    expect(r.ok).toBe(true);
    expect(tokenTableHash(loadTokenTable())).toBe(tokenTableHash(table));
    // 哈希被篡改 → 导入拒绝
    const bad = JSON.parse(JSON.stringify(pkg)) as typeof pkg;
    bad.themeHash = "deadbeef";
    const r2 = importArchive(bad);
    expect(r2.ok).toBe(false);
    expect(r2.reason).toContain("哈希");
  });

  it("档案 15 分节在册", () => {
    expect(ARCHIVE_SECTIONS).toHaveLength(15);
  });
});

describe("F162 每应用主题例外", () => {
  it("上限 10 强制（超出提示精简）", () => {
    let cfg = loadAppExceptions();
    for (let i = 0; i < MAX_EXCEPTIONS; i++) {
      const r = addException(cfg, { appId: `app${i}`, appName: `应用${i}`, mode: "dark", accentOverride: null });
      expect(r.ok).toBe(true);
      cfg = r.config;
    }
    const over = addException(cfg, { appId: "appX", appName: "超", mode: "dark", accentOverride: null });
    expect(over.ok).toBe(false);
    expect(over.reason).toContain("上限");
  });

  it("同应用重复合并（冲突例外 → 合并提示）", () => {
    let cfg = loadAppExceptions();
    const r1 = addException(cfg, { appId: "term", appName: "终端", mode: "dark", accentOverride: null });
    cfg = r1.config;
    const r2 = addException(cfg, { appId: "term", appName: "终端", mode: "light", accentOverride: null });
    expect(r2.merged).toBe(true);
    expect(r2.config.exceptions).toHaveLength(1);
    expect(r2.config.exceptions[0]?.mode).toBe("light");
  });

  it("非法强调色拒绝", () => {
    const r = addException(loadAppExceptions(), { appId: "a", appName: "a", mode: "dark", accentOverride: "red" });
    expect(r.ok).toBe(false);
    expect(r.reason).toContain("#rrggbb");
  });

  it("全局广播跳过集合 + 锁定侧查询", () => {
    let cfg = loadAppExceptions();
    cfg = addException(cfg, { appId: "term", appName: "终端", mode: "dark", accentOverride: null }).config;
    expect(skipOnGlobalBroadcast(cfg.exceptions).has("term")).toBe(true);
    expect(lockedModeFor(cfg.exceptions, "term")).toBe("dark");
    expect(lockedModeFor(cfg.exceptions, "other")).toBeNull();
  });

  it("卸载自动清条目（返回被清名单）", () => {
    let cfg = loadAppExceptions();
    cfg = addException(cfg, { appId: "gone", appName: "已卸载应用", mode: "dark", accentOverride: null }).config;
    const pruned = pruneUninstalled(cfg, new Set<string>());
    expect(pruned.removed).toEqual(["已卸载应用"]);
    expect(pruned.config.exceptions).toHaveLength(0);
  });

  it("未接入令牌应用的诚实降级标注", () => {
    expect(degradationLabel({ appId: "a", appName: "a", mode: "dark", accentOverride: null, tokenAware: false })).toContain("未接入令牌");
    expect(degradationLabel({ appId: "a", appName: "a", mode: "dark", accentOverride: null, tokenAware: true })).toBeNull();
  });
});
