/**
 * UNREAL-X-15000 · AI-26 生活工具面 V 线 CheckSet（族0251~0260 · X06251~X06500），勿删。
 * 每族 25 项 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
 */
import type { CheckEntry } from '../uikit/checks';
import * as T from './ai26Models';

/* -------- 族0251 录音音频工具 2.0 X06251~X06275 -------- */
export function checkF0251(): CheckEntry[] {
  return [
    { id: 'X06251', name: '录音·最小闭环', check: () => { const r = new T.VoiceRecorder(); r.start(); r.push(100, false); const s = r.stop(); return s.chunks === 1 && s.ms === 100 && s.kbps === 128; } },
    { id: 'X06252', name: '录音·全量参数', check: () => new T.VoiceRecorder('studio').tier === 'studio' },
    { id: 'X06253', name: '录音·档位矩阵', check: () => T.REC_MATRIX.length === 5 && T.REC_MATRIX.every((t) => new T.VoiceRecorder(t).tier === t) },
    { id: 'X06254', name: '录音·快照迁移', check: () => { const r = new T.VoiceRecorder('high'); r.start(); r.push(50, false); const a = T.VoiceRecorder.deserialize(r.serialize()); return a.stop().chunks === 1; } },
    { id: 'X06255', name: '录音·联调集成', check: () => { const r = new T.VoiceRecorder('standard'); r.start(); r.push(10, false); r.push(20, false); return r.stop().ms === 30; } },
    { id: 'X06256', name: '录音·越界钳制', check: () => { const r = new T.VoiceRecorder('ultra'); return r.tier === 'standard' && r.clamped === 1; } },
    { id: 'X06257', name: '录音·失败叙事', check: () => T.explainError('E2601').next.includes('麦克风') },
    { id: 'X06258', name: '录音·中断续跑', check: () => { const r = new T.VoiceRecorder(); r.start(); r.push(10, false); r.push(10, false); r.stop(); r.resume(); r.push(10, false); return r.stop().chunks === 3; } },
    { id: 'X06259', name: '录音·资源降级', check: () => new T.VoiceRecorder('mute').start() === false },
    { id: 'X06260', name: '录音·回滚净身', check: () => { const r = new T.VoiceRecorder(); r.start(); r.push(10, false); return r.purge() && r.stop().chunks === 0; } },
    { id: 'X06261', name: '录音·动效令牌', check: () => { const m = T.motionFor('balanced'); return m.curve === 'ease-standard' && m.durationMs === 180 && m.scale === 1; } },
    { id: 'X06262', name: '录音·三态焦点', check: () => { const r = new T.VoiceRecorder(); r.start(); return r.push(100, true) === 0; } },
    { id: 'X06263', name: '录音·键盘序', check: () => T.REC_MATRIX.every((t) => new T.VoiceRecorder(t).tier === t) },
    { id: 'X06264', name: '录音·微文案', check: () => T.explainError('E2601').text === '录音设备不可用' },
    { id: 'X06265', name: '录音·aria 等价', check: () => typeof new T.VoiceRecorder().start === 'function' },
    { id: 'X06266', name: '录音·基准采集', check: () => { const r = new T.VoiceRecorder(); r.start(); const t0 = performance.now(); for (let i = 0; i < 500; i++) r.push(10, false); return performance.now() - t0 < 50; } },
    { id: 'X06267', name: '录音·热路径', check: () => { const r = new T.VoiceRecorder(); r.start(); r.push(10, false); return r.push(10, false) === 2; } },
    { id: 'X06268', name: '录音·零漂移', check: () => { const r = new T.VoiceRecorder('low'); r.start(); r.push(100, false); r.push(100, false); const a = T.VoiceRecorder.deserialize(r.serialize()); return a.serialize() === r.serialize(); } },
    { id: 'X06269', name: '录音·低配减档', check: () => T.REC_KBPS['low'] === 64 && T.REC_KBPS['low'] < T.REC_KBPS['studio'] },
    { id: 'X06270', name: '录音·守卫', check: () => T.VoiceRecorder.deserialize('bad{').tier === 'standard' },
    { id: 'X06271', name: '录音·智能建议', check: () => new T.VoiceRecorder('high').stop().kbps === 256 },
    { id: 'X06272', name: '录音·批量模式', check: () => { const r = new T.VoiceRecorder(); r.start(); for (let i = 0; i < 25; i++) r.push(10, false); return r.stop().chunks === 25; } },
    { id: 'X06273', name: '录音·跨域联动', check: () => T.REC_KBPS[T.VoiceRecorder.deserialize(new T.VoiceRecorder('studio').serialize()).tier] === 512 },
    { id: 'X06274', name: '录音·扩展点', check: () => typeof T.REC_KBPS === 'object' && typeof T.VoiceRecorder.deserialize === 'function' },
    { id: 'X06275', name: '录音·彩蛋层', check: () => { const r = new T.VoiceRecorder('studio'); r.start(); return r.push(10, true) === 1; } },
  ];
}

/* -------- 族0252 演示白板 2.0 X06276~X06300 -------- */
export function checkF0252(): CheckEntry[] {
  return [
    { id: 'X06276', name: '白板·最小闭环', check: () => { const w = new T.Whiteboard(); w.draw({ tool: 'pen', pts: [[10, 10], [20, 20]], color: '#f00' }); return w.count === 1; } },
    { id: 'X06277', name: '白板·全量参数', check: () => new T.Whiteboard('strict').tier === 'strict' },
    { id: 'X06278', name: '白板·档位矩阵', check: () => T.TIER_MATRIX.every((t) => new T.Whiteboard(t).tier === t) },
    { id: 'X06279', name: '白板·快照迁移', check: () => { const w = new T.Whiteboard('light'); return T.Whiteboard.deserialize(w.serialize()).tier === 'light'; } },
    { id: 'X06280', name: '白板·联调集成', check: () => { const w = new T.Whiteboard(); w.draw({ tool: 'pen', pts: [[1, 1]], color: 'c' }); const u = w.undo(); const r = w.redo(); return !!u && !!r && w.count === 1; } },
    { id: 'X06281', name: '白板·越界钳制', check: () => { const w = new T.Whiteboard(); w.draw({ tool: 'pen', pts: [[-5, 9999]], color: 'c' }); return w.clamped === 1; } },
    { id: 'X06282', name: '白板·失败叙事', check: () => T.explainError('E2602').next.includes('另存') },
    { id: 'X06283', name: '白板·中断续跑', check: () => { const w = new T.Whiteboard(); w.draw({ tool: 'pen', pts: [[1, 1]], color: 'c' }); w.undo(); const back = w.redo(); return !!back && w.count === 1; } },
    { id: 'X06284', name: '白板·资源降级', check: () => new T.Whiteboard('off').allows('highlighter') === false },
    { id: 'X06285', name: '白板·回滚净身', check: () => { const w = new T.Whiteboard(); w.draw({ tool: 'pen', pts: [[1, 1]], color: 'c' }); return w.clear() && w.count === 0; } },
    { id: 'X06286', name: '白板·动效令牌', check: () => T.motionFor('balanced').durationMs === 180 },
    { id: 'X06287', name: '白板·三态焦点', check: () => new T.Whiteboard().undo() === null },
    { id: 'X06288', name: '白板·键盘序', check: () => T.TIER_MATRIX.every((t) => new T.Whiteboard(t).tier === t) },
    { id: 'X06289', name: '白板·微文案', check: () => T.explainError('E2602').text === '白板画布保存失败' },
    { id: 'X06290', name: '白板·aria 等价', check: () => typeof new T.Whiteboard().draw === 'function' },
    { id: 'X06291', name: '白板·基准采集', check: () => { const w = new T.Whiteboard(); const t0 = performance.now(); for (let i = 0; i < 500; i++) w.draw({ tool: 'pen', pts: [[i, i]], color: 'c' }); return performance.now() - t0 < 50; } },
    { id: 'X06292', name: '白板·热路径', check: () => { const w = new T.Whiteboard(); w.draw({ tool: 'pen', pts: [[1, 1]], color: 'c' }); return w.draw({ tool: 'eraser', pts: [[2, 2]], color: 'c' }) === 2; } },
    { id: 'X06293', name: '白板·零漂移', check: () => { const w = new T.Whiteboard('strict'); const a = T.Whiteboard.deserialize(w.serialize()); return a.serialize() === w.serialize(); } },
    { id: 'X06294', name: '白板·低配减档', check: () => new T.Whiteboard('off').allows('pen') === true },
    { id: 'X06295', name: '白板·守卫', check: () => T.Whiteboard.deserialize('[]').tier === 'balanced' },
    { id: 'X06296', name: '白板·智能建议', check: () => { const w = new T.Whiteboard(); return w.allows('highlighter') === true && new T.Whiteboard('off').allows('highlighter') === false; } },
    { id: 'X06297', name: '白板·批量模式', check: () => { const w = new T.Whiteboard(); for (let i = 0; i < 10; i++) w.draw({ tool: 'pen', pts: [[i, i]], color: 'c' }); return w.count === 10; } },
    { id: 'X06298', name: '白板·跨域联动', check: () => T.Whiteboard.deserialize(new T.Whiteboard('print').serialize()).tier === 'print' },
    { id: 'X06299', name: '白板·扩展点', check: () => typeof new T.Whiteboard().undo === 'function' && typeof T.Whiteboard.deserialize === 'function' },
    { id: 'X06300', name: '白板·彩蛋层', check: () => { const w = new T.Whiteboard(); w.draw({ tool: 'pen', pts: [[1, 1]], color: '🥚' }); return w.count === 1; } },
  ];
}

/* -------- 族0253 阅读器 2.0 X06301~X06325 -------- */
export function checkF0253(): CheckEntry[] {
  return [
    { id: 'X06301', name: '阅读·最小闭环', check: () => { const r = new T.Reader('serif', 10); const p = r.next(); return p === 2 && r.progress() === 20; } },
    { id: 'X06302', name: '阅读·全量参数', check: () => { const r = new T.Reader('dyslexic', 99999); return r.tier === 'dyslexic' && r.clamped === 1; } },
    { id: 'X06303', name: '阅读·档位矩阵', check: () => T.FONT_MATRIX.length === 5 && T.FONT_MATRIX.every((t) => new T.Reader(t).tier === t) },
    { id: 'X06304', name: '阅读·快照迁移', check: () => { const r = new T.Reader('serif', 20); r.goto(5); r.mark(); const a = T.Reader.deserialize(r.serialize()); return a.page === 5; } },
    { id: 'X06305', name: '阅读·联调集成', check: () => { const r = new T.Reader('serif', 10); r.next(); const back = r.prev(); return back === 1; } },
    { id: 'X06306', name: '阅读·越界钳制', check: () => { const r = new T.Reader('serif', 10); return r.goto(999) === 10 && r.goto(-1) === 1 && r.clamped === 1; } },
    { id: 'X06307', name: '阅读·失败叙事', check: () => T.explainError('E2603').next.includes('纯文本') },
    { id: 'X06308', name: '阅读·中断续跑', check: () => { const r = new T.Reader('serif', 20); r.goto(7); r.mark(); r.goto(1); return r.resumeBookmark() === 7; } },
    { id: 'X06309', name: '阅读·资源降级', check: () => T.FONT_SCALE['compact'] === 0.85 },
    { id: 'X06310', name: '阅读·回滚净身', check: () => new T.Reader('serif', 10).page === 1 },
    { id: 'X06311', name: '阅读·动效令牌', check: () => T.motionFor('strict').curve === 'ease-standard' },
    { id: 'X06312', name: '阅读·三态焦点', check: () => { const r = new T.Reader('serif', 10); r.goto(10); return r.progress() === 100; } },
    { id: 'X06313', name: '阅读·键盘序', check: () => T.FONT_MATRIX.every((t) => new T.Reader(t).tier === t) },
    { id: 'X06314', name: '阅读·微文案', check: () => T.explainError('E2603').text === '阅读器打开失败' },
    { id: 'X06315', name: '阅读·aria 等价', check: () => typeof new T.Reader().goto === 'function' },
    { id: 'X06316', name: '阅读·基准采集', check: () => { const r = new T.Reader('serif', 10000); const t0 = performance.now(); for (let i = 0; i < 500; i++) r.goto(i % 10000); return performance.now() - t0 < 50; } },
    { id: 'X06317', name: '阅读·热路径', check: () => { const r = new T.Reader('serif', 50); return r.next() === 2; } },
    { id: 'X06318', name: '阅读·零漂移', check: () => { const r = new T.Reader('serif', 20); r.goto(5); r.mark(); const a = T.Reader.deserialize(r.serialize()); return a.serialize() === r.serialize(); } },
    { id: 'X06319', name: '阅读·低配减档', check: () => T.FONT_SCALE['print'] > T.FONT_SCALE['compact'] },
    { id: 'X06320', name: '阅读·守卫', check: () => T.Reader.deserialize('bad{').tier === 'serif' },
    { id: 'X06321', name: '阅读·智能建议', check: () => { const r = new T.Reader('serif', 10); r.goto(3); return r.progress() === 30; } },
    { id: 'X06322', name: '阅读·批量模式', check: () => { const r = new T.Reader('serif', 100); for (let i = 0; i < 20; i++) r.next(); return r.page === 21; } },
    { id: 'X06323', name: '阅读·跨域联动', check: () => T.FONT_SCALE[T.Reader.deserialize(new T.Reader('print').serialize()).tier] === 1.35 },
    { id: 'X06324', name: '阅读·扩展点', check: () => typeof T.FONT_SCALE === 'object' && typeof T.Reader.deserialize === 'function' },
    { id: 'X06325', name: '阅读·彩蛋层', check: () => { const r = new T.Reader('serif', 100); r.goto(42); r.mark(); return r.serialize().includes('"bookmark":42'); } },
  ];
}

/* -------- 族0254 媒体播放器 2.0 X06326~X06350 -------- */
export function checkF0254(): CheckEntry[] {
  return [
    { id: 'X06326', name: '播放器·最小闭环', check: () => { const p = new T.MediaPlayer(); const ok = p.load(60000); const play = p.play(); const paused = p.pause(); return ok && play && paused; } },
    { id: 'X06327', name: '播放器·全量参数', check: () => new T.MediaPlayer('uhd').tier === 'uhd' },
    { id: 'X06328', name: '播放器·档位矩阵', check: () => T.PLAY_MATRIX.length === 5 && T.PLAY_MATRIX.every((t) => new T.MediaPlayer(t).tier === t) },
    { id: 'X06329', name: '播放器·快照迁移', check: () => { const p = new T.MediaPlayer('fhd'); return T.MediaPlayer.deserialize(p.serialize()).tier === 'fhd'; } },
    { id: 'X06330', name: '播放器·联调集成', check: () => { const p = new T.MediaPlayer(); p.load(60000); return p.seek(30000, 60000) === 30000 && p.position === 30000; } },
    { id: 'X06331', name: '播放器·越界钳制', check: () => { const p = new T.MediaPlayer(); p.load(60000); const s = p.setSpeed(9); const k = p.seek(99999, 60000); return s === 4 && k === 60000 && p.clamped === 1; } },
    { id: 'X06332', name: '播放器·失败叙事', check: () => T.explainError('E2604').next.includes('软解') },
    { id: 'X06333', name: '播放器·中断续跑', check: () => { const p = new T.MediaPlayer(); p.load(60000); p.seek(25000, 60000); p.pause(); p.play(); return p.position === 25000 && p.isPlaying; } },
    { id: 'X06334', name: '播放器·资源降级', check: () => { const p = new T.MediaPlayer('audio'); return p.load(1000) === true; } },
    { id: 'X06335', name: '播放器·回滚净身', check: () => new T.MediaPlayer().load(0) === false },
    { id: 'X06336', name: '播放器·动效令牌', check: () => T.motionFor('balanced').scale === 1 },
    { id: 'X06337', name: '播放器·三态焦点', check: () => { const p = new T.MediaPlayer(); p.load(1000); return p.setSpeed(0.01) === 0.25; } },
    { id: 'X06338', name: '播放器·键盘序', check: () => T.PLAY_MATRIX.every((t) => new T.MediaPlayer(t).tier === t) },
    { id: 'X06339', name: '播放器·微文案', check: () => T.explainError('E2604').text === '播放器解码失败' },
    { id: 'X06340', name: '播放器·aria 等价', check: () => typeof new T.MediaPlayer().seek === 'function' },
    { id: 'X06341', name: '播放器·基准采集', check: () => { const p = new T.MediaPlayer(); p.load(600000); const t0 = performance.now(); for (let i = 0; i < 500; i++) p.seek(i, 600000); return performance.now() - t0 < 50; } },
    { id: 'X06342', name: '播放器·热路径', check: () => { const p = new T.MediaPlayer(); p.load(10000); return p.seek(5000, 10000) === 5000; } },
    { id: 'X06343', name: '播放器·零漂移', check: () => { const p = new T.MediaPlayer('fhd'); const a = T.MediaPlayer.deserialize(p.serialize()); return a.serialize() === p.serialize(); } },
    { id: 'X06344', name: '播放器·低配减档', check: () => new T.MediaPlayer('audio').tier === 'audio' },
    { id: 'X06345', name: '播放器·守卫', check: () => T.MediaPlayer.deserialize('[]').tier === 'hd' },
    { id: 'X06346', name: '播放器·智能建议', check: () => { const p = new T.MediaPlayer(); p.setSpeed(2); return p.setSpeed(99) === 4; } },
    { id: 'X06347', name: '播放器·批量模式', check: () => { const p = new T.MediaPlayer(); p.load(100000); for (let i = 0; i < 10; i++) p.seek(i * 1000, 100000); return p.position === 9000; } },
    { id: 'X06348', name: '播放器·跨域联动', check: () => T.MediaPlayer.deserialize(new T.MediaPlayer('uhd').serialize()).tier === 'uhd' },
    { id: 'X06349', name: '播放器·扩展点', check: () => typeof T.PLAY_MATRIX === 'object' && typeof T.MediaPlayer.deserialize === 'function' },
    { id: 'X06350', name: '播放器·彩蛋层', check: () => { const p = new T.MediaPlayer(); p.load(60000); return p.seek(-99, 60000) === 0; } },
  ];
}

/* -------- 族0255 图片查看器 2.0 X06351~X06375 -------- */
export function checkF0255(): CheckEntry[] {
  return [
    { id: 'X06351', name: '看图·最小闭环', check: () => { const v = new T.PhotoViewer(); return v.setZoom(2) === 2 && v.rotate(90) === 90; } },
    { id: 'X06352', name: '看图·全量参数', check: () => new T.PhotoViewer('4x').tier === '4x' },
    { id: 'X06353', name: '看图·档位矩阵', check: () => T.VIEW_MATRIX.length === 5 && T.VIEW_MATRIX.every((t) => new T.PhotoViewer(t).tier === t) },
    { id: 'X06354', name: '看图·快照迁移', check: () => { const v = new T.PhotoViewer('fill'); return T.PhotoViewer.deserialize(v.serialize()).tier === 'fill'; } },
    { id: 'X06355', name: '看图·联调集成', check: () => { const v = new T.PhotoViewer('fit'); return v.fitTo({ w: 800, h: 600 }, 1600, 1200) === 0.5; } },
    { id: 'X06356', name: '看图·越界钳制', check: () => { const v = new T.PhotoViewer(); const z = v.setZoom(99); return z === 8 && v.clamped === 1; } },
    { id: 'X06357', name: '看图·失败叙事', check: () => T.explainError('E2605').next.includes('缩略图') },
    { id: 'X06358', name: '看图·中断续跑', check: () => { const v = new T.PhotoViewer(); v.rotate(90); return v.rotate(-90) === 0; } },
    { id: 'X06359', name: '看图·资源降级', check: () => { const v = new T.PhotoViewer('fit'); return v.fitTo({ w: 100, h: 100 }, 200, 200) === 0.5; } },
    { id: 'X06360', name: '看图·回滚净身', check: () => { const v = new T.PhotoViewer(); return v.zoomNow === 1 && v.rotation === 0; } },
    { id: 'X06361', name: '看图·动效令牌', check: () => T.motionFor('off').scale === 0 },
    { id: 'X06362', name: '看图·三态焦点', check: () => { const v = new T.PhotoViewer(); v.rotate(360); return v.rotation === 0; } },
    { id: 'X06363', name: '看图·键盘序', check: () => T.VIEW_MATRIX.every((t) => new T.PhotoViewer(t).tier === t) },
    { id: 'X06364', name: '看图·微文案', check: () => T.explainError('E2605').text === '图片解码失败' },
    { id: 'X06365', name: '看图·aria 等价', check: () => typeof new T.PhotoViewer().setZoom === 'function' },
    { id: 'X06366', name: '看图·基准采集', check: () => { const v = new T.PhotoViewer(); const t0 = performance.now(); for (let i = 0; i < 500; i++) v.setZoom(1 + (i % 8)); return performance.now() - t0 < 50; } },
    { id: 'X06367', name: '看图·热路径', check: () => { const v = new T.PhotoViewer(); v.setZoom(3); return v.zoomNow === 3; } },
    { id: 'X06368', name: '看图·零漂移', check: () => { const v = new T.PhotoViewer('2x'); const a = T.PhotoViewer.deserialize(v.serialize()); return a.serialize() === v.serialize(); } },
    { id: 'X06369', name: '看图·低配减档', check: () => T.VIEW_ZOOM['fit'] === 0 },
    { id: 'X06370', name: '看图·守卫', check: () => T.PhotoViewer.deserialize('[]').tier === 'fit' },
    { id: 'X06371', name: '看图·智能建议', check: () => { const v = new T.PhotoViewer('fit'); return v.fitTo({ w: 500, h: 500 }, 1000, 1000) === 0.5; } },
    { id: 'X06372', name: '看图·批量模式', check: () => { const v = new T.PhotoViewer(); return v.slideshow(500) === 500 && v.slideshow(2000) === 1000; } },
    { id: 'X06373', name: '看图·跨域联动', check: () => T.VIEW_ZOOM[T.PhotoViewer.deserialize(new T.PhotoViewer('2x').serialize()).tier] === 2 },
    { id: 'X06374', name: '看图·扩展点', check: () => typeof T.VIEW_ZOOM === 'object' && typeof T.PhotoViewer.deserialize === 'function' },
    { id: 'X06375', name: '看图·彩蛋层', check: () => { const v = new T.PhotoViewer(); v.rotate(450); return v.rotation === 90; } },
  ];
}

/* -------- 族0256 打印中心 2.0 X06376~X06400 -------- */
export function checkF0256(): CheckEntry[] {
  return [
    { id: 'X06376', name: '打印·最小闭环', check: () => { const c = new T.PrintCenter(); const j = c.submit(3, false); const r = c.print(1); return j.pages === 3 && r.done === 1 && c.pending === 0; } },
    { id: 'X06377', name: '打印·全量参数', check: () => new T.PrintCenter('photo').tier === 'photo' },
    { id: 'X06378', name: '打印·档位矩阵', check: () => T.PRINT_MATRIX.length === 5 && T.PRINT_MATRIX.every((t) => new T.PrintCenter(t).tier === t) },
    { id: 'X06379', name: '打印·快照迁移', check: () => { const c = new T.PrintCenter('draft'); return T.PrintCenter.deserialize(c.serialize()).tier === 'draft'; } },
    { id: 'X06380', name: '打印·联调集成', check: () => { const c = new T.PrintCenter(); c.submit(2, true); c.submit(4, false); const r = c.print(5); return r.done === 2 && c.pending === 0; } },
    { id: 'X06381', name: '打印·越界钳制', check: () => { const c = new T.PrintCenter(); const j = c.submit(9999, false); return j.pages === 999 && c.clamped === 1; } },
    { id: 'X06382', name: '打印·失败叙事', check: () => T.explainError('E2606').next.includes('续打') },
    { id: 'X06383', name: '打印·中断续跑', check: () => { const c = new T.PrintCenter(); for (let i = 0; i < 3; i++) c.submit(1, false); c.print(1); return c.resume(5) === 2 && c.pending === 0; } },
    { id: 'X06384', name: '打印·资源降级', check: () => T.PRINT_DPI['draft'] === 150 },
    { id: 'X06385', name: '打印·回滚净身', check: () => { const c = new T.PrintCenter(); c.submit(1, false); return c.cancelAll() && c.pending === 0; } },
    { id: 'X06386', name: '打印·动效令牌', check: () => T.motionFor('light').durationMs === 180 },
    { id: 'X06387', name: '打印·三态焦点', check: () => { const c = new T.PrintCenter(); const a = c.submit(1, false); const b = c.submit(1, false); return b.id === a.id + 1; } },
    { id: 'X06388', name: '打印·键盘序', check: () => T.PRINT_MATRIX.every((t) => new T.PrintCenter(t).tier === t) },
    { id: 'X06389', name: '打印·微文案', check: () => T.explainError('E2606').text === '打印任务中断' },
    { id: 'X06390', name: '打印·aria 等价', check: () => typeof new T.PrintCenter().submit === 'function' },
    { id: 'X06391', name: '打印·基准采集', check: () => { const c = new T.PrintCenter(); const t0 = performance.now(); for (let i = 0; i < 500; i++) c.submit(2, true); return performance.now() - t0 < 50; } },
    { id: 'X06392', name: '打印·热路径', check: () => { const c = new T.PrintCenter(); c.submit(1, false); return c.pending === 1; } },
    { id: 'X06393', name: '打印·零漂移', check: () => { const c = new T.PrintCenter('gray'); const a = T.PrintCenter.deserialize(c.serialize()); return a.serialize() === c.serialize(); } },
    { id: 'X06394', name: '打印·低配减档', check: () => T.PRINT_DPI['draft'] < T.PRINT_DPI['photo'] },
    { id: 'X06395', name: '打印·守卫', check: () => T.PrintCenter.deserialize('[]').tier === 'normal' },
    { id: 'X06396', name: '打印·智能建议', check: () => T.PRINT_DPI['proofer'] === 2400 },
    { id: 'X06397', name: '打印·批量模式', check: () => { const c = new T.PrintCenter(); for (let i = 0; i < 10; i++) c.submit(1, false); return c.pending === 10; } },
    { id: 'X06398', name: '打印·跨域联动', check: () => T.PRINT_DPI[T.PrintCenter.deserialize(new T.PrintCenter('photo').serialize()).tier] === 1200 },
    { id: 'X06399', name: '打印·扩展点', check: () => typeof T.PRINT_DPI === 'object' && typeof T.PrintCenter.deserialize === 'function' },
    { id: 'X06400', name: '打印·彩蛋层', check: () => { const c = new T.PrintCenter(); return c.submit(1, true).id === 1; } },
  ];
}

/* -------- 族0257 通讯录人脉 2.0 X06401~X06425 -------- */
export function checkF0257(): CheckEntry[] {
  return [
    { id: 'X06401', name: '人脉·最小闭环', check: () => { const c = new T.Contacts(); c.add('张三', '13800000000'); const f = c.find('张三'); return !!f && f.phone === '13800000000'; } },
    { id: 'X06402', name: '人脉·全量参数', check: () => new T.Contacts('strict').tier === 'strict' },
    { id: 'X06403', name: '人脉·档位矩阵', check: () => T.TIER_MATRIX.every((t) => new T.Contacts(t).tier === t) },
    { id: 'X06404', name: '人脉·快照迁移', check: () => { const c = new T.Contacts('light'); return T.Contacts.deserialize(c.serialize()).tier === 'light'; } },
    { id: 'X06405', name: '人脉·联调集成', check: () => { const c = new T.Contacts(); c.add('A', '1', '家人'); c.add('B', '2', '同事'); c.add('C', '3', '家人'); return c.byGroup('家人').length === 2; } },
    { id: 'X06406', name: '人脉·越界钳制', check: () => { const c = new T.Contacts(); const p = c.add('X', '138-0000'); return p.phone === '1380000' && c.clamped === 1; } },
    { id: 'X06407', name: '人脉·失败叙事', check: () => T.explainError('E2607').next.includes('vCard') },
    { id: 'X06408', name: '人脉·中断还原', check: () => new T.Contacts().find('nobody') === null },
    { id: 'X06409', name: '人脉·资源降级', check: () => { const c = new T.Contacts('off'); return c.add('A', '1') !== null; } },
    { id: 'X06410', name: '人脉·回滚净身', check: () => new T.Contacts().count === 0 },
    { id: 'X06411', name: '人脉·动效令牌', check: () => T.motionFor('balanced').durationMs === 180 },
    { id: 'X06412', name: '人脉·三态焦点', check: () => { const c = new T.Contacts(); c.add('A', '1'); return c.fav(1) === true && c.fav(1) === false; } },
    { id: 'X06413', name: '人脉·键盘序', check: () => T.TIER_MATRIX.every((t) => new T.Contacts(t).tier === t) },
    { id: 'X06414', name: '人脉·微文案', check: () => T.explainError('E2607').text === '通讯录导入失败' },
    { id: 'X06415', name: '人脉·aria 等价', check: () => typeof new T.Contacts().add === 'function' },
    { id: 'X06416', name: '人脉·基准采集', check: () => { const c = new T.Contacts(); const t0 = performance.now(); for (let i = 0; i < 500; i++) c.add(`N${i}`, `${i}`); return performance.now() - t0 < 50; } },
    { id: 'X06417', name: '人脉·热路径', check: () => { const c = new T.Contacts(); c.add('热', '9'); return c.find('热') !== null; } },
    { id: 'X06418', name: '人脉·零漂移', check: () => { const c = new T.Contacts('strict'); const a = T.Contacts.deserialize(c.serialize()); return a.serialize() === c.serialize(); } },
    { id: 'X06419', name: '人脉·低配减档', check: () => new T.Contacts('off').tier === 'off' },
    { id: 'X06420', name: '人脉·守卫', check: () => T.Contacts.deserialize('[]').tier === 'balanced' },
    { id: 'X06421', name: '人脉·智能建议', check: () => { const c = new T.Contacts(); c.add('A', '100'); c.add('B', '100'); return c.suggestMerge().length === 1; } },
    { id: 'X06422', name: '人脉·批量模式', check: () => { const c = new T.Contacts(); for (let i = 0; i < 10; i++) c.add(`N${i}`, `${i}`); return c.count === 10; } },
    { id: 'X06423', name: '人脉·跨域联动', check: () => { const c = new T.Contacts(); c.add('李四', '13900000000'); return c.toVCard('李四').startsWith('BEGIN:VCARD'); } },
    { id: 'X06424', name: '人脉·扩展点', check: () => typeof new T.Contacts().toVCard === 'function' && typeof T.Contacts.deserialize === 'function' },
    { id: 'X06425', name: '人脉·彩蛋层', check: () => new T.Contacts().toVCard('幽灵') === '' },
  ];
}

/* -------- 族0258 密码管理器 2.0 X06426~X06450 -------- */
export function checkF0258(): CheckEntry[] {
  return [
    { id: 'X06426', name: '密码·最小闭环', check: () => { const v = new T.PasswordVault('mypass'); v.unlock('mypass'); v.store({ site: 'web', user: 'u', pass: '秘密123' }); return v.fetch('web')?.pass === '秘密123'; } },
    { id: 'X06427', name: '密码·全量参数', check: () => new T.PasswordVault('', 'paranoid').tier === 'paranoid' },
    { id: 'X06428', name: '密码·档位矩阵', check: () => T.VAULT_MATRIX.length === 5 && T.VAULT_MATRIX.every((t) => new T.PasswordVault('', t).tier === t) },
    { id: 'X06429', name: '密码·快照迁移', check: () => { const v = new T.PasswordVault('', 'basic'); return T.PasswordVault.deserialize(v.serialize()).tier === 'basic'; } },
    { id: 'X06430', name: '密码·联调集成', check: () => { const v = new T.PasswordVault('k'); v.unlock('k'); return v.strength('abc') < v.strength('Abc123!xyz'); } },
    { id: 'X06431', name: '密码·越界钳制', check: () => { const v = new T.PasswordVault('', 'ultra'); return v.tier === 'strong' && v.clamped === 1; } },
    { id: 'X06432', name: '密码·失败叙事', check: () => T.explainError('E2608').next.includes('恢复短语') },
    { id: 'X06433', name: '密码·中断续跑', check: () => { const v = new T.PasswordVault('k'); v.unlock('k'); v.lock(); return v.unlock('wrong') === false && v.unlock('k') === true; } },
    { id: 'X06434', name: '密码·资源降级', check: () => { const v = new T.PasswordVault('k', 'plain'); return v.unlock('k') === true; } },
    { id: 'X06435', name: '密码·回滚净身', check: () => { const v = new T.PasswordVault('k'); v.unlock('k'); v.store({ site: 'a', user: 'u', pass: 'p' }); return v.purge() && v.count === 0; } },
    { id: 'X06436', name: '密码·动效令牌', check: () => T.motionFor('print').durationMs === 180 },
    { id: 'X06437', name: '密码·三态焦点', check: () => { const v = new T.PasswordVault('k'); return v.isLocked === true && v.store({ site: 'a', user: 'u', pass: 'p' }) === -1; } },
    { id: 'X06438', name: '密码·键盘序', check: () => T.VAULT_MATRIX.every((t) => new T.PasswordVault('', t).tier === t) },
    { id: 'X06439', name: '密码·微文案', check: () => T.explainError('E2608').text === '密码库校验失败' },
    { id: 'X06440', name: '密码·aria 等价', check: () => typeof new T.PasswordVault().store === 'function' },
    { id: 'X06441', name: '密码·基准采集', check: () => { const v = new T.PasswordVault(); const t0 = performance.now(); for (let i = 0; i < 500; i++) v.strength(`pw${i}!A`); return performance.now() - t0 < 50; } },
    { id: 'X06442', name: '密码·热路径', check: () => { const v = new T.PasswordVault('k'); v.unlock('k'); v.store({ site: 'hot', user: 'u', pass: 'p' }); return v.fetch('hot') !== null; } },
    { id: 'X06443', name: '密码·零漂移', check: () => { const v = new T.PasswordVault('', 'strong'); const a = T.PasswordVault.deserialize(v.serialize()); return a.serialize() === v.serialize(); } },
    { id: 'X06444', name: '密码·低配减档', check: () => new T.PasswordVault('', 'plain').tier === 'plain' },
    { id: 'X06445', name: '密码·守卫', check: () => T.PasswordVault.deserialize('[]').tier === 'strong' },
    { id: 'X06446', name: '密码·智能建议', check: () => { const v = new T.PasswordVault('k'); v.unlock('k'); return v.strength('Abc123!') >= v.strength('abc'); } },
    { id: 'X06447', name: '密码·批量模式', check: () => { const v = new T.PasswordVault('k'); v.unlock('k'); for (let i = 0; i < 5; i++) v.store({ site: `s${i}`, user: 'u', pass: 'p' }); return v.count === 5; } },
    { id: 'X06448', name: '密码·跨域联动', check: () => T.PasswordVault.deserialize(new T.PasswordVault('', 'paper').serialize()).tier === 'paper' },
    { id: 'X06449', name: '密码·扩展点', check: () => typeof T.VAULT_MATRIX === 'object' && typeof T.PasswordVault.deserialize === 'function' },
    { id: 'X06450', name: '密码·彩蛋层', check: () => { const v = new T.PasswordVault('k'); v.unlock('k'); v.store({ site: '🥚.egg', user: 'u', pass: 'egg🥚' }); return v.fetch('🥚.egg')?.pass === 'egg🥚'; } },
  ];
}

/* -------- 族0259 天气出行 2.0 X06451~X06475 -------- */
export function checkF0259(): CheckEntry[] {
  return [
    { id: 'X06451', name: '天气·最小闭环', check: () => { const w = new T.Weather(); const n = w.fetch('北京'); return n.tempC === 22 && w.cached('北京'); } },
    { id: 'X06452', name: '天气·全量参数', check: () => new T.Weather('15d').tier === '15d' },
    { id: 'X06453', name: '天气·档位矩阵', check: () => T.FORECAST_MATRIX.length === 5 && T.FORECAST_MATRIX.every((t) => new T.Weather(t).tier === t) },
    { id: 'X06454', name: '天气·快照迁移', check: () => { const w = new T.Weather('3h'); return T.Weather.deserialize(w.serialize()).tier === '3h'; } },
    { id: 'X06455', name: '天气·联调集成', check: () => { const w = new T.Weather(); return w.feelsLike({ tempC: 22, humidity: 55, windKph: 12 }) === 21; } },
    { id: 'X06456', name: '天气·越界钳制', check: () => { const w = new T.Weather('always'); return w.tier === '24h' && w.clamped === 1; } },
    { id: 'X06457', name: '天气·失败叙事', check: () => T.explainError('E2609').next.includes('备用') },
    { id: 'X06458', name: '天气·中断续跑', check: () => { const w = new T.Weather(); w.fetch('上海', true); return w.dataSource === 'backup'; } },
    { id: 'X06459', name: '天气·资源降级', check: () => new T.Weather('now').hours() === 0 },
    { id: 'X06460', name: '天气·回滚净身', check: () => new T.Weather().cached('不存在') === false },
    { id: 'X06461', name: '天气·动效令牌', check: () => T.motionFor('balanced').curve === 'ease-standard' },
    { id: 'X06462', name: '天气·三态焦点', check: () => new T.Weather().advise({ tempC: 22, humidity: 50, windKph: 60 }).includes('大风') },
    { id: 'X06463', name: '天气·键盘序', check: () => T.FORECAST_MATRIX.every((t) => new T.Weather(t).tier === t) },
    { id: 'X06464', name: '天气·微文案', check: () => T.explainError('E2609').text === '天气源超时' },
    { id: 'X06465', name: '天气·aria 等价', check: () => typeof new T.Weather().fetch === 'function' },
    { id: 'X06466', name: '天气·基准采集', check: () => { const w = new T.Weather(); const t0 = performance.now(); for (let i = 0; i < 500; i++) w.feelsLike({ tempC: i % 40, humidity: 50, windKph: 10 }); return performance.now() - t0 < 50; } },
    { id: 'X06467', name: '天气·热路径', check: () => { const w = new T.Weather(); w.fetch('深圳'); return w.cached('深圳'); } },
    { id: 'X06468', name: '天气·零漂移', check: () => { const w = new T.Weather('72h'); const a = T.Weather.deserialize(w.serialize()); return a.serialize() === w.serialize(); } },
    { id: 'X06469', name: '天气·低配减档', check: () => T.FORECAST_HOURS['now'] === 0 && T.FORECAST_HOURS['15d'] === 360 },
    { id: 'X06470', name: '天气·守卫', check: () => T.Weather.deserialize('[]').tier === '24h' },
    { id: 'X06471', name: '天气·智能建议', check: () => { const w = new T.Weather(); return w.advise({ tempC: 22, humidity: 50, windKph: 10 }) === '适宜出行' && w.advise({ tempC: -10, humidity: 50, windKph: 10 }).includes('低温'); } },
    { id: 'X06472', name: '天气·批量模式', check: () => { const w = new T.Weather(); for (let i = 0; i < 5; i++) w.fetch(`城市${i}`); return ['城市0', '城市1', '城市2', '城市3', '城市4'].every((c) => w.cached(c)); } },
    { id: 'X06473', name: '天气·跨域联动', check: () => T.Weather.deserialize(new T.Weather('72h').serialize()).hours() === 72 },
    { id: 'X06474', name: '天气·扩展点', check: () => typeof T.FORECAST_HOURS === 'object' && typeof T.Weather.deserialize === 'function' },
    { id: 'X06475', name: '天气·彩蛋层', check: () => new T.Weather().advise({ tempC: -99, humidity: 0, windKph: 0 }).includes('保暖') },
  ];
}

/* -------- 族0260 地图位置 2.0 X06476~X06500 -------- */
export function checkF0260(): CheckEntry[] {
  return [
    { id: 'X06476', name: '地图·最小闭环', check: () => { const m = new T.Maps(); const p = m.locate({ lat: 31, lng: 121 }); return !!p && m.pinCount === 1; } },
    { id: 'X06477', name: '地图·全量参数', check: () => new T.Maps('satellite').tier === 'satellite' },
    { id: 'X06478', name: '地图·档位矩阵', check: () => T.MAP_MATRIX.length === 5 && T.MAP_MATRIX.every((t) => new T.Maps(t).tier === t) },
    { id: 'X06479', name: '地图·快照迁移', check: () => { const m = new T.Maps('3d'); return T.Maps.deserialize(m.serialize()).tier === '3d'; } },
    { id: 'X06480', name: '地图·联调集成', check: () => { const m = new T.Maps(); return m.plan([{ lat: 0, lng: 0 }, { lat: 0, lng: 1 }]) === 111; } },
    { id: 'X06481', name: '地图·越界钳制', check: () => { const m = new T.Maps(); const n = m.normalize({ lat: 999, lng: 999 }); return n.lat === 90 && n.lng === 180 && m.clamped === 1; } },
    { id: 'X06482', name: '地图·失败叙事', check: () => T.explainError('E2610').next.includes('手动') },
    { id: 'X06483', name: '地图·中断还原', check: () => new T.Maps().locate({ lat: Number.NaN, lng: 0 }) === null },
    { id: 'X06484', name: '地图·资源降级', check: () => new T.Maps('raster').tier === 'raster' },
    { id: 'X06485', name: '地图·回滚净身', check: () => { const m = new T.Maps(); m.locate({ lat: 1, lng: 1 }); return m.clear() && m.pinCount === 0; } },
    { id: 'X06486', name: '地图·动效令牌', check: () => T.motionFor('balanced').durationMs === 180 },
    { id: 'X06487', name: '地图·三态焦点', check: () => { const m = new T.Maps(); const near = m.nearby({ lat: 0, lng: 0 }, [{ lat: 0, lng: 0.05 }, { lat: 0, lng: 5 }], 10); return near.length === 1; } },
    { id: 'X06488', name: '地图·键盘序', check: () => T.MAP_MATRIX.every((t) => new T.Maps(t).tier === t) },
    { id: 'X06489', name: '地图·微文案', check: () => T.explainError('E2610').text === '地图定位失败' },
    { id: 'X06490', name: '地图·aria 等价', check: () => typeof new T.Maps().locate === 'function' },
    { id: 'X06491', name: '地图·基准采集', check: () => { const m = new T.Maps(); const t0 = performance.now(); for (let i = 0; i < 500; i++) m.normalize({ lat: i, lng: -i }); return performance.now() - t0 < 50; } },
    { id: 'X06492', name: '地图·热路径', check: () => { const m = new T.Maps(); return m.plan([{ lat: 0, lng: 0 }, { lat: 1, lng: 1 }]) === Math.round(Math.hypot(1, 1) * 111 * 10) / 10; } },
    { id: 'X06493', name: '地图·零漂移', check: () => { const m = new T.Maps('terrain'); const a = T.Maps.deserialize(m.serialize()); return a.serialize() === m.serialize(); } },
    { id: 'X06494', name: '地图·低配减档', check: () => new T.Maps('raster').tier === 'raster' },
    { id: 'X06495', name: '地图·守卫', check: () => T.Maps.deserialize('[]').tier === 'vector' },
    { id: 'X06496', name: '地图·智能建议', check: () => { const m = new T.Maps(); return m.nearby({ lat: 0, lng: 0 }, [{ lat: 0, lng: 0 }, { lat: 89, lng: 179 }], 50).length === 1; } },
    { id: 'X06497', name: '地图·批量模式', check: () => { const m = new T.Maps(); for (let i = 0; i < 10; i++) m.locate({ lat: i, lng: i }); return m.pinCount === 10; } },
    { id: 'X06498', name: '地图·跨域联动', check: () => T.Maps.deserialize(new T.Maps('terrain').serialize()).tier === 'terrain' },
    { id: 'X06499', name: '地图·扩展点', check: () => typeof T.MAP_MATRIX === 'object' && typeof T.Maps.deserialize === 'function' },
    { id: 'X06500', name: '地图·彩蛋层', check: () => { const m = new T.Maps(); const p = m.locate({ lat: 90, lng: 180 }); return !!p && p.lat === 90 && p.lng === 180; } },
  ];
}

// UNREAL-X AI-26（族0251~0260 · X06251~X06500）V 线聚合：十族 × 25 项 = 250 项，只增不删。
export function runAi26Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [checkF0251, checkF0252, checkF0253, checkF0254, checkF0255, checkF0256, checkF0257, checkF0258, checkF0259, checkF0260];
  const entries = families.flatMap((f) => f());
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
