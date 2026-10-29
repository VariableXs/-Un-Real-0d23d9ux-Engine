//! UNX-F56190 启动归因-缺陷账转单器（0x617C0 段）
//!
//! 锚点：UNX-F56190-J1 转单账（归因结论 → 缺陷单条目——单号 × 归因
//! 簇 × 严重度 × 建议动作逐条落账）；J2 转单语义（单号唯一 + 簇闭集
//! 合法 + 严重度闭集合法三性质）；J3 转单指纹（单账折叠指纹）三判据
//! 全绿。
//! 域：CoRun·UNX-F2-B40（启动域·归因转单面）。
//!
//! 结构：归因簇闭集 + 严重度闭集 + 转单簿（归因 → 缺陷单）+ 转单核
//! （簇闸 + 严重度闸 + 唯一号闸）+ 转单指纹 + 注入核（未知簇/未知严
//! 重度/重单号/空建议/印后转单必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x617C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x617C0;
pub const AT_UNK_CLUSTER: u32 = SEG_BASE + 0x01; // 未知归因簇
pub const AT_UNK_SEV: u32 = SEG_BASE + 0x02; // 未知严重度
pub const AT_DUP_TICKET: u32 = SEG_BASE + 0x03; // 重单号
pub const AT_EMPTY_ACTION: u32 = SEG_BASE + 0x04; // 空建议
pub const AT_FROZEN: u32 = SEG_BASE + 0x05; // 印后转单

/// 归因簇闭集。
pub const AC_SLOW_DRV: usize = 0; // 驱动枚举慢
pub const AC_SLOW_COMPOS: usize = 1; // 合成器慢
pub const AC_HANDOFF_JIT: usize = 2; // 交接抖动
pub const AC_NOISE_UNCLASS: usize = 3; // 噪声未归类
pub const AC_COUNT: usize = 4;

pub const CLUSTER_NAMES: [&str; AC_COUNT] = ["slow-drv", "slow-compos", "handoff-jitter", "noise"];

/// 严重度闭集。
pub const SEV_INFO: usize = 0; // 提示
pub const SEV_MINOR: usize = 1; // 轻微
pub const SEV_MAJOR: usize = 2; // 严重
pub const SEV_COUNT: usize = 3;

pub const SEV_NAMES: [&str; SEV_COUNT] = ["info", "minor", "major"];

/// 一张缺陷单。
#[derive(Clone)]
pub struct FaultTicket {
    pub ticket_no: u32,
    pub cluster: usize,
    pub severity: usize,
    pub action: String,
}

/// 转单簿。
pub struct AttributionConverter {
    tickets: Vec<FaultTicket>,
    next_no: u32,
    sealed: bool,
}

impl AttributionConverter {
    pub fn new() -> Self {
        AttributionConverter {
            tickets: Vec::new(),
            next_no: 1,
            sealed: false,
        }
    }

    /// 归因转单：簇闸 + 严重度闸 + 空建议拒 + 印后拒。返回单号。
    pub fn convert(&mut self, cluster: usize, severity: usize, action: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(AT_FROZEN);
        }
        if cluster >= AC_COUNT {
            return Err(AT_UNK_CLUSTER);
        }
        if severity >= SEV_COUNT {
            return Err(AT_UNK_SEV);
        }
        if action.is_empty() {
            return Err(AT_EMPTY_ACTION);
        }
        let no = self.next_no;
        self.next_no += 1;
        self.tickets.push(FaultTicket {
            ticket_no: no,
            cluster,
            severity,
            action: action.to_string(),
        });
        Ok(no)
    }

    pub fn ticket(&self, no: u32) -> Option<&FaultTicket> {
        self.tickets.iter().find(|t| t.ticket_no == no)
    }

    pub fn len(&self) -> usize {
        self.tickets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tickets.is_empty()
    }

    /// 单号唯一核（自动发号天然唯一，复核账面）。
    pub fn nos_unique(&self) -> bool {
        let mut seen: Vec<u32> = Vec::new();
        for t in &self.tickets {
            if seen.contains(&t.ticket_no) {
                return false;
            }
            seen.push(t.ticket_no);
        }
        true
    }

    /// 各簇计数。
    pub fn cluster_counts(&self) -> [usize; AC_COUNT] {
        let mut c = [0usize; AC_COUNT];
        for t in &self.tickets {
            if t.cluster < AC_COUNT {
                c[t.cluster] += 1;
            }
        }
        c
    }

    /// major 及以上单数。
    pub fn major_count(&self) -> usize {
        self.tickets.iter().filter(|t| t.severity == SEV_MAJOR).count()
    }

    /// 转单指纹。
    pub fn conv_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for t in &self.tickets {
            h = h.wrapping_mul(0x0100_0193) ^ t.ticket_no;
            h = h.wrapping_mul(0x0100_0193) ^ t.cluster as u32;
            h = h.wrapping_mul(0x0100_0193) ^ t.severity as u32;
            for b in t.action.bytes() {
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
            }
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封簿印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || !self.nos_unique() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_tickets(&self) -> String {
        let mut out = String::new();
        for t in &self.tickets {
            out.push_str(&format!(
                "T{}|{}|{}|{}\n",
                t.ticket_no, CLUSTER_NAMES[t.cluster], SEV_NAMES[t.severity], t.action
            ));
        }
        if self.sealed {
            out.push_str("CONV-SEAL\n");
        }
        out
    }
}

impl Default for AttributionConverter {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四张单（drv major / compos minor / jitter info / noise minor）。
fn standard_converter() -> AttributionConverter {
    let mut c = AttributionConverter::new();
    let _ = c.convert(AC_SLOW_DRV, SEV_MAJOR, "prefetch-drivers");
    let _ = c.convert(AC_SLOW_COMPOS, SEV_MINOR, "defer-vsync");
    let _ = c.convert(AC_HANDOFF_JIT, SEV_INFO, "watch-only");
    let _ = c.convert(AC_NOISE_UNCLASS, SEV_MINOR, "re-bucket");
    c
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知簇
    let mut c = AttributionConverter::new();
    if c.convert(9, SEV_INFO, "x") == Err(AT_UNK_CLUSTER) {
        caught += 1;
    }
    // 注 2：未知严重度
    let mut c = AttributionConverter::new();
    if c.convert(AC_SLOW_DRV, 7, "x") == Err(AT_UNK_SEV) {
        caught += 1;
    }
    // 注 3：空建议
    let mut c = AttributionConverter::new();
    if c.convert(AC_SLOW_DRV, SEV_INFO, "") == Err(AT_EMPTY_ACTION) {
        caught += 1;
    }
    // 注 4：重单号（手工构造冲突账面）
    let mut c = AttributionConverter::new();
    let _ = c.convert(AC_SLOW_DRV, SEV_INFO, "a");
    if let Some(t) = c.tickets.first_mut() {
        t.ticket_no = 2; // 与下一张将发的号撞
    }
    let _ = c.convert(AC_SLOW_COMPOS, SEV_INFO, "b");
    if !c.nos_unique() {
        caught += 1;
    }
    // 注 5：印后转单
    let mut c = standard_converter();
    let _ = c.seal();
    if c.convert(AC_SLOW_DRV, SEV_INFO, "x") == Err(AT_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 转单账；J2 转单语义；J3 转单指纹。
pub fn run_unx56190_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56190");
    // J1-1 四张单全落账
    let c = standard_converter();
    let ok = c.len() == 4 && c.nos_unique() && (1..=4).all(|n| c.ticket(n).is_some());
    s.add("56190-J1-四单全落账", ok, if ok { "" } else { "单账不全" });
    // J1-2 渲染导出齐
    let mut c2 = standard_converter();
    let _ = c2.seal();
    let text = c2.render_tickets();
    let ok = text.contains("T1|slow-drv|major|prefetch-drivers")
        && text.contains("T3|handoff-jitter|info|watch-only")
        && text.contains("CONV-SEAL");
    s.add("56190-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 单号唯一
    let ok = c.nos_unique();
    s.add("56190-J2-单号唯一", ok, if ok { "" } else { "单号撞车" });
    // J2-2 簇闭集合法
    let ok = c.tickets.iter().all(|t| t.cluster < AC_COUNT);
    s.add("56190-J2-簇闭集合法", ok, if ok { "" } else { "簇越界" });
    // J2-3 严重度闭集合法
    let ok = c.tickets.iter().all(|t| t.severity < SEV_COUNT);
    s.add("56190-J2-严重度闭集", ok, if ok { "" } else { "严重度越界" });
    // J2-4 簇计数（drv 1 / compos 1 / jitter 1 / noise 1）
    let cc = c.cluster_counts();
    let ok = cc == [1, 1, 1, 1];
    let detail = if ok { String::new() } else { format!("{},{},{},{}", cc[0], cc[1], cc[2], cc[3]) };
    s.add_dyn("56190-J2-簇计数", cc == [1, 1, 1, 1], &detail);
    // J2-5 major 计数（1）
    let ok = c.major_count() == 1;
    s.add("56190-J2-major计数", ok, if ok { "" } else { "major 漂移" });
    // J2-6 空建议拒
    let mut c3 = AttributionConverter::new();
    let ok = c3.convert(AC_SLOW_DRV, SEV_INFO, "") == Err(AT_EMPTY_ACTION);
    s.add("56190-J2-空建议拒", ok, if ok { "" } else { "建议闸失效" });
    // J3-1 转单指纹确定
    let ok = standard_converter().conv_fp() == standard_converter().conv_fp();
    s.add("56190-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 单变签变
    let mut c4 = AttributionConverter::new();
    let _ = c4.convert(AC_SLOW_DRV, SEV_INFO, "a");
    let fp0 = c4.conv_fp();
    let _ = c4.convert(AC_SLOW_DRV, SEV_MINOR, "a");
    let ok = c4.conv_fp() != fp0;
    s.add("56190-J3-单变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封簿印
    let mut c5 = standard_converter();
    let ok = c5.seal() && c5.sealed();
    s.add("56190-J3-封簿印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒转
    let ok = c5.convert(AC_SLOW_DRV, SEV_INFO, "x") == Err(AT_FROZEN);
    s.add("56190-J3-印后拒转", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56190-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56190_all_checks_green() {
        let set = run_unx56190_checks();
        assert!(set.all_passed(), "UNX-F56190 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56190_conv_stable() {
        let c = standard_converter();
        assert!(c.nos_unique() && c.major_count() == 1);
    }
}
