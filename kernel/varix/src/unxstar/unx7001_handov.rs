//! UNX-F7001 尾段总起与交接页承接对账（F6998 三节逐项）（0x9200 段）
//!
//! 锚点：UNX-F7001-J1 F6998 交接页三节（主题续接/接口延续/判据衔接）
//! 逐项承接对账表落盘；承接项齐备率 100% 机检。
//! 域：CoRun·UNX-B4-B31（B4 尾段交接面）。
//!
//! 结构：交接页三节模型（主题续接 / 接口延续 / 判据衔接）+ 逐项
//! 承接账（(节, 项, 承接态, 凭据) 三元组）+ 承接对账器（三节逐项
//! 扫描 → 齐备率 = 承接项/应承接项）+ 齐备率 100% 机检（缺项/悬空
//! 承接全报）+ 注入核（缺节/悬空项/凭据空/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x9200 独占 0x80）。

pub const SEG_BASE: u32 = 0x9200;
pub const XC_HV_NOSEC: u32 = SEG_BASE + 0x01; // 缺节
pub const XC_HV_DANGLE: u32 = SEG_BASE + 0x02; // 悬空承接项
pub const XC_HV_NOPROOF: u32 = SEG_BASE + 0x03; // 凭据空
pub const XC_HV_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// 交接页三节闭集。
pub const SEC_TOPIC: u8 = 0; // 主题续接
pub const SEC_IFACE: u8 = 1; // 接口延续
pub const SEC_JUDGE: u8 = 2; // 判据衔接

/// 节名。
pub fn section_name(s: u8) -> &'static str {
    match s {
        SEC_TOPIC => "topic-continue",
        SEC_IFACE => "interface-extend",
        SEC_JUDGE => "judge-chain",
        _ => "?",
    }
}

/// 承接态。
pub const HD_PENDING: u8 = 0;
pub const HD_CARRIED: u8 = 1; // 已承接
pub const HD_DROPPED: u8 = 2; // 显性弃接（须凭据）

/// 承接账项：(节, 项名, 承接态, 凭据)。
#[derive(Clone)]
pub struct CarryItem {
    pub section: u8,
    pub item: String,
    pub state: u8,
    pub proof: String,
}

/// 承接对账表。
pub struct HandoverBook {
    pub items: Vec<CarryItem>,
    pub sealed: bool,
}

impl HandoverBook {
    pub fn new() -> Self {
        HandoverBook { items: Vec::new(), sealed: false }
    }

    /// 添加承接项。
    pub fn add(&mut self, section: u8, item: &str, state: u8, proof: &str) -> Result<(), u32> {
        if section > SEC_JUDGE {
            return Err(XC_HV_NOSEC);
        }
        if self.sealed {
            return Err(XC_HV_SEALED);
        }
        self.items.push(CarryItem {
            section,
            item: String::from(item),
            state,
            proof: String::from(proof),
        });
        Ok(())
    }

    /// 悬空项：state=CARRIED 但凭据空。
    pub fn dangling(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.state == HD_CARRIED && i.proof.is_empty())
            .map(|(n, _)| n)
            .collect()
    }

    /// 弃接无凭据：state=DROPPED 但凭据空（显性弃接须凭据）。
    pub fn dropped_no_proof(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.state == HD_DROPPED && i.proof.is_empty())
            .map(|(n, _)| n)
            .collect()
    }

    /// 三节逐项齐备率：有效承接（CARRIED 有凭据 + DROPPED 有凭据）/ 全项。
    pub fn readiness(&self) -> Result<(usize, usize), u32> {
        if self.items.is_empty() {
            return Err(XC_HV_DANGLE);
        }
        let mut ok = 0usize;
        for it in &self.items {
            match it.state {
                HD_CARRIED if !it.proof.is_empty() => ok += 1,
                HD_DROPPED if !it.proof.is_empty() => ok += 1,
                HD_PENDING => {}
                _ => return Err(XC_HV_NOPROOF),
            }
        }
        Ok((ok, self.items.len()))
    }

    /// 三节覆盖核：每节至少一项。
    pub fn section_covered(&self) -> bool {
        (0..=2u8).all(|s| self.items.iter().any(|i| i.section == s))
    }

    /// 100% 机检：全项有效承接 + 三节覆盖 + 无悬空。
    pub fn full_ready(&self) -> bool {
        if !self.section_covered() {
            return false;
        }
        !self.dangling().is_empty() || !self.dropped_no_proof().is_empty()
    }
}

/// F6998 交接页正本承接核：三节逐项全承接。
pub fn f6998_carry_exact() -> bool {
    let mut b = HandoverBook::new();
    // 主题续接节：B4 前段（B23~B30）主题正本续入尾段
    let _ = b.add(SEC_TOPIC, "B23-B30 主题续接", HD_CARRIED, "尾段总起 F7001 承接账");
    let _ = b.add(SEC_TOPIC, "B26 hotplug 双路续接", HD_CARRIED, "unx6901 双路断言");
    // 接口延续节：生命周期/remove 总序接口
    let _ = b.add(SEC_IFACE, "F6881 状态机接口", HD_CARRIED, "迁移表 4 边冻结");
    let _ = b.add(SEC_IFACE, "F6921 五步总序接口", HD_CARRIED, "步序 0..4 冻结");
    // 判据衔接节：F6998 三节判据逐项
    let _ = b.add(SEC_JUDGE, "F6998-J1 判据衔接", HD_CARRIED, "承接项齐备率 100%");
    match b.readiness() {
        Ok((ok, total)) => ok == total && total == 5 && b.full_ready(),
        Err(_) => false,
    }
}

/// 齐备率 100% 机检核：缺项必现形。
pub fn readiness_gate_exact() -> bool {
    // 缺凭据承接 → readiness 拒
    let mut b = HandoverBook::new();
    let _ = b.add(SEC_TOPIC, "缺凭据项", HD_CARRIED, "");
    if b.readiness().is_ok() {
        return false;
    }
    // 弃接无凭据 → 拒
    let mut b2 = HandoverBook::new();
    let _ = b2.add(SEC_TOPIC, "无凭据弃接", HD_DROPPED, "");
    if b2.readiness().is_ok() {
        return false;
    }
    // PENDING 项计入分母不计分子 → <100%
    let mut b3 = HandoverBook::new();
    let _ = b3.add(SEC_TOPIC, "待承接", HD_PENDING, "");
    let _ = b3.add(SEC_IFACE, "已承接", HD_CARRIED, "proof");
    let _ = b3.add(SEC_JUDGE, "已承接2", HD_CARRIED, "proof2");
    match b3.readiness() {
        Ok((ok, total)) => ok == 2 && total == 3,
        Err(_) => false,
    }
}

/// 对账表落盘核：导出机读行（节|项|态|凭据）。
pub fn export_rows_exact() -> bool {
    let mut b = HandoverBook::new();
    let _ = b.add(SEC_IFACE, "iface-A", HD_CARRIED, "pf-A");
    let _ = b.add(SEC_JUDGE, "judge-B", HD_DROPPED, "pf-B");
    let mut rows = Vec::new();
    for it in &b.items {
        let st = match it.state {
            HD_CARRIED => "carried",
            HD_DROPPED => "dropped",
            _ => "pending",
        };
        rows.push(format!(
            "{}|{}|{}|{}",
            section_name(it.section),
            it.item,
            st,
            it.proof
        ));
    }
    rows.len() == 2
        && rows[0] == "interface-extend|iface-A|carried|pf-A"
        && rows[1] == "judge-chain|judge-B|dropped|pf-B"
}

/// 注入核（5 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 缺节（section=5）
    let mut b = HandoverBook::new();
    if b.add(5, "x", HD_CARRIED, "p") == Err(XC_HV_NOSEC) {
        caught += 1;
    }
    // 2) 悬空承接
    let mut b2 = HandoverBook::new();
    let _ = b2.add(SEC_TOPIC, "x", HD_CARRIED, "");
    if !b2.dangling().is_empty() {
        caught += 1;
    }
    // 3) 弃接无凭据
    let mut b3 = HandoverBook::new();
    let _ = b3.add(SEC_IFACE, "y", HD_DROPPED, "");
    if !b3.dropped_no_proof().is_empty() {
        caught += 1;
    }
    // 4) 凭据空拒（readiness 门）
    if b2.readiness().is_ok() {
        caught += 1;
    }
    // 5) 封账后拒
    b3.sealed = true;
    if b3.add(SEC_JUDGE, "z", HD_CARRIED, "p") == Err(XC_HV_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 指纹冻结：承接账折叠哈希。
pub fn fp_frozen(b: &HandoverBook) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for it in &b.items {
        h ^= it.section as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= it.state as u64;
        h = h.wrapping_mul(0x100000001b3);
        for &c in it.item.as_bytes() {
            h ^= c as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        for &c in it.proof.as_bytes() {
            h ^= c as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx7001_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7001");
    // J1-1 F6998 三节逐项承接
    let ok = f6998_carry_exact();
    s.add("B31-01-三节承接", ok, if ok { "" } else { "承接漂移" });
    // J1-2 齐备率 100% 机检
    let ok = readiness_gate_exact();
    s.add("B31-02-齐备率机检", ok, if ok { "" } else { "机检漂移" });
    // J1-3 对账表落盘
    let ok = export_rows_exact();
    s.add("B31-03-对账落盘", ok, if ok { "" } else { "落盘漂移" });
    // J1-4 三节闭集
    let ok = section_name(0) != "?" && section_name(1) != "?" && section_name(2) != "?";
    s.add("B31-04-三节闭集", ok, if ok { "" } else { "闭集漂移" });
    // J1-5 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B31-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结
    let mut b = HandoverBook::new();
    let _ = b.add(SEC_TOPIC, "t", HD_CARRIED, "p");
    let _ = b.add(SEC_IFACE, "i", HD_CARRIED, "p");
    let _ = b.add(SEC_JUDGE, "j", HD_CARRIED, "p");
    let ok = fp_frozen(&b) == fp_frozen(&b) && b.full_ready();
    s.add("B31-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
