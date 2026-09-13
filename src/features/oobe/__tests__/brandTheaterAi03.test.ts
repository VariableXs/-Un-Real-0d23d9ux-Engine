// UNREAL-X AI-03：brandTheaterDeep 逻辑核测试（X00501~X00700 V 线），勿删。
import { describe, expect, it } from 'vitest';
import {
  BOOT_NARRATION,
  KELVIN_TOKENS,
  MARK_TIERS,
  SOUNDSCAPES,
  applyDnd,
  captionEnabled,
  circadianKelvin,
  clampMarkTier,
  contrastOk,
  degradeMarkTier,
  glyphDelay,
  kelvinToken,
  markMotionParams,
  moodFgDark,
  moodForbiddenInHc,
  moodTriad,
  narrationAt,
  pulseOffset,
  ringPct,
  soundscapeToken,
  transitionIn,
  transitionOut,
  transitionReduce,
  urgencyOf,
  urgencyToken,
  wordmarkTotal,
} from '../brandTheaterDeep';

describe('族0021 动态标识（X00501~X00525）', () => {
  it('五档档位表完整且非法档回默认', () => {
    expect(MARK_TIERS).toHaveLength(5);
    expect(clampMarkTier('egg')).toBe('egg');
    expect(clampMarkTier('nope')).toBe('breath');
  });
  it('帧数 8~40 且 30fps 预算内', () => {
    const frames = MARK_TIERS.map((t) => markMotionParams(t).frames);
    expect(frames).toEqual([8, 16, 24, 32, 40]);
    expect(markMotionParams('egg').durationMs).toBe(40 * 33);
  });
  it('降级链：低配/省电/中配', () => {
    expect(degradeMarkTier('theater', 2, false)).toBe('static');
    expect(degradeMarkTier('egg', 16, true)).toBe('static');
    expect(degradeMarkTier('theater', 4, false)).toBe('rhythm');
    expect(degradeMarkTier('theater', 16, false)).toBe('theater');
  });
});

describe('族0022 声景 2.0（X00526~X00550）', () => {
  it('三声景总时长 1000/1000/100ms', () => {
    expect(soundscapeToken(SOUNDSCAPES.boot!)).toBe('1000ms');
    expect(soundscapeToken(SOUNDSCAPES.shutdown!)).toBe('1000ms');
    expect(soundscapeToken(SOUNDSCAPES.hint!)).toBe('100ms');
  });
  it('勿扰降级静音并钳首尾段', () => {
    const dnd = applyDnd(SOUNDSCAPES.boot!);
    expect(dnd.muted).toBe(true);
    expect(dnd.attackMs).toBe(50);
    expect(dnd.releaseMs).toBe(50);
  });
});

describe('族0023 色温曲线（X00551~X00575）', () => {
  it('正午 6500K、午夜 2700K、对称回绕', () => {
    expect(circadianKelvin(720)).toBe(6500);
    expect(circadianKelvin(0)).toBe(2700);
    expect(circadianKelvin(1440)).toBe(2700);
    expect(circadianKelvin(1441)).toBe(circadianKelvin(1));
  });
  it('色温映射到档位令牌，禁裸值', () => {
    expect(kelvinToken(6500)).toBe(KELVIN_TOKENS[6500]);
    expect(kelvinToken(6300)).toBe(KELVIN_TOKENS[6500]);
    expect(kelvinToken(1000)).toBe(KELVIN_TOKENS[2700]);
    expect(Object.values(KELVIN_TOKENS).every((t) => t.startsWith('var('))).toBe(true);
  });
});

describe('族0024 字标动势（X00576~X00600）', () => {
  it('逐字 stagger 与总时长', () => {
    expect(glyphDelay(0, 40)).toBe(0);
    expect(wordmarkTotal(5, 40, 200)).toBe(360);
    expect(wordmarkTotal(0, 40, 200)).toBe(0);
    expect(wordmarkTotal(5, 40, 200, true)).toBeLessThan(360);
  });
  it('脉动相位对称、静态恒零', () => {
    expect(pulseOffset(0, 'rhythm')).toBe(0);
    expect(pulseOffset(2, 'rhythm')).toBe(100);
    expect(pulseOffset(6, 'rhythm')).toBe(-100);
    expect(pulseOffset(8, 'rhythm')).toBe(0);
    expect(pulseOffset(2, 'static')).toBe(0);
  });
});

describe('族0025 倒计时美学（X00601~X00625）', () => {
  it('三态紧迫度与语义令牌', () => {
    expect(urgencyOf(9)).toBe('danger');
    expect(urgencyOf(59)).toBe('warn');
    expect(urgencyOf(60)).toBe('normal');
    expect(urgencyToken('danger')).toBe('var(--danger)');
    expect(urgencyToken('normal')).toBe('var(--text-primary)');
  });
  it('进度环百分比钳制', () => {
    expect(ringPct(50, 100)).toBe(50);
    expect(ringPct(200, 100)).toBe(100);
    expect(ringPct(10, 0)).toBe(0);
    expect(ringPct(NaN, 100)).toBe(0);
  });
});

describe('族0026 转场语法（X00626~X00650）', () => {
  it('三档规格互异、退出反向更快', () => {
    expect(transitionIn('fade').durMs).toBe(170);
    expect(transitionIn('slide').shiftPx).toBe(8);
    expect(transitionIn('reveal').durMs).toBe(360);
    expect(transitionOut('slide').shiftPx).toBe(-8);
    expect(transitionOut('fade').durMs).toBe(136);
  });
  it('reduce-motion 全量降级为 80ms 纯淡入淡出', () => {
    const r = transitionReduce();
    expect(r.durMs).toBe(80);
    expect(r.scaleFrom).toBe(1);
    expect(r.shiftPx).toBe(0);
  });
});

describe('族0027 情绪板 2.0（X00651~X00675）', () => {
  it('三色组生成并钳制 L/C', () => {
    const m = moodTriad(262);
    expect(m.accent).toContain('oklch(0.680');
    expect(m.neighbor).toContain('0.700');
    expect(m.complement).toContain('0.560');
    expect(m.complement).toContain('0.070');
  });
  it('色相回绕与对比门禁', () => {
    expect(moodTriad(-40).accent).toBe(moodTriad(320).accent);
    expect(moodFgDark(0.68)).toBe(true);
    expect(moodFgDark(0.4)).toBe(false);
    expect(moodForbiddenInHc()).toBe(true);
  });
});

describe('族0028 启动无障碍 2.0（X00676~X00700）', () => {
  it('五步读屏文案，越界静默，长度克制', () => {
    expect(BOOT_NARRATION).toHaveLength(5);
    expect(narrationAt(0)).toContain('启动');
    expect(narrationAt(1)).toContain('Esc');
    expect(narrationAt(99)).toBe('');
    expect(BOOT_NARRATION.every((n) => n.length <= 20)).toBe(true);
  });
  it('字幕开关与 AA 对比门禁', () => {
    expect(captionEnabled(undefined, true)).toBe(true);
    expect(captionEnabled(false, true)).toBe(false);
    expect(contrastOk(0.72, 0.14)).toBe(true);
    expect(contrastOk(0.5, 0.4)).toBe(false);
  });
});
