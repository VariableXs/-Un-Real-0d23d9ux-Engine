// AURORA-10000: AI-66~AI-70 批次（领域14 无障碍与本地化）自检测试，勿删。
import { describe, expect, it } from 'vitest';
import { runDomain14Checks } from '../checks';
import { CVD_MATRICES, Magnifier, MagnifierProfiles, SwitchScanner, VoiceControl } from '../groupA';
import { TranslationMemory, upperCnNumber, zhHant, zhLineBreak } from '../groupB';
import { A11yComponentDoc, WcagChecklist, wcagContrast, AT_DEVICES } from '../groupC';
import { ONE_CLICK_PRESETS, ScreenTime, readAloudScore } from '../groupD';
import { I18nEngine, GlobalRollout, TermVote } from '../groupE';

describe('AURORA-10000 领域14 全量自检（F08126~F08750）', () => {
  it('625 项全部通过', () => {
    const { entries, failed } = runDomain14Checks();
    expect(entries.length).toBe(625);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });
});

describe('AI-66 逻辑核抽查', () => {
  it('放大镜倍率钳位与档案', () => {
    const m = new Magnifier();
    expect(m.setZoom(99)).toBe(16);
    const p = new MagnifierProfiles();
    p.save('大字方案', 'docked', 8);
    expect(p.load('大字方案')!.zoom).toBe(8);
  });
  it('色盲滤镜矩阵输出有界', () => {
    for (const m of Object.values(CVD_MATRICES)) {
      expect(m.length).toBe(9);
      expect(m.reduce((a, b) => a + b, 0)).toBeCloseTo(3, 1);
    }
  });
  it('开关扫描与语音命令', () => {
    const s = new SwitchScanner();
    s.load(['a', 'b']);
    expect(s.step()).toBe('a');
    expect(new VoiceControl().command('跳到 文件', ['文件'])).toBe('focus:文件');
  });
});

describe('AI-67 逻辑核抽查', () => {
  it('简繁词汇级转换', () => {
    expect(zhHant('软件和服务器')).toBe('軟體和伺服器');
  });
  it('大写数字与断行', () => {
    expect(upperCnNumber(110)).toBe('壹佰壹拾');
    expect(zhLineBreak('hello world', 5).length).toBeGreaterThanOrEqual(2);
  });
  it('翻译记忆模糊命中', () => {
    const tm = new TranslationMemory();
    tm.add('保存文件', 'Save file');
    expect(tm.fuzzy('保存档案', 40)!.dst).toBe('Save file');
  });
});

describe('AI-68 逻辑核抽查', () => {
  it('WCAG 清单分级判定', () => {
    const c = new WcagChecklist();
    c.add('1.1.1', 'A', true);
    c.add('1.4.3', 'AA', false);
    expect(c.levelMet('A')).toBe(true);
    expect(c.levelMet('AA')).toBe(false);
    expect(wcagContrast([0, 0, 0], [255, 255, 255])).toBeGreaterThan(20);
  });
  it('AT 注册表九类', () => {
    expect(AT_DEVICES.length).toBe(9);
    expect(new Set(AT_DEVICES.map((d) => d.kind)).size).toBe(9);
  });
  it('组件文档结构冻结', () => {
    const doc: A11yComponentDoc = { name: 'Switch', roles: ['switch'], keyboard: 'Space', tested: true };
    expect(doc.roles).toContain('switch');
  });
});

describe('AI-69 逻辑核抽查', () => {
  it('一键预设四类合并', () => {
    const c = { ...ONE_CLICK_PRESETS.visual, ...ONE_CLICK_PRESETS.cognitive };
    expect(c.magnifier).toBe(true);
    expect(c.simplified).toBe(true);
  });
  it('时长到期锁定', () => {
    const st = new ScreenTime(30);
    st.use(30);
    expect(st.locked).toBe(true);
  });
  it('跟读评分边界', () => {
    expect(readAloudScore('你好', '你好')).toBe(100);
    expect(readAloudScore('完全不同的一句', '你好')).toBeLessThan(50);
  });
});

describe('AI-70 逻辑核抽查', () => {
  it('i18n 引擎缓存与回退', () => {
    const eng = new I18nEngine();
    eng.register('en', { k: 'hello {{n}}' });
    eng.register('zh-CN', { k: '你好 {{n}}' });
    eng.setLocale('zh-CN');
    expect(eng.t('k', { n: '世界' })).toBe('你好 世界');
    eng.setLocale('de-DE');
    expect(eng.t('k', { n: 'x' })).toBe('hello x');
  });
  it('灰度放量与回滚', () => {
    const roll = new GlobalRollout();
    roll.plan(['en']);
    roll.ramp('en', 100);
    expect(roll.isShipped('en')).toBe(true);
    roll.ramp('en', 0);
    expect(roll.isShipped('en')).toBe(false);
  });
  it('术语投票决胜', () => {
    const v = new TermVote();
    v.vote('甲'); v.vote('甲'); v.vote('乙');
    expect(v.winner()).toBe('甲');
  });
});
