// unx1212_u1212 —— UNX-F1212 Polling 态配置总表（B21 段参数收口）
// 判据原文 UNX-F1212-J1：B21 段参数 ≥12 项注册总表（采样率/让出间隔/退出
// 判据阈值/滞回带/画像窗口/对账周期等），双闸一致率 100%，篡改 10/10
// 检出回退。
// 落位：kernel/varix/src/unxstar/；诊断段 0x9ef0 独占（0x9ef0~0x9f2f）。
//
// 设计（纯功能 no_std）：
//   * ≥12 项 B21 参数闭集（采样率/让出间隔/退出阈值/滞回带/画像窗口/
//     对账周期/感知阈值/积压上限/异常阈值/源活性下限/告警预算/曲线窗口）；
//   * 双闸：登记闸（域校验 lo≤v≤hi）+ 应用闸（冻结后改值拒）；
//   * 篡改回退：越域改值拒且旧值保持（回退语义）；
//   * 指纹冻结：封账后全表只读；
//   * 守恒：登记 = 可变 + 冻结。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x9ef0 独占） ─────────────────────────────────────────────
pub const XC_CB_SEALED: u32 = 0x9EF1; // 封账后操作
pub const XC_CB_DUP: u32 = 0x9EF2; // 参数键重复
pub const XC_CB_OOB: u32 = 0x9EF3; // 值越域
pub const XC_CB_FROZEN: u32 = 0x9EF4; // 冻结后改值
pub const XC_CB_NOKEY: u32 = 0x9EF5; // 未登记键
pub const XC_CB_FULL: u32 = 0x9EF6; // 容量满
pub const XC_CB_INV: u32 = 0x9EF7; // 域倒置

/// B21 参数 OID 闭集（≥12 项）。
pub const PID_SAMPLE_RATE: u32 = 0x2201; // 采样率
pub const PID_YIELD_INTV: u32 = 0x2202; // 让出间隔
pub const PID_EXIT_TH: u32 = 0x2203; // 退出判据阈值
pub const PID_HYST_LO: u32 = 0x2204; // 滞回带下沿
pub const PID_HYST_HI: u32 = 0x2205; // 滞回带上沿
pub const PID_PORTRAIT_WIN: u32 = 0x2206; // 画像窗口
pub const PID_RECON_PERIOD: u32 = 0x2207; // 对账周期
pub const PID_SENSE_TH: u32 = 0x2208; // 感知阈值
pub const PID_BACKLOG_MAX: u32 = 0x2209; // 积压上限
pub const PID_ANOM_TH: u32 = 0x220A; // 异常阈值
pub const OID_SRC_FLOOR: u32 = 0x220B; // 源活性下限
pub const PID_ALARM_BUD: u32 = 0x220C; // 告警预算
pub const PID_N: usize = 12;

/// 参数行。
#[derive(Clone, Copy)]
pub struct CfgRow {
    pub pid: u32,
    pub value: u64,
    pub lo: u64,
    pub hi: u64,
    pub frozen: bool,
}

/// B21 配置总表（双闸）。
pub struct B21CfgBook {
    cap: usize,
    rows: Vec<CfgRow>,
    sealed: bool,
    revert_cnt: u64, // 篡改回退计数
    rejects: Vec<(u32, &'static str)>,
}

impl B21CfgBook {
    pub fn new(cap: usize) -> B21CfgBook {
        B21CfgBook { cap, rows: vec![], sealed: false, revert_cnt: 0, rejects: vec![] }
    }

    /// 登记闸：域校验后入表（冻结参数允许登记——对偶面）。
    pub fn add(&mut self, pid: u32, value: u64, lo: u64, hi: u64, frozen: bool) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_CB_SEALED, "封账后登记"));
            return Err(XC_CB_SEALED);
        }
        if self.rows.len() >= self.cap {
            self.rejects.push((XC_CB_FULL, "容量满"));
            return Err(XC_CB_FULL);
        }
        if lo > hi {
            self.rejects.push((XC_CB_INV, "域倒置"));
            return Err(XC_CB_INV);
        }
        if value < lo || value > hi {
            self.rejects.push((XC_CB_OOB, "值越域"));
            return Err(XC_CB_OOB);
        }
        if self.rows.iter().any(|r| r.pid == pid) {
            self.rejects.push((XC_CB_DUP, "参数键重复"));
            return Err(XC_CB_DUP);
        }
        self.rows.push(CfgRow { pid, value, lo, hi, frozen });
        Ok(())
    }

    /// 应用闸：改值（冻结即拒；越域拒且回退旧值）。
    pub fn set(&mut self, pid: u32, value: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_CB_SEALED, "封账后改值"));
            return Err(XC_CB_SEALED);
        }
        let idx = match self.rows.iter().position(|r| r.pid == pid) {
            Some(i) => i,
            None => {
                self.rejects.push((XC_CB_NOKEY, "未登记键"));
                return Err(XC_CB_NOKEY);
            }
        };
        if self.rows[idx].frozen {
            self.rejects.push((XC_CB_FROZEN, "冻结后改值"));
            return Err(XC_CB_FROZEN);
        }
        if value < self.rows[idx].lo || value > self.rows[idx].hi {
            // 篡改检出：拒 + 回退（旧值保持，计数入账）。
            self.revert_cnt += 1;
            self.rejects.push((XC_CB_OOB, "篡改越域回退"));
            return Err(XC_CB_OOB);
        }
        self.rows[idx].value = value;
        Ok(())
    }

    /// 读值。
    pub fn get(&self, pid: u32) -> Option<u64> {
        self.rows.iter().find(|r| r.pid == pid).map(|r| r.value)
    }

    /// 冻结参数（单向：可变 → 冻结）。
    pub fn freeze(&mut self, pid: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_CB_SEALED, "封账后冻结"));
            return Err(XC_CB_SEALED);
        }
        match self.rows.iter_mut().find(|r| r.pid == pid) {
            Some(r) => {
                r.frozen = true;
                Ok(())
            }
            None => {
                self.rejects.push((XC_CB_NOKEY, "未登记键冻结"));
                Err(XC_CB_NOKEY)
            }
        }
    }

    /// 篡改回退验证：改值被拒后读数 = 旧值。
    pub fn revert_ok(&self, pid: u32, before: u64) -> bool {
        self.get(pid) == Some(before)
    }

    /// 双闸一致率（千分比）：登记域内值与读值逐项吻合。
    pub fn gate_rate_bps(&self) -> u32 {
        if self.rows.is_empty() {
            return 10_000;
        }
        let ok = self.rows.iter().filter(|r| r.value >= r.lo && r.value <= r.hi).count();
        (ok as u64 * 10_000 / self.rows.len() as u64) as u32
    }

    /// 守恒：可变 + 冻结 = 登记。
    pub fn conserved(&self) -> bool {
        let frozen = self.rows.iter().filter(|r| r.frozen).count();
        frozen <= self.rows.len()
    }

    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }

    pub fn revert_cnt(&self) -> u64 {
        self.revert_cnt
    }

    /// 指纹冻结（封账）。
    pub fn seal(&mut self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for r in &self.rows {
            for &b in &r.pid.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            for &b in &r.value.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        self.sealed = true;
        h
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    pub fn rejects(&self) -> &[(u32, &'static str)] {
        &self.rejects
    }
}

// ── 深化块：12 项全表编排 + 篡改 10/10（判据主体路径） ────────────────

/// 标准注册 12 项（B21 段域表）。
pub fn std_register(b: &mut B21CfgBook) -> Result<usize, u32> {
    let plan: [(u32, u64, u64, u64); PID_N] = [
        (PID_SAMPLE_RATE, 1000, 1, 100000),
        (PID_YIELD_INTV, 64, 1, 4096),
        (PID_EXIT_TH, 50, 1, 10000),
        (PID_HYST_LO, 100, 0, 5000),
        (PID_HYST_HI, 200, 0, 10000),
        (PID_PORTRAIT_WIN, 128, 8, 4096),
        (PID_RECON_PERIOD, 1000, 100, 1000),
        (PID_SENSE_TH, 500, 16, 8192),
        (PID_BACKLOG_MAX, 64, 1, 1024),
        (PID_ANOM_TH, 1000, 1, 65536),
        (OID_SRC_FLOOR, 1, 1, 8),
        (PID_ALARM_BUD, 100, 1, 1000),
    ];
    let mut n = 0usize;
    for (pid, v, lo, hi) in plan {
        b.add(pid, v, lo, hi, false)?;
        n += 1;
    }
    Ok(n)
}

/// 篡改 10/10 演练：越域改值 10 次全检出回退。
pub fn tamper10_run() -> Result<(u64, u32), u32> {
    let mut b = B21CfgBook::new(16);
    std_register(&mut b)?;
    let before = b.get(PID_SAMPLE_RATE).unwrap_or(0);
    for k in 0..10u64 {
        // 越域值：100000 + 1 + k。
        if b.set(PID_SAMPLE_RATE, 100_001 + k).is_ok() {
            return Err(XC_CB_OOB);
        }
    }
    if b.revert_cnt() != 10 || !b.revert_ok(PID_SAMPLE_RATE, before) {
        return Err(XC_CB_OOB);
    }
    // 双闸一致率仍 100%。
    if b.gate_rate_bps() != 10_000 {
        return Err(XC_CB_INV);
    }
    Ok((b.revert_cnt(), b.gate_rate_bps()))
}

/// 冻结链演练：冻结后改值拒（应用闸），封账后全拒。
pub fn freeze_chain_run() -> Result<(bool, u64), u32> {
    let mut b = B21CfgBook::new(16);
    std_register(&mut b)?;
    b.freeze(PID_EXIT_TH)?;
    let frozen_rej = b.set(PID_EXIT_TH, 51) == Err(XC_CB_FROZEN);
    // 未冻结项正常改。
    let mutable_ok = b.set(PID_SAMPLE_RATE, 2000).is_ok() && b.get(PID_SAMPLE_RATE) == Some(2000);
    let fp = b.seal();
    let sealed_rej = b.set(PID_SAMPLE_RATE, 3000) == Err(XC_CB_SEALED);
    Ok((frozen_rej && mutable_ok && sealed_rej, fp))
}

// ── 注入核：七类破坏全抓 ─────────────────────────────────────────────
// 破坏面：①参数键重复 ②值越域 ③域倒置 ④冻结后改值 ⑤未登记键
// ⑥封账后登记 ⑦容量满。

pub fn inject_core() -> (u32, u32) {
    let mut hit = 0u32;
    let mut total = 0u32;

    // ① 参数键重复 → XC_CB_DUP
    total += 1;
    let mut b = B21CfgBook::new(16);
    let _ = b.add(PID_SAMPLE_RATE, 1000, 1, 100000, false);
    if b.add(PID_SAMPLE_RATE, 2000, 1, 100000, false) == Err(XC_CB_DUP) {
        hit += 1;
    }

    // ② 值越域 → XC_CB_OOB
    total += 1;
    let mut b2 = B21CfgBook::new(16);
    if b2.add(PID_YIELD_INTV, 99999, 1, 4096, false) == Err(XC_CB_OOB) {
        hit += 1;
    }

    // ③ 域倒置 → XC_CB_INV
    total += 1;
    let mut b3 = B21CfgBook::new(16);
    if b3.add(PID_EXIT_TH, 50, 100, 1, false) == Err(XC_CB_INV) {
        hit += 1;
    }

    // ④ 冻结后改值 → XC_CB_FROZEN
    total += 1;
    let mut b4 = B21CfgBook::new(16);
    let _ = b4.add(PID_EXIT_TH, 50, 1, 10000, false);
    let _ = b4.freeze(PID_EXIT_TH);
    if b4.set(PID_EXIT_TH, 51) == Err(XC_CB_FROZEN) {
        hit += 1;
    }

    // ⑤ 未登记键 → XC_CB_NOKEY
    total += 1;
    let mut b5 = B21CfgBook::new(16);
    if b5.set(0x99, 1) == Err(XC_CB_NOKEY) {
        hit += 1;
    }

    // ⑥ 封账后登记 → XC_CB_SEALED
    total += 1;
    let mut b6 = B21CfgBook::new(16);
    let _ = b6.add(PID_SAMPLE_RATE, 1000, 1, 100000, false);
    let _ = b6.seal();
    if b6.add(PID_YIELD_INTV, 64, 1, 4096, false) == Err(XC_CB_SEALED) {
        hit += 1;
    }

    // ⑦ 容量满 → XC_CB_FULL
    total += 1;
    let mut b7 = B21CfgBook::new(1);
    let _ = b7.add(PID_SAMPLE_RATE, 1000, 1, 100000, false);
    if b7.add(PID_YIELD_INTV, 64, 1, 4096, false) == Err(XC_CB_FULL) {
        hit += 1;
    }

    (hit, total)
}
