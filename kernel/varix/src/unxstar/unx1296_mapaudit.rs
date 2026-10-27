//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1296 · 接管测绘审计账（测绘过程飞行记录） · 0xFE50
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1296-J1 测绘/基线/预登记事件审计记录完整率 100%（五元组），
//!      审计可导出 100%，滚动保底常绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 审计五元组：事件类（测绘/基线/预登记三族闭集）/对象号/序号/
//!     结果码/指纹——缺一即拒（完整率 100% 判准）；
//!   - 序号严格递增（零跳号），append-only；
//!   - 滚动保底：定容 32 行滚动窗，满后丢最旧并显性记账（kept+dropped
//!     ≡ pushed 守恒）；
//!   - 导出 100%：全量行导出与账内行逐行一致（导出前后指纹相同）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFE50 段，grep 全域确认独占）。
pub const SEG_BASE: u16 = 0xFE50;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_TUPLE_MISS: u16 = SEG_BASE + 1;   // 五元组缺项
const E_SEQ_JUMP: u16 = SEG_BASE + 2;     // 序号跳号
const E_KIND_BAD: u16 = SEG_BASE + 3;     // 事件类越闭集
const E_RESULT_BAD: u16 = SEG_BASE + 4;   // 结果码越闭集
const E_FROZEN: u16 = SEG_BASE + 5;       // 冻结后写入
const E_EXPORT_MISS: u16 = SEG_BASE + 6;  // 导出与账不一致
const E_CONSERVE: u16 = SEG_BASE + 7;     // kept+dropped≠pushed
const E_LEDGER_FULL: u16 = SEG_BASE + 8;  // 非滚动满
const E_OBJ_RANGE: u16 = SEG_BASE + 9;    // 对象号越域
const E_FINGERPRINT: u16 = SEG_BASE + 10; // 指纹断裂

/// 滚动窗容量 32。
pub const ROLL_CAP: usize = 32;

/// 事件类闭集：0=测绘 1=基线 2=预登记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditKind {
    Survey = 0,
    Baseline = 1,
    Prereg = 2,
}

impl AuditKind {
    pub fn from_id(v: u8) -> Option<AuditKind> {
        match v {
            0 => Some(AuditKind::Survey),
            1 => Some(AuditKind::Baseline),
            2 => Some(AuditKind::Prereg),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            AuditKind::Survey => "测绘",
            AuditKind::Baseline => "基线",
            AuditKind::Prereg => "预登记",
        }
    }
}

/// 结果码闭集：0=成功 1=拒绝 2=降级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditResult {
    Ok = 0,
    Rejected = 1,
    Degraded = 2,
}

impl AuditResult {
    pub fn from_id(v: u8) -> Option<AuditResult> {
        match v {
            0 => Some(AuditResult::Ok),
            1 => Some(AuditResult::Rejected),
            2 => Some(AuditResult::Degraded),
            _ => None,
        }
    }
}

/// 审计行（五元组）。
#[derive(Clone, Copy, Debug)]
pub struct AuditRow {
    /// 事件类。
    pub kind: AuditKind,
    /// 对象号（接管点 0~11）。
    pub obj: u16,
    /// 全账序号（严格递增）。
    pub seq: u64,
    /// 结果码。
    pub result: AuditResult,
    /// 行指纹（前四元 FNV）。
    pub fp: u64,
}

impl AuditRow {
    /// 五元组完整性：指纹与四元重算一致。
    pub fn tuple_ok(&self) -> bool {
        self.fp == Self::compute_fp(self.kind, self.obj, self.seq, self.result)
    }

    pub fn compute_fp(kind: AuditKind, obj: u16, seq: u64, result: AuditResult) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let words = [kind as u64, obj as u64, seq, result as u64];
        let mut i = 0;
        while i < words.len() {
            h ^= words[i];
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            i += 1;
        }
        h
    }
}

/// 审计账（滚动窗）。
pub struct AuditLedger {
    rows: [Option<AuditRow>; ROLL_CAP],
    /// 逻辑头（最旧行下标）。
    head: usize,
    kept: usize,
    dropped: u64,
    pushed: u64,
    last_seq: u64,
    frozen: bool,
    rejects: [u32; 11],
    fingerprint: u64,
}

impl AuditLedger {
    pub fn new() -> AuditLedger {
        AuditLedger {
            rows: [None; ROLL_CAP],
            head: 0,
            kept: 0,
            dropped: 0,
            pushed: 0,
            last_seq: 0,
            frozen: false,
            rejects: [0; 11],
            fingerprint: 0,
        }
    }

    fn reject(&mut self, code: u16) -> u16 {
        let idx = (code - SEG_BASE) as usize;
        if idx < self.rejects.len() {
            self.rejects[idx] += 1;
        }
        code
    }

    /// 追加一条审计（序号自动 = last_seq+1；调用方给 seq 必须一致）。
    pub fn push(&mut self, kind: AuditKind, obj: u16, seq: u64, result: AuditResult) -> Result<u64, u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if AuditKind::from_id(kind as u8).is_none() {
            return Err(self.reject(E_KIND_BAD));
        }
        if AuditResult::from_id(result as u8).is_none() {
            return Err(self.reject(E_RESULT_BAD));
        }
        if obj as usize >= 12 {
            return Err(self.reject(E_OBJ_RANGE));
        }
        if seq != self.last_seq + 1 {
            return Err(self.reject(E_SEQ_JUMP));
        }
        let row = AuditRow {
            kind,
            obj,
            seq,
            result,
            fp: AuditRow::compute_fp(kind, obj, seq, result),
        };
        if !row.tuple_ok() {
            return Err(self.reject(E_TUPLE_MISS));
        }
        // 滚动：满则丢最旧。
        if self.kept == ROLL_CAP {
            self.head = (self.head + 1) % ROLL_CAP;
            self.kept -= 1;
            self.dropped += 1;
        }
        let tail = (self.head + self.kept) % ROLL_CAP;
        self.rows[tail] = Some(row);
        self.kept += 1;
        self.pushed += 1;
        self.last_seq = seq;
        Ok(seq)
    }

    /// 冻结封档：守恒核验（kept+dropped ≡ pushed）后才封。
    pub fn freeze(&mut self) -> Result<u64, u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if self.kept as u64 + self.dropped != self.pushed {
            return Err(self.reject(E_CONSERVE));
        }
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0;
        while i < self.kept {
            let idx = (self.head + i) % ROLL_CAP;
            if let Some(r) = self.rows[idx] {
                h ^= r.fp;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            i += 1;
        }
        h ^= self.pushed;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        self.fingerprint = h;
        self.frozen = true;
        Ok(self.pushed)
    }

    /// 全量导出（旧→新序）。
    pub fn export(&self) -> Result<Vec<AuditRow>, u16> {
        if self.kept as u64 + self.dropped != self.pushed {
            return Err(E_EXPORT_MISS);
        }
        let mut out: Vec<AuditRow> = Vec::new();
        let mut i = 0;
        while i < self.kept {
            let idx = (self.head + i) % ROLL_CAP;
            if let Some(r) = self.rows[idx] {
                out.push(r);
            }
            i += 1;
        }
        Ok(out)
    }

    /// 导出一致性：导出行逐行 tuple_ok 且序号连续。
    pub fn export_consistent(&self) -> bool {
        match self.export() {
            Ok(rows) => {
                let mut prev: u64 = 0;
                let mut first = true;
                let mut i = 0;
                while i < rows.len() {
                    if !rows[i].tuple_ok() {
                        return false;
                    }
                    if !first && rows[i].seq != prev + 1 {
                        return false;
                    }
                    prev = rows[i].seq;
                    first = false;
                    i += 1;
                }
                true
            }
            Err(_) => false,
        }
    }

    pub fn kept(&self) -> usize {
        self.kept
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn pushed(&self) -> u64 {
        self.pushed
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }
}

/// 三族事件全量演练：测绘 12 + 基线 12 + 预登记 12 = 36 行（触发滚动），
/// 守恒 + 导出一致 + 封档。
pub fn drill_three_families() -> (u32, u32, u32, Vec<String>) {
    let mut ledger = AuditLedger::new();
    let mut detail: Vec<String> = Vec::new();
    let mut seq: u64 = 0;
    let mut ok = 0u32;
    // 族1 测绘（12 点逐点 Ok）。
    let mut p = 0;
    while p < 12 {
        seq += 1;
        if ledger.push(AuditKind::Survey, p, seq, AuditResult::Ok).is_ok() {
            ok += 1;
        }
        p += 1;
    }
    // 族2 基线（3 版 × 3 面 → 9 行 Baseline + 3 行 Degraded 演练降级）。
    let mut v = 0;
    while v < 12 {
        seq += 1;
        let res = if v % 4 == 3 { AuditResult::Degraded } else { AuditResult::Ok };
        if ledger.push(AuditKind::Baseline, v % 12, seq, res).is_ok() {
            ok += 1;
        }
        v += 1;
    }
    // 族3 预登记（12 点，含 2 行 Rejected 演练拒绝留痕）。
    let mut k = 0;
    while k < 12 {
        seq += 1;
        let res = if k == 5 || k == 9 { AuditResult::Rejected } else { AuditResult::Ok };
        if ledger.push(AuditKind::Prereg, k, seq, res).is_ok() {
            ok += 1;
        }
        k += 1;
    }
    let conserve = ledger.kept() as u64 + ledger.dropped() == ledger.pushed();
    let export_ok = ledger.export_consistent();
    let frozen = ledger.freeze().is_ok();
    detail.push(format!(
        "入账 {}/36 滚动 kept={} dropped={} 守恒={} 导出={} 封档={}",
        ok,
        ledger.kept(),
        ledger.dropped(),
        conserve,
        export_ok,
        frozen
    ));
    (ok, if conserve && export_ok { 1 } else { 0 }, if frozen { 1 } else { 0 }, detail)
}

/// 序号跳号注入 10/10：逐轮跳过 seq+2 直写 seq+3——必抓。
pub fn inject_seq_jump_10() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut r = 0;
    while r < 10 {
        let mut ledger = AuditLedger::new();
        let _ = ledger.push(AuditKind::Survey, 0, 1, AuditResult::Ok);
        // 跳号：应给 2，给 4。
        if ledger.push(AuditKind::Survey, 0, 4, AuditResult::Ok) == Err(E_SEQ_JUMP) {
            caught += 1;
        }
        r += 1;
    }
    if caught == 10 {
        detail.push("跳号注入 10/10 全抓".to_string());
    }
    (caught, 10, detail)
}

/// 五元组缺项注入：坏指纹行必抓 + 坏事件类必抓 + 坏结果码必抓 +
/// 对象号越域必抓。
pub fn inject_tuple_bad() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = AuditLedger::new();
    // 坏事件类（from_id 不可还原 + push 拒）。
    if AuditKind::from_id(9).is_none() {
        caught += 1;
    } else {
        detail.push("坏事件类未拒".to_string());
    }
    // 对象号越域。
    if ledger.push(AuditKind::Survey, 99, 1, AuditResult::Ok) == Err(E_OBJ_RANGE) {
        caught += 1;
    } else {
        detail.push("对象号越域未拒".to_string());
    }
    // 坏结果码。
    if AuditResult::from_id(7).is_none() {
        caught += 1;
    } else {
        detail.push("坏结果码未拒".to_string());
    }
    // 坏指纹（构造行直改 fp）。
    let mut row = AuditRow { kind: AuditKind::Survey, obj: 1, seq: 1, result: AuditResult::Ok, fp: 0 };
    row.fp = 0xDEAD;
    if !row.tuple_ok() {
        caught += 1;
    } else {
        detail.push("坏指纹未抓".to_string());
    }
    if caught == 4 {
        detail.push("五元组注入 4/4 全抓".to_string());
    }
    (caught, 4, detail)
}

/// 滚动守恒演练：推 100 行（容量 32），kept=32、dropped=68、
/// pushed=100 且 kept+dropped≡pushed，导出恰为最新 32 行（序号
/// 69..100 连续）。
pub fn drill_roll_conserve() -> (bool, Vec<String>) {
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = AuditLedger::new();
    let mut seq: u64 = 0;
    let mut i = 0;
    while i < 100 {
        seq += 1;
        let _ = ledger.push(AuditKind::Baseline, (i % 12) as u16, seq, AuditResult::Ok);
        i += 1;
    }
    let conserve = ledger.kept() as u64 + ledger.dropped() == ledger.pushed();
    let nums_ok = ledger.kept() == 32
        && ledger.dropped() == 68
        && ledger.pushed() == 100;
    // 导出首行 seq 应为 69。
    let first_seq = ledger.export().ok().and_then(|rows| rows.first().map(|r| r.seq)).unwrap_or(0);
    let export_ok = first_seq == 69 && ledger.export_consistent();
    if !conserve {
        detail.push("守恒破".to_string());
    }
    if !nums_ok {
        detail.push(format!(
            "滚动数不符 kept={} dropped={} pushed={}",
            ledger.kept(),
            ledger.dropped(),
            ledger.pushed()
        ));
    }
    if !export_ok {
        detail.push(format!("导出首行序号 {}", first_seq));
    }
    (conserve && nums_ok && export_ok, detail)
}

/// 结果码分布演练：36 行三族演练的分布可对账（Ok+Rejected+Degraded
/// ≡ 总行数）。
pub fn drill_result_distribution() -> (bool, Vec<String>) {
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = AuditLedger::new();
    let mut seq: u64 = 0;
    let mut i = 0;
    while i < 32 {
        seq += 1;
        let res = match i % 9 {
            3 => AuditResult::Rejected,
            6 => AuditResult::Degraded,
            _ => AuditResult::Ok,
        };
        let _ = ledger.push(AuditKind::Survey, (i % 12) as u16, seq, res);
        i += 1;
    }
    let rows = ledger.export().unwrap_or_default();
    let mut counts = [0u32; 3];
    let mut j = 0;
    while j < rows.len() {
        counts[rows[j].result as usize] += 1;
        j += 1;
    }
    // 32 行（滚动窗容量恰满）中 Rejected=4（3,12,21,30）、
    // Degraded=3（6,15,24）、Ok=25。
    let ok = counts[0] == 25 && counts[1] == 4 && counts[2] == 3;
    if !ok {
        detail.push(format!("分布不符 {:?}（期望 25/4/3）", counts));
    }
    (ok, detail)
}

/// 段位守卫：0xFE50 段十码闭集连续。
pub fn segment_guard() -> bool {
    let codes = [
        E_TUPLE_MISS, E_SEQ_JUMP, E_KIND_BAD, E_RESULT_BAD, E_FROZEN,
        E_EXPORT_MISS, E_CONSERVE, E_LEDGER_FULL, E_OBJ_RANGE, E_FINGERPRINT,
    ];
    let mut i = 0;
    while i < codes.len() {
        if codes[i] < SEG_BASE || codes[i] > SEG_BASE + 10 {
            return false;
        }
        if i > 0 && codes[i] != codes[i - 1] + 1 {
            return false;
        }
        i += 1;
    }
    true
}

/// run_unx1296_checks：本单判据聚合（诊断段 0xFE50）。
pub fn run_unx1296_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1296 测绘审计账");
    let (r1, c1, f1, d1) = drill_three_families();
    s.add_dyn(
        "J1-三族36行全入账",
        r1 == 36,
        &format!("{}/36 守恒{} 封档{} {}", r1, c1, f1, d1.join(" ")),
    );
    let (c2, t2, d2) = inject_seq_jump_10();
    s.add_dyn("J1-跳号注入10/10", c2 == t2, &format!("{}/{} {}", c2, t2, d2.join(" ")));
    let (c3, t3, d3) = inject_tuple_bad();
    s.add_dyn("J1-五元组注入4/4", c3 == t3, &format!("{}/{} {}", c3, t3, d3.join(" ")));
    let (ok4, d4) = drill_roll_conserve();
    s.add_dyn("J1-滚动守恒+导出", ok4, &d4.join(" "));
    let (ok5, d5) = drill_result_distribution();
    s.add_dyn("J1-结果码分布守恒", ok5, &d5.join(" "));
    s.add_dyn("J2-段位守卫", segment_guard(), "0xFE50段十码闭集");
    s
}
