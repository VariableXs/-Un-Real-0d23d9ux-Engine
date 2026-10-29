// unx1157_u1157 —— UNX-F1157 top half 配置面 B18 段（检测器参数总表）
// 判据原文 UNX-F1157-J1：B18 段可配置项（检测开关/阈值/余量/迁移判定 N/
// 静默参数 ≥10 项）注册总表，默认值一致率 100%，篡改 10/10 次检出。
// 落位：kernel/varix/src/unxstar/；诊断段 0x8670 独占（0x8670~0x86af）。
//
// 设计（纯功能 no_std）：
//   * ≥10 项检测器参数闭集（开关/阈值/余量/迁移判定 N/静默窗…）；
//   * 默认值锚：出厂默认表注册后逐项比对（一致率 100%）；
//   * 篡改检出账：越域改值 10/10 检出并回退；
//   * 双域守恒：可变 + 冻结 = 登记；指纹冻结只读。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x8670 独占） ─────────────────────────────────────────────
pub const XC_T2_SEALED: u32 = 0x8671; // 封账后操作
pub const XC_T2_DUP: u32 = 0x8672; // 参数键重复
pub const XC_T2_OOB: u32 = 0x8673; // 值越域
pub const XC_T2_NOKEY: u32 = 0x8674; // 未登记键
pub const XC_T2_TAMPER: u32 = 0x8675; // 篡改检出
pub const XC_T2_INV: u32 = 0x8676; // 域倒置
pub const XC_T2_FULL: u32 = 0x8677; // 容量满

/// B18 检测器参数 OID 闭集（≥10 项）。
pub const D2_ENABLE: u32 = 0x1801; // 检测开关
pub const D2_TH_EXCEED: u32 = 0x1802; // 越限阈值
pub const D2_MARGIN: u32 = 0x1803; // 余量
pub const D2_MIG_N: u32 = 0x1804; // 迁移判定 N
pub const D2_SILENT_WIN: u32 = 0x1805; // 静默窗
pub const D2_TH_BUDGET: u32 = 0x1806; // 预算阈值
pub const D2_TH_STORM: u32 = 0x1807; // 风暴阈值
pub const D2_SAMPLE_INT: u32 = 0x1808; // 采样间隔
pub const D2_GUARD_TOL: u32 = 0x1809; // 防护容差
pub const D2_DEAD_TH: u32 = 0x180A; // 失活阈值
pub const D2_N: usize = 10;

/// 参数行。
#[derive(Clone, Copy)]
pub struct DetRow {
    pub oid: u32,
    pub value: u64,
    pub lo: u64,
    pub hi: u64,
    pub frozen: bool,
}

/// 出厂默认表（默认值锚）。
pub const DEFAULTS: [(u32, u64, u64, u64); D2_N] = [
    (D2_ENABLE, 1, 0, 1),
    (D2_TH_EXCEED, 8000, 1000, 10000),
    (D2_MARGIN, 500, 0, 5000),
    (D2_MIG_N, 4, 1, 64),
    (D2_SILENT_WIN, 1000, 0, 60000),
    (D2_TH_BUDGET, 6400, 100, 100000),
    (D2_TH_STORM, 50000, 1000, 1000000),
    (D2_SAMPLE_INT, 100, 1, 10000),
    (D2_GUARD_TOL, 200, 0, 10000),
    (D2_DEAD_TH, 3, 1, 100),
];

/// 检测器参数总表。
pub struct DetCfgBook {
    cap: usize,
    rows: Vec<DetRow>,
    tamper: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl DetCfgBook {
    pub fn new(cap: usize) -> DetCfgBook {
        DetCfgBook { cap, rows: vec![], tamper: 0, sealed: false, rejects: vec![] }
    }

    /// 登记参数。
    pub fn add(&mut self, oid: u32, value: u64, lo: u64, hi: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_T2_SEALED, "封账后登记"));
            return Err(XC_T2_SEALED);
        }
        if self.rows.len() >= self.cap {
            self.rejects.push((XC_T2_FULL, "容量满"));
            return Err(XC_T2_FULL);
        }
        if lo > hi {
            self.rejects.push((XC_T2_INV, "域倒置"));
            return Err(XC_T2_INV);
        }
        if value < lo || value > hi {
            self.rejects.push((XC_T2_OOB, "值越域"));
            return Err(XC_T2_OOB);
        }
        if self.rows.iter().any(|r| r.oid == oid) {
            self.rejects.push((XC_T2_DUP, "参数键重复"));
            return Err(XC_T2_DUP);
        }
        self.rows.push(DetRow { oid, value, lo, hi, frozen: false });
        Ok(())
    }

    /// 出厂默认全量登记（默认值锚）。
    pub fn load_defaults(&mut self) -> Result<usize, u32> {
        let mut n = 0usize;
        for (oid, v, lo, hi) in DEFAULTS {
            self.add(oid, v, lo, hi)?;
            n += 1;
        }
        Ok(n)
    }

    /// 改值（越域 = 篡改检出 + 回退）。
    pub fn set(&mut self, oid: u32, value: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_T2_SEALED, "封账后改值"));
            return Err(XC_T2_SEALED);
        }
        let idx = match self.rows.iter().position(|r| r.oid == oid) {
            Some(i) => i,
            None => {
                self.rejects.push((XC_T2_NOKEY, "未登记键"));
                return Err(XC_T2_NOKEY);
            }
        };
        if self.rows[idx].frozen {
            self.rejects.push((XC_T2_SEALED, "冻结参数改值"));
            return Err(XC_T2_SEALED);
        }
        if value < self.rows[idx].lo || value > self.rows[idx].hi {
            self.tamper += 1;
            self.rejects.push((XC_T2_TAMPER, "篡改检出回退"));
            return Err(XC_T2_TAMPER);
        }
        self.rows[idx].value = value;
        Ok(())
    }

    pub fn get(&self, oid: u32) -> Option<u64> {
        self.rows.iter().find(|r| r.oid == oid).map(|r| r.value)
    }

    /// 默认值一致率（千分比）：登记值 = 出厂默认。
    pub fn default_rate_bps(&self) -> u32 {
        if self.rows.is_empty() {
            return 0;
        }
        let ok = self
            .rows
            .iter()
            .filter(|r| DEFAULTS.iter().any(|(o, v, _, _)| *o == r.oid && *v == r.value))
            .count();
        (ok as u64 * 10_000 / self.rows.len() as u64) as u32
    }

    /// 冻结单项。
    pub fn freeze(&mut self, oid: u32) -> Result<(), u32> {
        match self.rows.iter_mut().find(|r| r.oid == oid) {
            Some(r) => {
                r.frozen = true;
                Ok(())
            }
            None => Err(XC_T2_NOKEY),
        }
    }

    pub fn tamper_cnt(&self) -> u64 {
        self.tamper
    }

    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }

    /// 指纹冻结。
    pub fn seal(&mut self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for r in &self.rows {
            for &b in &r.oid.to_le_bytes() {
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

// ── 深化块：篡改 10/10 与默认锚演练（判据主体路径） ───────────────────

/// 篡改 10/10：越域改值 10 次全检出且回退。
pub fn tamper10_run() -> Result<(u64, u64), u32> {
    let mut b = DetCfgBook::new(16);
    b.load_defaults()?;
    let before = b.get(D2_TH_EXCEED).unwrap_or(0);
    for k in 0..10u64 {
        if b.set(D2_TH_EXCEED, 100_001 + k).is_ok() {
            return Err(XC_T2_OOB);
        }
    }
    if b.tamper_cnt() != 10 || b.get(D2_TH_EXCEED) != Some(before) {
        return Err(XC_T2_TAMPER);
    }
    Ok((b.tamper_cnt(), before))
}

/// 默认锚演练：全量登记后一致率 100%。
pub fn default_anchor_run() -> Result<(usize, u32), u32> {
    let mut b = DetCfgBook::new(16);
    let n = b.load_defaults()?;
    if n != D2_N || b.default_rate_bps() != 10_000 {
        return Err(XC_T2_OOB);
    }
    Ok((n, b.default_rate_bps()))
}

// ── 注入核：七类破坏全抓 ─────────────────────────────────────────────
// 破坏面：①值越域登记 ②域倒置 ③键重复 ④未登记键 ⑤封账后改值
// ⑥容量满 ⑦冻结参数改值。

pub fn inject_core() -> (u32, u32) {
    let mut hit = 0u32;
    let mut total = 0u32;

    // ① 值越域登记 → XC_T2_OOB
    total += 1;
    let mut b = DetCfgBook::new(16);
    if b.add(D2_MARGIN, 99999, 0, 5000) == Err(XC_T2_OOB) {
        hit += 1;
    }

    // ② 域倒置 → XC_T2_INV
    total += 1;
    let mut b2 = DetCfgBook::new(16);
    if b2.add(D2_MIG_N, 4, 64, 1) == Err(XC_T2_INV) {
        hit += 1;
    }

    // ③ 键重复 → XC_T2_DUP
    total += 1;
    let mut b3 = DetCfgBook::new(16);
    let _ = b3.add(D2_ENABLE, 1, 0, 1);
    if b3.add(D2_ENABLE, 0, 0, 1) == Err(XC_T2_DUP) {
        hit += 1;
    }

    // ④ 未登记键 → XC_T2_NOKEY
    total += 1;
    let mut b4 = DetCfgBook::new(16);
    if b4.set(0x99, 1) == Err(XC_T2_NOKEY) {
        hit += 1;
    }

    // ⑤ 封账后改值 → XC_T2_SEALED
    total += 1;
    let mut b5 = DetCfgBook::new(16);
    let _ = b5.add(D2_ENABLE, 1, 0, 1);
    let _ = b5.seal();
    if b5.set(D2_ENABLE, 0) == Err(XC_T2_SEALED) {
        hit += 1;
    }

    // ⑥ 容量满 → XC_T2_FULL
    total += 1;
    let mut b6 = DetCfgBook::new(1);
    let _ = b6.add(D2_ENABLE, 1, 0, 1);
    if b6.add(D2_MIG_N, 4, 1, 64) == Err(XC_T2_FULL) {
        hit += 1;
    }

    // ⑦ 冻结参数改值 → XC_T2_SEALED（冻结面）
    total += 1;
    let mut b7 = DetCfgBook::new(16);
    let _ = b7.add(D2_MIG_N, 4, 1, 64);
    let _ = b7.freeze(D2_MIG_N);
    if b7.set(D2_MIG_N, 5) == Err(XC_T2_SEALED) {
        hit += 1;
    }

    (hit, total)
}
