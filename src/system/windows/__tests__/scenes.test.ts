import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  applyScene,
  contextSuggestion,
  deleteScene,
  loadScenes,
  manualTrigger,
  saveScene,
  timerTriggerAt,
  unsavedGuard,
  validateScene,
} from "../scenes";
import type { Scene } from "../scenes";

function scene(p: Partial<Scene>): Scene {
  return {
    id: "s1",
    name: "晨间写作",
    visual: { theme: "light", fontSize: "md" },
    orchestration: {},
    behavior: { dnd: true },
    triggers: [{ type: "manual" }],
    ...p,
  };
}

beforeEach(() => localStorage.clear());

describe("N-05 场景 CRUD 与校验", () => {
  it("保存/加载/删除（持久化跨读取存活）", () => {
    saveScene(scene({}));
    saveScene(scene({ id: "s2", name: "夜间阅读" }));
    expect(loadScenes()).toHaveLength(2);
    deleteScene("s1");
    expect(loadScenes().map((s) => s.id)).toEqual(["s2"]);
  });

  it("合法场景零错误；语法错误行级报错", () => {
    expect(validateScene(scene({}))).toEqual([]);
    const errs = validateScene(scene({ name: "", triggers: [{ type: "timer", at: "25:00" }] }));
    expect(errs.some((e) => e.includes("name"))).toBe(true);
    expect(errs.some((e) => e.includes("HH:mm"))).toBe(true);
  });
});

describe("N-05 未保存窗口守卫（验收 ④）", () => {
  it("标题含未保存特征 → 拦截并列出窗口", () => {
    const g = unsavedGuard(["文档.txt - 记事本", "草稿* 未保存", "干净窗口"]);
    expect(g.blocked).toBe(true);
    expect(g.titles).toEqual(["草稿* 未保存"]);
  });
  it("干净标题不拦截（绝不误伤）", () => {
    expect(unsavedGuard(["文档", "代码", "浏览器"]).blocked).toBe(false);
  });
});

describe("N-05 应用与全量回滚（验收 ①②）", () => {
  const ctx = (fail?: string) => ({
    applyVisual: vi.fn(() => {
      if (fail === "visual") throw new Error("视觉包应用失败");
    }),
    applyBehavior: vi.fn(),
    applySnapshot: vi.fn(),
    applyStage: vi.fn(),
    applySnap: vi.fn(),
    workArea: { x: 0, y: 0, w: 1920, h: 1046 },
    titles: ["普通窗口"],
  });

  it("三包顺序应用成功", () => {
    const c = ctx();
    const r = applyScene(
      scene({ orchestration: { stageRef: "stage-a", snaps: [{ app: "write", rect: { x: 0, y: 0, w: 0.5, h: 1 } }] } }),
      c,
    );
    expect(r.ok).toBe(true);
    expect(r.applied).toEqual(["orchestration", "visual", "behavior"]);
    expect(c.applySnap).toHaveBeenCalledWith("write", { x: 0, y: 0, w: 960, h: 1046 });
  });

  it("任一包失败 → applied 为空 + 如实报错（全量回滚，绝不半套生效）", () => {
    const c = ctx("visual");
    const r = applyScene(scene({}), c);
    expect(r.ok).toBe(false);
    expect(r.applied).toEqual([]);
    expect(r.error).toContain("视觉包应用失败");
  });

  it("守卫生效：未保存窗口存在时不应用任何包", () => {
    const c = ctx();
    c.titles = ["文档* 未保存"];
    const r = applyScene(scene({}), c);
    expect(r.ok).toBe(false);
    expect(r.guard?.blocked).toBe(true);
    expect(c.applyVisual).not.toHaveBeenCalled();
  });
});

describe("N-05 触发器三型（验收 ①）", () => {
  it("manual：直通", () => {
    const s = scene({});
    expect(manualTrigger(s).id).toBe("s1");
  });
  it("timer：按 HH:mm 命中场景", () => {
    const scenes = [scene({ id: "morning" }), scene({ id: "night", triggers: [{ type: "timer", at: "22:30" }] })];
    expect(timerTriggerAt(scenes, "22:30")?.id).toBe("night");
    expect(timerTriggerAt(scenes, "12:00")).toBeNull();
  });
  it("context：前台应用匹配 → 非强制建议（含场景与原因）", () => {
    const scenes = [scene({ id: "code-scene", triggers: [{ type: "context", app: "code" }] })];
    expect(contextSuggestion(scenes, "code")?.sceneId).toBe("code-scene");
    expect(contextSuggestion(scenes, "fate")).toBeNull();
  });
});
