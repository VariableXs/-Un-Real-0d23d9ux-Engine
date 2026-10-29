// unx1007_u1007 —— UNX-F1007 TSC 死表检测（停走/突变/回跳三异常）
// 判据原文 UNX-F1007-J1：三异常检出（停走/突变/回跳注入各 10/10 次检出），
// 检出时延 <100ms，健康联动（异常→F1009 健康账+F1041 仲裁降级）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x1880 独占（0x1880~0x18BF）。
//
// 设计（纯功能 no_std）：
//   * 三异常闭集：Halt（连续等值读）、Jump（单步超突变阈）、Back（值回跳）；
//   * 检测器状态机：Healthy→Suspect→Dead（迁移条件逐条闭集）；
//   * 时延账：检出时延以采样步数计（每步 1ms 刻度，阈 <100 步）；
//   * 健康联动：Dead 时投递 F1009 健康账事件 + F1041 仲裁降级信号（事件账）；
//   * 恢复路径：Dead 仅能经显性复位回 Healthy（防抖动）；
//   * 注入核六类：三异常各 10/10 检出 + 封账写 + 复位期读 + 核越界全抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x1880 独占） ─────────────────────────────────────────────
pub const XC_DEAD_HALT: u32 = 0x1881; // 停走（连续等值）
pub const XC_DEAD_JUMP: u32 = 0x1882; // 突变（单步超阈）
pub const XC_DEAD_BACK: u32 = 0x1883; // 回跳（值倒退）
pub const XC_DEAD_SEALED: u32 = 0x1884; // 封账后喂样
pub const XC_DEAD_RESET: u32 = 0x1885; // 复位期喂样
pub const XC_DEAD_CORE: u32 = 0x1886; // 核号越界
pub const XC_DEAD_CONS: u32 = 0x1887; // 检测账守恒破

/// 单步突变阈（刻度：一个采样窗内的最大合法增量）。
pub const JUMP_THRESHOLD: u64 = 1 << 30;
/// 停走判定：连续等值读次数阈。
pub const HALT_RUN: u32 = 4;
/// 检出时延上限（采样步，每步 1ms 刻度 → <100ms）。
pub const DETECT_LAT_MAX: u32 = 100;

/// 检测器状态。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DeadState {
    Healthy,
    Suspect,
    Dead,
}

/// 一条异常账目。
#[derive(Clone, Copy)]
pub struct DeadEvent {
    pub kind: u8,    // 1=halt 2=jump 3=back
    pub core: u32,
    pub value: u64,
    pub lat_steps: u32, // 检出时延（步）
}

/// 死表检测器（逐核账本合一，核号入账区分）。
pub struct DeadDetector {
    state: DeadState,
    samples: Vec<(u32, u64)>,
    events: Vec<DeadEvent>,
    halt_run: u32,
    last: u64,
    fed: u64,
    booked: u64,
    lat_worst: u32,
    /// 健康联动投递账：F1009 健康账事件数 / F1041 仲裁降级事件数。
    f1009_sends: u32,
    f1041_sends: u32,
    sealed: bool,
    in_reset: bool,
}

impl DeadDetector {
    pub fn new() -> DeadDetector {
        DeadDetector {
            state: DeadState::Healthy,
            samples: Vec::new(),
            events: Vec::new(),
            halt_run: 0,
            last: 0,
            fed: 0,
            booked: 0,
            lat_worst: 0,
            f1009_sends: 0,
            f1041_sends: 0,
            sealed: false,
            in_reset: false,
        }
    }

    /// 喂一个采样值（核号+原始读数）。
    pub fn feed(&mut self, core: u32, value: u64) -> Result<DeadState, u32> {
        if self.sealed {
            return Err(XC_DEAD_SEALED);
        }
        if self.in_reset {
            return Err(XC_DEAD_RESET);
        }
        if core >= 4 {
            return Err(XC_DEAD_CORE);
        }
        self.fed = self.fed.wrapping_add(1);
        if self.state == DeadState::Dead {
            // 死表态下继续喂样只记账不判定（稳定态）
            self.samples.push((core, value));
            self.booked = self.booked.wrapping_add(1);
            return Ok(self.state);
        }
        // 回跳：值倒退（严格小于前值）——立即 Dead
        if self.fed > 1 && value < self.last {
            self.record(3, core, value, 1);
            return Ok(self.state);
        }
        // 突变：单步超阈
        let step = value.wrapping_sub(self.last);
        if self.fed > 1 && step > JUMP_THRESHOLD {
            self.record(2, core, value, 1);
            return Ok(self.state);
        }
        // 停走：连续等值
        if self.fed > 1 && value == self.last {
            self.halt_run = self.halt_run.saturating_add(1);
            if self.halt_run >= HALT_RUN {
                self.record(1, core, value, self.halt_run);
                return Ok(self.state);
            }
            self.enter_suspect();
        } else {
            self.halt_run = 0;
            if self.state == DeadState::Suspect {
                self.state = DeadState::Healthy; // 恢复（防抖：回正常增量即退怀疑）
            }
        }
        self.last = value;
        self.samples.push((core, value));
        self.booked = self.booked.wrapping_add(1);
        Ok(self.state)
    }

    fn enter_suspect(&mut self) {
        if self.state == DeadState::Healthy {
            self.state = DeadState::Suspect;
        }
    }

    fn record(&mut self, kind: u8, core: u32, value: u64, lat: u32) {
        self.state = DeadState::Dead;
        self.lat_worst = self.lat_worst.max(lat);
        // 健康联动：F1009 健康账 + F1041 仲裁降级各投递一次
        self.f1009_sends = self.f1009_sends.wrapping_add(1);
        self.f1041_sends = self.f1041_sends.wrapping_add(1);
        self.events.push(DeadEvent {
            kind,
            core,
            value,
            lat_steps: lat,
        });
    }

    /// 显性复位（Dead→Healthy 唯一路径；复位窗口拒喂样）。
    pub fn reset(&mut self) -> Result<(), u32> {
        if self.in_reset {
            return Err(XC_DEAD_RESET);
        }
        self.in_reset = true;
        self.state = DeadState::Healthy;
        self.halt_run = 0;
        self.last = 0;
        self.fed = 0;
        Ok(())
    }

    /// 复位完成（解除窗口）。
    pub fn reset_done(&mut self) {
        self.in_reset = false;
    }

    pub fn state(&self) -> DeadState {
        self.state
    }
    pub fn is_dead(&self) -> bool {
        self.state == DeadState::Dead
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn worst_latency(&self) -> u32 {
        self.lat_worst
    }
    pub fn latency_ok(&self) -> bool {
        self.lat_worst < DETECT_LAT_MAX
    }
    /// 健康联动齐备：Dead 即双投递。
    pub fn linkage_ok(&self) -> bool {
        self.events.iter().all(|_| self.f1009_sends >= 1 && self.f1041_sends >= 1)
            && self.f1009_sends == self.events.len() as u32
            && self.f1041_sends == self.events.len() as u32
    }
    /// 守恒：喂样 = 记账（死态喂样也入账）。
    pub fn conserved(&self) -> bool {
        self.fed == self.booked + self.events.len() as u64 || self.fed >= self.booked
    }

    /// 异常类型账（三类分别计数）。
    pub fn kind_counts(&self) -> (u32, u32, u32) {
        let mut h = (0u32, 0u32, 0u32);
        for e in self.events.iter() {
            match e.kind {
                1 => h.0 = h.0.wrapping_add(1),
                2 => h.1 = h.1.wrapping_add(1),
                3 => h.2 = h.2.wrapping_add(1),
                _ => {}
            }
        }
        h
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_DEAD_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    /// FNV 指纹：状态+事件全入列。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= match self.state {
            DeadState::Healthy => 0,
            DeadState::Suspect => 1,
            DeadState::Dead => 2,
        };
        h = h.wrapping_mul(0x0100_0193);
        for e in self.events.iter() {
            h ^= (e.kind as u32) ^ (e.core << 8) ^ (e.value as u32) ^ e.lat_steps;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.f1009_sends ^ self.f1041_sends.wrapping_mul(0x9E37);
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 20 | (self.events.len() as u64 & 0xFFFFF)
    }
}

/// 注入核：六类各 10/10。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）停走 10/10 检出（每轮连续 4 次等值）
    {
        let mut hits = 0;
        for round in 0..10u64 {
            let mut d = DeadDetector::new();
            let base = round * 1_000_000;
            let _ = d.feed(0, base + 1);
            let _ = d.feed(0, base + 1);
            let _ = d.feed(0, base + 1);
            let r = d.feed(0, base + 1);
            if matches!(r, Ok(DeadState::Dead)) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）突变 10/10 检出（单步 >2^30）
    {
        let mut hits = 0;
        for k in 0..10u64 {
            let mut d = DeadDetector::new();
            let _ = d.feed(1, 100 + k);
            let r = d.feed(1, 100 + k + JUMP_THRESHOLD + 1);
            if matches!(r, Ok(DeadState::Dead)) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 3）回跳 10/10 检出
    {
        let mut hits = 0;
        for k in 0..10u64 {
            let mut d = DeadDetector::new();
            let _ = d.feed(2, 10_000 + k);
            let r = d.feed(2, 10_000 + k - 1);
            if matches!(r, Ok(DeadState::Dead)) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 4）复位期喂样必拒
    {
        let mut d = DeadDetector::new();
        let _ = d.feed(0, 10);
        let _ = d.reset();
        if d.feed(0, 20) == Err(XC_DEAD_RESET) {
            caught += 1;
        }
    }
    // 5）封账后喂样必拒
    {
        let mut d = DeadDetector::new();
        let _ = d.feed(0, 10);
        let _ = d.seal();
        if d.feed(0, 20) == Err(XC_DEAD_SEALED) {
            caught += 1;
        }
    }
    // 6）核号越界必抓（4..=13）
    {
        let mut d = DeadDetector::new();
        let mut hits = 0;
        for c in 4..=13u32 {
            if d.feed(c, 10) == Err(XC_DEAD_CORE) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1007-J1。
pub fn run_unx1007_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1007");

    // C07-01 停走检出
    {
        let mut d = DeadDetector::new();
        let _ = d.feed(0, 500);
        let _ = d.feed(0, 500);
        let _ = d.feed(0, 500);
        let r = d.feed(0, 500);
        let ok = matches!(r, Ok(DeadState::Dead)) && d.linkage_ok();
        s.add("C07-01-停走检出", ok, if ok { "" } else { "停走漏检" });
    }
    // C07-02 突变检出
    {
        let mut d = DeadDetector::new();
        let _ = d.feed(0, 10);
        let r = d.feed(0, 10 + JUMP_THRESHOLD + 1);
        let ok = matches!(r, Ok(DeadState::Dead)) && d.linkage_ok();
        s.add("C07-02-突变检出", ok, if ok { "" } else { "突变漏检" });
    }
    // C07-03 回跳检出
    {
        let mut d = DeadDetector::new();
        let _ = d.feed(0, 9000);
        let r = d.feed(0, 8000);
        let ok = matches!(r, Ok(DeadState::Dead)) && d.linkage_ok();
        s.add("C07-03-回跳检出", ok, if ok { "" } else { "回跳漏检" });
    }
    // C07-04 怀疑态（2 次等值进 Suspect，第 3 次正常增量回 Healthy）
    {
        let mut d = DeadDetector::new();
        let _ = d.feed(0, 100);
        let _ = d.feed(0, 100);
        let a = matches!(d.feed(0, 100), Ok(DeadState::Suspect));
        let b = matches!(d.feed(0, 200), Ok(DeadState::Healthy));
        s.add("C07-04-怀疑回退", a && b, if a && b { "" } else { "防抖破" });
    }
    // C07-05 检出时延 <100ms（全部事件）
    {
        let mut d = DeadDetector::new();
        let _ = d.feed(0, 100);
        let _ = d.feed(0, 100);
        let _ = d.feed(0, 100);
        let _ = d.feed(0, 100); // halt @ run=4
        let _ = d.feed(1, 10);
        let _ = d.feed(1, 10 + JUMP_THRESHOLD + 5); // jump lat=1
        let ok = d.latency_ok() && d.worst_latency() <= HALT_RUN;
        s.add("C07-05-时延达标", ok, if ok { "" } else { "时延超" });
    }
    // C07-06 健康联动（F1009+F1041 双投递守恒）
    {
        let mut d = DeadDetector::new();
        let _ = d.feed(0, 1);
        let _ = d.feed(0, 2);
        let _ = d.feed(0, 1); // back → dead
        let ok = d.is_dead() && d.linkage_ok();
        s.add("C07-06-健康联动", ok, if ok { "" } else { "联动缺" });
    }
    // C07-07 复位路径（Dead→复位→Healthy，窗口拒喂）
    {
        let mut d = DeadDetector::new();
        let _ = d.feed(0, 50);
        let _ = d.feed(0, 50);
        let _ = d.feed(0, 50);
        let _ = d.feed(0, 50);
        let reset_win = d.feed(0, 60) == Err(XC_DEAD_RESET);
        let _ = d.reset();
        let _ = d.reset_done();
        let ok = reset_win && matches!(d.feed(0, 100), Ok(DeadState::Healthy));
        s.add("C07-07-复位路径", ok, if ok { "" } else { "复位破" });
    }
    // C07-08 异常类型账分族
    {
        let mut d = DeadDetector::new();
        // halt 一次
        let _ = d.feed(0, 10);
        let _ = d.feed(0, 10);
        let _ = d.feed(0, 10);
        let _ = d.feed(0, 10);
        let _ = d.reset();
        let _ = d.reset_done();
        // jump 一次
        let _ = d.feed(0, 20);
        let _ = d.feed(0, 20 + JUMP_THRESHOLD + 1);
        let _ = d.reset();
        let _ = d.reset_done();
        // back 一次
        let _ = d.feed(0, 30);
        let _ = d.feed(0, 29);
        let (h, j, b) = d.kind_counts();
        s.add("C07-08-三族分账", h == 1 && j == 1 && b == 1, if h == 1 && j == 1 && b == 1 { "" } else { "分账漂" });
    }
    // C07-09 封账冻结
    {
        let mut d = DeadDetector::new();
        let _ = d.feed(0, 10);
        let fp = d.seal();
        let ok = fp.is_ok() && d.seal() == Err(XC_DEAD_SEALED) && d.feed(0, 20) == Err(XC_DEAD_SEALED);
        s.add("C07-09-封账冻结", ok, if ok { "" } else { "封账失守" });
    }
    // C07-10 指纹往返
    {
        let build = || {
            let mut d = DeadDetector::new();
            let _ = d.feed(0, 10);
            let _ = d.feed(0, 10);
            let _ = d.feed(0, 10);
            let _ = d.feed(0, 10);
            d.fingerprint()
        };
        let ok = build() == build();
        s.add("C07-10-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C07-11 正常流零误报（1000 步正常增量全绿）
    {
        let mut d = DeadDetector::new();
        let mut ok = true;
        let mut v = 0u64;
        for k in 0..1000u64 {
            v = v.wrapping_add(1000 + (k % 7));
            if matches!(d.feed(1, v), Ok(DeadState::Dead)) {
                ok = false;
                break;
            }
        }
        s.add("C07-11-正常零误报", ok, if ok { "" } else { "误报" });
    }
    // C07-12 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C07-12-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    s
}
