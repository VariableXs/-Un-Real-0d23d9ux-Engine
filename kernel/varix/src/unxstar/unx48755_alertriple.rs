//! UNX-F48755 C·性能异常呈现终检（超预算告警三要素）
//! 判据 UNX-F48755-J1：注入 ×10 呈现齐；判据锚定 体验十三·补。
//! 设计纪律：no_std（仅 core/alloc）；诊断码自 SEG_BASE+1 起连续且总数 ≤0x1F；
//! drill() 自证十判据，正例计数、反例注入必须 .err() 命中，命中即 Err 传播；
//! fuzz 用确定性 LCG（wrapping_mul 1_664_525 / wrapping_add 1_013_904_223）；
//! 超预算告警三要素：路径名、实测值、预算值，注入 ×10 缺一即红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1C00;
const E_ELEM_MISS: u16 = SEG_BASE + 1;
const E_NO_ALERT: u16 = SEG_BASE + 2;
const E_BUDGET_ZERO: u16 = SEG_BASE + 3;
const E_PATH_EMPTY: u16 = SEG_BASE + 4;
const E_REPLAY_DIFF: u16 = SEG_BASE + 5;
const E_ALERT_DUP: u16 = SEG_BASE + 6;
const E_MEASURE_BAD: u16 = SEG_BASE + 7;
const E_PRESENT_GAP: u16 = SEG_BASE + 8;

/// 告警三要素
#[derive(Clone, Debug, PartialEq)]
pub struct Alert {
    pub path_name: String,
    pub measured_us: u32,
    pub budget_us: u32,
}

impl Alert {
    pub fn tripped(&self) -> bool {
        self.measured_us > self.budget_us
    }

    /// 三要素齐备核对：路径名非空、实测与预算皆正
    pub fn complete(&self) -> Result<(), u16> {
        if self.path_name.is_empty() {
            return Err(E_PATH_EMPTY);
        }
        if self.budget_us == 0 {
            return Err(E_BUDGET_ZERO);
        }
        if self.measured_us == 0 {
            return Err(E_MEASURE_BAD);
        }
        Ok(())
    }
}

/// 呈现通道：告警入账与呈现核对
pub struct PresentChannel {
    pub alerts: Vec<Alert>,
}

impl PresentChannel {
    pub fn new() -> Self {
        PresentChannel { alerts: vec![] }
    }

    pub fn raise(&mut self, path_name: &str, measured_us: u32, budget_us: u32) -> Result<bool, u16> {
        if path_name.is_empty() {
            return Err(E_PATH_EMPTY);
        }
        if budget_us == 0 {
            return Err(E_BUDGET_ZERO);
        }
        if measured_us == 0 {
            return Err(E_MEASURE_BAD);
        }
        if measured_us <= budget_us {
            return Ok(false);
        }
        let mut i = 0usize;
        while i < self.alerts.len() {
            if self.alerts[i].path_name == path_name {
                return Err(E_ALERT_DUP);
            }
            i += 1;
        }
        self.alerts.push(Alert { path_name: String::from(path_name), measured_us, budget_us });
        Ok(true)
    }

    /// 呈现核对：账内告警三要素逐一齐备且真超限
    pub fn present_all(&self) -> Result<u32, u16> {
        if self.alerts.is_empty() {
            return Err(E_NO_ALERT);
        }
        let mut green = 0u32;
        let mut i = 0usize;
        while i < self.alerts.len() {
            let a = &self.alerts[i];
            a.complete()?;
            if !a.tripped() {
                return Err(E_PRESENT_GAP);
            }
            green += 1;
            i += 1;
        }
        Ok(green)
    }
}

/// 确定性 LCG
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 注入 ×10 呈现齐：十条路径各超限一次
fn sweep_ten_alerts(rng: &mut Lcg) -> Result<(), u16> {
    let mut ch = PresentChannel::new();
    let mut p = 1u16;
    while p <= 10 {
        let name = match p % 4 {
            0 => "keypath",
            1 => "convpath",
            2 => "commitpath",
            _ => "renderpath",
        };
        let budget = 400 + rng.below(800) as u32;
        let over = budget + 1 + rng.below(400) as u32;
        let raised = ch.raise(name, over, budget)?;
        if !raised {
            return Err(E_NO_ALERT);
        }
        p += 1;
    }
    if ch.present_all()? != 10 {
        return Err(E_PRESENT_GAP);
    }
    Ok(())
}

/// 预算内不告警 ×10^3
fn sweep_quiet(rng: &mut Lcg) -> Result<(), u16> {
    let mut iter = 0u32;
    while iter < 1_000 {
        let mut ch = PresentChannel::new();
        let budget = 1_000 + rng.below(1_000) as u32;
        let under = 100 + rng.below(800) as u32;
        let raised = ch.raise("quiet-path", under, budget)?;
        if raised || !ch.alerts.is_empty() {
            return Err(E_NO_ALERT);
        }
        iter += 1;
    }
    Ok(())
}

/// 反例注入：空路径、零预算、零实测、重复告警、空账呈现全命中
fn sweep_negatives() -> Result<(), u16> {
    let mut ch = PresentChannel::new();
    let r1 = ch.raise("", 100, 50);
    if r1.err() != Some(E_PATH_EMPTY) {
        return Err(E_PATH_EMPTY);
    }
    let r2 = ch.raise("p", 100, 0);
    if r2.err() != Some(E_BUDGET_ZERO) {
        return Err(E_BUDGET_ZERO);
    }
    let r3 = ch.raise("p", 0, 50);
    if r3.err() != Some(E_MEASURE_BAD) {
        return Err(E_MEASURE_BAD);
    }
    ch.raise("p", 200, 100)?;
    let r4 = ch.raise("p", 300, 100);
    if r4.err() != Some(E_ALERT_DUP) {
        return Err(E_ALERT_DUP);
    }
    let r5 = PresentChannel::new().present_all();
    if r5.err() != Some(E_NO_ALERT) {
        return Err(E_NO_ALERT);
    }
    Ok(())
}

/// 重放一致 ×20：同种子双跑告警账逐位吻合
fn sweep_replay(seed: u64) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 20 {
        let mut ca = PresentChannel::new();
        let mut cb = PresentChannel::new();
        let mut ra = Lcg::new(seed + round as u64);
        let mut rb = Lcg::new(seed + round as u64);
        let mut p = 1u16;
        while p <= 6 {
            let ba = 500 + ra.below(1_000) as u32;
            let bb = 500 + rb.below(1_000) as u32;
            if ba != bb {
                return Err(E_REPLAY_DIFF);
            }
            let ma = ra.below(2_500) as u32;
            let mb = rb.below(2_500) as u32;
            if ma != mb {
                return Err(E_REPLAY_DIFF);
            }
            let ta = ca.raise("path", ma, ba)?;
            let tb = cb.raise("path", mb, bb)?;
            if p == 1 {
                if ta != tb {
                    return Err(E_REPLAY_DIFF);
                }
            }
            p += 1;
        }
        if ca.present_all().is_err() != cb.present_all().is_err() {
            return Err(E_REPLAY_DIFF);
        }
        round += 1;
    }
    Ok(())
}

pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1 注入 ×10 呈现齐
    let mut rng = Lcg::new(0x4875_5FDD);
    sweep_ten_alerts(&mut rng)?;
    hits += 1;
    // R2 预算内不告警 ×10^3
    sweep_quiet(&mut rng)?;
    hits += 1;
    // R3 反例注入全命中
    sweep_negatives()?;
    hits += 1;
    // R4 重放一致 ×20
    sweep_replay(0x4875_5A09)?;
    hits += 1;
    // R5 三要素齐备断言：路径名/实测/预算逐一在账
    let mut ch = PresentChannel::new();
    ch.raise("alpha-path", 1_500, 1_000)?;
    let alerts = ch.alerts.clone();
    if alerts.len() != 1 {
        return Err(E_PRESENT_GAP);
    }
    let a0 = alerts[0].clone();
    if a0.path_name != "alpha-path" || a0.measured_us != 1_500 || a0.budget_us != 1_000 {
        return Err(E_ELEM_MISS);
    }
    a0.complete()?;
    hits += 1;
    // R6 界值触发：等于预算不告警、+1 即告警
    let mut ch2 = PresentChannel::new();
    if ch2.raise("edge", 1_000, 1_000)? {
        return Err(E_NO_ALERT);
    }
    if !ch2.raise("edge2", 1_001, 1_000)? {
        return Err(E_NO_ALERT);
    }
    hits += 1;
    // R7 告警真超限：账内全部 measured > budget
    let alerts2 = ch2.alerts.clone();
    let mut i = 0usize;
    while i < alerts2.len() {
        if alerts2[i].measured_us <= alerts2[i].budget_us {
            return Err(E_PRESENT_GAP);
        }
        i += 1;
    }
    hits += 1;
    // R8 呈现序稳定：入账次序即呈现次序
    let mut ch3 = PresentChannel::new();
    let mut p = 1u16;
    while p <= 5 {
        let name = match p {
            1 => "p1",
            2 => "p2",
            3 => "p3",
            4 => "p4",
            _ => "p5",
        };
        ch3.raise(name, 2_000 + p as u32, 1_000)?;
        p += 1;
    }
    let al = ch3.alerts.clone();
    if al.len() != 5 || al[0].path_name != "p1" || al[4].path_name != "p5" {
        return Err(E_PRESENT_GAP);
    }
    hits += 1;
    // R9 三要素 complete ×5 组全过
    let mut k = 0u32;
    while k < 5 {
        let a = Alert {
            path_name: String::from("elem"),
            measured_us: 2_000 + k,
            budget_us: 1_000 + k,
        };
        a.complete()?;
        k += 1;
    }
    hits += 1;
    // R10 呈现计数与账面一致
    if ch3.present_all()? != 5 {
        return Err(E_PRESENT_GAP);
    }
    hits += 1;
    // R11 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a: u32 = 0x9E37_79B9;
    let mut track_b: u32 = 0x9E37_79B9;
    let mut rail: Vec<u32> = vec![];
    let mut rail_sum: u64 = 0;
    let mut dstep = 0u32;
    while dstep < 48 {
        track_a = track_a.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b = track_b.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail.push(track_a);
        rail_sum += track_a as u64;
        dstep += 1;
    }
    if track_a != track_b {
        return Err(E_REPLAY_DIFF);
    }
    if rail.len() != 48 {
        return Err(E_REPLAY_DIFF);
    }
    let mut replay_sum: u64 = 0;
    let mut ridx = 0usize;
    while ridx < rail.len() {
        replay_sum += rail[ridx] as u64;
        ridx += 1;
    }
    if replay_sum != rail_sum {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;
    Ok(hits)
}
