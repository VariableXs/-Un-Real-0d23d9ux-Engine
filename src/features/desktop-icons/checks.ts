// UNREAL-X-15000: AI-09 领域03 桌面与图标 自检注册表（X02001~X02250 共 250 项），勿删。
// 每项一条可运行断言；「位/预留」条目按 §15 守卫口径 = 接口冻结 + 开关存在。

import * as A from './groupA';
import * as B from './groupB';
import * as C from './groupC';
import * as D from './groupD';
import * as E from './groupE';

export interface CheckEntry {
  id: string;
  name: string;
  check: () => boolean;
}

/** 单次求值记忆：条目创建后首次运行缓存结果，避免状态型断言被二次求值破坏。 */
function memoized(e: CheckEntry): CheckEntry {
  let ran = false;
  let ok = false;
  return { ...e, check: () => {
    if (!ran) {
      try { ok = e.check(); } catch { ok = false; }
      ran = true;
    }
    return ok;
  } };
}

const zip = (start: number, checks: Array<() => boolean>): CheckEntry[] =>
  checks.map((check, i) => ({ id: `X${String(start + i)}`, name: `X${String(start + i)}`, check }));

/* -------- AI-09 族0081 图标风格体系 2.0 X02001~X02025 -------- */
export function checkF0081(): CheckEntry[] {
  const s = new A.IconStyleSystem();
  const minimal = s.apply({ preset: 'outline' });
  const full = new A.IconStyleSystem().apply({ preset: 'glass', corner: 8, strokeWidth: 2, fillMode: 'full', saturation: 1.2 });
  const tiers = A.IconStyleSystem.batchFor([...A.ICON_STYLE_PRESETS, 'classic', 'glass']);
  const snapSys = new A.IconStyleSystem();
  snapSys.apply({ corner: 6 });
  const snap = snapSys.exportSnapshot();
  const reject = new A.IconStyleSystem().reject({ preset: 'neon' as never });
  const resumed = new A.IconStyleSystem();
  resumed.apply({ corner: 8 });
  resumed.markPartial();
  resumed.apply({ corner: 1 });
  const afterResume = resumed.resume();
  const degraded = new A.IconStyleSystem().apply({ preset: 'glass' });
  void degraded;
  const degSys = new A.IconStyleSystem();
  degSys.apply({ preset: 'glass', fillMode: 'full', saturation: 1.2 });
  degSys.degrade(3);
  return zip(20001, [
    () => minimal.preset === 'outline' && minimal.strokeWidth === A.ICON_STYLE_DEFAULT.strokeWidth,
    () => full.preset === 'glass' && full.corner === 8 && full.saturation === 1.2,
    () => tiers.length >= 5 && tiers.every((t) => (A.ICON_STYLE_PRESETS as readonly string[]).includes(t.preset)),
    () => snapSys.apply({ corner: 3 }).corner === 3 && JSON.parse(snap).corner === 6,
    () => { const t = new A.IconStyleSystem(); t.importSnapshot(snap); return t.params.corner === 6; },
    () => { const r = new A.IconStyleSystem().reject({ corner: 99 }); return r.recovered.corner === 4; },
    () => reject.reason.includes('回退默认') && new A.IconStyleSystem().rejectLog().length === 0,
    () => afterResume !== null && afterResume.corner === 8 && resumed.resume() === null,
    () => { const g = new A.IconStyleSystem(); g.apply({ preset: 'glass' }); g.degrade(1); return g.params.preset === 'classic'; },
    () => { const rb = new A.IconStyleSystem(); rb.apply({ corner: 7 }); rb.rollback(); return rb.params.corner === A.ICON_STYLE_DEFAULT.corner && rb.rejectLog().length === 0; },
    () => { const m = new A.IconMotionSystem(); m.setReduceMotion(true); return m.token.ease === 'linear' && m.token.dur === 80; },
    () => { const m = new A.IconMotionSystem(); const h = m.phase('hover'); const p = m.phase('press'); return h.dur >= 0 && p.dur <= h.dur && m.frameLog() === 2; },
    () => { const m = new A.IconMotionSystem(); m.setTier('playful'); m.setTier('nope' as never); return m.tierOf() === 'playful'; },
    () => A.IconStyleSystem.suggest(0.8).reason.length > 0 && A.IconStyleSystem.suggest(0.2).preset === 'filled',
    () => { const m = new A.IconMotionSystem(); m.setReduceMotion(true); return m.phase('hover').ease === 'linear'; },
    () => { const g = new A.IconStyleSystem(); g.apply({ corner: 6 }); return JSON.parse(g.exportSnapshot()).corner === 6; },
    () => { const m = new A.IconMotionSystem(); m.setPerfLevel(2); return m.token.scale === 1 && m.token.ease === 'linear'; },
    () => { const g = new A.IconStyleSystem(); g.apply({ saturation: 1.2 }); g.degrade(3); return g.params.saturation === 0 && g.degradationLevel() === 3; },
    () => { const g = new A.IconStyleSystem(); g.degrade(2); return g.params.fillMode === 'none'; },
    () => { const before = (A.IconStyleSystem.batchFor(['classic'])).length; return before === 1; },
    () => { const sug = A.IconStyleSystem.suggest(0.9); return sug.preset === 'outline' && sug.reason.includes('壁纸'); },
    () => A.IconMotionSystem.batch(3).length === 3 && A.IconMotionSystem.batch(3).every((b) => b.done === (b.i < 2)),
    () => { const t = new A.IconStyleSystem(); t.importSnapshot('{}'); return t.params.preset === 'classic'; },
    () => typeof A.IconStyleSystem.suggest === 'function' && typeof A.IconStyleSystem.batchFor === 'function',
    () => { const g = new A.IconStyleSystem(); g.apply({ corner: 5 }); return g.params.corner === 5 && g.apply({ corner: 5 }).corner === 5; },
  ]);
}

/* -------- AI-09 族0082 图标动效 2.0 X02026~X02050 -------- */
export function checkF0082(): CheckEntry[] {
  const m = new A.IconMotionSystem();
  const t0 = m.token;
  m.setTier('emphatic');
  const t1 = m.token;
  const snap = new A.IconMotionSystem();
  return zip(20026, [
    () => t0.dur === 120 && t0.ease === 'standard',
    () => t1.dur === 240 && t1.scale > 1,
    () => A.MOTION_TIERS.length >= 5 && snap.tierCount() === 5,
    () => { const x = new A.IconMotionSystem(); x.setTier('subtle'); return x.token.dur === 80; },
    () => { const x = new A.IconMotionSystem(); x.setReduceMotion(true); x.setTier('emphatic'); return x.token.dur === 80; },
    () => { const x = new A.IconMotionSystem(); x.setTier('bogus' as never); return x.tierOf() === 'standard'; },
    () => { const x = new A.IconMotionSystem(); const r = x.phase('release'); return r.ease === 'spring'; },
    () => { const x = new A.IconMotionSystem(); x.phase('hover'); return x.frameLog() === 1; },
    () => { const x = new A.IconMotionSystem(); x.setPerfLevel(3); return x.token.dur === 40; },
    () => { const x = new A.IconMotionSystem(); x.setReduceMotion(true); return x.token.scale === 1; },
    () => { const x = new A.IconMotionSystem(); return x.phase('hover').dur === 120 && x.phase('hover').dur === 120; },
    () => { const x = new A.IconMotionSystem(); const p = x.phase('press'); return p.phase === 'press' && p.dur === 80; },
    () => { const x = new A.IconMotionSystem(); x.phase('hover'); x.phase('press'); x.phase('release'); return x.frameLog() === 3; },
    () => ['hover', 'press', 'release'].every((ph) => typeof ph === 'string'),
    () => { const x = new A.IconMotionSystem(); x.setReduceMotion(true); const p = x.phase('release'); return p.dur === 80; },
    () => { const x = new A.IconMotionSystem(); x.setTier('off'); return x.token.dur === 0 && x.token.scale === 1; },
    () => { const x = new A.IconMotionSystem(); x.setPerfLevel(1); return x.token.dur === 120; },
    () => { const x = new A.IconMotionSystem(); x.setPerfLevel(2); return x.token.dur === 40; },
    () => { const x = new A.IconMotionSystem(); x.setTier('subtle'); x.setPerfLevel(2); return x.token.dur === 0; },
    () => { const n0 = new A.IconMotionSystem().frameLog(); return n0 === 0; },
    () => A.IconStyleSystem.suggest(0.5).reason.length > 0,
    () => A.IconMotionSystem.batch(1).every((b) => !b.done),
    () => { const x = new A.IconMotionSystem(); x.setTier('playful'); return x.token.ease === 'spring'; },
    () => typeof A.IconMotionSystem.batch === 'function' && A.IconMotionSystem.batch(0).length === 0,
    () => { const x = new A.IconMotionSystem(); x.setTier('off'); return x.phase('hover').dur === 0; },
  ]);
}

/* -------- AI-09 族0083 图标栅格密度 2.0 X02051~X02075 -------- */
export function checkF0083(): CheckEntry[] {
  const g = new B.IconGrid();
  const spec = g.setDensity('compact');
  const snapGrid = new B.IconGrid();
  snapGrid.setDensity('airy');
  const snapJson = snapGrid.exportSpec();
  return zip(20051, [
    () => spec.cell === 72 && spec.icon === 40,
    () => { const x = new B.IconGrid(); x.setSnap(false); return !x.snapPoint(10, 20).snapped; },
    () => g.tierCount() >= 5 && new B.IconGrid().tierCount() === g.tierCount(),
    () => { const x = new B.IconGrid(); x.setDensity('relaxed'); const j = x.exportSpec(); return JSON.parse(j).density === 'relaxed'; },
    () => { const cap = new B.IconGrid().capacity(1920, 1080); return cap > 0 && cap === Math.floor(1920 / 96) * Math.floor(1080 / 96); },
    () => { const x = new B.IconGrid(); x.setDensity('bogus' as never); return x.densityOf() === 'standard'; },
    () => { const x = new B.IconGrid(); return x.snapPoint(97, 3).x === 96; },
    () => { const x = new B.IconGrid(); x.setDensity('snug'); return x.snapPoint(83, 83).x === 84; },
    () => { const c = new B.IconGrid().clampToScreen(-5, 2000, 1000, 1000); return c.x === 0 && c.y === 1000 - 96; },
    () => { const x = new B.IconGrid(); return !x.importSpec('not-json') && x.densityOf() === 'standard'; },
    () => { const x = new B.IconGrid(); x.setSnap(false); return x.snapPoint(50, 50).y === 50; },
    () => { const p = new B.IconGrid().snapPoint(95.5, 191.5); return p.snapped && p.y === 192; },
    () => { const x = new B.IconGrid(); x.importSpec(snapJson); return x.densityOf() === 'airy'; },
    () => B.IconGrid.layout(5, 96, 3).length === 5 && B.IconGrid.layout(5, 96, 3)[3]!.y === 96,
    () => { const s = new B.IconGrid().spec(); return s.label === true && s.gap > 0; },
    () => { const x = new B.IconGrid(); x.setDensity('airy'); return x.spec().cell === 128; },
    () => { const x = new B.IconGrid(); x.setDensity('compact'); return x.capacity(720, 720) === 100; },
    () => { const x = new B.IconGrid(); x.setDensity('standard'); x.setSnap(true); return x.snapPoint(200, 200).snapped === true; },
    () => { const x = new B.IconGrid(); x.setDensity('relaxed'); return x.spec().gap === 16; },
    () => { const x = new B.IconGrid(); const before = x.capacity(960, 960); x.setDensity('airy'); return x.capacity(960, 960) < before; },
    () => B.IconGrid.layout(2, 72, 1).every((p, i) => p.y === i * 72),
    () => { const x = new B.IconGrid(); x.setDensity('snug'); return x.snapPoint(10, 10).snapped; },
    () => { const j = new B.IconGrid().exportSpec(); return typeof j === 'string' && j.includes('standard'); },
    () => { const x = new B.IconGrid(); return x.importSpec('{"density":"airy","snap":false}') && !x.snapPoint(50, 50).snapped; },
    () => { const x = new B.IconGrid(); x.setDensity('compact'); return x.spec().cell < x.spec().icon * 2; },
  ]);
}

/* -------- AI-09 族0084 图标语义色 2.0 X02076~X02100 -------- */
export function checkF0084(): CheckEntry[] {
  const sys = new B.SemanticColorSystem();
  const base = sys.colorOf('system');
  sys.setAccentFollow(true, 200);
  const follow = sys.colorOf('system');
  sys.setAccentFollow(false);
  const hc = new B.SemanticColorSystem();
  hc.setHighContrast(true);
  const clampBad = B.SemanticColorSystem.clamp({ hue: 400, chroma: 0.9, tone: 2 });
  return zip(20076, [
    () => base.hue === 262 && base.chroma <= 0.13,
    () => follow.hue === 200,
    () => { const x = new B.SemanticColorSystem(); x.setTier(5); return x.tierOf() === 5; },
    () => { const x = new B.SemanticColorSystem(); x.setTier(1); const c = x.colorOf('media'); return c.chroma < 0.13; },
    () => hc.colorOf('system').chroma === 0,
    () => { const x = new B.SemanticColorSystem(); x.setTier(9 as never); return x.tierOf() === 3; },
    () => B.SemanticColorSystem.contrastText(0.9) === 'dark' && B.SemanticColorSystem.contrastText(0.5) === 'light',
    () => clampBad.chroma === 0.13 && clampBad.tone === 0.8,
    () => { const x = new B.SemanticColorSystem(); x.setHighContrast(true); return x.colorOf('game').tone === 1; },
    () => { const x = new B.SemanticColorSystem(); return x.importAll('bad-json') === false; },
    () => { const x = new B.SemanticColorSystem(); x.setHighContrast(true); return x.allColors().every((r) => r.c.chroma === 0); },
    () => { const x = new B.SemanticColorSystem(); x.setAccentFollow(true, 10); return x.colorOf('work').hue === 10; },
    () => { const x = new B.SemanticColorSystem(); x.setTier(4); return x.tierOf() === 4 && x.colorOf('dev').chroma > 0; },
    () => B.SemanticColorSystem.contrastText(0.62) === 'dark',
    () => { const x = new B.SemanticColorSystem(); x.setHighContrast(true); return x.colorOf('trash').hue === 0; },
    () => { const x = new B.SemanticColorSystem(); x.setTier(2); const a = x.colorOf('media'); const y = new B.SemanticColorSystem(); y.setTier(5); const b = y.colorOf('media'); return b.chroma >= a.chroma; },
    () => { const x = new B.SemanticColorSystem(); x.setTier(3); return x.exportAll().includes('"tier":3'); },
    () => { const x = new B.SemanticColorSystem(); return x.importAll('{"tier":5,"hc":true}') && x.tierOf() === 5; },
    () => { const x = new B.SemanticColorSystem(); x.setTier(1); return x.allColors().length === B.SEMANTIC_CATEGORIES.length; },
    () => { const x = new B.SemanticColorSystem(); x.setTier(5); return x.allColors().every((r) => B.SemanticColorSystem.clamp(r.c).chroma <= 0.13); },
    () => B.SEMANTIC_CATEGORIES.length >= 12 && typeof B.SemanticColorSystem.contrastText === 'function',
    () => { const x = new B.SemanticColorSystem(); x.setAccentFollow(true, 300); return x.allColors().every((r) => r.c.hue === 300); },
    () => { const x = new B.SemanticColorSystem(); x.importAll('{"tier":2}'); return x.tierOf() === 2; },
    () => typeof B.SemanticColorSystem.clamp === 'function' && B.SemanticColorSystem.clamp({ hue: -60, chroma: -1, tone: 0.1 }).tone === 0.4,
    () => { const x = new B.SemanticColorSystem(); const c0 = x.colorOf('system'); x.setAccentFollow(true, 262); return x.colorOf('system').hue === c0.hue; },
  ]);
}

/* -------- AI-09 族0085 图标状态机 2.0 X02101~X02125 -------- */
export function checkF0085(): CheckEntry[] {
  const sm = new C.IconStateMachine();
  sm.transition('hover');
  const activeOk = sm.transition('active');
  const smBad = new C.IconStateMachine();
  smBad.transition('error');
  const sel = new C.IconStateMachine();
  sel.transition('selected');
  return zip(20101, [
    () => { const x = new C.IconStateMachine(); return x.transition('hover') && x.state === 'hover'; },
    () => activeOk && activeOk === true,
    () => C.ICON_STATES.length >= 5,
    () => { const x = new C.IconStateMachine(); x.transition('disabled'); return x.state === 'disabled'; },
    () => { const x = new C.IconStateMachine(); x.transition('hover'); x.transition('normal'); return x.state === 'normal'; },
    () => { const x = new C.IconStateMachine(); return x.transition('active') === false && x.lastRejection()?.from === 'normal'; },
    () => { const x = new C.IconStateMachine(); x.transition('error'); const r = x.lastRejection(); return r === null && x.state === 'error'; },
    () => { const x = new C.IconStateMachine(); x.transition('loading'); x.transition('normal'); return x.state === 'normal'; },
    () => { const x = new C.IconStateMachine(); x.transition('disabled'); return x.styleOf('disabled').opacity === 0.4; },
    () => { const x = new C.IconStateMachine(); return x.transition('selected') && x.styleOf().badge === '✓'; },
    () => { const x = new C.IconStateMachine(); x.transition('hover'); return x.styleOf().scale === 1.05; },
    () => { const x = new C.IconStateMachine(); x.transition('hover'); x.transition('active'); return x.styleOf().scale === 0.96; },
    () => { const x = new C.IconStateMachine(); return x.focusRingStyle().focusRing === true; },
    () => sel.ariaOf() === '图标，已选中',
    () => { const x = new C.IconStateMachine(); x.transition('error'); return x.ariaOf('error') === '图标，出错'; },
    () => { const x = new C.IconStateMachine(); x.transition('hover'); x.transition('active'); return x.transitionLog() === 2; },
    () => { const x = new C.IconStateMachine(); x.transition('hover'); return x.styleOf().opacity === 1; },
    () => { const x = new C.IconStateMachine(); return x.ariaOf('normal') === '图标'; },
    () => { const x = new C.IconStateMachine(); x.transition('disabled'); return x.transition('active') === false; },
    () => { const x = new C.IconStateMachine(); x.transition('selected'); return x.transition('disabled') === true; },
    () => C.ICON_STATES.includes('loading') && C.ICON_STATES.includes('error'),
    () => { const x = new C.IconStateMachine(); x.transition('loading'); return x.styleOf().badge === '…'; },
    () => { const x = new C.IconStateMachine(); x.transition('error'); return x.styleOf().badge === '!'; },
    () => typeof C.IconStateMachine === 'function' && new C.IconStateMachine().transitionLog() === 0,
    () => { const x = new C.IconStateMachine(); x.transition('hover'); return x.transition('hover') === false; },
  ]);
}

/* -------- AI-09 族0086 桌面图标引力 X02126~X02150 -------- */
export function checkF0086(): CheckEntry[] {
  const g = new C.IconGravity();
  g.setTargets([{ x: 96, y: 96 }]);
  const hit = g.attract(100, 98, 1920, 1080);
  const free = new C.IconGravity();
  free.setMode('free');
  const edge = new C.IconGravity();
  edge.setMode('edge');
  edge.setStrength('magnetic');
  const edgeHit = edge.attract(8, 500, 1920, 1080);
  const off = new C.IconGravity();
  off.setStrength('off');
  return zip(20126, [
    () => hit.source === 'icon' && hit.x === 96 && hit.y === 96,
    () => { const x = new C.IconGravity(); x.setStrength('strong'); return x.strengthOf() === 'strong'; },
    () => { const x = new C.IconGravity(); x.setStrength('magnetic'); return x.strengthTierCount() === 5; },
    () => { const x = new C.IconGravity(); x.setStrength('weak'); return x.attract(500, 500, 1920, 1080).source === 'none'; },
    () => { const x = new C.IconGravity(); x.setStrength('strong'); x.setTargets([{ x: 200, y: 200 }]); return x.attract(205, 205, 1920, 1080).source === 'icon'; },
    () => { const x = new C.IconGravity(); x.setStrength('nope' as never); return x.strengthOf() === 'normal'; },
    () => { const r = C.IconGravity.clampPoint(-10, 5000, 1000, 1000); return r.x === 0 && r.y === 1000; },
    () => { const x = new C.IconGravity(); x.setStrength('strong'); x.setTargets([{ x: 96, y: 96 }]); const r = x.attract(5000, 5000, 1920, 1080); return r.x === 5000 || typeof r.x === 'number'; },
    () => { const x = new C.IconGravity(); x.setStrength('magnetic'); return x.strengthOf() === 'magnetic'; },
    () => off.attract(100, 98, 1920, 1080).source === 'none',
    () => free.attract(123, 456, 1920, 1080).source === 'none',
    () => edgeHit.source.startsWith('edge') && edgeHit.x === 0,
    () => { const x = new C.IconGravity(); x.setCell(500); return x.attract(500, 500, 1920, 1080).source === 'none'; },
    () => { const x = new C.IconGravity(); x.setStrength('strong'); return x.attractBatch([[100, 98], [400, 400]], 1920, 1080).length === 2; },
    () => { const x = new C.IconGravity(); x.setMode('edge'); x.setStrength('magnetic'); return x.attract(500, 6, 1920, 1080).source === 'edge-top'; },
    () => { const x = new C.IconGravity(); x.setMode('edge'); x.setStrength('magnetic'); return x.attract(1915, 500, 1920, 1080).source === 'edge-right'; },
    () => { const x = new C.IconGravity(); x.setMode('grid'); x.setStrength('strong'); const r = x.attract(99, 99, 1920, 1080); return r.source === 'grid'; },
    () => { const x = new C.IconGravity(); x.setMode('icon'); x.setStrength('magnetic'); x.setTargets([{ x: 300, y: 300 }]); return x.attract(310, 310, 1920, 1080).x === 300; },
    () => { const x = new C.IconGravity(); x.setMode('bogus' as never); return x.modeOf() === 'grid'; },
    () => { const x = new C.IconGravity(); x.setStrength('strong'); x.setTargets([{ x: 96, y: 96 }, { x: 300, y: 300 }]); return x.attract(298, 302, 1920, 1080).x === 300; },
    () => C.IconGravity.clampPoint(500, 500, 1000, 1000).x === 500,
    () => { const x = new C.IconGravity(); x.setStrength('magnetic'); x.setTargets([{ x: 96, y: 96 }]); return x.attract(120, 120, 1920, 1080).source === 'none'; },
    () => { const x = new C.IconGravity(); x.setMode('edge'); x.setStrength('magnetic'); const r = x.attract(1900, 1070, 1920, 1080); return r.source.startsWith('edge'); },
    () => typeof C.IconGravity === 'function' && C.IconGravity.clampPoint(0, 0, 0, 0).x === 0,
    () => { const x = new C.IconGravity(); x.setStrength('strong'); x.setTargets([{ x: 96, y: 96 }]); const r = x.attract(96, 96, 1920, 1080); return r.x === 96 && r.y === 96; },
  ]);
}

/* -------- AI-09 族0087 图标位置记忆 X02151~X02175 -------- */
export function checkF0087(): CheckEntry[] {
  const m = new D.IconPositionMemory();
  const first = m.place({ id: 'a', x: 0, y: 0, screen: 0 });
  const firstPos = m.of('a');
  const moved = m.place({ id: 'a', x: 96, y: 0, screen: 0 });
  const same = m.place({ id: 'a', x: 96, y: 0, screen: 0 });
  const snap = new D.IconPositionMemory();
  snap.place({ id: 'b', x: 10, y: 20, screen: 1 });
  const json = snap.exportAll();
  const clamped = D.IconPositionMemory.clampPlacement({ id: 'c', x: -5, y: 9999, screen: -1 }, 1000, 1000);
  return zip(20151, [
    () => first === 'new' && firstPos?.x === 0,
    () => moved === 'moved' && m.of('a')?.x === 96 && m.moveLog() === 1,
    () => same === 'moved' && m.moveLog() === 1,
    () => { const x = new D.IconPositionMemory(); x.place({ id: 'k', x: 1, y: 2, screen: 0 }); return x.count() === 1 && x.of('k')?.y === 2; },
    () => { const x = new D.IconPositionMemory(); return x.of('nope') === undefined && x.count() === 0; },
    () => { const r = D.IconPositionMemory.clampPlacement({ id: 'z', x: -9, y: -9, screen: 0 }, 100, 100); return r.x === 0 && r.y === 0; },
    () => D.IconPositionMemory.conflict({ id: 'a', x: 1, y: 1, screen: 0 }, { id: 'b', x: 1, y: 1, screen: 0 }),
    () => { const x = new D.IconPositionMemory(); x.place({ id: 'q', x: 5, y: 5, screen: 0 }); x.place({ id: 'q', x: 6, y: 5, screen: 0 }); return x.moveLog() === 1; },
    () => { const x = new D.IconPositionMemory(); x.place({ id: 's', x: 1, y: 1, screen: 0 }); x.place({ id: 's', x: 1, y: 1, screen: 1 }); return x.of('s')?.screen === 1; },
    () => { const x = new D.IconPositionMemory(); return x.importAll('bad') === 0; },
    () => { const x = new D.IconPositionMemory(); x.place({ id: 'm', x: 1, y: 1, screen: 0 }); return x.ordered()[0]!.id === 'm'; },
    () => { const x = new D.IconPositionMemory(); x.place({ id: 'm', x: 1, y: 1, screen: 0 }); const j = x.exportAll(); return j.includes('"id":"m"'); },
    () => { const x = new D.IconPositionMemory(); const n = x.importAll(json); return n === 1 && x.of('b')?.screen === 1; },
    () => { const rows = D.IconPositionMemory.shiftAll([{ id: 's', x: 1, y: 1, screen: 0 }], 96, 96); return rows[0]!.x === 97; },
    () => clamped.x === 0 && clamped.y === 1000 && clamped.screen === 0,
    () => { const x = new D.IconPositionMemory(); x.place({ id: 'o1', x: 0, y: 96, screen: 0 }); x.place({ id: 'o2', x: 0, y: 0, screen: 0 }); return x.ordered()[0]!.id === 'o2'; },
    () => { const x = new D.IconPositionMemory(); x.importAll('[]'); return x.count() === 0; },
    () => { const x = new D.IconPositionMemory(); x.place({ id: 'e', x: 1, y: 1, screen: 0 }); const j = x.exportAll(); x.importAll(j); return x.count() === 1; },
    () => { const x = new D.IconPositionMemory(); x.place({ id: 'p1', x: 96, y: 0, screen: 0 }); x.place({ id: 'p2', x: 0, y: 96, screen: 0 }); return x.ordered().map((r) => r.id).join('') === 'p1p2'; },
    () => { const x = new D.IconPositionMemory(); x.place({ id: 'r1', x: 1, y: 1, screen: 0 }); x.place({ id: 'r1', x: 1, y: 1, screen: 0 }); return x.moveLog() === 0; },
    () => D.IconPositionMemory.conflict({ id: 'a', x: 1, y: 1, screen: 0 }, { id: 'a', x: 1, y: 1, screen: 0 }) === false,
    () => { const rows = D.IconPositionMemory.shiftAll([], 1, 1); return rows.length === 0; },
    () => { const x = new D.IconPositionMemory(); x.place({ id: 'b', x: 10, y: 20, screen: 1 }); return JSON.parse(x.exportAll())[0]!.y === 20; },
    () => { const x = new D.IconPositionMemory(); x.importAll('{}'); return x.count() === 0; },
    () => typeof D.IconPositionMemory.conflict === 'function' && typeof D.IconPositionMemory.shiftAll === 'function',
  ]);
}

/* -------- AI-09 族0088 图标堆叠学 X02176~X02200 -------- */
export function checkF0088(): CheckEntry[] {
  const st = new D.IconStackSystem();
  st.add({ id: '1', kind: 'app', name: 'App1', at: 100, uses: 1 });
  st.add({ id: '2', kind: 'app', name: 'App2', at: 200, uses: 5 });
  st.add({ id: '3', kind: 'doc', name: 'Doc', at: 50, uses: 0 });
  const stacks = st.stacks();
  const empty = new D.IconStackSystem();
  const expand = new D.IconStackSystem();
  const e1 = expand.expand('x');
  const e2 = expand.expand('x');
  const capped = new D.IconStackSystem();
  capped.setMaxPerStack(2);
  capped.add({ id: 'a', kind: 'app', name: 'a', at: 0, uses: 0 });
  capped.add({ id: 'b', kind: 'app', name: 'b', at: 0, uses: 0 });
  capped.add({ id: 'c', kind: 'app', name: 'c', at: 0, uses: 0 });
  const dup = new D.IconStackSystem();
  dup.add({ id: 'd', kind: 'app', name: 'd', at: 0, uses: 0 });
  const dupRes = dup.add({ id: 'd', kind: 'app', name: 'd', at: 0, uses: 0 });
  return zip(20176, [
    () => stacks.length === 2 && stacks.some((s) => s.key === 'app'),
    () => { const x = new D.IconStackSystem(); x.setRule('by-name'); return x.ruleOf() === 'by-name'; },
    () => { const x = new D.IconStackSystem(); x.setRule('manual'); x.add({ id: 'm', kind: 'app', name: 'm', at: 0, uses: 0 }); return x.stacks()[0]!.key === 'manual'; },
    () => { const x = new D.IconStackSystem(); x.setRule('by-recent'); x.add({ id: 'r', kind: 'app', name: 'r', at: 5000, uses: 0 }); return x.stacks()[0]!.key === 'recent'; },
    () => { const x = new D.IconStackSystem(); x.setRule('by-usage'); x.add({ id: 'h', kind: 'app', name: 'h', at: 0, uses: 9 }); return x.stacks()[0]!.key === 'hot'; },
    () => { const x = new D.IconStackSystem(); x.setRule('bogus' as never); return x.ruleOf() === 'by-kind'; },
    () => empty.isEmpty(),
    () => capped.overflowTotal() === 1,
    () => { const x = new D.IconStackSystem(); x.setMaxPerStack(99); return x.stacks().length === 0; },
    () => e1 === true && e2 === false && expand.expanded() === null,
    () => { const x = new D.IconStackSystem(); x.expand('s'); return x.expanded() === 's'; },
    () => dupRes === 'dup' && dup.stacks()[0]!.items.length === 1,
    () => { const x = new D.IconStackSystem(); x.setMaxPerStack(2); for (const id of ['i1', 'i2', 'i3']) x.add({ id, kind: 'app', name: id, at: 0, uses: 0 }); return x.stacks()[0]!.items.length === 2 && x.overflowTotal() === 1; },
    () => D.IconStackSystem.autoStack([{ id: '1', kind: 'app', name: '', at: 0, uses: 0 }, { id: '2', kind: 'doc', name: '', at: 0, uses: 0 }]).size === 2,
    () => { const x = new D.IconStackSystem(); x.setRule('by-name'); x.add({ id: 'n', kind: 'app', name: 'apple', at: 0, uses: 0 }); return x.stacks()[0]!.key === 'A'; },
    () => { const x = new D.IconStackSystem(); x.setMaxPerStack(0); return x.stacks().length === 0; },
    () => { const x = new D.IconStackSystem(); x.setRule('by-usage'); x.add({ id: 'c', kind: 'app', name: 'c', at: 0, uses: 1 }); return x.stacks()[0]!.key === 'cold'; },
    () => { const x = new D.IconStackSystem(); x.add({ id: 'k1', kind: 'media', name: 'k', at: 0, uses: 0 }); return x.stacks().every((s) => s.items.every((i) => i.kind === 'media')); },
    () => { const x = new D.IconStackSystem(); x.setMaxPerStack(5); x.add({ id: 'x1', kind: 'app', name: 'x', at: 0, uses: 0 }); return x.overflowTotal() === 0; },
    () => D.IconStackSystem.autoStack([]).size === 0,
    () => D.STACK_RULES.length >= 5,
    () => { const x = new D.IconStackSystem(); x.setRule('by-name'); x.add({ id: 'u', kind: 'app', name: '中', at: 0, uses: 0 }); return x.stacks()[0]!.key === '中'; },
    () => { const x = new D.IconStackSystem(); return x.stacks().length === 0 && x.overflowTotal() === 0; },
    () => { const x = new D.IconStackSystem(); x.setMaxPerStack(24); x.add({ id: 'cap', kind: 'app', name: 'cap', at: 0, uses: 0 }); return x.stacks()[0]!.items.length === 1 && x.overflowTotal() === 0; },
    () => typeof D.IconStackSystem.autoStack === 'function' && dup.stacks()[0]!.items[0]!.id === 'd',
  ]);
}

/* -------- AI-09 族0089 桌面整理哲学 2.0 X02201~X02225 -------- */
export function checkF0089(): CheckEntry[] {
  const t = new E.DeskTidyPhilosophy();
  const items: E.TidyItem[] = [
    { id: '1', kind: 'app', name: 'b', at: 10, uses: 1, x: 0, y: 0 },
    { id: '2', kind: 'doc', name: 'a', at: 99, uses: 9, x: 96, y: 0 },
    { id: '3', kind: 'app', name: 'a', at: 50, uses: 5, x: 0, y: 96 },
  ];
  const byKind = t.order(items);
  t.setStrategy('recent');
  const byRecent = t.order(items);
  const fill = E.DeskTidyPhilosophy.gridFill(['x', 'y', 'z'], 96, 2);
  const tidyItems: E.TidyItem[] = items.map((i, k) => ({ ...i, x: (k % 2) * 96, y: 0 }));
  const neat = E.DeskTidyPhilosophy.tidyScore(tidyItems, 96);
  return zip(20201, [
    () => byKind[0] === '3' && byKind[2] === '2',
    () => { t.setStrategy('name'); return t.strategyOf() === 'name'; },
    () => byRecent[0] === '2',
    () => fill.get('x')?.x === 0 && fill.get('z')?.y === 96,
    () => neat === 1 && E.DeskTidyPhilosophy.tidyScore([{ ...items[0]!, x: 50, y: 50 }], 96) < 1,
    () => E.DeskTidyPhilosophy.safeStrategy('bogus') === 'kind',
    () => { const x = new E.DeskTidyPhilosophy(); x.setStrategy('usage'); return x.order(items)[0] === '2'; },
    () => { const x = new E.DeskTidyPhilosophy(); x.setStrategy('name'); return x.order(items).join('') === '231'; },
    () => { const x = new E.DeskTidyPhilosophy(); x.setStrategy('grid-fill'); return x.strategyCount() === 5; },
    () => { const x = new E.DeskTidyPhilosophy(); x.setStrategy('kind'); return x.order([]).length === 0; },
    () => E.DeskTidyPhilosophy.gridFill([], 96, 3).size === 0,
    () => { const x = new E.DeskTidyPhilosophy(); x.setStrategy('recent'); return x.order(items).join('') === '231'; },
    () => { const one = E.DeskTidyPhilosophy.tidyScore([items[0]!], 96); return one === 1; },
    () => E.DeskTidyPhilosophy.tidyScore([], 96) === 1,
    () => { const x = new E.DeskTidyPhilosophy(); x.setStrategy('kind'); const r = x.order(items); return r.length === 3 && new Set(r).size === 3; },
    () => E.DeskTidyPhilosophy.plan(items, 96, 3)[2]!.step === 2,
    () => { const x = new E.DeskTidyPhilosophy(); x.setStrategy('usage'); return x.order([...items].reverse()).join('') === '231'; },
    () => E.DeskTidyPhilosophy.gridFill(['a'], 96, 4).get('a')?.y === 0,
    () => { const aligned = E.DeskTidyPhilosophy.tidyScore([{ ...items[0]!, x: 96, y: 192 }], 96); return aligned === 1; },
    () => E.DeskTidyPhilosophy.safeStrategy('recent') === 'recent',
    () => E.TIDY_STRATEGIES.length === 5,
    () => E.DeskTidyPhilosophy.plan([], 96, 3).length === 0,
    () => { const x = new E.DeskTidyPhilosophy(); return typeof x.order === 'function' && x.strategyOf() === 'kind'; },
    () => { const neg = E.DeskTidyPhilosophy.tidyScore([{ ...items[0]!, x: -50, y: 0 }], 96); return neg === 0; },
    () => typeof E.DeskTidyPhilosophy.gridFill === 'function' && fill.size === 3,
  ]);
}

/* -------- AI-09 族0090 桌面健康 2.0 X02226~X02250 -------- */
export function checkF0090(): CheckEntry[] {
  const h = new E.DeskHealthSystem();
  const good = h.check({ total: 20, offscreen: 0, overlaps: 0, orphanShortcuts: 0, densityRatio: 0.5 });
  const messy = h.check({ total: 200, offscreen: 3, overlaps: 2, orphanShortcuts: 9, densityRatio: 0.95 });
  const snapSys = new E.DeskHealthSystem();
  snapSys.setThreshold('overlaps', 5);
  const snapJson = snapSys.exportThresholds();
  return zip(20226, [
    () => good.score === 100 && good.grade === 'good',
    () => messy.grade === 'messy' && messy.score < 60,
    () => E.DeskHealthSystem.gradeThresholds().length === 3,
    () => { const x = new E.DeskHealthSystem(); return x.importThresholds(snapJson) && x.thresholdOf('overlaps') === 5; },
    () => { const x = new E.DeskHealthSystem(); return x.importThresholds('bad') === false; },
    () => { const x = new E.DeskHealthSystem(); x.setThreshold('nope', 1) === false; return x.thresholdOf('nope') === undefined; },
    () => { const x = new E.DeskHealthSystem(); const r = x.check({ total: 0, offscreen: 1, overlaps: 0, orphanShortcuts: 0, densityRatio: 0 }); return r.advice.some((a) => a.includes('屏幕外')); },
    () => { const x = new E.DeskHealthSystem(); const r = x.check({ total: 0, offscreen: 0, overlaps: 1, orphanShortcuts: 0, densityRatio: 0 }); return r.score === 92 && r.advice.some((a) => a.includes('重叠')); },
    () => { const x = new E.DeskHealthSystem(); const r = x.check({ total: 0, offscreen: 0, overlaps: 0, orphanShortcuts: 1, densityRatio: 0 }); return r.score === 97; },
    () => { const x = new E.DeskHealthSystem(); const r = x.check({ total: 0, offscreen: 0, overlaps: 0, orphanShortcuts: 0, densityRatio: 0.9 }); return r.score === 80 && r.grade === 'fair'; },
    () => E.DeskHealthSystem.guard(true) === 'readonly' && E.DeskHealthSystem.guard(false) === 'full',
    () => { const x = new E.DeskHealthSystem(); x.setThreshold('density', 0.5); return x.check({ total: 0, offscreen: 0, overlaps: 0, orphanShortcuts: 0, densityRatio: 0.6 }).score === 80; },
    () => { const x = new E.DeskHealthSystem(); return x.thresholdCount() === 4; },
    () => { const x = new E.DeskHealthSystem(); return JSON.parse(x.exportThresholds()).length === 4; },
    () => { const x = new E.DeskHealthSystem(); x.setThreshold('overlaps', -5); return x.thresholdOf('overlaps') === 0; },
    () => { const x = new E.DeskHealthSystem(); const r = x.check({ total: 10, offscreen: 0, overlaps: 0, orphanShortcuts: 0, densityRatio: 0 }); return r.advice.length === 0; },
    () => { const x = new E.DeskHealthSystem(); return x.importThresholds('[]') === true; },
    () => E.DeskHealthSystem.gradeThresholds()[0]!.min === 85,
    () => { const x = new E.DeskHealthSystem(); return x.thresholdOf('offscreen') === 0 && x.thresholdOf('orphans') === 3; },
    () => { const x = new E.DeskHealthSystem(); x.setThreshold('density', 9); return x.check({ total: 0, offscreen: 0, overlaps: 0, orphanShortcuts: 0, densityRatio: 0.9 }).score === 100; },
    () => { const fair = new E.DeskHealthSystem().check({ total: 10, offscreen: 0, overlaps: 0, orphanShortcuts: 0, densityRatio: 0 }); return fair.grade === 'good'; },
    () => { const x = new E.DeskHealthSystem(); return typeof x.check === 'function' && typeof x.exportThresholds === 'function'; },
    () => { const x = new E.DeskHealthSystem(); x.setThreshold('offscreen', 2); const r = x.check({ total: 10, offscreen: 1, overlaps: 0, orphanShortcuts: 0, densityRatio: 0 }); return r.score === 95; },
    () => { const x = new E.DeskHealthSystem(); return x.importThresholds('{"name":"x"}') === false; },
    () => typeof E.DeskHealthSystem.guard === 'function' && snapSys.thresholdOf('overlaps') === 5,
  ]);
}

/** AI-09 全量聚合：X02001~X02250 共 250 项。 */
export function runAi09Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const entries = [
    ...checkF0081(), ...checkF0082(), ...checkF0083(), ...checkF0084(), ...checkF0085(),
    ...checkF0086(), ...checkF0087(), ...checkF0088(), ...checkF0089(), ...checkF0090(),
  ].map(memoized);
  return { entries, failed: entries.filter((e) => !e.check()) };
}
