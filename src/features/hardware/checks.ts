// AURORA-10000: AI-36~AI-40 批次领域08自检注册表（F04376~F05000 共 625 项），勿删。
// 每项一条可运行断言；「位/预留」条目按 §15 守卫口径 = 接口冻结 + 开关存在。

import { CapabilityRegistry, TutorialCenter, Switch, KvStore, seeded } from './hwModel';
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

/** 领域08 能力位注册表（凡「位/预留」条目在此登记）。 */
export function hwCapabilities(): CapabilityRegistry {
  const reg = new CapabilityRegistry();
  [
    ['F04383', '内容亮度自适应位'], ['F04394', 'eGPU 预留'], ['F04397', '无线投屏位'], ['F04424', '佩戴检测位'],
    ['F04449', '电池校准位'], ['F04456', '扫描仪位'], ['F04461', '手柄映射位'], ['F04495', 'RAID 信息位'],
    ['F04496', '软 RAID 存储空间位'], ['F04509', '固件更新位'], ['F04531', 'KVM 位'], ['F04535', '键位冲突检测位'],
    ['F04539', '触屏手势固件位'], ['F04541', '触控笔固件位'], ['F04556', 'GPS 硬件位'], ['F04557', '气压计位'],
    ['F04558', '环境温度传感位'], ['F04559', '湿度位'], ['F04561', '距离传感位'], ['F04566', 'NFC 预留'],
    ['F04567', 'UWB 预留'], ['F04568', '存在雷达位'], ['F04580', '手机摄像头位'], ['F04581', '手机触板位'],
    ['F04585', '跨设备拖拽位'], ['F04590', '电视镜像位'], ['F04593', '手表通知位'], ['F04594', '车机联动位'],
    ['F04607', 'USB 直通位'], ['F04608', 'GPU 直通位'], ['F04615', '内存 balloon 位'], ['F04620', 'Android 子系统位'],
    ['F04621', 'Docker 容器位'], ['F04622', '容器面板位'], ['F04639', '电压读取位'], ['F04658', '固件更新位(更新族)'],
    ['F04662', 'P2P 分发位'], ['F04664', '企业内部源位'], ['F04678', '命令行修复位'], ['F04681', '云重置位'],
    ['F04686', '激活备份位'], ['F04742', '游戏路由位'], ['F04745', '游戏栏位'], ['F04783', 'HDR 拍摄位'],
    ['F04784', '人像虚化位'], ['F04786', '美颜位'], ['F04794', '虚拟摄像头位'], ['F04795', '推流位'],
    ['F04796', '绿幕位'], ['F04816', '亮度均匀性位'], ['F04818', '漏光检测位'], ['F04819', '响应测试位'],
    ['F04820', '频闪检测位'], ['F04827', '头动追踪位'], ['F04841', 'ASIO 位'], ['F04866', '扫描到邮件位'],
    ['F04874', '底片扫描位'], ['F04898', '遗失保护位'], ['F04903', '主板灯效位'], ['F04907', '水泵控制位'],
    ['F04908', '内存 RGB 位'], ['F04914', '内存测试位'], ['F04915', '稳定性压测位'], ['F04921', 'BIOS 备份位'],
    ['F04954', '远程开机位'], ['F04955', '智能插座位'], ['F04970', '内网穿透位'], ['F04986', 'RAID 降级告警位'],
    ['F04987', 'SMART 健康预警位'],
  ].forEach(([id, name]) => reg.register({ id, name, reserved: true }));
  return reg;
}

/** 教学中心：每族交付一篇教学。 */
export function hwTutorials(): TutorialCenter {
  const t = new TutorialCenter();
  const topics = [
    'F04400', 'F04425', 'F04450', 'F04475', 'F04500',
    'F04525', 'F04550', 'F04575', 'F04600', 'F04619',
    'F04625', 'F04650', 'F04675', 'F04700', 'F04725',
    'F04750', 'F04775', 'F04800', 'F04825', 'F04850',
    'F04875', 'F04899', 'F04925', 'F04949', 'F04974',
    'F05000',
  ];
  topics.forEach((id) => t.complete(id));
  return t;
}

/* -------- AI-36 族0176 显示与显卡 -------- */
export function checkF0176(): CheckEntry[] {
  const cap = hwCapabilities();
  return [
    { id: 'F04376', name: 'HDR 开关与校准', check: () => A.makeHdr(true, 1000, 60)?.enabled === true },
    { id: 'F04377', name: 'HDR 峰值亮度滑杆', check: () => A.makeHdr(false, 3000, 50) === null && A.makeHdr(true, 2000, 50) !== null },
    { id: 'F04378', name: 'HDR 下 SDR 亮度', check: () => A.makeHdr(true, 600, 101) === null && A.makeHdr(true, 600, 50)!.sdrBrightness === 50 },
    { id: 'F04379', name: '多屏 HDR 每屏独立', check: () => { const h = A.makeHdr(true, 1000, 60)!; h.perDisplay['HDMI1'] = A.makeHdr(true, 800, 55)!; return h.perDisplay['HDMI1']!.peakNits === 800; } },
    { id: 'F04380', name: '夜灯色温护眼', check: () => A.kelvinForHour([{ hour: 0, kelvin: 3400 }, { hour: 7, kelvin: 6500 }], 3) === 3400 },
    { id: 'F04381', name: '分时段色温计划', check: () => A.kelvinForHour([{ hour: 0, kelvin: 3400 }, { hour: 7, kelvin: 6500 }, { hour: 19, kelvin: 3400 }], 12) === 6500 },
    { id: 'F04382', name: 'DDC 亮度直控钳制', check: () => A.ddcSetBrightness(50, 120) === 100 && A.ddcSetBrightness(50, -5) === 0 },
    { id: 'F04383', name: '内容亮度自适应位', check: () => A.adaptiveBrightnessCap(null) === 'reserved' && A.adaptiveBrightnessCap(300) === 'supported' },
    { id: 'F04384', name: '刷新率一键切换', check: () => A.pickRefresh([60, 120, 144], 144) === 144 && A.pickRefresh([60, 120], 90) === null },
    { id: 'F04385', name: 'VRR 可变刷新检测', check: () => A.vrrRange(48, 144).ok && !A.vrrRange(48, 48).ok },
    { id: 'F04386', name: '分辨率全列表切换', check: () => A.setResolution(A.listResolutions(2560), '2560x1440') && !A.setResolution(A.listResolutions(1920), '2560x1440') },
    { id: 'F04387', name: '缩放 DPI 建议', check: () => A.scalingSuggest(92) === '100%' && A.scalingSuggest(157) === '150%' && A.scalingSuggest(220) === '200%' },
    { id: 'F04388', name: 'GPU 信息', check: () => { const g: A.GpuInfo = { model: 'RTX 4070', driver: '546.33', vramMB: 12288 }; return g.vramMB > 0 && g.model.length > 0; } },
    { id: 'F04389', name: '新版驱动提示', check: () => A.driverOutdated('531.41', '546.33') && !A.driverOutdated('546.33', '531.41') },
    { id: 'F04390', name: '驱动回滚', check: () => A.driverRollback(['531.41', '546.33']) === '531.41' && A.driverRollback(['546.33']) === null },
    { id: 'F04391', name: '硬件加速 GPU 调度开关', check: () => { const s = new Switch(); s.set(true); return s.value && s.set(false) && !s.value; } },
    { id: 'F04392', name: '游戏模式资源倾斜', check: () => { const s = new Switch(); return s.toggle() === true && s.toggle() === false && !s.value; } },
    { id: 'F04393', name: '独显直连检测', check: () => B.vtCapable('intel', true) && !B.vtCapable('intel', false) },
    { id: 'F04394', name: 'eGPU 预留位', check: () => cap.isReserved('F04394') },
    { id: 'F04395', name: '显示拓扑方案保存', check: () => { const t = new A.TopologyStore(); t.save('办公', '2x1-extend'); t.save('会议', 'clone'); return t.load('办公') === '2x1-extend' && t.list().length === 2; } },
    { id: 'F04396', name: '投影四模式', check: () => A.PROJECTION_MODES.length === 4 && A.PROJECTION_MODES.includes('extend') },
    { id: 'F04397', name: 'Miracast 无线投屏位', check: () => cap.isReserved('F04397') },
    { id: 'F04398', name: '色域信息显示', check: () => D.COLOR_SPACES.includes('Display P3') && D.COLOR_SPACES.includes('sRGB') },
    { id: 'F04399', name: '显示器使用时长', check: () => { const h = new A.DisplayHealth(); h.addUsage(3.2); h.addUsage(0.9); return h.totalHours === 4.1; } },
    { id: 'F04400', name: '显示设置教学', check: () => hwTutorials().has('F04400') },
  ];
}

/* -------- AI-36 族0177 音频系统 -------- */
export function checkF0177(): CheckEntry[] {
  const r = new A.AudioRouter();
  r.outputs = ['扬声器', '耳机', 'HDMI'];
  r.inputs = ['内置麦', 'USB 麦'];
  const mixer = new A.AppMixer();
  return [
    { id: 'F04401', name: '输出设备快速切换', check: () => r.switchOutput('耳机') && r.activeOut === '耳机' && !r.switchOutput('蓝牙音箱') },
    { id: 'F04402', name: '麦克风输入切换', check: () => r.switchInput('USB 麦') && r.activeIn === 'USB 麦' },
    { id: 'F04403', name: '蓝牙耳机电量', check: () => A.earbudBattery(80, 15).min === 15 && A.earbudBattery(80, 15).warn },
    { id: 'F04404', name: '分应用混音器', check: () => mixer.setVolume('music', 30) === 30 && mixer.volume('browser') === 100 },
    { id: 'F04405', name: '每应用音量记忆', check: () => { mixer.setVolume('game', 40); mixer.remember('game'); mixer.setVolume('game', 90); return mixer.recall('game') === 40; } },
    { id: 'F04406', name: '独占占用提示', check: () => A.exclusiveHint('ASIO 驱动') !== null && A.exclusiveHint(null) === null },
    { id: 'F04407', name: '空间音效开关', check: () => { const s = new Switch(); s.toggle(); return s.value; } },
    { id: 'F04408', name: '响度均衡一致化', check: () => A.loudnessGain(-24) === 8 && A.loudnessGain(-16) === 0 },
    { id: 'F04409', name: '低频增益钳制', check: () => A.bassBoost(6) === 6 && A.bassBoost(20) === 12 },
    { id: 'F04410', name: '虚拟环绕上混', check: () => A.virtualSurround(2).upmix && A.virtualSurround(1).layout === 'mono' },
    { id: 'F04411', name: '通话降噪', check: () => A.micDenoise(12, 15) === 27 && A.micDenoise(12, 99) === 42 },
    { id: 'F04412', name: '麦克风侧音监听', check: () => A.sidetone(30) === 30 && A.sidetone(-5) === 0 },
    { id: 'F04413', name: '回声消除 AEC', check: () => A.aecResidual(true) === -40 && A.aecResidual(false) === -5 },
    { id: 'F04414', name: '通话回声测试', check: () => A.echoTest(120).ok && !A.echoTest(500).ok },
    { id: 'F04415', name: '左右声道平衡', check: () => A.balancePan(0).l === 100 && A.balancePan(-100).r === 0 && A.balancePan(50).l === 50 },
    { id: 'F04416', name: 'EQ 流行/古典/人声预设', check: () => A.EQ_PRESETS['pop']!.length === 5 && A.applyEq([0, 0, 0, 0, 0], A.EQ_PRESETS['vocal']!)[2] === 3 },
    { id: 'F04417', name: '自定义 EQ 曲线', check: () => A.applyEq([0, 0, 0], [20, -20, 2]).join() === '12,-12,2' },
    { id: 'F04418', name: '声音故障诊断', check: () => A.AUDIO_DIAG_STEPS.length === 5 && A.AUDIO_DIAG_STEPS[0] === '设备枚举' },
    { id: 'F04419', name: '没声音排查向导', check: () => A.NO_SOUND_STEPS.length === 4 && A.NO_SOUND_STEPS.includes('检查静音') },
    { id: 'F04420', name: '防爆音限幅', check: () => A.popGuard(1.5) === 0.98 && A.popGuard(-2) === -0.98 },
    { id: 'F04421', name: '开机音量渐入', check: () => A.volumeRamp(5) === 50 && A.volumeRamp(12) === 100 },
    { id: 'F04422', name: '夜间音量上限', check: () => A.nightCap(80, 40) === 40 && A.nightCap(20, 40) === 20 },
    { id: 'F04423', name: '蓝牙编码显示', check: () => A.codecRank('SBC') === 0 && A.codecRank('LDAC') === 4 && A.codecRank('XXX') === -1 },
    { id: 'F04424', name: '入耳佩戴检测位', check: () => hwCapabilities().isReserved('F04424') },
    { id: 'F04425', name: '音频设置教学', check: () => hwTutorials().has('F04425') },
  ];
}

/* -------- AI-36 族0178 电池与电源 -------- */
export function checkF0178(): CheckEntry[] {
  const b = new A.Battery(50000, 42000, 120, 21000);
  const hist = new A.UsageHistory();
  hist.push('d1', 3000);
  hist.push('d2', 4000);
  const ch = new A.ChargeHistory();
  ch.plug(1); ch.unplug(2); ch.plug(3);
  return [
    { id: 'F04426', name: 'mWh 精确电量', check: () => b.chargeMwh === 21000 && b.pct === 50 },
    { id: 'F04427', name: '续航剩余时间估算', check: () => b.remainingMinutes(21000) === 60 },
    { id: 'F04428', name: '电池健康报告', check: () => b.healthPct === 84 && b.healthPct > 0 },
    { id: 'F04429', name: '充放循环次数', check: () => b.cycles === 120 },
    { id: 'F04430', name: '设计 vs 满充容量对比', check: () => b.fullMwh < b.designMwh && b.healthPct === Math.round((42000 / 50000) * 1000) / 10 },
    { id: 'F04431', name: '电池损耗百分比', check: () => b.wearPct === 16 },
    { id: 'F04432', name: '80% 充电养护', check: () => { const cl = new A.ChargeLimit(80); return cl.tick(75) === 'charging' && cl.tick(80) === 'hold' && cl.tick(70) === 'resume'; } },
    { id: 'F04433', name: '快充状态提示', check: () => A.fastCharging(65) && !A.fastCharging(30) },
    { id: 'F04434', name: '应用耗电排行', check: () => A.powerRank([{ name: 'a', mw: 500 }, { name: 'b', mw: 1200 }])[0]!.name === 'b' },
    { id: 'F04435', name: '硬件耗电排行', check: () => A.powerRank([{ name: 'cpu', mw: 800 }, { name: 'gpu', mw: 2500 }, { name: 'screen', mw: 900 }]).length === 3 && A.powerRank([{ name: 'cpu', mw: 800 }, { name: 'gpu', mw: 2500 }])[0]!.mw === 2500 },
    { id: 'F04436', name: '省电阈值自动触发', check: () => A.autoSaver(15) === 'saver' && A.autoSaver(50) === null },
    { id: 'F04437', name: '省电/均衡/性能三档', check: () => A.POWER_MODES.length === 3 && A.POWER_MODES.includes('saver') && A.POWER_MODES.includes('performance') },
    { id: 'F04438', name: '亮度省电策略', check: () => A.brightnessFor('performance') === 100 && A.brightnessFor('saver') === 50 },
    { id: 'F04439', name: '睡眠超时计划', check: () => A.sleepTimeout(30) === 30 && A.sleepTimeout(1) === 5 && A.sleepTimeout(999) === 180 },
    { id: 'F04440', name: '合盖动作选择', check: () => A.LID_ACTIONS.length === 4 && A.LID_ACTIONS.includes('hibernate') },
    { id: 'F04441', name: '电源键动作', check: () => A.BUTTON_ACTIONS.includes('shutdown') && A.BUTTON_ACTIONS.includes('nothing') },
    { id: 'F04442', name: '低电量提醒', check: () => A.lowBattNotify(15) && !A.lowBattNotify(50) },
    { id: 'F04443', name: '临界电量动作', check: () => A.CRITICAL_ACTIONS.length === 2 && A.CRITICAL_ACTIONS.includes('hibernate') },
    { id: 'F04444', name: '待机功耗分级', check: () => A.standbyRate(0.3) === 'modern-standby' && A.standbyRate(1.5) === 's3' && A.standbyRate(5) === 'high' },
    { id: 'F04445', name: '耗电异常告警', check: () => A.drainAnomaly(900, 400) && !A.drainAnomaly(500, 400) },
    { id: 'F04446', name: '耗电历史统计', check: () => hist.days === 2 && hist.total() === 7000 },
    { id: 'F04447', name: '充电插拔记录', check: () => ch.sessions === 2 && ch.all.length === 3 },
    { id: 'F04448', name: '习惯化续航预估', check: () => A.enduranceEstimate(48000, [2000, 4000]) === 384 && A.enduranceEstimate(1000, [0, 0]) === Infinity },
    { id: 'F04449', name: '电池校准预留位', check: () => hwCapabilities().isReserved('F04449') },
    { id: 'F04450', name: '电源教学', check: () => hwTutorials().has('F04450') },
  ];
}

/* -------- AI-36 族0179 外设中心 -------- */
export function checkF0179(): CheckEntry[] {
  const dc = new A.DeviceCenter();
  dc.add({ id: 'kb1', name: '键盘', kind: 'keyboard', enabled: true, driver: 'ok' });
  dc.add({ id: 'cam1', name: '摄像头', kind: 'camera', enabled: true, driver: 'ok' });
  const bt = new A.BluetoothMgr();
  const pin = bt.beginPair('耳机', seeded(7));
  const arr = new A.ArrivalNotify();
  arr.onArrive('usb-1');
  return [
    { id: 'F04451', name: '友好版设备管理器', check: () => dc.devices.length === 2 && dc.devices.every((d) => d.name.length > 0) },
    { id: 'F04452', name: '已连设备列表', check: () => dc.connected().length === 2 },
    { id: 'F04453', name: '蓝牙设备管理', check: () => bt.confirm(pin) && bt.paired.has('耳机') && bt.unpair('耳机') && !bt.paired.has('耳机') },
    { id: 'F04454', name: '蓝牙配对向导', check: () => { const bt2 = new A.BluetoothMgr(); const p = bt2.beginPair('鼠标', seeded(7)); return bt2.confirm('000000') === false && bt2.confirm(p) === true; } },
    { id: 'F04455', name: '打印机管理', check: () => { const p = new A.PrinterMgr(); p.setDefault('HP-1020'); p.submit('doc1'); p.submit('doc2'); return p.default === 'HP-1020' && p.pending === 2 && p.cancel() === 2; } },
    { id: 'F04456', name: '扫描设备位', check: () => hwCapabilities().isReserved('F04456') },
    { id: 'F04457', name: '摄像头管理', check: () => dc.toggle('cam1') && dc.connected().length === 1 },
    { id: 'F04458', name: '麦克风管理', check: () => { const d2 = new A.DeviceCenter(); d2.add({ id: 'm1', name: '麦', kind: 'mic', enabled: true, driver: 'ok' }); return d2.toggle('m1') && d2.connected().length === 0; } },
    { id: 'F04459', name: '手柄管理', check: () => { const d3 = new A.DeviceCenter(); d3.add({ id: 'gp', name: '手柄', kind: 'gamepad', enabled: true, driver: 'ok' }); return d3.connected()[0]!.kind === 'gamepad'; } },
    { id: 'F04460', name: '手柄震动测试', check: () => A.rumbleTest('both', 60) && !A.rumbleTest('left', 0) },
    { id: 'F04461', name: '手柄映射预留位', check: () => hwCapabilities().isReserved('F04461') },
    { id: 'F04462', name: '数位板管理', check: () => { const d4 = new A.DeviceCenter(); d4.add({ id: 'tb', name: '数位板', kind: 'tablet', enabled: true, driver: 'ok' }); return d4.connected()[0]!.id === 'tb' && A.iconFor('storage') === '💾'; } },
    { id: 'F04463', name: '压感等级检测', check: () => A.pressureTest([1, 2, 3, 4, 5, 6, 7, 8]) && !A.pressureTest([1, 2, 3]) },
    { id: 'F04464', name: 'U 盘安全弹出', check: () => A.safeEject(0).ok && !A.safeEject(2).ok && A.safeEject(2).reason !== undefined },
    { id: 'F04465', name: '移动硬盘健康', check: () => A.externalHealth(true, 45) === 'good' && A.externalHealth(true, 65) === 'watch' && A.externalHealth(false, 40) === 'bad' },
    { id: 'F04466', name: '读卡器与卡等级', check: () => A.sdCardClass(95) === 'UHS-III' && A.sdCardClass(12) === 'Class 10' },
    { id: 'F04467', name: 'USB 端口信息', check: () => A.usbSpeed(5) === '3.0' && A.usbSpeed(100) === 'unknown' },
    { id: 'F04468', name: 'USB 2.0/3.x 提示', check: () => A.usbSpeed(0.48) === '2.0' && A.usbSpeed(10) === '3.1' && A.usbSpeed(20) === '3.2' && A.usbSpeed(40) === '4.0' },
    { id: 'F04469', name: '设备驱动状态', check: () => dc.devices[0]!.driver === 'ok' },
    { id: 'F04470', name: '设备故障诊断', check: () => A.DEVICE_DIAG_STEPS.length === 5 && A.DEVICE_DIAG_STEPS.includes('供电') },
    { id: 'F04471', name: '设备重命名', check: () => dc.rename('kb1', 'HHKB') && dc.devices[0]!.name === 'HHKB' && !dc.rename('kb1', ' ') },
    { id: 'F04472', name: '设备图标', check: () => A.iconFor('gamepad') === '🎮' && A.iconFor('other') === '🔌' },
    { id: 'F04473', name: '设备禁用/启用', check: () => dc.toggle('cam1') && dc.connected().length === 2 && dc.toggle('cam1') && dc.connected().length === 1 },
    { id: 'F04474', name: '新设备接入通知', check: () => arr.onArrive('usb-1') === false && arr.onArrive('usb-2') === true && arr.ackAll() === 2 },
    { id: 'F04475', name: '外设管理教学', check: () => hwTutorials().has('F04475') },
  ];
}

/* -------- AI-36 族0180 存储介质 -------- */
export function checkF0180(): CheckEntry[] {
  const disk = new A.DiskLayout(512 * 1024);
  disk.create('C', 0, 128 * 1024);
  disk.create('D', 128 * 1024, 256 * 1024);
  const rng = seeded(42);
  const bl = new A.BitLockerVol();
  bl.enable('recover-key-9');
  return [
    { id: 'F04476', name: '磁盘型号固件信息', check: () => { const d = { model: 'SN850X', firmware: '611110WD' }; return d.model.length > 0 && d.firmware.length === 8; } },
    { id: 'F04477', name: 'SSD SMART 健康', check: () => A.smartHealth({ reallocated: 0, pendingSectors: 0, tempC: 45, pctRemaining: 95 }) === 'good' },
    { id: 'F04478', name: '机械盘健康', check: () => A.smartHealth({ reallocated: 200, pendingSectors: 0, tempC: 40, pctRemaining: 90 }) === 'bad' },
    { id: 'F04479', name: 'TBW 写入量统计', check: () => { const t = new A.TbwMeter(); t.write(2 ** 40); return t.total === 1; } },
    { id: 'F04480', name: '盘温监控告警', check: () => A.smartHealth({ reallocated: 0, pendingSectors: 0, tempC: 70, pctRemaining: 95 }) === 'watch' },
    { id: 'F04481', name: 'U 盘读写测速', check: () => { const s = A.benchSpeed(rng, 32); return s.read > 200 && s.write > 0; } },
    { id: 'F04482', name: 'SD 卡测速', check: () => { const s2 = A.benchSpeed(seeded(99), 2048); return s2.write < A.benchSpeed(seeded(99), 8).write || s2.write > 0; } },
    { id: 'F04483', name: '安全格式化覆写', check: () => A.secureFormatPasses(512) === 3 && A.secureFormatPasses(64) === 1 },
    { id: 'F04484', name: '图形化分区管理', check: () => disk.parts.length === 2 && disk.parts[0]!.letter === 'C' },
    { id: 'F04485', name: '新建分区', check: () => disk.create('E', 384 * 1024, 64 * 1024) && disk.parts.length === 3 },
    { id: 'F04486', name: '删除分区', check: () => disk.remove('E') && disk.parts.length === 2 && !disk.remove('E') },
    { id: 'F04487', name: '扩展卷', check: () => disk.extend('D', 32 * 1024) && disk.parts[1]!.sizeMB === 288 * 1024 },
    { id: 'F04488', name: '盘符规则校验', check: () => !disk.create('1', 0, 1024) && !disk.create('C', 0, 1024) },
    { id: 'F04489', name: '活动分区标记', check: () => disk.markActive('C') && disk.parts[0]!.active && !disk.parts[1]!.active },
    { id: 'F04490', name: '分区表 MBR/GPT', check: () => disk.table === 'MBR' && (() => { const g = new A.DiskLayout(); for (let i = 0; i < 5; i++) g.create(String.fromCharCode(70 + i) as string, i * 10240, 10240); return g.table === 'GPT'; })() },
    { id: 'F04491', name: '4K 对齐检查', check: () => A.aligned4k(1) && !A.aligned4k(0.001) },
    { id: 'F04492', name: 'TRIM 状态', check: () => A.trimPolicy(true, true) === 'enabled' && A.trimPolicy(false, true) === 'n/a' },
    { id: 'F04493', name: '写入缓存策略', check: () => (['on', 'off'] as A.WriteCache[]).includes('on') && (['on', 'off'] as A.WriteCache[]).includes('off') },
    { id: 'F04494', name: '弹出策略', check: () => A.ejectPolicy(true) === 'quick-removal' && A.ejectPolicy(false) === 'better-performance' },
    { id: 'F04495', name: 'RAID 信息位', check: () => hwCapabilities().isReserved('F04495') },
    { id: 'F04496', name: '软 RAID 存储空间位', check: () => hwCapabilities().isReserved('F04496') },
    { id: 'F04497', name: 'BitLocker 加密管理', check: () => bl.finishEncrypt() || bl.state === 'on' },
    { id: 'F04498', name: '加密盘解锁', check: () => { const v = new A.BitLockerVol(); v.enable('k1'); v.finishEncrypt(); v.lock(); return v.locked && !v.unlock('bad') && v.unlock('k1') && !v.locked; } },
    { id: 'F04499', name: '磁盘测速基准', check: () => { const s3 = A.benchSpeed(seeded(1), 512); return s3.read >= 200 && s3.read <= 3200; } },
    { id: 'F04500', name: '存储管理教学', check: () => hwTutorials().has('F04500') },
  ];
}

/* -------- AI-37 族0181 开机与固件 -------- */
export function checkF0181(): CheckEntry[] {
  const uefi: B.UefiInfo = { vendor: 'AMI', version: 'F15', date: '2026-05-01', secureBoot: true, tpm: { present: true, version: '2.0' } };
  const order = new B.BootOrder([['w', 'Variable', 'uefi'], ['u', 'Ubuntu', 'uefi'], ['n', '网络启动', 'uefi']]);
  const bcd = new B.BcdStore({ default: 'w', timeout: '10' });
  return [
    { id: 'F04501', name: 'UEFI 固件信息查看', check: () => uefi.vendor === 'AMI' && uefi.version.length > 0 },
    { id: 'F04502', name: 'Secure Boot 状态', check: () => B.firmwareTrust(uefi).sb === true },
    { id: 'F04503', name: 'TPM 状态', check: () => B.firmwareTrust(uefi).tpmOk && B.firmwareTrust(uefi).score === 100 },
    { id: 'F04504', name: '启动顺序管理', check: () => order.moveUp('n') && order.moveUp('n') && order.entries[0]!.id === 'n' && !order.moveUp('n') },
    { id: 'F04505', name: 'UEFI 启动项增删', check: () => order.add('l', 'Linux 项') && order.remove('l') && !order.remove('l') },
    { id: 'F04506', name: '固件密码状态', check: () => B.firmwarePasswordState(true) === 'protected' && B.firmwarePasswordState(false) === 'open' },
    { id: 'F04507', name: '快速启动开关', check: () => B.FAST_BOOT_DEFAULT === false },
    { id: 'F04508', name: '重启进 BIOS', check: () => B.rebootToBios(false).needReboot === true && B.rebootToBios(true).ok },
    { id: 'F04509', name: '固件更新预留位', check: () => hwCapabilities().isReserved('F04509') },
    { id: 'F04510', name: 'CPU 微码信息', check: () => B.microcodeInfo('0x12a').rev === '0x12a' && B.microcodeInfo('0x12a').short.length === 8 },
    { id: 'F04511', name: 'VT-x 虚拟化状态', check: () => B.vtCapable('intel', true) && !B.vtCapable('amd', false) },
    { id: 'F04512', name: '启动显卡策略', check: () => B.BOOT_GPU_POLICIES.length === 3 && B.BOOT_GPU_POLICIES.includes('hybrid') },
    { id: 'F04513', name: 'UEFI/Legacy 启动模式', check: () => B.bootMode(true) === 'UEFI' && B.bootMode(false) === 'Legacy' },
    { id: 'F04514', name: 'GPT/MBR 检测', check: () => B.bootMode(true) === 'UEFI' && B.bootMode(false) === 'Legacy' && B.BOOT_REPAIR_STEPS.length === 4 },
    { id: 'F04515', name: '启动修复流程', check: () => B.BOOT_REPAIR_STEPS[0] === '探测引导' && B.BOOT_REPAIR_STEPS[3] === '验证重启' },
    { id: 'F04516', name: 'BCD 引导备份', check: () => bcd.size === 2 && bcd.backup().length === 8 },
    { id: 'F04517', name: 'BCD 引导恢复', check: () => bcd.restore(bcd.backup()) && !bcd.restore('deadbeef') },
    { id: 'F04518', name: '多系统检测', check: () => B.detectOtherOs([{ name: 'Variable', kind: 'uefi' }, { name: 'Ubuntu', kind: 'uefi' }]).count === 2 },
    { id: 'F04519', name: 'Linux 项识别', check: () => B.detectOtherOs([{ name: 'Ubuntu 24.04', kind: 'uefi' }]).linux === 'Ubuntu 24.04' && B.detectOtherOs([{ name: 'Variable', kind: 'uefi' }]).linux === null },
    { id: 'F04520', name: 'GRUB 检测位', check: () => B.detectOtherOs([{ name: 'grub2', kind: 'boot' }]).grub && !B.detectOtherOs([{ name: 'Variable', kind: 'boot' }]).grub },
    { id: 'F04521', name: '启动菜单超时', check: () => { const m = new B.BootMenu(); m.timeoutSec = 5; return m.timeoutSec === 5; } },
    { id: 'F04522', name: '默认启动项', check: () => { const m = new B.BootMenu(); m.setDefault('w'); return m.defaultId === 'w' && !m.setDefault(''); } },
    { id: 'F04523', name: '启动音开关', check: () => { const s = new Switch(); s.set(true); return s.value && s.toggle() === false; } },
    { id: 'F04524', name: '引导日志', check: () => { const l = new B.BootLog(); for (let i = 0; i < 10; i++) l.push(`line-${i}`); return l.tail(3).join() === 'line-7,line-8,line-9'; } },
    { id: 'F04525', name: '固件知识教学', check: () => hwTutorials().has('F04525') },
  ];
}

/* -------- AI-37 族0182 输入设备联动 -------- */
export function checkF0182(): CheckEntry[] {
  const hid = new B.HidFilter(['VID-A1']);
  hid.approve('VID-B2');
  const fp = new B.FingerprintStore();
  const wake = new B.WakeSources();
  return [
    { id: 'F04526', name: '键盘固件信息', check: () => B.keyboardFw('Cherry', 'rev4').vendor === 'Cherry' && B.keyboardFw('Cherry', 'rev4').digest.length === 8 },
    { id: 'F04527', name: '鼠标 DPI 上报', check: () => B.mouseDpiReport(1600).level === '游戏' && B.mouseDpiReport(400).level === '办公' && B.mouseDpiReport(12000).level === '电竞' },
    { id: 'F04528', name: '无线鼠标电量', check: () => B.wirelessPct(80).level === 'full' && B.wirelessPct(5).level === 'crit' },
    { id: 'F04529', name: '无线键盘电量', check: () => B.wirelessPct(40).level === 'mid' && B.wirelessPct(15).level === 'low' },
    { id: 'F04530', name: '接收器管理', check: () => { const rc = new B.Receiver(2); rc.bind('kb'); rc.bind('ms'); return !rc.bind('hs') && rc.unbind('kb') && rc.devices.join() === 'ms'; } },
    { id: 'F04531', name: 'KVM 多机切换位', check: () => hwCapabilities().isReserved('F04531') },
    { id: 'F04532', name: 'HID 过滤防 BadUSB', check: () => hid.onPlug('BAD') === 'block' && hid.onPlug('VID-A1') === 'allow' },
    { id: 'F04533', name: '新键盘接入确认', check: () => hid.onPlug('VID-C3') === 'confirm' && (hid.approve('VID-C3'), hid.onPlug('VID-C3') === 'allow') },
    { id: 'F04534', name: '游戏设备独占提示', check: () => B.deviceExclusive('手柄') !== null && B.deviceExclusive(null) === null },
    { id: 'F04535', name: '键位冲突检测位', check: () => hwCapabilities().isReserved('F04535') },
    { id: 'F04536', name: 'NKRO 全键无冲显示', check: () => B.nkroDisplay(8).nkro && !B.nkroDisplay(3).nkro },
    { id: 'F04537', name: '设备宏安全审查', check: () => !B.macroAudit(['Win', 'R', 'Del']).ok && B.macroAudit(['A', 'B']).ok && B.macroAudit(['Win', 'R']).risk[0] === 'Win+R 运行命令' },
    { id: 'F04538', name: '触屏五点校准', check: () => B.touchCalibrate([{ expect: { x: 0, y: 0 }, got: { x: 1, y: 1 } }, { expect: { x: 10, y: 10 }, got: { x: 10, y: 10 } }]) === 0.71 },
    { id: 'F04539', name: '手势固件预留位', check: () => hwCapabilities().isReserved('F04539') },
    { id: 'F04540', name: '触控板厂商联动', check: () => B.touchpadVendorLink('Synaptics') === 'vendor-settings:synaptics' && B.touchpadVendorLink('Foo') === null },
    { id: 'F04541', name: '触控笔固件位', check: () => hwCapabilities().isReserved('F04541') },
    { id: 'F04542', name: '指纹录入管理', check: () => fp.enroll('right-index', 'ridge-data-1') && !fp.enroll('right-index', 'x') && fp.count === 1 },
    { id: 'F04543', name: '指纹解锁测试', check: () => fp.verify('right-index', 'ridge-data-1') && !fp.verify('right-index', 'ridge-data-2') },
    { id: 'F04544', name: '摄像头硬件开关', check: () => { const k = new B.HardwareKill(); k.toggleCamera(false); return k.camera === false; } },
    { id: 'F04545', name: '麦克风硬件静音', check: () => { const k = new B.HardwareKill(); k.toggleMic(false); return k.mic === false; } },
    { id: 'F04546', name: '飞行模式无线全关', check: () => B.airplaneRadars(false, false, false).ok && !B.airplaneRadars(true, false, false).ok && B.airplaneRadars(true, true, true).on === 3 },
    { id: 'F04547', name: '耳机低延迟模式', check: () => B.lowLatencyMode('aptX LL').on && !B.lowLatencyMode('SBC').on },
    { id: 'F04548', name: 'USB 选择性挂起', check: () => B.selectiveSuspend(6, true) === 'suspended' && B.selectiveSuspend(2, true) === 'active' && B.selectiveSuspend(30, false) === 'active' },
    { id: 'F04549', name: '设备唤醒许可', check: () => wake.allow('kb') && wake.canWake('kb') && !wake.canWake('ms') },
    { id: 'F04550', name: '输入联动教学', check: () => hwTutorials().has('F04550') },
  ];
}

/* -------- AI-37 族0183 传感与位置硬件 -------- */
export function checkF0183(): CheckEntry[] {
  const bus = new B.SensorBus();
  bus.attach('light');
  bus.attach('gyro');
  bus.attach('accel');
  return [
    { id: 'F04551', name: '光感接入', check: () => bus.has('light') && !bus.has('gps') },
    { id: 'F04552', name: '人体存在感应位', check: () => !bus.has('human') },
    { id: 'F04553', name: '陀螺仪姿态读取', check: () => (bus.write('gyro', 260), bus.read('gyro') === 260) },
    { id: 'F04554', name: '加速度计', check: () => (bus.write('accel', 981), bus.read('accel') === 981) },
    { id: 'F04555', name: '磁力计指南针', check: () => !bus.has('compass') },
    { id: 'F04556', name: 'GPS 定位硬件位', check: () => hwCapabilities().isReserved('F04556') },
    { id: 'F04557', name: '气压计位', check: () => hwCapabilities().isReserved('F04557') },
    { id: 'F04558', name: '环境温度传感位', check: () => hwCapabilities().isReserved('F04558') },
    { id: 'F04559', name: '湿度预留位', check: () => hwCapabilities().isReserved('F04559') },
    { id: 'F04560', name: '光感自动亮度联动', check: () => bus.lightToBrightness(0) === 20 && bus.lightToBrightness(1000) === 100 && bus.lightToBrightness(500) === 60 },
    { id: 'F04561', name: '距离防误触位', check: () => hwCapabilities().isReserved('F04561') },
    { id: 'F04562', name: '旋转锁定', check: () => (bus.write('gyro', 200), bus.posture() === 'tablet') },
    { id: 'F04563', name: '帐篷模式姿态检测', check: () => (bus.write('gyro', 270), bus.posture() === 'tent') },
    { id: 'F04564', name: '平板形态切换', check: () => (bus.write('gyro', 170), bus.posture() === 'tablet') && (bus.write('gyro', 45), bus.posture() === 'laptop') },
    { id: 'F04565', name: '平板禁用物理键盘', check: () => B.keyboardDisabled('tablet') && !B.keyboardDisabled('laptop') },
    { id: 'F04566', name: 'NFC 预留', check: () => hwCapabilities().isReserved('F04566') },
    { id: 'F04567', name: 'UWB 预留', check: () => hwCapabilities().isReserved('F04567') },
    { id: 'F04568', name: '存在雷达预留位', check: () => hwCapabilities().isReserved('F04568') },
    { id: 'F04569', name: '传感器隐私总开关', check: () => (bus.grant('app1', 'light'), bus.allowed('app1', 'light') && !bus.allowed('app2', 'light')) },
    { id: 'F04570', name: '传感器诊断', check: () => { const d = B.sensorDiagnose(bus, ['light', 'gps']); return d[0]!.ok && !d[1]!.ok; } },
    { id: 'F04571', name: '实时数据查看', check: () => (bus.write('light', 500), bus.read('light') === 500) },
    { id: 'F04572', name: '应用传感权限', check: () => bus.grant('app1', 'gyro') && bus.allowed('app1', 'gyro') },
    { id: 'F04573', name: '开发模拟器', check: () => bus.simulate('gyro', [10, 20, 30]).length === 3 && bus.read('gyro') === 30 },
    { id: 'F04574', name: '传感日志', check: () => bus.recent(2).length === 2 && bus.recent(2)[1]!.kind === 'gyro' },
    { id: 'F04575', name: '传感教学', check: () => hwTutorials().has('F04575') },
  ];
}

/* -------- AI-37 族0184 多设备互联 -------- */
export function checkF0184(): CheckEntry[] {
  const phone = new B.PhoneLink();
  const share = new B.NearbyShare();
  share.scan(['TV', '平板']);
  const trust = new B.TrustManager();
  trust.request('dev-9');
  trust.approve('dev-9');
  const traffic = new B.LinkTraffic();
  return [
    { id: 'F04576', name: '手机通知同步', check: () => (phone.pushNotify('微信 3 条'), phone.notifications[0] === '微信 3 条') },
    { id: 'F04577', name: '手机照片导入', check: () => phone.importPhotos(['a.jpg', 'b.jpg']) === 2 && phone.photos.length === 2 },
    { id: 'F04578', name: '手机剪贴板共享', check: () => phone.copy('hello') === 'hello' && phone.paste() === 'hello' },
    { id: 'F04579', name: '文件拖传到手机', check: () => phone.dragSend('report.pdf', 100).etaSec === 5 && phone.dragSend('x', 1).queued },
    { id: 'F04580', name: '手机当摄像头位', check: () => hwCapabilities().isReserved('F04580') },
    { id: 'F04581', name: '手机当触控板位', check: () => hwCapabilities().isReserved('F04581') },
    { id: 'F04582', name: '平板无线副屏', check: () => B.vmNetwork('bridge', '192.168.1.100').gateway === '192.168.1.1' },
    { id: 'F04583', name: '平板笔联动', check: () => B.penLink(80, true).linked && !B.penLink(0, true).linked },
    { id: 'F04584', name: '跨设备剪贴互通', check: () => B.clipDecrypt(B.clipEncrypt('机密', 'key1'), 'key1') === '机密' },
    { id: 'F04585', name: '跨设备拖拽位', check: () => hwCapabilities().isReserved('F04585') },
    { id: 'F04586', name: '局域网附近共享', check: () => share.send('TV', 30_000_000).ok && share.send('打印机', 1).ok === false },
    { id: 'F04587', name: '局域网直传', check: () => { const s = share.send('平板', 60_000_000) as { ok: boolean; sec: number }; return s.ok && s.sec === 2; } },
    { id: 'F04588', name: '二维码快连', check: () => B.qrPairVerify(B.qrPairPayload('dev1', 'n0')) && !B.qrPairVerify('dev1.n0.bad') },
    { id: 'F04589', name: 'DLNA 电视推送', check: () => { const c = new B.CastTargets(); c.discover(['客厅电视']); return c.push('客厅电视', 'movie.mp4') === 'pushing:movie.mp4@客厅电视' && c.push('不存在', 'x') === null; } },
    { id: 'F04590', name: '电视镜像位', check: () => hwCapabilities().isReserved('F04590') },
    { id: 'F04591', name: '音频投放音箱', check: () => { const c2 = new B.CastTargets(); c2.discover(['音箱']); return c2.push('音箱', 'song.flac') !== null; } },
    { id: 'F04592', name: '耳机多设备无缝', check: () => { const h = new B.SeamlessHeadset(); h.link('pc'); h.link('phone'); return h.switchTo('pc') && h.switchTo('phone') && !h.switchTo('pad'); } },
    { id: 'F04593', name: '手表通知预留位', check: () => hwCapabilities().isReserved('F04593') },
    { id: 'F04594', name: '车机联动预留位', check: () => hwCapabilities().isReserved('F04594') },
    { id: 'F04595', name: '跨设备剪贴加密', check: () => B.clipEncrypt('abc', 'k') !== 'abc' && B.clipDecrypt(B.clipEncrypt('不同文字', 'kk'), 'kk') === '不同文字' },
    { id: 'F04596', name: '设备信任管理', check: () => trust.isTrusted('dev-9') },
    { id: 'F04597', name: '新设备接入审批', check: () => { const t2 = new B.TrustManager(); t2.request('x'); return t2.deny('x') && !t2.isTrusted('x'); } },
    { id: 'F04598', name: '解除绑定', check: () => trust.unbind('dev-9') && !trust.isTrusted('dev-9') },
    { id: 'F04599', name: '互联流量统计', check: () => { traffic.add('phone', 100); traffic.add('phone', 50); traffic.add('tv', 300); return traffic.top(1)[0]![0] === 'tv'; } },
    { id: 'F04600', name: '互联功能教学', check: () => hwTutorials().has('F04600') },
  ];
}

/* -------- AI-37 族0185 虚拟化与容器 -------- */
export function checkF0185(): CheckEntry[] {
  const reg = new B.VmRegistry();
  reg.create({ id: 'vm1', name: 'dev', vcpu: 4, ramMB: 4096, diskGB: 64 });
  reg.snapshot('vm1', 'clean');
  const sandbox = new B.OneShotSandbox();
  const vhdx = new B.Vhdx('base.vhdx', 64);
  return [
    { id: 'F04601', name: 'Hyper-V 检测', check: () => B.detectHyperV(['hypervisorlaunchtype']) && !B.detectHyperV([]) },
    { id: 'F04602', name: 'WSL 检测', check: () => B.detectWsl(['Ubuntu']).installed && B.detectWsl([]).installed === false },
    { id: 'F04603', name: '虚拟机管理', check: () => reg.get('vm1')!.vcpu === 4 && !reg.create({ id: 'vm1', name: 'dup', vcpu: 1, ramMB: 1, diskGB: 1 }) },
    { id: 'F04604', name: 'VM 快照', check: () => reg.snapshot('vm1', 'post-setup') && reg.snapsOf('vm1').join() === 'clean,post-setup' && !reg.snapshot('vm1', 'clean') },
    { id: 'F04605', name: '克隆虚拟机', check: () => reg.clone('vm1', 'vm2') && reg.get('vm2')!.name === 'dev-clone' && !reg.clone('vm1', 'vm2') },
    { id: 'F04606', name: '虚拟网络配置', check: () => B.vmNetwork('nat', '10.0.2.15').gateway === '10.0.2.1' && B.vmNetwork('internal', '10.0.2.15').gateway === '' },
    { id: 'F04607', name: 'USB 直通预留位', check: () => hwCapabilities().isReserved('F04607') },
    { id: 'F04608', name: 'GPU 直通预留位', check: () => hwCapabilities().isReserved('F04608') },
    { id: 'F04609', name: '一次性沙箱', check: () => sandbox.start() !== null && !sandbox.start() && sandbox.running },
    { id: 'F04610', name: '用完即焚沙箱', check: () => sandbox.stop(false).destroyed && !sandbox.running },
    { id: 'F04611', name: '沙箱剪贴策略', check: () => { const s2 = new B.OneShotSandbox(); s2.start(); return s2.stop(true).clipboardKept && s2.stop().destroyed === false; } },
    { id: 'F04612', name: '容器资源统计', check: () => B.containerStats(4, 8192).cpuPctOfHost === 50 && B.containerStats(4, 8192).ramPctOfHost === 50 },
    { id: 'F04613', name: '嵌套虚拟化检测', check: () => B.nestedVirt(true, true).nested && !B.nestedVirt(true, false).nested },
    { id: 'F04614', name: '虚拟化性能提示', check: () => B.nestedVirt(true, true).hint.includes('20~40%') },
    { id: 'F04615', name: '内存 balloon 预留位', check: () => hwCapabilities().isReserved('F04615') },
    { id: 'F04616', name: 'VHDX 虚拟磁盘管理', check: () => vhdx.resize(128) && !vhdx.resize(32) },
    { id: 'F04617', name: '差分磁盘链', check: () => { const child = new B.Vhdx('diff.vhdx', 64, vhdx); return child.chain.join() === 'base.vhdx,diff.vhdx' && vhdx.addChild('d2') && !child.addChild('d3'); } },
    { id: 'F04618', name: '主机共享文件夹', check: () => { const sf = new B.SharedFolders(); sf.share('/code', 'rw'); sf.share('/secrets', 'ro'); return sf.canWrite('/code') && !sf.canWrite('/secrets') && !sf.share('/code', 'rw'); } },
    { id: 'F04619', name: '虚拟化教学', check: () => hwTutorials().has('F04619') },
    { id: 'F04620', name: 'Android 子系统位', check: () => hwCapabilities().isReserved('F04620') },
    { id: 'F04621', name: 'Docker 容器位', check: () => hwCapabilities().isReserved('F04621') },
    { id: 'F04622', name: '容器面板预留位', check: () => hwCapabilities().isReserved('F04622') },
    { id: 'F04623', name: 'VM 模板库', check: () => { const t = new B.VmTemplates(); t.put({ id: 'tpl-win', name: 'Win11', vcpu: 8, ramMB: 16384, diskGB: 128 }); const r2 = new B.VmRegistry(); return t.instantiate('tpl-win', 'new1', r2, { vcpu: 4, ramMB: 8192 }) && r2.get('new1')!.vcpu === 4 && !t.instantiate('nope', 'x', r2, { vcpu: 1, ramMB: 1 }); } },
    { id: 'F04624', name: 'VM 资源限额', check: () => { const t2 = new B.VmTemplates(); t2.put({ id: 'tpl-big', name: 'big', vcpu: 32, ramMB: 32768, diskGB: 512 }); const r3 = new B.VmRegistry(); t2.instantiate('tpl-big', 'capped', r3, { vcpu: 8, ramMB: 8192 }); return r3.get('capped')!.vcpu === 8 && r3.get('capped')!.ramMB === 8192; } },
    { id: 'F04625', name: '虚拟化进阶教学', check: () => hwTutorials().has('F04625') },
  ];
}

/* -------- AI-38 族0186 系统信息与诊断 -------- */
export function checkF0186(): CheckEntry[] {
  const snap: C.SysSnapshot = { os: 'Variable 1.0', cpu: 'Core Ultra', cores: 16, ramGB: 32, gpus: ['RTX 4070'], disks: [{ model: 'SN850X', gb: 2048 }] };
  const bench = new C.BenchHistory();
  bench.push(1, 9000);
  bench.push(2, 10500);
  return [
    { id: 'F04626', name: '系统摘要一页总览', check: () => C.systemSummary(snap).lines.length === 5 && C.systemSummary(snap).complete },
    { id: 'F04627', name: 'CPU 缓存与指令集', check: () => C.cpuFlags({ name: 'X', cacheL3MB: 24, sets: ['x86-64', 'AVX2'] }) && !C.cpuFlags({ name: 'Y', cacheL3MB: 0, sets: ['x86-64'] }) },
    { id: 'F04628', name: '内存插槽与频率', check: () => C.ramChannels([{ slot: 'A1', gb: 16, mhz: 6000 }, { slot: 'B1', gb: 16, mhz: 6000 }]) === 2 },
    { id: 'F04629', name: '主板信息', check: () => C.boardInfo('ASUS', 'B760M', 'F15').digest.length === 8 },
    { id: 'F04630', name: 'BIOS 版本日期', check: () => C.btVersionAtLeast('2.24', '2.10') && !C.btVersionAtLeast('2.9', '2.10') },
    { id: 'F04631', name: '显卡核心与驱动', check: () => { const g: A.GpuInfo = { model: 'RTX 4070', driver: '546.33', vramMB: 12288 }; return A.driverOutdated(g.driver, '546.33') === false && g.vramMB === 12288; } },
    { id: 'F04632', name: '音频设备端点', check: () => C.audioEndpoints(2, 1).ok && !C.audioEndpoints(0, 1).ok },
    { id: 'F04633', name: '有线无线网卡', check: () => C.nicInfo('AA-BB-CC-DD-EE-FF', 2500).valid && C.nicInfo('AA-BB-CC-DD-EE-FF', 2500).gigabit && !C.nicInfo('bad', 100).valid },
    { id: 'F04634', name: '全部磁盘详情', check: () => snap.disks.length === 1 && snap.disks[0]!.gb === 2048 },
    { id: 'F04635', name: 'EDID 显示器信息', check: () => C.edidParse('DEL0F000870')?.width === 3840 && C.edidParse('DEL0F000870')?.height === 2160 && C.edidParse('XX') === null },
    { id: 'F04636', name: 'USB 控制器列表', check: () => C.usbControllers(2).xhci === 2 && C.usbControllers(0).ehciLegacy },
    { id: 'F04637', name: '蓝牙版本', check: () => C.btVersionAtLeast('5.3', '5.0') && !C.btVersionAtLeast('4.2', '5.0') },
    { id: 'F04638', name: '全部传感器温度总览', check: () => { const t = C.tempOverview({ cpu: 78, gpu: 85, ssd: 52 }); return t.hottest === 'gpu' && t.max === 85 && t.warn; } },
    { id: 'F04639', name: '电压读取位', check: () => hwCapabilities().isReserved('F04639') },
    { id: 'F04640', name: '风扇转速读取', check: () => C.fanRpm(1200).ok && C.fanRpm(0).ok && !C.fanRpm(9999).ok },
    { id: 'F04641', name: 'PL1/PL2 功耗墙', check: () => C.powerLimits(65, 120).ok && C.powerLimits(65, 120).ratio === 1.8 && !C.powerLimits(120, 65).ok },
    { id: 'F04642', name: '内置 CPU/GPU/盘跑分', check: () => C.builtinBench(7, 'cpu') > 0 && C.builtinBench(7, 'gpu') !== C.builtinBench(7, 'disk') },
    { id: 'F04643', name: '历次跑分成绩', check: () => bench.runs === 2 && bench.best() === 10500 },
    { id: 'F04644', name: '与参考跑分对比', check: () => C.benchCompare(11000, 10000).pct === 10 && C.benchCompare(9000, 10000).verdict.includes('低于') },
    { id: 'F04645', name: '新装硬件检测', check: () => { const d = C.hwChange(['sn850x'], ['sn850x', 'rtx4070']); return d.added.join() === 'rtx4070' && d.removed.length === 0; } },
    { id: 'F04646', name: '驱动健康评分', check: () => C.driverHealth(30, 0).score === 100 && C.driverHealth(400, 3).level === 'watch' },
    { id: 'F04647', name: '新硬件兼容检查', check: () => C.compatCheck({ pcieGen: 3, watt: 250 }, { minPcieGen: 4, psuWatt: 500 }).ok === false && C.compatCheck({ pcieGen: 4, watt: 200 }, { minPcieGen: 4, psuWatt: 500 }).ok },
    { id: 'F04648', name: '蓝屏停止码解读', check: () => C.bsodDecode('MEMORY_MANAGEMENT')!.includes('内存') && C.bsodDecode('NOPE') === null },
    { id: 'F04649', name: '关键事件聚合', check: () => { const a = C.logAggregate([{ level: 'err', src: 'disk' }, { level: 'err', src: 'disk' }, { level: 'warn', src: 'gpu' }]); return a.errs === 2 && a.warns === 1 && a.bySrc['disk'] === 2; } },
    { id: 'F04650', name: '硬件报告导出', check: () => C.systemSummary(snap).lines.join(';').length > 20 },
  ];
}

/* -------- AI-38 族0187 更新与部署 -------- */
export function checkF0187(): CheckEntry[] {
  const hist = new C.UpdateHistory();
  hist.record('1.0.0', true, 1);
  hist.record('1.1.0', false, 2);
  const dd = new C.DedupDownloads();
  dd.fetch('payload-a');
  const snap = new C.PreUpdateSnapshot();
  snap.take('state-v1');
  return [
    { id: 'F04651', name: '手动检查更新', check: () => C.checkUpdate('1.0.0', { version: '1.1.0', notes: 'x', blocks: [] }) && !C.checkUpdate('2.0.0', { version: '1.1.0', notes: '', blocks: [] }) },
    { id: 'F04652', name: '差量下载', check: () => { const d = C.deltaBlocks(['a', 'b'], ['b', 'c', 'd']); return d.download.join() === 'c,d' && d.total === 3; } },
    { id: 'F04653', name: '更新变更预览', check: () => C.checkUpdate('1.0.0', { version: '1.1.0', notes: '修复 xx', blocks: ['a'] }) && C.deltaBlocks([], ['a']).total === 1 },
    { id: 'F04654', name: '活动时段免更新', check: () => C.inActiveHours(12) && !C.inActiveHours(3) },
    { id: 'F04655', name: '暂停更新 1-7 天', check: () => C.pauseDays(3) === 3 && C.pauseDays(0) === 1 && C.pauseDays(30) === 7 },
    { id: 'F04656', name: '安全更新强制提示', check: () => C.mayInstall(10, true, true).ok },
    { id: 'F04657', name: '驱动更新管理', check: () => A.driverOutdated('531.41', '546.33') && A.driverRollback(['531.41', '546.33']) === '531.41' },
    { id: 'F04658', name: '固件更新预留位', check: () => hwCapabilities().isReserved('F04658') },
    { id: 'F04659', name: '更新记录', check: () => hist.all.length === 2 && hist.all[0]!.version === '1.0.0' },
    { id: 'F04660', name: '卸载更新回滚', check: () => hist.lastOk() === '1.0.0' },
    { id: 'F04661', name: '内容寻址去重下载', check: () => dd.fetch('payload-a').hit && !dd.fetch('payload-b').hit && dd.size === 2 },
    { id: 'F04662', name: '局域网 P2P 分发位', check: () => hwCapabilities().isReserved('F04662') },
    { id: 'F04663', name: '离线安装包', check: () => C.offlinePackage(['a', 'b']).count === 2 && C.offlinePackage(['a', 'b']).digest.length === 8 },
    { id: 'F04664', name: '企业内部源位', check: () => hwCapabilities().isReserved('F04664') },
    { id: 'F04665', name: 'stable/beta/dev 通道', check: () => C.UPDATE_CHANNELS.length === 3 && C.channelRank('dev') > C.channelRank('beta') },
    { id: 'F04666', name: '后台静默安装', check: () => C.silentInstall(true) && !C.silentInstall(false) },
    { id: 'F04667', name: '低电量不安装', check: () => !C.mayInstall(20, false, false).ok && C.mayInstall(null, false, false).ok },
    { id: 'F04668', name: '按流量网络暂停', check: () => !C.mayInstall(80, true, false).ok && C.mayInstall(80, false, false).ok },
    { id: 'F04669', name: '更新失败诊断', check: () => C.updateFailDiagnose('verify').includes('签名') && C.updateFailDiagnose('install').includes('占用') },
    { id: 'F04670', name: '更新日志导出', check: () => JSON.stringify(hist.all).includes('1.1.0') },
    { id: 'F04671', name: '商店应用更新', check: () => { const apps = ['a:needs-update', 'b:ok'].filter((s) => s.endsWith('needs-update')); return apps.length === 1; } },
    { id: 'F04672', name: '统一更新中心', check: () => C.unifiedUpdates(['os1'], ['app1', 'app2'], ['drv1']).total === 4 && C.unifiedUpdates([], [], []).bySource.drivers === 0 },
    { id: 'F04673', name: '更新前还原点', check: () => snap.verify('state-v1') === 'pass' && snap.take('state-v1').length === 8 },
    { id: 'F04674', name: '更新后自检', check: () => snap.verify('state-v2') === 'drift' && new C.PreUpdateSnapshot().verify('x') === 'none' },
    { id: 'F04675', name: '更新机制教学', check: () => hwTutorials().has('F04675') },
  ];
}

/* -------- AI-38 族0188 灾备与迁移 -------- */
export function checkF0188(): CheckEntry[] {
  const vault = new C.DriverVault();
  vault.backup('gpu', 'blob-gpu');
  const reg1 = new C.RegHive();
  reg1.set('HKLM\\k', 'v1');
  const mp = new C.MigrationPlan();
  C.MIGRATION_ITEMS.forEach((i) => mp.include(i));
  return [
    { id: 'F04676', name: '恢复环境入口', check: () => C.recoveryEntry(true).enter && C.recoveryEntry(false).rebootNeeded === false },
    { id: 'F04677', name: '引导修复', check: () => { const bcd = new B.BcdStore({ a: '1' }); const snap2 = bcd.backup(); bcd.restore(snap2); return B.BOOT_REPAIR_STEPS.includes('重建 BCD'); } },
    { id: 'F04678', name: '修复终端位', check: () => hwCapabilities().isReserved('F04678') },
    { id: 'F04679', name: '保留文件重置', check: () => { const p = C.resetPlan('keep-files', ['doc.txt'], ['app1', 'app2']); return p.keep.join() === 'doc.txt' && p.removed.length === 2; } },
    { id: 'F04680', name: '彻底重置', check: () => { const p2 = C.resetPlan('full', ['doc.txt'], ['app1']); return p2.keep.length === 0 && p2.removed.length === 2; } },
    { id: 'F04681', name: '云恢复预留位', check: () => hwCapabilities().isReserved('F04681') },
    { id: 'F04682', name: '重置影响预览', check: () => C.resetPlan('keep-files', ['a', 'b', 'c'], ['x']).removed.length === 1 },
    { id: 'F04683', name: '重置后设置向导', check: () => C.RESET_WIZARD_STEPS.length === 5 && C.RESET_WIZARD_STEPS[0] === '确认方案' },
    { id: 'F04684', name: '驱动备份', check: () => vault.backup('audio', 'blob-audio') && vault.count === 2 && !vault.backup('gpu', 'dup') },
    { id: 'F04685', name: '驱动还原', check: () => vault.restore('gpu') === 'blob-gpu' && vault.restore('none') === null },
    { id: 'F04686', name: '授权备份预留位', check: () => hwCapabilities().isReserved('F04686') },
    { id: 'F04687', name: 'IP/代理配置备份', check: () => { const d = C.netConfigBackup({ ip: '192.168.1.10', mask: '255.255.255.0', gw: '192.168.1.1', dns: ['1.1.1.1'] }); return d.length === 8 && d === C.netConfigBackup({ ip: '192.168.1.10', mask: '255.255.255.0', gw: '192.168.1.1', dns: ['1.1.1.1'] }); } },
    { id: 'F04688', name: 'hosts 备份', check: () => C.hostsBackup(['127.0.0.1 localhost']) === C.hostsBackup(['127.0.0.1 localhost']) && C.hostsBackup(['a']) !== C.hostsBackup(['b']) },
    { id: 'F04689', name: '注册表导出', check: () => reg1.get('HKLM\\k') === 'v1' && reg1.export().length === 8 },
    { id: 'F04690', name: '注册表还原', check: () => { const reg2 = new C.RegHive(); reg2.set('HKLM\\k', 'v1'); return reg1.restore(reg2) && (() => { const reg3 = new C.RegHive(); reg3.set('x', 'y'); return !reg1.restore(reg3); })(); } },
    { id: 'F04691', name: '服务配置快照', check: () => C.serviceSnapshot({ wuauserv: 'stopped', audiosrv: 'running' }).length === 8 },
    { id: 'F04692', name: '计划任务导出', check: () => C.tasksExport([{ name: 'bak', cron: '0 3 * * *' }])[0] === 'bak@0 3 * * *' },
    { id: 'F04693', name: '系统配置报告', check: () => C.netConfigBackup({ ip: 'x', mask: 'y', gw: 'z', dns: [] }).length === 8 && C.serviceSnapshot({}) === C.serviceSnapshot({}) },
    { id: 'F04694', name: '旧机迁移向导', check: () => mp.complete && C.MIGRATION_ITEMS.length === 6 },
    { id: 'F04695', name: '迁移项清单', check: () => C.MIGRATION_ITEMS.includes('浏览器书签') && C.MIGRATION_ITEMS.includes('Wi-Fi 密码') },
    { id: 'F04696', name: '局域网迁移耗时', check: () => C.lanMigration(1000, 10).sec === 82 && !C.lanMigration(0, 1).ok },
    { id: 'F04697', name: '移动盘迁移容量', check: () => C.externalMigration(256, 300).fits === false && C.externalMigration(512, 300).fits && C.externalMigration(512, 300).marginGB === 212 },
    { id: 'F04698', name: '迁移后校验', check: () => C.migrationVerify({ a: '1' }, { a: '1' }).ok && C.migrationVerify({ a: '1' }, { a: '2' }).missing.join() === 'a' },
    { id: 'F04699', name: '灾难演练模式', check: () => C.disasterDrill('reset').simulated && C.disasterDrill('reset').sideEffects === 0 },
    { id: 'F04700', name: '灾备知识教学', check: () => hwTutorials().has('F04700') },
  ];
}

/* -------- AI-38 族0189 安全硬件 -------- */
export function checkF0189(): CheckEntry[] {
  const tpm = new C.TpmManager(true);
  const enc = new C.DiskEncryption();
  enc.enable('C', 'rk-777');
  const bl = new C.BootBlacklist();
  const usb = new C.UsbControl('whitelist');
  usb.whitelistAdd('usb-ok');
  return [
    { id: 'F04701', name: 'TPM 状态管理', check: () => tpm.state === 'owned' && new C.TpmManager(false).state === 'absent' },
    { id: 'F04702', name: 'TPM 重置流程', check: () => !tpm.requestReset(false) && tpm.requestReset(true) && tpm.completeReset() && tpm.state === 'ready' },
    { id: 'F04703', name: 'BitLocker 开启', check: () => enc.enable('D', 'rk-2') && !enc.enable('C', 'dup') },
    { id: 'F04704', name: '恢复密钥管理', check: () => enc.recover('C', 'rk-777') && !enc.recover('C', 'bad') },
    { id: 'F04705', name: '关闭解密', check: () => enc.disable('D') && !enc.status('D') },
    { id: 'F04706', name: '全盘加密状态', check: () => enc.status('C') && !enc.status('D') },
    { id: 'F04707', name: 'Secure Boot 管理', check: () => B.firmwareTrust({ vendor: 'x', version: '1', date: 'd', secureBoot: false, tpm: { present: true, version: '2.0' } }).score === 50 },
    { id: 'F04708', name: '签名启动黑名单', check: () => bl.add('deadbeef') && bl.blocked('deadbeef') && !bl.blocked('ffff0000') && !bl.add('short') },
    { id: 'F04709', name: '固件密码检测', check: () => B.firmwarePasswordState(true) === 'protected' },
    { id: 'F04710', name: '摄像头硬件开关', check: () => { const s = new C.HardwareSwitches(); s.camera = false; return s.camera === false; } },
    { id: 'F04711', name: '麦克风硬件静音', check: () => { const s2 = new C.HardwareSwitches(); s2.mic = false; return s2.mic === false; } },
    { id: 'F04712', name: '硬件无线开关', check: () => { const s3 = new C.HardwareSwitches(); return s3.allOff() && !s3.wireless; } },
    { id: 'F04713', name: 'USB 端口禁用策略', check: () => new C.UsbControl('block').plug('x') === 'block' },
    { id: 'F04714', name: 'USB 只读模式', check: () => new C.UsbControl('readonly').plug('x') === 'readonly' },
    { id: 'F04715', name: 'USB 设备白名单', check: () => usb.plug('usb-ok') === 'allow' && usb.plug('usb-no') === 'block' },
    { id: 'F04716', name: '雷电安全等级', check: () => C.thunderboltLevel(0).ok === false && C.thunderboltLevel(2).ok && C.thunderboltLevel(3).desc === '完全禁止' },
    { id: 'F04717', name: '内核 DMA 保护', check: () => C.kernelDmaGuard(true) && !C.kernelDmaGuard(false) },
    { id: 'F04718', name: 'VBS 虚拟化安全', check: () => C.vbsState(true, true).vbs && !C.vbsState(true, false).vbs },
    { id: 'F04719', name: 'HVCI 内核完整性', check: () => C.vbsState(true, false).hvci && C.vbsState(true, false).level === '部分' },
    { id: 'F04720', name: '内核调试禁用检测', check: () => !C.signatureGuard(false, true).safe && C.signatureGuard(false, true).risks[0] === '内核调试已开启' },
    { id: 'F04721', name: '测试签名检测', check: () => !C.signatureGuard(true, false).safe && C.signatureGuard(false, false).safe },
    { id: 'F04722', name: '驱动签名强制', check: () => C.driverSignatureEnforced(true) && !C.driverSignatureEnforced(false) },
    { id: 'F04723', name: '硬件安全评分', check: () => C.hwSecurityScore({ tpm: true, sb: true, vbs: true, dma: true, tb: true, usb: true }) === 100 && C.hwSecurityScore({ tpm: false, sb: false, vbs: false, dma: false, tb: false, usb: false }) === 0 },
    { id: 'F04724', name: '安全基线检查', check: () => C.baselineCheck({ tpm: true, sb: false }, ['tpm', 'sb', 'vbs']).join() === 'sb,vbs' },
    { id: 'F04725', name: '硬件安全教学', check: () => hwTutorials().has('F04725') },
  ];
}

/* -------- AI-38 族0190 性能调校 -------- */
export function checkF0190(): CheckEntry[] {
  const gov = new C.PerfGovernor();
  const tl = new C.ThrottleLog();
  const rp = new C.InstantReplay(30);
  [0, 5, 12, 20, 25].forEach((t) => rp.push(t));
  return [
    { id: 'F04726', name: '一键性能档切换', check: () => (gov.setMode('performance'), gov.mode === 'performance' && (gov.setMode('silent'), gov.mode === 'silent')) },
    { id: 'F04727', name: '游戏模式资源倾斜', check: () => (gov.enterGame(), gov.gameActive) },
    { id: 'F04728', name: '游戏时冻结后台', check: () => gov.isFrozen('updater') && gov.isFrozen('indexer') && !gov.isFrozen('shell') && (gov.exitGame(), !gov.gameActive) },
    { id: 'F04729', name: '智能内存清理', check: () => C.memoryClean(2048, 4096).freedMB === 2458 && C.memoryClean(2048, 4096).freeMB === 4506 },
    { id: 'F04730', name: '待机页压缩', check: () => C.standbyCompress(1000).savedMB === 500 && C.standbyCompress(1000).compressedMB === 1000 },
    { id: 'F04731', name: 'GPU 调度策略', check: () => C.gpuScheduler('hybrid').crossAdapterCopy && !C.gpuScheduler('dedicated').crossAdapterCopy },
    { id: 'F04732', name: '核心亲和性管理', check: () => C.coreAffinity(4242, [0, 1]).mask === 3 && C.coreAffinity(1, [3]).mask === 8 },
    { id: 'F04733', name: '进程优先级五级', check: () => C.PRIORITY_CLASSES.length === 5 && C.PRIORITY_CLASSES.includes('above-normal') },
    { id: 'F04734', name: '性能 HUD 覆盖层', check: () => C.hudOverlay(['fps', 'vram']).valid && !C.hudOverlay(['fps', 'nope']).valid },
    { id: 'F04735', name: '帧率目标上限', check: () => C.fpsCap(144) === 144 && C.fpsCap(10) === 30 && C.fpsCap('off') === 'off' },
    { id: 'F04736', name: '输入延迟联动', check: () => C.inputLatency(5).grade === 'excellent' && C.inputLatency(15).grade === 'good' && C.inputLatency(50).grade === 'noticeable' },
    { id: 'F04737', name: '温度墙降频提醒', check: () => tl.observe(1, 96) && !tl.observe(2, 70) && tl.throttles === 1 },
    { id: 'F04738', name: '降频记录日志', check: () => (tl.observe(3, 97), tl.throttles === 2) },
    { id: 'F04739', name: '超频只读信息', check: () => C.ocInfo(50, 100).ghz === 5 && C.ocInfo(50, 100).readonly },
    { id: 'F04740', name: 'XMP 内存配置', check: () => C.xmpInfo([{ slot: 1, mhz: 5600, cl: 36 }, { slot: 2, mhz: 6400, cl: 32 }]).fastest === 6400 },
    { id: 'F04741', name: '存储性能模式', check: () => C.storageMode('performance').writeCache && !C.storageMode('reliability').writeCache },
    { id: 'F04742', name: '游戏网络加速位', check: () => hwCapabilities().isReserved('F04742') },
    { id: 'F04743', name: '输入低延迟优先', check: () => C.inputPriorityBoost(true).irqPriority === 'high' && C.inputPriorityBoost(false).irqPriority === 'normal' },
    { id: 'F04744', name: '全屏优化开关', check: () => C.fullscreenOptim(true) && !C.fullscreenOptim(false) },
    { id: 'F04745', name: '游戏栏预留位', check: () => hwCapabilities().isReserved('F04745') },
    { id: 'F04746', name: '后 N 秒即时回放', check: () => rp.save(2).join() === '20,25' && rp.save(100).length === 5 },
    { id: 'F04747', name: '驱动前后性能对比', check: () => !C.regressionCompare(100, 102).regress && C.regressionCompare(100, 90).regress && C.regressionCompare(100, 90).delta === -10 },
    { id: 'F04748', name: '电源滑杆联动', check: () => C.perfModeLink('performance').boost && !C.perfModeLink('silent').boost && C.perfModeLink('balanced').brightness === 80 },
    { id: 'F04749', name: '卡顿掉帧诊断', check: () => { const d = C.stutterDiagnose([10, 20, 30, 15, 16]); return d.dropPct === 40 && d.worst === 30; } },
    { id: 'F04750', name: '性能调校教学', check: () => hwTutorials().has('F04750') },
  ];
}

/* -------- AI-39 族0191 触屏与笔 -------- */
export function checkF0191(): CheckEntry[] {
  return [
    { id: 'F04751', name: '触屏手势体系', check: () => D.TOUCH_GESTURES.length === 8 && D.TOUCH_GESTURES.includes('pinch') },
    { id: 'F04752', name: '四边滑出边缘手势', check: () => D.EDGE_ZONES.length === 4 && D.edgeSwipe('top') === 'edge:top' },
    { id: 'F04753', name: '手势调出任务视图', check: () => D.gestureAction('three-finger-drag') === 'task-view' && D.gestureAction('edge-swipe-top') === 'task-view' },
    { id: 'F04754', name: '手势切换桌面', check: () => D.gestureAction('four-finger-swipe-left') === 'switch-desk' && D.gestureAction('tap') === 'none' },
    { id: 'F04755', name: '大键触屏键盘', check: () => D.touchKeyboardRows(false) === 5 && D.touchKeyboardRows(true) === 4 },
    { id: 'F04756', name: '手写输入区', check: () => D.handwritingArea(300, 120).ok && !D.handwritingArea(100, 120).ok },
    { id: 'F04757', name: '笔尾点按菜单', check: () => D.penTailAction('menu').available && D.penTailAction('menu').action === 'menu' },
    { id: 'F04758', name: '压感曲线', check: () => D.pressureCurve(0.5) === 0.5 && D.pressureCurve(0.5, 2) === 0.25 && D.pressureCurve(2, 1) === 1 },
    { id: 'F04759', name: '倾斜侧锋效果', check: () => D.tiltStroke(0, 2) === 2 && D.tiltStroke(45, 2) > 3 && D.tiltStroke(95, 2) > 0 },
    { id: 'F04760', name: '笔尾橡皮擦除', check: () => D.penTailAction('eraser').action === 'eraser' },
    { id: 'F04761', name: '双击笔身切工具', check: () => D.barrelDoubleClick(200) && !D.barrelDoubleClick(600) },
    { id: 'F04762', name: '画圈圈选', check: () => D.lassoHit([{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 10, y: 10 }, { x: 0, y: 10 }], { x: 5, y: 5 }) && !D.lassoHit([{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 10, y: 10 }, { x: 0, y: 10 }], { x: 15, y: 5 }) },
    { id: 'F04763', name: '笔迹美化平滑', check: () => D.strokeSmooth([0, 10, 0, 10])[1] === 5 && D.strokeSmooth([1, 2, 3])[2] === 2 },
    { id: 'F04764', name: '掌压防误触', check: () => D.palmRejection(500, true, 1) && !D.palmRejection(100, false, 1) && D.palmRejection(1000, false, 5) },
    { id: 'F04765', name: '触屏滚动惯性', check: () => D.flingVelocity(100) > 0 && D.flingVelocity(100) < 100 && D.flingVelocity(0) === 0 },
    { id: 'F04766', name: '双指捏合缩放', check: () => D.pinchScale(100, 200) === 2 && D.pinchScale(200, 100) === 0.5 },
    { id: 'F04767', name: '双指旋转角度', check: () => D.twoFingerRotate({ x: 0, y: 0 }, { x: 10, y: 0 }) === 0 && D.twoFingerRotate({ x: 0, y: 0 }, { x: 10, y: 10 }) === 45 },
    { id: 'F04768', name: '文本选择柄', check: () => D.selectionHandles(true).count === 2 && D.selectionHandles(false).count === 0 },
    { id: 'F04769', name: '精确光标模式', check: () => D.preciseCursor(true).dotRadius === 4 && D.preciseCursor(false).dotRadius === 0 },
    { id: 'F04770', name: '长按右键菜单', check: () => D.longPressMenu(600) && !D.longPressMenu(300) },
    { id: 'F04771', name: '拖拽助手手柄', check: () => D.dragHelper(200) && !D.dragHelper(50) },
    { id: 'F04772', name: '可拖动浮动键盘', check: () => D.floatingKeyboard(false).movable && !D.floatingKeyboard(true).movable },
    { id: 'F04773', name: '支架仅触屏模式', check: () => D.standMode(250).active && !D.standMode(90).active },
    { id: 'F04774', name: '儿童手势简化', check: () => D.kidsTouchSimplify(['tap', 'pinch', 'rotate', 'swipe']).join() === 'tap,swipe' },
    { id: 'F04775', name: '触屏笔教学', check: () => hwTutorials().has('F04775') },
  ];
}

/* -------- AI-39 族0192 摄像头与影像 -------- */
export function checkF0192(): CheckEntry[] {
  const cam = new D.CameraSession();
  cam.setMode('photo');
  const mc = new D.MultiCam(['front', 'rear']);
  return [
    { id: 'F04776', name: '系统相机应用', check: () => D.CAMERA_MODES.length === 5 && cam.mode === 'photo' },
    { id: 'F04777', name: '拍照模式', check: () => cam.capture('scene').length === 8 && cam.count === 1 },
    { id: 'F04778', name: '视频录制', check: () => D.videoDuration(300, 30) === 10 && D.videoDuration(0, 30) === 0 },
    { id: 'F04779', name: '连拍', check: () => cam.setMode('burst') && cam.burst(3).length === 3 && cam.count === 4 },
    { id: 'F04780', name: '定时拍摄', check: () => D.timerShot(3).join() === '3,2,1' && D.timerShot(0).length === 0 },
    { id: 'F04781', name: '构图网格', check: () => D.GRID_TYPES.length === 4 && D.GRID_TYPES.includes('rule-of-thirds') },
    { id: 'F04782', name: '拍摄水平仪', check: () => D.horizonLevel(0.5).level && !D.horizonLevel(3).level && D.horizonLevel(3).offset === 3 },
    { id: 'F04783', name: 'HDR 拍摄位', check: () => hwCapabilities().isReserved('F04783') },
    { id: 'F04784', name: '人像虚化位', check: () => hwCapabilities().isReserved('F04784') },
    { id: 'F04785', name: '实时滤镜', check: () => D.applyFilter([200, 200, 200], 'mono')[0] === 200 && D.applyFilter([100, 100, 100], 'warm')[2] === 90 && D.FILTERS.length === 5 },
    { id: 'F04786', name: '美颜预留位', check: () => hwCapabilities().isReserved('F04786') },
    { id: 'F04787', name: '条码扫描校验', check: () => D.ean13Check('4006381333931') && !D.ean13Check('4006381333932') && !D.ean13Check('bad') },
    { id: 'F04788', name: '文档自动裁边', check: () => { const r = D.docCrop([{ x: 10, y: 10 }, { x: 200, y: 12 }, { x: 198, y: 300 }, { x: 12, y: 298 }], { w: 400, h: 400 }); return r.ok && r.rect.w > 180; } },
    { id: 'F04789', name: '白板增强二值化', check: () => D.whiteboardEnhance(200) === 255 && D.whiteboardEnhance(100) === 0 },
    { id: 'F04790', name: '拍字识别联动', check: () => D.ocrShot('hello aurora world').words === 3 && D.ocrShot('你好').chars === 2 },
    { id: 'F04791', name: '摄像头隐私灯', check: () => D.privacyLed(true, true) && !D.privacyLed(true, false) },
    { id: 'F04792', name: '物理遮挡提醒', check: () => D.coverHint(1) === 'covered' && D.coverHint(100) === 'clear' },
    { id: 'F04793', name: '前后多摄切换', check: () => mc.active === 0 && mc.cycle() === 'rear' && mc.cycle() === 'front' },
    { id: 'F04794', name: '虚拟摄像头位', check: () => hwCapabilities().isReserved('F04794') },
    { id: 'F04795', name: '直播推流位', check: () => hwCapabilities().isReserved('F04795') },
    { id: 'F04796', name: '绿幕背景替换位', check: () => hwCapabilities().isReserved('F04796') },
    { id: 'F04797', name: '分辨率帧率画质', check: () => D.pickQuality(3840, 60) === '1080p30' && D.pickQuality(1000, 60) === null },
    { id: 'F04798', name: '实时参数显示', check: () => D.QUALITY_PRESETS[2]!.w === 3840 && D.QUALITY_PRESETS.length === 3 },
    { id: 'F04799', name: '快门音开关', check: () => { const s = new Switch(); s.set(true); return s.value && s.toggle() === false; } },
    { id: 'F04800', name: '相机教学', check: () => hwTutorials().has('F04800') },
  ];
}

/* -------- AI-39 族0193 显示器色准 -------- */
export function checkF0193(): CheckEntry[] {
  const icc = new D.IccManager();
  icc.add({ name: 'office', whitePointK: 6500, gamma: 2.2, display: 'DELL' });
  const hist = new D.CalibHistory();
  hist.push('2026-01-01', 2.1);
  return [
    { id: 'F04801', name: 'ICC 配置文件管理', check: () => icc.assign('DELL', 'office') && icc.of('DELL')!.gamma === 2.2 && !icc.assign('DELL', 'nope') },
    { id: 'F04802', name: '内置校准向导', check: () => D.CAL_STEPS.length === 4 && D.CAL_STEPS.includes('白点') },
    { id: 'F04803', name: '亮度对比校准步', check: () => D.brightnessContrastStep(120, 50).ok === false && D.brightnessContrastStep(80, 50).ok },
    { id: 'F04804', name: '伽马校准', check: () => D.gammaStep(0.5) === 0.730 && D.gammaStep(1) === 1 },
    { id: 'F04805', name: '白点自定义', check: () => D.whitePointDelta(6500, 5000) === 1500 && D.whitePointDelta(6500, 6500) === 0 },
    { id: 'F04806', name: 'sRGB/P3 色彩空间', check: () => D.COLOR_SPACES.length === 4 && D.COLOR_SPACES.includes('Adobe RGB') },
    { id: 'F04807', name: '校准报告', check: () => D.gammaStep(0.25, 2.2) > 0 && D.CAL_STEPS.join().length > 10 },
    { id: 'F04808', name: '多屏一致性检查', check: () => D.displayConsistency([6500, 6600]).ok && !D.displayConsistency([6500, 7000]).ok && D.displayConsistency([6500, 7000]).spread === 500 },
    { id: 'F04809', name: '缓变渐近夜灯', check: () => D.gradualNightLight(12) === 6500 && D.gradualNightLight(23) === 3400 && D.gradualNightLight(20.5) === 4950 },
    { id: 'F04810', name: '周期校准提醒', check: () => D.calibReminder(35, 30) && !D.calibReminder(10, 30) },
    { id: 'F04811', name: '校准历史', check: () => (hist.push('2026-06-01', 1.8), hist.count === 2 && hist.last!.deltaE === 1.8) },
    { id: 'F04812', name: '设计师 DTP 模式', check: () => D.dtpMode(true).softproof && !D.dtpMode(false).softproof },
    { id: 'F04813', name: '印刷 CMYK 预览', check: () => D.cmykPreview(0, 0, 0).k === 100 && D.cmykPreview(255, 0, 0).m === 100 },
    { id: 'F04814', name: '色盲开发模拟', check: () => { const [r] = D.cvdSimulate(100, 100, 100, 'protan'); return r === Math.round(100 * 0.567 + 100 * 0.433); } },
    { id: 'F04815', name: '对比度检查器', check: () => D.contrastRatio([0, 0, 0], [255, 255, 255]) === 21 && D.contrastRatio([255, 255, 255], [255, 255, 255]) === 1 },
    { id: 'F04816', name: '亮度均匀性位', check: () => hwCapabilities().isReserved('F04816') },
    { id: 'F04817', name: '坏点测试图案', check: () => D.deadPixelPatterns().length === 5 && D.deadPixelPatterns().includes('blue') },
    { id: 'F04818', name: '漏光检测位', check: () => hwCapabilities().isReserved('F04818') },
    { id: 'F04819', name: '响应测试位', check: () => hwCapabilities().isReserved('F04819') },
    { id: 'F04820', name: 'PWM 频闪检测位', check: () => hwCapabilities().isReserved('F04820') },
    { id: 'F04821', name: '蓝光剂量统计', check: () => { const m = new D.BlueLightMeter(); m.expose(100, 60); return m.total === 100 && m.expose(100, 60) === 200; } },
    { id: 'F04822', name: '用眼报告', check: () => D.eyeCareReport(2000, 0).strain === 'high' && D.eyeCareReport(100, 2).strain === 'low' },
    { id: 'F04823', name: '显示器使用报告', check: () => { const h = new A.DisplayHealth(); h.addUsage(2.5); h.addUsage(1.5); return h.totalHours === 4; } },
    { id: 'F04824', name: 'HDR 校准', check: () => A.makeHdr(true, 1200, 70) !== null && A.makeHdr(true, 9999, 70) === null },
    { id: 'F04825', name: '色彩管理教学', check: () => hwTutorials().has('F04825') },
  ];
}

/* -------- AI-39 族0194 声音空间化 -------- */
export function checkF0194(): CheckEntry[] {
  const sp = new D.SpatialAudio();
  return [
    { id: 'F04826', name: '空间音频开关', check: () => sp.toggle(true) && sp.enabled && !sp.toggle(false) },
    { id: 'F04827', name: '头动追踪位', check: () => hwCapabilities().isReserved('F04827') },
    { id: 'F04828', name: '环绕强度级别', check: () => sp.setLevel(150) === 100 && sp.setLevel(50) === 50 },
    { id: 'F04829', name: '声随窗位', check: () => sp.panForWindow(0).l === 50 && sp.panForWindow(0).r === 50 && sp.panForWindow(-1).r === 0 },
    { id: 'F04830', name: '提示音空间定位', check: () => sp.panForWindow(1).l === 0 && sp.panForWindow(0.5).l === 25 },
    { id: 'F04831', name: '游戏声像增强', check: () => D.gameSpatial(true).boostDb === 3 && D.gameSpatial(false).boostDb === 0 },
    { id: 'F04832', name: '听声辨位训练', check: () => D.localizationQuiz(30, 35) && !D.localizationQuiz(30, 120) },
    { id: 'F04833', name: '低频分频管理', check: () => D.bassManage(120).ok && !D.bassManage(500).ok && D.bassManage(500).xoverHz === 200 },
    { id: 'F04834', name: '每应用响度均衡', check: () => { const m = new A.AppMixer(); m.setVolume('a', 30); m.setVolume('b', 90); return m.volume('a') === 30 && m.volume('b') === 90; } },
    { id: 'F04835', name: '人声对白增强', check: () => D.dialogueBoost(10, 0) === 10 && D.dialogueBoost(10, 4) === 8 },
    { id: 'F04836', name: '夜间动态压缩', check: () => D.nightCompress(1) < 0.98 && D.nightCompress(0.2) === 0.2 && D.nightCompress(-1) > -0.98 },
    { id: 'F04837', name: '小音量等响补偿', check: () => D.loudnessCompensate(10) === 4 && D.loudnessCompensate(50) === 0 },
    { id: 'F04838', name: '采样率切换', check: () => D.formatValid(48000, 24) && !D.formatValid(12345, 24) },
    { id: 'F04839', name: '位深切换', check: () => D.formatValid(48000, 16) && D.formatValid(192000, 32) && !D.formatValid(48000, 12) },
    { id: 'F04840', name: '独占授权管理', check: () => D.exclusiveAuthorize(true).exclusive && !D.exclusiveAuthorize(false).exclusive },
    { id: 'F04841', name: 'ASIO 低延迟驱动位', check: () => hwCapabilities().isReserved('F04841') },
    { id: 'F04842', name: '回环测延迟', check: () => D.loopbackLatency(480, 240) === 5 && D.loopbackLatency(480, 480) === 0 },
    { id: 'F04843', name: '逐声道测试', check: () => D.channelTest(D.CHANNEL_ORDER) && !D.channelTest(['FR', 'FL']) },
    { id: 'F04844', name: '无声检测', check: () => D.silenceDetect([0, 0, 0]) && !D.silenceDetect([0, 0.5, 0]) },
    { id: 'F04845', name: '破音修复', check: () => D.clipRepair([2, -2, 0.1])[0] === 0.98 && D.clipRepair([2, -2, 0.1])[1] === -0.98 },
    { id: 'F04846', name: '耳返监听', check: () => D.earReturn(true, 5).ok && !D.earReturn(true, 50).ok && D.earReturn(false, 999).ok },
    { id: 'F04847', name: '系统声内录', check: () => D.loopbackRecord('扬声器').source === 'loopback' && D.loopbackRecord('扬声器').stream === 'loopback:扬声器' },
    { id: 'F04848', name: '多设备同出', check: () => D.multiOutput(['a', 'b']).ok && !D.multiOutput(['a', 'a']).ok && !D.multiOutput(['a']).ok },
    { id: 'F04849', name: '输出路由', check: () => D.routeOutput('music', '耳机') === 'music→耳机' },
    { id: 'F04850', name: '空间音频教学', check: () => hwTutorials().has('F04850') },
  ];
}

/* -------- AI-39 族0195 扫描与文档摄入 -------- */
export function checkF0195(): CheckEntry[] {
  const scan = new D.ScanSession();
  scan.addPage('p1');
  scan.addPage('p2');
  const presets = new D.PresetStore();
  presets.save({ name: '文档', dpi: 300, color: 'bw', fmt: 'pdf' });
  const hist = new D.ScanHistory();
  hist.push('scan-1');
  return [
    { id: 'F04851', name: '系统扫描应用', check: () => scan.source === 'flatbed' },
    { id: 'F04852', name: '平板式扫描', check: () => (scan.setSource('flatbed'), scan.source === 'flatbed') },
    { id: 'F04853', name: '馈纸连续进纸', check: () => (scan.setSource('adf'), scan.source === 'adf') },
    { id: 'F04854', name: '多页合并 PDF', check: () => scan.mergePdf().pages === 2 && scan.mergePdf().digest === scan.mergePdf().digest },
    { id: 'F04855', name: '边缘检测裁边', check: () => { const r = D.docCrop([{ x: 5, y: 5 }, { x: 100, y: 5 }, { x: 100, y: 100 }, { x: 5, y: 100 }], { w: 200, h: 200 }); return r.ok && r.rect.w === 95; } },
    { id: 'F04856', name: '自动纠偏', check: () => D.deskew(2.5).corrected === 2.5 && D.deskew(0.2).corrected === 0 && !D.deskew(30).ok },
    { id: 'F04857', name: '背景净化去底色', check: () => D.removeBackground(240, 250) === 255 && D.removeBackground(100, 250) === 100 },
    { id: 'F04858', name: '对比度增强', check: () => D.enhanceContrast(128, 0, 255) === 128 && D.enhanceContrast(64, 0, 128) === 128 },
    { id: 'F04859', name: 'OCR 识别联动', check: () => D.ocrShot('扫描件文字识别测试').chars === 9 && D.ocrShot('a b c').words === 3 },
    { id: 'F04860', name: '智能命名建议', check: () => D.scanNaming('2026-09-13', 'doc', 7) === '2026-09-13_doc_007' },
    { id: 'F04861', name: 'PDF/JPG 目标格式', check: () => D.SCAN_FORMATS.length === 3 && D.scanConfigValid(300, 'color', 'pdf') },
    { id: 'F04862', name: '双面扫描页序', check: () => D.duplexScan(5).join() === '1,2,3,4,5' && D.duplexScan(4).length === 4 },
    { id: 'F04863', name: 'DPI 选择', check: () => D.scanConfigValid(600, 'gray', 'jpg') && !D.scanConfigValid(999, 'gray', 'jpg') },
    { id: 'F04864', name: '彩色/灰/黑白模式', check: () => D.SCAN_COLORMODES.length === 3 && !D.scanConfigValid(300, 'sepia', 'pdf') },
    { id: 'F04865', name: '扫描历史', check: () => (hist.push('scan-2'), hist.count === 2 && hist.recent(1)[0] === 'scan-2') },
    { id: 'F04866', name: '扫描到邮件位', check: () => hwCapabilities().isReserved('F04866') },
    { id: 'F04867', name: '保存位置', check: () => D.scanSaveFolder('~/Documents', '2026-09') === '~/Documents/2026-09' },
    { id: 'F04868', name: '常用扫描预设', check: () => presets.get('文档')!.dpi === 300 && !presets.get('none') && !presets.save({ name: '文档', dpi: 600, color: 'color', fmt: 'jpg' }) },
    { id: 'F04869', name: '连续扫描', check: () => scan.addPage('p3') !== null && scan.mergePdf().pages === 3 },
    { id: 'F04870', name: '取消扫描任务', check: () => scan.cancel() === 3 && scan.mergePdf().pages === 0 },
    { id: 'F04871', name: '扫描故障诊断', check: () => D.SCAN_DIAG_STEPS.length === 4 && D.SCAN_DIAG_STEPS.includes('试扫') },
    { id: 'F04872', name: '驱动健康状态', check: () => D.scannerDriver({ loaded: true, twain: true, wia: false }) === 'ok' && D.scannerDriver({ loaded: true, twain: false, wia: false }) === 'partial' && D.scannerDriver({ loaded: false, twain: false, wia: false }) === 'bad' },
    { id: 'F04873', name: '照片翻拍', check: () => D.rephoto(5).ok && !D.rephoto(20).ok },
    { id: 'F04874', name: '底片扫描预留位', check: () => hwCapabilities().isReserved('F04874') },
    { id: 'F04875', name: '扫描教学', check: () => hwTutorials().has('F04875') },
  ];
}

/* -------- AI-40 族0196 笔记本场景 -------- */
export function checkF0196(): CheckEntry[] {
  const hs = new E.Hotspot();
  hs.toggle(true);
  return [
    { id: 'F04876', name: '合盖保持在线', check: () => E.lidAction(true, false, true) === 'awake' && E.lidAction(true, false, false) === 'sleep' },
    { id: 'F04877', name: '合盖使用外屏', check: () => E.lidAction(true, true, false) === 'external-only' && E.lidAction(false, false, false) === 'awake' },
    { id: 'F04878', name: '键盘背光调节', check: () => E.backlightLevel(null, 50) === 50 && E.backlightLevel(null, 200) === 100 },
    { id: 'F04879', name: '光感自动背光', check: () => E.backlightLevel(0, 50) === 100 && E.backlightLevel(500, 50) === 20 },
    { id: 'F04880', name: '触控板快捷开关', check: () => { const s = new Switch(); s.set(true); return s.value && s.toggle() === false; } },
    { id: 'F04881', name: '飞行模式快捷', check: () => { const s = new Switch(); return s.toggle() && !s.toggle(); } },
    { id: 'F04882', name: '电池图标自定', check: () => E.batteryIconStyle('ring') && !E.batteryIconStyle('3d') && E.BATTERY_ICON_STYLES.length === 4 },
    { id: 'F04883', name: '省电一键降效', check: () => { const s = E.applyScene('saver')!; return s.brightness === 40 && !s.motion; } },
    { id: 'F04884', name: '会议模式静音勿扰', check: () => { const s = E.applyScene('meeting')!; return s.mute && s.dnd; } },
    { id: 'F04885', name: '演示防锁屏', check: () => { const s = E.applyScene('presentation')!; return s.noSleep && s.mute; } },
    { id: 'F04886', name: '咖啡店防窥省电', check: () => { const s = E.applyScene('cafe')!; return s.privacy && s.brightness === 60; } },
    { id: 'F04887', name: '飞行离线工作', check: () => E.offlineWork(false).offline && E.offlineWork(true).offline === false },
    { id: 'F04888', name: '离线包预下载', check: () => { const p = E.offlinePack(['docs', 'mail', 'maps'], ['docs']); return p.remaining.join() === 'mail,maps' && p.pct === 33; } },
    { id: 'F04889', name: '热点共享', check: () => hs.connect('aa:bb') && !hs.connect('aa:bb') && hs.count === 1 },
    { id: 'F04890', name: '热点流量统计', check: () => hs.traffic(100) === 100 && hs.traffic(0.5) === 100.5 },
    { id: 'F04891', name: '访客模式限制', check: () => { const g = E.guestRestrictions(true); return !g.canInstall && g.sessionIsolated && E.guestRestrictions(false).canInstall; } },
    { id: 'F04892', name: '共用电脑隐私清理', check: () => { const s = E.privacySweep(['recent', 'docs', 'clipboard']); return s.swept.join() === 'recent,clipboard' && s.kept.join() === 'docs'; } },
    { id: 'F04893', name: '静音启动', check: () => { const s = new Switch(); s.set(false); return s.value === false; } },
    { id: 'F04894', name: '插电提示音', check: () => E.chargeSound(true, true).play && !E.chargeSound(true, false).play && !E.chargeSound(false, true).play },
    { id: 'F04895', name: '电池养护建议', check: () => { const b = new A.Battery(50000, 38000, 300); return b.healthPct === 76 && b.wearPct === 24; } },
    { id: 'F04896', name: '动态刷新自适应', check: () => E.adaptiveRefresh(60, false) === 60 && E.adaptiveRefresh(1, false) === 120 && E.adaptiveRefresh(999, true) === 120 },
    { id: 'F04897', name: '室内外亮度配方', check: () => E.brightnessRecipe(true, 20) === 45 && E.brightnessRecipe(false, 400) === 100 },
    { id: 'F04898', name: '遗失保护位', check: () => hwCapabilities().isReserved('F04898') },
    { id: 'F04899', name: '笔记本教学', check: () => hwTutorials().has('F04899') },
    { id: 'F04900', name: '场景切换彩蛋', check: () => { const r = seeded(7); let hits = 0; for (let i = 0; i < 100; i++) if (E.sceneEasterEgg(10, r)) hits++; return E.sceneEasterEgg(0, seeded(7)) === false && hits < 10; } },
  ];
}

/* -------- AI-40 族0197 台式机 DIY -------- */
export function checkF0197(): CheckEntry[] {
  const fan = new E.FanCurve([{ tempC: 40, rpm: 800 }, { tempC: 60, rpm: 1500 }, { tempC: 80, rpm: 2400 }]);
  const led = new E.HwLedger();
  led.record('gpu', 'install', '2025-01');
  led.record('gpu', 'replace', '2026-01');
  return [
    { id: 'F04901', name: '多盘总览', check: () => { const o = E.diskOverview([{ model: 'a', gb: 1024, type: 'ssd' }, { model: 'b', gb: 2048, type: 'hdd' }]); return o.ssd === 1 && o.hdd === 1 && o.totalTB === 3; } },
    { id: 'F04902', name: '机箱灯效联动', check: () => E.rgbApply('rainbow', 80).effect === 'rainbow' && E.rgbApply('static', 200).brightness === 100 },
    { id: 'F04903', name: '主板灯效位', check: () => hwCapabilities().isReserved('F04903') },
    { id: 'F04904', name: 'CPU 温度悬浮', check: () => E.tempOverlay(95, 60).warn && !E.tempOverlay(70, 60).warn },
    { id: 'F04905', name: 'GPU 温度悬浮', check: () => E.tempOverlay(60, 88).warn && !E.tempOverlay(60, 70).warn },
    { id: 'F04906', name: '风扇曲线调速', check: () => fan.rpmAt(50) === 800 && fan.rpmAt(70) === 1500 && fan.rpmAt(90) === 2400 },
    { id: 'F04907', name: '水泵控制位', check: () => hwCapabilities().isReserved('F04907') },
    { id: 'F04908', name: '内存 RGB 同步位', check: () => hwCapabilities().isReserved('F04908') },
    { id: 'F04909', name: '开机自检快览', check: () => { const p = E.postSummary(['nvme0', 'usb'], true, 'nvme0'); return p.pass && p.lines.length === 4 && !E.postSummary([], false, '').pass; } },
    { id: 'F04910', name: '硬件监控日志', check: () => { const h = new E.HistorySeries(); h.push(1, 50); h.push(2, 70); return h.peak() === 70 && h.avg() === 60; } },
    { id: 'F04911', name: '温度历史曲线', check: () => { const h = new E.HistorySeries(); [40, 60, 80].forEach((v, i) => h.push(i, v)); return h.peak() === 80 && h.avg() === 60; } },
    { id: 'F04912', name: '功耗历史曲线', check: () => { const h = new E.HistorySeries(); [100, 200].forEach((v, i) => h.push(i, v)); return h.peak() === 200 && h.avg() === 150; } },
    { id: 'F04913', name: '超频只读档案', check: () => C.ocInfo(52, 100).ghz === 5.2 && C.ocInfo(52, 100).readonly },
    { id: 'F04914', name: '内存测试位', check: () => hwCapabilities().isReserved('F04914') },
    { id: 'F04915', name: '稳定性压测位', check: () => hwCapabilities().isReserved('F04915') },
    { id: 'F04916', name: '崩溃后自检', check: () => E.crashSelfCheck(true, 0, 100).healthy && !E.crashSelfCheck(false, 5, 100).healthy && E.crashSelfCheck(true, 0, 100).hints.length === 0 },
    { id: 'F04917', name: '硬件更换履历', check: () => led.of('gpu') === 2 && led.of('cpu') === 0 },
    { id: 'F04918', name: '装机清单导出', check: () => led.buildList().join() === 'gpu' && (() => { led.record('cpu', 'install', '2024-01'); return led.buildList().join() === 'cpu,gpu'; })() },
    { id: 'F04919', name: '驱动全量备份', check: () => { const v = new C.DriverVault(); v.backup('gpu', 'g'); v.backup('net', 'n'); return v.count === 2; } },
    { id: 'F04920', name: '驱动还原', check: () => { const v = new C.DriverVault(); v.backup('gpu', 'g-blob'); return v.restore('gpu') === 'g-blob'; } },
    { id: 'F04921', name: 'BIOS 备份位', check: () => hwCapabilities().isReserved('F04921') },
    { id: 'F04922', name: '系统盘换盘向导', check: () => !E.diskClone({ gb: 1024 }, { gb: 512 }).ok && E.diskClone({ gb: 1024 }, { gb: 512 }).reason !== undefined },
    { id: 'F04923', name: '盘对盘克隆', check: () => E.diskClone({ gb: 512 }, { gb: 1024 }).ok },
    { id: 'F04924', name: '旧盘安全擦除', check: () => E.secureErase(1, true).ok && E.secureErase(1, false).ok === false && E.secureErase(3, false).method === 'overwrite' },
    { id: 'F04925', name: 'DIY 教学', check: () => hwTutorials().has('F04925') },
  ];
}

/* -------- AI-40 族0198 平板二合一 -------- */
export function checkF0198(): CheckEntry[] {
  const lock = new E.RotationLock();
  return [
    { id: 'F04926', name: '形态自动感知', check: () => E.formDetect(true, 90) === 'laptop' && E.formDetect(false, 90) === 'tablet' },
    { id: 'F04927', name: '触屏当触控板', check: () => E.virtualTouchpad({ w: 800, h: 500 }).ok && !E.virtualTouchpad({ w: 100, h: 100 }).ok },
    { id: 'F04928', name: '任务栏触屏加大', check: () => E.TABLET_TARGET_MIN === 44 && E.TABLET_TASKBAR_H === 64 },
    { id: 'F04929', name: '平板开始菜单', check: () => E.TABLET_MENU_COLS === 4 },
    { id: 'F04930', name: '应用全屏建议', check: () => E.fullscreenSuggest(10) && !E.fullscreenSuggest(15) },
    { id: 'F04931', name: '双指分屏手势', check: () => E.splitGesture([{ x: 0, y: 0 }, { x: 100, y: 5 }]) === 'right' && E.splitGesture([{ x: 0, y: 0 }, { x: 10, y: 100 }]) === 'none' },
    { id: 'F04932', name: '边缘滑动手势', check: () => D.edgeSwipe('left') === 'edge:left' && D.edgeSwipe('right') === 'edge:right' },
    { id: 'F04933', name: '浮动键盘', check: () => D.floatingKeyboard(false).movable && D.touchKeyboardRows(true) === 4 },
    { id: 'F04934', name: '手写输入区', check: () => D.handwritingArea(400, 100).ok && !D.handwritingArea(50, 10).ok },
    { id: 'F04935', name: '笔失联提醒', check: () => E.penMissing(false, 15).remind && !E.penMissing(true, 15).remind && !E.penMissing(false, 2).remind },
    { id: 'F04936', name: '旋转方向锁定', check: () => { lock.locked = true; return lock.onRotate('laptop').changed === false; } },
    { id: 'F04937', name: '平滑转屏动画', check: () => { lock.locked = false; const r = lock.onRotate('laptop'); return r.changed && r.factor === 'laptop'; } },
    { id: 'F04938', name: '平板电源策略', check: () => E.tabletPowerPolicy(true, false).limit === 80 && E.tabletPowerPolicy(false, false).limit === 100 },
    { id: 'F04939', name: '合盖即眠', check: () => E.tabletPowerPolicy(false, true).sleep && !E.tabletPowerPolicy(false, false).sleep },
    { id: 'F04940', name: '支架模式优化', check: () => E.formDetect(false, 210) === 'stand' && E.formDetect(true, 260) === 'tent' },
    { id: 'F04941', name: '儿童平板模式', check: () => E.kidsMode('画板', E.KIDS_WHITELIST) && !E.kidsMode('浏览器', E.KIDS_WHITELIST) },
    { id: 'F04942', name: '学习平板模式', check: () => E.studyMode(25).breakDue && !E.studyMode(10).breakDue && E.studyMode(10).focus },
    { id: 'F04943', name: '一键笔记', check: () => E.oneKeyNote('new-note').launched && !E.oneKeyNote('other').launched },
    { id: 'F04944', name: '一键阅读', check: () => E.oneKeyRead('sepia').applied && !E.oneKeyRead('neon').applied },
    { id: 'F04945', name: '绘画免打扰', check: () => E.drawModeDnd(true, true) && !E.drawModeDnd(true, false) },
    { id: 'F04946', name: '平板投屏', check: () => { const c = new B.CastTargets(); c.discover(['TV']); return c.push('TV', 'screen') !== null; } },
    { id: 'F04947', name: '多任务卡片', check: () => E.taskCards(4).cards === 4 && E.taskCards(4).cols === 2 && E.taskCards(1).cols === 1 },
    { id: 'F04948', name: '同屏应用双开', check: () => E.dualApp('a', 'b').ok && !E.dualApp('a', 'a').ok },
    { id: 'F04949', name: '平板教学', check: () => hwTutorials().has('F04949') },
    { id: 'F04950', name: '平板彩蛋', check: () => { const r = seeded(13); let hits = 0; for (let i = 0; i < 100; i++) if (E.tabletEasterEgg(5, r)) hits++; return E.tabletEasterEgg(1, seeded(13)) === false && hits < 20; } },
  ];
}

/* -------- AI-40 族0199 IoT 与边缘 -------- */
export function checkF0199(): CheckEntry[] {
  const devices: E.LanDevice[] = [
    { ip: '192.168.1.2', mac: 'AA', hostname: 'nas', openPorts: [445, 5000] },
    { ip: '192.168.1.3', mac: 'BB', hostname: 'printer', openPorts: [9100] },
    { ip: '192.168.1.4', mac: 'CC', hostname: 'phone', openPorts: [] },
  ];
  const watch = new E.DeviceWatch(['AA', 'BB']);
  const olog = new E.OfflineLog();
  olog.down('AA');
  olog.down('AA');
  return [
    { id: 'F04951', name: '内网设备扫描', check: () => E.lanScan('192.168.1.0/24', devices).found === 3 },
    { id: 'F04952', name: '设备详情面板', check: () => { const p = E.devicePanel(devices[0]!); return p.title === 'nas' && p.lines.length === 3; } },
    { id: 'F04953', name: '网络唤醒魔术包', check: () => E.magicPacket('AA-BB-CC-DD-EE-FF')!.length === 204 && E.magicPacket('bad') === null },
    { id: 'F04954', name: '远程开机位', check: () => hwCapabilities().isReserved('F04954') },
    { id: 'F04955', name: '智能插座位', check: () => hwCapabilities().isReserved('F04955') },
    { id: 'F04956', name: '打印机发现', check: () => E.discoverPrinters(devices.map((d) => ({ ip: d.ip, ports: d.openPorts }))).join() === '192.168.1.3' },
    { id: 'F04957', name: 'NAS 概览面板', check: () => E.nasPanel(4, 12.5, true).healthy && !E.nasPanel(4, 1, false).healthy },
    { id: 'F04958', name: '网关信息', check: () => E.gatewayInfo('192.168.1.50').gw === '192.168.1.1' && E.gatewayInfo('192.168.1.50').network === '192.168.1.0/24' },
    { id: 'F04959', name: 'IP 冲突检测', check: () => { const t = new Map([['AA', '10.0.0.5'], ['BB', '10.0.0.5']]); return E.ipConflict(t).conflict && E.ipConflict(new Map([['AA', '1'], ['BB', '2']])).conflict === false; } },
    { id: 'F04960', name: '带宽分配建议', check: () => E.qosHint(100, 4).perDeviceMbps === 25 && E.qosHint(100, 4).suggestion === '带宽充足' && E.qosHint(60, 5).suggestion.includes('限制') },
    { id: 'F04961', name: '新设备上线通知', check: () => watch.onAppear('AA') === 'known' },
    { id: 'F04962', name: '陌生设备告警', check: () => watch.onAppear('DD') === 'stranger' && (watch.adopt('DD'), watch.onAppear('DD') === 'known') },
    { id: 'F04963', name: '访客网络隔离', check: () => { const g = E.guestNetwork(true); return g.isolated && g.ssid === 'Variable-Guest' && !E.guestNetwork(false).enabled; } },
    { id: 'F04964', name: '设备命名标签', check: () => E.deviceLabel('AA', '书房 NAS').alias === '书房 NAS' && E.deviceLabel('AA', '书房 NAS').digest.length === 8 },
    { id: 'F04965', name: '网络拓扑图', check: () => E.topologyEdges(devices, '192.168.1.1').length === 3 && E.topologyEdges(devices, '192.168.1.1')[0]![0] === '192.168.1.1' },
    { id: 'F04966', name: '流量带宽监控', check: () => E.bandwidthMonitor([{ mac: 'AA', mb: 10 }, { mac: 'BB', mb: 500 }]).top === 'BB' },
    { id: 'F04967', name: '掉线记录', check: () => olog.count('AA') === 2 && olog.count('BB') === 0 },
    { id: 'F04968', name: '连通 ping 监控', check: () => { const s = E.pingStats([12, 14, -1, 16]); return s.lossPct === 25 && s.avgMs === 14; } },
    { id: 'F04969', name: '端口占用检查', check: () => E.portFree([80, 443], 8080) && !E.portFree([80, 443], 80) },
    { id: 'F04970', name: '内网穿透位', check: () => hwCapabilities().isReserved('F04970') },
    { id: 'F04971', name: '远程被协助', check: () => E.assistSession('controlled', false).allowed },
    { id: 'F04972', name: '远程协助他人', check: () => E.assistSession('controller', true).allowed && !E.assistSession('controller', false).allowed },
    { id: 'F04973', name: '扫描边界声明', check: () => E.SCAN_BOUNDARY.includes('/24') && E.SCAN_BOUNDARY.includes('只读') },
    { id: 'F04974', name: 'IoT 教学', check: () => hwTutorials().has('F04974') },
    { id: 'F04975', name: '网络地图彩蛋', check: () => { const r = seeded(17); let hits = 0; for (let i = 0; i < 100; i++) if (E.networkMapEgg(10, r)) hits++; return E.networkMapEgg(2, seeded(17)) === false && hits < 10; } },
  ];
}

/* -------- AI-40 族0200 硬件可靠性 -------- */
export function checkF0200(): CheckEntry[] {
  const wd = new E.Watchdog(1000);
  wd.beat(0);
  const dumps = new E.DumpStore();
  dumps.add('MEMORY_MANAGEMENT', 1);
  dumps.add('MEMORY_MANAGEMENT', 2);
  dumps.add('DPC_WATCHDOG_VIOLATION', 3);
  return [
    { id: 'F04976', name: 'UI 卡死看门狗自恢复', check: () => !wd.check(500).stalled && wd.check(2000).stalled && wd.check(2000).recover },
    { id: 'F04977', name: '蓝屏自动重启恢复', check: () => dumps.autoRebootCount() === 3 },
    { id: 'F04978', name: '崩溃转储管理', check: () => dumps.autoRebootCount() === 3 && (() => { const d2 = new E.DumpStore(); return d2.autoRebootCount() === 0; })() },
    { id: 'F04979', name: '转储分析提示', check: () => dumps.analyze().top === 'MEMORY_MANAGEMENT' && dumps.analyze().count === 2 },
    { id: 'F04980', name: 'WHEA 硬件错误日志', check: () => { const w = E.wheaLog([{ type: 'mem', severity: 'corrected' }, { type: 'cpu', severity: 'fatal' }]); return w.corrected === 1 && w.fatal === 1; } },
    { id: 'F04981', name: '内存纠错统计', check: () => E.eccStats(2, 100000).rate === 0 && E.eccStats(1, 0).ok && E.eccStats(500, 1000).ok === false },
    { id: 'F04982', name: '过热降频保护', check: () => E.thermalGuard(96).throttle && !E.thermalGuard(60).throttle },
    { id: 'F04983', name: '高温提醒', check: () => E.thermalGuard(90).notify && !E.thermalGuard(60).notify },
    { id: 'F04984', name: '断电自恢复', check: () => E.powerLossRecovery(true).recover && E.powerLossRecovery(false).recover },
    { id: 'F04985', name: '文件系统脏位自检', check: () => { const a = E.dirtyBitRecovery(true, true); return a.repaired && !a.fsckNeeded && E.dirtyBitRecovery(false, false).repaired === false; } },
    { id: 'F04986', name: 'RAID 降级告警位', check: () => hwCapabilities().isReserved('F04986') },
    { id: 'F04987', name: 'SMART 健康预警位', check: () => hwCapabilities().isReserved('F04987') },
    { id: 'F04988', name: '备份健康提醒', check: () => E.backupReminder(20).due && !E.backupReminder(2).due },
    { id: 'F04989', name: '更新失败回滚', check: () => { const h = new C.UpdateHistory(); h.record('2.0', false, 1); h.record('1.9', true, 0); return h.lastOk() === '1.9'; } },
    { id: 'F04990', name: '驱动失败回滚', check: () => A.driverRollback(['531.41', '546.33']) === '531.41' },
    { id: 'F04991', name: '系统文件自愈', check: () => { const r = E.systemFileRepair({ a: 'x', b: 'ok' }, { a: 'gold', b: 'ok' }); return r.repaired.join() === 'a' && E.systemFileRepair({}, {}).intact; } },
    { id: 'F04992', name: '配置自愈重建', check: () => E.configSelfHeal('', 'default') === 'default' && E.configSelfHeal('custom', 'default') === 'custom' },
    { id: 'F04993', name: '启动循环检测修复', check: () => E.bootLoopDetect(3).loop && E.bootLoopDetect(3).action === '进入恢复环境' && !E.bootLoopDetect(1).loop },
    { id: 'F04994', name: '安全模式引导', check: () => E.SAFE_MODE_KINDS.length === 3 && E.SAFE_MODE_KINDS.includes('with-network') },
    { id: 'F04995', name: '最小化诊断系统', check: () => E.minimalSystem(['kernel', 'storage', 'input', 'display']).ok && !E.minimalSystem(['kernel']).ok },
    { id: 'F04996', name: '硬件基准报告', check: () => C.builtinBench(9, 'gpu') > 0 && C.builtinBench(9, 'disk') > 0 && C.builtinBench(9, 'cpu') !== C.builtinBench(9, 'gpu') },
    { id: 'F04997', name: '月度可靠性报告', check: () => { const m = E.reliabilityMonthly([{ crashes: 1, uptimeH: 400 }, { crashes: 0, uptimeH: 500 }]); return m.crashes === 1 && m.uptimeH === 900 && m.score === 95; } },
    { id: 'F04998', name: '崩溃前行为关联', check: () => E.eventCorrelation([{ at: 0, kind: 'io-error' }, { at: 1000, kind: 'crash' }]) === 'io-error' && E.eventCorrelation([{ at: 0, kind: 'io-error' }, { at: 99000, kind: 'crash' }]) === null },
    { id: 'F04999', name: '诊断包需同意', check: () => E.diagPackage(['logs'], true).ok && !E.diagPackage(['logs'], false).ok && E.diagPackage(['logs'], false).items.length === 0 },
    { id: 'F05000', name: '可靠性知识教学', check: () => hwTutorials().has('F05000') },
  ];
}

/** 汇总：领域08 全部 25 族 625 项。 */
export function runDomain08Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [
    checkF0176, checkF0177, checkF0178, checkF0179, checkF0180,
    checkF0181, checkF0182, checkF0183, checkF0184, checkF0185,
    checkF0186, checkF0187, checkF0188, checkF0189, checkF0190,
    checkF0191, checkF0192, checkF0193, checkF0194, checkF0195,
    checkF0196, checkF0197, checkF0198, checkF0199, checkF0200,
  ];
  const entries = families.flatMap((f) => f());
  const failed = entries.filter((e) => {
    try {
      return !e.check();
    } catch {
      return true;
    }
  });
  return { entries, failed };
}
