import { beforeEach, describe, expect, it } from "vitest";
import {
  compatibility,
  dndStore,
  endDrag,
  registerDropTarget,
  setDndHover,
  startDrag,
  stash,
  unregisterDropTarget,
  unstash,
  type DndPayload,
} from "../bus";

const files: DndPayload = { kind: "files", files: ["a.md", "b.md"], sourceLabel: "写作" };
const text: DndPayload = { kind: "text", text: "hello", sourceLabel: "剪贴板" };
const win: DndPayload = { kind: "window", winId: "vwm-write-1", sourceLabel: "虚拟窗口" };

/** 测试间复位（bus 是模块级单例 store）。 */
beforeEach(() => {
  dndStore.setState({ payload: null, hoverId: null, stash: null, targets: {} });
});

describe("U-17 目标注册表", () => {
  it("注册与注销", () => {
    registerDropTarget("t1", ["files"]);
    registerDropTarget("t2", ["text", "window"]);
    expect(Object.keys(dndStore.getState().targets).sort()).toEqual(["t1", "t2"]);
    expect(dndStore.getState().targets["t1"]).toEqual({ id: "t1", accepts: ["files"] });

    unregisterDropTarget("t1");
    expect(Object.keys(dndStore.getState().targets)).toEqual(["t2"]);

    // 注销不存在的 id：无操作、不抛错
    expect(() => unregisterDropTarget("t1")).not.toThrow();
    expect(Object.keys(dndStore.getState().targets)).toEqual(["t2"]);
  });

  it("同 id 重复注册 = 更新 accepts（副本，防外部篡改）", () => {
    const accepts: Array<"files" | "text" | "window"> = ["files"];
    registerDropTarget("t1", accepts);
    accepts.push("text"); // 外部数组变动不影响注册表
    expect(dndStore.getState().targets["t1"]?.accepts).toEqual(["files"]);
    registerDropTarget("t1", ["text"]);
    expect(dndStore.getState().targets["t1"]?.accepts).toEqual(["text"]);
    expect(Object.keys(dndStore.getState().targets)).toEqual(["t1"]);
  });

  it("注销悬停中的目标时同步清除悬停态", () => {
    registerDropTarget("t1", ["files"]);
    startDrag(files);
    setDndHover("t1");
    expect(dndStore.getState().hoverId).toBe("t1");
    unregisterDropTarget("t1");
    expect(dndStore.getState().hoverId).toBeNull();
  });
});

describe("U-17 兼容性矩阵（files ↔ text ↔ window 全组合）", () => {
  const cases: Array<[DndPayload, Array<"files" | "text" | "window">, "accept" | "forbidden"]> = [
    [files, ["files"], "accept"],
    [files, ["text"], "forbidden"],
    [files, ["window"], "forbidden"],
    [files, ["files", "text"], "accept"],
    [files, ["text", "window"], "forbidden"],
    [files, ["files", "text", "window"], "accept"],
    [text, ["files"], "forbidden"],
    [text, ["text"], "accept"],
    [text, ["window"], "forbidden"],
    [text, ["files", "text"], "accept"],
    [text, ["files", "window"], "forbidden"],
    [text, ["files", "text", "window"], "accept"],
    [win, ["files"], "forbidden"],
    [win, ["text"], "forbidden"],
    [win, ["window"], "accept"],
    [win, ["files", "text"], "forbidden"],
    [win, ["files", "window"], "accept"],
    [win, ["text", "window"], "accept"],
    [win, ["files", "text", "window"], "accept"],
  ];
  it.each(cases)("%j × %j → %s", (payload, accepts, expected) => {
    expect(compatibility(payload, accepts)).toBe(expected);
  });

  it("空 accepts 一律禁止", () => {
    expect(compatibility(files, [])).toBe("forbidden");
    expect(compatibility(text, [])).toBe("forbidden");
    expect(compatibility(win, [])).toBe("forbidden");
  });
});

describe("U-17 会话状态流", () => {
  it("startDrag → hover → endDrag", () => {
    expect(dndStore.getState().payload).toBeNull();
    startDrag(files);
    expect(dndStore.getState().payload).toEqual(files);
    expect(dndStore.getState().hoverId).toBeNull();

    registerDropTarget("t1", ["files"]);
    setDndHover("t1");
    expect(dndStore.getState().hoverId).toBe("t1");
    // 同值悬停不触发订阅（无变化 = 无补丁）
    setDndHover("t1");
    expect(dndStore.getState().hoverId).toBe("t1");

    endDrag();
    expect(dndStore.getState().payload).toBeNull();
    expect(dndStore.getState().hoverId).toBeNull();

    // 再次 startDrag 清掉上一次悬停残留
    startDrag(text);
    setDndHover("t1");
    startDrag(win);
    expect(dndStore.getState().hoverId).toBeNull();
    expect(dndStore.getState().payload).toEqual(win);
  });
});

describe("U-17 收藏托盘（单槽）", () => {
  it("stash / unstash 往返", () => {
    // 无会话不可寄存
    expect(stash()).toBe(false);

    startDrag(files);
    expect(stash()).toBe(true);
    // 寄存即结束会话
    expect(dndStore.getState().payload).toBeNull();
    expect(dndStore.getState().hoverId).toBeNull();
    expect(dndStore.getState().stash).toEqual(files);

    // 取出
    const got = unstash();
    expect(got).toEqual(files);
    expect(dndStore.getState().stash).toBeNull();
    // 空槽再取 → null
    expect(unstash()).toBeNull();

    // 取出后可重新进入拖拽态（模拟视觉层行为）
    if (got) startDrag(got);
    expect(dndStore.getState().payload).toEqual(files);
  });

  it("寄存期间再次 stash 拒绝（单槽）", () => {
    startDrag(files);
    expect(stash()).toBe(true);

    // 托盘占用时，新的拖拽会话也无法寄存
    startDrag(text);
    expect(stash()).toBe(false);
    expect(dndStore.getState().stash).toEqual(files); // 仍是原寄存物
    expect(dndStore.getState().payload).toEqual(text); // 会话未被动

    // 取出旧寄存物后才能寄存新的
    expect(unstash()).toEqual(files);
    expect(stash()).toBe(true);
    expect(dndStore.getState().stash).toEqual(text);
  });
});
