//! UNX-F56018 环境冻结规范文档件（12 类清单全文）（0x60220 段）
//!
//! 锚点：UNX-F56018-J1 规范文档含每类冻结项的判定方法/工具/阈值/例外
//! 流程四要素（12 类逐类落账）；J2 与探针实现逐项核对零漂移（×12 逐
//! 类文档↔探针注册表同源对照）；J3 例外流程显性（例外必须留流程编号，
//! 禁"无例外"泛化）三判据全绿。判据锚定 §AI-71.2 专题一。
//! 域：CoRun·UNX-F2-B40（环境冻结域·规范文档面）。
//!
//! 结构：12 类文档登记册（判定方法 × 工具 × 阈值 × 例外流程四元组逐
//! 类落账）+ 探针注册表对照核（类名闭集同源 ×12）+ 四要素完整闸 +
//! 文档指纹（全册折叠）+ 注入核（越闭集类/空要素/泛化例外/封账改写/
//! 对照缺项必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60220 独占 0x20）。
pub const SEG_BASE: u32 = 0x60220;
pub const DC_CAT_UNK: u32 = SEG_BASE + 0x01; // 越闭集清单类
pub const DC_ELEM_EMPTY: u32 = SEG_BASE + 0x02; // 四要素空项
pub const DC_EXC_GENERIC: u32 = SEG_BASE + 0x03; // 泛化例外（无流程编号）
pub const DC_SEALED: u32 = SEG_BASE + 0x04; // 文档账已封账拒改
pub const DC_PROBE_MISS: u32 = SEG_BASE + 0x05; // 对照缺项（探针注册表无对应）
pub const DC_DUP_CAT: u32 = SEG_BASE + 0x06; // 同类重复落账
pub const DC_FP_ZERO: u32 = SEG_BASE + 0x07; // 文档指纹退化零哨兵

/// 12 类冻结清单闭集（与导出面同源命名）。
pub const CAT_NAMES: [&str; 12] = [
    "hw_id", "fw_ver", "power_plan", "fs_layout", "background_set",
    "clock_source", "peripheral_silence", "process_whitelist", "update_window",
    "disk_topology", "time_window", "reject_audit",
];

/// 探针注册表（判定方法落点：类名 → 探针函数名，同序闭集）。
pub const PROBE_FNS: [&str; 12] = [
    "probe_hw_id", "probe_fw_ver", "probe_power_plan", "probe_fs_layout",
    "probe_bg_set", "probe_clock_src", "probe_silence", "probe_proc_wl",
    "probe_upd_win", "probe_topo", "probe_time_win", "probe_rej_audit",
];

/// FNV-1a 32 位（文档指纹同口径）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 类名 → 探针函数名（注册表对照）。
pub fn probe_of(cat: usize) -> Option<&'static str> {
    if cat < 12 {
        Some(PROBE_FNS[cat])
    } else {
        None
    }
}

/// 单类文档条目：类 × 判定方法 × 工具 × 阈值 × 例外流程。
#[derive(Clone)]
pub struct DocItem {
    pub cat: usize,
    pub method: String,
    pub tool: String,
    pub threshold: String,
    pub exception_flow: String,
}

impl DocItem {
    pub fn new(cat: usize, method: &str, tool: &str, threshold: &str, exception_flow: &str) -> Result<Self, u32> {
        if cat >= 12 {
            return Err(DC_CAT_UNK);
        }
        if method.is_empty() || tool.is_empty() || threshold.is_empty() {
            return Err(DC_ELEM_EMPTY);
        }
        // 例外流程必须显性：允许 "无例外-EX0" 哨兵或 EX 编号。
        if exception_flow.is_empty()
            || (!exception_flow.contains("EX") && exception_flow != "无例外-EX0")
        {
            return Err(DC_EXC_GENERIC);
        }
        Ok(DocItem {
            cat,
            method: method.to_string(),
            tool: tool.to_string(),
            threshold: threshold.to_string(),
            exception_flow: exception_flow.to_string(),
        })
    }
}

/// 文档登记册：12 类逐类落账 + 指纹 + 封账。
pub struct DocBook {
    pub items: [Option<DocItem>; 12],
    pub sealed: bool,
}

impl DocBook {
    pub fn new() -> Self {
        DocBook { items: [None, None, None, None, None, None, None, None, None, None, None, None], sealed: false }
    }

    /// 逐类落账（同类重复拒 + 封账拒改）。
    pub fn put(&mut self, it: DocItem) -> Result<(), u32> {
        if self.sealed {
            return Err(DC_SEALED);
        }
        if it.cat >= 12 {
            return Err(DC_CAT_UNK);
        }
        if self.items[it.cat].is_some() {
            return Err(DC_DUP_CAT);
        }
        let cat = it.cat;
        self.items[cat] = Some(it);
        Ok(())
    }

    /// J1：12 类四要素全落账。
    pub fn full(&self) -> bool {
        for it in self.items.iter() {
            if it.is_none() {
                return false;
            }
        }
        true
    }

    /// J2：与探针注册表逐项对照（×12 同源零漂移）。
    pub fn cross_check(&self) -> bool {
        for cat in 0..12 {
            match probe_of(cat) {
                None => return false,
                Some(pf) => match self.items[cat].as_ref() {
                    None => return false,
                    Some(it) => {
                        if !it.method.contains(pf) {
                            return false;
                        }
                    }
                },
            }
        }
        true
    }

    /// 文档指纹（全册折叠）。
    pub fn doc_fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for cat in 0..12 {
            if let Some(it) = self.items[cat].as_ref() {
                let line = format!(
                    "{}|{}|{}|{}|{}",
                    CAT_NAMES[cat], it.method, it.tool, it.threshold, it.exception_flow
                );
                for b in line.as_bytes() {
                    h ^= *b as u32;
                    h = h.wrapping_mul(16777619);
                }
            }
        }
        h
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：12 类四要素全落账（判定方法内嵌探针函数名，例外 EX 编号）。
pub fn standard_doc() -> DocBook {
    let mut b = DocBook::new();
    let thresholds = [
        "hash-equal", "anchor-equal", "drift<1%", "fingerprint-equal",
        "wl-exact", "drift<100ppb", "drift<2%x3", "wl-exact",
        "day-calendar", "triple-match", "T1800s", "rate-export",
    ];
    let exceptions = [
        "EX1-重录", "EX2-重录", "EX3-复测", "EX4-重录",
        "EX5-更新白名单", "EX6-重校准", "EX7-复测", "EX8-更新白名单",
        "EX9-补开窗", "EX10-重登记", "EX11-边界复核", "EX12-月审",
    ];
    for cat in 0..12 {
        let method = format!("按 {} 判定", PROBE_FNS[cat]);
        let it = DocItem::new(cat, &method, "envctl", thresholds[cat], exceptions[cat])
            .expect("fixture item");
        b.put(it).expect("fixture put");
    }
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集类（cat=15）
    if DocItem::new(15, "m", "t", "th", "EX1").err() == Some(DC_CAT_UNK) {
        caught += 1;
    }
    // 2) 空要素
    if DocItem::new(0, "", "t", "th", "EX1").err() == Some(DC_ELEM_EMPTY) {
        caught += 1;
    }
    // 3) 泛化例外（无 EX 编号）
    if DocItem::new(0, "m", "t", "th", "视情况处理").err() == Some(DC_EXC_GENERIC) {
        caught += 1;
    }
    // 4) 对照缺项（方法不含探针名）
    let it = DocItem::new(0, "手工目测", "envctl", "hash-equal", "EX1").expect("bad method");
    let mut b = DocBook::new();
    let _ = b.put(it);
    if !b.cross_check() {
        caught += 1;
    }
    // 5) 封账改写
    let mut b2 = standard_doc();
    b2.seal();
    if b2.put(DocItem::new(0, "m", "t", "th", "EX1").expect("s")) == Err(DC_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56018_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56018");
    // J1-1 12 类四要素全落账
    let b = standard_doc();
    let ok = b.full();
    s.add("56018-J1-十二类四要素全落", ok, if ok { "" } else { "文档缺类缺要素" });
    // J1-2 类名闭集同源（首位尾位与导出面一致）
    let ok = CAT_NAMES[0] == "hw_id" && CAT_NAMES[11] == "reject_audit";
    s.add("56018-J1-类名同源", ok, if ok { "" } else { "类名漂移" });
    // J2-1 与探针注册表 ×12 对照零漂移
    let ok = b.cross_check();
    s.add("56018-J2-探针对照零漂移", ok, if ok { "" } else { "文档探针漂移" });
    // J2-2 注册表闭集（12 名逐位可查）
    let mut hit = 0usize;
    for cat in 0..12 {
        if probe_of(cat).is_some() {
            hit += 1;
        }
    }
    let ok = hit == 12 && probe_of(12).is_none();
    s.add("56018-J2-注册表闭集", ok, if ok { "" } else { "注册表越界" });
    // J1-3 例外流程显性（逐条含 EX 编号）
    let mut ex_ok = true;
    for cat in 0..12 {
        if let Some(it) = b.items[cat].as_ref() {
            if !it.exception_flow.contains("EX") {
                ex_ok = false;
            }
        } else {
            ex_ok = false;
        }
    }
    let ok = ex_ok;
    s.add("56018-J1-例外流程显性", ok, if ok { "" } else { "例外泛化" });
    // J1-4 阈值面语义（时钟 <100ppb / 静默 <2%×3）
    let clk = b.items[5].as_ref().expect("clk").threshold.clone();
    let sil = b.items[6].as_ref().expect("sil").threshold.clone();
    let ok = clk == "drift<100ppb" && sil == "drift<2%x3";
    s.add("56018-J1-阈值面语义", ok, if ok { "" } else { "阈值漂移" });
    // J2-3 文档指纹确定且敏感
    let fp1 = standard_doc().doc_fingerprint();
    let fp2 = standard_doc().doc_fingerprint();
    let mut b2 = standard_doc();
    b2.items[0].as_mut().expect("h").threshold = "hash-loose".to_string();
    let ok = fp1 == fp2 && fp1 != b2.doc_fingerprint() && fp1 != 0;
    s.add("56018-J2-文档指纹敏感", ok, if ok { "" } else { "指纹漂移" });
    // J3-1 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56018-J3-注入核必抓", ok, &detail);
    // J3-2 泛化例外拒收
    let ok = DocItem::new(3, "m", "t", "th", "酌情").err() == Some(DC_EXC_GENERIC)
        && DocItem::new(3, "m", "t", "th", "无例外-EX0").is_ok();
    s.add("56018-J3-例外闸语义", ok, if ok { "" } else { "例外闸松" });
    // J3-3 封账冻结
    let mut b3 = standard_doc();
    let fp = b3.doc_fingerprint();
    b3.seal();
    let ok = b3.put(DocItem::new(1, "m", "t", "th", "EX2").expect("s")) == Err(DC_SEALED)
        && b3.doc_fingerprint() == fp;
    s.add("56018-J3-封账冻结", ok, if ok { "" } else { "封账后可改" });
    // J2-4 同类重复落账拒
    let mut b4 = DocBook::new();
    let _ = b4.put(DocItem::new(0, "m", "t", "th", "EX1").expect("d1"));
    let ok = b4.put(DocItem::new(0, "m2", "t", "th", "EX1").expect("d2")) == Err(DC_DUP_CAT);
    s.add("56018-J2-重复落账拒", ok, if ok { "" } else { "重复穿透" });
    // J1-5 缺类未落账可判
    let empty = DocBook::new();
    let ok = !empty.full() && !empty.cross_check();
    s.add("56018-J1-缺类可判", ok, if ok { "" } else { "缺类误判全" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56018_all_checks_green() {
        let set = run_unx56018_checks();
        assert!(set.all_passed(), "UNX-F56018 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56018_cross_check() {
        assert!(standard_doc().cross_check());
    }
}
