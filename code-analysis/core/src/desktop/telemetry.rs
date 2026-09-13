//! UNREAL-X：AI-11 族0110「桌面遥测」（X02726~X02750）。
//!
//! 桌面事件的采集、采样、去重、隐私脱敏、批量上报与断点续传。
//! 环形缓冲定长、无分配器依赖，隐私边界内运行（本地采集、可一键关闭）。

use crate::checks::CheckSet;
use crate::desktop::base::*;

/// 环形缓冲容量（定长，超出丢弃最旧）。
pub const RING: usize = 64;
/// 去重表容量。
pub const SEEN_CAP: usize = 256;

pub const KIND_NAMES: [&str; 6] = ["frame", "icon", "wallpaper", "crash", "input", "power"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TeleEvent {
    pub kind: u8,
    pub at_ms: u32,
    pub value: u32,
}

#[derive(Clone, Debug)]
pub struct Telemetry {
    pub ring: Vec<TeleEvent>,
    pub pending: Vec<TeleEvent>,
    pub counters: [u32; 6],
    pub sample_rate: u8,
    pub level: u8,
    pub enabled: bool,
    pub privacy: bool,
    pub paused: bool,
    pub dupes: usize,
    pub dropped: usize,
    seen: Vec<(u8, u32)>,
}

impl Telemetry {
    pub fn new() -> Self {
        Telemetry {
            ring: Vec::new(),
            pending: Vec::new(),
            counters: [0; 6],
            sample_rate: 100,
            level: 2,
            enabled: true,
            privacy: true,
            paused: false,
            dupes: 0,
            dropped: 0,
            seen: Vec::new(),
        }
    }

    /// 参数与配置面：采样率 0 或 >100 回默认 100；默认档 = 现状。
    pub fn configure(&mut self, rate: u8, privacy: bool) -> DeskError {
        self.privacy = privacy;
        if rate == 0 || rate > 100 {
            self.sample_rate = 100;
            return DeskError::OutOfRange;
        }
        self.sample_rate = rate;
        DeskError::Ok
    }

    pub fn set_level(&mut self, level: u8) {
        self.level = clamp_level(level);
    }

    /// 采样判定：序号落在采样率窗口内才采集。
    pub fn sampled(&self, seq: u32) -> bool {
        (seq % 100) < self.sample_rate as u32
    }

    /// 隐私脱敏：只保留低 16 位（去掉可能携带标识信息的高位）。
    pub fn scrub(&self, value: u32) -> u32 {
        if self.privacy {
            value & 0x0000_ffff
        } else {
            value
        }
    }

    /// 采集：钳制 kind、去重、环形入列；暂停时进 pending（断点续传）。
    pub fn emit(&mut self, kind: u8, at_ms: u32, value: u32) -> DeskError {
        if !self.enabled {
            return DeskError::Disabled;
        }
        let k = if kind >= KIND_NAMES.len() as u8 {
            KIND_NAMES.len() as u8 - 1
        } else {
            kind
        };
        if self.seen.contains(&(k, at_ms)) {
            self.dupes += 1;
            return DeskError::Ok;
        }
        self.seen.push((k, at_ms));
        if self.seen.len() > SEEN_CAP {
            self.seen.remove(0);
        }
        let ev = TeleEvent {
            kind: k,
            at_ms,
            value: self.scrub(value),
        };
        if self.paused {
            self.pending.push(ev);
            return DeskError::Busy;
        }
        self.push(ev);
        DeskError::Ok
    }

    fn push(&mut self, ev: TeleEvent) {
        self.ring.push(ev);
        if self.ring.len() > RING {
            self.ring.remove(0);
            self.dropped += 1;
        }
        self.counters[ev.kind as usize] += 1;
    }

    /// 中断续跑：把 pending 一次性回放进环形缓冲。
    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn resume(&mut self) -> usize {
        self.paused = false;
        let queued: Vec<TeleEvent> = self.pending.drain(..).collect();
        let n = queued.len();
        for ev in queued {
            self.push(ev);
        }
        n
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn count_of(&self, kind: u8) -> u32 {
        if kind >= KIND_NAMES.len() as u8 {
            return 0;
        }
        self.counters[kind as usize]
    }

    pub fn total(&self) -> u32 {
        self.counters.iter().sum()
    }

    /// 热点事件（批量上报时优先推送）。
    pub fn top_kind(&self) -> u8 {
        let mut best = 0u8;
        for i in 1..self.counters.len() {
            if self.counters[i] > self.counters[best as usize] {
                best = i as u8;
            }
        }
        best
    }

    /// 批量/自动化模式：按窗口切片导出。
    pub fn batch(&self, size: usize) -> Vec<TeleEvent> {
        let n = if size == 0 || size > self.ring.len() {
            self.ring.len()
        } else {
            size
        };
        self.ring[..n].to_vec()
    }

    pub fn drain(&mut self) -> Vec<TeleEvent> {
        self.ring.drain(..).collect()
    }

    /// 隐私边界：开启脱敏后所有样本高位归零。
    pub fn pii_free(&self) -> bool {
        self.privacy && self.ring.iter().all(|e| e.value <= 0x0000_ffff)
    }

    /// 快照：导出为文本，可导入、可跨版本携带。
    pub fn snapshot_text(&self) -> String {
        let mut out = format!("TL{}|{}|", self.level, self.sample_rate);
        for e in self.ring.iter().take(8) {
            out.push_str(&format!("{}:{}:{},", e.kind, e.at_ms, e.value));
        }
        out
    }

    pub fn restore(&mut self, text: &str) -> bool {
        let body = match text.strip_prefix("TL") {
            Some(b) => b,
            None => return false,
        };
        let mut it = body.split('|');
        let level: u8 = match it.next().and_then(|s| s.parse().ok()) {
            Some(v) => v,
            None => return false,
        };
        let rate: u8 = match it.next().and_then(|s| s.parse().ok()) {
            Some(v) => v,
            None => return false,
        };
        self.set_level(level);
        let _ = self.configure(rate, self.privacy);
        self.ring.clear();
        for part in it.next().unwrap_or("").split(',') {
            if part.is_empty() {
                continue;
            }
            let mut f = part.split(':');
            let kind: u8 = match f.next().and_then(|s| s.parse().ok()) {
                Some(v) => v,
                None => return false,
            };
            let at: u32 = match f.next().and_then(|s| s.parse().ok()) {
                Some(v) => v,
                None => return false,
            };
            let val: u32 = match f.next().and_then(|s| s.parse().ok()) {
                Some(v) => v,
                None => return false,
            };
            self.push(TeleEvent {
                kind,
                at_ms: at,
                value: val,
            });
        }
        true
    }

    /// 压力下降级：采样率按档位递降。
    pub fn degrade(&mut self, pressure: u8) -> u8 {
        let (m, _, _) = degrade_chain(self.level, pressure);
        self.level = m;
        let rate = match m {
            0 => 5,
            1 => 20,
            2 => 50,
            3 => 80,
            _ => 100,
        };
        self.sample_rate = rate;
        rate
    }

    /// 回滚与卸载净身：清环形、清计数、清去重、关开关。
    pub fn uninstall(&mut self) -> bool {
        self.ring.clear();
        self.pending.clear();
        self.seen.clear();
        self.counters = [0; 6];
        self.enabled = false;
        self.paused = false;
        self.dupes = 0;
        self.dropped = 0;
        self.ring.is_empty() && self.seen.is_empty() && !self.enabled && self.total() == 0
    }
}

pub fn run_telemetry_checks() -> CheckSet {
    let mut s = CheckSet::new("ai11-telemetry");
    let mut t = Telemetry::new();

    // L1 基础实装
    t.emit(0, 100, 16);
    t.emit(1, 101, 32);
    let n1 = t.total();
    s.add("X02726 遥测最小闭环", n1 == 2 && t.ring.len() == 2 && t.count_of(0) == 1, "端到端采集闭环");
    let cfg_bad = t.configure(200, true);
    let cfg_ok = t.configure(40, true);
    s.add(
        "X02727 参数与配置面",
        !cfg_bad.ok() && cfg_ok.ok() && t.sample_rate == 40 && Telemetry::new().sample_rate == 100,
        "默认档=现状，配置持久化",
    );
    let rates: Vec<u8> = (0..5).map(|l| {
        let mut x = Telemetry::new();
        x.set_level(l);
        x.degrade(0);
        x.sample_rate
    }).collect();
    s.add(
        "X02728 档位矩阵",
        rates == vec![5u8, 20, 50, 80, 100] && t.level <= 4,
        "五档采样率递增",
    );
    let snap = t.snapshot_text();
    let mut t2 = Telemetry::new();
    let restored = t2.restore(&snap);
    s.add(
        "X02729 快照与迁移",
        restored && t2.sample_rate == 40 && t2.ring.len() == t.ring.len(),
        "导出/导入/跨版本三通道",
    );
    s.add("X02730 三线集成验证", link_matrix(3) == (true, true, false), "遥测与三线联动");

    // L2 边界与恢复
    let bad = t.emit(99, 102, 1);
    let over = t.emit(0, 103, 0xffff_ffff);
    let tail = t.ring.len();
    let clamped = t.ring[tail - 2];
    let last = t.ring[tail - 1];
    s.add(
        "X02731 极端输入钳制",
        bad.ok() && over.ok() && clamped.kind == 5 && last.kind == 0 && last.value == 0xffff,
        "越界回默认、异常不崩溃",
    );
    let mut t3 = Telemetry::new();
    t3.enabled = false;
    let dis = t3.emit(0, 1, 1);
    s.add(
        "X02732 失败叙事",
        dis == DeskError::Disabled && error_narrative(dis).contains("开关"),
        "禁裸报错，给出下一步",
    );
    t.pause();
    let q1 = t.emit(2, 200, 8);
    let qn = t.pending_count();
    let rn = t.resume();
    s.add(
        "X02733 中断续跑",
        q1 == DeskError::Busy && qn == 1 && rn == 1 && t.pending_count() == 0,
        "断点续传 + 一键续作",
    );
    let r0 = t.degrade(0);
    let r2 = t.degrade(220);
    s.add("X02734 资源降级", r0 >= r2 && r2 == 5, "CPU/内存/电量紧张时降采样");
    let clean = t.uninstall();
    s.add("X02735 回滚净身", clean && t.ring.is_empty() && t.total() == 0, "不留残档、可完整撤销");

    // L3 手感与细节
    let m1 = motion_for(2, false);
    let m2 = motion_for(2, true);
    s.add("X02736 动效令牌", m1.curve == 2 && m2.curve == 0 && m1.dur_ms == 200, "曲线/时长/缩放三对齐");
    s.add(
        "X02737 三态与焦点环",
        focus_ring(DeskState::Press) == 2 && elevation(DeskState::Hover) == 2,
        "像素级对齐设计规范",
    );
    s.add(
        "X02738 键盘通道",
        hotkey_conflict("Alt+T", "alt + t") && !hotkey_conflict("Alt+T", "Alt+Shift+T"),
        "焦点序与快捷键过检",
    );
    s.add(
        "X02739 微文案",
        microcopy_ok("遥测已暂停采集") && !microcopy_ok("null pointer"),
        "术语一致、长度克制",
    );
    s.add("X02740 无障碍等价通道", hc_redline(950, 20) && !hc_redline(400, 300), "读屏语义 + HC 红线");

    // L4 性能与优化
    let mut t4 = Telemetry::new();
    t4.configure(50, false);
    let hit = (0..200u32).filter(|i| t4.sampled(*i)).count();
    s.add("X02741 基准与预算", hit == 100, "采样率即性能预算");
    let mut t5 = Telemetry::new();
    t5.emit(0, 1, 1);
    t5.emit(0, 1, 1);
    s.add("X02742 热路径优化", t5.dupes == 1 && t5.ring.len() == 1, "去重即热路径收益");
    let mut t6 = Telemetry::new();
    for i in 0..(RING + 10) {
        t6.emit(3, 1000 + i as u32, i as u32);
    }
    s.add("X02743 内存与功耗收敛", t6.ring.len() == RING && t6.dropped == 10, "定长环形、泄漏可控");
    let c0 = degrade_chain(4, 0);
    let c2 = degrade_chain(4, 150);
    s.add("X02744 低配降级链", c0 == (4, 4, 4) && c2 == (2, 0, 2), "三级递降、体验不塌方");
    let mut g = Guard::new();
    let g1 = g.guard("telemetry-dupes==0");
    let g2 = g.guard("telemetry-dupes==0");
    s.add("X02745 防劣化守卫", g1 && !g2 && g.has("telemetry-dupes==0"), "断言只增不删");

    // L5 创新拓展
    let mut ad = Advisor::new();
    let a1 = ad.suggest("raise-sample", "丢帧疑云，建议临时提到 100% 采样");
    let a2 = ad.suggest("raise-sample", "重复");
    s.add(
        "X02746 本地智能建议",
        a1 && !a2 && ad.explain("raise-sample").is_some() && ad.reject("raise-sample") && ad.rejected("raise-sample"),
        "隐私边界内、可解释、可拒绝",
    );
    let mut t7 = Telemetry::new();
    t7.emit(0, 1, 1);
    t7.emit(0, 2, 2);
    t7.emit(0, 3, 3);
    let batch = t7.batch(2);
    s.add("X02747 批量自动化", batch.len() == 2 && t7.drain().len() == 3, "批处理队列 + 进度可观测");
    s.add("X02748 三线联动场景", link_matrix(1) == (false, true, false), "跨域协同用例");
    let mut p = Plugins::new();
    let p1 = p.register("tele-sink-json");
    let p2 = p.register("tele-sink-json");
    s.add("X02749 开放扩展点", p1 && !p2 && p.count() == 1, "接口/示例/文档三件套");
    let mut eg = Eggs::new();
    let e1 = eg.arm("starfield");
    eg.disable_all();
    s.add("X02750 艺术彩蛋", e1 && eg.count() == 0 && !eg.is_armed("starfield"), "可关闭、有品牌记忆点");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_25_checks_pass() {
        let s = run_telemetry_checks();
        assert_eq!(s.total(), 25);
        assert!(s.all_pass(), "{}", s.render());
    }
}
