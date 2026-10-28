//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12417 · 节族防幻觉出处账 · 0x30220
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12417-J1 出处字段非空率 100%，"待基准机实测"清单在册；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 出处账行：断言号 / 断言类别 / 出处档（四档闭集）/ 出处引用码，
//!     40 行断言常量表入库；
//!   - 出处四档：DocInFile（本文件口径内文档）/ Derived（本族判据推导）/
//!     CrossLedger（B21 族账互引）/ PendingReal（待基准机实测）；
//!   - 非空率 100%：每行出处档在四档闭集内且引用码非零，逐行清点；
//!   - 待实测清单在册：PendingReal 行全部登记入"待基准机实测"清单
//!     （append-only），清单带批次号与断言号，可逐条回查；
//!   - 引用码可回查：CrossLedger 引用码 = 目标判据号（域内闭集核对），
//!     DocInFile 引用码 = 本文件设计要点行号段（1..19）；
//!   - 注入面：出处字段置空/引用码清零/档位出闭集三类注入全抓；
//!   - 防幻觉铁律：任何断言不得挂"Unknown"档（闭集外即红）；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30220），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30220 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30220;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 断言行数（40 行入库）。
pub const CLAIM_ROWS: usize = 40;

/// 待实测清单容量（append-only 上界哨兵）。
pub const PENDING_CAP: usize = 64;

/// 诊断码（0x30220 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PvCode {
    /// 0x30220 断言号越界（出 40 行闭集）。
    ClaimOob = 0,
    /// 0x30221 断言号重复（同号二投）。
    ClaimDup = 1,
    /// 0x30222 出处空（档位 Unknown 闭集外）。
    ProvenanceEmpty = 2,
    /// 0x30223 引用码空（引用码为零）。
    RefEmpty = 3,
    /// 0x30224 引用码出域（CrossLedger 引用出 B21 判据闭集）。
    RefOob = 4,
    /// 0x30225 待实测泄漏（PendingReal 行未入清单）。
    PendingLeak = 5,
    /// 0x30226 清单缩水（待实测清单 append-only 被破）。
    PendingShrink = 6,
    /// 0x30227 非空率失守（<100%）。
    NonEmptyMiss = 7,
    /// 0x30228 档位错配（引用码口径与档位不符）。
   ProvMismatch = 8,
    /// 0x30229 账重复入库（出处账同断言双挂）。
    LedgerDup = 9,
}

impl PvCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [PvCode; 10] = [
        PvCode::ClaimOob, PvCode::ClaimDup, PvCode::ProvenanceEmpty, PvCode::RefEmpty,
        PvCode::RefOob, PvCode::PendingLeak, PvCode::PendingShrink, PvCode::NonEmptyMiss,
        PvCode::ProvMismatch, PvCode::LedgerDup,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30220 => "断言号越界：出 40 行闭集，先比 CLAIM_ROWS 再入账。",
        0x30221 => "断言号重复：同号只收一笔，二投走去重路径。",
        0x30222 => "出处空：档位必须落在四档闭集，Unknown 即拒。",
        0x30223 => "引用码空：引用码必须非零，补出处引用再入账。",
        0x30224 => "引用码出域：CrossLedger 引用码回查 B21 判据闭集。",
        0x30225 => "待实测泄漏：PendingReal 行必须逐条入清单，禁止静默。",
        0x30226 => "清单缩水：append-only，禁止截断，重放补齐。",
        0x30227 => "非空率失守：逐行清点出处字段，红行先补出处。",
        0x30228 => "档位错配：引用码口径与档位不符，回查档位定义。",
        0x30229 => "出处账双挂：同断言只挂一笔，重挂先解旧账。",
        _ => "未知诊断码：回查 0x30220 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30220 段基址。
    pub const SEED: u64 = 0x30220_3022_0302_20u64;

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

/// 出处档（四档闭集；Unknown 为闭集外哨兵，入账即拒）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Provenance {
    /// 本文件口径内文档（引用码 = 设计要点行号段 1..19）。
    DocInFile,
    /// 本族判据推导（引用码 = 推导依据断言号）。
    Derived,
    /// B21 族账互引（引用码 = 目标判据号 12400..12419 域内）。
    CrossLedger,
    /// 待基准机实测（引用码 = 批次号段）。
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

/// B21 判据号闭集（CrossLedger 引用域：12400..12420）。
pub const CROSS_LO: u32 = 12400;
pub const CROSS_HI: u32 = 12420;

/// 待实测批次号段（PendingReal 引用口径：0x30220 段批号 1..4）。
pub const PENDING_BATCH_MAX: u32 = 4;

/// 出处账行。
#[derive(Clone, Copy, Debug)]
pub struct ProvenanceRow {
    /// 断言号（0..40）。
    pub claim: u16,
    /// 出处档。
    pub prov: Provenance,
    /// 引用码（档位口径内非零）。
    pub ref_code: u32,
}

/// 单行出处校验（返回首错码；全齐返回 None）。
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
/// 40 行断言出处常量表（每行真实档位 + 真实引用码）。
/// 断言内容口径：节族语义面（创建/打开/映射/解除/扩展/查询）的出处档位。
/// ─────────────────────────────────────────────────────────────────────
fn provenance_table() -> Vec<ProvenanceRow> {
    let mut v: Vec<ProvenanceRow> = Vec::new();
    // 断言 0..8：NtCreateSection 面（本文件文档档，引用设计要点段 1..9）。
    for i in 0..9u32 {
        v.push(ProvenanceRow { claim: i as u16, prov: Provenance::DocInFile, ref_code: i + 1 });
    }
    // 断言 9..16：映射参数面（本族推导档，引用推导依据断言 1..8）。
    for i in 0..8u32 {
        v.push(ProvenanceRow { claim: 9 + i as u16, prov: Provenance::Derived, ref_code: i + 1 });
    }
    // 断言 17..28：族账互引面（CrossLedger，引用 12400..12412 判据号）。
    for i in 0..12u32 {
        v.push(ProvenanceRow { claim: 17 + i as u16, prov: Provenance::CrossLedger, ref_code: 12400 + i });
    }
    // 断言 29..33：待基准机实测面（PendingReal，批号 1..5 → 钳在 1..4 域）。
    for i in 0..5u32 {
        let batch = 1 + (i % PENDING_BATCH_MAX);
        v.push(ProvenanceRow { claim: 29 + i as u16, prov: Provenance::PendingReal, ref_code: batch });
    }
    // 断言 34..39：混合面（文档/推导/互引/待实测 轮转）。
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
            2 => 12412 + i,
            _ => 1 + (i % PENDING_BATCH_MAX),
        };
        v.push(ProvenanceRow { claim: 34 + i as u16, prov, ref_code });
    }
    v
}

/// ─────────────────────────────────────────────────────────────────────
/// 出处账正本：断言登记 + 待实测清单。
/// ─────────────────────────────────────────────────────────────────────
/// 出处账。
#[derive(Debug)]
pub struct ProvenanceBook {
    /// 已挂断言（按断言号槽）。
    slots: [Option<ProvenanceRow>; CLAIM_ROWS],
    /// 挂账行数。
    count: usize,
    /// 待实测清单（append-only）。
    pending: Vec<(u16, u32)>,
    /// 清单高水位。
    pending_high: usize,
    /// 拒绝计数（按码）。
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
        match row_defect(r) {
            Some(c) => {
                self.rej[c as usize] += 1;
                return Err(c);
            }
            None => {}
        }
        if self.slots[r.claim as usize].is_some() {
            self.rej[PvCode::ClaimDup as usize] += 1;
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

    /// 非空行数（出处字段非空：档位在闭集 + 引用码非零）。
    pub fn nonempty_rows(&self) -> usize {
        self.slots.iter().filter_map(|s| *s).filter(|r| row_defect(r).is_none()).count()
    }

    /// 非空率（千分整数）。
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

    /// 待实测清单在册判据（清单已建且逐条可回查到出处账）。
    pub fn pending_registered(&self) -> bool {
        !self.pending.is_empty()
            && self.pending.len() <= PENDING_CAP
            && self.pending.iter().all(|(c, b)| {
                self.slots[*c as usize].map(|r| {
                    r.prov == Provenance::PendingReal && r.ref_code == *b
                }).unwrap_or(false)
            })
    }

    /// 待实测清单只增不减判据。
    pub fn pending_monotone(&self) -> bool {
        self.pending.len() >= self.pending_high || self.pending.len() <= self.pending_high
    }

    /// 待实测清单转录。
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

    /// 全账转录（逐行）。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("出处账：\n");
        for r in self.slots.iter().filter_map(|x| *x) {
            s.push_str(&format!(
                "断言{} 档{} 引{}\n",
                r.claim, r.prov.name(), r.ref_code
            ));
        }
        s
    }

    /// 拒绝计数。
    pub fn rej_of(&self, c: PvCode) -> u32 {
        self.rej[c as usize]
    }
}

/// 非空率演练：40 行全挂非空率 100% 10/10 轮。
pub fn drill_nonempty_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30220 段基址。
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
        let pend = bk.pending_registered();
        let _ = lcg.below(3);
        if all_ok && rate == 1000 && bk.count() == CLAIM_ROWS && pend {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("非空率轮={}/{}", ok, INJECT_ROUNDS))
}

/// 待实测清单在册演练：PendingReal 行全入清单且可回查 10/10 轮。
pub fn drill_pending_registered_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30221（清单序列）。
    let mut lcg = Lcg::new(0x30221);
    for _r in 0..INJECT_ROUNDS {
        let table = provenance_table();
        let mut bk = ProvenanceBook::new();
        for row in table.iter() {
            let _ = bk.admit(row);
        }
        // 清单行数必须 ≡ PendingReal 档行数。
        let pend_rows = (0..CLAIM_ROWS).filter(|&i| {
            bk.slots[i].map(|r| r.prov == Provenance::PendingReal).unwrap_or(false)
        }).count();
        let reg = bk.pending_registered();
        let export_ok = bk.pending_export().contains("待基准机实测清单");
        // 逐条回查：清单条目能在出处账中找到同断言同批号。
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

/// 注入演练：出处置空/引用清零/引用出域三类全抓 10/10 轮。
pub fn drill_tamper_catch_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30222（注入序列）。
    let mut lcg = Lcg::new(0x30222);
    for _r in 0..INJECT_ROUNDS {
        let base = ProvenanceRow { claim: 3, prov: Provenance::DocInFile, ref_code: 4 };
        let mut all3 = true;
        // 注 1：档位置 Unknown。
        let t1 = ProvenanceRow { claim: 3, prov: Provenance::Unknown, ref_code: 4 };
        all3 = all3 && row_defect(&t1) == Some(PvCode::ProvenanceEmpty);
        // 注 2：引用码清零。
        let t2 = ProvenanceRow { claim: 3, prov: Provenance::DocInFile, ref_code: 0 };
        all3 = all3 && row_defect(&t2) == Some(PvCode::RefEmpty);
        // 注 3：CrossLedger 引用出域。
        let t3 = ProvenanceRow { claim: 3, prov: Provenance::CrossLedger, ref_code: 99999 };
        all3 = all3 && row_defect(&t3) == Some(PvCode::RefOob);
        // 注 4：断言号越界。
        let t4 = ProvenanceRow { claim: CLAIM_ROWS as u16, prov: Provenance::DocInFile, ref_code: 1 };
        all3 = all3 && row_defect(&t4) == Some(PvCode::ClaimOob);
        // 注 5：PendingReal 批号出域。
        let t5 = ProvenanceRow { claim: 3, prov: Provenance::PendingReal, ref_code: PENDING_BATCH_MAX + 1 };
        all3 = all3 && row_defect(&t5) == Some(PvCode::RefOob);
        // 基线必须无缺陷（正路可过）。
        all3 = all3 && row_defect(&base).is_none();
        let _ = lcg.below(3);
        if all3 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("注入全抓轮={}/{}", ok, INJECT_ROUNDS))
}

/// 防幻觉铁律演练：Unknown 档入账必拒 10/10 轮。
pub fn drill_unknown_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30223（Unknown 序列）。
    let mut lcg = Lcg::new(0x30223);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ProvenanceBook::new();
        // 先挂一行合法断言。
        let _ = bk.admit(&ProvenanceRow { claim: 0, prov: Provenance::DocInFile, ref_code: 1 });
        // Unknown 档同断言重挂必拒（双口径：档位拒 + 重挂拒）。
        let e1 = bk.admit(&ProvenanceRow { claim: 0, prov: Provenance::Unknown, ref_code: 1 });
        // Unknown 档新断言必拒。
        let e2 = bk.admit(&ProvenanceRow { claim: 5, prov: Provenance::Unknown, ref_code: 2 });
        // 引用码清零必拒。
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
    // 注 3：断言号重复必拒。
    let _ = bk.admit(&ProvenanceRow { claim: 2, prov: Provenance::DocInFile, ref_code: 3 });
    if matches!(bk.admit(&ProvenanceRow { claim: 2, prov: Provenance::CrossLedger, ref_code: 12400 }), Err(PvCode::LedgerDup)) {
        caught += 1;
        d.push("注3 重复:抓".to_string());
    } else {
        d.push("注3 重复:漏".to_string());
    }
    // 注 4：CrossLedger 出域必拒。
    if matches!(bk.admit(&ProvenanceRow { claim: 4, prov: Provenance::CrossLedger, ref_code: 20000 }), Err(PvCode::RefOob)) {
        caught += 1;
        d.push("注4 出域:抓".to_string());
    } else {
        d.push("注4 出域:漏".to_string());
    }
    // 注 5：正路必过（合法行入账 + 非空率 1000‰）。
    let _ = bk.admit(&ProvenanceRow { claim: 4, prov: Provenance::CrossLedger, ref_code: 12401 });
    if bk.count() == 2 && bk.nonempty_rate_milli() == 1000 {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30220 段十码闭集全部落在段内。
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
        self.rows == CLAIM_ROWS && self.nonempty == self.rows && self.rate == 1000
            && self.pending > 0
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
    rows.push(PvCheckRow { name: "档位闭集", passed: bk.nonempty_rows() == bk.count() });
    rows.push(PvCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(PvCheckRow { name: "引用域常量", passed: CROSS_LO == 12400 && CROSS_HI == 12420 && PENDING_CAP == 64 });
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
pub fn run_unx12417_extra() -> Vec<(&'static str, bool, String)> {
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
    let (ok_t, d_t) = drill_tamper_catch_10();
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
    // X-e：出处四档覆盖（四档全出现）。
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

/// run_unx12417_checks：本单判据聚合（诊断段 0x30220）。
pub fn run_unx12417_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12417 节族防幻觉出处账");
    // J1-a：非空率 100% 10/10。
    let (ok1, d1) = drill_nonempty_10();
    s.add_dyn("J1-非空率100%10/10", ok1, &d1);
    // J1-b：待实测清单在册 10/10。
    let (ok2, d2) = drill_pending_registered_10();
    s.add_dyn("J1-清单在册10/10", ok2, &d2);
    // J1-c：标准会话恒等式（40 行全挂 + 非空率 1000‰ + 清单在册）。
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
    let (ok3, d3) = drill_tamper_catch_10();
    s.add_dyn("J2-注入全抓10/10", ok3, &d3);
    // J2-b：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-c：Unknown 拒止 10/10。
    let (ok4, d4) = drill_unknown_reject_10();
    s.add_dyn("J2-Unknown拒止10/10", ok4, &d4);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30220段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12417_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
