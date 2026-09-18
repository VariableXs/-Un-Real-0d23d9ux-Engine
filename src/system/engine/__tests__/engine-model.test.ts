import { describe, expect, it } from "vitest";
import {
  EMPTY_ENGINE_SESSION,
  ENGINE_BOOT_ESTIMATE,
  ENGINE_BOOT_STAGES,
  ENGINE_VWM_SUPPORT,
  cancelEngineLaunch,
  latencyP95,
  latencyTotal,
  markEngineWindowOpened,
  qualityParamsFor,
  reduceEngineMsg,
  requestEngineLaunch,
  varianceWithinFivePct,
  type EngineSession,
  type EngineStateMsg,
} from "../engineModel";

function msg(seq: number, kind: EngineStateMsg["kind"], extra?: Partial<EngineStateMsg>): EngineStateMsg {
  return { seq, kind, ...extra };
}

describe("任务 51 · 引擎冷启动阶段化叙事", () => {
  it("20-40s 拆解为 5 个命名阶段，每阶段有名称与预估时长", () => {
    expect(ENGINE_BOOT_STAGES.length).toBe(5);
    for (const s of ENGINE_BOOT_STAGES) {
      expect(s.key.length).toBeGreaterThan(0);
      expect(s.label.length).toBeGreaterThan(0);
      expect(s.estimatedS).toBeGreaterThan(0);
    }
  });

  it("预估区间落在 20-40s（如实公示口径，不虚构快启动）", () => {
    expect(ENGINE_BOOT_ESTIMATE.minS).toBeGreaterThanOrEqual(20);
    expect(ENGINE_BOOT_ESTIMATE.maxS).toBeLessThanOrEqual(40);
    expect(ENGINE_BOOT_ESTIMATE.minS).toBeLessThan(ENGINE_BOOT_ESTIMATE.maxS);
  });

  it("boot-stage 事件推进当前阶段；未知阶段忽略不点亮", () => {
    let s: EngineSession = { ...EMPTY_ENGINE_SESSION };
    s = reduceEngineMsg(s, msg(1, "boot-stage", { stage: "vhdx-mount" })).next;
    expect(s.lifecycle).toBe("starting");
    expect(s.stage).toBe("vhdx-mount");
    s = reduceEngineMsg(s, msg(2, "boot-stage", { stage: "vm-power" })).next;
    expect(s.stage).toBe("vm-power");
    s = reduceEngineMsg(s, msg(3, "boot-stage", { stage: "bogus-stage" })).next;
    expect(s.stage).toBe("vm-power");
  });
});

describe("任务 50 · 拉起协议 reducer（幂等 / 乱序安全）", () => {
  it("重复投递（seq ≤ lastSeq）被吸收，状态不回卷", () => {
    let s = reduceEngineMsg(EMPTY_ENGINE_SESSION, msg(1, "boot-stage", { stage: "vm-create" })).next;
    const again = reduceEngineMsg(s, msg(1, "boot-stage", { stage: "vhdx-mount" }));
    expect(again.next).toBe(s); // 幂等：同一对象（零副作用）
    const stale = reduceEngineMsg(s, msg(0, "closed"));
    expect(stale.next.lifecycle).toBe("starting"); // 旧消息不得把 starting 打回 closed
  });

  it("ready 就绪 → 挂起请求逐个自动开窗（open-app 副作用；计数由 store 落实）", () => {
    let s = requestEngineLaunch(EMPTY_ENGINE_SESSION, "app.foobar", 1000).next;
    s = requestEngineLaunch(s, "app.baz", 1100).next;
    const { next, effects } = reduceEngineMsg(s, msg(9, "ready"));
    expect(next.lifecycle).toBe("ready");
    expect(next.pending).toEqual([]);
    expect(next.openedWindows).toBe(0); // 纯模型不虚计 —— store 在 openVwmEngine 成功后 mark
    expect(effects.filter((e) => e.type === "open-app").map((e) => (e as { appKey: string }).appKey)).toEqual([
      "app.foobar",
      "app.baz",
    ]);
  });

  it("就绪前取消的请求：撤卡不启动（drop-placeholder 而非 open-app）", () => {
    let s = requestEngineLaunch(EMPTY_ENGINE_SESSION, "app.foobar", 1000).next;
    s = cancelEngineLaunch(s, "app.foobar");
    const { next, effects } = reduceEngineMsg(s, msg(2, "ready"));
    expect(next.openedWindows).toBe(0);
    expect(effects).toContainEqual({ type: "drop-placeholder", appKey: "app.foobar" });
  });
});

describe("任务 54 · 异常三场景（占位卡撤除 + 如实原因 + 下次插入恢复）", () => {
  it("场景一 拉起中拔盘（usb-removed @ starting）：撤卡 + 提示下次插入重试", () => {
    let s = requestEngineLaunch(EMPTY_ENGINE_SESSION, "app.foobar", 1000).next;
    s = reduceEngineMsg(s, msg(1, "boot-stage", { stage: "vm-power" })).next;
    const { next, effects } = reduceEngineMsg(s, msg(2, "usb-removed", { reason: "U 盘已移除" }));
    expect(next.lifecycle).toBe("closed");
    expect(next.pending).toEqual([]);
    expect(effects.some((e) => e.type === "drop-placeholder")).toBe(true);
    const note = effects.find((e) => e.type === "notify") as { message: string };
    expect(note.message).toContain("下次插入");
  });

  it("场景二 VM 崩溃（crashed）：撤卡 + 如实原因，Variable 桌面不受影响（独立会话状态）", () => {
    let s = requestEngineLaunch(EMPTY_ENGINE_SESSION, "app.foobar", 1000).next;
    const { next, effects } = reduceEngineMsg(s, msg(5, "crashed", { reason: "虚拟机异常退出" }));
    expect(next.lifecycle).toBe("crashed");
    expect(next.reason).toBe("虚拟机异常退出");
    expect(effects.some((e) => e.type === "drop-placeholder")).toBe(true);
    // 引擎崩溃不触碰 VWM/Shell 其它状态（本模块零对 vwmStore 写入 —— 由实现约束保证）
  });

  it("场景三 就绪后拔盘：会话结束 + 恢复语义（closed 态可重新 request）", () => {
    let s = reduceEngineMsg(EMPTY_ENGINE_SESSION, msg(1, "ready")).next;
    s = reduceEngineMsg(s, msg(2, "usb-removed", { reason: "U 盘已移除" })).next;
    expect(s.lifecycle).toBe("closed");
    const again = requestEngineLaunch(s, "app.foobar", 2000);
    expect(again.needWake).toBe(true);
  });

  it("100 次开关无泄漏：开-关循环后 pending 清空、openedWindows 精确计数（store 侧 mark 模拟）", () => {
    let s: EngineSession = { ...EMPTY_ENGINE_SESSION };
    let seq = 0;
    for (let i = 0; i < 100; i += 1) {
      seq += 1;
      s = requestEngineLaunch(s, "app.stress", i).next;
      seq += 1;
      s = reduceEngineMsg(s, msg(seq, "ready")).next;
      expect(s.pending).toEqual([]);
      s = markEngineWindowOpened(s); // store 副作用：openVwmEngine 成功后计数
      seq += 1;
      s = reduceEngineMsg(s, msg(seq, "closed")).next;
      expect(s.lifecycle).toBe("closed");
    }
    expect(s.openedWindows).toBe(100); // 每轮恰好 1 窗
    s = markEngineWindowOpened(s);
    expect(s.openedWindows).toBe(101);
  });
});

describe("任务 53 · 画质三档与延迟测量", () => {
  it("三档参数表：办公/均衡/游戏逐档核对（fps 与码率单调上升）", () => {
    const o = qualityParamsFor("office");
    const b = qualityParamsFor("balanced");
    const g = qualityParamsFor("gaming");
    expect(o.fps).toBeLessThan(b.fps);
    expect(b.fps).toBeLessThan(g.fps);
    expect(o.bitrateKbps).toBeLessThan(b.bitrateKbps);
    expect(b.bitrateKbps).toBeLessThan(g.bitrateKbps);
    expect(o.codecPreset).toBe("speed");
    expect(g.codecPreset).toBe("quality");
  });

  it("开放性：档位参数用户可自定义覆盖 + 非法值钳制", () => {
    const p = qualityParamsFor("office", { fps: 90, bitrateKbps: 999999, codecPreset: "quality" });
    expect(p.fps).toBe(90);
    expect(p.bitrateKbps).toBe(80000); // 上限钳制
    expect(p.codecPreset).toBe("quality");
    expect(qualityParamsFor("balanced", { fps: 5 }).fps).toBe(10); // 下限钳制
    expect(qualityParamsFor("balanced", null).fps).toBe(30); // 空覆盖 = 纯预设
  });

  it("延迟五段拆解合计 + P95 线性插值", () => {
    const total = latencyTotal({ captureMs: 2, encodeMs: 8, transmitMs: 20, decodeMs: 6, composeMs: 4 });
    expect(total).toBe(40);
    expect(latencyP95([])).toBe(0);
    expect(latencyP95([10, 20, 30, 40])).toBe(38.5); // 0.95 索引 2.85 → 插值
    expect(latencyP95([5, 10])).toBe(9.75);
  });

  it("同机三次方差 ≤5% 判定（任务 53 验收口径）", () => {
    expect(varianceWithinFivePct([40.0, 41.0, 40.5])).toBe(true); // spread/mean ≈ 2.4%
    expect(varianceWithinFivePct([40.0, 45.0, 50.0])).toBe(false); // 23.8%
    expect(varianceWithinFivePct([40.0, 41.0])).toBe(false); // 样本不足
  });
});

describe("任务 50 完善性 · 与 embed 收编窗口能力对照表", () => {
  it("对照表逐项如实：不支持的特性必须给出说明（卷帘/标签组/不透明度）", () => {
    expect(ENGINE_VWM_SUPPORT.length).toBeGreaterThanOrEqual(8);
    for (const row of ENGINE_VWM_SUPPORT) {
      expect(row.note.length).toBeGreaterThan(0);
      expect(row.feature.length).toBeGreaterThan(0);
    }
    const roll = ENGINE_VWM_SUPPORT.find((r) => r.feature.includes("卷帘"));
    expect(roll?.supported).toBe(false);
    const tab = ENGINE_VWM_SUPPORT.find((r) => r.feature.includes("标签组"));
    expect(tab?.supported).toBe(false);
    const geom = ENGINE_VWM_SUPPORT.find((r) => r.feature.includes("几何持久化"));
    expect(geom?.supported).toBe(true);
  });
});
