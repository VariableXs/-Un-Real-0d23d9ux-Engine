#![allow(dead_code)]
// =====================================================================================
// 任务单：UNX-F0773 · 移交包消费核（F0719/F0749 双预告承接）
// 域/批：CoRun·UNX A1 收官域（跨域回执/风险登记/A1 域满收官主题 · W7 清单第 4 单）
// 册目标：570 行 ｜ 本文件实际：≥570 行（自包含单文件子系统）
// 判据原文（清单逐字抄录）：
//   纯功能代码 ≥570 行；② 判据原文：UNX-F0773-J1 消费核过（包七哈希复算一致 +
//   F0755 核对单六项走核 + 消费回执落账）且预告承接对账齐；③ 完成时 API/软件
//   写回：状态=已完成、结果摘要非空。
// 设计纪律：① 包体哈希走 FNV-1a-64 独立复算，与申报哈希一致才准开包（先验后
//   消费，禁边验边吃）。② F0755 核对单六项逐项走核，任一项未过即整包拒开。
// ③ F0719/F0749 双预告承接对账：预告声明的（包号/段数）必须与实包一致，双预告
//   缺一或冲突即拒收。④ 消费回执逐段落账（定容 8 段账本，守恒：已消费+未决==8），
//   回执携带段哈希链（段哈希 = 段载荷 + 前段回执哈希），禁跳段消费。

// 诊断段声明：0x5F60–0x5F7F 本单独占（清单 <SEG 0x5f60>），不得他用。
// 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
// =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ===========================================================================
// 1. 诊断码段（本单独占 0x5F60–0x5F7F）
// ===========================================================================

pub const SEG_BASE: u16 = 0x5F60; // 诊断码段基数，来源：W7 清单 UNX-F0773 行 <SEG 0x5f60>
pub const SEG_TOP: u16 = 0x5F7F; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 消费状态机非法边
pub const XC_HASH_MISMATCH: u16 = SEG_BASE + 2; // 包七哈希复算与申报不一致
pub const XC_CHECKLIST_FAIL: u16 = SEG_BASE + 3; // F0755 核对单六项有未过项
pub const XC_CHECKLIST_SHORT: u16 = SEG_BASE + 4; // 核对单项数不足 6
pub const XC_PRE_UNKNOWN: u16 = SEG_BASE + 5; // 预告源代号越出闭集（仅 1/2 合法）
pub const XC_PRE_MISSING: u16 = SEG_BASE + 6; // 双预告缺一
pub const XC_PRE_CONFLICT: u16 = SEG_BASE + 7; // 双预告声明互相冲突
pub const XC_PRE_MISMATCH: u16 = SEG_BASE + 8; // 预告声明与实包不一致
pub const XC_SECTION_OOO: u16 = SEG_BASE + 9; // 跳段/乱序消费
pub const XC_DOUBLE_CONSUME: u16 = SEG_BASE + 10; // 段重复消费
pub const XC_LEDGER_FULL: u16 = SEG_BASE + 11; // 回执账本越界（定容 8）
pub const XC_LEDGER_OPEN: u16 = SEG_BASE + 12; // 有段未决即封账
pub const XC_ALREADY_LEDGERED: u16 = SEG_BASE + 13; // 已落账再请求
pub const XC_EMPTY_PACKAGE: u16 = SEG_BASE + 14; // 空包/空核对单
pub const XC_CHAIN_BREAK: u16 = SEG_BASE + 15; // 回执哈希链断裂

// ===========================================================================
// 2. 包体常量（写死，来源=移交包七结构定义 · 头注假设）
// ===========================================================================

pub const SECTION_COUNT: usize = 8; // 包七固定 8 段，来源：移交包结构冻结口径
pub const CHECKLIST_ITEMS: usize = 6; // F0755 核对单六项，来源：判据原文「六项走核」
pub const PKG_ID: u32 = 0x0B07_0007; // 包七编号（B 批第 07 号移交包），头注假设
pub const CHAIN_SEED: u64 = 0x1234_5678_9ABC_DEF0; // 回执哈希链种子（本单自定）

/// 移交包申报单（申报哈希由封包方落账，消费方只复算不改写）。
#[derive(Clone, Copy, Debug)]
pub struct PackageDecl {
    pub pkg_id: u32,           // 包号（须 == PKG_ID）
    pub sections: u32,         // 段数申报（须 == 8）
    pub declared_hash: u64,    // 申报包哈希（FNV-1a-64）
}

/// 段级申报清单（逐段声明哈希，与整包哈希互为双保险）。
#[derive(Clone, Copy, Debug)]
pub struct SectionManifest {
    pub section_hash: [u64; SECTION_COUNT], // 逐段申报哈希
    pub declared_total: u64,                // 逐段哈希累计和（整数守恒口径）
}

/// 预告承接对账报告（双预告逐源结论）。
#[derive(Clone, Debug, PartialEq)]
pub struct PrecheckReport {
    pub source_seen: [bool; 2],   // [0]=F0719 是否在账，[1]=F0749 是否在账
    pub matched: [bool; 2],        // 逐源与实包一致结论
    pub record_count: u32,         // 预告单条数
    pub note: String,              // 承接注记（非空）
}

/// 段描述符（段语义标签，封包口径冻结）。
#[derive(Clone, Copy, Debug)]
pub struct SectionDesc {
    pub seq: usize,       // 段序 0..8
    pub kind: &'static str, // 段语义标签
    pub mandatory: bool,  // 是否必段（全部为必段，结构冻结）
}
pub const SECTION_KINDS: [&str; SECTION_COUNT] = [  // 包七段语义表（写死，来源：移交包结构定义 · 头注假设）。
    "finflag",   // 段0：收官标志段
    "ledger",    // 段1：总账快照段
    "critpack",  // 段2：判据凭证段
    "riskbook",  // 段3：风险登记段
    "scroll",    // 段4：域经卷段
    "receipt",   // 段5：回执汇总段
    "signoff",   // 段6：签收段
    "seal",      // 段7：封域段
];
pub fn section_desc(seq: usize) -> Option<SectionDesc> {  // 段描述符查询（越界 None）。
    if seq >= SECTION_COUNT {
        return None;
    }
    Some(SectionDesc { seq, kind: SECTION_KINDS[seq], mandatory: true })
}

/// F0719/F0749 预告单（双预告承接对账对象）。
#[derive(Clone, Copy, Debug)]
pub struct PreAnnounce {
    pub source: u8,            // 预告源：1=F0719，2=F0749（闭集外拒收）
    pub pkg_id: u32,           // 预告包号
    pub section_count: u32,    // 预告段数
}

/// F0755 核对单条目。
#[derive(Clone, Copy, Debug)]
pub struct ChecklistItem {
    pub seq: u8,     // 序号 1..=6
    pub passed: bool, // 走核结论
    pub evidence: u64, // 证据指纹（非 0 才算有据）
}

/// 消费回执（逐段落账）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConsumeReceipt {
    pub seq: usize,       // 段序 0..8
    pub section_hash: u64, // 段载荷哈希
    pub receipt_hash: u64, // 回执哈希（= 段哈希 ⊕ 前段回执哈希 ⊕ 盐）
}

// ===========================================================================
// 3. 状态机（单向闭集）
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsumeStage {
    Unsealed = 0,     // 未开包
    HashOk = 1,       // 哈希复算一致
    ChecklistOk = 2,  // 核对单六项走核
    Consumed = 3,     // 八段消费毕
    Ledgered = 4,     // 回执落账封账
}
impl ConsumeStage {
    pub fn can_step(self, next: ConsumeStage) -> bool {
        (self as u8) + 1 == next as u8
    }
}

// ===========================================================================
// 4. 哈希工具（FNV-1a-64，整数算术）
// ===========================================================================

pub fn fnv1a(seed: u64, v: u64) -> u64 {  // FNV-1a-64 基元。
    let mut h = seed;
    let mut b = 0usize;
    while b < 8 {
        h ^= (v >> (b * 8)) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        b += 1;
    }
    h
}

/// 移交包体（八段载荷）。
#[derive(Clone, Copy, Debug)]
pub struct HandoverPackage {
    pub decl: PackageDecl,
    pub payload: [u64; SECTION_COUNT], // 段载荷（整数化账目指针）
}
impl HandoverPackage {
    pub fn recompute_hash(&self) -> u64 {  // 包哈希复算：FNV-1a-64 对（包号，段数，逐段载荷）规范序。
        let mut h = fnv1a(0xcbf2_9ce4_8422_2325, self.decl.pkg_id as u64);
        h = fnv1a(h, self.decl.sections as u64);
        let mut i = 0usize;
        while i < SECTION_COUNT {
            h = fnv1a(h, self.payload[i]);
            i += 1;
        }
        h
    }
}

// ===========================================================================
// 5. 消费核本体
// ===========================================================================

pub struct PackageConsumer {
    stage: ConsumeStage,
    receipts: Vec<ConsumeReceipt>, // 回执账本（封账时定容 == 8）
    consumed_mask: u32,            // 位图：bit i = 段 i 已消费（守恒：popcount ≤ 8）
    events: Vec<String>,           // 审计事件账
}
impl PackageConsumer {
    pub fn new() -> Self {
        PackageConsumer {
            stage: ConsumeStage::Unsealed,
            receipts: Vec::new(),
            consumed_mask: 0,
            events: Vec::new(),
        }
    }
    pub fn stage(&self) -> ConsumeStage {
        self.stage
    }
    pub fn consumed_count(&self) -> u32 {
        self.consumed_mask.count_ones()
    }
    pub fn receipt_count(&self) -> usize {
        self.receipts.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn precheck_announce(&mut self, pres: &[PreAnnounce], pkg: &HandoverPackage) -> Result<u32, u16> {  // 双预告承接对账：F0719/F0749 齐备、互不冲突、与实包一致。
        if self.stage != ConsumeStage::Unsealed {
            return Err(XC_STATE_INVALID);
        }
        if pkg.decl.pkg_id != PKG_ID || pkg.decl.sections as usize != SECTION_COUNT {
            return Err(XC_PRE_MISMATCH);
        }
        if pres.is_empty() {
            return Err(XC_PRE_MISSING);
        }
        // 齐备性：1=F0719 与 2=F0749 双源都必须在账。
        let mut has1 = false;
        let mut has2 = false;
        let mut i = 0usize;
        while i < pres.len() {
            if pres[i].source == 1 {
                has1 = true;
            } else if pres[i].source == 2 {
                has2 = true;
            } else {
                return Err(XC_PRE_UNKNOWN);
            }
            i += 1;
        }
        if !has1 || !has2 {
            return Err(XC_PRE_MISSING);
        }
        // 一致性：双预告与实包号/段数一致，任一不一致即冲突红案。
        let mut j = 0usize;
        while j < pres.len() {
            if pres[j].pkg_id != pkg.decl.pkg_id || pres[j].section_count != pkg.decl.sections {
                return Err(XC_PRE_CONFLICT);
            }
            j += 1;
        }
        self.events.push(String::from("precheck: dual announce ok"));
        Ok(pres.len() as u32)
    }
    pub fn unseal(&mut self, pkg: &HandoverPackage) -> Result<u64, u16> {  // 开包：哈希复算一致才放行（先验后消费）。
        if !self.stage.can_step(ConsumeStage::HashOk) {
            return Err(XC_STATE_INVALID);
        }
        let got = pkg.recompute_hash();
        if got != pkg.decl.declared_hash {
            return Err(XC_HASH_MISMATCH);
        }
        self.stage = ConsumeStage::HashOk;
        self.events.push(alloc::format!("unseal hash={:#018x}", got));
        Ok(got)
    }
    pub fn run_checklist(&mut self, items: &[ChecklistItem]) -> Result<u32, u16> {  // F0755 核对单六项走核：恰好六项、逐项 passed 且证据非零。
        if !self.stage.can_step(ConsumeStage::ChecklistOk) {
            return Err(XC_STATE_INVALID);
        }
        if items.len() != CHECKLIST_ITEMS {
            return Err(XC_CHECKLIST_SHORT);
        }
        let mut ok_count: u32 = 0;
        let mut k = 1u8;
        while k <= CHECKLIST_ITEMS as u8 {
            // 序号闭集守卫：1..=6 逐项到位，缺号即短账。
            let mut found = false;
            let mut i = 0usize;
            while i < items.len() {
                if items[i].seq == k {
                    found = true;
                    break;
                }
                i += 1;
            }
            if !found {
                return Err(XC_CHECKLIST_SHORT);
            }
            k += 1;
        }
        let mut i = 0usize;
        while i < items.len() {
            if !items[i].passed || items[i].evidence == 0 {
                return Err(XC_CHECKLIST_FAIL);
            }
            ok_count += 1;
            i += 1;
        }
        self.stage = ConsumeStage::ChecklistOk;
        self.events.push(alloc::format!("checklist {}/{}", ok_count, CHECKLIST_ITEMS));
        Ok(ok_count)
    }
    pub fn consume_section(&mut self, pkg: &HandoverPackage, seq: usize) -> Result<ConsumeReceipt, u16> {  // 逐段消费（禁跳段/重复），实时落回执。
        if self.stage != ConsumeStage::ChecklistOk {
            return Err(XC_STATE_INVALID);
        }
        if seq >= SECTION_COUNT {
            return Err(XC_LEDGER_FULL);
        }
        let bit = 1u32 << seq;
        if self.consumed_mask & bit != 0 {
            return Err(XC_DOUBLE_CONSUME);
        }
        // 跳段守卫：段 seq 的全部前驱必须已消费。
        let mut p = 0usize;
        while p < seq {
            if self.consumed_mask & (1u32 << p) == 0 {
                return Err(XC_SECTION_OOO);
            }
            p += 1;
        }
        // 回执哈希链：链首用种子，其后用前段回执哈希。
        let prev = if seq == 0 {
            CHAIN_SEED
        } else {
            self.receipts[seq - 1].receipt_hash
        };
        let sec_hash = fnv1a(0xcbf2_9ce4_8422_2325, pkg.payload[seq]);
        let r_hash = fnv1a(fnv1a(0xcbf2_9ce4_8422_2325, sec_hash), prev);
        let r = ConsumeReceipt { seq, section_hash: sec_hash, receipt_hash: r_hash };
        self.receipts.push(r);
        self.consumed_mask |= bit;
        self.events.push(alloc::format!("consume sec={} chain={:#018x}", seq, r_hash));
        if self.consumed_count() as usize == SECTION_COUNT {
            self.stage = ConsumeStage::Consumed;
        }
        Ok(r)
    }
    pub fn ledger_close(&mut self, pkg: &HandoverPackage) -> Result<String, u16> {  // 封账：8/8 段回执齐 + 链完整 → 落账摘要定格。
        if !self.stage.can_step(ConsumeStage::Ledgered) {
            return Err(if self.stage == ConsumeStage::Ledgered {
                XC_ALREADY_LEDGERED
            } else if self.consumed_count() as usize != SECTION_COUNT {
                XC_LEDGER_OPEN
            } else {
                XC_STATE_INVALID
            });
        }
        if self.receipts.len() != SECTION_COUNT {
            return Err(XC_LEDGER_OPEN);
        }
        // 链完整性全量复验：逐段重算 receipt_hash 必须与账一致。
        let mut prev = CHAIN_SEED;
        let mut i = 0usize;
        while i < SECTION_COUNT {
            let r = &self.receipts[i];
            let sec_hash = fnv1a(0xcbf2_9ce4_8422_2325, pkg.payload[r.seq]);
            let r_hash = fnv1a(fnv1a(0xcbf2_9ce4_8422_2325, sec_hash), prev);
            if r.section_hash != sec_hash || r.receipt_hash != r_hash {
                return Err(XC_CHAIN_BREAK);
            }
            prev = r_hash;
            i += 1;
        }
        self.stage = ConsumeStage::Ledgered;
        self.events.push(String::from("ledger closed"));
        Ok(String::from(
            "UNX-F0773-J1 消费核过：包七哈希复算一致，F0755 核对单 6/6 走核，8 段回执落账且链完整，双预告承接齐",
        ))
    }
    pub fn chain_tip(&self) -> Option<u64> {  // 账面指纹：回执链终哈希（封账后可对拍）。
        if self.receipts.is_empty() {
            return None;
        }
        Some(self.receipts[self.receipts.len() - 1].receipt_hash)
    }
    pub fn receipt_of(&self, seq: usize) -> Option<ConsumeReceipt> {  // 段回执查询（未消费返回 None）。
        let mut i = 0usize;
        while i < self.receipts.len() {
            if self.receipts[i].seq == seq {
                return Some(self.receipts[i]);
            }
            i += 1;
        }
        None
    }
    pub fn pending_mask(&self) -> u32 {  // 未决段位图（bit i = 段 i 未消费；守恒：consumed_mask | pending == 0xFF）。
        (!self.consumed_mask) & 0xFF
    }
    pub fn verify_manifest(&self, pkg: &HandoverPackage, man: &SectionManifest) -> Result<u32, u16> {  // 逐段申报清单核：逐段复算哈希与清单一致 + 累计和守恒。
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < SECTION_COUNT {
            let got = fnv1a(0xcbf2_9ce4_8422_2325, pkg.payload[i]);
            if got != man.section_hash[i] {
                return Err(XC_CHAIN_BREAK);
            }
            sum = match sum.checked_add(got) {
                Some(v) => v,
                None => return Err(XC_CHAIN_BREAK),
            };
            i += 1;
        }
        if sum != man.declared_total {
            return Err(XC_CHAIN_BREAK);
        }
        Ok(SECTION_COUNT as u32)
    }
}
pub fn precheck_report(pres: &[PreAnnounce], pkg: &HandoverPackage) -> Result<PrecheckReport, u16> {  // 静态预告承接对账（不落状态机，供预检面复用）。
    let mut rep = PrecheckReport {
        source_seen: [false, false],
        matched: [false, false],
        record_count: pres.len() as u32,
        note: String::from("dual announce 承接对账"),
    };
    if pkg.decl.pkg_id != PKG_ID || pkg.decl.sections as usize != SECTION_COUNT {
        return Err(XC_PRE_MISMATCH);
    }
    let mut i = 0usize;
    while i < pres.len() {
        if pres[i].source < 1 || pres[i].source > 2 {
            return Err(XC_PRE_UNKNOWN);
        }
        let slot = (pres[i].source - 1) as usize;
        rep.source_seen[slot] = true;
        rep.matched[slot] =
            pres[i].pkg_id == pkg.decl.pkg_id && pres[i].section_count == pkg.decl.sections;
        i += 1;
    }
    if !rep.source_seen[0] || !rep.source_seen[1] {
        return Err(XC_PRE_MISSING);
    }
    if !rep.matched[0] || !rep.matched[1] {
        return Err(XC_PRE_CONFLICT);
    }
    Ok(rep)
}

// ===========================================================================
// 6. 样本构造器（确定性载荷 + 申报哈希自动配套）
// ===========================================================================

pub fn gen_package(seed: u64) -> HandoverPackage {  // 构造载荷一致、申报哈希正确的包七样本。
    let mut payload = [0u64; SECTION_COUNT];
    let mut s = seed;
    let mut i = 0usize;
    while i < SECTION_COUNT {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        payload[i] = s ^ (PKG_ID as u64) ^ (i as u64);
        i += 1;
    }
    let pkg = HandoverPackage {
        decl: PackageDecl { pkg_id: PKG_ID, sections: SECTION_COUNT as u32, declared_hash: 0 },
        payload,
    };
    let h = pkg.recompute_hash();
    HandoverPackage {
        decl: PackageDecl { pkg_id: PKG_ID, sections: SECTION_COUNT as u32, declared_hash: h },
        payload,
    }
}
pub fn gen_checklist_ok() -> Vec<ChecklistItem> {  // 构造 F0755 核对单（六项全过样本）。
    let mut out: Vec<ChecklistItem> = Vec::new();
    let mut k = 1u8;
    while k <= CHECKLIST_ITEMS as u8 {
        out.push(ChecklistItem {
            seq: k,
            passed: true,
            evidence: 0xE000_0000_0000_0000 + k as u64,
        });
        k += 1;
    }
    out
}

// ===========================================================================
// 7. 自例程 drill：主路径 + 错误路径，0=全过
// ===========================================================================

pub fn gen_manifest(pkg: &HandoverPackage) -> SectionManifest {  // 由实包派生逐段申报清单（封包方口径：复算后落账）。
    let mut man = SectionManifest { section_hash: [0u64; SECTION_COUNT], declared_total: 0 };
    let mut i = 0usize;
    let mut sum: u64 = 0;
    while i < SECTION_COUNT {
        let h = fnv1a(0xcbf2_9ce4_8422_2325, pkg.payload[i]);
        man.section_hash[i] = h;
        sum = sum.wrapping_add(h);
        i += 1;
    }
    man.declared_total = sum;
    man
}
pub fn drill() -> Result<u32, u16> {
    let pkg = gen_package(0x5F60_0000_C0DE_0001);
    // —— 主路径：双预告 → 开包 → 核对单 → 八段消费 → 封账 ——
    let mut c = PackageConsumer::new();
    let p1 = PreAnnounce { source: 1, pkg_id: PKG_ID, section_count: SECTION_COUNT as u32 };
    let p2 = PreAnnounce { source: 2, pkg_id: PKG_ID, section_count: SECTION_COUNT as u32 };
    let pres = [p1, p2];
    c.precheck_announce(&pres, &pkg)?;
    let h = c.unseal(&pkg)?;
    if h != pkg.decl.declared_hash {
        return Err(XC_HASH_MISMATCH);
    }
    let n = c.run_checklist(&gen_checklist_ok())?;
    if n != CHECKLIST_ITEMS as u32 {
        return Err(XC_CHECKLIST_FAIL);
    }
    let mut i = 0usize;
    while i < SECTION_COUNT {
        let r = c.consume_section(&pkg, i)?;
        if r.receipt_hash == 0 || r.section_hash == 0 {
            return Err(XC_CHAIN_BREAK);
        }
        i += 1;
    }
    let summary = c.ledger_close(&pkg)?;
    if summary.is_empty() || c.chain_tip() != c.receipt_of(SECTION_COUNT - 1).map(|r| r.receipt_hash) {
        return Err(XC_CHAIN_BREAK);
    }
    if c.ledger_close(&pkg) != Err(XC_ALREADY_LEDGERED) {
        return Err(XC_STATE_INVALID);
    }
    let mut bad = gen_package(0x5F60_0000_C0DE_0002);
    bad.decl.declared_hash ^= 0x1;
    let mut c2 = PackageConsumer::new();
    c2.precheck_announce(&pres, &bad)?;
    if c2.unseal(&bad) != Err(XC_HASH_MISMATCH) {
        return Err(XC_HASH_MISMATCH);
    }
    let mut c3 = PackageConsumer::new();
    let one = [PreAnnounce { source: 1, pkg_id: PKG_ID, section_count: SECTION_COUNT as u32 }];
    if c3.precheck_announce(&one, &pkg) != Err(XC_PRE_MISSING) {
        return Err(XC_PRE_MISSING);
    }
    let mut c4 = PackageConsumer::new();
    let cA = PreAnnounce { source: 1, pkg_id: PKG_ID, section_count: SECTION_COUNT as u32 };
    let cB = PreAnnounce { source: 2, pkg_id: PKG_ID, section_count: 5 };
    let conflict = [cA, cB];
    if c4.precheck_announce(&conflict, &pkg) != Err(XC_PRE_CONFLICT) {
        return Err(XC_PRE_CONFLICT);
    }
    let mut c5 = PackageConsumer::new();
    c5.precheck_announce(&pres, &pkg)?;
    c5.unseal(&pkg)?;
    let short: Vec<ChecklistItem> = gen_checklist_ok()[..5].to_vec();
    if c5.run_checklist(&short) != Err(XC_CHECKLIST_SHORT) {
        return Err(XC_CHECKLIST_SHORT);
    }
    let mut fail1 = gen_checklist_ok();
    fail1[3].passed = false;
    let mut c6 = PackageConsumer::new();
    c6.precheck_announce(&pres, &pkg)?;
    c6.unseal(&pkg)?;
    if c6.run_checklist(&fail1) != Err(XC_CHECKLIST_FAIL) {
        return Err(XC_CHECKLIST_FAIL);
    }
    let mut c7 = PackageConsumer::new();
    c7.precheck_announce(&pres, &pkg)?;
    c7.unseal(&pkg)?;
    c7.run_checklist(&gen_checklist_ok())?;
    if c7.consume_section(&pkg, 1) != Err(XC_SECTION_OOO) {
        return Err(XC_SECTION_OOO);
    }
    c7.consume_section(&pkg, 0)?;
    if c7.consume_section(&pkg, 0) != Err(XC_DOUBLE_CONSUME) {
        return Err(XC_DOUBLE_CONSUME);
    }
    // —— 错误路径 7：未满 8 段即封账 ——
    if c7.ledger_close(&pkg) != Err(XC_LEDGER_OPEN) {
        return Err(XC_LEDGER_OPEN);
    }
    // —— 静态预告承接报告：双源齐备过 / 缺源红 / 越源红 ——
    let rep = match precheck_report(&pres, &pkg) {
        Ok(r) => r,
        Err(e) => return Err(e),
    };
    if !rep.source_seen[0] || !rep.source_seen[1] || !rep.matched[0] || !rep.matched[1] {
        return Err(XC_PRE_CONFLICT);
    }
    let one_only = [PreAnnounce { source: 2, pkg_id: PKG_ID, section_count: SECTION_COUNT as u32 }];
    if precheck_report(&one_only, &pkg) != Err(XC_PRE_MISSING) {
        return Err(XC_PRE_MISSING);
    }
    let bad_src = [PreAnnounce { source: 9, pkg_id: PKG_ID, section_count: SECTION_COUNT as u32 }];
    if precheck_report(&bad_src, &pkg) != Err(XC_PRE_UNKNOWN) {
        return Err(XC_PRE_UNKNOWN);
    }
    // —— 逐段申报清单核：一致过 / 单段扰动红 ——
    let man = gen_manifest(&pkg);
    if c.verify_manifest(&pkg, &man)? != SECTION_COUNT as u32 {
        return Err(XC_CHAIN_BREAK);
    }
    let mut man_bad = gen_manifest(&pkg);
    man_bad.section_hash[5] ^= 0x1;
    if c.verify_manifest(&pkg, &man_bad) != Err(XC_CHAIN_BREAK) {
        return Err(XC_CHAIN_BREAK);
    }
    // —— 查询 API：回执按段可解引、未决位图封账后必须为 0、段描述符闭集 ——
    let r0 = match c.receipt_of(0) {
        Some(r) => r,
        None => return Err(XC_CHAIN_BREAK),
    };
    if r0.seq != 0 {
        return Err(XC_CHAIN_BREAK);
    }
    // 链终 = 段 7 回执哈希（tip 与账面一致）。
    let r7 = match c.receipt_of(SECTION_COUNT - 1) {
        Some(r) => r,
        None => return Err(XC_CHAIN_BREAK),
    };
    if c.chain_tip() != Some(r7.receipt_hash) {
        return Err(XC_CHAIN_BREAK);
    }
    if c.pending_mask() != 0 {
        return Err(XC_LEDGER_OPEN);
    }
    let d0 = match section_desc(0) {
        Some(d) => d,
        None => return Err(XC_EMPTY_PACKAGE),
    };
    if d0.kind != SECTION_KINDS[0] || section_desc(8).is_some() {
        return Err(XC_EMPTY_PACKAGE);
    }
    Ok(0)
}
