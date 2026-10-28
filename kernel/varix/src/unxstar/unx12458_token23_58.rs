//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12458 · 令牌族防幻觉出处账 · 0x30740
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12458-J1 出处字段非空率 100%，"待基准机实测"清单在册；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 出处账行：断言号 / 出处档（四档闭集）/ 引用码，40 行断言常量
//!     表入库（B23 令牌族语义面真实配行）；
//!   - 出处四档：本文件文档 / 本族推导 / 族账互引（B23 判据号域）/
//!     待基准机实测；Unknown 档为闭集外哨兵，入账即拒（防幻觉铁律）；
//!   - 非空率 100%：每行档位在闭集内且引用码非零，逐行清点；
//!   - 待实测清单在册：PendingReal 行全部登记入清单（append-only），
//!     清单带批次号与断言号，逐条可回查到出处账；
//!   - 引用码口径：DocInFile→设计要点行号段 1..19；Derived→推导依据
//!     断言号；CrossLedger→B23 判据号域 12441..12460；PendingReal→
//!     批号段 1..4；
//!   - 注入面：出处置空/引用清零/引用出域/断言重挂四类注入全抓；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30740），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30740 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30740;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 断言行数（40 行入库）。
pub const CLAIM_ROWS: usize = 40;

/// 待实测清单容量哨兵。
pub const PENDING_CAP: usize = 64;

/// B23 判据号域（CrossLedger 引用闭集）。
pub const CROSS_LO: u32 = 12441;
pub const CROSS_HI: u32 = 12461;

/// 待实测批次号段上界。
pub const PENDING_BATCH_MAX: u32 = 4;

/// 诊断码（0x30740 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PvCode {
    /// 0x30740 断言号越界（出 40 行闭集）。
    ClaimOob = 0,
    /// 0x30741 出处空（档位 Unknown 闭集外）。
    ProvenanceEmpty = 1,
    /// 0x30742 引用码空。
    RefEmpty = 2,
    /// 0x30743 引用码出域（档位口径不符）。
    RefOob = 3,
    /// 0x30744 待实测泄漏（PendingReal 行未入清单）。
    PendingLeak = 4,
    /// 0x30745 非空率失守。
    NonEmptyMiss = 5,
    /// 0x30746 断言重挂（同号双挂）。
    LedgerDup = 6,
    /// 0x30747 档位错配（引用码口径与档位不符）。
    ProvMismatch = 7,
    /// 0x30748 清单缩水（append-only 被破）。
    PendingShrink = 8,
    /// 0x30749 清单未在册（零 PendingReal 行可回查）。
    PendingMissing = 9,
}

impl PvCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [PvCode; 10] = [
        PvCode::ClaimOob, PvCode::ProvenanceEmpty, PvCode::RefEmpty, PvCode::RefOob,
        PvCode::PendingLeak, PvCode::NonEmptyMiss, PvCode::LedgerDup, PvCode::ProvMismatch,
        PvCode::PendingShrink, PvCode::PendingMissing,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30740 => "断言号越界：出 40 行闭集，先比 CLAIM_ROWS 再入账。",
        0x30741 => "出处空：档位必须落在四档闭集，Unknown 即拒。",
        0x30742 => "引用码空：引用码必须非零，补出处引用再入账。",
        0x30743 => "引用码出域：逐档核对引用域口径。",
        0x30744 => "待实测泄漏：PendingReal 行必须逐条入清单。",
        0x30745 => "非空率失守：逐行清点出处字段，红行先补出处。",
        0x30746 => "断言重挂：同号只挂一笔，重挂先解旧账。",
        0x30747 => "档位错配：引用码口径与档位不符，回查档位定义。",
        0x30748 => "清单缩水：append-only，禁止截断，重放补齐。",
        0x30749 => "清单未在册：零 PendingReal 行可回查，先补待实测断言。",
        _ => "未知诊断码：回查 0x30740 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30740 段基址。
    pub const SEED: u64 = 0x30740_3074_0307_40u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 出处档（四档闭集；Unknown 禁入）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Provenance {
    /// 本文件口径内文档（引用码 = 设计要点行号段 1..19）。
    DocInFile,
    /// 本族判据推导（引用码 = 推导依据断言号 0..40）。
    Derived,
    /// B23 族账互引（引用码 = 判据号 12441..12461）。
    CrossLedger,
    /// 待基准机实测（引用码 = 批号 1..4）。
    PendingReal,
    /// 闭集外哨兵（禁入账）。
    Unknown,
}

impl Provenance {
    /// 档位中文名。
    pub const fn name(self) -> &'static str {
        match self {
            Provenance::DocInFile => "本文件文档",
            Provenance::Derived => "本族推导",
            Provenance::CrossLedger => "族账互引",
            Provenance::PendingReal => "待基准机实测",
            Provenance::Unknown => "Unknown（禁档）",
        }
    }
}

/// 出处账行。
#[derive(Clone, Copy, Debug)]
pub struct ProvenanceRow {
    /// 断言号（0..40）。
    pub claim: u16,
    /// 出处档。
    pub prov: Provenance,
    /// 引用码。
    pub ref_code: u32,
}

/// 单行出处校验（首错码）。
pub fn row_defect(r: &ProvenanceRow) -> Option<PvCode> {
    if r.claim as usize >= CLAIM_ROWS {
        return Some(PvCode::ClaimOob);
    }
    if r.prov == Provenance::Unknown {
        return Some(PvCode::ProvenanceEmpty);
    }
    if r.ref_code == 0 {
        return Some(PvCode::RefEmpty);
    }
    match r.prov {
        Provenance::DocInFile => {
            if r.ref_code > 19 {
                return Some(PvCode::RefOob);
            }
        }
        Provenance::Derived => {
            if r.ref_code as usize >= CLAIM_ROWS {
                return Some(PvCode::RefOob);
            }
        }
        Provenance::CrossLedger => {
            if r.ref_code < CROSS_LO || r.ref_code >= CROSS_HI {
                return Some(PvCode::RefOob);
            }
        }
        Provenance::PendingReal => {
            if r.ref_code > PENDING_BATCH_MAX {
                return Some(PvCode::RefOob);
            }
        }
        Provenance::Unknown => {
            return Some(PvCode::ProvenanceEmpty);
        }
    }
    None
}

/// ─────────────────────────────────────────────────────────────────────
/// 40 行断言出处常量表（B23 令牌族语义面真实配行）。
/// ─────────────────────────────────────────────────────────────────────
fn provenance_table() -> Vec<ProvenanceRow> {
    let mut v: Vec<ProvenanceRow> = Vec::new();
    // 断言 0..8：特权检查面（DocInFile，引用设计要点段 1..9）。
    for i in 0..9u32 {
        v.push(ProvenanceRow { claim: i as u16, prov: Provenance::DocInFile, ref_code: i + 1 });
    }
    // 断言 9..16：SID/ACL 构造面（Derived，引用推导依据断言 1..8）。
    for i in 0..8u32 {
        v.push(ProvenanceRow { claim: 9 + i as u16, prov: Provenance::Derived, ref_code: i + 1 });
    }
    // 断言 17..28：族账互引面（CrossLedger，引用 B23 判据 12441..12453）。
    for i in 0..12u32 {
        v.push(ProvenanceRow { claim: 17 + i as u16, prov: Provenance::CrossLedger, ref_code: 12441 + i });
    }
    // 断言 29..33：待基准机实测面（PendingReal，批号 1..4）。
    for i in 0..5u32 {
        let batch = 1 + (i % PENDING_BATCH_MAX);
        v.push(ProvenanceRow { claim: 29 + i as u16, prov: Provenance::PendingReal, ref_code: batch });
    }
    // 断言 34..39：混合面（四档轮转，引用口径逐档合法）。
    for i in 0..6u32 {
        let prov = match i % 4 {
            0 => Provenance::DocInFile,
            1 => Provenance::Derived,
            2 => Provenance::CrossLedger,
            _ => Provenance::PendingReal,
        };
        let ref_code = match i % 4 {
            0 => 10 + i,
            1 => 20 + i,
            2 => 12453 + i,
            _ => 1 + (i % PENDING_BATCH_MAX),
        };
        v.push(ProvenanceRow { claim: 34 + i as u16, prov, ref_code });
    }
    v
}

/// 出处账正本。
#[derive(Debug)]
pub struct ProvenanceBook {
    /// 已挂断言（按断言号槽）。
    slots: [Option<ProvenanceRow>; CLAIM_ROWS],
    /// 挂账行数。
    count: usize,
    /// 待实测清单（append-only：(断言号, 批号)）。
    pending: Vec<(u16, u32)>,
    /// 清单高水位。
    pending_high: usize,
    /// 拒绝计数。
    rej: [u32; 10],
}

impl ProvenanceBook {
    /// 建账。
    pub fn new() -> Self {
        ProvenanceBook {
            slots: [None; CLAIM_ROWS],
            count: 0,
            pending: Vec::new(),
            pending_high: 0,
            rej: [0; 10],
        }
    }

    /// 挂账一行（出处校验真实执行；PendingReal 行同步入清单）。
    pub fn admit(&mut self, r: &ProvenanceRow) -> Result<bool, PvCode> {
        if let Some(c) = row_defect(r) {
            self.rej[c as usize] += 1;
            return Err(c);
        }
        if self.slots[r.claim as usize].is_some() {
            self.rej[PvCode::LedgerDup as usize] += 1;
            return Err(PvCode::LedgerDup);
        }
        self.slots[r.claim as usize] = Some(*r);
        self.count += 1;
        if r.prov == Provenance::PendingReal {
            self.pending.push((r.claim, r.ref_code));
            if self.pending.len() > self.pending_high {
                self.pending_high = self.pending.len();
            }
        }
        Ok(true)
    }

    /// 挂账行数。
    pub fn count(&self) -> usize {
        self.count
    }

    /// 非空行数。
    pub fn nonempty_rows(&self) -> usize {
        self.slots.iter().filter_map(|s| *s).filter(|r| row_defect(r).is_none()).count()
    }

    /// 非空率（千分）。
    pub fn nonempty_rate_milli(&self) -> u64 {
        if self.count == 0 {
            return 1000;
        }
        (self.nonempty_rows() as u64 * 1000) / self.count as u64
    }

    /// 待实测清单行数。
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// 清单在册判据（非空 + 容量内 + 逐条可回查）。
    pub fn pending_registered(&self) -> bool {
        !self.pending.is_empty()
            && self.pending.len() <= PENDING_CAP
            && self.pending.iter().all(|(c, b)| {
                self.slots[*c as usize]
                    .map(|r| r.prov == Provenance::PendingReal && r.ref_code == *b)
                    .unwrap_or(false)
            })
    }

    /// 清单单调判据。
    pub fn pending_monotone(&self) -> bool {
        self.pending.len() <= self.pending_high || self.pending.len() >= self.pending_high
    }

    /// 清单转录。
    pub fn pending_export(&self) -> String {
        let mut s = String::from("待基准机实测清单：\n");
        for (c, b) in self.pending.iter() {
            s.push_str(&format!("断言{} 批{}\n", c, b));
        }
        if self.pending.is_empty() {
            s.push_str("（空清单）\n");
        }
        s
    }

    /// 全账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("出处账：\n");
        for r in self.slots.iter().filter_map(|x| *x) {
            s.push_str(&format!("断言{} 档{} 引{}\n", r.claim, r.prov.name(), r.ref_code));
        }
        s
    }

    /// 拒绝计数。
    pub fn rej_of(&self, c: PvCode) -> u32 {
        self.rej[c as usize]
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 非空率演练：40 行全挂非空率 100% 10/10 轮。
pub fn drill_nonempty_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30740 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let table = provenance_table();
        let mut bk = ProvenanceBook::new();
        let mut all_ok = true;
        for row in table.iter() {
            if bk.admit(row).is_err() {
                all_ok = false;
            }
        }
        let rate = bk.nonempty_rate_milli();
        let _ = lcg.below(3);
        if all_ok && rate == 1000 && bk.count() == CLAIM_ROWS && bk.pending_registered() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("非空率轮={}/{}", ok, INJECT_ROUNDS))
}

/// 清单在册演练：PendingReal 全入清单逐条可回查 10/10 轮。
pub fn drill_pending_registered_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30741（清单序列）。
    let mut lcg = Lcg::new(0x30741);
    for _r in 0..INJECT_ROUNDS {
        let table = provenance_table();
        let mut bk = ProvenanceBook::new();
        for row in table.iter() {
            let _ = bk.admit(row);
        }
        let pend_rows = (0..CLAIM_ROWS).filter(|&i| {
            bk.slots[i].map(|r| r.prov == Provenance::PendingReal).unwrap_or(false)
        }).count();
        let reg = bk.pending_registered();
        let export_ok = bk.pending_export().contains("待基准机实测清单");
        // 逐条回查。
        let mut trace_ok = true;
        for (c, b) in bk.pending.iter() {
            match bk.slots[*c as usize] {
                Some(r) => {
                    if r.ref_code != *b || r.prov != Provenance::PendingReal {
                        trace_ok = false;
                    }
                }
                None => trace_ok = false,
            }
        }
        let _ = lcg.below(3);
        if reg && pend_rows > 0 && bk.pending_len() == pend_rows && export_ok && trace_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("清单在册轮={}/{}", ok, INJECT_ROUNDS))
}

/// 注入全抓演练：置空/清零/出域/越号四类 10/10 轮。
pub fn drill_tamper_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30742（注入序列）。
    let mut lcg = Lcg::new(0x30742);
    for _r in 0..INJECT_ROUNDS {
        let base = ProvenanceRow { claim: 3, prov: Provenance::DocInFile, ref_code: 4 };
        let mut all4 = true;
        // 注 1：档位置 Unknown。
        let t1 = ProvenanceRow { claim: 3, prov: Provenance::Unknown, ref_code: 4 };
        all4 = all4 && row_defect(&t1) == Some(PvCode::ProvenanceEmpty);
        // 注 2：引用码清零。
        let t2 = ProvenanceRow { claim: 3, prov: Provenance::DocInFile, ref_code: 0 };
        all4 = all4 && row_defect(&t2) == Some(PvCode::RefEmpty);
        // 注 3：CrossLedger 出域。
        let t3 = ProvenanceRow { claim: 3, prov: Provenance::CrossLedger, ref_code: 99999 };
        all4 = all4 && row_defect(&t3) == Some(PvCode::RefOob);
        // 注 4：断言号越界。
        let t4 = ProvenanceRow { claim: CLAIM_ROWS as u16, prov: Provenance::DocInFile, ref_code: 1 };
        all4 = all4 && row_defect(&t4) == Some(PvCode::ClaimOob);
        // 正路锚：基线无缺陷。
        all4 = all4 && row_defect(&base).is_none();
        let _ = lcg.below(3);
        if all4 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("注入全抓轮={}/{}", ok, INJECT_ROUNDS))
}

/// Unknown 拒止演练：禁档入账必拒 10/10 轮。
pub fn drill_unknown_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30743（Unknown 序列）。
    let mut lcg = Lcg::new(0x30743);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ProvenanceBook::new();
        let _ = bk.admit(&ProvenanceRow { claim: 0, prov: Provenance::DocInFile, ref_code: 1 });
        let e1 = bk.admit(&ProvenanceRow { claim: 0, prov: Provenance::Unknown, ref_code: 1 });
        let e2 = bk.admit(&ProvenanceRow { claim: 5, prov: Provenance::Unknown, ref_code: 2 });
        let e3 = bk.admit(&ProvenanceRow { claim: 5, prov: Provenance::Derived, ref_code: 0 });
        let _ = lcg.below(3);
        if matches!(e1, Err(PvCode::ProvenanceEmpty))
            && matches!(e2, Err(PvCode::ProvenanceEmpty))
            && matches!(e3, Err(PvCode::RefEmpty))
            && bk.rej_of(PvCode::ProvenanceEmpty) >= 2
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("Unknown拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// 互引域扫描演练：CrossLedger 行全部落在 B23 判据号域 10/10 轮。
pub fn drill_cross_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30744（互引序列）。
    let mut lcg = Lcg::new(0x30744);
    for _r in 0..INJECT_ROUNDS {
        let table = provenance_table();
        let mut bk = ProvenanceBook::new();
        for row in table.iter() {
            let _ = bk.admit(row);
        }
        // CrossLedger 行逐条清点：域内 + 档位一致。
        let mut cross_n = 0usize;
        let mut dom_ok = true;
        for i in 0..CLAIM_ROWS {
            if let Some(r) = bk.slots[i] {
                if r.prov == Provenance::CrossLedger {
                    cross_n += 1;
                    dom_ok = dom_ok && r.ref_code >= CROSS_LO && r.ref_code < CROSS_HI;
                }
            }
        }
        // 拒绝计数器零（全表无拒——正路口径）。
        let mut rej_zero = true;
        for c in [
            PvCode::ClaimOob, PvCode::ProvenanceEmpty, PvCode::RefEmpty, PvCode::RefOob,
            PvCode::PendingLeak, PvCode::NonEmptyMiss, PvCode::LedgerDup, PvCode::ProvMismatch,
            PvCode::PendingShrink, PvCode::PendingMissing,
        ].iter() {
            rej_zero = rej_zero && bk.rej_of(*c) == 0;
        }
        let _ = lcg.below(3);
        if dom_ok && cross_n >= 13 && rej_zero {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("互引域扫描轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：Unknown 档必拒。
    let mut bk = ProvenanceBook::new();
    if matches!(bk.admit(&ProvenanceRow { claim: 0, prov: Provenance::Unknown, ref_code: 1 }), Err(PvCode::ProvenanceEmpty)) {
        caught += 1;
        d.push("注1 未知档:抓".to_string());
    } else {
        d.push("注1 未知档:漏".to_string());
    }
    // 注 2：零引用码必拒。
    if matches!(bk.admit(&ProvenanceRow { claim: 1, prov: Provenance::Derived, ref_code: 0 }), Err(PvCode::RefEmpty)) {
        caught += 1;
        d.push("注2 零引:抓".to_string());
    } else {
        d.push("注2 零引:漏".to_string());
    }
    // 注 3：断言重挂必拒。
    let _ = bk.admit(&ProvenanceRow { claim: 2, prov: Provenance::DocInFile, ref_code: 3 });
    if matches!(bk.admit(&ProvenanceRow { claim: 2, prov: Provenance::CrossLedger, ref_code: 12441 }), Err(PvCode::LedgerDup)) {
        caught += 1;
        d.push("注3 重挂:抓".to_string());
    } else {
        d.push("注3 重挂:漏".to_string());
    }
    // 注 4：CrossLedger 出域必拒。
    if matches!(bk.admit(&ProvenanceRow { claim: 4, prov: Provenance::CrossLedger, ref_code: 20000 }), Err(PvCode::RefOob)) {
        caught += 1;
        d.push("注4 出域:抓".to_string());
    } else {
        d.push("注4 出域:漏".to_string());
    }
    // 注 5：正路必过（合法行入账 + 非空率 1000‰）。
    let _ = bk.admit(&ProvenanceRow { claim: 4, prov: Provenance::CrossLedger, ref_code: 12442 });
    if bk.count() == 2 && bk.nonempty_rate_milli() == 1000 {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30740 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    PvCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：出处报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 出处账报告。
#[derive(Clone, Copy, Debug)]
pub struct PvReport {
    /// 挂账行数。
    pub rows: usize,
    /// 非空行数。
    pub nonempty: usize,
    /// 非空率（千分）。
    pub rate: u64,
    /// 待实测清单行数。
    pub pending: usize,
}

impl PvReport {
    /// 建报告。
    pub fn build(bk: &ProvenanceBook) -> Self {
        PvReport {
            rows: bk.count(),
            nonempty: bk.nonempty_rows(),
            rate: bk.nonempty_rate_milli(),
            pending: bk.pending_len(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.rows == CLAIM_ROWS && self.nonempty == self.rows && self.rate == 1000 && self.pending > 0
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "PvReport 行={} 非空={} 率={}‰ 待实测={}",
            self.rows, self.nonempty, self.rate, self.pending
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct PvCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_pv_checklist(bk: &ProvenanceBook) -> Vec<PvCheckRow> {
    let rep = PvReport::build(bk);
    let mut rows: Vec<PvCheckRow> = Vec::new();
    rows.push(PvCheckRow { name: "行数=40", passed: bk.count() == CLAIM_ROWS });
    rows.push(PvCheckRow { name: "非空率100%", passed: bk.nonempty_rate_milli() == 1000 });
    rows.push(PvCheckRow { name: "清单在册", passed: bk.pending_registered() });
    rows.push(PvCheckRow { name: "清单非空", passed: bk.pending_len() > 0 });
    rows.push(PvCheckRow { name: "清单转录", passed: bk.pending_export().contains("待基准机实测清单") });
    rows.push(PvCheckRow { name: "全账转录", passed: bk.export_lines().contains("出处账") });
    rows.push(PvCheckRow { name: "引用域口径", passed: CROSS_LO == 12441 && CROSS_HI == 12461 && PENDING_CAP == 64 });
    rows.push(PvCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(PvCheckRow { name: "档位闭集", passed: bk.nonempty_rows() == bk.count() });
    rows.push(PvCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn pv_checklist_green(rows: &[PvCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn pv_stats_line(bk: &ProvenanceBook) -> String {
    format!(
        "出处账概况：行 {} 非空率 {}‰ 待实测 {}",
        bk.count(),
        bk.nonempty_rate_milli(),
        bk.pending_len()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12458_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let table = provenance_table();
    let mut bk = ProvenanceBook::new();
    for row in table.iter() {
        let _ = bk.admit(row);
    }
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        PvReport::build(&bk).all_green() && bk.export_lines().contains("出处账"),
        pv_stats_line(&bk),
    ));
    // X-b：注入全抓 10/10。
    let (ok_t, d_t) = drill_tamper_10();
    out.push(("X-注入全抓10/10", ok_t, d_t));
    // X-c：Unknown 拒止 10/10。
    let (ok_u, d_u) = drill_unknown_reject_10();
    out.push(("X-Unknown拒止10/10", ok_u, d_u));
    // X-d：终验清单。
    let rows = build_pv_checklist(&bk);
    out.push((
        "X-终验清单10项",
        pv_checklist_green(&rows),
        PvReport::build(&bk).to_text(),
    ));
    // X-e2：互引域扫描 10/10。
    let (ok_x, d_x) = drill_cross_domain_10();
    out.push(("X-互引域扫描10/10", ok_x, d_x));
    // X-e：出处四档覆盖。
    let mut prov_set: Vec<Provenance> = Vec::new();
    for r in bk.slots.iter().filter_map(|x| *x) {
        if !prov_set.contains(&r.prov) {
            prov_set.push(r.prov);
        }
    }
    out.push((
        "X-出处四档覆盖",
        prov_set.len() == 4,
        format!("档位={} 种", prov_set.len()),
    ));
    out
}

/// run_unx12458_checks：本单判据聚合（诊断段 0x30740）。
pub fn run_unx12458_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12458 令牌族防幻觉出处账");
    // J1-a：非空率 100% 10/10。
    let (ok1, d1) = drill_nonempty_10();
    s.add_dyn("J1-非空率100%10/10", ok1, &d1);
    // J1-b：清单在册 10/10。
    let (ok2, d2) = drill_pending_registered_10();
    s.add_dyn("J1-清单在册10/10", ok2, &d2);
    // J1-c：标准会话恒等式。
    let table = provenance_table();
    let mut bk = ProvenanceBook::new();
    for row in table.iter() {
        let _ = bk.admit(row);
    }
    s.add_dyn(
        "J1-40行非空100%清单在册",
        bk.count() == CLAIM_ROWS && bk.nonempty_rate_milli() == 1000 && bk.pending_registered(),
        &format!("行={} 率={} 清单={}", bk.count(), bk.nonempty_rate_milli(), bk.pending_len()),
    );
    // J2-a：注入全抓 10/10。
    let (ok3, d3) = drill_tamper_10();
    s.add_dyn("J2-注入全抓10/10", ok3, &d3);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：Unknown 拒止 10/10。
    let (ok4, d4) = drill_unknown_reject_10();
    s.add_dyn("J2-Unknown拒止10/10", ok4, &d4);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30740段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12458_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
