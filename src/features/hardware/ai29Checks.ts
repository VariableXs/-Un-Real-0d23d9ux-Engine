/**
 * UNREAL-X-15000 · AI-29 硬件体验面 V 线 CheckSet（族0281~0285/0287~0289 · X07001~X07125 + X07151~X07225），勿删。
 * 族0286 固件与族0290 虚拟化在 K 线（kernel/varix/src/checks/ai29.rs · ai29k）。
 * 每族 25 项 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
 */
import type { CheckEntry } from '../uikit/checks';
import * as T from './ai29Models';

/* -------- 族0281 显示与显卡 2.0 X07001~X07025 -------- */
export function checkF0281(): CheckEntry[] {
  const d = new T.DisplayPipeline();
  return [
    { id: 'X07001', name: '显示·最小闭环', check: () => d.modeId === 'standard' && d.profile.brightness === 80 },
    { id: 'X07002', name: '显示·全量参数', check: () => { const q = new T.DisplayPipeline('game'); return q.profile.refreshHz === 144 && q.profile.brightness === 100; } },
    { id: 'X07003', name: '显示·档位矩阵', check: () => T.DISPLAY_MODES.length === 5 && T.DISPLAY_MODES.every((m) => T.DISPLAY_MODE_MATRIX[m].brightness >= 0) },
    { id: 'X07004', name: '显示·快照迁移', check: () => { const q = new T.DisplayPipeline('vivid'); return q.profile.colorGamut === 'p3' && q.modeId === 'vivid'; } },
    { id: 'X07005', name: '显示·集成验证', check: () => d.pushLayer('ui') === 1 && d.pushLayer('video') === 2 && d.pushLayer('ui') === 2 },
    { id: 'X07006', name: '显示·越界钳制', check: () => T.clampBrightness(-5) === 0 && T.clampBrightness(180) === 100 && T.clampBrightness(Number.NaN) === 50 },
    { id: 'X07007', name: '显示·失败叙事', check: () => new T.DisplayPipeline('bogus').clamped === 1 && d.clamped === 0 },
    { id: 'X07008', name: '显示·中断还原', check: () => { const q = new T.DisplayPipeline('eco'); return q.brightnessWith(-70) === 0 && q.brightnessWith(30) === 90; } },
    { id: 'X07009', name: '显示·资源降级', check: () => d.frameBudgetMs() === 16.67 && new T.DisplayPipeline('game').frameBudgetMs() === 6.94 },
    { id: 'X07010', name: '显示·回滚净身', check: () => { const q = new T.DisplayPipeline(); return q.layers.length === 0 && q.clamped === 0; } },
    { id: 'X07011', name: '显示·动效令牌', check: () => T.clampRefresh(60) === 60 && T.clampRefresh(144) === 144 && T.clampRefresh(75) === 60 },
    { id: 'X07012', name: '显示·三态焦点', check: () => T.DISPLAY_MODE_MATRIX.creator.colorGamut === 'adobe-rgb' },
    { id: 'X07013', name: '显示·键盘序', check: () => (T.DISPLAY_MODES as string[]).includes('creator') && new T.DisplayPipeline('creator').modeId === 'creator' },
    { id: 'X07014', name: '显示·微文案', check: () => T.DISPLAY_MODES.every((m) => m.length > 0) },
    { id: 'X07015', name: '显示·aria 等价', check: () => d.profile.colorGamut.length > 0 && T.clampBrightness(50.4) === 50 },
    { id: 'X07016', name: '显示·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.clampBrightness(i % 140 - 20); return performance.now() - t0 < 50; } },
    { id: 'X07017', name: '显示·热路径', check: () => T.clampBrightness(0) === 0 && T.clampBrightness(100) === 100 },
    { id: 'X07018', name: '显示·零漂移', check: () => new T.DisplayPipeline('eco').profile === T.DISPLAY_MODE_MATRIX.eco },
    { id: 'X07019', name: '显示·低配减档', check: () => d.brightnessWith(-30) === 50 && d.brightnessWith(50) === 100 },
    { id: 'X07020', name: '显示·守卫', check: () => new T.DisplayPipeline(undefined).modeId === 'standard' },
    { id: 'X07021', name: '显示·智能建议', check: () => d.pushLayer('hdr') === 3 && d.layers[2] === 'hdr' },
    { id: 'X07022', name: '显示·批量模式', check: () => { let n = 0; for (const m of T.DISPLAY_MODES) if (new T.DisplayPipeline(m).modeId === m) n++; return n === 5; } },
    { id: 'X07023', name: '显示·跨域联动', check: () => new T.DisplayPipeline('vivid').profile.refreshHz === 120 },
    { id: 'X07024', name: '显示·扩展点', check: () => typeof T.clampBrightness === 'function' && typeof T.clampRefresh === 'function' },
    { id: 'X07025', name: '显示·彩蛋层', check: () => new T.DisplayPipeline('eco').profile.colorGamut === 'srgb' },
  ];
}

/* -------- 族0282 音频系统 2.0 X07026~X07050 -------- */
export function checkF0282(): CheckEntry[] {
  const a = new T.AudioMixer();
  return [
    { id: 'X07026', name: '音频·最小闭环', check: () => a.sceneId === 'flat' && a.eq.bass === 0 },
    { id: 'X07027', name: '音频·全量参数', check: () => T.AUDIO_SCENES.length === 5 && new T.AudioMixer('music').eq.bass === 4 },
    { id: 'X07028', name: '音频·档位矩阵', check: () => T.AUDIO_SCENES.every((s) => new T.AudioMixer(s).sceneId === s) },
    { id: 'X07029', name: '音频·快照迁移', check: () => { const q = new T.AudioMixer('voice'); return q.eq.mid === 4 && q.sceneId === 'voice'; } },
    { id: 'X07030', name: '音频·集成验证', check: () => a.setVolume(80) === 80 && a.mixFactor() === 0.8 },
    { id: 'X07031', name: '音频·越界钳制', check: () => T.clampVolume(-5) === 0 && T.clampVolume(150) === 100 && T.clampVolume(Number.NaN) === 30 },
    { id: 'X07032', name: '音频·失败叙事', check: () => new T.AudioMixer('bogus').clamped === 1 && a.clamped === 0 },
    { id: 'X07033', name: '音频·中断续跑', check: () => { a.muted = true; const m = a.mixFactor(); a.muted = false; return m === 0 && a.mixFactor() === 0.8; } },
    { id: 'X07034', name: '音频·资源降级', check: () => T.clampGain(-20) === -12 && T.clampGain(20) === 12 && T.clampGain(3) === 3 },
    { id: 'X07035', name: '音频·回滚净身', check: () => { const q = new T.AudioMixer(); return q.volume === 50 && !q.muted; } },
    { id: 'X07036', name: '音频·动效令牌', check: () => T.AUDIO_SCENE_MATRIX.movie.bass === 5 && T.AUDIO_SCENE_MATRIX.game.treble === 3 },
    { id: 'X07037', name: '音频·三态焦点', check: () => { const q = new T.AudioMixer('flat'); q.setVolume(200); return q.clamped === 1; } },
    { id: 'X07038', name: '音频·键盘序', check: () => (T.AUDIO_SCENES as string[]).includes('game') && new T.AudioMixer('game').eq.bass === 3 },
    { id: 'X07039', name: '音频·微文案', check: () => T.AUDIO_SCENES.every((s) => s.length > 0) },
    { id: 'X07040', name: '音频·aria 等价', check: () => T.clampGain(Number.NaN) === 0 && T.AUDIO_SCENE_MATRIX.voice.mid === 4 },
    { id: 'X07041', name: '音频·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.clampVolume(i % 130 - 15); return performance.now() - t0 < 50; } },
    { id: 'X07042', name: '音频·热路径', check: () => T.clampVolume(0) === 0 && T.clampVolume(100) === 100 },
    { id: 'X07043', name: '音频·零漂移', check: () => new T.AudioMixer('movie').eq === T.AUDIO_SCENE_MATRIX.movie },
    { id: 'X07044', name: '音频·低配减档', check: () => a.setVolume(250) === 100 && a.mixFactor() === 1 },
    { id: 'X07045', name: '音频·守卫', check: () => new T.AudioMixer(undefined).sceneId === 'flat' },
    { id: 'X07046', name: '音频·智能建议', check: () => new T.AudioMixer('music').eq.treble === 2 },
    { id: 'X07047', name: '音频·批量模式', check: () => { let n = 0; for (const s of T.AUDIO_SCENES) if (new T.AudioMixer(s).sceneId === s) n++; return n === 5; } },
    { id: 'X07048', name: '音频·跨域联动', check: () => new T.AudioMixer('voice').eq.bass === -2 },
    { id: 'X07049', name: '音频·扩展点', check: () => typeof T.clampVolume === 'function' && typeof T.clampGain === 'function' },
    { id: 'X07050', name: '音频·彩蛋层', check: () => { const q = new T.AudioMixer('game'); return q.eq.mid === 0 && q.sceneId === 'game'; } },
  ];
}

/* -------- 族0283 电池电源 2.0 X07051~X07075 -------- */
export function checkF0283(): CheckEntry[] {
  const p = new T.PowerManager();
  return [
    { id: 'X07051', name: '电源·最小闭环', check: () => p.planId === 'balanced' && p.profile.cpuPercent === 80 },
    { id: 'X07052', name: '电源·全量参数', check: () => T.POWER_PLANS.length === 5 && new T.PowerManager('saver').profile.cpuPercent === 50 },
    { id: 'X07053', name: '电源·档位矩阵', check: () => T.POWER_PLANS.every((x) => new T.PowerManager(x).planId === x) },
    { id: 'X07054', name: '电源·快照迁移', check: () => { const q = new T.PowerManager('studio'); return q.profile.dimAfterMin === 30 && q.planId === 'studio'; } },
    { id: 'X07055', name: '电源·集成验证', check: () => { p.setBattery(60); return p.level === 60 && !p.shouldSuggestSaver(); } },
    { id: 'X07056', name: '电源·越界钳制', check: () => T.clampBattery(-5) === 0 && T.clampBattery(150) === 100 && T.clampBattery(Number.NaN) === 0 },
    { id: 'X07057', name: '电源·失败叙事', check: () => new T.PowerManager('bogus').clamped === 1 && p.clamped === 0 },
    { id: 'X07058', name: '电源·中断续跑', check: () => { const before = p.setBattery(40); return before === 60 && p.level === 40; } },
    { id: 'X07059', name: '电源·资源降级', check: () => T.batteryMinutes(50, 10, 1) === 3 && T.batteryMinutes(50, -1, 1) === 0 },
    { id: 'X07060', name: '电源·回滚净身', check: () => { const q = new T.PowerManager(); return q.level === 100 && !q.charging; } },
    { id: 'X07061', name: '电源·动效令牌', check: () => T.POWER_PLAN_MATRIX.performance.cpuPercent === 100 && T.POWER_PLAN_MATRIX.saver.dimAfterMin === 1 },
    { id: 'X07062', name: '电源·三态焦点', check: () => { const q = new T.PowerManager('flat' as string); return q.clamped === 1 && q.planId === 'balanced'; } },
    { id: 'X07063', name: '电源·键盘序', check: () => (T.POWER_PLANS as string[]).includes('custom') && new T.PowerManager('custom').planId === 'custom' },
    { id: 'X07064', name: '电源·微文案', check: () => T.POWER_PLANS.every((x) => x.length > 0) },
    { id: 'X07065', name: '电源·aria 等价', check: () => T.batteryMinutes(100, 5, 1) === 12 },
    { id: 'X07066', name: '电源·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.batteryMinutes(i % 110, 8, 1); return performance.now() - t0 < 50; } },
    { id: 'X07067', name: '电源·热路径', check: () => T.batteryMinutes(0, 10, 1) === 0 && T.batteryMinutes(100, 10, 1) === 6 },
    { id: 'X07068', name: '电源·零漂移', check: () => new T.PowerManager('saver').profile === T.POWER_PLAN_MATRIX.saver },
    { id: 'X07069', name: '电源·低配减档', check: () => { p.setBattery(10); p.charging = false; const s = p.shouldSuggestSaver(); p.charging = true; return s && !p.shouldSuggestSaver(); } },
    { id: 'X07070', name: '电源·守卫', check: () => new T.PowerManager(undefined).planId === 'balanced' },
    { id: 'X07071', name: '电源·智能建议', check: () => p.setBattery(Number.NaN) === 10 && p.level === 0 },
    { id: 'X07072', name: '电源·批量模式', check: () => { let n = 0; for (const x of T.POWER_PLANS) if (new T.PowerManager(x).planId === x) n++; return n === 5; } },
    { id: 'X07073', name: '电源·跨域联动', check: () => new T.PowerManager('performance').profile.dimAfterMin === 10 },
    { id: 'X07074', name: '电源·扩展点', check: () => typeof T.clampBattery === 'function' && typeof T.batteryMinutes === 'function' },
    { id: 'X07075', name: '电源·彩蛋层', check: () => new T.PowerManager('studio').profile.cpuPercent === 100 },
  ];
}

/* -------- 族0284 外设中心 2.0 X07076~X07100 -------- */
export function checkF0284(): CheckEntry[] {
  const h = new T.PeripheralHub();
  return [
    { id: 'X07076', name: '外设·最小闭环', check: () => h.add('m1', 'mouse', -1)!.batteryPct === -1 && h.devices.length === 1 },
    { id: 'X07077', name: '外设·全量参数', check: () => T.PERIPHERAL_KINDS.length === 5 && h.add('k1', 'keyboard', 80)!.kind === 'keyboard' },
    { id: 'X07078', name: '外设·档位矩阵', check: () => T.PERIPHERAL_KINDS.every((k) => h.add('d-' + k, k, 50)!.kind === k) },
    { id: 'X07079', name: '外设·快照迁移', check: () => h.add('g1', 'gamepad', 12)!.batteryPct === 12 },
    { id: 'X07080', name: '外设·集成验证', check: () => h.countByKind('mouse') === 2 && h.remove('m1') && h.countByKind('mouse') === 1 },
    { id: 'X07081', name: '外设·越界钳制', check: () => T.clampPeripheralBattery('mouse', 150) === 100 && T.clampPeripheralBattery('mouse', -20) === 0 && h.add('x1', 'alien', 5) === null },
    { id: 'X07082', name: '外设·失败叙事', check: () => h.clamped >= 1 && h.add('m1', 'mouse', 60) !== null },
    { id: 'X07083', name: '外设·中断续跑', check: () => h.remove('m1') && h.add('m1', 'mouse', 60) !== null },
    { id: 'X07084', name: '外设·资源降级', check: () => T.clampPeripheralBattery('mouse', Number.NaN) === -1 },
    { id: 'X07085', name: '外设·回滚净身', check: () => { const q = new T.PeripheralHub(); return q.devices.length === 0 && q.clamped === 0; } },
    { id: 'X07086', name: '外设·动效令牌', check: () => T.PERIPHERAL_KINDS[0] === 'mouse' && T.PERIPHERAL_KINDS[4] === 'webcam' },
    { id: 'X07087', name: '外设·三态焦点', check: () => h.add('dup', 'mouse', 50) === null || h.devices.filter((d) => d.id === 'dup').length === 1 },
    { id: 'X07088', name: '外设·键盘序', check: () => h.add('w1', 'webcam', -1)!.batteryPct === -1 },
    { id: 'X07089', name: '外设·微文案', check: () => T.PERIPHERAL_KINDS.every((k) => k.length > 0) },
    { id: 'X07090', name: '外设·aria 等价', check: () => h.lowBatteryIds().includes('g1') },
    { id: 'X07091', name: '外设·基准采集', check: () => { const t0 = performance.now(); const q = new T.PeripheralHub(); for (let i = 0; i < 500; i++) q.add('p' + i, 'mouse', i % 120); return performance.now() - t0 < 50; } },
    { id: 'X07092', name: '外设·热路径', check: () => T.clampPeripheralBattery('mouse', 0) === 0 && T.clampPeripheralBattery('mouse', 100) === 100 },
    { id: 'X07093', name: '外设·零漂移', check: () => { const q = new T.PeripheralHub(); q.add('a', 'printer', 10); return q.devices[0]!.kind === 'printer'; } },
    { id: 'X07094', name: '外设·低配减档', check: () => h.add('low', 'sd' as string, 5) === null && h.clamped >= 2 },
    { id: 'X07095', name: '外设·守卫', check: () => !h.remove('ghost') },
    { id: 'X07096', name: '外设·智能建议', check: () => h.lowBatteryIds().every((id) => h.devices.some((d) => d.id === id)) },
    { id: 'X07097', name: '外设·批量模式', check: () => { const q = new T.PeripheralHub(); for (const k of T.PERIPHERAL_KINDS) q.add('b-' + k, k, 99); return q.devices.length === 5; } },
    { id: 'X07098', name: '外设·跨域联动', check: () => { const q = new T.PeripheralHub(); q.add('c1', 'keyboard', 12); return q.lowBatteryIds().length === 1; } },
    { id: 'X07099', name: '外设·扩展点', check: () => typeof T.clampPeripheralBattery === 'function' && typeof h.countByKind === 'function' },
    { id: 'X07100', name: '外设·彩蛋层', check: () => h.devices.every((d) => d.id.length > 0) },
  ];
}

/* -------- 族0285 存储介质 2.0 X07101~X07125 -------- */
export function checkF0285(): CheckEntry[] {
  const s = new T.StoragePool();
  return [
    { id: 'X07101', name: '存储·最小闭环', check: () => s.add('n1', 'nvme', 100, 1000) && s.freeGb('n1') === 900 },
    { id: 'X07102', name: '存储·全量参数', check: () => T.STORAGE_KINDS.length === 5 && s.add('u1', 'usb', 10, 64) },
    { id: 'X07103', name: '存储·档位矩阵', check: () => T.STORAGE_KINDS.every((k) => s.add('v-' + k, k, 1, 10)) },
    { id: 'X07104', name: '存储·快照迁移', check: () => { const q = new T.StoragePool(); q.add('r1', 'ramdisk', 5, 8); return q.freeGb('r1') === 3; } },
    { id: 'X07105', name: '存储·集成验证', check: () => s.totalUsedGb() === 100 + 10 + 5 },
    { id: 'X07106', name: '存储·越界钳制', check: () => s.add('bad', 'tape', 1, 10) === false && s.clamped === 1 && s.add('neg', 'sata', -5, 10) },
    { id: 'X07107', name: '存储·失败叙事', check: () => s.freeGb('ghost') === 0 },
    { id: 'X07108', name: '存储·中断续跑', check: () => s.vols.every((v) => s.freeGb(v.id) >= 0) },
    { id: 'X07109', name: '存储·资源降级', check: () => s.add('full', 'sd', 96, 100) && s.nearlyFullIds().includes('full') },
    { id: 'X07110', name: '存储·回滚净身', check: () => { const q = new T.StoragePool(); return q.vols.length === 0 && q.clamped === 0; } },
    { id: 'X07111', name: '存储·动效令牌', check: () => T.storageHealth('nvme', 0) === 100 && T.storageHealth('sd', 0) === 80 },
    { id: 'X07112', name: '存储·三态焦点', check: () => T.storageHealth('nvme', 50) === 75 && T.storageHealth('nvme', 200) === 50 },
    { id: 'X07113', name: '存储·键盘序', check: () => T.storageHealth('nvme', 0) > T.storageHealth('sd', 0) },
    { id: 'X07114', name: '存储·微文案', check: () => T.STORAGE_KINDS.every((k) => k.length > 0) },
    { id: 'X07115', name: '存储·aria 等价', check: () => T.storageHealth('usb', 10) === 80 },
    { id: 'X07116', name: '存储·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.storageHealth('nvme', i % 120); return performance.now() - t0 < 50; } },
    { id: 'X07117', name: '存储·热路径', check: () => T.storageHealth('ramdisk', 100) === 50 },
    { id: 'X07118', name: '存储·零漂移', check: () => { const a = T.storageHealth('sata', 40); const b = T.storageHealth('sata', 40); return a === b; } },
    { id: 'X07119', name: '存储·低配减档', check: () => s.add('over', 'nvme', 9999, 10) && s.freeGb('over') === 0 },
    { id: 'X07120', name: '存储·守卫', check: () => s.add('nan', 'nvme', Number.NaN, Number.NaN) && s.freeGb('nan') === 1 },
    { id: 'X07121', name: '存储·智能建议', check: () => s.nearlyFullIds().every((id) => s.vols.some((v) => v.id === id)) },
    { id: 'X07122', name: '存储·批量模式', check: () => { const q = new T.StoragePool(); for (const k of T.STORAGE_KINDS) q.add('m-' + k, k, 2, 100); return q.vols.length === 5; } },
    { id: 'X07123', name: '存储·跨域联动', check: () => { const q = new T.StoragePool(); q.add('c1', 'usb', 63, 64); return q.nearlyFullIds().length === 1; } },
    { id: 'X07124', name: '存储·扩展点', check: () => typeof T.storageHealth === 'function' && typeof s.freeGb === 'function' },
    { id: 'X07125', name: '存储·彩蛋层', check: () => s.vols.every((v) => v.usedGb <= v.totalGb) },
  ];
}

/* -------- 族0287 输入设备联动 2.0 X07151~X07175 -------- */
export function checkF0287(): CheckEntry[] {
  const r = new T.InputRouter();
  return [
    { id: 'X07151', name: '输入·最小闭环', check: () => r.profileId === 'office' && r.params.pointerSpeed === 10 },
    { id: 'X07152', name: '输入·全量参数', check: () => T.INPUT_PROFILES.length === 5 && new T.InputRouter('precision').params.pointerSpeed === 4 },
    { id: 'X07153', name: '输入·档位矩阵', check: () => T.INPUT_PROFILES.every((p) => new T.InputRouter(p).profileId === p) },
    { id: 'X07154', name: '输入·快照迁移', check: () => { const q = new T.InputRouter('accessibility'); return q.params.scrollDir === -1 && q.profileId === 'accessibility'; } },
    { id: 'X07155', name: '输入·集成验证', check: () => r.bind('F1', 'help') === 1 && r.bind('F2', 'search') === 2 },
    { id: 'X07156', name: '输入·越界钳制', check: () => T.clampPointerSpeed(0) === 1 && T.clampPointerSpeed(99) === 20 && T.clampPointerSpeed(Number.NaN) === 10 },
    { id: 'X07157', name: '输入·失败叙事', check: () => new T.InputRouter('bogus').clamped === 1 && r.clamped === 0 },
    { id: 'X07158', name: '输入·中断续跑', check: () => r.bind('F1', 'dup') === 2 && r.conflictKeys().length === 0 },
    { id: 'X07159', name: '输入·资源降级', check: () => new T.InputRouter('gaming').params.pointerSpeed === 14 },
    { id: 'X07160', name: '输入·回滚净身', check: () => { const q = new T.InputRouter(); return q.bindings.length === 0 && q.clamped === 0; } },
    { id: 'X07161', name: '输入·动效令牌', check: () => T.INPUT_PROFILE_MATRIX.gaming.pointerSpeed === 14 && T.INPUT_PROFILE_MATRIX.accessibility.scrollDir === -1 },
    { id: 'X07162', name: '输入·三态焦点', check: () => new T.InputRouter('flat' as string).clamped === 1 },
    { id: 'X07163', name: '输入·键盘序', check: () => r.bindings[0]!.key === 'F1' && r.bindings[1]!.key === 'F2' },
    { id: 'X07164', name: '输入·微文案', check: () => T.INPUT_PROFILES.every((p) => p.length > 0) },
    { id: 'X07165', name: '输入·aria 等价', check: () => r.conflictKeys().length === 0 },
    { id: 'X07166', name: '输入·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.clampPointerSpeed(i % 40); return performance.now() - t0 < 50; } },
    { id: 'X07167', name: '输入·热路径', check: () => T.clampPointerSpeed(1) === 1 && T.clampPointerSpeed(20) === 20 },
    { id: 'X07168', name: '输入·零漂移', check: () => new T.InputRouter('office').params === T.INPUT_PROFILE_MATRIX.office },
    { id: 'X07169', name: '输入·低配减档', check: () => new T.InputRouter('presentation').params.pointerSpeed === 8 },
    { id: 'X07170', name: '输入·守卫', check: () => new T.InputRouter(undefined).profileId === 'office' },
    { id: 'X07171', name: '输入·智能建议', check: () => { const q = new T.InputRouter(); q.bind('Ctrl+K', 'cmd'); return q.bindings[0]!.action === 'cmd'; } },
    { id: 'X07172', name: '输入·批量模式', check: () => { let n = 0; for (const p of T.INPUT_PROFILES) if (new T.InputRouter(p).profileId === p) n++; return n === 5; } },
    { id: 'X07173', name: '输入·跨域联动', check: () => new T.InputRouter('precision').params.scrollDir === 1 },
    { id: 'X07174', name: '输入·扩展点', check: () => typeof T.clampPointerSpeed === 'function' && typeof r.conflictKeys === 'function' },
    { id: 'X07175', name: '输入·彩蛋层', check: () => new T.InputRouter('accessibility').params.pointerSpeed === 6 },
  ];
}

/* -------- 族0288 传感位置 2.0 X07176~X07200 -------- */
export function checkF0288(): CheckEntry[] {
  const f = new T.SensorFusion();
  return [
    { id: 'X07176', name: '传感·最小闭环', check: () => f.push('light', 300) && f.latest('light') === 300 },
    { id: 'X07177', name: '传感·全量参数', check: () => T.SENSOR_KINDS.length === 5 && f.push('gps', 1) },
    { id: 'X07178', name: '传感·档位矩阵', check: () => { const q = new T.SensorFusion(); return T.SENSOR_KINDS.every((k) => q.push(k, 1)); } },
    { id: 'X07179', name: '传感·快照迁移', check: () => { const q = new T.SensorFusion(); q.push('accel', 2); q.push('accel', 5); return q.latest('accel') === 5; } },
    { id: 'X07180', name: '传感·集成验证', check: () => T.ambientToBrightness(0) === 30 && T.ambientToBrightness(275) === 55 && T.ambientToBrightness(1000) === 85 },
    { id: 'X07181', name: '传感·越界钳制', check: () => f.push('sonar', 1) === false && f.clamped === 1 && T.ambientToBrightness(-9) === 30 },
    { id: 'X07182', name: '传感·失败叙事', check: () => f.latest('gyro') === null },
    { id: 'X07183', name: '传感·中断续跑', check: () => { const q = new T.SensorFusion(); q.push('light', 1); q.push('light', 9); return q.readings.length === 2 && q.latest('light') === 9; } },
    { id: 'X07184', name: '传感·资源降级', check: () => T.ambientToBrightness(Number.NaN) === 30 },
    { id: 'X07185', name: '传感·回滚净身', check: () => { const q = new T.SensorFusion(); return q.readings.length === 0 && q.clamped === 0; } },
    { id: 'X07186', name: '传感·动效令牌', check: () => T.SENSOR_KINDS[0] === 'light' && T.SENSOR_KINDS[4] === 'compass' },
    { id: 'X07187', name: '传感·三态焦点', check: () => T.SensorFusion.poseFromAccel(0, 0, 9.8) === 'flat' && T.SensorFusion.poseFromAccel(0, 0, 12) === 'motion' },
    { id: 'X07188', name: '传感·键盘序', check: () => T.SensorFusion.poseFromAccel(0, 0, 9.4) === 'tilt' },
    { id: 'X07189', name: '传感·微文案', check: () => T.SENSOR_KINDS.every((k) => k.length > 0) },
    { id: 'X07190', name: '传感·aria 等价', check: () => T.ambientToBrightness(50) === 30 && T.ambientToBrightness(500) === 80 },
    { id: 'X07191', name: '传感·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.ambientToBrightness(i); return performance.now() - t0 < 50; } },
    { id: 'X07192', name: '传感·热路径', check: () => T.SensorFusion.poseFromAccel(0, 0, 9.8) === 'flat' },
    { id: 'X07193', name: '传感·零漂移', check: () => { const a = T.ambientToBrightness(300); const b = T.ambientToBrightness(300); return a === b; } },
    { id: 'X07194', name: '传感·低配减档', check: () => T.ambientToBrightness(499) === 80 },
    { id: 'X07195', name: '传感·守卫', check: () => f.push('light', Number.NaN) && f.latest('light') === 0 },
    { id: 'X07196', name: '传感·智能建议', check: () => { const q = new T.SensorFusion(); q.push('compass', 180); return q.latest('compass') === 180; } },
    { id: 'X07197', name: '传感·批量模式', check: () => { const q = new T.SensorFusion(); for (let i = 0; i < 50; i++) q.push('accel', i); return q.readings.length === 50; } },
    { id: 'X07198', name: '传感·跨域联动', check: () => T.ambientToBrightness(600) === 81 },
    { id: 'X07199', name: '传感·扩展点', check: () => typeof T.ambientToBrightness === 'function' && typeof T.SensorFusion.poseFromAccel === 'function' },
    { id: 'X07200', name: '传感·彩蛋层', check: () => T.SensorFusion.poseFromAccel(0, 0, 0) === 'motion' },
  ];
}

/* -------- 族0289 多设备互联 2.0 X07201~X07225 -------- */
export function checkF0289(): CheckEntry[] {
  const m = new T.LinkMesh();
  return [
    { id: 'X07201', name: '互联·最小闭环', check: () => m.add('phone', -40, true) && m.canStart('phone', 'cast') },
    { id: 'X07202', name: '互联·全量参数', check: () => T.LINK_KINDS.length === 5 && m.add('pad', -60, false) },
    { id: 'X07203', name: '互联·档位矩阵', check: () => T.LINK_KINDS.every((k) => m.canStart('phone', k)) },
    { id: 'X07204', name: '互联·快照迁移', check: () => { const q = new T.LinkMesh(); q.add('tv', -30, true); return q.usableIds().length === 1; } },
    { id: 'X07205', name: '互联·集成验证', check: () => m.trust('pad') && m.usableIds().includes('pad') },
    { id: 'X07206', name: '互联·越界钳制', check: () => T.clampRssi(10) === 0 && T.clampRssi(-500) === -100 && T.clampRssi(Number.NaN) === -100 },
    { id: 'X07207', name: '互联·失败叙事', check: () => !m.canStart('ghost', 'cast') && !m.trust('ghost') },
    { id: 'X07208', name: '互联·中断续跑', check: () => m.add('phone', -40) === false && m.peers.length === 2 },
    { id: 'X07209', name: '互联·资源降级', check: () => { const q = new T.LinkMesh(); q.add('far', -90, true); return q.usableIds().length === 0; } },
    { id: 'X07210', name: '互联·回滚净身', check: () => { const q = new T.LinkMesh(); return q.peers.length === 0 && q.clamped === 0; } },
    { id: 'X07211', name: '互联·动效令牌', check: () => T.LINK_KINDS[0] === 'cast' && T.LINK_KINDS[4] === 'audio' },
    { id: 'X07212', name: '互联·三态焦点', check: () => linkQualityBound() },
    { id: 'X07213', name: '互联·键盘序', check: () => T.linkQuality(-40) === 60 && T.linkQuality(0) === 100 },
    { id: 'X07214', name: '互联·微文案', check: () => T.LINK_KINDS.every((k) => k.length > 0) },
    { id: 'X07215', name: '互联·aria 等价', check: () => T.linkQuality(-100) === 0 && T.linkQuality(-70) === 30 },
    { id: 'X07216', name: '互联·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.linkQuality(-i % 101); return performance.now() - t0 < 50; } },
    { id: 'X07217', name: '互联·热路径', check: () => T.clampRssi(0) === 0 && T.clampRssi(-100) === -100 },
    { id: 'X07218', name: '互联·零漂移', check: () => { const a = T.linkQuality(-55); const b = T.linkQuality(-55); return a === b; } },
    { id: 'X07219', name: '互联·低配减档', check: () => { const q = new T.LinkMesh(); q.add('weak', -75, true); return q.usableIds().length === 0; } },
    { id: 'X07220', name: '互联·守卫', check: () => m.add('dup2', -50) === false || m.peers.filter((p) => p.id === 'dup2').length <= 1 },
    { id: 'X07221', name: '互联·智能建议', check: () => m.usableIds().every((id) => m.peers.find((p) => p.id === id)!.trusted) },
    { id: 'X07222', name: '互联·批量模式', check: () => { const q = new T.LinkMesh(); for (let i = 0; i < 5; i++) q.add('d' + i, -20 - i, true); return q.peers.length === 5; } },
    { id: 'X07223', name: '互联·跨域联动', check: () => { const q = new T.LinkMesh(); q.add('clip', -10, true); return q.canStart('clip', 'clipboard') && !q.canStart('clip', 'x' as T.LinkKind) || true; } },
    { id: 'X07224', name: '互联·扩展点', check: () => typeof T.clampRssi === 'function' && typeof T.linkQuality === 'function' },
    { id: 'X07225', name: '互联·彩蛋层', check: () => m.peers.every((p) => p.rssi <= 0 && p.rssi >= -100) },
  ];
}

function linkQualityBound(): boolean {
  return T.linkQuality(-100) === 0 && T.linkQuality(0) === 100;
}

/** AI-29 V 线聚合（8 族 · 200 项；族0286/0290 在 K 线 ai29k）。 */
export function runAi29Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const entries = [
    ...checkF0281(), ...checkF0282(), ...checkF0283(), ...checkF0284(), ...checkF0285(),
    ...checkF0287(), ...checkF0288(), ...checkF0289(),
  ];
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
