import { beforeEach, describe, expect, it } from "vitest";
import { defaultOverride, effectiveMenuIds, loadMenuOverride, saveMenuOverride, TASKBAR_MENU_REGISTRY } from "../../desktop/taskbarMenu";

/** M-15：任务栏空区菜单注册表 + 覆盖（默认项集与现状一致；只含注册表安全项）。 */
describe("taskbarMenu (M-15)", () => {
  beforeEach(() => {
    try { localStorage.removeItem("variable:taskbar:blankmenu:v1"); } catch { /* ignore */ }
  });

  it("默认覆盖 = 注册表默认显隐（壁纸中心默认可见）", () => {
    expect(effectiveMenuIds(defaultOverride())).toEqual([
      "showDesktop",
      "wallpaperCenter",
      "launcher",
      "taskbarSettings",
    ]);
    expect(TASKBAR_MENU_REGISTRY.every((e) => TASKBAR_MENU_REGISTRY.filter((x) => x.id === e.id).length === 1)).toBe(true);
  });

  it("隐藏项不再渲染；顺序可调", () => {
    saveMenuOverride({ order: ["sticky", "showDesktop", "taskbarSettings"], hidden: ["launcher"] });
    const ids = effectiveMenuIds(loadMenuOverride());
    expect(ids[0]).toBe("sticky");
    expect(ids).not.toContain("launcher");
  });

  it("sanitize：未知 id / 全空 order 被拒绝恢复默认", () => {
    saveMenuOverride({ order: ["hack" as string, "showDesktop"], hidden: [] });
    const o = loadMenuOverride();
    expect(o.order).toContain("showDesktop");
    expect(o.order).not.toContain("hack");
    saveMenuOverride({ order: [], hidden: [] });
    expect(effectiveMenuIds(loadMenuOverride())).toEqual([
      "showDesktop",
      "wallpaperCenter",
      "launcher",
      "taskbarSettings",
    ]);
  });

  it("sanitize：老用户已存覆盖 → 新默认可见项补入 order 尾部（升级后可见壁纸中心）", () => {
    // 模拟升级前的覆盖（无 wallpaperCenter）
    saveMenuOverride({ order: ["showDesktop", "launcher", "taskbarSettings"], hidden: [] });
    const o = loadMenuOverride();
    expect(o.order).toContain("wallpaperCenter");
    expect(effectiveMenuIds(o)).toContain("wallpaperCenter");
    // 默认隐藏项（sticky）仍补入 hidden
    expect(o.hidden).toContain("sticky");
  });
});
