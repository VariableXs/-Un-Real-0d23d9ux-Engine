import { beforeEach, describe, expect, it } from "vitest";
import {
  playheadAt, cycleMs, renderPlan, renderPlansForScheme, serializeVxPointer,
  parseVxPointer, evaluateCurAniCompatibility, isAniContainer, isCurFile,
} from "../pointer-engine";
import {
  SYNTH_SPECS, resolveEventSound, volumeRamp, rampAt, PreviewChannel, RAMP_MS,
} from "../audio-engine";
import {
  startMenuGeometry, presetThumbSpec, taskbarGeometry, autoHideNext,
  HIDE_AFTER_MS, REARRANGE_BUDGET_MS,
} from "../layout-engine";
import {
  parseFontNames, buildFallbackChain, BatchFontScanner, scanFontSync, BATCH_SIZE,
} from "../font-engine";
import {
  FrameTimeSampler, TierAdvisor, animationFallback, progressNumberSpec,
  progressNumberText, previewHonestyNote, SUGGEST_COOLDOWN_MS,
} from "../motion-monitor";
import { defaultSoundMixerConfig } from "../soundmix";
import { defaultStartLayout, officialPresets } from "../startpresets";
import { defaultTaskbarPrefs } from "../taskbarprefs";
import { defaultPointerScheme } from "../pointer";
import { generalCharset } from "../fontguard";
import { personaStore } from "../store";

beforeEach(() => {
  personaStore.reset();
});

describe("pointer-engine（F156 深化）", () => {
  it("播放头：fps 驱动帧序 + 循环计数 + 超 60fps 钳制", () => {
    const p = playheadAt(10, 4, 250); // 100ms/帧 → 2.5 帧
    expect(p.frameIndex).toBe(2);
    expect(p.frameElapsedMs).toBeCloseTo(50);
    expect(p.loop).toBe(0);
    expect(playheadAt(10, 4, 1000).loop).toBe(2); // 10 帧 = 2.5 循环
    expect(cycleMs(120, 4)).toBe(cycleMs(60, 4)); // 超 60 钳制
  });

  it("渲染规划：DPR=2 选 2x sprite；热点物理坐标精确", () => {
    const scheme = defaultPointerScheme();
    const arrow = scheme.roles["arrow"]!;
    arrow.size = 24;
    arrow.hotspot = { x: 3, y: 2 };
    const plan = renderPlan(arrow, 1, 2);
    expect(plan.cssSize).toBe(24);
    expect(plan.physicalSize).toBe(48);
    expect(plan.spriteScale).toBe(2);
    expect(plan.hotspotPhysical).toEqual({ x: 6, y: 4 });
  });

  it("全方案规划：空帧角色不出现在计划里", () => {
    const scheme = defaultPointerScheme();
    const plans = renderPlansForScheme(scheme, 1);
    expect(Object.keys(plans)).toHaveLength(0); // 默认方案未导入帧
  });

  it("vxpointer 序列化→解析 round-trip（含校验和）；篡改被检出", () => {
    const scheme = defaultPointerScheme();
    const file = serializeVxPointer(scheme);
    const r = parseVxPointer(JSON.parse(JSON.stringify(file)));
    expect(r.ok).toBe(true);
    const tampered = JSON.parse(JSON.stringify(file)) as typeof file;
    tampered.name = "篡改";
    const r2 = parseVxPointer(tampered);
    expect(r2.ok).toBe(false);
    expect(r2.issues.some((i) => i.includes("校验不匹配"))).toBe(true);
  });

  it(".cur/.ani 互通评估与魔数嗅探", () => {
    expect(evaluateCurAniCompatibility(".cur").supported).toBe(true);
    const ani = evaluateCurAniCompatibility(".ani");
    expect(ani.limitations.some((l) => l.includes("16 帧"))).toBe(true);
    const buf = new ArrayBuffer(12);
    const v = new DataView(buf);
    v.setUint32(0, 0x52494646);
    v.setUint32(8, 0x41434f4e);
    expect(isAniContainer(buf)).toBe(true);
    const cur = new ArrayBuffer(4);
    new DataView(cur).setUint16(2, 2);
    expect(isCurFile(cur)).toBe(true);
  });
});

describe("audio-engine（F157 深化）", () => {
  it("解析链：总闸/档位折算 + 路径标注", () => {
    const cfg = defaultSoundMixerConfig();
    const r = resolveEventSound(cfg, "notify");
    expect(r?.volume).toBe(1);
    expect(r?.path).toBe("default-synth");
    const muted = resolveEventSound({ ...cfg, masterMute: true }, "notify");
    expect(muted?.volume).toBe(0);
    expect(resolveEventSound(cfg, "bogus")).toBeNull(); // 未知事件拒绝
  });

  it("音量过渡：<2% 不过渡；过渡中点线性", () => {
    expect(volumeRamp(0.5, 0.51)).toBeNull();
    const ramp = volumeRamp(0.2, 0.8);
    expect(ramp?.durationMs).toBe(RAMP_MS);
    expect(rampAt(ramp!, 0.5)).toBeCloseTo(0.5);
  });

  it("六事件合成特征表齐备", () => {
    for (const id of ["boot", "notify", "usb-connect", "usb-eject", "battery-low", "error"]) {
      expect(SYNTH_SPECS[id]).toBeTruthy();
    }
  });

  it("无 AudioContext 环境：preview 返回 null（降级路径诚实）", () => {
    const ch = new PreviewChannel();
    const cfg = defaultSoundMixerConfig();
    const r = resolveEventSound(cfg, "notify")!;
    expect(ch.preview(r)).toBeNull();
    expect(PreviewChannel.silentByMasterGate({ ...cfg, masterMute: true }, "notify")).toBe(true);
  });
});

describe("layout-engine（F158/F168 深化）", () => {
  it("开始菜单几何：长辈档 tile 60 达标；简洁档区开关映射", () => {
    const elder = startMenuGeometry({ ...defaultStartLayout(), largeIcons: true }, 1920, 1080);
    expect(elder.tilePx).toBe(60);
    expect(elder.elderOk).toBe(true);
    const simple = officialPresets()[1]!;
    const thumb = presetThumbSpec(simple, 1920, 1080);
    expect(thumb.geometry.showRecent).toBe(false);
    expect(thumb.geometry.showRecommended).toBe(false);
  });

  it("全屏几何按屏比率；标准固定 560 宽", () => {
    const fs = startMenuGeometry({ ...defaultStartLayout(), fullscreen: true }, 1920, 1080);
    expect(fs.width).toBe(Math.round(1920 * 0.62));
    expect(fs.height).toBe(Math.round(1080 * 0.86));
    expect(startMenuGeometry(defaultStartLayout(), 1920, 1080).width).toBe(560);
  });

  it("任务栏几何：左对齐锚点 vs 居中；溢出折叠（pinned 豁免）；托盘联动", () => {
    const prefs = defaultTaskbarPrefs();
    const items = Array.from({ length: 12 }, (_, i) => ({ id: `i${i}`, pinned: i < 2 }));
    const left = taskbarGeometry({ ...prefs, align: "left" }, items, 1920, 0);
    expect(left.iconsOriginX).toBe(48); // 紧随开始钮
    const center = taskbarGeometry({ ...prefs, align: "center" }, items, 1920, 0);
    expect(center.iconsOriginX).toBeGreaterThan(left.iconsOriginX);
    // 溢出折叠：窄屏（500px）容量 = floor((500-48-192)/30) = 8 → 4 项折叠，pinned 豁免。
    const narrow = taskbarGeometry(prefs, items, 500, 0);
    expect(narrow.foldedIds.length).toBe(4);
    expect(narrow.foldedIds.every((id) => Number(id.slice(1)) >= 8)).toBe(true);
    // 大图标档高度 56 + 托盘折叠收缩。
    const large = taskbarGeometry({ ...prefs, iconSize: "large" }, items.slice(0, 4), 1920, 10);
    expect(large.heightPx).toBe(56);
    expect(large.trayOriginX).toBeGreaterThan(1920 - 192); // 折叠态托盘变窄
    expect(REARRANGE_BUDGET_MS).toBe(200);
  });

  it("自动隐藏状态机：热区唤出/离开延迟隐藏/全屏强制显示/焦点钉住", () => {
    const base = { cursorYFromBottom: 100, dwellMs: 0, focusPinned: false, fullscreenActive: false, autoHidePref: true, sinceTransitionMs: HIDE_AFTER_MS };
    expect(autoHideNext("shown", base)).toBe("hiding");
    expect(autoHideNext("shown", { ...base, cursorYFromBottom: 10 })).toBe("shown");
    expect(autoHideNext("hiding", base)).toBe("hidden");
    expect(autoHideNext("hiding", { ...base, cursorYFromBottom: 5 })).toBe("shown");
    expect(autoHideNext("hidden", { ...base, cursorYFromBottom: 3, dwellMs: 250 })).toBe("revealing");
    expect(autoHideNext("hidden", { ...base, cursorYFromBottom: 3, dwellMs: 100 })).toBe("hidden");
    expect(autoHideNext("shown", { ...base, fullscreenActive: true })).toBe("shown");
    expect(autoHideNext("hidden", { ...base, cursorYFromBottom: 3, dwellMs: 250, focusPinned: true })).toBe("pinned-by-focus");
  });
});

describe("font-engine（F159 深化）", () => {
  it("name 表解析：构造最小 sfnt 提取族名", () => {
    // name 表: format/count/stringOffset + 1 记录（platform3 lang0x0804 nameID1）+ 字符串。
    const text = "测试字体";
    const strBytes = new Uint8Array(text.length * 2);
    for (let i = 0; i < text.length; i++) {
      const c = text.codePointAt(i) ?? 0;
      strBytes[i * 2] = c >> 8;
      strBytes[i * 2 + 1] = c & 0xff;
    }
    const headerSize = 6 + 12;
    const total = 12 + 16 + headerSize + strBytes.length;
    const buf = new ArrayBuffer(total);
    const v = new DataView(buf);
    v.setUint16(4, 1); // numTables
    const enc = new TextEncoder();
    enc.encodeInto("name", new Uint8Array(buf, 12, 4));
    v.setUint32(20, 28); // name 表偏移
    const n = 28;
    v.setUint16(n, 0); // format
    v.setUint16(n + 2, 1); // count
    v.setUint16(n + 4, 6 + 12); // stringOffset（相对 name 表）
    const rec = n + 6;
    v.setUint16(rec, 3); // platform
    v.setUint16(rec + 2, 1);
    v.setUint16(rec + 4, 0x0804); // zh-CN
    v.setUint16(rec + 6, 1); // nameID family
    v.setUint16(rec + 8, strBytes.length);
    v.setUint16(rec + 10, 0); // string 相对偏移
    new Uint8Array(buf, n + 6 + 12 + 0, strBytes.length).set(strBytes);
    const names = parseFontNames(buf);
    expect(names?.family).toBe(text);
  });

  it("回退链：用户字体 → 中文栈 → 西文栈 → 兜底（永不落空）", () => {
    const chain = buildFallbackChain("艺术体", { cjk: "雅黑", latin: "Segoe", fallback: "System" });
    expect(chain.stack).toEqual(["艺术体", "雅黑", "Segoe", "System"]);
    expect(chain.notes).toHaveLength(4);
    const same = buildFallbackChain("雅黑", { cjk: "雅黑", latin: "雅黑", fallback: "System" });
    expect(new Set(same.stack).size).toBe(same.stack.length); // 去重
  });

  it("批量扫描：部分结论先行 + 完成后与同步口径一致", () => {
    const all = generalCharset();
    const covered = new Set<number>();
    for (let i = 0; i < all.length * 0.9; i++) covered.add((all[i] as string).codePointAt(0) ?? 0);
    const input = { fontId: "f", covered, interfaceChars: all };
    const scanner = new BatchFontScanner(input);
    let steps = 0;
    while (scanner.step()) steps++;
    expect(steps).toBe(Math.ceil(all.length / BATCH_SIZE) - 1); // 最后一步返回 false
    expect(scanner.snapshot.done).toBe(true);
    const final = scanner.finalResult();
    const sync = scanFontSync(input);
    expect(final?.interfaceMissingRate).toBeCloseTo(sync.interfaceMissingRate);
  });
});

describe("motion-monitor（F160 深化）", () => {
  it("帧时采样：P95/P99 与 80/60fps 预算判定", () => {
    const s = new FrameTimeSampler(100);
    for (let i = 0; i < 100; i++) s.record(i < 95 ? 10 : 25);
    const v = s.verdict();
    expect(v.p95).toBe(25);
    expect(v.within80fps).toBe(false);
    expect(v.samples).toBe(100);
    const good = new FrameTimeSampler(100);
    for (let i = 0; i < 100; i++) good.record(10);
    expect(good.verdict().within80fps).toBe(true);
  });

  it("档位建议：掉帧降档/恢复升档/冷却防振荡", () => {
    const s = new FrameTimeSampler(100);
    for (let i = 0; i < 100; i++) s.record(25);
    const advisor = new TierAdvisor(s);
    const drop = advisor.advise("full", SUGGEST_COOLDOWN_MS + 1); // 越过初始冷却
    expect(drop.suggest).toBe("reduced");
    // 冷却期内不改口。
    expect(advisor.advise("reduced", SUGGEST_COOLDOWN_MS + 1000).suggest).toBeNull();
    // 恢复（新采样器 + 冷却已过）。
    const s2 = new FrameTimeSampler(100);
    for (let i = 0; i < 100; i++) s2.record(10);
    const advisor2 = new TierAdvisor(s2);
    const up = advisor2.advise("reduced", SUGGEST_COOLDOWN_MS + 1);
    expect(up.suggest).toBe("full");
    void s;
  });

  it("超预算动画降级：必达动画转进度数字；普通动画降直线", () => {
    const s = new FrameTimeSampler(100);
    for (let i = 0; i < 100; i++) s.record(25);
    expect(animationFallback({ durationMs: 200, missionCritical: true }, s)).toBe("progress-number");
    expect(animationFallback({ durationMs: 200, missionCritical: false }, s)).toBe("linear");
    const good = new FrameTimeSampler(100);
    for (let i = 0; i < 100; i++) good.record(10);
    expect(animationFallback({ durationMs: 200, missionCritical: false }, good)).toBe("keep");
  });

  it("WP-207 进度数字：百分比/剩余秒双格式", () => {
    expect(progressNumberText(progressNumberSpec(0.42))).toBe("42%");
    expect(progressNumberText(progressNumberSpec(0.5, 100))).toBe("剩余约 50 秒");
  });

  it("预演诚实提示：有样本且掉帧才提示", () => {
    const empty = new FrameTimeSampler();
    expect(previewHonestyNote(empty)).toBeNull();
    const bad = new FrameTimeSampler(10);
    for (let i = 0; i < 10; i++) bad.record(30);
    expect(previewHonestyNote(bad)).toContain("性能受限");
  });
});
