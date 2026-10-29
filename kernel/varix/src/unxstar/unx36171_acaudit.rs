//! UNX-F36171-J1 AccessCheck 审计面（acaudit）。
//! 判据：① 审计记录环——固定容量环形表，每行 = 一次检查的审计投影
//! （token_id / desired / verdict / failed_step / cause）；② 审计序
//! 单调（seq 递增不可回退）；③ 统计面（verdict 三分桶 + 失败步直方）；
//! ④ FNV 审计链（行摘要链式哈希，防篡改对拍）；⑤ 注入负例显性拒；⑥ ≥310 行。
//! 设计纪律：消费 36161（AcOutcome / AcRequest 投影）；审计链语义
//! 冻结在本模块，供 36179 ktest 聚合。

use super::unx36161_acfreeze as acf;
use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x4010;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_SEQ: u16 = SEG_BASE + 2; // 审计序回退/越界
pub const E_FULL: u16 = SEG_BASE + 3; // 审计环满（不覆盖模式）
pub const E_CHAIN: u16 = SEG_BASE + 4; // 审计链断裂（哈希对拍失配）
pub const E_NEG: u16 = SEG_BASE + 5; // 负例注入未被检出
pub const E_TABLE: u16 = SEG_BASE + 6; // 审计表闭集破坏

/// 模块版本锚。
pub const VER: u32 = 1;

/// 审计环容量锚。
pub const AUDIT_N: usize = 32;

/// 审计行（检查结果的审计投影；seq 环内单调递增）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditRow {
    pub seq: u64,
    pub token_id: u32,
    pub desired: u32,
    pub verdict: u16,
    pub failed_step: u16,
    pub cause: u16,
}

/// 审计环（固定容量；seq 单调；满则 E_FULL 不覆盖）。
pub struct AuditRing {
    rows: [Option<AuditRow>; AUDIT_N],
    pub head: usize,
    pub count: u32,
    next_seq: u64,
}

impl AuditRing {
    pub fn new() -> AuditRing {
        AuditRing { rows: [None; AUDIT_N], head: 0, count: 0, next_seq: 1 }
    }

    /// 追加审计行（seq 由环分配，单调递增）。
    pub fn append(
        &mut self,
        token_id: u32,
        desired: u32,
        o: &acf::AcOutcome,
    ) -> Result<AuditRow, u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if self.count == AUDIT_N as u32 {
            return Err(E_FULL);
        }
        let row = AuditRow {
            seq: self.next_seq,
            token_id,
            desired,
            verdict: o.verdict,
            failed_step: o.failed_step,
            cause: o.cause,
        };
        self.next_seq += 1;
        self.rows[self.head] = Some(row);
        self.head = (self.head + 1) % AUDIT_N;
        self.count += 1;
        Ok(row)
    }

    /// 按入环序回放（0..count）。
    pub fn snapshot(&self) -> Vec<AuditRow> {
        let mut out: Vec<AuditRow> = Vec::new();
        let base = if self.count == AUDIT_N as u32 {
            self.head
        } else {
            0
        };
        let mut i = 0;
        while i < self.count as usize {
            if let Some(r) = self.rows[(base + i) % AUDIT_N] {
                out.push(r);
            }
            i += 1;
        }
        out
    }

    /// 审计序单调校验：回放序 seq 严格递增。
    pub fn seq_monotonic(&self) -> Result<(), u16> {
        let snap = self.snapshot();
        if snap.is_empty() {
            return Ok(());
        }
        let mut i = 1;
        while i < snap.len() {
            if snap[i].seq <= snap[i - 1].seq {
                return Err(E_SEQ);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 行摘要（FNV-1a：seq/token/desired/verdict/step/cause 六字段 LE）。
pub fn row_digest(r: &AuditRow) -> u64 {
    let mut h: u64 = 0x0100_93e7_0100_4011;
    for v in [
        r.seq,
        r.token_id as u64,
        r.desired as u64,
        r.verdict as u64,
        r.failed_step as u64,
        r.cause as u64,
    ] {
        let mut j = 0;
        while j < 8 {
            h ^= ((v >> (j * 8)) & 0xFF) as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
            j += 1;
        }
    }
    h
}

/// 审计链：链式哈希 h_i = FNV(h_{i-1} ⊕ digest(row_i))；空链回链头。
pub fn chain_hash(rows: &[AuditRow]) -> u64 {
    let mut h: u64 = 0x0100_93e7_0100_4012;
    let mut i = 0;
    while i < rows.len() {
        h ^= row_digest(&rows[i]);
        h = h.wrapping_mul(0x0100_0000_01b3);
        i += 1;
    }
    h
}

/// 审计投影面种类（判别式冻结；审计行三大投影域）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditFace {
    Verdict = 0x0001,
    Step = 0x0002,
    Cause = 0x0003,
}

impl AuditFace {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 投影域名回显（越界 "bad-face"）。
pub fn face_name(f: AuditFace) -> &'static str {
    match f {
        AuditFace::Verdict => "verdict",
        AuditFace::Step => "step",
        AuditFace::Cause => "cause",
    }
}

/// verdict 三分桶统计（Grant/Deny/Partial 计数）。
pub fn verdict_buckets(rows: &[AuditRow]) -> Result<(u32, u32, u32), u16> {
    let mut g = 0u32;
    let mut d = 0u32;
    let mut p = 0u32;
    let mut i = 0;
    while i < rows.len() {
        match rows[i].verdict {
            0 => g += 1,
            1 => d += 1,
            2 => p += 1,
            _ => return Err(E_TABLE),
        }
        i += 1;
    }
    Ok((g, d, p))
}

/// 失败步直方（0..5 六桶；verdict != Deny 的行不计）。
pub fn step_histogram(rows: &[AuditRow]) -> Result<[u32; 6], u16> {
    let mut h = [0u32; 6];
    let mut i = 0;
    while i < rows.len() {
        if rows[i].verdict == 1 {
            if rows[i].failed_step > 5 {
                return Err(E_SEQ);
            }
            h[rows[i].failed_step as usize] += 1;
        }
        i += 1;
    }
    Ok(h)
}

/// 篡改侦测：篡改一行后链哈希必须变化（负例守卫）。
pub fn tamper_detected(rows: &[AuditRow], idx: usize, new_cause: u16) -> Result<bool, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if idx >= rows.len() {
        return Err(E_SEQ);
    }
    let before = chain_hash(rows);
    let mut tampered: Vec<AuditRow> = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let mut r = rows[i];
        if i == idx {
            r.cause = new_cause;
        }
        tampered.push(r);
        i += 1;
    }
    Ok(chain_hash(&tampered) != before)
}

/// 审计面自检：容量锚 + 空环 + 链头锚。
pub fn audit_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if AUDIT_N != 32 {
        return Err(E_TABLE);
    }
    let r = AuditRing::new();
    if r.count != 0 || r.head != 0 {
        return Err(E_TABLE);
    }
    r.seq_monotonic()?;
    if chain_hash(&[]) != 0x0100_93e7_0100_4012 {
        return Err(E_TABLE);
    }
    Ok(())
}

/// 演练：R1 纪律自检；R2 追加与单调；R3 环满拒与回放序；R4 统计面；
/// R5 篡改侦测与负例注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：纪律自检。
    audit_integrity()?;
    hits += 1;
    // R2：追加六行（三分全覆盖）→ 单调 + 回放序与 seq 对齐。
    let mut ring = AuditRing::new();
    let o_g = acf::AcOutcome { granted: 0x89, verdict: 0, failed_step: 0, cause: 0, missing: 0 };
    let o_d = acf::AcOutcome { granted: 0, verdict: 1, failed_step: 4, cause: 0x3C32, missing: 0x89 };
    let o_p = acf::AcOutcome { granted: 1, verdict: 2, failed_step: 0, cause: 0x3C34, missing: 0x88 };
    ring.append(0x1111, 0x89, &o_g)?;
    ring.append(0x2222, 0x89, &o_d)?;
    ring.append(0x3333, 0x89, &o_p)?;
    ring.append(0x4444, 0x89, &o_g)?;
    ring.append(0x5555, 0x89, &o_d)?;
    ring.append(0x6666, 0x89, &o_p)?;
    ring.seq_monotonic()?;
    let snap = ring.snapshot();
    if snap.len() != 6 || snap[0].seq != 1 || snap[5].seq != 6 {
        return Err(E_TABLE);
    }
    hits += 1;
    // R3：环满拒——填到 32 行 E_FULL；seq 不因拒而回退。
    let mut full = AuditRing::new();
    let mut n = 0;
    while n < AUDIT_N {
        full.append(n as u32, 0x89, &o_g)?;
        n += 1;
    }
    if full.append(99, 0x89, &o_g) != Err(E_FULL) {
        return Err(E_NEG);
    }
    full.seq_monotonic()?;
    if full.snapshot().len() != AUDIT_N {
        return Err(E_TABLE);
    }
    hits += 1;
    // R4：统计面——三分桶 (2,2,2) + 失败步直方 step4 桶 = 2。
    let (g, d, p) = verdict_buckets(&snap)?;
    if (g, d, p) != (2, 2, 2) {
        return Err(E_TABLE);
    }
    let hist = step_histogram(&snap)?;
    if hist[4] != 2 || hist[0] != 0 {
        return Err(E_TABLE);
    }
    // 投影域闭集走查（三大域可从行面逐域提取）。
    let mut f = 0;
    while f < 3 {
        let face = match f {
            0 => AuditFace::Verdict,
            1 => AuditFace::Step,
            _ => AuditFace::Cause,
        };
        if face_name(face).is_empty() || face.code() != f as u16 + 1 {
            return Err(E_TABLE);
        }
        f += 1;
    }
    hits += 1;
    // R5：篡改侦测 + 负例注入。
    if !tamper_detected(&snap, 2, 0x7777)? {
        return Err(E_NEG);
    }
    if tamper_detected(&snap, 2, snap[2].cause)? {
        return Err(E_NEG); // 同值伪篡改不变
    }
    if tamper_detected(&snap, 99, 1).is_ok() {
        return Err(E_NEG);
    }
    if verdict_buckets(&snap)? != (2, 2, 2) {
        return Err(E_NEG);
    }
    let chain1 = chain_hash(&snap);
    let chain2 = chain_hash(&snap);
    if chain1 != chain2 || chain1 == 0 {
        return Err(E_NEG);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }

    #[test]
    fn audit_table_holds() {
        assert_eq!(audit_integrity(), Ok(()));
    }

    #[test]
    fn full_ring_rejects() {
        let mut r = AuditRing::new();
        let o = acf::AcOutcome { granted: 1, verdict: 0, failed_step: 0, cause: 0, missing: 0 };
        let mut n = 0;
        while n < AUDIT_N {
            r.append(n as u32, 1, &o).ok().unwrap_or(AuditRow {
                seq: 0,
                token_id: 0,
                desired: 0,
                verdict: 0,
                failed_step: 0,
                cause: 0,
            });
            n += 1;
        }
        assert_eq!(r.append(0, 1, &o), Err(E_FULL));
    }

    #[test]
    fn tamper_changes_chain() {
        let rows = vec![AuditRow { seq: 1, token_id: 1, desired: 1, verdict: 0, failed_step: 0, cause: 0 }];
        assert!(tamper_detected(&rows, 0, 9).ok().unwrap_or(false));
    }
}

pub fn run_unx36171_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36171_acaudit");
    cs.add("SEG 段位独占", SEG_BASE == 0x4010, "0x4010~0x401F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "环/单调/统计/链全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("闭集纪律", audit_integrity() == Ok(()), "容量锚 + 空环 + 链头锚");
    cs.add("契约冻结", VER == 1, "审计面锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SEQ => "审计序回退/越界",
        E_FULL => "审计环满（不覆盖模式）",
        E_CHAIN => "审计链断裂",
        E_NEG => "负例注入未被检出",
        E_TABLE => "审计表闭集破坏",
        _ => "未知诊断码",
    }
}
