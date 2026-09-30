import { beforeEach, describe, expect, it } from "vitest";
import {
  officialPresets, defaultStartLayout, applyPreset, saveAsCustom,
  diffFromLayout, fullscreenFitWarning, elderTouchOk, loadStartPresetConfig,
  SIMPLE_FIXED_COUNT, ELDER_MIN_TOUCH_PX,
} from "../startpresets";
import {
  parseCmapFormat4, scanFont, monospaceCheck, generalCharset,
  baseCharset, WARN_THRESHOLD, DANGER_THRESHOLD, MISSING_LIST_PREVIEW,
} from "../fontguard";
import { personaStore } from "../store";

beforeEach(() => {
  personaStore.reset();
});

describe("F158 开始菜单布局预设", () => {
  it("三官方预设定义一致（效率/简洁/全屏）", () => {
    const p = officialPresets();
    expect(p).toHaveLength(3);
    expect(p.map((x) => x.name)).toEqual(["效率", "简洁", "全屏"]);
    expect(p[1]!.diff.pinned).toHaveLength(SIMPLE_FIXED_COUNT);
    expect(p[2]!.diff.fullscreen).toBe(true);
  });

  it("切换结果与定义一致（逐项对拍）：简洁预设关掉最近/推荐区", () => {
    const simple = officialPresets()[1]!;
    const { layout, skipped } = applyPreset(simple, new Set(["common-1", "common-2", "common-3", "common-4", "common-5", "common-6"]));
    expect(layout.regions.recent).toBe(false);
    expect(layout.regions.recommended).toBe(false);
    expect(layout.largeIcons).toBe(true);
    expect(skipped).toHaveLength(0);
  });

  it("卸载应用项跳过+标注", () => {
    const efficiency = officialPresets()[0]!;
    const { layout, skipped } = applyPreset(efficiency, new Set(["common-1"]));
    expect(layout.pinned).toEqual(["common-1"]);
    expect(skipped).toHaveLength(7);
  });

  it("自定义保存与官方同名 → 自动加「（自定义）」", () => {
    const cfg = loadStartPresetConfig();
    const { preset } = saveAsCustom(cfg, "简洁", defaultStartLayout());
    expect(preset.name).toBe("简洁（自定义）");
    expect(preset.official).toBe(false);
  });

  it("差异集存储：diffFromLayout 只记与默认的差异", () => {
    const base = defaultStartLayout();
    const layout = { ...base, fullscreen: true };
    const diff = diffFromLayout(base, layout);
    expect(Object.keys(diff)).toEqual(["fullscreen"]);
    expect(diffFromLayout(base, base)).toEqual({});
  });

  it("全屏预设分辨率警告 + 长辈模式可点面积达标线", () => {
    expect(fullscreenFitWarning(768)).toBeNull(); // ≥768px 适配
    expect(fullscreenFitWarning(720)).toContain("768"); // <768px 警告
    expect(elderTouchOk(56)).toBe(true);
    expect(elderTouchOk(48)).toBe(false);
    expect(ELDER_MIN_TOUCH_PX).toBe(56);
  });
});

describe("F159 字体安全档", () => {
  it("cmap format 4 最小解析：手工构造 sfnt 头覆盖码点", () => {
    // 构造只含 cmap format 4 单段（U+0041..U+004A）的最小字体。
    const buf = new ArrayBuffer(512);
    const v = new DataView(buf);
    v.setUint32(0, 0x00010000); // sfnt version
    v.setUint16(4, 1); // numTables
    // 表记录 "cmap" @ offset 12
    const enc = new TextEncoder();
    enc.encodeInto("cmap", new Uint8Array(buf, 12, 4));
    v.setUint32(20, 28); // cmap offset
    // cmap: version 0, numTables 1, platform 3 enc 1 offset 12
    v.setUint16(28, 0);
    v.setUint16(30, 1);
    v.setUint16(32, 3);
    v.setUint16(34, 1);
    v.setUint32(36, 12);
    // format 4 @ 40: format/length/langCode/segCountX2/searchRange/entrySelector/rangeShift/endCode/reservedPad/startCode/idDelta
    v.setUint16(40, 4);
    v.setUint16(42, 32);
    v.setUint16(44, 0);
    v.setUint16(46, 2); // segCountX2 = 2（1 段）
    v.setUint16(48, 2); // searchRange
    v.setUint16(50, 0); // entrySelector
    v.setUint16(52, 2); // rangeShift
    v.setUint16(54, 0x004a); // endCode[0]
    v.setUint16(56, 0); // reservedPad
    v.setUint16(58, 0x0041); // startCode[0]
    v.setInt16(60, 0); // idDelta[0]
    const r = parseCmapFormat4(buf);
    expect(r.ok).toBe(true);
    expect(r.covered.has(0x41)).toBe(true);
    expect(r.covered.has(0x4a)).toBe(true);
    expect(r.covered.has(0x4b)).toBe(false);
  });

  it("损坏字体 → 拒绝导入（错误可解释）", () => {
    const r = parseCmapFormat4(new ArrayBuffer(4));
    expect(r.ok).toBe(false);
    expect(r.reason).toContain("过短");
  });

  it("三档判定阈值：0%/8%/20% 缺字样本 → ok/warn/danger", () => {
    // 构造覆盖全集/92%/80% 的三个 cmap 集合。
    const all = [...generalCharset(), ...baseCharset()];
    function coveredByRate(rate: number): Set<number> {
      const covered = new Set<number>();
      const keep = Math.floor(all.length * rate);
      for (let i = 0; i < keep; i++) covered.add((all[i] as string).codePointAt(0) ?? 0);
      return covered;
    }
    expect(scanFont({ fontId: "f0", covered: coveredByRate(1) }).verdict).toBe("ok");
    const w = scanFont({ fontId: "f8", covered: coveredByRate(0.92) });
    expect(w.verdict).toBe("warn");
    expect(w.interfaceMissingRate).toBeGreaterThan(WARN_THRESHOLD);
    expect(w.interfaceMissingRate).toBeLessThanOrEqual(DANGER_THRESHOLD);
    const d = scanFont({ fontId: "f20", covered: coveredByRate(0.8) });
    expect(d.verdict).toBe("danger");
    expect(d.interfaceMissingRate).toBeGreaterThan(DANGER_THRESHOLD);
  });

  it("缺字清单前 20 字展示准确", () => {
    const covered = new Set<number>([..."ab中文"].map((c) => c.codePointAt(0) ?? 0));
    const r = scanFont({ fontId: "f", covered });
    expect(r.missingChars.slice(0, MISSING_LIST_PREVIEW)).toHaveLength(MISSING_LIST_PREVIEW);
    expect(r.missingChars).not.toContain("a");
  });

  it("界面集优先口径：interfaceChars 提供时缺字率按界面集", () => {
    const covered = new Set<number>([..."你好界"].map((c) => c.codePointAt(0) ?? 0));
    const r = scanFont({ fontId: "f", covered, interfaceChars: "你好界面测试".split("") });
    // 界面集去重 6 字（你好界面测试），缺 面/试/测 3 字 → 3/6
    expect(r.interfaceMissingRate).toBeCloseTo(3 / 6);
  });

  it("等宽判定：advance 一致为等宽", () => {
    expect(monospaceCheck({ "0": 600, "1": 600, A: 600 })).toBe(true);
    expect(monospaceCheck({ "0": 600, "1": 600, A: 550 })).toBe(false);
    expect(monospaceCheck({ "0": 600 })).toBe(false);
  });
});
