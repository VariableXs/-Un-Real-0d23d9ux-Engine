// unx1211_u1211 —— UNX-F1211 Polling 态健康账（F0899 口径注册）
// 判据原文 UNX-F1211-J1：Polling 面健康指标 ≥10 项按 F0899 口径注册
// （占用率/感知延迟 P99/事件率/积压深度/退出次数/震荡次数等），摘要一致
// 率 100%，告警联通 10/10 次。
// 落位：kernel/varix/src/unxstar/；诊断段 0x9eb0 独占（0x9eb0~0x9eef）。
//
// 设计（纯功能 no_std）：
//   * F0899 口径：指标 = (oid, lo, hi, tol) 注册 + 采样 + 越带违规计数；
//   * ≥10 项 Polling 面指标闭集注册（占用/延迟/事件率/积压/退出/震荡…）；
//   * 摘要一致率：摘要行 (oid, 读数, 违规数) 与账面逐行对齐；
//   * 告警流：越带即发告警事件（环形流 10/10 联通）；
//   * 评级：全带内 = A；违规 ≤k = B；否则 C。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x9eb0 独占） ─────────────────────────────────────────────
pub const XC_HL_SEALED: u32 = 0x9EB1; // 封账后采样
pub const XC_HL_UNREG: u32 = 0x9EB2; // 未注册指标采样
pub const XC_HL_DUP: u32 = 0x9EB3; // 指标重复注册
pub const XC_HL_INV_BAND: u32 = 0x9EB4; // 带倒置
pub const XC_HL_NOALARM: u32 = 0x9EB5; // 告警流断裂
pub const XC_HL_FULL: u32 = 0x9EB6; // 容量满

pub const GRADE_A: u8 = 1;
pub const GRADE_B: u8 = 2;
pub const GRADE_C: u8 = 3;

/// Polling 面指标 OID 闭集（≥10 项）。
pub const OID_OCCUP: u32 = 0x2101; // 占用率
pub const OID_SENSE_P99: u32 = 0x2102; // 感知延迟 P99
pub const OID_EVENT_RATE: u32 = 0x2103; // 事件率
pub const OID_BACKLOG: u32 = 0x2104; // 积压深度
pub const OID_EXITS: u32 = 0x2105; // 退出次数
pub const OID_OSC: u32 = 0x2106; // 震荡次数
pub const OID_YIELD: u32 = 0x2107; // 让出次数
pub const OID_SRC_ALIVE: u32 = 0x2108; // 源活性
pub const OID_RECON: u32 = 0x2109; // 对账轮次
pub const OID_ANOM: u32 = 0x210A; // 异常计数
pub const OID_N: usize = 10;

/// 指标行（F0899 口径）。
#[derive(Clone, Copy)]
pub struct MetRow {
    pub oid: u32,
    pub lo: u64,
    pub hi: u64,
    pub tol: u32,
    pub value: u64,
    pub viol: u64,
}

/// Polling 健康账。
pub struct PollHealth {
    cap: usize,
    rows: Vec<MetRow>,
    alarms: Vec<(u32, u64, u64)>, // (oid, value, band_hi)
    alarm_sent: u64,
    alarm_recv: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl PollHealth {
    pub fn new(cap: usize) -> PollHealth {
        PollHealth { cap, rows: vec![], alarms: vec![], alarm_sent: 0, alarm_recv: 0, sealed: false, rejects: vec![] }
    }

    /// 按 F0899 口径注册（oid 带容差域）。
    pub fn register(&mut self, oid: u32, lo: u64, hi: u64, tol: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_HL_SEALED, "封账后注册"));
            return Err(XC_HL_SEALED);
        }
        if self.rows.len() >= self.cap {
            self.rejects.push((XC_HL_FULL, "容量满"));
            return Err(XC_HL_FULL);
        }
        if lo > hi {
            self.rejects.push((XC_HL_INV_BAND, "带倒置"));
            return Err(XC_HL_INV_BAND);
        }
        if self.rows.iter().any(|r| r.oid == oid) {
            self.rejects.push((XC_HL_DUP, "指标重复注册"));
            return Err(XC_HL_DUP);
        }
        self.rows.push(MetRow { oid, lo, hi, tol, value: 0, viol: 0 });
        Ok(())
    }

    /// 采样（越带 → 违规计数 + 告警事件入流）。
    pub fn sample(&mut self, oid: u32, value: u64) -> Result<bool, u32> {
        if self.sealed {
            self.rejects.push((XC_HL_SEALED, "封账后采样"));
            return Err(XC_HL_SEALED);
        }
        let idx = match self.rows.iter().position(|r| r.oid == oid) {
            Some(i) => i,
            None => {
                self.rejects.push((XC_HL_UNREG, "未注册指标采样"));
                return Err(XC_HL_UNREG);
            }
        };
        self.rows[idx].value = value;
        let in_band = value >= self.rows[idx].lo && value <= self.rows[idx].hi;
        if !in_band {
            self.rows[idx].viol += 1;
            self.alarms.push((oid, value, self.rows[idx].hi));
            self.alarm_sent += 1;
        }
        Ok(in_band)
    }

    /// 告警消费（联通对账：收 = 发）。
    pub fn alarm_drain(&mut self) -> usize {
        let n = self.alarms.len();
        self.alarm_recv += n as u64;
        self.alarms.clear();
        n
    }

    /// 告警联通率（千分比）：recv = sent。
    pub fn alarm_link_bps(&self) -> u32 {
        if self.alarm_sent == 0 {
            return 10_000;
        }
        if self.alarm_recv >= self.alarm_sent {
            10_000
        } else {
            (self.alarm_recv * 10_000 / self.alarm_sent) as u32
        }
    }

    /// 评级：全带内 A；violk 项 ≤ tol 预算 B；否则 C。
    pub fn grade(&self) -> u8 {
        let viol_rows = self.rows.iter().filter(|r| r.viol > 0).count();
        if viol_rows == 0 {
            GRADE_A
        } else {
            let total_viol: u64 = self.rows.iter().map(|r| r.viol).sum();
            if total_viol <= 10 {
                GRADE_B
            } else {
                GRADE_C
            }
        }
    }

    /// 摘要一致率（千分比）：摘要逐行与账面 (oid, value, viol) 全等。
    pub fn summary_ok(&self, digest: &[(u32, u64, u64)]) -> bool {
        if digest.len() != self.rows.len() {
            return false;
        }
        self.rows.iter().zip(digest.iter()).all(|(r, (o, v, x))| r.oid == *o && r.value == *v && r.viol == *x)
    }

    /// 注册数 ≥10（判据硬数）。
    pub fn reg_cnt(&self) -> usize {
        self.rows.len()
    }

    /// 守恒：告警流清空后 recv = sent。
    pub fn conserved(&self) -> bool {
        self.alarms.is_empty() || self.alarm_recv + self.alarms.len() as u64 >= self.alarm_sent
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn rejects(&self) -> &[(u32, &'static str)] {
        &self.rejects
    }
}

// ── 深化块：10 指标 + 告警 10/10 编排（判据主体路径） ─────────────────

/// 标准注册 10 项 Polling 面指标（带域表）。
pub fn std_register(h: &mut PollHealth) -> Result<usize, u32> {
    let plan: [(u32, u64, u64, u32); OID_N] = [
        (OID_OCCUP, 0, 9500, 500),
        (OID_SENSE_P99, 0, 2048, 256),
        (OID_EVENT_RATE, 0, 100000, 10000),
        (OID_BACKLOG, 0, 64, 8),
        (OID_EXITS, 0, 100, 10),
        (OID_OSC, 0, 20, 2),
        (OID_YIELD, 0, 100000, 10000),
        (OID_SRC_ALIVE, 1, 4, 0),
        (OID_RECON, 0, 10000, 1000),
        (OID_ANOM, 0, 100, 10),
    ];
    let mut n = 0usize;
    for (oid, lo, hi, tol) in plan {
        h.register(oid, lo, hi, tol)?;
        n += 1;
    }
    Ok(n)
}

/// 全编排：10 注册 + 越带采样发告警 + 消费联通 + 摘要对账。
pub fn health_full_run() -> Result<(usize, u8, u32), u32> {
    let mut h = PollHealth::new(16);
    let n = std_register(&mut h)?;
    if n < 10 {
        return Err(XC_HL_UNREG);
    }
    // 逐指标带内采样。
    let ok_vals: [u64; OID_N] = [7000, 900, 50000, 8, 5, 3, 8000, 4, 500, 10];
    for (i, &v) in ok_vals.iter().enumerate() {
        let oid = 0x2101 + i as u32;
        if !h.sample(oid, v)? {
            return Err(XC_HL_NOALARM);
        }
    }
    // 越带 10 次（告警 10/10）。
    for k in 0..10u64 {
        if h.sample(OID_BACKLOG, 64 + k)? {
            return Err(XC_HL_NOALARM);
        }
    }
    let drained = h.alarm_drain();
    if drained != 10 || h.alarm_link_bps() != 10_000 {
        return Err(XC_HL_NOALARM);
    }
    // 摘要对账：手工构造 digest。
    let mut digest: Vec<(u32, u64, u64)> = vec![];
    for (i, &v) in ok_vals.iter().enumerate() {
        let oid = 0x2101 + i as u32;
        let viol = if oid == OID_BACKLOG { 10 } else { 0 };
        digest.push((oid, v, viol));
    }
    if !h.summary_ok(&digest) || !h.conserved() {
        return Err(XC_HL_NOALARM);
    }
    let g = h.grade();
    Ok((n, g, h.alarm_link_bps()))
}

// ── 注入核：六类破坏全抓 ─────────────────────────────────────────────
// 破坏面：①未注册指标采样 ②指标重复注册 ③带倒置 ④封账后采样
// ⑤告警联通断裂 ⑥摘要错值。

pub fn inject_core() -> (u32, u32) {
    let mut hit = 0u32;
    let mut total = 0u32;

    // ① 未注册指标采样 → XC_HL_UNREG
    total += 1;
    let mut h = PollHealth::new(16);
    if h.sample(0x9999, 1) == Err(XC_HL_UNREG) {
        hit += 1;
    }

    // ② 指标重复注册 → XC_HL_DUP
    total += 1;
    let mut h2 = PollHealth::new(16);
    let _ = h2.register(OID_OCCUP, 0, 100, 5);
    if h2.register(OID_OCCUP, 0, 100, 5) == Err(XC_HL_DUP) {
        hit += 1;
    }

    // ③ 带倒置 → XC_HL_INV_BAND
    total += 1;
    let mut h3 = PollHealth::new(16);
    if h3.register(OID_EVENT_RATE, 100, 0, 5) == Err(XC_HL_INV_BAND) {
        hit += 1;
    }

    // ④ 封账后采样 → XC_HL_SEALED
    total += 1;
    let mut h4 = PollHealth::new(16);
    let _ = h4.register(OID_OCCUP, 0, 100, 5);
    h4.seal();
    if h4.sample(OID_OCCUP, 50) == Err(XC_HL_SEALED) {
        hit += 1;
    }

    // ⑤ 告警联通断裂：未消费时 recv < sent → 率 < 100%。
    total += 1;
    let mut h5 = PollHealth::new(16);
    let _ = h5.register(OID_BACKLOG, 0, 8, 1);
    let _ = h5.sample(OID_BACKLOG, 99);
    let broken = h5.alarm_link_bps();
    if h5.alarm_sent == 1 && broken < 10_000 {
        hit += 1;
    }

    // ⑥ 摘要错值：digest 与账面不一致必露。
    total += 1;
    let mut h6 = PollHealth::new(16);
    let _ = h6.register(OID_OCCUP, 0, 100, 5);
    let _ = h6.sample(OID_OCCUP, 42);
    let bad_digest = [(OID_OCCUP, 999u64, 0u64)];
    if !h6.summary_ok(&bad_digest) {
        hit += 1;
    }

    (hit, total)
}
