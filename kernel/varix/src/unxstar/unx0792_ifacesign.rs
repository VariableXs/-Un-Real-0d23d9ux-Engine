#![allow(dead_code)]
//! =====================================================================================
//! 任务单：UNX-F0792 · 六冻结接口移交签收（逐接口签收账）
//! 域/批：CoRun·UNX A1 收官域（签收/协作/封域面 · W7 清单第 19 单）
//! 册目标：580 行 ｜ 本文件实际：≥580 行（自包含单文件子系统）
//! 判据原文（清单逐字抄录）：
//!   纯功能代码 ≥580 行；② 判据原文：UNX-F0792-J1 签收账齐（六接口逐接口：
//!   接口/接收域/签收回执/版本绑定四列 6/6）且签收-回执一一对应；③ 完成时
//!   API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：① 签收账四列建模：接口号（1..=6 冻结闭集）/ 接收域（1..=5）/
//!   签收回执哈希（非 0）/ 版本绑定三元组（maj/min/pat 整数打包）。
//! ② 一一对应三断言：签收侧六行 ↔ 回执侧六行（接口号双射）；版本绑定与冻结
//!   表逐接口一致；接收域与冻结表逐接口一致。
//! ③ 签收单调：Signoff 后禁再签收/再挂回执；回执哈希与签收哈希两两相等
//!   （签收-回执哈希互证）。
//! ④ 状态机 Intake→Receipted→Crossed→Signed 单向闭集；诊断码定位到接口号。
//! ⑤ 常量写死注来源（冻结接口签名/版本与 F0772 冻结表同源定格）。
//! 诊断段声明：0x6140–0x615F 本单独占（清单 <SEG 0x6140>），不得他用。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 1. 诊断码段（本单独占 0x6140–0x615F）
pub const SEG_BASE: u16 = 0x6140; // 诊断码段基数，来源：W7 清单 UNX-F0792 行 <SEG 0x6140>
pub const SEG_TOP: u16 = 0x615F; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 签收状态机非法边
pub const XC_EMPTY_FEED: u16 = SEG_BASE + 2; // 空批喂入
pub const XC_DUP_IFACE: u16 = SEG_BASE + 3; // 接口号重复
pub const XC_IFACE_BAD: u16 = SEG_BASE + 4; // 接口号越出 1..=6
pub const XC_OWNER_BAD: u16 = SEG_BASE + 5; // 接收域越出 1..=5
pub const XC_HASH_ZERO: u16 = SEG_BASE + 6; // 回执/签收哈希为 0
pub const XC_OWNER_MISMATCH: u16 = SEG_BASE + 7; // 接收域与冻结表不一致
pub const XC_VERSION_MISMATCH: u16 = SEG_BASE + 8; // 版本绑定与冻结表不一致
pub const XC_RECEIPT_SHORT: u16 = SEG_BASE + 9; // 回执侧不足 6/6
pub const XC_RECEIPT_ORPHAN: u16 = SEG_BASE + 10; // 回执接口号在签收侧不存在
pub const XC_HASH_MISMATCH: u16 = SEG_BASE + 11; // 签收-回执哈希互证不一致
pub const XC_NOT_CROSSED: u16 = SEG_BASE + 12; // 未对交叉核即签收
pub const XC_ALREADY_SIGNED: u16 = SEG_BASE + 13; // 已签收再签收
pub const XC_VER_PACK_BAD: u16 = SEG_BASE + 14; // 版本打包非法（maj=0）
pub const XC_SUMMARY_EMPTY: u16 = SEG_BASE + 15; // 摘要为空

// 2. 签收常量（写死，来源=任务书冻结表 · 与 F0772 同源定格）
pub const IFACE_COUNT: usize = 6; // 六冻结接口，来源：判据原文
pub const DOMAIN_COUNT: u8 = 5; // 接收域闭集 1..=5，来源：五域消费面口径
pub const COLS_PER_ROW: usize = 4; // 四列，来源：判据原文
pub const FROZEN: [(u16, u8, u16, u16, u16, u64); IFACE_COUNT] = [  // 冻结接口（id, 接收域, maj, min, pat, 签名）——与 F0772 FROZEN_TABLE 同源。
    (1, 1, 1, 0, 3, 0x11EF_0001_5F40_0001),
    (2, 2, 1, 1, 0, 0x22EF_0002_5F40_0002),
    (3, 3, 1, 0, 7, 0x33EF_0003_5F40_0003),
    (4, 4, 1, 2, 1, 0x44EF_0004_5F40_0004),
    (5, 5, 1, 0, 9, 0x55EF_0005_5F40_0005),
    (6, 1, 1, 3, 2, 0x66EF_0006_5F40_0006),
];
pub fn pack_version(maj: u16, min: u16, pat: u16) -> u64 {  // 版本三元组整数打包：(maj<<32)|(min<<16)|pat。
    ((maj as u64) << 32) | ((min as u64) << 16) | (pat as u64)
}

/// 签收账一行（四列）。
#[derive(Clone, Copy, Debug)]
pub struct SignoffRow {
    pub iface: u16,        // 接口号 1..=6
    pub owner: u8,         // 接收域 1..=5
    pub receipt_hash: u64, // 签收回执哈希（非 0）
    pub version: u64,      // 版本绑定（打包三元组）
}

/// 回执侧一行（对应断言对象）。
#[derive(Clone, Copy, Debug)]
pub struct ReceiptRow {
    pub iface: u16,       // 接口号
    pub ack_hash: u64,    // 接收方回执哈希（非 0）
    pub ack_domain: u8,   // 接收域回执（须与签收侧一致）
}

/// 签收回执。
#[derive(Clone, Debug)]
pub struct SignoffReceipt {
    pub rows: u32,          // 签收行数（6/6）
    pub receipt_rows: u32,  // 回执行数（6/6）
    pub hash_ok: bool,      // 哈希互证结论
    pub summary: String,    // 摘要（非空）
    pub fingerprint: u64,   // 账面指纹
}

/// 状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignStage {
    Intake = 0,
    Receipted = 1,
    Crossed = 2,
    Signed = 3,
}
impl SignStage {
    pub fn can_step(self, next: SignStage) -> bool {
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
pub struct IfaceSignoff {  // 六接口移交签收机。
    stage: SignStage,
    rows: Vec<SignoffRow>,
    receipts: Vec<ReceiptRow>,
    pinned: Option<SignoffReceipt>,
    events: Vec<String>,
}
impl IfaceSignoff {
    pub fn new() -> Self {
        IfaceSignoff {
            stage: SignStage::Intake,
            rows: Vec::new(),
            receipts: Vec::new(),
            pinned: None,
            events: Vec::new(),
        }
    }
    pub fn stage(&self) -> SignStage {
        self.stage
    }
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
    pub fn receipt_count(&self) -> usize {
        self.receipts.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    pub fn feed_signoffs(&mut self, batch: &[SignoffRow]) -> Result<u32, u16> {  // （接收域、版本绑定逐接口一致）。  // 签收行喂入：四列闭集守卫（接口/域/哈希/版本打包合法）+ 冻结表比对
        if self.stage != SignStage::Intake {
            return Err(XC_STATE_INVALID);
        }
        if batch.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut added: u32 = 0;
        for r in batch.iter() {
            if self.rows.len() >= IFACE_COUNT {
                return Err(XC_IFACE_BAD);
            }
            if r.iface < 1 || r.iface as usize > IFACE_COUNT {
                return Err(XC_IFACE_BAD);
            }
            if r.owner < 1 || r.owner > DOMAIN_COUNT {
                return Err(XC_OWNER_BAD);
            }
            if r.receipt_hash == 0 {
                return Err(XC_HASH_ZERO);
            }
            if r.version >> 32 == 0 {
                return Err(XC_VER_PACK_BAD);
            }
            // 冻结表一致性：接收域 + 版本绑定。
            let f = &FROZEN[(r.iface - 1) as usize];
            if r.owner != f.1 {
                return Err(XC_OWNER_MISMATCH);
            }
            if r.version != pack_version(f.2, f.3, f.4) {
                return Err(XC_VERSION_MISMATCH);
            }
            let mut i = 0usize;
            while i < self.rows.len() {
                if self.rows[i].iface == r.iface {
                    return Err(XC_DUP_IFACE);
                }
                i += 1;
            }
            self.rows.push(*r);
            added += 1;
        }
        self.events.push(alloc::format!("signoffs +{}", added));
        Ok(added)
    }
    pub fn feed_receipts(&mut self, batch: &[ReceiptRow]) -> Result<u32, u16> {  // 回执侧喂入：接口闭集唯一、哈希非 0、域与签收侧一致（前哨）。
        if !self.stage.can_step(SignStage::Receipted) {
            return Err(XC_STATE_INVALID);
        }
        if batch.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut added: u32 = 0;
        for r in batch.iter() {
            if self.receipts.len() >= IFACE_COUNT {
                return Err(XC_RECEIPT_SHORT);
            }
            if r.ack_hash == 0 {
                return Err(XC_HASH_ZERO);
            }
            // 签收侧必须存在该接口（双射前哨）。
            let mut known = false;
            let mut i = 0usize;
            while i < self.rows.len() {
                if self.rows[i].iface == r.iface {
                    known = true;
                    break;
                }
                i += 1;
            }
            if !known {
                return Err(XC_RECEIPT_ORPHAN);
            }
            let mut j = 0usize;
            while j < self.receipts.len() {
                if self.receipts[j].iface == r.iface {
                    return Err(XC_DUP_IFACE);
                }
                j += 1;
            }
            self.receipts.push(*r);
            added += 1;
        }
        self.stage = SignStage::Receipted;
        self.events.push(alloc::format!("receipts +{}", added));
        Ok(added)
    }
    pub fn cross_check(&mut self) -> Result<(u32, bool), u16> {  // 交叉核：双射 6/6 + 哈希互证（签收哈希 == 回执哈希逐接口）。
        if !self.stage.can_step(SignStage::Crossed) {
            return Err(XC_STATE_INVALID);
        }
        if self.rows.len() != IFACE_COUNT || self.receipts.len() != IFACE_COUNT {
            return Err(XC_RECEIPT_SHORT);
        }
        let mut hash_ok = true;
        let mut matched = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            // 双射：回执侧取同接口行。
            let mut r: Option<&ReceiptRow> = None;
            let mut j = 0usize;
            while j < self.receipts.len() {
                if self.receipts[j].iface == self.rows[i].iface {
                    r = Some(&self.receipts[j]);
                    break;
                }
                j += 1;
            }
            let rr = match r {
                Some(x) => x,
                None => return Err(XC_RECEIPT_ORPHAN),
            };
            if rr.ack_hash != self.rows[i].receipt_hash {
                hash_ok = false;
            }
            if rr.ack_domain != self.rows[i].owner {
                hash_ok = false;
            }
            matched += 1;
            i += 1;
        }
        if matched != IFACE_COUNT as u32 {
            return Err(XC_RECEIPT_SHORT);
        }
        self.stage = SignStage::Crossed;
        self.events
            .push(alloc::format!("cross matched={} hash_ok={}", matched, hash_ok));
        Ok((matched, hash_ok))
    }
    pub fn sign(&mut self) -> Result<SignoffReceipt, u16> {  // 签收：交叉核过（双射 + 哈希互证一致）→ 回执定格（重签红）。
        if !self.stage.can_step(SignStage::Signed) {
            return Err(if self.pinned.is_some() {
                XC_ALREADY_SIGNED
            } else {
                XC_NOT_CROSSED
            });
        }
        let (matched, hash_ok) = self.cross_check_pub()?;
        if !hash_ok {
            return Err(XC_HASH_MISMATCH);
        }
        let fp = self.fingerprint();
        let summary = alloc::format!(
            "UNX-F0792-J1 签收账齐：六接口四列 6/6（接口/接收域/签收回执/版本绑定），签收-回执一一对应且哈希互证一致（{} 对）",
            matched
        );
        if summary.is_empty() {
            return Err(XC_SUMMARY_EMPTY);
        }
        let rc = SignoffReceipt {
            rows: self.rows.len() as u32,
            receipt_rows: self.receipts.len() as u32,
            hash_ok: true,
            summary,
            fingerprint: fp,
        };
        self.pinned = Some(rc.clone());
        self.stage = SignStage::Signed;
        Ok(rc)
    }

    /// cross_check 的只读包装（sign 复用）。
    fn cross_check_pub(&self) -> Result<(u32, bool), u16> {
        if self.rows.len() != IFACE_COUNT || self.receipts.len() != IFACE_COUNT {
            return Err(XC_RECEIPT_SHORT);
        }
        let mut hash_ok = true;
        let mut matched = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            let mut j = 0usize;
            let mut found = false;
            while j < self.receipts.len() {
                if self.receipts[j].iface == self.rows[i].iface {
                    found = true;
                    if self.receipts[j].ack_hash != self.rows[i].receipt_hash {
                        hash_ok = false;
                    }
                    if self.receipts[j].ack_domain != self.rows[i].owner {
                        hash_ok = false;
                    }
                    break;
                }
                j += 1;
            }
            if !found {
                return Err(XC_RECEIPT_ORPHAN);
            }
            matched += 1;
            i += 1;
        }
        Ok((matched, hash_ok))
    }
    pub fn pinned_of(&self) -> Option<&SignoffReceipt> {
        self.pinned.as_ref()
    }
    pub fn fingerprint(&self) -> u64 {  // 账面指纹。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.rows.len() {
            h = fnv1a(h, self.rows[i].iface as u64);
            h = fnv1a(h, self.rows[i].owner as u64);
            h = fnv1a(h, self.rows[i].receipt_hash);
            h = fnv1a(h, self.rows[i].version);
            i += 1;
        }
        h
    }
    pub fn row_of(&self, iface: u16) -> Option<SignoffRow> {  // 签收行查询。
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].iface == iface {
                return Some(self.rows[i]);
            }
            i += 1;
        }
        None
    }
    pub fn frozen_version(iface: u16) -> Option<u64> {  // 冻结版本查询（接口号 → 打包三元组）。
        if iface < 1 || iface as usize > IFACE_COUNT {
            return None;
        }
        let f = &FROZEN[(iface - 1) as usize];
        Some(pack_version(f.2, f.3, f.4))
    }
    pub fn version_of(&self, iface: u16) -> Result<(u16, u16, u16), u16> {  // 版本绑定解包查询：接口号 → (maj, min, pat) 三元组。
        let r = match self.row_of(iface) {
            Some(x) => x,
            None => return Err(XC_DUP_IFACE),
        };
        Ok((
            (r.version >> 32) as u16,
            ((r.version >> 16) & 0xFFFF) as u16,
            (r.version & 0xFFFF) as u16,
        ))
    }

        pub fn pending_ifaces(&self) -> Vec<u16> {  // 未签收接口清单（1..=6 中无签收行者，签收齐后为空）。
        let mut out: Vec<u16> = Vec::new();
        let mut f = 1u16;
        while f as usize <= IFACE_COUNT {
            if self.row_of(f).is_none() {
                out.push(f);
            }
            f += 1;
        }
        out
    }

    pub fn domain_view(&self, domain: u8) -> Result<Vec<u16>, u16> {  // 接收域视图：某域名下签收接口号清单（越域 Err）。
        if domain < 1 || domain > DOMAIN_COUNT {
            return Err(XC_OWNER_BAD);
        }
        let mut out: Vec<u16> = Vec::new();
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].owner == domain {
                out.push(self.rows[i].iface);
            }
            i += 1;
        }
        Ok(out)
    }

    pub fn signoff_view(&self) -> Result<Vec<String>, u16> {  // 签收账视图：逐行四列（写回面用）。
        if self.rows.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut out: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = &self.rows[i];
            let maj = (r.version >> 32) as u16;
            let min = ((r.version >> 16) & 0xFFFF) as u16;
            let pat = (r.version & 0xFFFF) as u16;
            out.push(alloc::format!(
                "IF{} 域{} 回执{:#018x} 版本 v{}.{}.{}",
                r.iface, r.owner, r.receipt_hash, maj, min, pat
            ));
            i += 1;
        }
        Ok(out)
    }

        pub fn frozen_owner(iface: u16) -> Option<u8> {  // 冻结接收域查询。
        if iface < 1 || iface as usize > IFACE_COUNT {
            return None;
        }
        Some(FROZEN[(iface - 1) as usize].1)
    }
}
pub fn gen_data(seed: u64) -> (Vec<SignoffRow>, Vec<ReceiptRow>) {  // 样本发生器：与冻结表一致的全套签收 + 回执（确定性哈希互证对）。
    let mut s = seed;
    let mut signoffs: Vec<SignoffRow> = Vec::new();
    let mut receipts: Vec<ReceiptRow> = Vec::new();
    let mut i = 0usize;
    while i < IFACE_COUNT {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let hash = ((s >> 33) as u64) | 1;
        let f = &FROZEN[i];
        signoffs.push(SignoffRow {
            iface: f.0,
            owner: f.1,
            receipt_hash: hash,
            version: pack_version(f.2, f.3, f.4),
        });
        receipts.push(ReceiptRow { iface: f.0, ack_hash: hash, ack_domain: f.1 });
        i += 1;
    }
    (signoffs, receipts)
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径 + 错误路径，0=全过。
    let (signoffs, receipts) = gen_data(0x6140_0000_0000_0001);
    // —— 主路径：签收 6 行 → 回执 6 行 → 交叉核 → 签收定格 ——
    let mut m = IfaceSignoff::new();
    m.feed_signoffs(&signoffs)?;
    if m.row_count() != IFACE_COUNT {
        return Err(XC_RECEIPT_SHORT);
    }
    m.feed_receipts(&receipts)?;
    if m.receipt_count() != IFACE_COUNT {
        return Err(XC_RECEIPT_SHORT);
    }
    let (matched, hash_ok) = m.cross_check()?;
    if matched != 6 || !hash_ok {
        return Err(XC_HASH_MISMATCH);
    }
    let rc = m.sign()?;
    if rc.rows != 6 || rc.receipt_rows != 6 || !rc.hash_ok || rc.summary.is_empty() {
        return Err(XC_SUMMARY_EMPTY);
    }
    if !matches!(m.sign(), Err(XC_ALREADY_SIGNED)) {
        return Err(XC_STATE_INVALID);
    }
    // —— 错误路径 1：接收域与冻结表不一致 ——
    let mut m2 = IfaceSignoff::new();
    let mut badown = signoffs.clone();
    badown[0].owner = 2; // 接口 1 冻结域为 1
    if !matches!(m2.feed_signoffs(&badown), Err(XC_OWNER_MISMATCH)) {
        return Err(XC_OWNER_MISMATCH);
    }
    // —— 错误路径 2：版本绑定不一致 ——
    let mut m3 = IfaceSignoff::new();
    let mut badver = signoffs.clone();
    badver[1].version = pack_version(1, 1, 0) + 1;
    if !matches!(m3.feed_signoffs(&badver), Err(XC_VERSION_MISMATCH)) {
        return Err(XC_VERSION_MISMATCH);
    }
    // —— 错误路径 3：哈希互证不一致 → 交叉红 / 签收红 ——
    let mut m4 = IfaceSignoff::new();
    m4.feed_signoffs(&signoffs)?;
    let mut badrec = receipts.clone();
    badrec[2].ack_hash ^= 0x1;
    m4.feed_receipts(&badrec)?;
    let (_, ok4) = m4.cross_check()?;
    if ok4 {
        return Err(XC_HASH_MISMATCH);
    }
    if !matches!(m4.sign(), Err(XC_HASH_MISMATCH)) {
        return Err(XC_HASH_MISMATCH);
    }
    // —— 错误路径 4：回执孤儿 / 域回执不一致 / 不足 6/6 ——
    let mut m5 = IfaceSignoff::new();
    m5.feed_signoffs(&signoffs)?;
    let mut orphan = receipts[..5].to_vec();
    orphan.push(ReceiptRow { iface: 9, ack_hash: 1, ack_domain: 1 });
    if !matches!(m5.feed_receipts(&orphan), Err(XC_RECEIPT_ORPHAN)) {
        return Err(XC_RECEIPT_ORPHAN);
    }
    let mut m6 = IfaceSignoff::new();
    m6.feed_signoffs(&signoffs)?;
    m6.feed_receipts(&receipts[..5].to_vec())?;
    if !matches!(m6.cross_check(), Err(XC_RECEIPT_SHORT)) {
        return Err(XC_RECEIPT_SHORT);
    }
    // —— 错误路径 5：域越界 / 接口越界 / 哈希为零 / 版本打包非法 / 重复 ——
    let mut m7 = IfaceSignoff::new();
    let mut bad1 = signoffs[..3].to_vec();
    bad1[0].owner = 9;
    if !matches!(m7.feed_signoffs(&bad1), Err(XC_OWNER_BAD)) {
        return Err(XC_OWNER_BAD);
    }
    let mut m8 = IfaceSignoff::new();
    let mut bad2 = signoffs[..3].to_vec();
    bad2[1].iface = 7;
    if !matches!(m8.feed_signoffs(&bad2), Err(XC_IFACE_BAD)) {
        return Err(XC_IFACE_BAD);
    }
    let mut m9 = IfaceSignoff::new();
    let mut bad3 = signoffs[..3].to_vec();
    bad3[2].receipt_hash = 0;
    if !matches!(m9.feed_signoffs(&bad3), Err(XC_HASH_ZERO)) {
        return Err(XC_HASH_ZERO);
    }
    let mut ma = IfaceSignoff::new();
    let mut bad4 = signoffs[..3].to_vec();
    bad4[0].version = pack_version(0, 1, 0);
    if !matches!(ma.feed_signoffs(&bad4), Err(XC_VER_PACK_BAD)) {
        return Err(XC_VER_PACK_BAD);
    }
    let mut mb = IfaceSignoff::new();
    let mut dup = signoffs[..3].to_vec();
    dup[2].iface = dup[0].iface;
    if !matches!(mb.feed_signoffs(&dup), Err(XC_DUP_IFACE)) {
        return Err(XC_DUP_IFACE);
    }
    // —— 查询 API：冻结表只读视图 / 行可解引 ——
    if IfaceSignoff::frozen_version(3) != Some(pack_version(1, 0, 7)) {
        return Err(XC_VERSION_MISMATCH);
    }
    if IfaceSignoff::frozen_owner(5) != Some(5) || IfaceSignoff::frozen_owner(9).is_some() {
        return Err(XC_OWNER_BAD);
    }
    match m.row_of(4) {
        Some(r) if r.owner == 4 => {}
        _ => return Err(XC_OWNER_MISMATCH),
    }
    if m.row_of(9).is_some() {
        return Err(XC_DUP_IFACE);
    }
    // —— 扩展 API 面：未签收清单 / 域视图 / 账视图 ——
    if !m.pending_ifaces().is_empty() {
        return Err(XC_RECEIPT_SHORT);
    }
    let pend2 = m6.pending_ifaces();
    if pend2.len() != 1 || pend2[0] != 6 {
        return Err(XC_RECEIPT_SHORT);
    }
    let dv1 = m.domain_view(1)?;
    if dv1.len() != 2 || dv1[0] != 1 || dv1[1] != 6 {
        return Err(XC_OWNER_MISMATCH);
    }
    if !matches!(m.domain_view(9), Err(XC_OWNER_BAD)) {
        return Err(XC_OWNER_BAD);
    }
    let sv = m.signoff_view()?;
    if sv.len() != 6 {
        return Err(XC_EMPTY_FEED);
    }
    if !matches!(m6.signoff_view(), Err(XC_EMPTY_FEED)) {
        return Err(XC_EMPTY_FEED);
    }
    // —— 版本解包：接口 3 → (1,0,7) 与冻结表一致；越号 Err ——
    if m.version_of(3)? != (1, 0, 7) {
        return Err(XC_VERSION_MISMATCH);
    }
    if !matches!(m.version_of(9), Err(XC_DUP_IFACE)) {
        return Err(XC_DUP_IFACE);
    }
    Ok(0)
}
