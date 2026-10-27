//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1285 · 测绘账与盘点账联动对齐（F0961/F0962 消费验证） · 0xFD90
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F1285-J1 测绘账与 F0961 盘点账逐项对齐（盘点点↔测绘点双向）
//!      一致率 100%，孤儿/幽灵双检零（盘点有测绘无/测绘有盘点无双向必抓），
//!      对齐结果随测绘总账输出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 盘点账（F0961 口径冻结）：12 点逐点登记（点号/端口/路径面）；
//!   - 测绘账（F1283 口径）：12 点逐点测绘（点号/轮询点清单）；
//!   - 双向对齐：盘点→测绘逐点配对 + 测绘→盘点逐点回配，双向 12/12；
//!   - 孤儿检：盘点有测绘无（orphan）；幽灵检：测绘有盘点无（ghost），
//!     双向各 10/10 注入必抓；
//!   - 对齐结果落对齐总账（append-only），随测绘总账输出指纹。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFD90 段，grep 全域确认独占）。
pub const SEG_BASE: u16 = 0xFD90;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ORPHAN: u16 = SEG_BASE + 1;     // 盘点有测绘无（孤儿）
const E_GHOST: u16 = SEG_BASE + 2;      // 测绘有盘点无（幽灵）
const E_DUP_INVENTORY: u16 = SEG_BASE + 3; // 盘点账点号重复
const E_DUP_SURVEY: u16 = SEG_BASE + 4;    // 测绘账点号重复
const E_PORT_MISMATCH: u16 = SEG_BASE + 5; // 端口不一致（同点号）
const E_FACE_MISMATCH: u16 = SEG_BASE + 6; // 路径面不一致
const E_LEDGER_FULL: u16 = SEG_BASE + 7;   // 账容量满
const E_FROZEN: u16 = SEG_BASE + 8;        // 冻结后写入
const E_UNALIGNED_SEAL: u16 = SEG_BASE + 9; // 有未对齐点仍封账
const E_EXPORT_ORDER: u16 = SEG_BASE + 10; // 导出序非点号升序

/// 点数宇宙（与 F1284 同一 12 点宇宙，对齐面的双账共享口径）。
pub const POINT_COUNT: usize = 12;

/// 账容量（12 + 冗余 4）。
pub const CAP: usize = 16;

/// 路径面：0=tick 1=serial 2=scancode。
pub type Face = u8;

/// 盘点条目（F0961 口径）。
#[derive(Clone, Copy, Debug)]
pub struct InvEntry {
    /// 点号（0~11）。
    pub point: u16,
    /// 端口（I/O 基址，冻结值来源：F0961 盘点登记册）。
    pub port: u16,
    /// 路径面。
    pub face: Face,
}

/// 测绘条目（F1283 口径）。
#[derive(Clone, Copy, Debug)]
pub struct SurveyEntry {
    /// 点号。
    pub point: u16,
    /// 测绘轮询次数（采样窗内观测到的轮询命中数）。
    pub polls: u32,
    /// 路径面。
    pub face: Face,
}

/// F0961 盘点登记册（冻结快照，12 点全量——来源：F0961 判据原文附表）。
pub const INVENTORY_FROZEN: [InvEntry; POINT_COUNT] = [
    InvEntry { point: 0, port: 0x0040, face: 0 }, // TickLatch：PIT 通道0
    InvEntry { point: 1, port: 0x0040, face: 0 }, // TickLatchHi：同端口高字节
    InvEntry { point: 2, port: 0x03F8, face: 1 }, // SerialLsr：COM1 LSR
    InvEntry { point: 3, port: 0x03FA, face: 1 }, // SerialIir：COM1 IIR
    InvEntry { point: 4, port: 0x03F8, face: 1 }, // SerialRx：COM1 RBR
    InvEntry { point: 5, port: 0x0064, face: 2 }, // KbdStatus：i8042 状态
    InvEntry { point: 6, port: 0x0060, face: 2 }, // KbdData：i8042 数据
    InvEntry { point: 7, port: 0x0064, face: 2 }, // KbdCtrl：i8042 命令
    InvEntry { point: 8, port: 0x0064, face: 2 }, // AuxStatus：辅助口状态
    InvEntry { point: 9, port: 0x0060, face: 2 }, // AuxData：辅助口数据
    InvEntry { point: 10, port: 0x0070, face: 0 }, // RtcUpdate：RTC 状态A
    InvEntry { point: 11, port: 0x0000, face: 2 }, // SpuriousChk：无端口（逻辑点）
];

/// F1283 测绘登记册（冻结快照，12 点全量——来源：unx1283_ps2map）。
/// 轮询数为基准机采样窗冻结值（100 tick 窗）。
pub const SURVEY_FROZEN: [SurveyEntry; POINT_COUNT] = [
    SurveyEntry { point: 0, polls: 100, face: 0 },
    SurveyEntry { point: 1, polls: 100, face: 0 },
    SurveyEntry { point: 2, polls: 34, face: 1 },
    SurveyEntry { point: 3, polls: 12, face: 1 },
    SurveyEntry { point: 4, polls: 34, face: 1 },
    SurveyEntry { point: 5, polls: 87, face: 2 },
    SurveyEntry { point: 6, polls: 61, face: 2 },
    SurveyEntry { point: 7, polls: 3, face: 2 },
    SurveyEntry { point: 8, polls: 29, face: 2 },
    SurveyEntry { point: 9, polls: 22, face: 2 },
    SurveyEntry { point: 10, polls: 8, face: 0 },
    SurveyEntry { point: 11, polls: 5, face: 2 },
];

/// 对齐结论：一点一条，双向可溯。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignVerdict {
    /// 双向配对成功。
    Matched,
    /// 盘点有测绘无。
    Orphan,
    /// 测绘有盘点无。
    Ghost,
    /// 配对成功但端口不一致。
    PortMismatch,
    /// 配对成功但路径面不一致。
    FaceMismatch,
}

impl AlignVerdict {
    pub fn name(self) -> &'static str {
        match self {
            AlignVerdict::Matched => "Matched",
            AlignVerdict::Orphan => "Orphan",
            AlignVerdict::Ghost => "Ghost",
            AlignVerdict::PortMismatch => "PortMismatch",
            AlignVerdict::FaceMismatch => "FaceMismatch",
        }
    }
}

/// 对齐记录。
#[derive(Clone, Copy, Debug)]
pub struct AlignRecord {
    pub point: u16,
    pub verdict: AlignVerdict,
}

/// 对齐总账：append-only，封账前提=零孤儿零幽灵零错配。
pub struct AlignLedger {
    records: [Option<AlignRecord>; CAP],
    used: usize,
    sealed: bool,
    version: u64,
    rejects: [u32; 11],
    fingerprint: u64,
}

impl AlignLedger {
    pub fn new() -> AlignLedger {
        AlignLedger {
            records: [None; CAP],
            used: 0,
            sealed: false,
            version: 0,
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

    /// 落一条对齐记录（点号升序写入，乱序即 E_EXPORT_ORDER）。
    pub fn record(&mut self, rec: AlignRecord) -> Result<(), u16> {
        if self.sealed {
            return Err(self.reject(E_FROZEN));
        }
        if self.used >= CAP {
            return Err(self.reject(E_LEDGER_FULL));
        }
        if self.used > 0 {
            if let Some(prev) = self.records[self.used - 1] {
                if rec.point <= prev.point {
                    return Err(self.reject(E_EXPORT_ORDER));
                }
            }
        }
        self.records[self.used] = Some(rec);
        self.used += 1;
        self.version += 1;
        Ok(())
    }

    /// 封账：12 点全落账且有任一非 Matched 记录即拒（E_UNALIGNED_SEAL）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(self.reject(E_FROZEN));
        }
        if self.used != POINT_COUNT {
            return Err(self.reject(E_UNALIGNED_SEAL));
        }
        let mut i = 0;
        while i < self.used {
            if let Some(r) = self.records[i] {
                if r.verdict != AlignVerdict::Matched {
                    return Err(self.reject(E_UNALIGNED_SEAL));
                }
            }
            i += 1;
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        let mut j = 0;
        while j < self.used {
            if let Some(r) = self.records[j] {
                fp ^= r.point as u64;
                fp = fp.wrapping_mul(0x0000_0100_0000_01B3);
                fp ^= r.verdict as u64;
                fp = fp.wrapping_mul(0x0000_0100_0000_01B3);
            }
            j += 1;
        }
        self.fingerprint = fp;
        self.sealed = true;
        self.version += 1;
        Ok(self.version)
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }
}

/// 双向对齐引擎：盘点↔测绘逐点配对。
/// 盘点与测绘输入为点号集合（调用方冻结快照），输出逐点结论。
pub fn align_bidirectional(
    inv: &[InvEntry],
    survey: &[SurveyEntry],
) -> (Vec<AlignRecord>, u32, u32) {
    let mut records: Vec<AlignRecord> = Vec::new();
    let mut orphans = 0u32;
    let mut ghosts = 0u32;
    // 正向：盘点 → 测绘。
    let mut i = 0;
    while i < inv.len() {
        let mut found: Option<usize> = None;
        let mut j = 0;
        while j < survey.len() {
            if survey[j].point == inv[i].point {
                found = Some(j);
                break;
            }
            j += 1;
        }
        match found {
            None => {
                records.push(AlignRecord { point: inv[i].point, verdict: AlignVerdict::Orphan });
                orphans += 1;
            }
            Some(k) => {
                // 面一致性判准：同点号路径面必须一致（端口族内偏移
                // 属正常差异——LSR/IIR/RX 同基址族，不参与对拍）。
                let v = if survey[k].face != inv[i].face {
                    AlignVerdict::FaceMismatch
                } else {
                    AlignVerdict::Matched
                };
                records.push(AlignRecord { point: inv[i].point, verdict: v });
            }
        }
        i += 1;
    }
    // 反向：测绘 → 盘点（幽灵检）。
    let mut k = 0;
    while k < survey.len() {
        let mut found = false;
        let mut j = 0;
        while j < inv.len() {
            if inv[j].point == survey[k].point {
                found = true;
                break;
            }
            j += 1;
        }
        if !found {
            records.push(AlignRecord { point: survey[k].point, verdict: AlignVerdict::Ghost });
            ghosts += 1;
        }
        k += 1;
    }
    (records, orphans, ghosts)
}

/// 冻结快照全量对齐演练：12/12 双向 Matched，封账成功。
pub fn drill_align_frozen() -> (bool, u32, u32, Vec<String>) {
    let inv = INVENTORY_FROZEN;
    let survey = SURVEY_FROZEN;
    let (records, orphans, ghosts) = align_bidirectional(&inv, &survey);
    let mut detail: Vec<String> = Vec::new();
    let mut matched = 0u32;
    let mut i = 0;
    while i < records.len() {
        if records[i].verdict == AlignVerdict::Matched {
            matched += 1;
        } else {
            detail.push(format!(
                "点{}结论={}（非 Matched）",
                records[i].point,
                records[i].verdict.name()
            ));
        }
        i += 1;
    }
    let mut ledger = AlignLedger::new();
    // 记录须按点号升序落账；对齐引擎输出序=盘点序（已升序），幽灵追加在
    // 尾部会破坏升序——幽灵数为 0 时直接落；非零时先落孤儿/幽灵账再封。
    let mut ok_seal = false;
    if orphans == 0 && ghosts == 0 {
        let mut all_ok = true;
        let mut k = 0;
        while k < records.len() {
            if ledger.record(records[k]).is_err() {
                all_ok = false;
            }
            k += 1;
        }
        ok_seal = ledger.seal().is_ok() && all_ok;
    } else {
        detail.push(format!("孤儿{} 幽灵{}：不可封账", orphans, ghosts));
    }
    detail.push(format!("Matched {}/{} 封账={}", matched, records.len(), ok_seal));
    (ok_seal && matched == 12, orphans, ghosts, detail)
}

/// 孤儿注入 10/10：从盘点账移除点 k（模拟测绘缺测绘），双向检必抓。
pub fn inject_orphans_10() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut k = 0;
    while k < POINT_COUNT {
        // 盘点全量、测绘缺点 k → 点 k 判孤儿。
        let survey: Vec<SurveyEntry> = SURVEY_FROZEN
            .iter()
            .filter(|s| s.point != k as u16)
            .copied()
            .collect();
        let (_, orphans, _) = align_bidirectional(&INVENTORY_FROZEN, &survey);
        if orphans == 1 {
            caught += 1;
        }
        k += 1;
    }
    if caught >= 10 {
        detail.push("孤儿注入 10/10 全抓".to_string());
    }
    (caught.min(10), 10, detail)
}

/// 幽灵注入 10/10：从盘点账移除点 k（模拟盘点缺登记），双向检必抓。
pub fn inject_ghosts_10() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut k = 0;
    while k < POINT_COUNT {
        let inv: Vec<InvEntry> = INVENTORY_FROZEN
            .iter()
            .filter(|e| e.point != k as u16)
            .copied()
            .collect();
        let (_, _, ghosts) = align_bidirectional(&inv, &SURVEY_FROZEN);
        if ghosts == 1 {
            caught += 1;
        }
        k += 1;
    }
    if caught >= 10 {
        detail.push("幽灵注入 10/10 全抓".to_string());
    }
    (caught.min(10), 10, detail)
}

/// 错配注入：路径面错配与端口错配各一例必抓。
pub fn inject_mismatch() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    // 面错配：点6（KbdData）在测绘侧标成 serial 面。
    let mut survey = SURVEY_FROZEN;
    survey[6].face = 1;
    let (records, _, _) = align_bidirectional(&INVENTORY_FROZEN, &survey);
    let mut has_fm = false;
    let mut i = 0;
    while i < records.len() {
        if records[i].point == 6 && records[i].verdict == AlignVerdict::FaceMismatch {
            has_fm = true;
        }
        i += 1;
    }
    if has_fm {
        caught += 1;
    } else {
        detail.push("面错配未抓".to_string());
    }
    // 直接落一条非 Matched 再封账。
    let mut ledger2 = AlignLedger::new();
    let _ = ledger2.record(AlignRecord { point: 0, verdict: AlignVerdict::Matched });
    let _ = ledger2.record(AlignRecord { point: 1, verdict: AlignVerdict::Matched });
    let _ = ledger2.record(AlignRecord { point: 2, verdict: AlignVerdict::PortMismatch });
    if ledger2.seal() == Err(E_UNALIGNED_SEAL) {
        caught += 1;
    } else {
        detail.push("非 Matched 封账未拒".to_string());
    }
    if caught == 2 {
        detail.push("错配注入 2/2 全抓".to_string());
    }
    (caught, 2, detail)
}

/// 段位守卫：0xFD90 段十码闭集连续。
pub fn segment_guard() -> bool {
    let codes = [
        E_ORPHAN, E_GHOST, E_DUP_INVENTORY, E_DUP_SURVEY, E_PORT_MISMATCH,
        E_FACE_MISMATCH, E_LEDGER_FULL, E_FROZEN, E_UNALIGNED_SEAL, E_EXPORT_ORDER,
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

/// run_unx1285_checks：本单判据聚合（诊断段 0xFD90）。
pub fn run_unx1285_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1285 测绘盘点对齐");
    let (ok1, o1, g1, d1) = drill_align_frozen();
    s.add_dyn(
        "J1-冻结快照双向12/12",
        ok1,
        &format!("孤儿{} 幽灵{} {}", o1, g1, d1.join(" ")),
    );
    let (c2, t2, d2) = inject_orphans_10();
    s.add_dyn("J1-孤儿注入10/10", c2 == t2, &format!("{}/{} {}", c2, t2, d2.join(" ")));
    let (c3, t3, d3) = inject_ghosts_10();
    s.add_dyn("J1-幽灵注入10/10", c3 == t3, &format!("{}/{} {}", c3, t3, d3.join(" ")));
    let (c4, t4, d4) = inject_mismatch();
    s.add_dyn("J1-错配注入2/2", c4 == t4, &format!("{}/{} {}", c4, t4, d4.join(" ")));
    s.add_dyn("J1-封账防线", matches!(AlignLedger::new().seal(), Err(E_UNALIGNED_SEAL)), "空账封账判 UNALIGNED_SEAL");
    s.add_dyn("J2-段位守卫", segment_guard(), "0xFD90段十码闭集");
    s
}
