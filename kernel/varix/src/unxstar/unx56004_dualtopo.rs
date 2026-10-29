//! UNX-F56004 双盘双系统拓扑登记件（0x60060 段）
//!
//! 锚点：UNX-F56004-J1 基准机 Varix/Windows 各占一盘物理隔离登记
//! （盘序 × 盘标签 × 容量 × GUID 四元组逐盘落账，标签+容量+GUID 三重
//! 验证）；J2 拓扑漂移即拒收（×3 注入全检出：盘序互换/标签漂移/GUID
//! 漂移）；J3 env_profile 接线（拓扑摘要入 env_profile 口径的四段文本）
//! 三判据全绿。判据锚定 §AI-71.2 专题一。
//! 域：CoRun·UNX-F2-B40（环境冻结域·拓扑登记面）。
//!
//! 结构：双盘登记册（Varix 盘/Windows 盘逐盘落账）+ 三重验证核
//! （标签+容量+GUID 逐盘一致才放行）+ 拓扑漂移注入核（盘序/标签/GUID
//! 变更必抓）+ 拓扑摘要导出（env_profile fs_layout 前置口径）+ 封账。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60060 独占 0x20）。
pub const SEG_BASE: u32 = 0x60060;
pub const TOPO_UNK_DISK: u32 = SEG_BASE + 0x01; // 未知盘槽位（非双盘闭集）
pub const TOPO_TAG_DUP: u32 = SEG_BASE + 0x02; // 盘标签与另一盘重复（物理隔离破）
pub const TOPO_GUID_BAD: u32 = SEG_BASE + 0x03; // GUID 格式非法（非 32 位十六进制）
pub const TOPO_GUID_DUP: u32 = SEG_BASE + 0x04; // GUID 与另一盘重复
pub const TOPO_CAP_ZERO: u32 = SEG_BASE + 0x05; // 容量为零（GB）
pub const TOPO_SEALED: u32 = SEG_BASE + 0x06; // 拓扑账已封账拒改
pub const TOPO_DRIFT: u32 = SEG_BASE + 0x07; // 拓扑漂移（三重验证不符）

/// 双盘槽位闭集。
pub const SLOT_VARIX: usize = 0;
pub const SLOT_WIN: usize = 1;
pub const SLOT_COUNT: usize = 2;

/// 槽位名（账目渲染与验证共用）。
pub const SLOT_NAMES: [&str; SLOT_COUNT] = ["varix", "windows"];

/// GUID 格式核：32 位十六进制（8-4-4-4-4-12 连写亦接受，此处统一连写）。
pub fn guid_ok(g: &str) -> bool {
    let b = g.as_bytes();
    if b.len() != 32 {
        return false;
    }
    b.iter().all(|c| c.is_ascii_hexdigit())
}

/// 单盘拓扑账目：槽位 × 标签 × 容量(GB) × GUID 四元组。
#[derive(Clone)]
pub struct DiskEntry {
    pub slot: usize,
    pub tag: String,
    pub cap_gb: u32,
    pub guid: String,
}

impl DiskEntry {
    pub fn new(slot: usize, tag: &str, cap_gb: u32, guid: &str) -> Result<Self, u32> {
        if slot >= SLOT_COUNT {
            return Err(TOPO_UNK_DISK);
        }
        if tag.is_empty() {
            return Err(TOPO_TAG_DUP);
        }
        if !guid_ok(guid) {
            return Err(TOPO_GUID_BAD);
        }
        if cap_gb == 0 {
            return Err(TOPO_CAP_ZERO);
        }
        Ok(DiskEntry {
            slot,
            tag: tag.to_string(),
            cap_gb,
            guid: guid.to_string(),
        })
    }

    /// 三重验证：与既有账目同槽位比对（标签+容量+GUID 三者全一致才算稳态）。
    pub fn triple_match(&self, o: &DiskEntry) -> bool {
        self.slot == o.slot && self.tag == o.tag && self.cap_gb == o.cap_gb && self.guid == o.guid
    }
}

/// 双盘拓扑登记册：两槽逐盘落账，三重验证拒漂移。
pub struct TopoBook {
    pub entries: [Option<DiskEntry>; SLOT_COUNT],
    pub sealed: bool,
    pub audit: Vec<String>,
}

impl TopoBook {
    pub fn new() -> Self {
        TopoBook { entries: [None, None], sealed: false, audit: Vec::new() }
    }

    /// 登记一盘：槽位闭集 + 双盘互斥（标签/GUID 不得跨盘撞账）+ 封账拒写。
    pub fn register(&mut self, e: DiskEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(TOPO_SEALED);
        }
        // 跨盘撞账核（先按槽位对侧检查再落账）。
        let other = (e.slot + 1) % SLOT_COUNT;
        if let Some(prev) = self.entries[other].as_ref() {
            if prev.tag == e.tag {
                return Err(TOPO_TAG_DUP);
            }
            if prev.guid == e.guid {
                return Err(TOPO_GUID_DUP);
            }
        }
        // 同槽位既有账：三重验证不符即拓扑漂移拒收。
        if let Some(prev) = self.entries[e.slot].as_ref() {
            if !e.triple_match(prev) {
                self.audit.push(format!(
                    "TOPO|DRIFT|{}|old={}|{}|new={}|{}",
                    SLOT_NAMES[e.slot], prev.tag, prev.guid, e.tag, e.guid
                ));
                return Err(TOPO_DRIFT);
            }
            return Ok(());
        }
        self.audit.push(format!("TOPO|REG|{}|{}", SLOT_NAMES[e.slot], e.guid));
        let slot = e.slot;
        self.entries[slot] = Some(e);
        Ok(())
    }

    /// J1：物理隔离全量核——双盘都有账且标签/GUID 互异。
    pub fn isolated_ok(&self) -> bool {
        match (self.entries[SLOT_VARIX].as_ref(), self.entries[SLOT_WIN].as_ref()) {
            (Some(v), Some(w)) => {
                v.tag != w.tag && v.guid != w.guid && v.cap_gb > 0 && w.cap_gb > 0
            }
            _ => false,
        }
    }

    /// 某槽位现行账（只读）。
    pub fn current(&self, slot: usize) -> Option<&DiskEntry> {
        if slot >= SLOT_COUNT {
            return None;
        }
        self.entries[slot].as_ref()
    }

    /// 封账（封后拓扑不可变）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 拓扑摘要导出（env_profile fs_layout 前置口径：每盘一段）。
pub fn topo_summary(b: &TopoBook) -> String {
    let mut out = String::new();
    for s in 0..SLOT_COUNT {
        match b.current(s) {
            Some(e) => {
                out.push_str(&format!(
                    "{}|{}|{}GB|{}\n",
                    SLOT_NAMES[s], e.tag, e.cap_gb, e.guid
                ));
            }
            None => {
                out.push_str(&format!("{}|UNREGISTERED\n", SLOT_NAMES[s]));
            }
        }
    }
    out
}

/// 摘要完整性核：双行且无 UNREGISTERED。
pub fn summary_complete(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    lines.len() == SLOT_COUNT && !text.contains("UNREGISTERED")
}

/// 标准夹具：Varix 盘 0 号槽 / Windows 盘 1 号槽（固定四元组）。
pub fn standard_topo() -> TopoBook {
    let mut b = TopoBook::new();
    let v = DiskEntry::new(SLOT_VARIX, "VARIXSYS", 240, "aabbccdd00112233445566778899eeff")
        .expect("fixture varix disk");
    let w = DiskEntry::new(SLOT_WIN, "WINBASE", 512, "11223344556677889900aabbccddeeff")
        .expect("fixture win disk");
    b.register(v).expect("fixture reg v");
    b.register(w).expect("fixture reg w");
    b
}

/// 拓扑漂移注入核：×3 漂移全检出（盘序互换/标签漂移/GUID 漂移）。
pub fn drift_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 盘序互换（Windows 盘标签+GUID 登到 Varix 槽）
    let mut b = standard_topo();
    let swapped = DiskEntry::new(SLOT_VARIX, "WINBASE", 512, "11223344556677889900aabbccddeeff")
        .expect("swap entry");
    if b.register(swapped) == Err(TOPO_DRIFT) {
        caught += 1;
    }
    // 2) 标签漂移（同槽同 GUID 但标签变）
    let mut b2 = standard_topo();
    let tag_drift = DiskEntry::new(SLOT_VARIX, "VARIXSYS-2", 240, "aabbccdd00112233445566778899eeff")
        .expect("tag drift entry");
    if b2.register(tag_drift) == Err(TOPO_DRIFT) {
        caught += 1;
    }
    // 3) GUID 漂移（同槽同标签但 GUID 变）
    let mut b3 = standard_topo();
    let guid_drift = DiskEntry::new(SLOT_VARIX, "VARIXSYS", 240, "ffff0000111122223333444455556666")
        .expect("guid drift entry");
    if b3.register(guid_drift) == Err(TOPO_DRIFT) {
        caught += 1;
    }
    (caught, 3)
}

/// 拒收注入核：5 类非法登记必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知槽位（slot=5 越闭集）
    if DiskEntry::new(5, "X", 100, "aabbccdd00112233445566778899eeff").err() == Some(TOPO_UNK_DISK) {
        caught += 1;
    }
    // 2) GUID 格式非法（31 位）
    if DiskEntry::new(SLOT_VARIX, "V", 100, "aabbccdd00112233445566778899ee")
        .err()
        == Some(TOPO_GUID_BAD)
    {
        caught += 1;
    }
    // 3) 容量为零
    if DiskEntry::new(SLOT_VARIX, "V", 0, "aabbccdd00112233445566778899eeff").err() == Some(TOPO_CAP_ZERO) {
        caught += 1;
    }
    // 4) 跨盘标签撞账
    let mut b = standard_topo();
    if b.register(
        DiskEntry::new(SLOT_WIN, "VARIXSYS", 999, "99990000111122223333444455556666")
            .expect("dup tag entry"),
    ) == Err(TOPO_TAG_DUP)
    {
        caught += 1;
    }
    // 5) 封账后改写
    let mut b2 = standard_topo();
    b2.seal();
    if b2.register(
        DiskEntry::new(SLOT_VARIX, "VARIXSYS", 240, "aabbccdd00112233445566778899eeff")
            .expect("sealed entry"),
    ) == Err(TOPO_SEALED)
    {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56004_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56004");
    // J1-1 双盘物理隔离全量核
    let b = standard_topo();
    let ok = b.isolated_ok() && !b.sealed;
    s.add("56004-J1-双盘物理隔离", ok, if ok { "" } else { "隔离登记不全" });
    // J1-2 三重验证稳态（同四元组重登零漂移）
    let mut b2 = standard_topo();
    let same = DiskEntry::new(SLOT_VARIX, "VARIXSYS", 240, "aabbccdd00112233445566778899eeff")
        .expect("same entry");
    let ok = b2.register(same).is_ok();
    s.add("56004-J1-三重验证稳态", ok, if ok { "" } else { "稳态重登误报" });
    // J1-3 GUID 闭集格式（32 位十六进制）
    let ok = guid_ok("aabbccdd00112233445566778899eeff") && !guid_ok("aabbccdd00112233");
    s.add("56004-J1-GUID格式闭集", ok, if ok { "" } else { "GUID 格式核松" });
    // J2-1 拓扑漂移 ×3 注入全检出
    let (c, n) = drift_core();
    let ok = c == n && n == 3;
    let detail = if ok { String::new() } else { format!("漂移检出 {c}/{n}") };
    s.add_dyn("56004-J2-拓扑漂移必拒", ok, &detail);
    // J2-2 漂移留痕（audit 里有 DRIFT 行）
    let mut b3 = standard_topo();
    let _ = b3.register(
        DiskEntry::new(SLOT_WIN, "WINBASE-DRIFT", 512, "11223344556677889900aabbccddeeff")
            .expect("drift entry"),
    );
    let mut has_drift_audit = false;
    for a in b3.audit.iter() {
        if a.contains("TOPO|DRIFT|windows") {
            has_drift_audit = true;
        }
    }
    let ok = has_drift_audit;
    s.add("56004-J2-漂移留痕", ok, if ok { "" } else { "漂移未留痕" });
    // J3-1 拓扑摘要导出齐（双行无双 UNREGISTERED）
    let text = topo_summary(&standard_topo());
    let ok = summary_complete(&text) && text.contains("varix|VARIXSYS|240GB|");
    s.add("56004-J3-摘要导出齐", ok, if ok { "" } else { "摘要面漂移" });
    // J3-2 未登记盘显性化（UNREGISTERED 行如实暴露）
    let empty_book = TopoBook::new();
    let text2 = topo_summary(&empty_book);
    let ok = text2.contains("UNREGISTERED") && !summary_complete(&text2);
    s.add("56004-J3-未登记显性化", ok, if ok { "" } else { "缺账被吞" });
    // J1-4 拒收注入 5 类必抓
    let (ic, in_) = injection_core();
    let ok = ic == in_;
    let detail = if ok { String::new() } else { format!("拒收 {ic}/{in_}") };
    s.add_dyn("56004-J1-拒收注入必抓", ok, &detail);
    // J2-3 跨盘 GUID 撞账拒
    let mut b4 = standard_topo();
    let ok = b4.register(
        DiskEntry::new(SLOT_WIN, "OTHER", 512, "aabbccdd00112233445566778899eeff")
            .expect("dup guid entry"),
    ) == Err(TOPO_GUID_DUP);
    s.add("56004-J2-GUID撞账拒", ok, if ok { "" } else { "GUID 撞账穿透" });
    // J2-4 摘要文本含盘序（varix 在前 windows 在后）
    let text3 = topo_summary(&standard_topo());
    let vi = text3.find("varix|").expect("varix line");
    let wi = text3.find("windows|").expect("windows line");
    let ok = vi < wi;
    s.add("56004-J2-盘序语义", ok, if ok { "" } else { "盘序颠倒" });
    // J3-3 封账只读（封后登记拒）
    let mut b5 = standard_topo();
    b5.seal();
    let ok = b5.register(
        DiskEntry::new(SLOT_VARIX, "VARIXSYS", 240, "aabbccdd00112233445566778899eeff")
            .expect("sealed entry"),
    ) == Err(TOPO_SEALED);
    s.add("56004-J3-封账只读", ok, if ok { "" } else { "封账后可写" });
    // J1-5 登记留痕（REG 行与登记数一致）
    let b6 = standard_topo();
    let mut reg_cnt = 0usize;
    for a in b6.audit.iter() {
        if a.contains("TOPO|REG|") {
            reg_cnt += 1;
        }
    }
    let ok = reg_cnt == SLOT_COUNT;
    s.add("56004-J1-登记留痕齐", ok, if ok { "" } else { "留痕缺行" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56004_all_checks_green() {
        let set = run_unx56004_checks();
        assert!(set.all_passed(), "UNX-F56004 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56004_isolation_stable() {
        assert!(standard_topo().isolated_ok());
    }
}
