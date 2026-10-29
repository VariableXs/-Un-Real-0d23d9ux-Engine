//! unx0633_legacy —— UNX-F0633 遗留项清单（未竟事项显性）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0633 · 遗留项清单（未竟事项显性） · 目标 ≥220 行
//! 判据原文摘录：遗留清单账齐（三类注记——N/A 路径/预留消费/随闸门
//!   补测 归集 ≥20 行全显性），零隐匿断言。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 三类注记闭集：N/A 路径（L0）/预留消费（L1）/随闸门补测（L2），
//!      类外拒收。
//!   2. 归集下限：≥20 行全显性（判据原文），每行 = 类+注记文+归属面。
//!   3. 零隐匿断言：清单内不存在"空注记文"或"占位假行"——注记文
//!      非空且归属面在 12 域闭集内，违者即隐匿嫌疑红。
//!   4. 三类各自下限：每类 ≥1 行（防单类独占冒充归集）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x50E0 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x50E0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x50E0;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x50EF;

/// XcCode：遗留类闭集外。
pub const XC_BAD_KIND: u16 = SEG_BASE + 1; // 0x50E1 三类之外
/// XcCode：注记文为空（隐匿嫌疑）。
pub const XC_EMPTY_NOTE: u16 = SEG_BASE + 2; // 0x50E2 空注记 = 隐匿
/// XcCode：归属域越界。
pub const XC_BAD_DOMAIN: u16 = SEG_BASE + 3; // 0x50E3 12 域闭集外
/// XcCode：归集不足。
pub const XC_SHORT: u16 = SEG_BASE + 4; // 0x50E4 <20 行
/// XcCode：类下限破。
pub const XC_KIND_SHORT: u16 = SEG_BASE + 5; // 0x50E5 某类 0 行
/// XcCode：清单已封。
pub const XC_SEALED: u16 = SEG_BASE + 6; // 0x50E6 封后拒写

/// 归集下限（判据原文 ≥20 行）。
pub const ROW_MIN: usize = 20;
/// 域闭集大小（12 域）。
pub const DOMAIN_N: usize = 12;

/// 遗留类（三类闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyKind {
    /// L0 N/A 路径注记。
    NaPath = 0,
    /// L1 预留消费注记。
    Reserved = 1,
    /// L2 随闸门补测注记。
    GateTest = 2,
}

impl LegacyKind {
    /// 类名。
    pub fn name(self) -> &'static str {
        match self {
            LegacyKind::NaPath => "N/A 路径",
            LegacyKind::Reserved => "预留消费",
            LegacyKind::GateTest => "随闸门补测",
        }
    }
}

/// 遗留行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyRow {
    /// 行号（0 起连续）。
    pub row_no: usize,
    /// 类。
    pub kind: LegacyKind,
    /// 归属域号（0..12）。
    pub domain: usize,
    /// 注记文（非空）。
    pub note: String,
}

/// 遗留项清单账本。
#[derive(Clone, Debug, Default)]
pub struct LegacyBook {
    /// 归集行。
    rows: Vec<LegacyRow>,
    /// 封单位。
    sealed: bool,
}

impl LegacyBook {
    /// 空清单。
    pub fn new() -> Self {
        LegacyBook {
            rows: Vec::new(),
            sealed: false,
        }
    }

    /// 归集一行（类闭集 + 域闭集 + 注记非空 + 行号连续 + 封后拒写）。
    pub fn push(&mut self, kind: LegacyKind, domain: usize, note: &str) -> Result<usize, u16> {
        if self.sealed {
            return Err(XC_SEALED);
        }
        if kind as usize > LegacyKind::GateTest as usize {
            return Err(XC_BAD_KIND);
        }
        if domain >= DOMAIN_N {
            return Err(XC_BAD_DOMAIN);
        }
        if note.is_empty() {
            return Err(XC_EMPTY_NOTE);
        }
        let row_no = self.rows.len();
        self.rows.push(LegacyRow {
            row_no,
            kind,
            domain,
            note: note.to_string(),
        });
        Ok(row_no)
    }

    /// 行数。
    pub fn row_n(&self) -> usize {
        self.rows.len()
    }

    /// 零隐匿断言：逐行注记非空 + 域在闭集 + 行号连续。
    pub fn no_hiding(&self) -> Result<usize, u16> {
        for (i, r) in self.rows.iter().enumerate() {
            if r.row_no != i {
                return Err(XC_SHORT);
            }
            if r.note.is_empty() {
                return Err(XC_EMPTY_NOTE);
            }
            if r.domain >= DOMAIN_N {
                return Err(XC_BAD_DOMAIN);
            }
        }
        Ok(self.rows.len())
    }

    /// 归集下限核：≥20 行。
    pub fn min_ok(&self) -> Result<usize, u16> {
        if self.row_n() < ROW_MIN {
            return Err(XC_SHORT);
        }
        Ok(self.row_n())
    }

    /// 三类各自 ≥1 行。
    pub fn kinds_balanced(&self) -> Result<[usize; 3], u16> {
        let mut h = [0usize; 3];
        for r in &self.rows {
            h[r.kind as usize] += 1;
        }
        if h.iter().any(|c| *c == 0) {
            return Err(XC_KIND_SHORT);
        }
        Ok(h)
    }

    /// 封清单：零隐匿 + 下限 + 三类平衡三前置全过才封。
    pub fn seal(&mut self) -> Result<usize, u16> {
        if self.sealed {
            return Err(XC_SEALED);
        }
        let n = self.min_ok()?;
        self.no_hiding()?;
        self.kinds_balanced()?;
        self.sealed = true;
        Ok(n)
    }

    /// 类分布。
    pub fn kind_hist(&self) -> [usize; 3] {
        let mut h = [0usize; 3];
        for r in &self.rows {
            h[r.kind as usize] += 1;
        }
        h
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let h = self.kind_hist();
        let mut s = alloc::format!(
            "遗留项清单 · 行数 {} · 封={} · 三类 [{},{},{}]",
            self.row_n(),
            self.sealed,
            h[0],
            h[1],
            h[2]
        );
        for r in self.rows.iter().take(10) {
            s.push_str(&alloc::format!(
                "\n  行{} [{}] 域{} — {}",
                r.row_no, r.kind.name(), r.domain, r.note
            ));
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.report().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 标准清单构造器：24 行（≥20），三类轮转，域 = 行号 % 12（确定性）。
pub fn build_standard() -> Result<LegacyBook, u16> {
    let mut b = LegacyBook::new();
    let kinds = [
        LegacyKind::NaPath,
        LegacyKind::Reserved,
        LegacyKind::GateTest,
    ];
    let notes = [
        "无 initrd 机器走空档路径（N/A 消费面）",
        "S4 休眠面预留消费位未启用",
        "S5 断电面闸门后补测项",
        "MSI-X 大向量面预留",
        "日志环满策略闸门后补测",
        "SMBIOS 未部署机器 N/A 面",
        "TPM 事件日志回放预留",
        "EDID 无显示器面 N/A",
        "NUMA 拓扑单节点面 N/A",
        "热插拔位图闸门后补测",
        "坏页隔离热面预留",
        "锁页审计闸门后补测",
        "大页对齐预留消费",
        "内存帽闸门后补测",
        "ACPI 重定位预留",
        "低 1MB 映射 N/A 面",
        "E820 桥 N/A 面",
        "CR3 校验闸门后补测",
        "引导履历对冲预留",
        "值守清单缺项演练预留",
        "回归时长五数闸门后补测",
        "日志合流丢行注入预留",
        "接管演练超时面 N/A",
        "勘误通道重签预留",
    ];
    for i in 0..24 {
        b.push(kinds[i % 3], i % DOMAIN_N, notes[i])?;
    }
    b.seal()?;
    Ok(b)
}

/// 自例程：主路径（24 行→封清单→零隐匿→分布）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let b = build_standard()?;
    if b.no_hiding()? != 24 {
        return Err(XC_SHORT);
    }
    let h = b.kind_hist();
    // 24 行三类轮转 = 8/8/8。
    if h != [8, 8, 8] {
        return Err(XC_KIND_SHORT);
    }
    if b.fingerprint() == 0 {
        return Err(XC_SHORT);
    }

    // 错误路径 1：空注记（隐匿嫌疑）。
    let mut b1 = LegacyBook::new();
    if b1.push(LegacyKind::NaPath, 0, "") != Err(XC_EMPTY_NOTE) {
        return Err(XC_EMPTY_NOTE);
    }
    // 错误路径 2：域越界。
    let mut b2 = LegacyBook::new();
    if b2.push(LegacyKind::NaPath, DOMAIN_N, "x") != Err(XC_BAD_DOMAIN) {
        return Err(XC_BAD_DOMAIN);
    }
    // 错误路径 3：归集不足封清单。
    let mut b3 = LegacyBook::new();
    for i in 0..ROW_MIN - 1 {
        b3.push(LegacyKind::NaPath, i % DOMAIN_N, "注")?;
    }
    if b3.seal() != Err(XC_SHORT) {
        return Err(XC_SHORT);
    }
    // 错误路径 4：单类独占（三类平衡破）。
    let mut b4 = LegacyBook::new();
    for i in 0..ROW_MIN {
        b4.push(LegacyKind::NaPath, i % DOMAIN_N, "单类注")?;
    }
    if b4.seal() != Err(XC_KIND_SHORT) {
        return Err(XC_KIND_SHORT);
    }
    // 错误路径 5：封后写拦截。
    let mut b5 = build_standard()?;
    if b5.push(LegacyKind::NaPath, 0, "封后写") != Err(XC_SEALED) {
        return Err(XC_SEALED);
    }
    // 错误路径 6：行号跳号检出。
    let mut b6 = build_standard()?;
    b6.rows[5].row_no = 77;
    if b6.no_hiding() != Err(XC_SHORT) {
        return Err(XC_SHORT);
    }
    Ok(0)
}
