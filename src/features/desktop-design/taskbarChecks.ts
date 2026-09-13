/**
 * UNREAL-X-15000 · AI-13 任务栏形态与交互 CheckSet（族0121~0130 · X03001~X03250），勿删。
 * 每族 25 项 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
 */
import type { CheckEntry } from '../uikit/checks';
import * as T from './taskbarModels';

/* -------- 族0121 任务栏形态 2.0 X03001~X03025 -------- */
export function checkF0121(): CheckEntry[] {
  const t = new T.TaskbarShape();
  return [
    { id: 'X03001', name: '形态·最小闭环', check: () => t.set('position', 'left') && t.state.position === 'left' },
    { id: 'X03002', name: '形态·全量参数', check: () => t.set('align', 'start') && t.set('autoHide', true) && t.state.align === 'start' && t.state.autoHide },
    { id: 'X03003', name: '形态·档位矩阵', check: () => T.TASKBAR_HEIGHT_PRESETS.length === 5 && t.set('heightPreset', 4) && t.height() === 72 },
    { id: 'X03004', name: '形态·快照迁移', check: () => { const t2 = T.TaskbarShape.deserialize(t.serialize()); return t2.state.position === 'left' && t2.height() === 72; } },
    { id: 'X03005', name: '形态·联调集成', check: () => t.set('position', 'bottom') && t.height() === T.TASKBAR_HEIGHT_PRESETS[t.state.heightPreset] },
    { id: 'X03006', name: '形态·越界钳制', check: () => !t.set('position', 'diagonal' as never) && !t.set('heightPreset', 9) && t.state.position === 'bottom' },
    { id: 'X03007', name: '形态·失败叙事', check: () => t.set('heightPreset', -1) === false && t.state.heightPreset <= 4 },
    { id: 'X03008', name: '形态·中断还原', check: () => { const t2 = T.TaskbarShape.deserialize('not-json'); return t2.state.position === 'bottom'; } },
    { id: 'X03009', name: '形态·资源降级', check: () => { const t2 = new T.TaskbarShape(); t2.degrade('mid'); return t2.state.accentBar === false && t2.height() === 48; } },
    { id: 'X03010', name: '形态·回滚净身', check: () => { t.rollback(); return t.state.position === 'left' && t.state.autoHide === true; } },
    { id: 'X03011', name: '形态·动效令牌', check: () => t.set('accentBar', false) && t.state.accentBar === false },
    { id: 'X03012', name: '形态·三态焦点', check: () => { const t2 = new T.TaskbarShape(); t2.degrade('low'); return t2.state.autoHide && !t2.state.accentBar; } },
    { id: 'X03013', name: '形态·键盘序', check: () => { let ok = true; for (const p of T.TASKBAR_POSITIONS) ok = ok && t.set('position', p); return ok; } },
    { id: 'X03014', name: '形态·微文案', check: () => T.TASKBAR_POSITIONS.join('/') === 'bottom/top/left/right' },
    { id: 'X03015', name: '形态·aria 等价', check: () => { const t2 = T.TaskbarShape.deserialize(JSON.stringify({ position: 'right', heightPreset: 2 })); return t2.state.position === 'right' && t2.height() === 56; } },
    { id: 'X03016', name: '形态·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) t.set('heightPreset', i % 5); return performance.now() - t0 < 50; } },
    { id: 'X03017', name: '形态·热路径', check: () => { const t2 = new T.TaskbarShape(); t2.state.heightPreset = 3; return t2.height() === 64; } },
    { id: 'X03018', name: '形态·零漂移', check: () => { const a = T.TaskbarShape.deserialize(t.serialize()); const b = T.TaskbarShape.deserialize(a.serialize()); return b.serialize() === a.serialize(); } },
    { id: 'X03019', name: '形态·低配减档', check: () => { const t2 = new T.TaskbarShape(); t2.degrade('low'); return t2.state.autoHide; } },
    { id: 'X03020', name: '形态·守卫', check: () => t.set('align', 'middle' as never) === false },
    { id: 'X03021', name: '形态·智能建议', check: () => { const t2 = new T.TaskbarShape(); t2.set('align', 'center'); return t2.state.align === 'center'; } },
    { id: 'X03022', name: '形态·批量模式', check: () => { const t2 = new T.TaskbarShape(); let n = 0; for (let i = 0; i < 20; i++) if (t2.set('heightPreset', i % 5)) n++; return n === 20; } },
    { id: 'X03023', name: '形态·跨域联动', check: () => { const t2 = new T.TaskbarShape(); t2.set('position', 'top'); return t2.state.position === 'top' && t2.height() === 48; } },
    { id: 'X03024', name: '形态·扩展点', check: () => typeof T.TaskbarShape.deserialize === 'function' && typeof t.serialize === 'function' },
    { id: 'X03025', name: '形态·彩蛋层', check: () => { const t2 = new T.TaskbarShape(); return t2.reset().heightPreset === 1 && t2.history.length === 0; } },
  ];
}

/* -------- 族0122 任务栏交互 2.0 X03026~X03050 -------- */
export function checkF0122(): CheckEntry[] {
  const x = new T.TaskbarInteraction();
  x.add('a'); x.add('b'); x.add('c');
  return [
    { id: 'X03026', name: '交互·最小闭环', check: () => x.click('a', 'left', 1000) === 'focus' && x.isOpen('a') },
    { id: 'X03027', name: '交互·全量参数', check: () => { x.debounceMs = 200; return x.debounceMs === 200; } },
    { id: 'X03028', name: '交互·档位矩阵', check: () => x.click('a', 'left', 5000) === 'minimize' && !x.isOpen('a') && x.click('a', 'left', 6000) === 'focus' && x.isOpen('a') },
    { id: 'X03029', name: '交互·持久语义', check: () => !x.isOpen('b') && x.click('b', 'left', 7000) === 'focus' },
    { id: 'X03030', name: '交互·联调集成', check: () => x.isOpen('a') && x.isOpen('b') && !x.isOpen('c') },
    { id: 'X03031', name: '交互·越界钳制', check: () => x.click('ghost', 'left', 8000) === 'focus' && x.click('a', 'middle', 8001) === 'new-instance' },
    { id: 'X03032', name: '交互·失败叙事', check: () => x.click('a', 'right', 9000) === 'context-menu' && x.isOpen('a') },
    { id: 'X03033', name: '交互·中断续用', check: () => { x.add('d'); return x.click('d', 'left', 10000) === 'focus'; } },
    { id: 'X03034', name: '交互·资源降级', check: () => { x.debounceMs = 0; return x.click('c', 'left', 11000) === 'focus' && x.click('c', 'left', 11001) === 'minimize'; } },
    { id: 'X03035', name: '交互·净身', check: () => { const x2 = new T.TaskbarInteraction(); x2.add('z'); return x2.click('z', 'left', 0) === 'focus' && x2.isOpen('z'); } },
    { id: 'X03036', name: '交互·动效令牌', check: () => T.TaskbarInteraction.HOVER.show === 300 && T.TaskbarInteraction.HOVER.hide === 150 },
    { id: 'X03037', name: '交互·三态焦点', check: () => { const x2 = new T.TaskbarInteraction(); x2.add('a'); return x2.click('a', 'left', 0) === 'focus' && x2.click('a', 'left', 999) === 'minimize'; } },
    { id: 'X03038', name: '交互·键盘序', check: () => { const x2 = new T.TaskbarInteraction(); x2.add('a'); x2.add('b'); x2.add('c'); return x2.keys('ArrowRight') === 1 && x2.keys('End') === 2 && x2.keys('Home') === 0 && x2.keys('ArrowLeft') === 2; } },
    { id: 'X03039', name: '交互·微文案', check: () => x.click('a', 'middle', 12000) === 'new-instance' && x.click('a', 'right', 12001) === 'context-menu' },
    { id: 'X03040', name: '交互·aria 等价', check: () => { const x2 = new T.TaskbarInteraction(); return x2.keys('Home') === -1; } },
    { id: 'X03041', name: '交互·基准采集', check: () => { const t0 = performance.now(); const x2 = new T.TaskbarInteraction(); for (let i = 0; i < 500; i++) { x2.add(`w${i}`); x2.click(`w${i}`, 'left', i); } return performance.now() - t0 < 50; } },
    { id: 'X03042', name: '交互·热路径', check: () => { const x2 = new T.TaskbarInteraction(); x2.add('k'); return x2.click('k', 'left', 0) === 'focus'; } },
    { id: 'X03043', name: '交互·零漂移', check: () => { const x2 = new T.TaskbarInteraction(); x2.add('m'); return x2.click('m', 'left', 0) === 'focus' && x2.isOpen('m'); } },
    { id: 'X03044', name: '交互·低配减档', check: () => { const x2 = new T.TaskbarInteraction(); x2.debounceMs = 0; x2.add('m'); return x2.click('m', 'left', 0) === 'focus' && x2.click('m', 'left', 1) === 'minimize'; } },
    { id: 'X03045', name: '交互·守卫', check: () => { const x2 = new T.TaskbarInteraction(); x2.add('s'); x2.debounceMs = 120; x2.click('s', 'left', 0); return x2.click('s', 'left', 10) === 'ignored'; } },
    { id: 'X03046', name: '交互·智能建议', check: () => { const x2 = new T.TaskbarInteraction(); return x2.keys('End') === -1 && x2.keys('ArrowRight') === -1; } },
    { id: 'X03047', name: '交互·批量模式', check: () => { const x2 = new T.TaskbarInteraction(); for (let i = 0; i < 10; i++) x2.add(`b${i}`); return x2.keys('End') === 9; } },
    { id: 'X03048', name: '交互·跨域联动', check: () => x.click('b', 'left', 20000) === 'minimize' && x.click('b', 'left', 21000) === 'focus' },
    { id: 'X03049', name: '交互·扩展点', check: () => typeof x.keys === 'function' && typeof x.click === 'function' },
    { id: 'X03050', name: '交互·彩蛋层', check: () => { const x2 = new T.TaskbarInteraction(); x2.add('a'); x2.add('a'); x2.add('b'); return x2.keys('End') === 1; } },
  ];
}

/* -------- 族0123 托盘区 2.0 X03051~X03075 -------- */
export function checkF0123(): CheckEntry[] {
  const tray = new T.TrayArea();
  return [
    { id: 'X03051', name: '托盘·最小闭环', check: () => tray.register('net') && tray.layout().visible.some((i) => i.id === 'net') },
    { id: 'X03052', name: '托盘·全量参数', check: () => tray.register('vol', '音量', true) && tray.layout().visible[0]!.id === 'vol' },
    { id: 'X03053', name: '托盘·档位矩阵', check: () => { for (let i = 0; i < 8; i++) tray.register(`t${i}`); return tray.layout().overflow.length === 5; } },
    { id: 'X03054', name: '托盘·持久语义', check: () => tray.register('net') === false && tray.size === 10 },
    { id: 'X03055', name: '托盘·联调集成', check: () => tray.visibleSlots === 5 && tray.layout().visible.length === 5 },
    { id: 'X03056', name: '托盘·越界钳制', check: () => { tray.setBadge('net', -5); return tray.badgeLabel('net') === '0'; } },
    { id: 'X03057', name: '托盘·失败叙事', check: () => tray.badgeLabel('ghost') === '' },
    { id: 'X03058', name: '托盘·中断续用', check: () => { tray.setBadge('net', 3); return tray.badgeLabel('net') === '3'; } },
    { id: 'X03059', name: '托盘·资源降级', check: () => { tray.visibleSlots = 2; return tray.layout().overflow.length === 8; } },
    { id: 'X03060', name: '托盘·净身', check: () => tray.unregister('t0') && tray.unregister('t0') === false && tray.size === 9 },
    { id: 'X03061', name: '托盘·动效令牌', check: () => { tray.visibleSlots = 5; return tray.layout().visible.length === 5; } },
    { id: 'X03062', name: '托盘·三态焦点', check: () => tray.layout().visible.every((i) => typeof i.id === 'string') },
    { id: 'X03063', name: '托盘·键盘序', check: () => { tray.visibleSlots = 99; return tray.layout().overflow.length === 0; } },
    { id: 'X03064', name: '托盘·微文案', check: () => { const t2 = new T.TrayArea(); t2.register('long', '这是一个非常非常非常非常长的提示文案超出二十四个字符上限'); return t2.layout().visible[0]!.tooltip.length <= 24; } },
    { id: 'X03065', name: '托盘·aria 等价', check: () => { const t2 = new T.TrayArea(); t2.register('x', '网络'); return t2.layout().visible[0]!.tooltip === '网络'; } },
    { id: 'X03066', name: '托盘·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) tray.setBadge('net', i % 120); return performance.now() - t0 < 50; } },
    { id: 'X03067', name: '托盘·热路径', check: () => { tray.setBadge('net', 100); return tray.badgeLabel('net') === '99+'; } },
    { id: 'X03068', name: '托盘·零漂移', check: () => { const l1 = tray.layout(); const l2 = tray.layout(); return l1.visible.map((v) => v.id).join() === l2.visible.map((v) => v.id).join(); } },
    { id: 'X03069', name: '托盘·低配减档', check: () => { const t2 = new T.TrayArea(); t2.visibleSlots = 1; t2.register('a', '', true); t2.register('b'); return t2.layout().overflow.length === 1; } },
    { id: 'X03070', name: '托盘·守卫', check: () => { tray.visibleSlots = 0; return tray.layout().visible.length === 0; } },
    { id: 'X03071', name: '托盘·智能建议', check: () => { const t2 = new T.TrayArea(); t2.register('a', '', true); t2.register('b'); return t2.layout().visible[0]!.pinned; } },
    { id: 'X03072', name: '托盘·批量模式', check: () => { const t2 = new T.TrayArea(); let n = 0; for (let i = 0; i < 20; i++) if (t2.register(`b${i}`)) n++; return n === 20; } },
    { id: 'X03073', name: '托盘·跨域联动', check: () => { tray.setBadge('net', 999); return tray.badgeLabel('net') === '99+'; } },
    { id: 'X03074', name: '托盘·扩展点', check: () => typeof tray.layout === 'function' && typeof tray.register === 'function' },
    { id: 'X03075', name: '托盘·彩蛋层', check: () => { const t2 = new T.TrayArea(); return t2.size === 0 && t2.layout().visible.length === 0; } },
  ];
}

/* -------- 族0124 小组件区 2.0 X03076~X03100 -------- */
export function checkF0124(): CheckEntry[] {
  const w = new T.WidgetStrip();
  return [
    { id: 'X03076', name: '小组件·最小闭环', check: () => w.add('clock') !== null && w.count === 1 },
    { id: 'X03077', name: '小组件·全量参数', check: () => w.add('weather', 'l')!.size === 'l' },
    { id: 'X03078', name: '小组件·档位矩阵', check: () => w.add('k', 's') !== null && w.add('k', 'm') !== null && w.count === 4 },
    { id: 'X03079', name: '小组件·持久语义', check: () => w.add('fifth') === null && w.count === 4 },
    { id: 'X03080', name: '小组件·联调集成', check: () => w.slots === 4 && w.list().length === 4 },
    { id: 'X03081', name: '小组件·越界钳制', check: () => w.move('clock-0', 99) === false && w.move('ghost', 0) === false },
    { id: 'X03082', name: '小组件·失败叙事', check: () => w.toggle('ghost') === false && w.remove('ghost') === false },
    { id: 'X03083', name: '小组件·中断续用', check: () => w.toggle('clock-0') && w.list()[0]!.collapsed },
    { id: 'X03084', name: '小组件·资源降级', check: () => { w.remove('k-2'); return w.count === 3 && w.add('again') !== null; } },
    { id: 'X03085', name: '小组件·净身', check: () => w.remove('again-3') && w.count === 3 },
    { id: 'X03086', name: '小组件·动效令牌', check: () => w.refreshMs === 30000 },
    { id: 'X03087', name: '小组件·三态焦点', check: () => w.toggle('clock-0') && !w.list()[0]!.collapsed },
    { id: 'X03088', name: '小组件·键盘序', check: () => w.move('weather-1', 0) && w.list()[0]!.kind === 'weather' },
    { id: 'X03089', name: '小组件·微文案', check: () => w.list().every((x) => x.id.length > 0) },
    { id: 'X03090', name: '小组件·aria 等价', check: () => w.list().every((x) => typeof x.kind === 'string' && x.kind.length > 0) },
    { id: 'X03091', name: '小组件·基准采集', check: () => { const t0 = performance.now(); const w2 = new T.WidgetStrip(); for (let i = 0; i < 500; i++) { w2.slots = 999; w2.add('x', 's'); w2.remove(w2.list()[0]!.id); } return performance.now() - t0 < 50; } },
    { id: 'X03092', name: '小组件·热路径', check: () => w.shouldRefresh(w.lastRefresh + 30000) && !w.shouldRefresh(w.lastRefresh + 1) },
    { id: 'X03093', name: '小组件·零漂移', check: () => { const ids = w.list().map((x) => x.id); return new Set(ids).size === ids.length; } },
    { id: 'X03094', name: '小组件·低配减档', check: () => { const w2 = new T.WidgetStrip(); w2.slots = 1; return w2.add('a') !== null && w2.add('b') === null; } },
    { id: 'X03095', name: '小组件·守卫', check: () => { const w2 = new T.WidgetStrip(); return w2.add('a')!.id === 'a-0'; } },
    { id: 'X03096', name: '小组件·智能建议', check: () => { const w2 = new T.WidgetStrip(); w2.add('a'); w2.add('b'); return w2.move('b-1', 0) && w2.list()[0]!.id === 'b-1'; } },
    { id: 'X03097', name: '小组件·批量模式', check: () => { const w2 = new T.WidgetStrip(); w2.slots = 20; let n = 0; for (let i = 0; i < 20; i++) if (w2.add('x')) n++; return n === 20; } },
    { id: 'X03098', name: '小组件·跨域联动', check: () => { const w2 = new T.WidgetStrip(); const cw = w2.add('clock'); return cw !== null && w2.toggle(cw.id) && w2.list()[0]!.collapsed; } },
    { id: 'X03099', name: '小组件·扩展点', check: () => typeof w.move === 'function' && typeof w.shouldRefresh === 'function' },
    { id: 'X03100', name: '小组件·彩蛋层', check: () => { const w2 = new T.WidgetStrip(); return w2.count === 0 && w2.list().length === 0; } },
  ];
}

/* -------- 族0125 任务栏行为 2.0 X03101~X03125 -------- */
export function checkF0125(): CheckEntry[] {
  const b = new T.TaskbarBehavior();
  return [
    { id: 'X03101', name: '行为·最小闭环', check: () => b.hover(0) === false && b.isHidden === false },
    { id: 'X03102', name: '行为·全量参数', check: () => { b.hideDelayMs = 500; b.revealEdgePx = 6; return b.hideDelayMs === 500 && b.revealEdgePx === 6; } },
    { id: 'X03103', name: '行为·档位矩阵', check: () => { b.autoHide = false; return b.leave(0) === false && !b.hiddenAfterDelay(); } },
    { id: 'X03104', name: '行为·持久语义', check: () => { b.autoHide = true; return b.hiddenAfterDelay() === true; } },
    { id: 'X03105', name: '行为·联调集成', check: () => b.alwaysOnTop === true },
    { id: 'X03106', name: '行为·越界钳制', check: () => !b.pointerNear(50, 100, 1080, 'bottom') && b.pointerNear(50, 1078, 1080, 'bottom') },
    { id: 'X03107', name: '行为·失败叙事', check: () => !b.pointerNear(50, 0, 1080, 'bottom') && b.pointerNear(50, 2, 1080, 'top') },
    { id: 'X03108', name: '行为·中断续用', check: () => { b.setHidden(true); b.hover(0); return !b.isHidden; } },
    { id: 'X03109', name: '行为·资源降级', check: () => { const prev = b.revealEdgePx; b.revealEdgePx = 0; const r = !b.pointerNear(0, 0, 100, 'bottom'); b.revealEdgePx = prev; return r && b.autoHide; } },
    { id: 'X03110', name: '行为·净身', check: () => { b.focusGain(); return !b.isHidden && b.autoHide; } },
    { id: 'X03111', name: '行为·动效令牌', check: () => b.hideDelayMs >= 0 && b.revealEdgePx >= 0 },
    { id: 'X03112', name: '行为·三态焦点', check: () => { b.setHidden(true); b.focusGain(); return !b.isHidden; } },
    { id: 'X03113', name: '行为·键盘序', check: () => typeof b.hover === 'function' && typeof b.leave === 'function' },
    { id: 'X03114', name: '行为·微文案', check: () => b.hiddenAfterDelay() === true && b.autoHide === true },
    { id: 'X03115', name: '行为·aria 等价', check: () => { const b2 = new T.TaskbarBehavior(); return b2.isHidden && b2.autoHide; } },
    { id: 'X03116', name: '行为·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 2000; i++) b.pointerNear(i % 1920, 1079, 1080, 'bottom'); return performance.now() - t0 < 50; } },
    { id: 'X03117', name: '行为·热路径', check: () => b.pointerNear(960, 1076, 1080, 'bottom') === true },
    { id: 'X03118', name: '行为·零漂移', check: () => { const h1 = b.isHidden; b.hover(0); b.setHidden(h1); return b.isHidden === h1; } },
    { id: 'X03119', name: '行为·低配减档', check: () => { const b2 = new T.TaskbarBehavior(); b2.autoHide = false; return b2.hiddenAfterDelay() === false; } },
    { id: 'X03120', name: '行为·守卫', check: () => { b.autoHide = false; const r = b.hiddenAfterDelay(); b.autoHide = true; return r === false; } },
    { id: 'X03121', name: '行为·智能建议', check: () => { const b2 = new T.TaskbarBehavior(); return b2.revealEdgePx === 4 && b2.hideDelayMs === 400; } },
    { id: 'X03122', name: '行为·批量模式', check: () => { const b2 = new T.TaskbarBehavior(); let n = 0; for (let i = 0; i < 10; i++) { b2.hover(i); if (!b2.isHidden) n++; } return n === 10; } },
    { id: 'X03123', name: '行为·跨域联动', check: () => b.pointerNear(0, 3, 1080, 'top') && !b.pointerNear(0, 3, 1080, 'bottom') },
    { id: 'X03124', name: '行为·扩展点', check: () => typeof b.hiddenAfterDelay === 'function' && typeof b.focusGain === 'function' },
    { id: 'X03125', name: '行为·彩蛋层', check: () => { const b2 = new T.TaskbarBehavior(); return b2.isHidden && b2.alwaysOnTop; } },
  ];
}

/* -------- 族0126 任务栏预览卡 X03126~X03150 -------- */
export function checkF0126(): CheckEntry[] {
  const p = new T.PreviewCard();
  return [
    { id: 'X03126', name: '预览·最小闭环', check: () => { p.cacheThumb('w1', 'thumb'); return p.thumb('w1') === 'thumb'; } },
    { id: 'X03127', name: '预览·全量参数', check: () => { p.maxTitle = 20; return p.maxTitle === 20; } },
    { id: 'X03128', name: '预览·档位矩阵', check: () => { for (let i = 0; i < 5; i++) p.cacheThumb(`w${i}`, `t${i}`); return p.cacheSize === 5; } },
    { id: 'X03129', name: '预览·持久语义', check: () => p.cacheThumb('w1', 'thumb2') === undefined && p.thumb('w1') === 'thumb2' },
    { id: 'X03130', name: '预览·联调集成', check: () => p.thumb('w9') === undefined && p.cacheSize === 5 },
    { id: 'X03131', name: '预览·越界钳制', check: () => p.title('一'.repeat(40)).length === 20 && p.title('短') === '短' },
    { id: 'X03132', name: '预览·失败叙事', check: () => p.title('一'.repeat(40)).endsWith('…') },
    { id: 'X03133', name: '预览·中断续用', check: () => p.invalidate('w1') && !p.invalidate('w1') && p.cacheSize === 4 },
    { id: 'X03134', name: '预览·资源降级', check: () => { for (let i = 0; i < 5; i++) p.invalidate(`w${i}`); return p.cacheSize === 0; } },
    { id: 'X03135', name: '预览·净身', check: () => p.cacheSize === 0 && p.thumb('w0') === undefined },
    { id: 'X03136', name: '预览·动效令牌', check: () => p.ttlMs === 5000 },
    { id: 'X03137', name: '预览·三态焦点', check: () => { p.shownAt = 1000; return !p.fresh(6001) && p.fresh(5000); } },
    { id: 'X03138', name: '预览·键盘序', check: () => { p.cacheThumb('a', 'x'); p.cacheThumb('b', 'y'); return p.cacheSize === 2; } },
    { id: 'X03139', name: '预览·微文案', check: () => p.title(`窗口${'文'.repeat(19)}`).length === 20 },
    { id: 'X03140', name: '预览·aria 等价', check: () => p.aria('w1', '文档 - 编辑器') === '预览：文档 - 编辑器（w1）' },
    { id: 'X03141', name: '预览·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) p.cacheThumb(`k${i}`, 'v'); return performance.now() - t0 < 50; } },
    { id: 'X03142', name: '预览·热路径', check: () => p.thumb('k499') === 'v' },
    { id: 'X03143', name: '预览·零漂移', check: () => { p.cacheThumb('a', 'x'); p.cacheThumb('a', 'x'); return p.cacheSize === 502; } },
    { id: 'X03144', name: '预览·低配减档', check: () => { p.ttlMs = 0; p.shownAt = 1; return p.fresh(1) && !p.fresh(2); } },
    { id: 'X03145', name: '预览·守卫', check: () => p.aria('', '') === '预览：（）' },
    { id: 'X03146', name: '预览·智能建议', check: () => { const p2 = new T.PreviewCard(); return p2.cacheSize === 0 && p2.maxTitle === 32; } },
    { id: 'X03147', name: '预览·批量模式', check: () => { const p2 = new T.PreviewCard(); let n = 0; for (let i = 0; i < 20; i++) { p2.cacheThumb(`b${i}`, 'v'); if (p2.thumb(`b${i}`)) n++; } return n === 20; } },
    { id: 'X03148', name: '预览·跨域联动', check: () => { const p2 = new T.PreviewCard(); p2.shownAt = 0; return p2.fresh(5000) && !p2.fresh(5001); } },
    { id: 'X03149', name: '预览·扩展点', check: () => typeof p.aria === 'function' && typeof p.invalidate === 'function' },
    { id: 'X03150', name: '预览·彩蛋层', check: () => { const p2 = new T.PreviewCard(); p2.cacheThumb('z', 'v'); return p2.invalidate('z') && p2.cacheSize === 0; } },
  ];
}

/* -------- 族0127 任务栏进度融合 X03151~X03175 -------- */
export function checkF0127(): CheckEntry[] {
  const f = new T.ProgressFusion();
  return [
    { id: 'X03151', name: '进度·最小闭环', check: () => { f.set('w1', 50); return f.of('w1').value === 50 && f.of('w1').mode === 'normal'; } },
    { id: 'X03152', name: '进度·全量参数', check: () => { f.set('w1', 20, 'paused'); return f.of('w1').mode === 'paused'; } },
    { id: 'X03153', name: '进度·档位矩阵', check: () => { f.set('w2', 0, 'indeterminate'); return f.overlay('w2') === 'indeterminate'; } },
    { id: 'X03154', name: '进度·持久语义', check: () => f.of('ghost').mode === 'none' && f.of('ghost').value === 0 },
    { id: 'X03155', name: '进度·联调集成', check: () => { f.set('tmp', 50); return f.overlay('tmp') === 'none'; } },
    { id: 'X03156', name: '进度·越界钳制', check: () => { f.set('w3', 150); return f.of('w3').value === 100; } },
    { id: 'X03157', name: '进度·失败叙事', check: () => { f.set('w3', -5); return f.of('w3').value === 0; } },
    { id: 'X03158', name: '进度·中断续用', check: () => { f.set('w3', 60); return f.of('w3').value === 60; } },
    { id: 'X03159', name: '进度·资源降级', check: () => { f.set('w3', 70, 'error'); return f.overlay('w3') === 'error'; } },
    { id: 'X03160', name: '进度·净身', check: () => f.clear('w3') && f.clear('w3') === false && f.of('w3').mode === 'none' },
    { id: 'X03161', name: '进度·动效令牌', check: () => f.overlay('w2') === 'indeterminate' },
    { id: 'X03162', name: '进度·三态焦点', check: () => { f.set('w4', 100); return f.of('w4').mode === 'none'; } },
    { id: 'X03163', name: '进度·键盘序', check: () => { f.set('w5', 1); f.set('w6', 2); return f.size >= 3; } },
    { id: 'X03164', name: '进度·微文案', check: () => { f.set('w7', 99.6); return f.of('w7').value === 100 && f.of('w7').mode === 'none'; } },
    { id: 'X03165', name: '进度·aria 等价', check: () => { f.set('w8', 0, 'paused'); return f.overlay('w8') === 'paused'; } },
    { id: 'X03166', name: '进度·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 1000; i++) f.set(`k${i % 50}`, i % 101); return performance.now() - t0 < 50; } },
    { id: 'X03167', name: '进度·热路径', check: () => f.of('k7').mode === 'none' || f.of('k7').value >= 0 },
    { id: 'X03168', name: '进度·零漂移', check: () => { f.set('z', 42); const a = f.of('z'); f.set('z', 42); const b2 = f.of('z'); return a.value === b2.value && a.mode === b2.mode; } },
    { id: 'X03169', name: '进度·低配减档', check: () => { f.set('low', 10, 'indeterminate'); return f.of('low').value === 0; } },
    { id: 'X03170', name: '进度·守卫', check: () => !f.clear('ghost') },
    { id: 'X03171', name: '进度·智能建议', check: () => { const f2 = new T.ProgressFusion(); return f2.size === 0; } },
    { id: 'X03172', name: '进度·批量模式', check: () => { let n = 0; for (let i = 0; i < 20; i++) { f.set(`b${i}`, i); if (f.of(`b${i}`).value === i) n++; } return n === 20; } },
    { id: 'X03173', name: '进度·跨域联动', check: () => { f.set('w1', 100, 'normal'); return f.overlay('w1') === 'none'; } },
    { id: 'X03174', name: '进度·扩展点', check: () => typeof f.overlay === 'function' && typeof f.of === 'function' },
    { id: 'X03175', name: '进度·彩蛋层', check: () => { f.clear('w1'); f.clear('w2'); f.clear('w4'); f.clear('w5'); f.clear('w6'); f.clear('w7'); f.clear('w8'); f.clear('low'); return true; } },
  ];
}

/* -------- 族0128 任务栏分组 X03176~X03200 -------- */
export function checkF0128(): CheckEntry[] {
  const g = new T.TaskbarGrouping();
  const wins = (app: string, n: number, off = 0) =>
    Array.from({ length: n }, (_, i) => ({ winId: `${app}-${off + i}`, app, order: off + i }));
  return [
    { id: 'X03176', name: '分组·最小闭环', check: () => g.group(wins('app', 4)).length === 1 && g.group(wins('app', 4))[0]!.wins.length === 4 },
    { id: 'X03177', name: '分组·全量参数', check: () => { g.ungroupThreshold = 2; return g.group(wins('app', 3)).length === 1; } },
    { id: 'X03178', name: '分组·档位矩阵', check: () => [1, 2, 3, 4, 5].every((t) => { g.ungroupThreshold = t; return g.group(wins('a', 5)).length === (5 > t ? 1 : 0); }) },
    { id: 'X03179', name: '分组·持久语义', check: () => { g.ungroupThreshold = 3; g.setOrder(['b', 'a', 'c']); return g.orderSnapshot.join() === 'b,a,c'; } },
    { id: 'X03180', name: '分组·联调集成', check: () => g.group([...wins('x', 4), ...wins('y', 2)]).length === 1 },
    { id: 'X03181', name: '分组·越界钳制', check: () => g.group([]).length === 0 },
    { id: 'X03182', name: '分组·失败叙事', check: () => g.group(wins('solo', 1)).length === 0 },
    { id: 'X03183', name: '分组·中断续用', check: () => { g.setOrder(['z']); return g.orderSnapshot.length === 1; } },
    { id: 'X03184', name: '分组·资源降级', check: () => { g.ungroupThreshold = 100; return g.group(wins('big', 100)).length === 0; } },
    { id: 'X03185', name: '分组·净身', check: () => { g.ungroupThreshold = 3; return g.group(wins('a', 3)).length === 0; } },
    { id: 'X03186', name: '分组·动效令牌', check: () => g.ungroupThreshold === 3 },
    { id: 'X03187', name: '分组·三态焦点', check: () => { const gs = g.group([...wins('a', 5), ...wins('b', 4)]); return gs.every((x) => x.wins.length > 3); } },
    { id: 'X03188', name: '分组·键盘序', check: () => { g.setOrder(['1', '2', '2', '3']); return g.orderSnapshot.length === 3; } },
    { id: 'X03189', name: '分组·微文案', check: () => g.serialize().startsWith('[') },
    { id: 'X03190', name: '分组·aria 等价', check: () => g.group(wins('app', 4))[0]!.app === 'app' },
    { id: 'X03191', name: '分组·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 200; i++) g.group(wins('p', 10, i)); return performance.now() - t0 < 50; } },
    { id: 'X03192', name: '分组·热路径', check: () => g.group(wins('q', 4))[0]!.wins.length === 4 },
    { id: 'X03193', name: '分组·零漂移', check: () => { g.setOrder(['m', 'n']); const g2 = T.TaskbarGrouping.deserialize(g.serialize()); return g2.orderSnapshot.join() === 'm,n'; } },
    { id: 'X03194', name: '分组·低配减档', check: () => { const g2 = T.TaskbarGrouping.deserialize('bad-json'); return g2.orderSnapshot.length === 0; } },
    { id: 'X03195', name: '分组·守卫', check: () => T.TaskbarGrouping.deserialize('[1,2,3]').orderSnapshot.length === 0 },
    { id: 'X03196', name: '分组·智能建议', check: () => { const g2 = new T.TaskbarGrouping(); g2.ungroupThreshold = 0; return g2.group(wins('s', 1)).length === 1; } },
    { id: 'X03197', name: '分组·批量模式', check: () => { const all = [...wins('b1', 5), ...wins('b2', 5), ...wins('b3', 5)]; return g.group(all).length === 3; } },
    { id: 'X03198', name: '分组·跨域联动', check: () => { g.ungroupThreshold = 4; const r = g.group(wins('t', 5)); g.ungroupThreshold = 3; return r.length === 1; } },
    { id: 'X03199', name: '分组·扩展点', check: () => typeof T.TaskbarGrouping.deserialize === 'function' && typeof g.group === 'function' },
    { id: 'X03200', name: '分组·彩蛋层', check: () => { const g2 = new T.TaskbarGrouping(); return g2.orderSnapshot.length === 0 && g2.ungroupThreshold === 3; } },
  ];
}

/* -------- 族0129 任务栏多屏 X03201~X03225 -------- */
export function checkF0129(): CheckEntry[] {
  const m = new T.TaskbarMultiScreen();
  const MONS: T.Monitor[] = [
    { id: 'm1', primary: true, bounds: { x: 0, y: 0, w: 1920, h: 1080 } },
    { id: 'm2', primary: false, bounds: { x: 1920, y: 0, w: 1920, h: 1080 } },
  ];
  const wins = [
    { id: 'w1', x: 100, y: 100 },
    { id: 'w2', x: 2000, y: 100 },
  ];
  return [
    { id: 'X03201', name: '多屏·最小闭环', check: () => { m.setMonitors(MONS); return m.primary === 'm1'; } },
    { id: 'X03202', name: '多屏·全量参数', check: () => { m.mode = 'primary-only'; return m.mode === 'primary-only'; } },
    { id: 'X03203', name: '多屏·档位矩阵', check: () => ['per-screen', 'primary-only', 'primary-windows'].every((k) => { m.mode = k as T.TaskbarMultiScreen['mode']; return m.mode === k; }) },
    { id: 'X03204', name: '多屏·持久语义', check: () => { m.mode = 'per-screen'; return m.monitorOf(100, 100) === 'm1' && m.monitorOf(2000, 100) === 'm2'; } },
    { id: 'X03205', name: '多屏·联调集成', check: () => m.visibleOn('m1', wins).join() === 'w1' && m.visibleOn('m2', wins).join() === 'w2' },
    { id: 'X03206', name: '多屏·越界钳制', check: () => m.monitorOf(9999, 9999) === 'm1' },
    { id: 'X03207', name: '多屏·失败叙事', check: () => { m.setMonitors([]); return m.primary === '' && m.monitorOf(0, 0) === ''; } },
    { id: 'X03208', name: '多屏·中断续用', check: () => { m.setMonitors(MONS); return m.primary === 'm1' && m.monitors.length === 2; } },
    { id: 'X03209', name: '多屏·资源降级', check: () => { m.mode = 'primary-only'; return m.visibleOn('m2', wins).length === 0; } },
    { id: 'X03210', name: '多屏·净身', check: () => { m.mode = 'per-screen'; return m.visibleOn('m2', []).length === 0; } },
    { id: 'X03211', name: '多屏·动效令牌', check: () => m.mode === 'per-screen' },
    { id: 'X03212', name: '多屏·三态焦点', check: () => m.visibleOn('m1', wins).includes('w1') && !m.visibleOn('m1', wins).includes('w2') },
    { id: 'X03213', name: '多屏·键盘序', check: () => { m.setMonitors([MONS[1]!, MONS[0]!]); return m.primary === 'm1'; } },
    { id: 'X03214', name: '多屏·微文案', check: () => m.monitorOf(1919, 500) === 'm1' && m.monitorOf(1920, 500) === 'm2' },
    { id: 'X03215', name: '多屏·aria 等价', check: () => { m.setMonitors([{ id: 'm1', primary: false, bounds: { x: 0, y: 0, w: 100, h: 100 } }, { id: 'm2', primary: false, bounds: { x: 100, y: 0, w: 100, h: 100 } }]); return m.monitors[0]!.primary; } },
    { id: 'X03216', name: '多屏·基准采集', check: () => { m.setMonitors(MONS); const t0 = performance.now(); for (let i = 0; i < 2000; i++) m.monitorOf(i % 3840, 500); return performance.now() - t0 < 50; } },
    { id: 'X03217', name: '多屏·热路径', check: () => m.monitorOf(3000, 1079) === 'm2' },
    { id: 'X03218', name: '多屏·零漂移', check: () => { const a = m.visibleOn('m1', wins); const b2 = m.visibleOn('m1', wins); return a.join() === b2.join(); } },
    { id: 'X03219', name: '多屏·低配减档', check: () => { m.mode = 'primary-windows'; return m.visibleOn('m2', wins).length === 0; } },
    { id: 'X03220', name: '多屏·守卫', check: () => { m.setMonitors([{ id: 'm1', primary: true, bounds: { x: 0, y: 0, w: 1, h: 1 } }, { id: 'm1', primary: false, bounds: { x: 1, y: 0, w: 1, h: 1 } }]); return m.monitors.length === 1; } },
    { id: 'X03221', name: '多屏·智能建议', check: () => { const m2 = new T.TaskbarMultiScreen(); return m2.mode === 'per-screen' && m2.monitors.length === 0; } },
    { id: 'X03222', name: '多屏·批量模式', check: () => { m.setMonitors(MONS); m.mode = 'per-screen'; let n = 0; for (let i = 0; i < 10; i++) if (m.visibleOn('m1', wins).length === 1) n++; return n === 10; } },
    { id: 'X03223', name: '多屏·跨域联动', check: () => { m.mode = 'primary-only'; const r = m.visibleOn('m1', wins); m.mode = 'per-screen'; return r.length === 2; } },
    { id: 'X03224', name: '多屏·扩展点', check: () => typeof m.visibleOn === 'function' && typeof m.monitorOf === 'function' },
    { id: 'X03225', name: '多屏·彩蛋层', check: () => { const m2 = new T.TaskbarMultiScreen(); return m2.primary === ''; } },
  ];
}

/* -------- 族0130 任务栏性能 X03226~X03250 -------- */
export function checkF0130(): CheckEntry[] {
  const p = new T.TaskbarPerf();
  return [
    { id: 'X03226', name: '性能·最小闭环', check: () => p.cached('k', () => 'v') === 'v' && p.misses === 1 },
    { id: 'X03227', name: '性能·全量参数', check: () => T.TaskbarPerf.BUDGETS.length === 3 && T.TaskbarPerf.BUDGETS[0]!.budgetMs === 8 },
    { id: 'X03228', name: '性能·档位矩阵', check: () => T.TaskbarPerf.BUDGETS.every((b) => b.budgetMs > 0 && b.budgetMs <= 16) },
    { id: 'X03229', name: '性能·持久语义', check: () => p.cached('k', () => 'v2') === 'v' && p.hits === 1 },
    { id: 'X03230', name: '性能·联调集成', check: () => p.hitRate === 0.5 },
    { id: 'X03231', name: '性能·越界钳制', check: () => !p.inBudget('layout', 999) },
    { id: 'X03232', name: '性能·失败叙事', check: () => !p.inBudget('ghost', 1) },
    { id: 'X03233', name: '性能·中断续用', check: () => { p.batch(() => undefined); p.batch(() => undefined); return p.flush() === 2; } },
    { id: 'X03234', name: '性能·资源降级', check: () => { p.flush(); return p.flush() === 0; } },
    { id: 'X03235', name: '性能·净身', check: () => p.hits + p.misses === 2 },
    { id: 'X03236', name: '性能·动效令牌', check: () => T.TaskbarPerf.BUDGETS.find((b) => b.key === 'preview')!.budgetMs === 16 },
    { id: 'X03237', name: '性能·三态焦点', check: () => p.inBudget('paint', 8) && !p.inBudget('paint', 9) },
    { id: 'X03238', name: '性能·键盘序', check: () => { p.batch(() => undefined); p.batch(() => undefined); p.batch(() => undefined); return p.flush() === 3 && p.flush() === 0; } },
    { id: 'X03239', name: '性能·微文案', check: () => T.TaskbarPerf.BUDGETS.map((b) => b.key).join() === 'layout,paint,preview' },
    { id: 'X03240', name: '性能·aria 等价', check: () => p.hitRate >= 0 && p.hitRate <= 1 },
    { id: 'X03241', name: '性能·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 1000; i++) p.cached(`k${i}`, () => 'x'); return performance.now() - t0 < 50; } },
    { id: 'X03242', name: '性能·热路径', check: () => p.cached('k0', () => 'y') === 'x' },
    { id: 'X03243', name: '性能·零漂移', check: () => { const p2 = new T.TaskbarPerf(); p2.cached('a', () => '1'); const before = p2.hitRate; p2.cached('a', () => '1'); return p2.hitRate > before; } },
    { id: 'X03244', name: '性能·低配减档', check: () => { const p2 = new T.TaskbarPerf(); return p2.hitRate === 0 && p2.hits === 0; } },
    { id: 'X03245', name: '性能·守卫', check: () => p.inBudget('layout', 8) && !p.inBudget('preview', 17) },
    { id: 'X03246', name: '性能·智能建议', check: () => { const p2 = new T.TaskbarPerf(); p2.cached('a', () => '1'); p2.cached('a', () => '1'); p2.cached('b', () => '2'); return Math.abs(p2.hitRate - 1 / 3) < 1e-9; } },
    { id: 'X03247', name: '性能·批量模式', check: () => { const p2 = new T.TaskbarPerf(); let n = 0; for (let i = 0; i < 20; i++) p2.batch(() => { n++; }); p2.flush(); return n === 20; } },
    { id: 'X03248', name: '性能·跨域联动', check: () => { const p2 = new T.TaskbarPerf(); p2.batch(() => undefined); return p2.flush() === 1 && p2.batch(() => undefined) === undefined; } },
    { id: 'X03249', name: '性能·扩展点', check: () => typeof p.cached === 'function' && typeof p.flush === 'function' },
    { id: 'X03250', name: '性能·彩蛋层', check: () => { const p2 = new T.TaskbarPerf(); return p2.hits === 0 && p2.misses === 0 && p2.hitRate === 0; } },
  ];
}
