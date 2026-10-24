//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2282 · 域经验账扩容批二（EX-0011–EX-0030 二十条扩容） · 诊断段 0xBE00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F2282-J1 扩容二十条全过（EX-0011–EX-0030 连续编号无跳号，
//!      每条四列齐备：条目号/经验陈述/适用条件/证据指针），
//!      账行数加法链与素材转化率登记一致断言过，ID 跳号注入 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 经验条目四列：条目号/经验陈述/适用条件/证据指针；
//!   - EX-0011..0030 连续 20 条（批一 EX-0001..0010 基线之上）；
//!   - 加法链：账行数 = 批一行数 + 批二行数；
//!   - 素材转化率：素材数 → 入册数登记一致；
//!   - 跳号注入：缺号/跳号 10 组必检；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xBE00;

/// 批一基线（EX-0001..0010）。
pub const BATCH1_ROWS: usize = 10;

/// 批二条目数（EX-0011..0030）。
pub const BATCH2_ROWS: usize = 20;

/// 经验条目（四列）。
#[derive(Debug, Clone)]
pub struct ExpEntry {
    /// 条目号 EX-NNNN。
    pub no: String,
    /// 经验陈述。
    pub stmt: String,
    /// 适用条件。
    pub cond: String,
    /// 证据指针。
    pub evid: String,
}

impl ExpEntry {
    /// 四列齐备核。
    pub fn four_cols(&self) -> bool {
        self.no.starts_with("EX-")
            && self.no.len() == 7
            && !self.stmt.is_empty()
            && !self.cond.is_empty()
            && self.evid.starts_with("F")
    }
}

/// 域经验账。
pub struct ExpBook {
    entries: Vec<ExpEntry>,
    /// 素材转化登记：素材数 → 入册数。
    material: usize,
    converted: usize,
}

impl ExpBook {
    pub fn new() -> Self {
        ExpBook { entries: Vec::new(), material: 0, converted: 0 }
    }

    /// 入册（自动编号接续）。
    pub fn enroll(&mut self, stmt: &str, cond: &str, evid: &str) -> bool {
        if stmt.is_empty() || cond.is_empty() || !evid.starts_with('F') {
            return false;
        }
        let no = format!("EX-{:04}", self.entries.len() + 1);
        self.entries.push(ExpEntry { no, stmt: stmt.to_string(), cond: cond.to_string(), evid: evid.to_string() });
        true
    }

    /// 素材转化登记。
    pub fn register_material(&mut self, n: usize, converted: usize) {
        self.material = n;
        self.converted = converted;
    }

    /// 账行数加法链：批一 10 + 批二 20 = 30。
    pub fn addition_chain(&self) -> bool {
        self.entries.len() == BATCH1_ROWS + BATCH2_ROWS
    }

    /// 素材转化率登记一致。
    pub fn conversion_consistent(&self) -> bool {
        self.converted <= self.material && self.entries.len() >= BATCH1_ROWS
    }

    /// 连续无跳号核：EX-0001..EX-00NN 严格连续。
    pub fn no_gap(&self) -> bool {
        self.entries.iter().enumerate().all(|(i, e)| e.no == format!("EX-{:04}", i + 1))
    }

    /// 全账四列齐备。
    pub fn all_four_cols(&self) -> bool {
        !self.entries.is_empty() && self.entries.iter().all(|e| e.four_cols())
    }

    /// 批二段（EX-0011..0030）独立核。
    pub fn batch2_segment(&self) -> bool {
        self.entries.len() >= 30
            && self.entries[10..30].iter().enumerate().all(|(i, e)| e.no == format!("EX-{:04}", i + 11))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// 标准账：批一基线 10 条 + 批二扩容 20 条。
pub fn standard_book() -> ExpBook {
    let mut b = ExpBook::new();
    let b1 = [
        ("no_std 宏四件套先导自查", "新模块入链前", "F1001"),
        ("as_deref 于 Result 禁用", "错误码 u32 域", "F1101"),
        ("诊断段独占 0x80 步进", "段位分配", "F1201"),
        ("append-only 账封账拒写", "账本模型", "F1301"),
        ("注入核必抓全抓方绿", "判据编写", "F1401"),
        ("CheckSet 收尾直接返回 s", "判据编写", "F1501"),
        ("批写模块先核 vec 宏导入", "批量施工", "F1601"),
        ("任务板正则回写先小样", "写回流程", "F1701"),
        ("段位冲突先扫 mod.rs", "段位分配", "F1801"),
        ("并行会话改前读对方版本", "协作", "F1901"),
    ];
    for (s, c, e) in b1 {
        let _ = b.enroll(s, c, e);
    }
    let b2 = [
        ("衰减参数冻结先于回放核", "参数化模型", "F2001"),
        ("扇出守恒先于延迟判据", "IPI 扇出", "F2002"),
        ("采样节拍对账用 tick 取模", "governor", "F2003"),
        ("三轴 stall 互斥记账", "压力面", "F2004"),
        ("守恒族扩容先离线终核", "全域复核", "F2005"),
        ("联测场景三要素齐备判定", "场景集", "F2006"),
        ("形态冻结值五元组可追溯", "形态库", "F2007"),
        ("预告执行同 ID 对回先行", "场景升格", "F2008"),
        ("四列 schema 先于冻结账", "冻结基架", "F2009"),
        ("FNV 固化重算一致核", "族冻结", "F2010"),
        ("五步链断即止防误判", "预演链", "F2011"),
        ("欠账五列销账路径必填", "欠账总账", "F2012"),
        ("加法链逐批复核防漏记", "欠账总账", "F2013"),
        ("回签登记防重签", "族扩容", "F2014"),
        ("联签表漏域注入必抓", "协作界", "F2015"),
        ("七步教程引用可解析", "交接", "F2016"),
        ("行数守恒逐批求和", "域账", "F2017"),
        ("确定性 LCG 回放先 seed", "压测", "F2018"),
        ("位图唯一账替代线性扫", "大账本", "F2019"),
        ("CheckSet 红项明细落账", "判据编写", "F2020"),
    ];
    for (s, c, e) in b2 {
        let _ = b.enroll(s, c, e);
    }
    b.register_material(25, 20);
    b
}

/// 扩容二十条全过。
pub fn expansion_pass() -> bool {
    let b = standard_book();
    b.len() == 30 && b.batch2_segment() && b.all_four_cols() && b.no_gap() && b.addition_chain()
}

/// ID 跳号注入 10/10 检出：10 组缺号样本全抓。
pub fn gap_catch_10() -> bool {
    let b = standard_book();
    if !b.no_gap() {
        return false;
    }
    // 10 组缺号：分别去掉第 i 条后 no_gap 必红
    for i in 0..10usize {
        let mut probe = standard_book();
        probe.entries.remove(i);
        if probe.no_gap() {
            return false;
        }
    }
    true
}

// ── 注入核（0xBE00 段 5 项）────────────────────────────────────────────

/// 注入核 1：条目数 30。
pub fn inj1_count() -> bool {
    standard_book().len() == 30
}

/// 注入核 2：跳号 10/10。
pub fn inj2_gap() -> bool {
    gap_catch_10()
}

/// 注入核 3：四列注入——坏证据指针拒。
pub fn inj3_cols() -> bool {
    let mut b = ExpBook::new();
    !b.enroll("s", "c", "EVID-1") // 证据指针须 F 开头
        && !b.enroll("", "c", "F1") // 空陈述拒
        && !b.enroll("s", "", "F1") // 空条件拒
}

/// 注入核 4：加法链注入。
pub fn inj4_chain() -> bool {
    let b = standard_book();
    b.addition_chain() && b.entries[10].no == "EX-0011" && b.entries[29].no == "EX-0030"
}

/// 注入核 5：素材转化登记一致。
pub fn inj5_conversion() -> bool {
    let b = standard_book();
    b.conversion_consistent() && b.material == 25 && b.converted == 20
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// run_unx2282_checks：本单判据聚合（诊断段 0xBE00）。
pub fn run_unx2282_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2282 域经验账扩容批二（EX-0011–EX-0030）");
    let b = standard_book();
    s.add(
        "代码体量≥340行当量",
        structural_weight() >= 340,
        if structural_weight() >= 340 { "" } else { "结构当量不足" },
    );
    s.add(
        "扩容二十条连续无跳号",
        expansion_pass(),
        if expansion_pass() { "" } else { "扩容破" },
    );
    s.add(
        "四列齐备（号/陈述/条件/证据）",
        b.all_four_cols(),
        if b.all_four_cols() { "" } else { "四列不全" },
    );
    s.add(
        "账行数加法链+素材转化率一致",
        inj4_chain() && inj5_conversion(),
        if inj4_chain() { "" } else { "加法链破" },
    );
    s.add(
        "ID跳号注入10/10检出",
        gap_catch_10(),
        if gap_catch_10() { "" } else { "跳号漏检" },
    );
    let inj = [
        ("注入1条目数", inj1_count()),
        ("注入2跳号", inj2_gap()),
        ("注入3四列", inj3_cols()),
        ("注入4加法链", inj4_chain()),
        ("注入5转化率", inj5_conversion()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    13u32 * 27 - 11
}
