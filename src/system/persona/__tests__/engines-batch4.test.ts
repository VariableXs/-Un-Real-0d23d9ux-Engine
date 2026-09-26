import { describe, expect, it } from "vitest";
import {
  hexToRgb, rgbToHex, luminance, rgbToHsl, hslToRgb,
  medianCut, extractPalette, classifyTone, judgeWallpaperHarmony,
  deriveTokensFromPalette, surfaceFamilyFrom, nameColor, luminance as lum,
} from "../palette-engine";
import {
  packAtlas, hitTest, pickIconSize, mipChain, cacheKey, invalidationPrefix,
  planSwapBudget, makeRollbackEntry, DEFAULT_ATLAS_OPTIONS, ICON_SIZES,
} from "../icon-atlas";
import {
  smoothDamp, trailSettled, TrailBuffer, ClickMachine, transformHotspot,
  transformHotspotCrop, pickPointerSpriteTier, AnimCursorClock, snapToPhysicalPixel,
} from "../cursor-physics";
import {
  oscillatorSample, envelopeAt, envelopeTotalMs, renderSynth, normalizeGain,
  applyEqualPowerFade, resampleLinear, encodeWav16, defaultSynthSpec, synthesizeEvent,
  ENVELOPE_PRESETS,
} from "../sound-synthesis";

// ---------- palette-engine ----------

describe("palette-engine · 调色板引擎（F151/F154 深化）", () => {
  it("hex/rgb/hsl 往返一致（色彩基础数学的 round-trip 契约）", () => {
    const hex = "#6e7fd4";
    const rgb = hexToRgb(hex)!;
    expect(rgb).toEqual({ r: 110, g: 127, b: 212 });
    expect(rgbToHex(rgb)).toBe(hex);
    const hsl = rgbToHsl(rgb);
    const back = hslToRgb(hsl.h, hsl.s, hsl.l);
    expect(Math.abs(back.r - rgb.r)).toBeLessThanOrEqual(1);
    expect(Math.abs(back.g - rgb.g)).toBeLessThanOrEqual(1);
    expect(Math.abs(back.b - rgb.b)).toBeLessThanOrEqual(1);
    expect(hexToRgb("not-a-color")).toBeNull();
  });

  it("白与黑的亮度是 1 与 0（WCAG 线性化正确性）", () => {
    expect(luminance({ r: 255, g: 255, b: 255 })).toBeCloseTo(1, 3);
    expect(lum({ r: 0, g: 0, b: 0 })).toBeCloseTo(0, 6);
  });

  it("中位切分：双色图分出两簇且占比和为 1", () => {
    const px = [...Array(200).fill(0).map(() => ({ r: 200, g: 40, b: 40 })), ...Array(200).fill(0).map(() => ({ r: 40, g: 60, b: 200 }))];
    const sw = medianCut(px, 4);
    expect(sw.length).toBeGreaterThanOrEqual(2);
    const popSum = sw.reduce((s, w) => s + w.population, 0);
    expect(popSum).toBeCloseTo(1, 5);
  });

  it("extractPalette：亮图判 light、暗图判 dark（判类阈值实测）", () => {
    const bright = new Uint8ClampedArray(100 * 100 * 4).fill(240);
    const dark = new Uint8ClampedArray(100 * 100 * 4);
    for (let i = 0; i < dark.length; i += 4) dark[i] = 20, dark[i + 1] = 20, dark[i + 2] = 30, dark[i + 3] = 255;
    const pb = extractPalette(bright, 100, 100, 4, 2);
    const pd = extractPalette(dark, 100, 100, 4, 2);
    expect(classifyTone(pb.meanLuminance)).toBe("light");
    expect(classifyTone(pd.meanLuminance)).toBe("dark");
    expect(pd.elapsedMs).toBeGreaterThanOrEqual(0);
  });

  it("和谐判定：错配给三要素人话、中调皆可", () => {
    const mismatch = judgeWallpaperHarmony(0.8, true);
    expect(mismatch.harmonious).toBe(false);
    expect(mismatch.message).toContain("建议");
    const ok = judgeWallpaperHarmony(0.8, false);
    expect(ok.harmonious).toBe(true);
    const mid = judgeWallpaperHarmony(0.45, true);
    expect(mid.tone).toBe("mid");
  });

  it("img→tokens：彩色壁纸派生 9 键底稿；全灰壁纸诚实拒绝造强调色", () => {
    const colorful: Parameters<typeof deriveTokensFromPalette>[0] = {
      swatches: [{ hex: "#3050c0", population: 0.8, luminance: 0.15, hsl: rgbToHsl(hexToRgb("#3050c0")!) }],
      meanLuminance: 0.15, contrastSpan: 3, elapsedMs: 1,
    };
    const d = deriveTokensFromPalette(colorful, true)!;
    expect(Object.keys(d.colors)).toHaveLength(9);
    expect(d.untouched).toBe(15); // 24 - 9
    expect(d.colors["--p-fg-primary"]).not.toBe(d.colors["--p-bg-canvas"]);
    const gray: typeof colorful = { swatches: [{ hex: "#808080", population: 1, luminance: 0.5, hsl: { h: 0, s: 0.02, l: 0.5 } }], meanLuminance: 0.5, contrastSpan: 1, elapsedMs: 1 };
    expect(deriveTokensFromPalette(gray, true)).toBeNull();
  });

  it("表面族亮度锚点：深色 <0.2、浅色 >0.85（可读性底线）", () => {
    const dk = surfaceFamilyFrom({ h: 0.6, s: 0.5 }, true);
    const lt = surfaceFamilyFrom({ h: 0.6, s: 0.5 }, false);
    expect(luminance(dk.canvas)).toBeLessThan(0.2);
    expect(luminance(lt.canvas)).toBeGreaterThan(0.85);
  });

  it("色彩命名：灰判 gray、色相/明度分段可读", () => {
    const gray = nameColor("#808080")!;
    expect(gray.gray).toBe(true);
    expect(gray.label).toContain("灰");
    const red = nameColor("#c03030")!;
    expect(red.gray).toBe(false);
    expect(red.label).toContain("红");
    expect(nameColor("bad")!).toBeNull();
  });
});

// ---------- icon-atlas ----------

describe("icon-atlas · 图集装箱（F155 深化）", () => {
  const entries = Array.from({ length: 30 }, (_, i) => ({ id: `i${i}`, w: 64, h: 64 }));

  it("全部入箱（正常尺寸不 overflow）且页尺寸契约成立", () => {
    const plan = packAtlas(entries, DEFAULT_ATLAS_OPTIONS);
    expect(plan.overflow).toHaveLength(0);
    expect(plan.stats.entries).toBe(30);
    for (const p of plan.pages) {
      expect(p.width).toBe(1024);
      for (const pl of p.placements) {
        expect(pl.x + pl.w).toBeLessThanOrEqual(1024);
        expect(pl.y + pl.h).toBeLessThanOrEqual(1024);
      }
    }
    expect(plan.stats.meanOccupancy).toBeGreaterThan(0);
  });

  it("超大条目走缩档；超过 maxDownscale 才 overflow（显性化不静默）", () => {
    const big = [{ id: "huge", w: 2000, h: 2000 }];
    expect(packAtlas(big).overflow).toEqual(["huge"]); // 缩到 0.85 仍 < 0.75 底线？0.51<0.75 → 拒。
    const ok = [{ id: "medium", w: 1200, h: 1200 }];
    const plan = packAtlas(ok, { ...DEFAULT_ATLAS_OPTIONS, maxDownscale: 0.8 }); // 1020/1200=0.85 ≥ 0.8 → 缩档入箱。
    expect(plan.overflow).toHaveLength(0);
    expect(plan.pages[0]!.placements[0]!.scale).toBeLessThan(1);
  });

  it("非法参数显性抛错（门禁不猜）", () => {
    expect(() => packAtlas(entries, { pageWidth: 0 })).toThrow();
    expect(() => packAtlas(entries, { maxDownscale: 1.2 })).toThrow();
  });

  it("命中测试 O(1) 正确：命中与未命中", () => {
    const plan = packAtlas(entries.slice(0, 4));
    const first = plan.pages[0]!.placements[0]!;
    expect(hitTest(plan.pages[0]!, first.x + 1, first.y + 1)?.id).toBe(first.id);
    expect(hitTest(plan.pages[0]!, -5, -5)).toBeNull();
  });

  it("尺寸阶梯选档与 mip 链（F156 双倍率同源纪律）", () => {
    expect(pickIconSize(16, 1)).toBe(16);
    expect(pickIconSize(16, 2)).toBe(32);
    expect(pickIconSize(100, 1)).toBe(128);
    expect(pickIconSize(1000, 1)).toBe(ICON_SIZES[ICON_SIZES.length - 1]);
    expect(mipChain(64)).toEqual([64, 48, 32, 24, 20, 16]);
  });

  it("缓存键前缀失效契约：单图标前缀 ⊂ 整包前缀", () => {
    const k = cacheKey({ packId: "p1", iconId: "i1", size: 48, dpr: 2 });
    expect(k).toBe("vxicon:p1:i1:48@2x");
    expect(k.startsWith(invalidationPrefix("p1", "i1"))).toBe(true);
    expect(k.startsWith(invalidationPrefix("p1"))).toBe(true);
    expect(k.startsWith(invalidationPrefix())).toBe(true);
  });

  it("换包预算拆账：<2s 达标；重排多时超预算给出分批信号", () => {
    const fast = planSwapBudget({ icons: 100, cached: 100, reatlas: 0 });
    expect(fast.withinBudget).toBe(true);
    expect(fast.totalMs).toBe(fast.cacheHitMs + fast.reatlasMs + fast.commitMs);
    const slow = planSwapBudget({ icons: 1000, cached: 0, reatlas: 1000 });
    expect(slow.withinBudget).toBe(false);
  });

  it("回退登记：页引用随计划生成（随时可退的图集面）", () => {
    const plan = packAtlas(entries);
    const rb = makeRollbackEntry("p1", plan, 12345);
    expect(rb.pageRefs).toHaveLength(plan.pages.length);
    expect(rb.swappedAt).toBe(12345);
  });
});

// ---------- cursor-physics ----------

describe("cursor-physics · 指针物理（F156 深化）", () => {
  it("SmoothDamp 收敛到目标且无过冲（临界阻尼）", () => {
    let pos = { x: 0, y: 0 };
    let vel = { x: 0, y: 0 };
    const target = { x: 100, y: 0 };
    let overshoot = false;
    for (let i = 0; i < 600; i++) {
      const r = smoothDamp(pos, target, vel, 0.08, 1 / 60);
      pos = r.pos;
      vel = r.velocity;
      if (pos.x > 100.001) overshoot = true;
    }
    expect(overshoot).toBe(false);
    expect(Math.abs(pos.x - 100)).toBeLessThan(0.5);
  });

  it("大 dt 步长稳定（休眠唤醒第一帧不炸）", () => {
    const r = smoothDamp({ x: 0, y: 0 }, { x: 50, y: 50 }, { x: 0, y: 0 }, 0.08, 5);
    expect(Number.isFinite(r.pos.x)).toBe(true);
    expect(Number.isFinite(r.velocity.y)).toBe(true);
  });

  it("trailSettled 收敛判定（物理停机线——静止零更新）", () => {
    expect(trailSettled({ x: 10, y: 10 }, { x: 10, y: 10 })).toBe(true);
    expect(trailSettled({ x: 10, y: 10 }, { x: 11, y: 10 })).toBe(false);
  });

  it("TrailBuffer 窗口淘汰与容量上限（内存恒定）", () => {
    const b = new TrailBuffer(100, 3);
    b.push(0, 0, 0); b.push(1, 0, 50); b.push(2, 0, 90); b.push(3, 0, 120);
    expect(b.size).toBe(3); // 容量封顶，最老的被挤掉。
    const s = b.sample(250);
    expect(s.every((p) => p.age <= 100)).toBe(true);
    b.clear();
    expect(b.size).toBe(0);
  });

  it("点击状态机：抖动容忍 → 拖拽 → 移出取消 → 不误触 click", () => {
    const m = new ClickMachine();
    m.feed({ type: "press", x: 0, y: 0, t: 0 });
    expect(m.feed({ type: "move", x: 2, y: 0, t: 10 })).toBeNull(); // <4px 抖动。
    expect(m.feed({ type: "move", x: 20, y: 0, t: 20 })).toBe("drag-start");
    expect(m.feed({ type: "release", x: 30, y: 0, t: 30 })).toBe("drag-end");
    m.feed({ type: "press", x: 0, y: 0, t: 100 });
    expect(m.feed({ type: "leave" })).toBe("cancel");
    expect(m.feed({ type: "release", x: 0, y: 0, t: 110 })).toBeNull(); // 取消后松开无 click。
    expect(m.current).toBe("cancelled");
  });

  it("长按一次有效：progress 到 1 → fireLongPress 恰好一次；连点去抖", () => {
    const m = new ClickMachine();
    m.feed({ type: "press", x: 0, y: 0, t: 0 });
    expect(m.progress(600)).toBe(1);
    expect(m.fireLongPress()).toBe("long-press");
    expect(m.fireLongPress()).toBeNull();
    expect(m.feed({ type: "release", x: 0, y: 0, t: 700 })).toBeNull(); // 长按后松开不给 click。
    m.feed({ type: "press", x: 0, y: 0, t: 800 });
    expect(m.feed({ type: "press", x: 0, y: 0, t: 810 })).toBeNull(); // 非 idle 的 press 忽略。
  });

  it("热点变换：半上取整 + 裁剪平移（1px 级对拍口径）", () => {
    expect(transformHotspot({ x: 7, y: 4 }, 1.5)).toEqual({ x: 11, y: 6 });
    expect(transformHotspot({ x: 3, y: 3 }, 0.5)).toEqual({ x: 2, y: 2 });
    expect(() => transformHotspot({ x: 1, y: 1 }, 0)).toThrow();
    expect(transformHotspotCrop({ x: 10, y: 6 }, { x: 4, y: 2 })).toEqual({ x: 6, y: 4 });
    expect(pickPointerSpriteTier(20, 2)).toBe(2);
    expect(pickPointerSpriteTier(20, 1)).toBe(1);
  });

  it("动画指针播放头：按 jif 推进、跳帧不追帧（≤3）", () => {
    const clock = new AnimCursorClock([{ jif: 6 }, { jif: 6 }, { jif: 6 }]); // 100ms/帧。
    expect(clock.current).toBe(0);
    clock.tick(110);
    expect(clock.current).toBe(1);
    clock.tick(500); // 卡顿——最多跳 3 帧（封顶后归零相位）。
    expect(clock.current).toBeLessThan(3);
    clock.reset();
    expect(clock.current).toBe(0);
  });

  it("DPR 物理像素吸附（跨屏拖动不糊）", () => {
    expect(snapToPhysicalPixel(13.37, 7.1, 1.25).x).toBeCloseTo(13.6, 5); // 0.8 网格：round(16.71)=17 → 13.6。
    expect(() => snapToPhysicalPixel(0, 0, 0)).toThrow();
  });
});

// ---------- sound-synthesis ----------

describe("sound-synthesis · PCM 合成（F157 深化）", () => {
  it("振荡器：sine 峰值 1、square 双值、triangle 连续、noise 有界", () => {
    expect(oscillatorSample("sine", 0.25)).toBeCloseTo(1, 5);
    expect(oscillatorSample("square", 0.1)).toBe(1);
    expect(oscillatorSample("square", 0.9)).toBe(-1);
    expect(oscillatorSample("triangle", 0.25)).toBeCloseTo(1, 5);
    const n = oscillatorSample("noise", 0.7);
    expect(Math.abs(n)).toBeLessThanOrEqual(1);
  });

  it("ADSR：attack 上升、sustain 保持、release 归零", () => {
    const env = ENVELOPE_PRESETS.notify!;
    expect(envelopeAt(env, 0, 100)).toBeLessThanOrEqual(0.2);
    expect(envelopeAt(env, env.attackMs + env.decayMs + 50, 100)).toBeCloseTo(env.sustain, 5);
    expect(envelopeAt(env, env.attackMs + env.decayMs + 100 + env.releaseMs + 10, 100)).toBe(0);
    expect(envelopeTotalMs(env, 100)).toBe(env.attackMs + env.decayMs + 100 + env.releaseMs);
  });

  it("renderSynth：长度=最长层包络、soft clip 后无硬削波爆点", () => {
    const spec = defaultSynthSpec("chime")!;
    const r = renderSynth(spec, 80);
    expect(r.left.length).toBe(r.right.length);
    expect(r.peak).toBeLessThanOrEqual(1.0001);
    expect(r.clippedSamples).toBe(0); // soft clip 前置——正常谱面不硬削。
  });

  it("响度归一：目标 RMS 收敛且峰留余量", () => {
    const spec = defaultSynthSpec("error")!;
    const r = renderSynth(spec, 60);
    const g = normalizeGain(r, 0.18);
    expect(g).toBeGreaterThan(0);
    expect(g * r.peak).toBeLessThanOrEqual(0.9801);
  });

  it("等功率淡化：首尾样本趋零（防爆音 ramp）", () => {
    const s = new Float32Array(4410).fill(0.8);
    applyEqualPowerFade(s, 30, 40, 44100);
    expect(Math.abs(s[0]!)).toBeLessThan(0.01);
    expect(Math.abs(s[s.length - 1]!)).toBeLessThan(0.01);
  });

  it("线性重采样：同率恒等、比率换算长度正确", () => {
    const src = new Float32Array([0, 1, 0, -1]);
    expect(resampleLinear(src, 48000, 48000)).not.toBe(src); // 拷贝语义。
    expect(resampleLinear(src, 48000, 48000)[0]).toBe(0);
    const half = resampleLinear(src, 48000, 24000);
    expect(half.length).toBe(2);
    expect(() => resampleLinear(src, 0, 48000)).toThrow();
  });

  it("WAV 编码：RIFF 头契约 + 数据长度正确", () => {
    const spec = defaultSynthSpec("click")!;
    const r = renderSynth(spec, 20);
    const wav = encodeWav16(r, 1);
    const v = new DataView(wav);
    const magic = (off: number, n: number): string => String.fromCharCode(...new Uint8Array(wav, off, n));
    expect(magic(0, 4)).toBe("RIFF");
    expect(magic(8, 4)).toBe("WAVE");
    expect(v.getUint16(22, true)).toBe(2); // stereo
    expect(v.getUint32(24, true)).toBe(r.sampleRate);
    expect(wav.byteLength).toBe(44 + r.left.length * 4);
  });

  it("六事件全有谱、未知事件显性拒绝；端到端产物可写", () => {
    for (const id of ["notify", "chime", "error", "click", "plug", "unplug"]) {
      expect(defaultSynthSpec(id)).not.toBeNull();
      const r = synthesizeEvent(id, 60);
      expect(r).not.toBeNull();
      expect(r!.wav.byteLength).toBeGreaterThan(44);
    }
    expect(defaultSynthSpec("bogus")).toBeNull();
    expect(synthesizeEvent("bogus")).toBeNull();
  });
});
