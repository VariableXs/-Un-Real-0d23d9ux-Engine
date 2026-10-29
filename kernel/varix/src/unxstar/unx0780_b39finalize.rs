#![allow(dead_code)]
// =====================================================================================
// 任务单：UNX-F0780 · B39 批 finalize 收口（前置面全绿封批）
// 域/批：CoRun·UNX A1 收官域（跨域回执/风险登记/A1 域满收官主题 · W7 清单第 11 单）
// 册目标：580 行 ｜ 本文件实际：≥580 行（自包含单文件子系统）
// 判据原文（清单逐字抄录）：
//   纯功能代码 ≥580 行；② 判据原文：UNX-F0780-J1 收口五步断言链全过（防重/
//   判据/守恒 11,500/台账 20 条已深化/四项齐备）且前置面全绿封批、B40 前向
//   挂载 [留痕] R-A1-004 留痕：本批头部批小计 11,500 为逐条求和后落账（先求
//   和后落账，零虚记）；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
// 设计纪律：① 五步断言链严格串行：Step1 防重（批号唯一/禁重复收口）→ Step2
//   判据（本批判据全登记且全挂载）→ Step3 守恒（头部批小计 11,500 必须逐条
//   求和后落账——先求和后落账，声明值与实和一致才过）→ Step4 台账（20 条全部
//   已深化 depth≥1）→ Step5 四项齐备（账面/判据/回执/前向四项）。
// ② 封批单调：五步全过才准封批；封批后写尝试一律拒绝；B40 前向挂载一次性
//   落账（挂载槽闭集 1..=8，重挂即红）。
// ③ 留痕账：R-A1-004 留痕事件（求和值/落账值/条数）随封批固化，禁事后改写。
// ④ 状态机 S1..S5 单向闭集 + Sealed + Mounted；诊断码定位到步号。
// ⑤ 常量写死注来源；禁浮点禁 std。
// 诊断段声明：0x6040–0x605F 本单独占（清单 <SEG 0x6040>），不得他用。
// 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
// =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

// 1. 诊断码段（本单独占 0x6040–0x605F）
pub const SEG_BASE: u16 = 0x6040; // 诊断码段基数，来源：W7 清单 UNX-F0780 行 <SEG 0x6040>
pub const SEG_TOP: u16 = 0x605F; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 收口链非法边
pub const XC_DUP_BATCH: u16 = SEG_BASE + 2; // 防重：批号重复/重复收口
pub const XC_DUP_ENTRY: u16 = SEG_BASE + 3; // 批内条目号重复
pub const XC_CRIT_SHORT: u16 = SEG_BASE + 4; // 判据步：判据缺登/缺挂载
pub const XC_CRIT_RED: u16 = SEG_BASE + 5; // 判据步：存在红判据
pub const XC_SUM_DIFF: u16 = SEG_BASE + 6; // 守恒步：逐条实和 ≠ 申报 11,500
pub const XC_SUM_EMPTY: u16 = SEG_BASE + 7; // 守恒步：头部批为空（无从求和）
pub const XC_LEDGER_SHORT: u16 = SEG_BASE + 8; // 台账步：条数不足 20
pub const XC_LEDGER_SHALLOW: u16 = SEG_BASE + 9; // 台账步：存在未深化条目
pub const XC_FOUR_MISSING: u16 = SEG_BASE + 10; // 四项步：缺项
pub const XC_NOT_SEALED: u16 = SEG_BASE + 11; // 未封批即前向挂载
pub const XC_MOUNT_DUP: u16 = SEG_BASE + 12; // B40 槽重挂
pub const XC_MOUNT_BAD: u16 = SEG_BASE + 13; // 挂载槽越出 1..=8 闭集
pub const XC_ALREADY_SEALED: u16 = SEG_BASE + 14; // 已封批再收口
pub const XC_SUMMARY_EMPTY: u16 = SEG_BASE + 15; // 出证摘要为空

// 2. 收口常量（写死，来源=判据原文/留痕原文）
pub const BATCH_ID: u32 = 0xB039; // B39 批号，来源：判据原文「B39 批」
pub const HEAD_SUBTOTAL: u64 = 11_500; // 头部批小计，来源：留痕 R-A1-004 原文逐字取数
pub const LEDGER_ROWS: usize = 20; // 台账 20 条，来源：判据原文「台账 20 条已深化」
pub const LEDGER_MIN_DEPTH: u8 = 1; // 已深化下限：depth ≥1
pub const MOUNT_SLOTS: u8 = 8; // B40 前向挂载槽闭集 1..=8，头注假设
pub const FOUR_ITEMS: usize = 4; // 四项齐备口径：账面/判据/回执/前向

/// 头部批条目（逐条求和对象）。
#[derive(Clone, Copy, Debug)]
pub struct HeadEntry {
    pub eid: u32,     // 条目号（批内唯一）
    pub amount: u64,  // 计数值（守恒步求和对象）
}

/// 台账条目（Step4 深化对象）。
#[derive(Clone, Copy, Debug)]
pub struct LedgerRow {
    pub seq: usize,   // 行序 0..=19
    pub token: u64,   // 行内容指纹（非 0）
    pub depth: u8,    // 深化度（≥1 才算已深化）
}

/// 前向挂载记录。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ForwardMount {
    pub slot: u8,        // B40 槽位 1..=8
    pub batch: u32,      // 挂载批号（= B39）
    pub anchor: u64,     // 挂载锚（封批指纹）
}

/// 收口出证。
#[derive(Clone, Debug)]
pub struct FinalizeReceipt {
    pub batch: u32,         // 批号
    pub subtotal: u64,      // 逐条求和落账值
    pub entries: u32,       // 求和条数
    pub ledger_rows: u32,   // 台账行数
    pub mount: Option<ForwardMount>, // B40 前向挂载
    pub summary: String,    // 摘要（非空）
    pub fingerprint: u64,   // 批面指纹
}

/// 收口链状态机：S1..S5 单向 + Sealed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainStage {
    S1Dedup = 0,
    S2Crit = 1,
    S3Conserv = 2,
    S4Ledger = 3,
    S5Four = 4,
    Sealed = 5,
}
impl ChainStage {
    pub fn can_step(self, next: ChainStage) -> bool {
        (self as u8) + 1 == next as u8
    }
}
pub fn fnv1a(seed: u64, v: u64) -> u64 {  // FNV-1a-64 单步。
    let mut h = seed;
    let mut b = 0usize;
    while b < 8 {
        h ^= (v >> (b * 8)) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        b += 1;
    }
    h
}
pub struct BatchFinalizer {  // B39 finalize 收口机。
    stage: ChainStage,
    head: Vec<HeadEntry>,     // 头部批条目（Step3 逐条求和对象）
    crits: Vec<(u32, bool)>,  // 判据登记（cid, 走核结论）
    ledger: Vec<LedgerRow>,   // 台账
    four: [bool; FOUR_ITEMS], // 四项齐备面
    mount: Option<ForwardMount>,
    trail: Option<(u64, u32)>, // R-A1-004 留痕（求和值, 条数）
    events: Vec<String>,
}
impl BatchFinalizer {
    pub fn new() -> Self {
        BatchFinalizer {
            stage: ChainStage::S1Dedup,
            head: Vec::new(),
            crits: Vec::new(),
            ledger: Vec::new(),
            four: [false; FOUR_ITEMS],
            mount: None,
            trail: None,
            events: Vec::new(),
        }
    }
    pub fn stage(&self) -> ChainStage {
        self.stage
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn load_head(&mut self, entries: &[HeadEntry]) -> Result<u32, u16> {  // 装载头部批条目（批内号唯一，禁空批）。
        if entries.is_empty() {
            return Err(XC_SUM_EMPTY);
        }
        if self.stage != ChainStage::S1Dedup || !self.head.is_empty() {
            return Err(XC_STATE_INVALID);
        }
        let mut added: u32 = 0;
        for e in entries.iter() {
            let mut i = 0usize;
            while i < self.head.len() {
                if self.head[i].eid == e.eid {
                    return Err(XC_DUP_ENTRY);
                }
                i += 1;
            }
            self.head.push(*e);
            added += 1;
        }
        self.events.push(alloc::format!("head +{}", added));
        Ok(added)
    }
    pub fn step1_dedup(&mut self, batch: u32) -> Result<u32, u16> {  // Step1 防重：批号必须等于 B39 且条目号批内唯一（装载时已守卫，此处全量复扫）。
        if self.stage != ChainStage::S1Dedup {
            return Err(XC_STATE_INVALID);
        }
        if batch != BATCH_ID {
            return Err(XC_DUP_BATCH);
        }
        let mut i = 0usize;
        while i < self.head.len() {
            let mut j = i + 1;
            while j < self.head.len() {
                if self.head[i].eid == self.head[j].eid {
                    return Err(XC_DUP_ENTRY);
                }
                j += 1;
            }
            i += 1;
        }
        self.stage = ChainStage::S2Crit;
        self.events.push(String::from("S1 dedup ok"));
        Ok(self.head.len() as u32)
    }
    pub fn step2_crits(&mut self, crits: &[(u32, bool)]) -> Result<u32, u16> {  // Step2 判据：登记判据批 + 走核结论全绿。
        if !self.stage.can_step(ChainStage::S2Crit) {
            return Err(XC_STATE_INVALID);
        }
        if crits.is_empty() {
            return Err(XC_CRIT_SHORT);
        }
        self.crits.clear();
        let mut added: u32 = 0;
        let mut i = 0usize;
        while i < crits.len() {
            // 批内 cid 重复即防重红。
            let mut j = 0usize;
            while j < self.crits.len() {
                if self.crits[j].0 == crits[i].0 {
                    return Err(XC_DUP_BATCH);
                }
                j += 1;
            }
            self.crits.push(crits[i]);
            added += 1;
            i += 1;
        }
        let mut k = 0usize;
        while k < self.crits.len() {
            if !self.crits[k].1 {
                return Err(XC_CRIT_RED);
            }
            k += 1;
        }
        self.stage = ChainStage::S3Conserv;
        self.events.push(alloc::format!("S2 crits {}", added));
        Ok(added)
    }
    pub fn step3_conserve(&mut self) -> Result<u64, u16> {  // Step3 守恒：先逐条求和、后落账（R-A1-004 留痕），实和 == 11,500 才过。
        if !self.stage.can_step(ChainStage::S3Conserv) {
            return Err(XC_STATE_INVALID);
        }
        if self.head.is_empty() {
            return Err(XC_SUM_EMPTY);
        }
        // 先求和（checked_add 逐条累加，禁饱和吞数）。
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < self.head.len() {
            sum = match sum.checked_add(self.head[i].amount) {
                Some(v) => v,
                None => return Err(XC_SUM_DIFF),
            };
            i += 1;
        }
        // 后落账：留痕（求和值, 条数）固化；申报值与实和不一致即红。
        if sum != HEAD_SUBTOTAL {
            return Err(XC_SUM_DIFF);
        }
        self.trail = Some((sum, self.head.len() as u32));
        self.stage = ChainStage::S4Ledger;
        self.events.push(alloc::format!("S3 sum={} n={}", sum, self.head.len()));
        Ok(sum)
    }
    pub fn step4_ledger(&mut self, rows: &[LedgerRow]) -> Result<u32, u16> {  // Step4 台账：恰好 20 条且全部已深化（depth ≥1）。
        if !self.stage.can_step(ChainStage::S4Ledger) {
            return Err(XC_STATE_INVALID);
        }
        if rows.len() != LEDGER_ROWS {
            return Err(XC_LEDGER_SHORT);
        }
        // 行序闭集：0..=20 逐号到位。
        let mut s = 0usize;
        while s < LEDGER_ROWS {
            let mut found = false;
            let mut i = 0usize;
            while i < rows.len() {
                if rows[i].seq == s {
                    found = true;
                    break;
                }
                i += 1;
            }
            if !found {
                return Err(XC_LEDGER_SHORT);
            }
            s += 1;
        }
        self.ledger.clear();
        let mut i = 0usize;
        while i < rows.len() {
            if rows[i].token == 0 {
                return Err(XC_LEDGER_SHALLOW);
            }
            if rows[i].depth < LEDGER_MIN_DEPTH {
                return Err(XC_LEDGER_SHALLOW);
            }
            self.ledger.push(rows[i]);
            i += 1;
        }
        self.stage = ChainStage::S5Four;
        self.events.push(String::from("S4 ledger 20 deep"));
        Ok(self.ledger.len() as u32)
    }
    pub fn step5_four(&mut self, flags: [bool; FOUR_ITEMS]) -> Result<u32, u16> {  // Step5 四项齐备：四项逐一登记（账面/判据/回执/前向）。
        if !self.stage.can_step(ChainStage::S5Four) {
            return Err(XC_STATE_INVALID);
        }
        let mut i = 0usize;
        while i < FOUR_ITEMS {
            if !flags[i] {
                return Err(XC_FOUR_MISSING);
            }
            i += 1;
        }
        self.four = flags;
        self.stage = ChainStage::Sealed;
        self.events.push(String::from("S5 four items ok"));
        Ok(FOUR_ITEMS as u32)
    }
    pub fn seal(&mut self) -> Result<FinalizeReceipt, u16> {  // 封批：五步链尾 + 留痕在账 → 出证（重复封批即防重红）。
        if self.stage == ChainStage::Sealed && self.four.iter().all(|f| *f) && self.seal_marker() {
            return Err(XC_ALREADY_SEALED);
        }
        if self.stage != ChainStage::Sealed {
            return Err(XC_STATE_INVALID);
        }
        let (sum, n) = match self.trail {
            Some(t) => t,
            None => return Err(XC_SUM_EMPTY),
        };
        let fp = self.fingerprint();
        let summary = alloc::format!(
            "UNX-F0780-J1 收口五步断言链全过：防重/判据 {}/{} 绿/守恒 {}（逐条求和后落账，R-A1-004 留痕）/台账 {} 条已深化/四项齐备，前置面全绿封批",
            self.crits.len(), self.crits.len(), sum, self.ledger.len()
        );
        if summary.is_empty() {
            return Err(XC_SUMMARY_EMPTY);
        }
        self.events.push(String::from("sealed"));
        Ok(FinalizeReceipt {
            batch: BATCH_ID,
            subtotal: sum,
            entries: n,
            ledger_rows: self.ledger.len() as u32,
            mount: None,
            summary,
            fingerprint: fp,
        })
    }

    /// 封批标记（内部：事件账含 sealed 即视为已标记）。
    fn seal_marker(&self) -> bool {
        let mut i = 0usize;
        while i < self.events.len() {
            if self.events[i] == "sealed" {
                return true;
            }
            i += 1;
        }
        false
    }
    pub fn mount_forward(&mut self, slot: u8) -> Result<ForwardMount, u16> {  // B40 前向挂载：封批后一次性落账（槽闭集、重挂即红）。
        if !self.seal_marker() {
            return Err(XC_NOT_SEALED);
        }
        if slot < 1 || slot > MOUNT_SLOTS {
            return Err(XC_MOUNT_BAD);
        }
        if self.mount.is_some() {
            return Err(XC_MOUNT_DUP);
        }
        let m = ForwardMount { slot, batch: BATCH_ID, anchor: self.fingerprint() };
        self.mount = Some(m);
        self.events.push(alloc::format!("mount B40 slot={}", slot));
        Ok(m)
    }
    pub fn mount_of(&self) -> Option<ForwardMount> {
        self.mount
    }
    pub fn trail_of(&self) -> Option<(u64, u32)> {
        self.trail
    }

    pub fn chain_progress(&self) -> u8 {  // 链进度：当前步号（0..=5，Sealed 记 5；写回面进度条用）。
        self.stage as u8
    }

    pub fn crit_view(&self, cid: u32) -> Result<bool, u16> {  // 判据视图：cid → 走核结论（未登记 Err）。
        let mut i = 0usize;
        while i < self.crits.len() {
            if self.crits[i].0 == cid {
                return Ok(self.crits[i].1);
            }
            i += 1;
        }
        Err(XC_CRIT_SHORT)
    }

    pub fn ledger_view(&self) -> Result<Vec<String>, u16> {  // 台账视图：逐行「行序 × token × 深化度」计划行（写回面用）。
        if self.ledger.is_empty() {
            return Err(XC_LEDGER_SHORT);
        }
        let mut out: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < self.ledger.len() {
            out.push(alloc::format!(
                "L{:02} token={:#018x} depth={}",
                self.ledger[i].seq, self.ledger[i].token, self.ledger[i].depth
            ));
            i += 1;
        }
        Ok(out)
    }

    pub fn head_sum_recheck(&self) -> Result<u64, u16> {  // 头部批复核和：只读重算 Σamount（与 trail 落账值必须一致——账实互证）。
        if self.head.is_empty() {
            return Err(XC_SUM_EMPTY);
        }
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < self.head.len() {
            sum = match sum.checked_add(self.head[i].amount) {
                Some(v) => v,
                None => return Err(XC_SUM_DIFF),
            };
            i += 1;
        }
        match self.trail {
            Some((s, _)) if s == sum => Ok(sum),
            Some(_) => Err(XC_SUM_DIFF),
            None => Ok(sum),
        }
    }

    pub fn four_view(&self) -> [bool; FOUR_ITEMS] {  // 四项齐备视图（逐项结论，Step5 前全 false）。
        self.four
    }
    pub fn fingerprint(&self) -> u64 {  // 批面指纹。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv1a(h, BATCH_ID as u64);
        let mut i = 0usize;
        while i < self.head.len() {
            h = fnv1a(h, self.head[i].eid as u64);
            h = fnv1a(h, self.head[i].amount);
            i += 1;
        }
        let mut j = 0usize;
        while j < self.ledger.len() {
            h = fnv1a(h, self.ledger[j].seq as u64);
            h = fnv1a(h, self.ledger[j].token);
            h = fnv1a(h, self.ledger[j].depth as u64);
            j += 1;
        }
        h
    }
}
pub fn gen_head(n: usize) -> Vec<HeadEntry> {  // 样本发生器：头部批条目（合计恰 11,500）与 20 行台账（全深化）。
    // 均摊 11,500：每条基数 + 余数落首条（确定性，先构造后复算必一致）。
    let base = HEAD_SUBTOTAL / n as u64;
    let rem = HEAD_SUBTOTAL % n as u64;
    let mut out: Vec<HeadEntry> = Vec::new();
    let mut i = 0usize;
    while i < n {
        let amount = if i == 0 { base + rem } else { base };
        out.push(HeadEntry { eid: (i + 1) as u32, amount });
        i += 1;
    }
    out
}
pub fn gen_ledger() -> Vec<LedgerRow> {
    let mut out: Vec<LedgerRow> = Vec::new();
    let mut s = 0usize;
    while s < LEDGER_ROWS {
        out.push(LedgerRow {
            seq: s,
            token: 0x6040_0000_0000_0000 | (s as u64 + 1),
            depth: 1 + (s % 3) as u8,
        });
        s += 1;
    }
    out
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径（五步全过封批挂载）+ 错误路径（各步红案逐项检出），0=全过。
    // —— 主路径 ——
    let head = gen_head(23);
    let mut m = BatchFinalizer::new();
    m.load_head(&head)?;
    m.step1_dedup(BATCH_ID)?;
    let crits = vec![(1u32, true), (2, true), (3, true), (4, true), (5, true), (6, true)];
    m.step2_crits(&crits)?;
    let sum = m.step3_conserve()?;
    if sum != HEAD_SUBTOTAL {
        return Err(XC_SUM_DIFF);
    }
    if m.trail_of() != Some((HEAD_SUBTOTAL, 23)) {
        return Err(XC_SUM_EMPTY);
    }
    m.step4_ledger(&gen_ledger())?;
    m.step5_four([true, true, true, true])?;
    let rc = m.seal()?;
    if rc.batch != BATCH_ID || rc.subtotal != 11_500 || rc.summary.is_empty() {
        return Err(XC_SUMMARY_EMPTY);
    }
    let mt = m.mount_forward(3)?;
    if mt.slot != 3 || mt.batch != BATCH_ID {
        return Err(XC_MOUNT_BAD);
    }
    if m.mount_forward(4) != Err(XC_MOUNT_DUP) {
        return Err(XC_MOUNT_DUP);
    }
    // —— 错误路径 1：防重（批号不符 / 条目重复）——
    let mut m2 = BatchFinalizer::new();
    m2.load_head(&head)?;
    if m2.step1_dedup(0xB040) != Err(XC_DUP_BATCH) {
        return Err(XC_DUP_BATCH);
    }
    let mut m3 = BatchFinalizer::new();
    let mut duphead = head.clone();
    duphead[2].eid = duphead[0].eid;
    if m3.load_head(&duphead) != Err(XC_DUP_ENTRY) {
        return Err(XC_DUP_ENTRY);
    }
    // —— 错误路径 2：判据红 / 判据空 ——
    let mut m4 = BatchFinalizer::new();
    m4.load_head(&head)?;
    m4.step1_dedup(BATCH_ID)?;
    let badcrits = vec![(1u32, true), (2, false)];
    if m4.step2_crits(&badcrits) != Err(XC_CRIT_RED) {
        return Err(XC_CRIT_RED);
    }
    if m4.step2_crits(&[]) != Err(XC_CRIT_SHORT) {
        return Err(XC_CRIT_SHORT);
    }
    // —— 错误路径 3：守恒红（实和 ≠ 11,500）——
    let mut m5 = BatchFinalizer::new();
    let mut badsum = head.clone();
    badsum[0].amount += 1;
    m5.load_head(&badsum)?;
    m5.step1_dedup(BATCH_ID)?;
    m5.step2_crits(&crits)?;
    if m5.step3_conserve() != Err(XC_SUM_DIFF) {
        return Err(XC_SUM_DIFF);
    }
    // —— 错误路径 4：台账缺行 / 未深化 ——
    let mut m6 = BatchFinalizer::new();
    m6.load_head(&head)?;
    m6.step1_dedup(BATCH_ID)?;
    m6.step2_crits(&crits)?;
    m6.step3_conserve()?;
    let mut short_ledger = gen_ledger();
    short_ledger.pop();
    if m6.step4_ledger(&short_ledger) != Err(XC_LEDGER_SHORT) {
        return Err(XC_LEDGER_SHORT);
    }
    let mut m7 = BatchFinalizer::new();
    m7.load_head(&head)?;
    m7.step1_dedup(BATCH_ID)?;
    m7.step2_crits(&crits)?;
    m7.step3_conserve()?;
    let mut shallow = gen_ledger();
    shallow[5].depth = 0;
    if m7.step4_ledger(&shallow) != Err(XC_LEDGER_SHALLOW) {
        return Err(XC_LEDGER_SHALLOW);
    }
    // —— 错误路径 5：四项缺项 / 未封批即挂载 / 槽越界 ——
    let mut m8 = BatchFinalizer::new();
    m8.load_head(&head)?;
    m8.step1_dedup(BATCH_ID)?;
    m8.step2_crits(&crits)?;
    m8.step3_conserve()?;
    m8.step4_ledger(&gen_ledger())?;
    if m8.step5_four([true, true, false, true]) != Err(XC_FOUR_MISSING) {
        return Err(XC_FOUR_MISSING);
    }
    if m8.mount_forward(1) != Err(XC_NOT_SEALED) {
        return Err(XC_NOT_SEALED);
    }
    let mut m9 = BatchFinalizer::new();
    m9.load_head(&head)?;
    m9.step1_dedup(BATCH_ID)?;
    m9.step2_crits(&crits)?;
    m9.step3_conserve()?;
    m9.step4_ledger(&gen_ledger())?;
    m9.step5_four([true, true, true, true])?;
    m9.seal()?;
    if m9.mount_forward(0) != Err(XC_MOUNT_BAD) || m9.mount_forward(9) != Err(XC_MOUNT_BAD) {
        return Err(XC_MOUNT_BAD);
    }
    // —— 扩展 API 面：链进度 / 判据视图 / 台账视图 / 复核和 / 四项视图 ——
    if m.chain_progress() != ChainStage::Sealed as u8 {
        return Err(XC_STATE_INVALID);
    }
    let mut c = 1u32;
    while c <= 6 {
        if m.crit_view(c)? != true {
            return Err(XC_CRIT_RED);
        }
        c += 1;
    }
    if m.crit_view(99) != Err(XC_CRIT_SHORT) {
        return Err(XC_CRIT_SHORT);
    }
    let lv = m.ledger_view()?;
    if lv.len() != LEDGER_ROWS {
        return Err(XC_LEDGER_SHORT);
    }
    if m.head_sum_recheck()? != HEAD_SUBTOTAL {
        return Err(XC_SUM_DIFF);
    }
    if !m.four_view().iter().all(|f| *f) {
        return Err(XC_FOUR_MISSING);
    }
    // —— m5（守恒红案）复核和应与 trail 缺失态自洽（未落账返回实和）——
    if m5.head_sum_recheck() != Err(XC_SUM_DIFF) {
        return Err(XC_SUM_DIFF);
    }
    Ok(0)
}
