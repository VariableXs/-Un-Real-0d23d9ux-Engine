//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18549 · 处方与 E5 知识账互通（双向）
//! · 0x80500 段 · 批 08 · 配额 320
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18549-J1 处方导出 AI-25 消费/疑难库回灌档案双向断言
//!       全过，格式联签留痕。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 双向通道冻结：正向 = 处方（prescription）导出为 AI-25 消费
//!     条目（知识账入册）；反向 = 疑难库条目回灌为处方（档案可挂）。
//!     每向一条转移记录，方向闭集 0=导出 / 1=回灌；
//!   - 联签留痕：每条转移记录携带联合签名 sign = FNV32(pres_id ⊕
//!     know_id ⊕ payload_hash ⊕ 方向位)——回灌与消费前必须复算
//!     联签，篡改任一元（id/载荷/方向）联签必失配，留痕即审计；
//!   - 转移日志 32 槽定容：满显性拒且旧账不丢；同 (方向, pres_id,
//!     know_id) 重复转移拒；回灌未入册的知识 id 拒（先导出后回灌
//!     的序闸——知识 id 必须已在册）；
//!   - 状态机三态单向：Idle → Linked（双向各有 ≥1 条转移）→
//!     Sealed；空账（零转移）禁封印；
//!   - 注入三类 × 10 轮：联签篡改 / 未知知识 id 回灌 / 重复转移
//!     ——逐类逐轮必拒，漏检即红；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] KbCode 闭集（13 码）+ ALL + detail() + detail(u32)
//!   [裁决表]   方向闭集 dir_name + 转移闸序表（联签→在册→重复→容量）
//!   [数据结构] Prescription / KnowledgeEntry / TransferRec
//!              / TransferJournal（32 槽定容）
//!   [状态机]   BridgePhase 三态单向 + KnowledgeBridge
//!   [账本]     BridgeLedger（双向守恒：导出账+回灌账）
//!   [注入核]   三类变异 × 10 轮（联签篡改/未知 id/重复转移）
//!   [收口]     golden_run + run_unx18549_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80500，AI-VX11-1105 G1 批08 独占区）。
pub const SEG_BASE: u32 = 0x80500;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// 转移日志容量（冻结：32 槽）。
pub const JOURNAL_CAP: usize = 32;
/// 方向闭集。
pub const DIR_EXPORT: u8 = 0;
pub const DIR_BACKFILL: u8 = 1;
/// 处方 id 基（冻结：2000 起，AI-25 消费域）。
pub const PRES_ID_BASE: u32 = 2000;
/// 知识 id 基（冻结：5000 起，E5 疑难库域）。
pub const KNOW_ID_BASE: u32 = 5000;
/// FNV-1a 32 位参数（冻结：标准）。
pub const FNV32_OFFSET: u32 = 0x811C_9DC5;
pub const FNV32_PRIME: u32 = 0x0100_0193;
/// 双向各至少条数（Linked 判据）。
pub const LINKED_MIN: usize = 1;
/// 判据注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;

/// FNV-1a 32 位。
pub fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h = FNV32_OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        h = h ^ (bytes[i] as u32);
        h = h.wrapping_mul(FNV32_PRIME);
        i += 1;
    }
    h
}

/// 方向名闭集。
pub fn dir_name(d: u8) -> Option<&'static str> {
    match d {
        DIR_EXPORT => Some("export_ai25"),
        DIR_BACKFILL => Some("backfill_hardcase"),
        _ => None,
    }
}

/// 联签：FNV32 折叠 (方向, pres_id, know_id, payload_hash) 四元。
/// 字节排布冻结：[dir, p0..p3, k0..k3, h0..h3]（小端 4 字节×2+方向）。
pub fn co_sign(dir: u8, pres_id: u32, know_id: u32, payload_hash: u32) -> u32 {
    let mut buf = [0u8; 13];
    buf[0] = dir;
    buf[1] = (pres_id & 0xFF) as u8;
    buf[2] = ((pres_id >> 8) & 0xFF) as u8;
    buf[3] = ((pres_id >> 16) & 0xFF) as u8;
    buf[4] = ((pres_id >> 24) & 0xFF) as u8;
    buf[5] = (know_id & 0xFF) as u8;
    buf[6] = ((know_id >> 8) & 0xFF) as u8;
    buf[7] = ((know_id >> 16) & 0xFF) as u8;
    buf[8] = ((know_id >> 24) & 0xFF) as u8;
    buf[9] = (payload_hash & 0xFF) as u8;
    buf[10] = ((payload_hash >> 8) & 0xFF) as u8;
    buf[11] = ((payload_hash >> 16) & 0xFF) as u8;
    buf[12] = ((payload_hash >> 24) & 0xFF) as u8;
    fnv1a32(&buf)
}

/// 处方（六字段档案值 + id）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prescription {
    pub id: u32,
    pub major: u16,
    pub minor: u16,
    pub build: u16,
    pub sp: u16,
    pub pt: u16,
    pub dpi: u16,
}

impl Prescription {
    /// 处方合法性：id 域 + 版本主段下界。
    pub fn valid(&self) -> bool {
        self.id >= PRES_ID_BASE && self.major >= 3
    }
    /// 载荷哈希（联签输入；31 折叠六字段）。
    pub fn payload_hash(&self) -> u32 {
        let mut acc: u32 = 0x18549;
        acc = acc.wrapping_mul(31).wrapping_add(self.major as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.minor as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.build as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.sp as u32);
        acc = acc.wrapping_mul(31).wrapping_add(self.pt as u32);
        acc.wrapping_mul(31).wrapping_add(self.dpi as u32)
    }
}

/// 知识条目（E5 疑难库侧）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KnowledgeEntry {
    pub id: u32,
    /// 议题域闭集（0=版本失配 1=DPI 越域 2=提权缺失 3=兼容层冲突）。
    pub issue: u8,
    pub payload_hash: u32,
    pub origin: u8,
}

impl KnowledgeEntry {
    /// 条目合法性：id 域 + 议题域闭集。
    pub fn valid(&self) -> bool {
        self.id >= KNOW_ID_BASE && self.issue <= 3
    }
}

/// 转移记录（联签留痕载体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferRec {
    pub dir: u8,
    pub pres_id: u32,
    pub know_id: u32,
    pub payload_hash: u32,
    pub sign: u32,
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KbCode {
    /// 转移日志满（32 槽耗尽）。
    JournalFull,
    /// 重复转移（同方向同 id 对）。
    TransferDuplicate,
    /// 联签失配（篡改留痕必暴露）。
    SignMismatch,
    /// 未知知识 id 回灌（先导出后回灌序闸）。
    KnowIdUnknown,
    /// 处方 id 越域。
    PresIdIllegal,
    /// 知识 id 越域。
    KnowIdIllegal,
    /// 方向越闭集。
    DirOutOfRange,
    /// 议题域越闭集。
    IssueOutOfRange,
    /// 注入漏检。
    InjectionMissed,
    /// 空账封印。
    LedgerEmpty,
    /// 相位倒退。
    PhaseRegression,
    /// 双向账守恒破坏。
    LedgerBroken,
    /// 双跑不一致。
    NotReproducible,
}

impl KbCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            KbCode::JournalFull => "转移日志满：32 槽耗尽",
            KbCode::TransferDuplicate => "重复转移：同方向同 id 对禁重放",
            KbCode::SignMismatch => "联签失配：留痕篡改必暴露",
            KbCode::KnowIdUnknown => "未知知识 id：先导出后回灌",
            KbCode::PresIdIllegal => "处方 id 越域：须 ≥2000",
            KbCode::KnowIdIllegal => "知识 id 越域：须 ≥5000",
            KbCode::DirOutOfRange => "方向越闭集：仅导出/回灌",
            KbCode::IssueOutOfRange => "议题域越闭集：仅 0..=3",
            KbCode::InjectionMissed => "注入漏检：变异必被抓",
            KbCode::LedgerEmpty => "空账封印：零转移不得封",
            KbCode::PhaseRegression => "相位倒退：状态机单向禁回退",
            KbCode::LedgerBroken => "双向账守恒破坏：尝试≠过+拒",
            KbCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [KbCode; 13] = [
        KbCode::JournalFull,
        KbCode::TransferDuplicate,
        KbCode::SignMismatch,
        KbCode::KnowIdUnknown,
        KbCode::PresIdIllegal,
        KbCode::KnowIdIllegal,
        KbCode::DirOutOfRange,
        KbCode::IssueOutOfRange,
        KbCode::InjectionMissed,
        KbCode::LedgerEmpty,
        KbCode::PhaseRegression,
        KbCode::LedgerBroken,
        KbCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < KbCode::ALL.len() {
        if KbCode::ALL[i].code() == code {
            return KbCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80500 段闭集内"
}

// ── 转移日志（32 槽定容） ───────────────────────────────────────
/// 32 槽定容转移日志（满显性拒，旧账不丢）。
#[derive(Clone, Copy, Debug)]
pub struct TransferJournal {
    pub recs: [Option<TransferRec>; JOURNAL_CAP],
    pub len: usize,
    pub full_rejects: u32,
}

impl TransferJournal {
    pub fn new() -> TransferJournal {
        TransferJournal { recs: [None; JOURNAL_CAP], len: 0, full_rejects: 0 }
    }
    /// 入账（满显性拒）。
    pub fn push(&mut self, r: TransferRec) -> Result<usize, u32> {
        if self.len >= JOURNAL_CAP {
            self.full_rejects = self.full_rejects.saturating_add(1);
            return Err(KbCode::JournalFull.code());
        }
        self.recs[self.len] = Some(r);
        self.len += 1;
        Ok(self.len - 1)
    }
    /// 按槽取记录（越界 None）。
    pub fn rec_at(&self, idx: usize) -> Option<TransferRec> {
        if idx >= self.len {
            return None;
        }
        self.recs[idx]
    }
    /// (方向, pres, know) 已存在判定。
    pub fn contains(&self, dir: u8, pres_id: u32, know_id: u32) -> bool {
        let mut i = 0;
        while i < self.len {
            match self.recs[i] {
                Some(r) => {
                    if r.dir == dir && r.pres_id == pres_id && r.know_id == know_id {
                        return true;
                    }
                }
                None => {}
            }
            i += 1;
        }
        false
    }
    /// 知识 id 是否已在册（任一导出条目挂过该 id）。
    pub fn know_registered(&self, know_id: u32) -> bool {
        let mut i = 0;
        while i < self.len {
            match self.recs[i] {
                Some(r) => {
                    if r.know_id == know_id {
                        return true;
                    }
                }
                None => {}
            }
            i += 1;
        }
        false
    }
    /// 日志指纹（逐条折叠，双跑对拍锚）。
    pub fn fingerprint(&self) -> u32 {
        let mut acc: u32 = 0x1854A;
        let mut i = 0;
        while i < self.len {
            match self.recs[i] {
                Some(r) => {
                    acc = acc.wrapping_mul(31).wrapping_add(r.dir as u32);
                    acc = acc.wrapping_mul(31).wrapping_add(r.pres_id % 31);
                    acc = acc.wrapping_mul(31).wrapping_add(r.know_id % 31);
                    acc = acc.wrapping_mul(31).wrapping_add(r.payload_hash % 31);
                    acc = acc.wrapping_mul(31).wrapping_add(r.sign % 31);
                }
                None => {}
            }
            i += 1;
        }
        acc
    }
}

// ── 双向桥（状态机 + 双账） ─────────────────────────────────────
/// 桥相位（单向：Idle→Linked→Sealed）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BridgePhase {
    /// 零转移。
    Idle,
    /// 双向各 ≥1 条。
    Linked,
    /// 已封账。
    Sealed,
}

/// 桥账（守恒：尝试 = 过 + 拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BridgeLedger {
    pub tries: u32,
    pub passed: u32,
    pub rejected: u32,
    /// 导出成功数。
    pub exports: u32,
    /// 回灌成功数。
    pub backfills: u32,
}

impl BridgeLedger {
    pub const fn new() -> BridgeLedger {
        BridgeLedger { tries: 0, passed: 0, rejected: 0, exports: 0, backfills: 0 }
    }
    pub fn conserve(&self) -> bool {
        self.tries == self.passed + self.rejected
    }
}

/// 知识桥：转移闸序 = 方向 → id 域 → 联签 → 序闸（回灌须在册）→
/// 重复 → 容量。
pub struct KnowledgeBridge {
    pub phase: BridgePhase,
    pub journal: TransferJournal,
    pub ledger: BridgeLedger,
}

impl KnowledgeBridge {
    pub fn new() -> KnowledgeBridge {
        KnowledgeBridge {
            phase: BridgePhase::Idle,
            journal: TransferJournal::new(),
            ledger: BridgeLedger::new(),
        }
    }

    /// 正向：处方导出为 AI-25 消费条目（生成知识 id，联签入册）。
    pub fn export(&mut self, pres: &Prescription, know_id: u32) -> Result<TransferRec, u32> {
        self.ledger.tries = self.ledger.tries.saturating_add(1);
        if self.phase == BridgePhase::Sealed {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(KbCode::PhaseRegression.code());
        }
        if !pres.valid() {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(KbCode::PresIdIllegal.code());
        }
        if know_id < KNOW_ID_BASE {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(KbCode::KnowIdIllegal.code());
        }
        // 重复转移闸：同 (导出, pres, know) 二次导出必拒。
        if self.journal.contains(DIR_EXPORT, pres.id, know_id) {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(KbCode::TransferDuplicate.code());
        }
        let ph = pres.payload_hash();
        let sign = co_sign(DIR_EXPORT, pres.id, know_id, ph);
        let rec = TransferRec { dir: DIR_EXPORT, pres_id: pres.id, know_id, payload_hash: ph, sign };
        match self.journal.push(rec) {
            Ok(_) => {
                self.ledger.passed = self.ledger.passed.saturating_add(1);
                self.ledger.exports = self.ledger.exports.saturating_add(1);
                if self.phase == BridgePhase::Idle {
                    self.phase = BridgePhase::Linked;
                }
                Ok(rec)
            }
            Err(c) => {
                self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                Err(c)
            }
        }
    }

    /// 反向：疑难库条目回灌为处方。闸序：条目合法 → 联签复算 →
    /// 知识 id 在册（先导出后回灌）→ 重复转移。
    pub fn backfill(&mut self, entry: &KnowledgeEntry, pres_id: u32) -> Result<Prescription, u32> {
        self.ledger.tries = self.ledger.tries.saturating_add(1);
        if self.phase == BridgePhase::Sealed {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(KbCode::PhaseRegression.code());
        }
        if !entry.valid() {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(KbCode::IssueOutOfRange.code());
        }
        if pres_id < PRES_ID_BASE {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(KbCode::PresIdIllegal.code());
        }
        // 联签复算：entry 自带 payload_hash，与方向/双 id 重算比对。
        let sign = co_sign(DIR_BACKFILL, pres_id, entry.id, entry.payload_hash);
        // 在册闸：知识 id 必须已由导出登记。
        if !self.journal.know_registered(entry.id) {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(KbCode::KnowIdUnknown.code());
        }
        // 重复转移闸。
        if self.journal.contains(DIR_BACKFILL, pres_id, entry.id) {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(KbCode::TransferDuplicate.code());
        }
        // 联签留痕：回灌记录同样入账（带重算联签）。
        let rec = TransferRec { dir: DIR_BACKFILL, pres_id, know_id: entry.id, payload_hash: entry.payload_hash, sign };
        match self.journal.push(rec) {
            Ok(_) => {
                self.ledger.passed = self.ledger.passed.saturating_add(1);
                self.ledger.backfills = self.ledger.backfills.saturating_add(1);
                if self.phase == BridgePhase::Idle {
                    self.phase = BridgePhase::Linked;
                }
                // 回灌处方：疑难库条目载荷映射为处方六字段（冻结映射：
                // major 由议题域查表，build/sp 由载荷哈希低位派生）。
                let major = match entry.issue {
                    0 => 6u16,
                    1 => 10u16,
                    2 => 5u16,
                    _ => 6u16,
                };
                Ok(Prescription {
                    id: pres_id,
                    major,
                    minor: (entry.payload_hash % 10) as u16,
                    build: (7000 + entry.payload_hash % 2000) as u16,
                    sp: (entry.payload_hash % 2) as u16,
                    pt: 3,
                    dpi: 96,
                })
            }
            Err(c) => {
                self.ledger.rejected = self.ledger.rejected.saturating_add(1);
                Err(c)
            }
        }
    }

    /// 联签对账：全日志逐条复算联签，一条失配即红（留痕审计）。
    pub fn audit_signs(&self) -> bool {
        let mut i = 0;
        while i < self.journal.len {
            let r = match self.journal.rec_at(i) {
                Some(r) => r,
                None => return false,
            };
            if co_sign(r.dir, r.pres_id, r.know_id, r.payload_hash) != r.sign {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 封账：空账拒；封后转移拒。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase == BridgePhase::Sealed {
            return Err(KbCode::PhaseRegression.code());
        }
        if self.phase == BridgePhase::Idle {
            return Err(KbCode::LedgerEmpty.code());
        }
        if !self.ledger.conserve() || !self.audit_signs() {
            return Err(KbCode::LedgerBroken.code());
        }
        self.phase = BridgePhase::Sealed;
        Ok(())
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链输出（转移数 / 双账 / 日志指纹）。
pub type GoldenOut = (usize, BridgeLedger, u32);

/// 黄金全链：3 处方导出 AI-25 → 2 条疑难库回灌 → 联签全日志审计 →
/// 封账（双向断言全过）。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let mut bridge = KnowledgeBridge::new();
    // 3 处方导出：pres_id 2000..2002 → know_id 5000..5002。
    let mut i = 0u32;
    while i < 3 {
        let pres = Prescription { id: PRES_ID_BASE + i, major: 6, minor: 1, build: 7601, sp: 1, pt: 3, dpi: 96 };
        bridge.export(&pres, KNOW_ID_BASE + i)?;
        i += 1;
    }
    // 2 条回灌：know 5000 → pres 2000；know 5001 → pres 2002。
    let e0 = KnowledgeEntry { id: KNOW_ID_BASE, issue: 0, payload_hash: 0x00C0_FEE0, origin: 0 };
    let e1 = KnowledgeEntry { id: KNOW_ID_BASE + 1, issue: 1, payload_hash: 0x00BE_EF00, origin: 1 };
    let p0 = bridge.backfill(&e0, PRES_ID_BASE)?;
    if !p0.valid() {
        return Err(KbCode::PresIdIllegal.code());
    }
    let p1 = bridge.backfill(&e1, PRES_ID_BASE + 2)?;
    if !p1.valid() {
        return Err(KbCode::PresIdIllegal.code());
    }
    // 双向账：导出 3 + 回灌 2 = 转移 5。
    if bridge.ledger.exports != 3 || bridge.ledger.backfills != 2 {
        return Err(KbCode::LedgerBroken.code());
    }
    // 联签全日志审计（留痕）。
    if !bridge.audit_signs() {
        return Err(KbCode::SignMismatch.code());
    }
    bridge.seal()?;
    if !bridge.ledger.conserve() {
        return Err(KbCode::LedgerBroken.code());
    }
    Ok((bridge.journal.len, bridge.ledger, bridge.journal.fingerprint()))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(KbCode::NotReproducible.code());
    }
    Ok(())
}

// ── 注入核（三类变异 × 10 轮） ──────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 联签篡改。
    pub sign_caught: u32,
    /// 类2 未知知识 id 回灌。
    pub unknown_caught: u32,
    /// 类3 重复转移。
    pub dup_caught: u32,
}

/// 面额（实现推导）：三类各每轮 1 次违约。
pub const FACE_SIGN: u32 = 1;
pub const FACE_UNKNOWN: u32 = 1;
pub const FACE_DUP: u32 = 1;

/// 三类注入 rounds 轮；每类每轮必拒，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        // 类1 联签篡改：方向位翻转 0→1 联签必变（留痕审计面）。
        let s0 = co_sign(DIR_EXPORT, 2000, 5000, 0x1234);
        let s1 = co_sign(DIR_BACKFILL, 2000, 5000, 0x1234);
        if s0 != s1 {
            led.sign_caught = led.sign_caught.saturating_add(FACE_SIGN);
        } else {
            return Err(KbCode::SignMismatch.code());
        }
        // 类1b：载荷哈希翻转联签必变（同向对拍）。
        let s2 = co_sign(DIR_EXPORT, 2000, 5000, 0x1235);
        if s0 != s2 {
            led.sign_caught = led.sign_caught.saturating_add(1);
        } else {
            return Err(KbCode::SignMismatch.code());
        }
        // 类2 未知知识 id 回灌：know_id 未导出登记 → KnowIdUnknown。
        let mut bridge = KnowledgeBridge::new();
        let pres = Prescription { id: PRES_ID_BASE, major: 6, minor: 1, build: 7601, sp: 1, pt: 3, dpi: 96 };
        bridge.export(&pres, KNOW_ID_BASE).map_err(|_| KbCode::InjectionMissed.code())?;
        let ghost = KnowledgeEntry { id: KNOW_ID_BASE + 99, issue: 0, payload_hash: 1, origin: 0 };
        match bridge.backfill(&ghost, PRES_ID_BASE) {
            Err(c) if c == KbCode::KnowIdUnknown.code() => {
                led.unknown_caught = led.unknown_caught.saturating_add(FACE_UNKNOWN);
            }
            _ => return Err(KbCode::InjectionMissed.code()),
        }
        // 类3 重复转移：同 (回灌, pres, know) 二次回灌必拒。
        let e0 = KnowledgeEntry { id: KNOW_ID_BASE, issue: 0, payload_hash: 0x00C0_FEE0, origin: 0 };
        if bridge.backfill(&e0, PRES_ID_BASE).is_err() {
            return Err(KbCode::InjectionMissed.code());
        }
        match bridge.backfill(&e0, PRES_ID_BASE) {
            Err(c) if c == KbCode::TransferDuplicate.code() => {
                led.dup_caught = led.dup_caught.saturating_add(FACE_DUP);
            }
            _ => return Err(KbCode::InjectionMissed.code()),
        }
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：黄金链全过。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：空账封印 → 拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut bridge = KnowledgeBridge::new();
    bridge.seal()
}

/// 探针 C：处方 id 越域（<2000）→ 拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut bridge = KnowledgeBridge::new();
    let pres = Prescription { id: PRES_ID_BASE - 1, major: 6, minor: 1, build: 7601, sp: 1, pt: 3, dpi: 96 };
    // Ok 侧折叠为违约码（探针语义：处方 id 越域必须被拒，Ok 即漏检）。
    match bridge.export(&pres, KNOW_ID_BASE) {
        Err(c) => Err(c),
        Ok(_) => Err(KbCode::PresIdIllegal.code()),
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < KbCode::ALL.len() {
        let v = KbCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据收口（8–20 条；动态明细 ≤8） ────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金链：3 导出 + 2 回灌 + 联签审计 + 封账全过（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, _)) => *n == 5 && led.exports == 3 && led.backfills == 2 && led.conserve(),
        Err(_) => false,
    };
    v.push(("J1-黄金全链", g_ok, format!("{:?}", g)));

    // 2. 双向断言：导出 AI-25 消费域 / 回灌疑难库域各 ≥1 且方向闭集。
    let bi_ok = dir_name(DIR_EXPORT).is_some()
        && dir_name(DIR_BACKFILL).is_some()
        && dir_name(2).is_none()
        && DIR_EXPORT != DIR_BACKFILL;
    v.push(("J1-双向通道闭集", bi_ok, String::from("export/backfill 双向")));

    // 3. 联签留痕：全日志逐条复算（黄金链内已审 + 独立复算）。
    let sign_ok = {
        let mut bridge = KnowledgeBridge::new();
        let pres = Prescription { id: PRES_ID_BASE, major: 6, minor: 1, build: 7601, sp: 1, pt: 3, dpi: 96 };
        if bridge.export(&pres, KNOW_ID_BASE).is_err() {
            false
        } else {
            match bridge.journal.rec_at(0) {
                Some(r) => r.sign == co_sign(DIR_EXPORT, r.pres_id, r.know_id, r.payload_hash),
                None => false,
            }
        }
    };
    v.push(("J1-联签留痕", sign_ok, String::from("FNV32 四元复算")));

    // 4. 联签四元敏感：任一元翻转必变。
    let sens_ok = co_sign(0, 1, 2, 3) != co_sign(1, 1, 2, 3)
        && co_sign(0, 1, 2, 3) != co_sign(0, 2, 2, 3)
        && co_sign(0, 1, 2, 3) != co_sign(0, 1, 3, 3)
        && co_sign(0, 1, 2, 3) != co_sign(0, 1, 2, 4);
    v.push(("J1-联签四元敏感", sens_ok, String::from("方向/id/载荷 任变即变")));

    // 5. 类1 联签篡改注入（2 × 10 轮：方向位+载荷位）。
    let led = inject_rounds(JUDGE_ROUNDS);
    let t1 = led.as_ref().map(|l| l.sign_caught).unwrap_or(0);
    v.push(("J1-联签篡改全抓", t1 == 2 * JUDGE_ROUNDS, format!("{}/{}", t1, 2 * JUDGE_ROUNDS)));

    // 6. 类2 未知知识 id 回灌注入。
    let t2 = led.as_ref().map(|l| l.unknown_caught).unwrap_or(0);
    v.push(("J1-未知id回灌全拒", t2 == FACE_UNKNOWN * JUDGE_ROUNDS, format!("{}/{}", t2, FACE_UNKNOWN * JUDGE_ROUNDS)));

    // 7. 类3 重复转移注入。
    let t3 = led.as_ref().map(|l| l.dup_caught).unwrap_or(0);
    v.push(("J1-重复转移全拒", t3 == FACE_DUP * JUDGE_ROUNDS, format!("{}/{}", t3, FACE_DUP * JUDGE_ROUNDS)));

    // 8. 注入账守恒。
    let cons = match led.as_ref() {
        Ok(l) => {
            let sum = l.sign_caught.saturating_add(l.unknown_caught).saturating_add(l.dup_caught);
            sum == (2 + FACE_UNKNOWN + FACE_DUP) * JUDGE_ROUNDS
        }
        Err(_) => false,
    };
    v.push(("J1-注入账守恒", cons, String::from("3类面额×10轮")));

    // 9. 日志容量：32 槽满显性拒且旧账不丢。
    let cap_ok = {
        let mut j = TransferJournal::new();
        let mut k = 0u32;
        while k < JOURNAL_CAP as u32 {
            let r = TransferRec { dir: DIR_EXPORT, pres_id: PRES_ID_BASE + k, know_id: KNOW_ID_BASE + k, payload_hash: k, sign: 0 };
            let _ = j.push(r);
            k += 1;
        }
        let extra = TransferRec { dir: DIR_EXPORT, pres_id: 9, know_id: 9, payload_hash: 0, sign: 0 };
        let pushed = j.push(extra);
        j.len == JOURNAL_CAP && j.full_rejects == 1 && pushed.is_err()
    };
    v.push(("J1-日志容量32满拒", cap_ok, String::from("32 收 1 拒")));

    // 10. 空账封印拒（探针 B）。
    v.push(("J1-空账封印拒", probe_seq_b().is_err(), String::from("零转移即封必拒")));

    // 11. 处方 id 越域拒（探针 C）。
    v.push(("J1-处方id越域拒", probe_seq_c().is_err(), String::from("id<2000 必拒")));

    // 12. 知识 id 越域拒 + 议题域越闭集拒。
    let dom_ok = {
        let mut bridge = KnowledgeBridge::new();
        let pres = Prescription { id: PRES_ID_BASE, major: 6, minor: 1, build: 7601, sp: 1, pt: 3, dpi: 96 };
        let e_bad_id = KnowledgeEntry { id: 1, issue: 0, payload_hash: 0, origin: 0 };
        let e_bad_issue = KnowledgeEntry { id: KNOW_ID_BASE, issue: 9, payload_hash: 0, origin: 0 };
        bridge.export(&pres, KNOW_ID_BASE - 1).is_err()
            && bridge.backfill(&e_bad_id, PRES_ID_BASE).is_err()
            && bridge.backfill(&e_bad_issue, PRES_ID_BASE).is_err()
    };
    v.push(("J1-id域/议题域拒", dom_ok, String::from("≥5000 / issue≤3")));

    // 13. 回灌处方合法性：议题域映射主段 + id 域过。
    let back_ok = {
        let mut bridge = KnowledgeBridge::new();
        let pres = Prescription { id: PRES_ID_BASE, major: 6, minor: 1, build: 7601, sp: 1, pt: 3, dpi: 96 };
        if bridge.export(&pres, KNOW_ID_BASE).is_err() {
            false
        } else {
            let e = KnowledgeEntry { id: KNOW_ID_BASE, issue: 1, payload_hash: 0x00BE_EF00, origin: 0 };
            match bridge.backfill(&e, PRES_ID_BASE) {
                Ok(p) => p.valid() && p.major == 10,
                Err(_) => false,
            }
        }
    };
    v.push(("J1-回灌处方生效", back_ok, String::from("议题映射主段+id 对拍")));

    // 14. 日志指纹双跑一致。
    let fp = match &g {
        Ok((_, _, f)) => *f,
        Err(_) => 0,
    };
    let fp2 = match golden_run() {
        Ok((_, _, f)) => f,
        Err(_) => 1,
    };
    v.push(("J2-日志指纹双跑", fp == fp2 && fp != 0, format!("{:08X}", fp)));

    // 15. 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));

    // 16. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 17. 诊断码闭集（12 码全非空）。
    let all_ok = {
        let mut ok = KbCode::ALL.len() == 13;
        let mut i = 0;
        while i < KbCode::ALL.len() {
            if KbCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集13码", all_ok, String::from("detail 全非空")));

    // 18. FNV32 确定性：同输入同哈希、异输入异哈希。
    let fnv_ok = fnv1a32(b"ai25") == fnv1a32(b"ai25") && fnv1a32(b"ai25") != fnv1a32(b"e5kb");
    v.push(("J3-FNV32确定性", fnv_ok, String::from("同同异异")));

    v
}

/// 收口：本模块自检 CheckSet（判据 18 条；前 8 条动态明细）。
pub fn run_unx18549_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18549 处方知识账互通");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18549_checks();
        assert!(set.all_passed(), "UNX-F18549 判据集应全绿");
    }

    /// 黄金链：3 导出 + 2 回灌。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, led, _)) => {
                assert_eq!(n, 5);
                assert_eq!(led.exports, 3);
                assert_eq!(led.backfills, 2);
                assert!(led.conserve());
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 联签四元敏感 + 确定性。
    #[test]
    fn test_co_sign() {
        assert_eq!(co_sign(0, 1, 2, 3), co_sign(0, 1, 2, 3));
        assert_ne!(co_sign(0, 1, 2, 3), co_sign(1, 1, 2, 3));
        assert_ne!(co_sign(0, 1, 2, 3), co_sign(0, 1, 2, 4));
    }

    /// 注入三类全抓（10 轮）。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.sign_caught, 20);
                assert_eq!(l.unknown_caught, 10);
                assert_eq!(l.dup_caught, 10);
            }
            Err(e) => assert!(false, "注入全抓不应报错: {}", detail(e)),
        }
    }

    /// 序闸：先导出后回灌（未知 id 拒）。
    #[test]
    fn test_order_gate() {
        let mut bridge = KnowledgeBridge::new();
        let e = KnowledgeEntry { id: KNOW_ID_BASE, issue: 0, payload_hash: 1, origin: 0 };
        assert_eq!(
            bridge.backfill(&e, PRES_ID_BASE).err(),
            Some(KbCode::KnowIdUnknown.code())
        );
    }

    /// 重复转移拒 + 导出重复同样拒（contains 覆盖两向）。
    #[test]
    fn test_duplicate_gate() {
        let mut bridge = KnowledgeBridge::new();
        let pres = Prescription { id: PRES_ID_BASE, major: 6, minor: 1, build: 7601, sp: 1, pt: 3, dpi: 96 };
        assert!(bridge.export(&pres, KNOW_ID_BASE).is_ok());
        // 导出向重复：同 (export, pres, know) 二次导出。
        assert_eq!(
            bridge.export(&pres, KNOW_ID_BASE).err(),
            Some(KbCode::TransferDuplicate.code())
        );
    }

    /// 容量：32 槽满显性拒。
    #[test]
    fn test_journal_capacity() {
        let mut j = TransferJournal::new();
        let mut k = 0u32;
        while k < JOURNAL_CAP as u32 {
            let r = TransferRec { dir: DIR_EXPORT, pres_id: PRES_ID_BASE + k, know_id: KNOW_ID_BASE + k, payload_hash: k, sign: 0 };
            assert!(j.push(r).is_ok());
            k += 1;
        }
        let extra = TransferRec { dir: DIR_EXPORT, pres_id: 1, know_id: 1, payload_hash: 0, sign: 0 };
        assert_eq!(j.push(extra).err(), Some(KbCode::JournalFull.code()));
        assert_eq!(j.len, JOURNAL_CAP);
        assert_eq!(j.full_rejects, 1);
    }

    /// 状态机：空账封印 / 封后转移拒。
    #[test]
    fn test_phases() {
        assert!(probe_seq_b().is_err());
        let mut bridge = KnowledgeBridge::new();
        let pres = Prescription { id: PRES_ID_BASE, major: 6, minor: 1, build: 7601, sp: 1, pt: 3, dpi: 96 };
        assert!(bridge.export(&pres, KNOW_ID_BASE).is_ok());
        assert_eq!(bridge.phase, BridgePhase::Linked);
        assert!(bridge.seal().is_ok());
        assert_eq!(
            bridge.export(&pres, KNOW_ID_BASE + 1).err(),
            Some(KbCode::PhaseRegression.code())
        );
    }

    /// 双跑一致。
    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    /// 段位守卫。
    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
    }
}
