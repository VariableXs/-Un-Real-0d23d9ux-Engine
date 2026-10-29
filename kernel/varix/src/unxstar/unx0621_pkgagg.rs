//! unx0621_pkgagg —— UNX-F0621 域交接包聚合（12 包总装）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0621 · 域交接包聚合（12 包总装） · 目标 ≥260 行
//! 判据原文摘录：12 交接包聚合账齐（F0349/F0378/F0413/F0439/F0453/
//!   F0476/F0497/F0518/F0538/F0559/F0593/F0617 全登记 12/12），聚合
//!   索引表冻结。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 12 包判据号闭集写死（来源：判据原文逐号抄录），包号 0..11
//!      与判据号一一绑定，号外包拒收。
//!   2. 总装守恒恒等式：已登记包数 + 空包槽数 = 12；聚合放行前置
//!      = 全满。
//!   3. 每包带内容校验和（FNV-1a 64），聚合总校验和 = 12 包校验和
//!      按位异或（单包漂移即总装漂移）。
//!   4. 聚合索引表冻结语义：冻结后整表只读，再登记/再改包即拦截。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x4F60 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x4F60 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4F60;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x4F6F;

/// XcCode：包号越界。
pub const XC_BAD_PKG: u16 = SEG_BASE + 1; // 0x4F61 包号 0..12 闭集外
/// XcCode：判据号与包号绑定不符。
pub const XC_CRIT_MISMATCH: u16 = SEG_BASE + 2; // 0x4F62 包槽绑错判据号
/// XcCode：重复登记。
pub const XC_DUP_PKG: u16 = SEG_BASE + 3; // 0x4F63 一槽一包不许重登
/// XcCode：包未齐禁聚合。
pub const XC_PKG_SHORT: u16 = SEG_BASE + 4; // 0x4F64 12/12 未齐
/// XcCode：已冻结只读。
pub const XC_FROZEN_RDONLY: u16 = SEG_BASE + 5; // 0x4F65 冻结后写拦截
/// XcCode：包校验和不符。
pub const XC_PKG_CSUM: u16 = SEG_BASE + 6; // 0x4F66 内容校验和不中

/// 12 包判据号闭集（来源：判据原文逐号抄录，包号升序绑定）。
pub const PKG_CRIT_IDS: [u16; 12] = [
    349, 378, 413, 439, 453, 476, 497, 518, 538, 559, 593, 617,
];

/// 12 包名（与判据号同序）。
pub const PKG_NAMES: [&str; 12] = [
    "时钟域交接包",
    "内存域交接包",
    "计时交接包",
    "中断域交接包",
    "ACPI 域交接包",
    "PCI 域交接包",
    "显示域交接包",
    "输入域交接包",
    "ktest 域交接包",
    "日志域交接包",
    "参数域交接包",
    "initrd 域交接包",
];

/// 包槽容量。
pub const PKG_N: usize = 12;

/// 单个交接包。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoffPkg {
    /// 包号（0..12）。
    pub pkg_no: usize,
    /// 绑定判据号。
    pub crit_id: u16,
    /// 包名。
    pub name: String,
    /// 包内容字节数（账面行数口径）。
    pub payload_len: u32,
    /// 内容校验和（FNV-1a 64 对包名+长度算得）。
    pub csum: u64,
}

impl HandoffPkg {
    /// 构包并即算校验和。
    pub fn build(pkg_no: usize, crit_id: u16, name: &str, payload_len: u32) -> Self {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in name.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^= payload_len as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= crit_id as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        HandoffPkg {
            pkg_no,
            crit_id,
            name: name.to_string(),
            payload_len,
            csum: h,
        }
    }

    /// 校验和自证：重算 = 存值。
    pub fn csum_ok(&self) -> bool {
        let re = HandoffPkg::build(self.pkg_no, self.crit_id, &self.name, self.payload_len);
        re.csum == self.csum
    }
}

/// 聚合总装账本：定容 12 槽 + 冻结位。
#[derive(Clone, Debug)]
pub struct PkgAggregator {
    /// 12 包槽（None = 未登记）。
    slots: [Option<HandoffPkg>; PKG_N],
    /// 冻结位。
    frozen: bool,
}

impl PkgAggregator {
    /// 空总装台。
    pub fn new() -> Self {
        PkgAggregator {
            slots: [
                None, None, None, None, None, None, None, None, None, None, None, None,
            ],
            frozen: false,
        }
    }

    /// 已登记包数。
    pub fn registered(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// 守恒：登记数 + 空槽数 = 12。
    pub fn conserve_ok(&self) -> bool {
        self.registered() + self.slots.iter().filter(|s| s.is_none()).count() == PKG_N
    }

    /// 是否冻结。
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 登记一包（包号闭集 + 判据号绑定 + 去重 + 冻结拦截）。
    pub fn enroll(&mut self, pkg: HandoffPkg) -> Result<(), u16> {
        if self.frozen {
            return Err(XC_FROZEN_RDONLY);
        }
        if pkg.pkg_no >= PKG_N {
            return Err(XC_BAD_PKG);
        }
        if pkg.crit_id != PKG_CRIT_IDS[pkg.pkg_no] {
            return Err(XC_CRIT_MISMATCH);
        }
        if !pkg.csum_ok() {
            return Err(XC_PKG_CSUM);
        }
        if self.slots[pkg.pkg_no].is_some() {
            return Err(XC_DUP_PKG);
        }
        let no = pkg.pkg_no;
        self.slots[no] = Some(pkg);
        Ok(())
    }

    /// 聚合放行：12/12 全齐 + 逐包校验和自证 + 总装校验和落账。
    pub fn aggregate(&self) -> Result<u64, u16> {
        if self.registered() != PKG_N {
            return Err(XC_PKG_SHORT);
        }
        let mut total: u64 = 0;
        for i in 0..PKG_N {
            match &self.slots[i] {
                Some(p) => {
                    if !p.csum_ok() {
                        return Err(XC_PKG_CSUM);
                    }
                    total ^= p.csum;
                }
                None => return Err(XC_PKG_SHORT),
            }
        }
        Ok(total)
    }

    /// 冻结聚合索引表：全齐才可冻；冻后只读。
    pub fn freeze(&mut self) -> Result<u64, u16> {
        if self.frozen {
            return Err(XC_FROZEN_RDONLY);
        }
        let total = self.aggregate()?;
        self.frozen = true;
        Ok(total)
    }

    /// 按包号取包。
    pub fn pkg_at(&self, pkg_no: usize) -> Option<&HandoffPkg> {
        if pkg_no >= PKG_N {
            return None;
        }
        self.slots[pkg_no].as_ref()
    }

    /// 聚合索引快照（包号、判据号、长度三元组，按包号升序）。
    pub fn index_rows(&self) -> Vec<(usize, u16, u32)> {
        let mut rows = Vec::new();
        for i in 0..PKG_N {
            if let Some(p) = &self.slots[i] {
                rows.push((p.pkg_no, p.crit_id, p.payload_len));
            }
        }
        rows
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "12 包总装 · 登记 {}/{} · 冻结={} · 守恒={}",
            self.registered(),
            PKG_N,
            self.frozen,
            self.conserve_ok()
        );
        for r in self.index_rows() {
            s.push_str(&alloc::format!("\n  包{} 判据F0{} 长度{}", r.0, r.1, r.2));
        }
        s
    }

    /// 总装指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.report().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 12 包总装器：按闭集名册把 12 包全量登齐（确定性长度 = 包号*7+3）。
pub fn assemble_all() -> Result<(PkgAggregator, u64), u16> {
    let mut agg = PkgAggregator::new();
    for i in 0..PKG_N {
        let pkg = HandoffPkg::build(i, PKG_CRIT_IDS[i], PKG_NAMES[i], (i as u32) * 7 + 3);
        agg.enroll(pkg)?;
    }
    let total = agg.freeze()?;
    Ok((agg, total))
}

/// 自例程：主路径（总装→冻结→索引表）+ 错误路径（越界/绑错/重登/短包/冻后写/坏校验和）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let (agg, total) = assemble_all()?;
    if agg.registered() != PKG_N || !agg.frozen() || !agg.conserve_ok() {
        return Err(XC_PKG_SHORT);
    }
    if agg.aggregate()? != total {
        return Err(XC_PKG_CSUM);
    }
    let rows = agg.index_rows();
    if rows.len() != PKG_N {
        return Err(XC_PKG_SHORT);
    }
    // 索引表行序 = 包号升序。
    for i in 0..rows.len() {
        if rows[i].0 != i {
            return Err(XC_BAD_PKG);
        }
    }
    if agg.fingerprint() == 0 {
        return Err(XC_PKG_CSUM);
    }

    // 错误路径 1：包号越界。
    let mut a1 = PkgAggregator::new();
    let p1 = HandoffPkg::build(PKG_N, PKG_CRIT_IDS[0], "越界包", 1);
    if a1.enroll(p1) != Err(XC_BAD_PKG) {
        return Err(XC_BAD_PKG);
    }
    // 错误路径 2：判据号绑错。
    let mut a2 = PkgAggregator::new();
    let p2 = HandoffPkg::build(0, 999, "绑错包", 1);
    if a2.enroll(p2) != Err(XC_CRIT_MISMATCH) {
        return Err(XC_CRIT_MISMATCH);
    }
    // 错误路径 3：重复登记。
    let mut a3 = PkgAggregator::new();
    let p3 = HandoffPkg::build(0, PKG_CRIT_IDS[0], PKG_NAMES[0], 3);
    a3.enroll(p3.clone())?;
    if a3.enroll(p3) != Err(XC_DUP_PKG) {
        return Err(XC_DUP_PKG);
    }
    // 错误路径 4：未齐禁聚合。
    let mut a4 = PkgAggregator::new();
    let p4 = HandoffPkg::build(0, PKG_CRIT_IDS[0], PKG_NAMES[0], 3);
    a4.enroll(p4)?;
    if a4.aggregate() != Err(XC_PKG_SHORT) {
        return Err(XC_PKG_SHORT);
    }
    // 错误路径 5：冻结后写拦截。
    let (mut a5, _) = assemble_all()?;
    let p5 = HandoffPkg::build(11, PKG_CRIT_IDS[11], PKG_NAMES[11], 99);
    if a5.enroll(p5) != Err(XC_FROZEN_RDONLY) {
        return Err(XC_FROZEN_RDONLY);
    }
    if a5.freeze() != Err(XC_FROZEN_RDONLY) {
        return Err(XC_FROZEN_RDONLY);
    }
    // 错误路径 6：坏校验和（构造后篡改长度）。
    let mut a6 = PkgAggregator::new();
    let mut p6 = HandoffPkg::build(0, PKG_CRIT_IDS[0], PKG_NAMES[0], 3);
    p6.payload_len = 4;
    if a6.enroll(p6) != Err(XC_PKG_CSUM) {
        return Err(XC_PKG_CSUM);
    }
    Ok(0)
}
