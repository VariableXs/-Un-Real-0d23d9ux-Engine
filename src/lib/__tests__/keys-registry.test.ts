/** AI-07 · N-17 快捷键中心单测（注册表化/冲突检测/Profile/速查表导出/学习模式查询）。 */
import { describe, expect, it } from "vitest";
import {
  BUILTIN_PROFILES,
  defaultKeymap,
  detectConflicts,
  effectiveKeymap,
  exportCheatSheetHtml,
  exportCheatSheetMarkdown,
  exportProfile,
  findBindingByAccel,
  importProfile,
} from "../keys/registry";

const t = (k: string) => k; // 测试词典：恒等

describe("N-17 注册表化", () => {
  it("默认表含既有动作 + AI-07 效率增量，无重复 action", () => {
    const km = defaultKeymap();
    const ids = km.map((b) => b.action);
    expect(new Set(ids).size).toBe(ids.length);
    for (const must of ["commandPalette", "purePaste", "quickNote", "snipRegion", "clipboardHistory", "notifyCenter"]) {
      expect(ids).toContain(must);
    }
  });

  it("槽位冻结：ctrl+alt+n 归全局速记，notifyCenter 已迁移", () => {
    const km = defaultKeymap();
    expect(km.find((b) => b.action === "quickNote")!.accel).toBe("ctrl+alt+n");
    expect(km.find((b) => b.action === "notifyCenter")!.accel).toBe("ctrl+shift+n");
  });

  it("用户覆盖生效；非法覆盖如实回退默认", () => {
    const km = effectiveKeymap({ snapLeft: "ctrl+alt+q", snapRight: "!!!" });
    expect(km.find((b) => b.action === "snapLeft")!.accel).toBe("ctrl+alt+q");
    expect(km.find((b) => b.action === "snapRight")!.accel).toBe("ctrl+alt+right");
  });
});

describe("冲突检测", () => {
  it("内部互冲检出", () => {
    const km = effectiveKeymap({ snapLeft: "ctrl+alt+right" }); // 与 snapRight 撞
    const conflicts = detectConflicts(km);
    const hit = conflicts.find((c) => c.accel === "ctrl+alt+right");
    expect(hit).toBeDefined();
    expect(hit!.actions.sort()).toEqual(["snapLeft", "snapRight"]);
  });

  it("三方占用表命中（带建议）", () => {
    const km = effectiveKeymap({ purePaste: "ctrl+shift+esc" });
    const conflicts = detectConflicts(km);
    const hit = conflicts.find((c) => c.accel === "ctrl+shift+esc");
    expect(hit?.thirdParty?.app).toContain("Windows");
  });

  it("默认表无内部互冲", () => {
    const internal = detectConflicts(defaultKeymap()).filter((c) => c.actions.length > 1);
    expect(internal).toHaveLength(0);
  });
});

describe("Profile 方案档（.vkeys）", () => {
  it("内置三档可应用且导入导出往返一致", () => {
    for (const p of BUILTIN_PROFILES) {
      const km = effectiveKeymap(p.overrides);
      const exported = exportProfile(p.id, km);
      const round = importProfile(JSON.stringify(exported));
      expect(round.profile.overrides).toEqual(exported.overrides);
    }
  });

  it("单手档覆盖生效", () => {
    const km = effectiveKeymap(BUILTIN_PROFILES[1]!.overrides);
    expect(km.find((b) => b.action === "snapLeft")!.accel).toBe("ctrl+alt+q");
  });

  it("非法档案拒绝", () => {
    expect(() => importProfile('{"format":"nope","version":1}')).toThrow();
  });
});

describe("V-50 速查表导出", () => {
  it("Markdown：按域分组、含生成时间、冲突标 ⚠", () => {
    const km = effectiveKeymap({ snapLeft: "ctrl+alt+right" });
    const md = exportCheatSheetMarkdown(km, { lang: "zh", t, profileName: "test", now: new Date(0) });
    expect(md).toContain("## scGroup_system");
    expect(md).toContain("## scGroup_efficiency");
    expect(md).toContain("⚠️");
    expect(md).toContain("Ctrl + Alt + Right");
  });

  it("HTML：A4 双栏样式、冲突行标红、可转义", () => {
    const km = effectiveKeymap({});
    const html = exportCheatSheetHtml(km, { lang: "en", t, profileName: "p<name>" });
    expect(html).toContain("column-count: 2");
    expect(html).toContain("@page");
    expect(html).toContain("p&lt;name&gt;");
  });

  it("导出内容与生效表一致（每条绑定均出现）", () => {
    const km = effectiveKeymap({ snapUp: "ctrl+alt+q" });
    const md = exportCheatSheetMarkdown(km, { lang: "zh", t, profileName: "x", now: new Date(0) });
    expect(md).toContain("Ctrl + Alt + Q");
  });
});

describe("学习模式查询", () => {
  it("按 accel 反查（归一化）", () => {
    const km = defaultKeymap();
    expect(findBindingByAccel(km, "Ctrl+Alt+N").map((b) => b.action)).toEqual(["quickNote"]);
    expect(findBindingByAccel(km, "ctrl+alt+q")).toHaveLength(0);
    expect(findBindingByAccel(km, "garbage")).toHaveLength(0);
  });
});
