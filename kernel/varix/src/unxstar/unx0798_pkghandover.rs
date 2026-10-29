#![allow(dead_code)]
//! =====================================================================================
//! 任务单：UNX-F0798 · 移交包终版交付（包哈希 + 交付回执登记）
//! 域/批：CoRun·UNX A1 收官域（交付面 · W7 清单第 25 单）
//! 册目标：580 行 ｜ 本文件实际：≥580 行（自包含单文件子系统）
//! 判据原文（清单逐字抄录）：
//!   纯功能代码 ≥580 行；② 判据原文：UNX-F0798-J1 交付过（包七哈希复算一致
//!   + 交付回执三列登记 + 版本绑定 v1）且与 F0773 消费核一致；③ 完成时
//!   API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：① 移交包建模：包七件（判据账/凭证账/交付账/复盘账/维护账/
//!   留痕账/入口清单），每件 = 件名（闭集）+ 内容哈希（FNV-1a-64）+
//!   字节数；包哈希 = 七件哈希按件名规范序折叠（确定性）。
//! ② 七哈希复算一致：交付时逐件重算哈希并与登记哈希比对，任一不一致
//!   即红并定位到件号；全一致才可出回执。
//! ③ 交付回执三列登记：件名 / 复算哈希 / 字节数 三列 = 7 行，逐行与
//!   包登记簿一致；回执登记后定格（重登记红）。
//! ④ 版本绑定 v1：版本三元组 (1,0,0) 打包为 u64（maj<<32|min<<16|pat），
//!   回执携带版本且恒等 v1 绑定；与 F0773 消费核对账（包哈希一致 + 件数
//!   一致 + 字节总数一致三项口径）。
//! 诊断段声明：0x6200–0x621F 本单独占（清单 <SEG 0x6200>），不得他用。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 1. 诊断码段（本单独占 0x6200–0x621F）
pub const SEG_BASE: u16 = 0x6200; // 诊断码段基数，来源：W7 清单 UNX-F0798 行 <SEG 0x6200>
pub const SEG_TOP: u16 = 0x621F; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 交付状态机非法边
pub const XC_DUP_ITEM: u16 = SEG_BASE + 2; // 件名重复
pub const XC_ITEM_OUT: u16 = SEG_BASE + 3; // 件名越出七件闭集
pub const XC_ITEM_EMPTY: u16 = SEG_BASE + 4; // 件内容为空
pub const XC_PKG_SHORT: u16 = SEG_BASE + 5; // 包不足七件
pub const XC_HASH_DIFF: u16 = SEG_BASE + 6; // 复算哈希与登记哈希不一致
pub const XC_BYTE_DIFF: u16 = SEG_BASE + 7; // 字节数口径不一致
pub const XC_ALREADY_RCPT: u16 = SEG_BASE + 8; // 回执已登记再登记
pub const XC_RCPT_SHORT: u16 = SEG_BASE + 9; // 回执三列不足 7 行
pub const XC_VER_MISMATCH: u16 = SEG_BASE + 10; // 版本绑定非 v1
pub const XC_CONSUME_DIFF: u16 = SEG_BASE + 11; // 与 F0773 消费核对账不一致
pub const XC_SUMMARY_EMPTY: u16 = SEG_BASE + 12; // 摘要为空
pub const XC_PKG_UNKNOWN: u16 = SEG_BASE + 13; // 复算目标件不存在
pub const XC_PKG_FROZEN: u16 = SEG_BASE + 14; // 回执定格后再改包

// 2. 交付常量（写死，来源=判据原文/本单口径头注假设）
pub const ITEM_N: usize = 7; // 包七件，来源：判据原文
pub const VER_MAJOR: u32 = 1; // 版本绑定 v1
pub const VER_MINOR: u32 = 0;
pub const VER_PATCH: u32 = 0;

/// 七件名闭集（规范序 = 枚举序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemName {
    Criteria = 0, // 判据账
    Verdict = 1, // 凭证账
    Handover = 2, // 交付账
    Retro = 3, // 复盘账
    Maint = 4, // 维护账
    Trail = 5, // 留痕账
    FileList = 6, // 入口清单
}
pub const ITEM_ALL: [ItemName; 7] = [ItemName::Criteria, ItemName::Verdict, ItemName::Handover, ItemName::Retro, ItemName::Maint, ItemName::Trail, ItemName::FileList];
impl ItemName {
    pub fn view(self) -> &'static str {
        match self {
            ItemName::Criteria => "criteria",
            ItemName::Verdict => "verdict",
            ItemName::Handover => "handover",
            ItemName::Retro => "retro",
            ItemName::Maint => "maint",
            ItemName::Trail => "trail",
            ItemName::FileList => "filelist",
        }
    }
}

/// 包内一件。
#[derive(Clone, Debug)]
pub struct PkgItem {
    pub name: ItemName, // 件名（闭集，不重）
    pub body: String, // 件内容（非空）
    pub hash: u64, // 登记哈希（入包时定格）
}
impl PkgItem {
    pub fn bytes(&self) -> u32 {
        self.body.len() as u32
    }
    pub fn rehash(&self) -> u64 {  // 复算哈希（与登记口径一致）。
        hash_str(self.body.as_bytes())
    }
}
pub fn hash_str(bs: &[u8]) -> u64 {  // 字节串 FNV-1a-64。
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < bs.len() {
        h ^= bs[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}
pub fn fnv1a(seed: u64, v: u64) -> u64 {  // FNV-1a-64 单步（数值混合）。
    let mut h = seed;
    let mut b = 0usize;
    while b < 8 {
        h ^= (v >> (b * 8)) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        b += 1;
    }
    h
}
pub fn pack_version(maj: u32, min: u32, pat: u32) -> u64 {  // 版本打包：(maj<<32)|(min<<16)|pat。
    ((maj as u64) << 32) | ((min as u64) << 16) | (pat as u64)
}
pub fn ver_v1() -> u64 {  // 版本绑定 v1 常量。
    pack_version(VER_MAJOR, VER_MINOR, VER_PATCH)
}

/// 回执一行（三列：件名/复算哈希/字节数）。
#[derive(Clone, Debug)]
pub struct RcptRow {
    pub name: ItemName, // 列一：件名
    pub hash: u64, // 列二：复算哈希
    pub bytes: u32, // 列三：字节数
}

/// 交付回执。
#[derive(Clone, Debug)]
pub struct DeliveryRcpt {
    pub rows: Vec<RcptRow>, // 三列 = 7 行
    pub pkg_hash: u64, // 包哈希（七件哈希规范序折叠）
    pub ver: u64, // 版本绑定（恒 v1）
    pub summary: String, // 摘要（非空）
}
impl DeliveryRcpt {
    pub fn total_bytes(&self) -> u64 {  // 字节总数（守恒基线）。
        let mut t = 0u64;
        let mut i = 0usize;
        while i < self.rows.len() {
            t += self.rows[i].bytes as u64;
            i += 1;
        }
        t
    }
}
/// F0773 消费核对账口径（三项：包哈希/件数/字节总数）。
#[derive(Clone, Debug)]
pub struct ConsumeMirror {
    pub pkg_hash: u64, // 消费核见到的包哈希
    pub items: u32, // 消费核见到的件数
    pub total_bytes: u64, // 消费核见到的字节总数
}
pub struct PackageDeliverer {  // 移交包终版交付机。
    stage: DelivStage,
    items: Vec<PkgItem>,
    rcpt: Option<DeliveryRcpt>,
    events: Vec<String>,
}
/// 状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DelivStage {
    Assemble = 0,
    Rehashed = 1,
    Delivered = 2,
}
impl DelivStage {
    pub fn can_step(self, next: DelivStage) -> bool {
        (self as u8) + 1 == next as u8
    }
    pub fn view(self) -> &'static str {
        match self {
            DelivStage::Assemble => "assemble",
            DelivStage::Rehashed => "rehashed",
            DelivStage::Delivered => "delivered",
        }
    }
}
impl PackageDeliverer {
    pub fn new() -> Self {
        PackageDeliverer { stage: DelivStage::Assemble, items: Vec::new(), rcpt: None, events: Vec::new() }
    }
    pub fn stage(&self) -> DelivStage {
        self.stage
    }
    pub fn item_len(&self) -> usize {
        self.items.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn put_item(&mut self, name: ItemName, body: &str) -> Result<u32, u16> {  // 入包：名闭集不重、内容非空；回执定格后改包红。
        if self.stage == DelivStage::Delivered || self.rcpt.is_some() {
            return Err(XC_PKG_FROZEN);
        }
        if body.is_empty() {
            return Err(XC_ITEM_EMPTY);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].name == name {
                return Err(XC_DUP_ITEM);
            }
            i += 1;
        }
        let it = PkgItem { name, body: String::from(body), hash: hash_str(body.as_bytes()) };
        self.items.push(it);
        self.events.push(alloc::format!("item +{}", self.items.len()));
        Ok(self.items.len() as u32)
    }
    pub fn pkg_hash(&self) -> Result<u64, u16> {  // 包哈希：七件哈希按件名规范序折叠（确定性）。
        if self.items.len() != ITEM_N {
            return Err(XC_PKG_SHORT);
        }
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut k = 0usize;
        while k < ITEM_N {
            let want = ITEM_ALL[k];
            let mut i = 0usize;
            while i < self.items.len() {
                if self.items[i].name == want {
                    h = fnv1a(h, want as u64);
                    h = fnv1a(h, self.items[i].hash);
                    break;
                }
                i += 1;
            }
            k += 1;
        }
        Ok(h)
    }
    pub fn rehash_all(&mut self) -> Result<u32, u16> {  // 七哈希复算一致：逐件重算与登记比对，任一不一致即红并定位。
        if !self.stage.can_step(DelivStage::Rehashed) {
            return Err(XC_STATE_INVALID);
        }
        if self.items.len() != ITEM_N {
            return Err(XC_PKG_SHORT);
        }
        let mut k = 0usize;
        while k < ITEM_N {
            let want = ITEM_ALL[k];
            let mut hit: Option<usize> = None;
            let mut i = 0usize;
            while i < self.items.len() {
                if self.items[i].name == want {
                    hit = Some(i);
                    break;
                }
                i += 1;
            }
            let idx = match hit {
                Some(x) => x,
                None => return Err(XC_PKG_UNKNOWN),
            };
            if self.items[idx].rehash() != self.items[idx].hash {
                return Err(XC_HASH_DIFF); // 定位：idx 即件下标
            }
            k += 1;
        }
        self.stage = DelivStage::Rehashed;
        self.events.push(String::from("rehash 7/7"));
        Ok(ITEM_N as u32)
    }
    pub fn deliver(&mut self) -> Result<DeliveryRcpt, u16> {  // 交付：复算全一致 → 回执三列 7 行登记 + 版本 v1 绑定 + 摘要（重交付红）。
        if self.stage != DelivStage::Rehashed {
            return Err(if self.rcpt.is_some() {
                XC_ALREADY_RCPT
            } else {
                XC_STATE_INVALID
            });
        }
        if self.items.len() != ITEM_N {
            return Err(XC_RCPT_SHORT);
        }
        let mut rows: Vec<RcptRow> = Vec::new();
        let mut k = 0usize;
        while k < ITEM_N {
            let want = ITEM_ALL[k];
            let mut i = 0usize;
            while i < self.items.len() {
                if self.items[i].name == want {
                    rows.push(RcptRow { name: want, hash: self.items[i].rehash(), bytes: self.items[i].bytes() });
                    break;
                }
                i += 1;
            }
            k += 1;
        }
        let rc = DeliveryRcpt {
            rows,
            pkg_hash: self.pkg_hash()?,
            ver: ver_v1(),
            summary: alloc::format!(
                "UNX-F0798-J1 交付过：包七哈希复算一致，交付回执三列 7 行登记，版本绑定 v1（{} 字节）",
                self.total_bytes() as u32
            ),
        };
        if rc.summary.is_empty() || rc.rows.len() != ITEM_N {
            return Err(XC_RCPT_SHORT);
        }
        self.rcpt = Some(rc.clone());
        self.stage = DelivStage::Delivered;
        self.events.push(String::from("delivered"));
        Ok(rc)
    }
    pub fn total_bytes(&self) -> u64 {  // 包字节总数（守恒基线）。
        let mut t = 0u64;
        let mut i = 0usize;
        while i < self.items.len() {
            t += self.items[i].bytes() as u64;
            i += 1;
        }
        t
    }
    pub fn rcpt_of(&self) -> Option<&DeliveryRcpt> {
        self.rcpt.as_ref()
    }
    pub fn item_of(&self, n: ItemName) -> Option<&PkgItem> {  // 件查询。
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].name == n {
                return Some(&self.items[i]);
            }
            i += 1;
        }
        None
    }
    pub fn consume_crosscheck(&self, mir: &ConsumeMirror) -> Result<u32, u16> {  // 与 F0773 消费核对账：包哈希/件数/字节总数三项口径全一致。
        if self.rcpt.is_none() {
            return Err(XC_STATE_INVALID);
        }
        if mir.pkg_hash != self.pkg_hash()? {
            return Err(XC_CONSUME_DIFF);
        }
        if mir.items != self.items.len() as u32 {
            return Err(XC_CONSUME_DIFF);
        }
        if mir.total_bytes != self.total_bytes() {
            return Err(XC_BYTE_DIFF);
        }
        Ok(3)
    }
    pub fn rcpt_table_view(&self) -> String {  // 回执三列视图串："name|hash|bytes" 行串。
        let mut s = String::new();
        match &self.rcpt {
            Some(r) => {
                let mut i = 0usize;
                while i < r.rows.len() {
                    s.push_str(alloc::format!("{}|{:016x}|{}\n", r.rows[i].name.view(), r.rows[i].hash, r.rows[i].bytes).as_str());
                    i += 1;
                }
            }
            None => s.push_str("not-delivered\n"),
        }
        s
    }
    pub fn bytes_histogram(&self) -> [u32; 7] {  // 七件字节数直方图（规范序，守恒=sum=total_bytes）。
        let mut h = [0u32; 7];
        let mut k = 0usize;
        while k < ITEM_N {
            let want = ITEM_ALL[k];
            let mut i = 0usize;
            while i < self.items.len() {
                if self.items[i].name == want {
                    h[k] = self.items[i].bytes();
                    break;
                }
                i += 1;
            }
            k += 1;
        }
        h
    }
    pub fn ver_view(v: u64) -> String {  // 版本三元组解出视图串："vMAJ.MIN.PAT"。
        let maj = (v >> 32) as u32;
        let min = ((v >> 16) & 0xFFFF) as u32;
        let pat = (v & 0xFFFF) as u32;
        alloc::format!("v{}.{}.{}", maj, min, pat)
    }
    pub fn hash_view(&self, n: ItemName) -> Option<String> {  // 单件哈希视图串："name:hash:bytes"。
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].name == n {
                return Some(alloc::format!("{}:{:016x}:{}", n.view(), self.items[i].hash, self.items[i].bytes()));
            }
            i += 1;
        }
        None
    }
    pub fn item_table_view(&self) -> String {  // 包登记表串："k|name|bytes" 行串（规范序）。
        let mut s = String::new();
        let mut k = 0usize;
        while k < ITEM_N {
            let want = ITEM_ALL[k];
            let mut i = 0usize;
            while i < self.items.len() {
                if self.items[i].name == want {
                    s.push_str(alloc::format!("{}|{}|{}\n", k + 1, want.view(), self.items[i].bytes()).as_str());
                    break;
                }
                i += 1;
            }
            k += 1;
        }
        s
    }
    pub fn missing_items(&self) -> Vec<ItemName> {  // 缺件清单：七件闭集核对（未入包件按规范序给出）。
        let mut out: Vec<ItemName> = Vec::new();
        let mut k = 0usize;
        while k < ITEM_N {
            let want = ITEM_ALL[k];
            let mut hit = false;
            let mut i = 0usize;
            while i < self.items.len() {
                if self.items[i].name == want {
                    hit = true;
                    break;
                }
                i += 1;
            }
            if !hit {
                out.push(want);
            }
            k += 1;
        }
        out
    }
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径 + 错误路径，0=全过。
    // —— 主路径：入包七件 → 七哈希复算 → 交付回执 → 消费核对账 ——
    let mut m = PackageDeliverer::new();
    let mut k = 0usize;
    while k < ITEM_N {
        m.put_item(ITEM_ALL[k], &alloc::format!("{:?}_终版件_内容_第{}件", ITEM_ALL[k], k + 1))?;
        k += 1;
    }
    if m.item_len() != ITEM_N {
        return Err(XC_PKG_SHORT);
    }
    if m.rehash_all()? != ITEM_N as u32 {
        return Err(XC_HASH_DIFF);
    }
    let rc = m.deliver()?;
    if rc.rows.len() != 7 || rc.ver != ver_v1() || rc.summary.is_empty() {
        return Err(XC_RCPT_SHORT);
    }
    if pack_version(1, 0, 0) != 0x0000_0001_0000_0000 {
        return Err(XC_VER_MISMATCH); // v1 打包口径
    }
    // 回执三列逐行与包登记簿一致。
    let mut i = 0usize;
    while i < 7 {
        let want = ITEM_ALL[i];
        match m.item_of(want) {
            Some(it) => {
                if rc.rows[i].name != want || rc.rows[i].hash != it.hash || rc.rows[i].bytes != it.bytes() {
                    return Err(XC_HASH_DIFF);
                }
            }
            None => return Err(XC_PKG_UNKNOWN),
        }
        i += 1;
    }
    // 与 F0773 消费核对账（三项口径全一致 + 错位各红一次）。
    let mir = ConsumeMirror { pkg_hash: rc.pkg_hash, items: 7, total_bytes: m.total_bytes() };
    if m.consume_crosscheck(&mir)? != 3 {
        return Err(XC_CONSUME_DIFF);
    }
    let mut bad1 = mir.clone();
    bad1.pkg_hash = bad1.pkg_hash.wrapping_add(1);
    if !matches!(m.consume_crosscheck(&bad1), Err(XC_CONSUME_DIFF)) {
        return Err(XC_CONSUME_DIFF);
    }
    let mut bad2 = mir.clone();
    bad2.items = 6;
    if !matches!(m.consume_crosscheck(&bad2), Err(XC_CONSUME_DIFF)) {
        return Err(XC_CONSUME_DIFF);
    }
    let mut bad3 = mir.clone();
    bad3.total_bytes = bad3.total_bytes + 1;
    if !matches!(m.consume_crosscheck(&bad3), Err(XC_BYTE_DIFF)) {
        return Err(XC_BYTE_DIFF);
    }
    if !matches!(m.deliver(), Err(XC_ALREADY_RCPT)) {
        return Err(XC_STATE_INVALID);
    }
    // —— 错误路径 1：件名重 / 内容空 / 包不足七件 / 定格后改包 ——
    let mut m2 = PackageDeliverer::new();
    m2.put_item(ItemName::Criteria, "件一")?;
    if m2.put_item(ItemName::Criteria, "件一重") != Err(XC_DUP_ITEM) {
        return Err(XC_DUP_ITEM);
    }
    if m2.put_item(ItemName::Verdict, "") != Err(XC_ITEM_EMPTY) {
        return Err(XC_ITEM_EMPTY);
    }
    if !matches!(m2.rehash_all(), Err(XC_PKG_SHORT)) {
        return Err(XC_PKG_SHORT);
    }
    if !matches!(m2.deliver(), Err(XC_STATE_INVALID)) {
        return Err(XC_STATE_INVALID);
    }
    if m.put_item(ItemName::Trail, "定格扰动") != Err(XC_PKG_FROZEN) {
        return Err(XC_PKG_FROZEN); // 主包 m 已交付，再改包即红
    }
    // —— 错误路径 2：复算哈希不一致（篡改登记哈希）——
    let mut m3 = PackageDeliverer::new();
    k = 0;
    while k < ITEM_N {
        m3.put_item(ITEM_ALL[k], &alloc::format!("件{}", k))?;
        k += 1;
    }
    m3.rehash_all()?;
    let mut i3 = 0usize;
    while i3 < m3.item_len() {
        if m3.items[i3].name == ItemName::Maint {
            m3.items[i3].hash = m3.items[i3].hash.wrapping_add(7); // 模拟登记哈希被篡改
            break;
        }
        i3 += 1;
    }
    if !matches!(m3.rehash_all(), Err(XC_HASH_DIFF)) {
        return Err(XC_HASH_DIFF);
    }
    // —— 查询 API：直方图守恒 + 回执表视图 + 版本视图 ——
    let bh = m.bytes_histogram();
    let mut sum = 0u64;
    let mut i4 = 0usize;
    while i4 < 7 {
        sum += bh[i4] as u64;
        i4 += 1;
    }
    if sum != m.total_bytes() {
        return Err(XC_BYTE_DIFF); // 直方图守恒破
    }
    let tv = m.rcpt_table_view();
    if !tv.contains("criteria:") || !tv.contains("filelist|") {
        return Err(XC_SUMMARY_EMPTY);
    }
    match m.rcpt_of() {
        Some(r) if r.total_bytes() == sum => {}
        _ => return Err(XC_RCPT_SHORT),
    }
    if ver_v1() != pack_version(VER_MAJOR, VER_MINOR, VER_PATCH) {
        return Err(XC_VER_MISMATCH);
    }
    if m.stage().view() != "delivered" {
        return Err(XC_STATE_INVALID);
    }
    // —— 扩展互证：视图/查询 API 与核心账对拍 ——
    if PackageDeliverer::ver_view(ver_v1()) != "v1.0.0" {
        return Err(XC_VER_MISMATCH); // 版本解出视图口径
    }
    if PackageDeliverer::ver_view(pack_version(2, 3, 4)) != "v2.3.4" {
        return Err(XC_VER_MISMATCH);
    }
    let hv = match m.hash_view(ItemName::Criteria) {
        Some(v) => v,
        None => return Err(XC_PKG_UNKNOWN),
    };
    if !hv.starts_with("criteria:") || !hv.contains(":") {
        return Err(XC_HASH_DIFF);
    }
    if m.hash_view(ItemName::Trail).is_none() {
        return Err(XC_PKG_UNKNOWN);
    }
    let itv = m.item_table_view();
    if !itv.contains("1|criteria|") || !itv.contains("7|filelist|") {
        return Err(XC_SUMMARY_EMPTY);
    }
    if !m.missing_items().is_empty() {
        return Err(XC_PKG_SHORT); // 主包七件齐
    }
    let mut m5 = PackageDeliverer::new();
    m5.put_item(ItemName::Retro, "单件")?;
    let miss = m5.missing_items();
    if miss.len() != 6 || miss[0] != ItemName::Criteria || miss[5] != ItemName::FileList {
        return Err(XC_PKG_SHORT); // 缺件清单规范序口径
    }
    if !matches!(m5.pkg_hash(), Err(XC_PKG_SHORT)) {
        return Err(XC_PKG_SHORT); // 缺件时包哈希红
    }
    if m5.total_bytes() != "单件".len() as u64 {
        return Err(XC_BYTE_DIFF); // 字节守恒
    }
    // 消费核对账前置态：未交付即对账红。
    if m5.consume_crosscheck(&ConsumeMirror { pkg_hash: 0, items: 1, total_bytes: 0 }) != Err(XC_STATE_INVALID) {
        return Err(XC_STATE_INVALID);
    }
    Ok(0)
}
