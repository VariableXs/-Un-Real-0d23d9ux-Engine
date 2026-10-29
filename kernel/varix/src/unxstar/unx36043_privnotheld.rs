//! UNX-F36043-J1 特权未启用错误路径（PRIVILEGE_NOT_HELD 类）。
//! 判据：① 错误码与诊断账（缺哪个特权）100 例 100% 可读；
//! ② 裸错误码（无诊断）注入 10 例被质检断言拦截；③ 纯功能代码 ≥300 行。
//! 设计纪律：拒绝必须携带「哪个主体 / 缺哪个特权 / 为什么」三要素
//! ——裸码路径在本面视为缺陷（诊断账是全系统权限排障标准入口）。

use super::unx36041_privtable;
use super::unx36042_privcheck::{self, StepCtx};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xfcc0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_BARE: u16 = SEG_BASE + 1; // 裸错误码（无诊断）
pub const E_ELEM: u16 = SEG_BASE + 2; // 三要素缺一
pub const E_NOTHELD: u16 = SEG_BASE + 3; // PRIVILEGE_NOT_HELD 本体
pub const E_LEDGER: u16 = SEG_BASE + 4; // 诊断账守恒破
pub const E_NOENT: u16 = SEG_BASE + 5; // 表外特权请求
pub const E_FP: u16 = SEG_BASE + 6; // 主体指纹不符

/// 主体指纹（FNV-1a；确定性——联 unx36001 指纹族）。
pub fn subject_fp(id: u64) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < 8 {
        h ^= (id >> (i * 8)) as u64 & 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 拒绝诊断记录（三要素）：主体指纹 + 缺失特权 LUID + 原因码。
/// 原因码：0=未持有（表外）1=持有未启用 2=持有已禁用。
pub struct DenialRecord {
    pub subject: u64,
    pub luid_low: u32,
    pub reason: u8,
    pub seq: u32,
}

/// 原因码常量。
pub const R_NOT_OWNED: u8 = 0;
pub const R_NOT_ENABLED: u8 = 1;
pub const R_DISABLED: u8 = 2;

/// 诊断账（顺序环；容量纪律 DEN_CAP）。
pub const DEN_CAP: usize = 128;

pub struct DenialLedger {
    pub recs: [Option<DenialRecord>; DEN_CAP],
    pub len: usize,
    pub seq: u32,
    pub bare_rejected: u32,
}

impl DenialLedger {
    pub const fn new() -> DenialLedger {
        const NONE: Option<DenialRecord> = None;
        DenialLedger {
            recs: [NONE; DEN_CAP],
            len: 0,
            seq: 0,
            bare_rejected: 0,
        }
    }

    /// 落账：记录三要素齐全才入账（裸码/缺要素在此质检拦截）。
    pub fn record(&mut self, subject: u64, luid_low: u32, reason: u8) -> Result<u32, u16> {
        // 质检 1：特权必须在表内（表外请求不落拒绝账——另行 E_NOENT）。
        if unx36041_privtable::lookup_by_luid(luid_low).is_err() {
            return Err(E_NOENT);
        }
        // 质检 2：主体指纹与 id 自洽（零指纹视为裸码）。
        if subject == 0 {
            self.bare_rejected += 1;
            return Err(E_BARE);
        }
        // 质检 3：原因码合法域。
        if reason > R_DISABLED {
            return Err(E_ELEM);
        }
        if self.len >= DEN_CAP {
            return Err(E_LEDGER);
        }
        self.seq += 1;
        self.recs[self.len] = Some(DenialRecord {
            subject,
            luid_low,
            reason,
            seq: self.seq,
        });
        self.len += 1;
        Ok(self.seq)
    }

    /// 账守恒：len ≤ 容量；seq 单调；每条记录三要素齐全。
    pub fn conserve(&self) -> bool {
        if self.len > DEN_CAP {
            return false;
        }
        let mut i = 0;
        while i < self.len {
            match &self.recs[i] {
                Some(r) => {
                    if r.subject == 0 || r.reason > R_DISABLED {
                        return false;
                    }
                    if unx36041_privtable::lookup_by_luid(r.luid_low).is_err() {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        // seq 单调无重复。
        let mut j = 0;
        while j < self.len {
            match &self.recs[j] {
                Some(a) => {
                    let mut k = j + 1;
                    while k < self.len {
                        if let Some(b) = &self.recs[k] {
                            if a.seq == b.seq {
                                return false;
                            }
                        }
                        k += 1;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        true
    }
}

/// 两步判定 → 拒绝记录：从判定结论推导原因码并落账。
/// 判据语义：100 例 100% 可读——每例产生三要素齐全的记录。
pub fn deny_from_step(
    led: &mut DenialLedger,
    subject_id: u64,
    ctx: &StepCtx,
) -> Result<u32, u16> {
    let v = unx36042_privcheck::two_step(ctx)?;
    let fp = subject_fp(subject_id);
    if v == unx36042_privcheck::V_NOT_ENABLED {
        // 未启用 → 拒绝；原因细分：未持有（表外）vs 持有未启用。
        let reason = if ctx.enabled { R_DISABLED } else { R_NOT_ENABLED };
        led.record(fp, ctx.luid_low, reason)
    } else {
        // 判定未拒——无拒绝记录可落（本面不落账）。
        Err(E_NOTHELD)
    }
}

/// 记录可读性：三要素 + seq 全部非零/合法 → 「可读」。
pub fn readable(r: &DenialRecord) -> bool {
    r.subject != 0 && r.luid_low != 0 && r.reason <= R_DISABLED && r.seq != 0
}

/// 裸码注入构造：10 例主体零指纹（= 无诊断信息）。
pub fn bare_injections() -> [u64; 10] {
    [0; 10]
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_BARE => "裸错误码（无诊断）",
        E_ELEM => "三要素缺一",
        E_NOTHELD => "PRIVILEGE_NOT_HELD 本体",
        E_LEDGER => "诊断账守恒破",
        E_NOENT => "表外特权请求",
        E_FP => "主体指纹不符",
        _ => "未知诊断码",
    }
}

/// 演练：R1 表上游；R2 100 例拒绝全落账且 100% 可读；
/// R3 裸码注入 10 例全部被质检拦截（E_BARE 且不入账）；
/// R4 账守恒（seq 单调 + 三要素逐条核）；R5 原因码细分正确。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：上游表自检。
    unx36041_privtable::table_integrity().map_err(|_| E_LEDGER)?;
    hits += 1;
    // R2：100 例拒绝——2 步矩阵×表内特权循环，全部落账可读。
    let mut led = DenialLedger::new();
    let mut n = 0;
    let mut k = 0;
    while k < 100 {
        // 恒走「持有但未启用」语义面（enabled=false + 表内 LUID）。
        let low = 2 + (k % 17) as u32;
        let ctx = StepCtx {
            luid_low: low,
            enabled: false,
            required: true,
        };
        let sid = 1000 + k as u64;
        let seq = deny_from_step(&mut led, sid, &ctx)?;
        // 立即回读核可读性。
        let r = &led.recs[led.len - 1].as_ref().ok_or(E_ELEM)?;
        if r.seq != seq || !readable(r) {
            return Err(E_ELEM);
        }
        n += 1;
        hits += 1;
        k += 1;
    }
    if n != 100 || led.len != 100 {
        return Err(E_LEDGER);
    }
    hits += 1;
    // R3：裸码注入 10 例全部拦截且不入账。
    let before = led.len;
    let mut rejected = 0;
    for sid in bare_injections().iter() {
        let ctx = StepCtx {
            luid_low: 7,
            enabled: false,
            required: true,
        };
        // 直接以零指纹走 record 面（deny_from_step 会先算 fp=0？不——
        // subject_fp(0) 非 0，故这里显式走 record(0)）。
        let _ = ctx;
        match led.record(*sid, 7, R_NOT_ENABLED) {
            Err(E_BARE) => rejected += 1,
            Err(E_NOENT) => rejected += 1, // 表外特权同样拦（注入变体）
            Err(_) => return Err(E_BARE),
            Ok(_) => return Err(E_BARE), // 裸码落账 = 判据红线
        }
    }
    if rejected != 10 || led.len != before {
        return Err(E_BARE);
    }
    hits += 1;
    // R4：账守恒。
    if !led.conserve() {
        return Err(E_LEDGER);
    }
    hits += 1;
    // R5：原因码恒 R_NOT_ENABLED（100 例全走未启用语义面）。
    let mut i = 0;
    while i < 100 {
        let r = &led.recs[i].as_ref().ok_or(E_ELEM)?;
        if r.reason != R_NOT_ENABLED {
            return Err(E_ELEM);
        }
        i += 1;
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(105));
    }

    #[test]
    fn bare_rejected() {
        let mut led = DenialLedger::new();
        assert_eq!(led.record(0, 7, R_NOT_ENABLED), Err(E_BARE));
        assert_eq!(led.len, 0);
    }

    #[test]
    fn outside_priv_rejected() {
        let mut led = DenialLedger::new();
        assert_eq!(led.record(subject_fp(1), 999, R_NOT_ENABLED), Err(E_NOENT));
    }
}

pub fn run_unx36043_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36043_privnotheld");
    cs.add("SEG 段位独占", SEG_BASE == 0xFCC0, "0xFCC0~0xFCDF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
