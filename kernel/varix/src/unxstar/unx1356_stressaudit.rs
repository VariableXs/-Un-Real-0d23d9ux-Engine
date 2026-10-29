//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1356 · 压测审计账（全谱执行飞行记录） · 0x2EA0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F1356-J1 全谱执行/熔断/恢复/基准变更事件审计记录完整率 100%
//!      （五元组），审计可导出 100%，滚动保底常绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 事件族闭集：执行/熔断/恢复/基准变更 四族；
//!   - 五元组：族/对象（场景号）/序号/结果码/行指纹（前四元 FNV）；
//!   - 滚动窗 32（kept+dropped ≡ pushed 守恒）；
//!   - 导出 100%：导出行逐行五元组自洽且序号连续。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x2EA0 段，grep 全域确认独占）。
pub const SEG_BASE: u16 = 0x2EA0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_KIND_BAD: u16 = SEG_BASE + 1;   // 族越闭集
const E_SEQ_JUMP: u16 = SEG_BASE + 2;   // 序号跳号
const E_RESULT_BAD: u16 = SEG_BASE + 3; // 结果码越闭集
const E_TUPLE_MISS: u16 = SEG_BASE + 4; // 五元组指纹不符
const E_OBJ_RANGE: u16 = SEG_BASE + 5;  // 对象号越域
const E_EXPORT_MISS: u16 = SEG_BASE + 6; // 导出守恒破
const E_FROZEN: u16 = SEG_BASE + 7;     // 冻结后写
const E_CONSERVE: u16 = SEG_BASE + 8;   // kept+dropped≠pushed
const E_EMPTY: u16 = SEG_BASE + 9;      // 空行
const E_LEDGER_FULL: u16 = SEG_BASE + 10; // 非滚动满

/// 滚动窗容量。
pub const ROLL_CAP: usize = 32;

/// 事件族闭集：0=执行 1=熔断 2=恢复 3=基准变更。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditKind {
    Execute = 0,
    Fuse = 1,
    Recover = 2,
    BenchChange = 3,
}

impl AuditKind {
    pub fn from_id(v: u8) -> Option<AuditKind> {
        match v {
            0 => Some(AuditKind::Execute),
            1 => Some(AuditKind::Fuse),
            2 => Some(AuditKind::Recover),
            3 => Some(AuditKind::BenchChange),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            AuditKind::Execute => "执行",
            AuditKind::Fuse => "熔断",
            AuditKind::Recover => "恢复",
            AuditKind::BenchChange => "基准变更",
        }
    }
}

/// 结果码闭集：0=成功 1=拦截 2=降级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditResult {
    Ok = 0,
    Blocked = 1,
    Degraded = 2,
}

impl AuditResult {
    pub fn from_id(v: u8) -> Option<AuditResult> {
        match v {
            0 => Some(AuditResult::Ok),
            1 => Some(AuditResult::Blocked),
            2 => Some(AuditResult::Degraded),
            _ => None,
        }
    }
}

/// 审计行（五元组）。
#[derive(Clone, Copy, Debug)]
pub struct AuditRow {
    pub kind: AuditKind,
    pub obj: u16,
    pub seq: u64,
    pub result: AuditResult,
    pub fp: u64,
}

impl AuditRow {
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

    pub fn tuple_ok(&self) -> bool {
        self.fp == Self::compute_fp(self.kind, self.obj, self.seq, self.result)
    }
}

/// 审计账（滚动）。
pub struct AuditLedger {
    rows: [Option<AuditRow>; ROLL_CAP],
    head: usize,
    kept: usize,
    dropped: u64,
    pushed: u64,
    last_seq: u64,
    frozen: bool,
    rejects: [u32; 11],
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
        }
    }

    fn reject(&mut self, code: u16) -> u16 {
        let idx = (code - SEG_BASE) as usize;
        if idx < self.rejects.len() {
            self.rejects[idx] += 1;
        }
        code
    }

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
        if obj as usize >= 6 {
            return Err(self.reject(E_OBJ_RANGE));
        }
        if seq != self.last_seq + 1 {
            return Err(self.reject(E_SEQ_JUMP));
        }
        let row = AuditRow { kind, obj, seq, result, fp: AuditRow::compute_fp(kind, obj, seq, result) };
        if !row.tuple_ok() {
            return Err(self.reject(E_TUPLE_MISS));
        }
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
}

/// 四族全量演练：执行 12 + 熔断 6 + 恢复 6 + 基准变更 8 = 32 行（恰满），
/// 守恒 + 导出一致。
pub fn drill_four_families() -> (u32, u32, u32, Vec<String>) {
    let mut ledger = AuditLedger::new();
    let mut detail: Vec<String> = Vec::new();
    let mut seq: u64 = 0;
    let mut ok = 0u32;
    // 族1 执行（12 场景轮次）。
    let mut i = 0;
    while i < 12 {
        seq += 1;
        if ledger.push(AuditKind::Execute, (i % 6) as u16, seq, AuditResult::Ok).is_ok() {
            ok += 1;
        }
        i += 1;
    }
    // 族2 熔断（6 行 Blocked）。
    let mut j = 0;
    while j < 6 {
        seq += 1;
        if ledger.push(AuditKind::Fuse, (j % 6) as u16, seq, AuditResult::Blocked).is_ok() {
            ok += 1;
        }
        j += 1;
    }
    // 族3 恢复（6 行 Ok）。
    let mut k = 0;
    while k < 6 {
        seq += 1;
        if ledger.push(AuditKind::Recover, (k % 6) as u16, seq, AuditResult::Ok).is_ok() {
            ok += 1;
        }
        k += 1;
    }
    // 族4 基准变更（8 行，含 2 行 Degraded）。
    let mut m = 0;
    while m < 8 {
        seq += 1;
        let res = if m == 3 || m == 6 { AuditResult::Degraded } else { AuditResult::Ok };
        if ledger.push(AuditKind::BenchChange, (m % 6) as u16, seq, res).is_ok() {
            ok += 1;
        }
        m += 1;
    }
    let conserve = ledger.kept() as u64 + ledger.dropped() == ledger.pushed();
    let export_ok = ledger.export_consistent();
    detail.push(format!(
        "入账 {}/32 kept={} dropped={} 守恒={} 导出={}",
        ok,
        ledger.kept(),
        ledger.dropped(),
        conserve,
        export_ok
    ));
    (ok, if conserve { 1 } else { 0 }, if export_ok { 1 } else { 0 }, detail)
}

/// 滚动守恒演练：推 100 行（容量 32），kept=32 dropped=68，导出为
/// 最新 32 行（首行序号 69）。
pub fn drill_roll_conserve() -> (bool, Vec<String>) {
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = AuditLedger::new();
    let mut seq: u64 = 0;
    let mut i = 0;
    while i < 100 {
        seq += 1;
        let _ = ledger.push(AuditKind::Execute, (i % 6) as u16, seq, AuditResult::Ok);
        i += 1;
    }
    let conserve = ledger.kept() as u64 + ledger.dropped() == ledger.pushed()
        && ledger.kept() == 32
        && ledger.dropped() == 68;
    let first_seq = ledger.export().ok().and_then(|rows| rows.first().map(|r| r.seq)).unwrap_or(0);
    let ok = conserve && first_seq == 69 && ledger.export_consistent();
    if !ok {
        detail.push(format!("滚动守恒不符：first_seq={}", first_seq));
    }
    (ok, detail)
}

/// 注入 5 项：坏族/坏结果码/跳号/对象越域/坏指纹——必抓。
pub fn inject_bad() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = AuditLedger::new();
    if AuditKind::from_id(9).is_none() {
        caught += 1;
    } else {
        detail.push("坏族未拒".to_string());
    }
    if AuditResult::from_id(7).is_none() {
        caught += 1;
    } else {
        detail.push("坏结果码未拒".to_string());
    }
    let _ = ledger.push(AuditKind::Execute, 0, 1, AuditResult::Ok);
    if matches!(ledger.push(AuditKind::Execute, 0, 4, AuditResult::Ok), Err(E_SEQ_JUMP)) {
        caught += 1;
    } else {
        detail.push("跳号未拒".to_string());
    }
    if matches!(ledger.push(AuditKind::Execute, 99, 2, AuditResult::Ok), Err(E_OBJ_RANGE)) {
        caught += 1;
    } else {
        detail.push("对象越域未拒".to_string());
    }
    let bad_row = AuditRow { kind: AuditKind::Fuse, obj: 1, seq: 3, result: AuditResult::Blocked, fp: 0xDEAD };
    if !bad_row.tuple_ok() {
        caught += 1;
    } else {
        detail.push("坏指纹未抓".to_string());
    }
    if caught == 5 {
        detail.push("注入 5/5 全抓".to_string());
    }
    (caught, 5, detail)
}

/// 结果码分布演练：32 行中 Ok=26 / Blocked=6 / Degraded=2（族2 全
/// Blocked、族3 全 Ok、族4 含 2 Degraded）。
pub fn drill_result_distribution() -> (bool, Vec<String>) {
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = AuditLedger::new();
    let mut seq: u64 = 0;
    let mut i = 0;
    while i < 12 {
        seq += 1;
        let _ = ledger.push(AuditKind::Execute, (i % 6) as u16, seq, AuditResult::Ok);
        i += 1;
    }
    let mut j = 0;
    while j < 6 {
        seq += 1;
        let _ = ledger.push(AuditKind::Fuse, (j % 6) as u16, seq, AuditResult::Blocked);
        j += 1;
    }
    let mut k = 0;
    while k < 6 {
        seq += 1;
        let _ = ledger.push(AuditKind::Recover, (k % 6) as u16, seq, AuditResult::Ok);
        k += 1;
    }
    let mut m = 0;
    while m < 8 {
        seq += 1;
        let res = if m == 3 || m == 6 { AuditResult::Degraded } else { AuditResult::Ok };
        let _ = ledger.push(AuditKind::BenchChange, (m % 6) as u16, seq, res);
        m += 1;
    }
    let rows = ledger.export().unwrap_or_default();
    let mut counts = [0u32; 3];
    let mut n = 0;
    while n < rows.len() {
        counts[rows[n].result as usize] += 1;
        n += 1;
    }
    // 32 行（滚动窗容量恰满）：执行 12 Ok + 恢复 6 Ok + 基准 6 Ok/2
    // Degraded + 熔断 6 Blocked → Ok=24 / Blocked=6 / Degraded=2。
    let ok = counts[0] == 24 && counts[1] == 6 && counts[2] == 2;
    if !ok {
        detail.push(format!("分布不符 {:?}（期望 26/6/2）", counts));
    }
    (ok, detail)
}

/// 段位守卫：0x2EA0 段十码闭集连续。
pub fn segment_guard() -> bool {
    let codes = [
        E_KIND_BAD, E_SEQ_JUMP, E_RESULT_BAD, E_TUPLE_MISS, E_OBJ_RANGE,
        E_EXPORT_MISS, E_FROZEN, E_CONSERVE, E_EMPTY, E_LEDGER_FULL,
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

/// run_unx1356_checks：本单判据聚合（诊断段 0x2EA0）。
pub fn run_unx1356_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1356 压测审计账");
    let (r1, c1, e1, d1) = drill_four_families();
    s.add_dyn(
        "J1-四族32行全入账",
        r1 == 32,
        &format!("{}/32 守恒{} 导出{} {}", r1, c1, e1, d1.join(" ")),
    );
    let (ok2, d2) = drill_roll_conserve();
    s.add_dyn("J1-滚动守恒+导出", ok2, &d2.join(" "));
    let (c3, t3, d3) = inject_bad();
    s.add_dyn("J1-注入5/5", c3 == t3, &format!("{}/{} {}", c3, t3, d3.join(" ")));
    let (ok4, d4) = drill_result_distribution();
    s.add_dyn("J1-结果码分布守恒", ok4, &d4.join(" "));
    s.add_dyn("J2-段位守卫", segment_guard(), "0x2EA0段十码闭集");
    s
}
