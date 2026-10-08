import { beforeEach, describe, expect, it } from "vitest";
import {
  POINTER_ROLES, POINTER_ROLE_COUNT, defaultPointerScheme, loadPointerScheme,
  savePointerScheme, spritePlan, clampFps, clampFrames, validateScheme, SIZE_LIMIT_PX, ANIM_MAX_FRAMES,
} from "../pointer";
import {
  SOUND_EVENTS, SOUND_EVENT_COUNT, defaultSoundMixerConfig, loadSoundMixerConfig,
  saveSoundMixerConfig, clampSlider, effectiveVolume, SLIDER_STEP,
} from "../soundmix";
import { personaStore } from "../store";

beforeEach(() => {
  personaStore.reset();
});

describe("F156 指针编辑器", () => {
  it("15 枚标准指针（乙语义全保留）", () => {
    expect(POINTER_ROLES).toHaveLength(POINTER_ROLE_COUNT);
    expect(POINTER_ROLE_COUNT).toBe(15);
    expect(POINTER_ROLES.filter((r) => r.id === "busy")[0]?.animated).toBe(true);
  });

  it("热点缺省 → 黄条提醒（validateScheme warning）；导出按 (0,0)", () => {
    const s = defaultPointerScheme();
    s.roles["arrow"]!.frames = ["data:image/svg+xml;base64,x"];
    const issues = validateScheme(s);
    expect(issues.some((i) => i.role === "arrow" && i.kind === "hotspot-unset" && i.level === "warning")).toBe(true);
    const plan = spritePlan(s.roles["arrow"]!, 1);
    expect(plan.hotspot1x).toEqual({ x: 0, y: 0 });
  });

  it("双倍率 sprite：热点按倍率精确放大（1px 级对拍）", () => {
    const s = defaultPointerScheme();
    const a = s.roles["hand"]!;
    a.size = 32;
    a.hotspot = { x: 6, y: 4 };
    const plan = spritePlan(a, 1.25);
    expect(plan.size1x).toBe(40);
    expect(plan.hotspot1x).toEqual({ x: 8, y: 5 });
    expect(plan.size2x).toBe(80);
    expect(plan.hotspot2x).toEqual({ x: 15, y: 10 });
  });

  it("尺寸 >128px 警告；帧数超 16 报错；fps>60 降采样", () => {
    const s = defaultPointerScheme();
    const a = s.roles["arrow"]!;
    a.frames = ["f"];
    a.hotspot = { x: 0, y: 0 };
    savePointerScheme({ ...s, scale: 2, roles: { ...s.roles, arrow: { ...a, size: 70 } } });
    const issues = validateScheme(loadPointerScheme());
    expect(issues.some((i) => i.kind === "size-overflow")).toBe(true);
    expect(SIZE_LIMIT_PX).toBe(128);
    expect(clampFrames(["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r"])).toHaveLength(ANIM_MAX_FRAMES);
    expect(clampFps(120)).toBe(60);
    expect(clampFps(0)).toBe(1);
  });

  it("缩放 0.5x-2x 钳制（load 时收敛）", () => {
    savePointerScheme({ ...defaultPointerScheme(), scale: 9 });
    expect(loadPointerScheme().scale).toBe(2);
    savePointerScheme({ ...defaultPointerScheme(), scale: 0.1 });
    expect(loadPointerScheme().scale).toBe(0.5);
  });
});

describe("F157 声音混合器", () => {
  it("六事件六档位独立生效（互不串扰）", () => {
    expect(SOUND_EVENTS).toHaveLength(SOUND_EVENT_COUNT);
    const cfg = defaultSoundMixerConfig();
    cfg.events["notify"] = { schemeId: null, slider: 0.4 };
    cfg.events["boot"] = { schemeId: null, slider: 1 };
    expect(effectiveVolume(cfg, "notify")).toBeCloseTo(0.16);
    expect(effectiveVolume(cfg, "boot")).toBe(1);
    expect(effectiveVolume(cfg, "error")).toBe(1); // 未配置 → 默认 1
  });

  it("总闸 100% 压制（含试听语义一致）", () => {
    const cfg = defaultSoundMixerConfig();
    cfg.masterMute = true;
    for (const e of SOUND_EVENTS) expect(effectiveVolume(cfg, e.id)).toBe(0);
  });

  it("2% 步进钳制", () => {
    expect(SLIDER_STEP).toBe(0.02);
    expect(clampSlider(0.513)).toBeCloseTo(0.52);
    expect(clampSlider(-2)).toBe(0);
    expect(clampSlider(2)).toBe(1);
  });

  it("持久化 round-trip", () => {
    const cfg = defaultSoundMixerConfig();
    cfg.events["error"] = { schemeId: "scheme-a", slider: 0.6 };
    saveSoundMixerConfig(cfg);
    expect(loadSoundMixerConfig().events["error"]?.slider).toBe(0.6);
  });
});
