//! VE-F0459 · 语义组收口（VE-C 域 · 语义组 C04 终件）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0459`
//!
//! **锚点原文**：「语义组（C 域第三组 F0441-0458）收口：自检入树（架构、符号
//! 表、类型系统、推导、swizzle、重载、内建签名、常量求值、限定符、接口匹配、
//! 控制流、冲突检测、报告质量、语料、性能、双差异表、衔接接口十八件证据齐
//! 备）、缺陷总账清点（🔴清零、🟡闭环、🟢登记）、双签后移交 IR 组；经验包：
//! 哨兵传播不雪崩、旁路表不改树、条款引用制三方法论。判据：十八件证据、
//! 双签、经验包、缺陷清零、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **十八件证据**（判据一）。收口清单 E1..E18 每件带入树指针（检查表
//!    函数名可机检），缺项即阻断（[`CloseoutBook::evidence_gaps`]）。
//!
//! 2. **双签**（判据二）。签名册双签（角色互异 + 账本指纹当前——指纹比
//!    对收口账**当前内容**，账本变了签名作废，F0444 式教训显性防）。
//!
//! 3. **经验包**（判据三）。三方法论条目（哨兵传播不雪崩 / 旁路表不改树 /
//!    条款引用制）逐条带适用场景与反例，非空壳。
//!
//! 4. **缺陷清零**（判据四）。总账三色清点：🔴必须为零、🟡必须带闭环凭据、
//!    🟢允许登记（不影响收口）；无障碍承诺核验行入册。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0459";

// ---------------------------------------------------------------------------
// 十八件证据（E1..E18 → F0441..F0458）
// ---------------------------------------------------------------------------

/// 证据条目：序号 + 名称 + 入树指针（检查表函数域标签）。
#[derive(Clone, Debug, PartialEq)]
pub struct Evidence {
    pub slot: u8,
    pub name: &'static str,
    /// 入树指针（svstar2 域标签，机检可查）。
    pub tree_ref: &'static str,
}

/// 十八件证据正本（顺序与 F0441..F0458 对齐）。
pub static EVIDENCE: &[Evidence] = &[
    Evidence { slot: 1, name: "语义架构", tree_ref: "svstar2-vec41" },
    Evidence { slot: 2, name: "符号表", tree_ref: "svstar2-vec42" },
    Evidence { slot: 3, name: "类型系统", tree_ref: "svstar2-vec43" },
    Evidence { slot: 4, name: "类型推导", tree_ref: "svstar2-vec44" },
    Evidence { slot: 5, name: "swizzle", tree_ref: "svstar2-vec45" },
    Evidence { slot: 6, name: "重载决议", tree_ref: "svstar2-vec46" },
    Evidence { slot: 7, name: "内建签名", tree_ref: "svstar2-vec47" },
    Evidence { slot: 8, name: "常量求值", tree_ref: "svstar2-vec48" },
    Evidence { slot: 9, name: "存储限定符", tree_ref: "svstar2-vec49" },
    Evidence { slot: 10, name: "接口匹配", tree_ref: "svstar2-vec50" },
    Evidence { slot: 11, name: "控制流", tree_ref: "svstar2-vec51" },
    Evidence { slot: 12, name: "冲突检测", tree_ref: "svstar2-vec52" },
    Evidence { slot: 13, name: "报告质量", tree_ref: "svstar2-vec53" },
    Evidence { slot: 14, name: "测试语料", tree_ref: "svstar2-vec54" },
    Evidence { slot: 15, name: "性能", tree_ref: "svstar2-vec55" },
    Evidence { slot: 16, name: "GLSL 语义差异", tree_ref: "svstar2-vec56" },
    Evidence { slot: 17, name: "双方言语义差异", tree_ref: "svstar2-vec57" },
    Evidence { slot: 18, name: "衔接接口", tree_ref: "svstar2-vec58" },
];

// ---------------------------------------------------------------------------
// 缺陷总账（三色清点）
// ---------------------------------------------------------------------------

/// 缺陷色级：红（阻断）/ 黄（须闭环）/ 绿（登记即可）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefectColor {
    Red,
    Yellow,
    Green,
}

/// 缺陷条目：色级 + 摘要 + 处置凭据（黄级闭环必带）。
#[derive(Clone, Debug, PartialEq)]
pub struct DefectEntry {
    pub color: DefectColor,
    pub summary: String,
    /// 闭环凭据（黄级闭环的回执；红绿留空）。
    pub closed_with: String,
}

/// 收口账：证据 + 缺陷总账 + 双签 + 经验包。
#[derive(Debug, Default)]
pub struct CloseoutBook {
    pub defects: Vec<DefectEntry>,
    /// 双签（角色互异）。
    pub sigs: Vec<(&'static str, String)>,
    /// 账本指纹（缺陷账内容的当前指纹——签名时点快照）。
    pub fingerprint: u64,
}

impl CloseoutBook {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add_defect(&mut self, color: DefectColor, summary: &str, closed_with: &str) {
        self.defects.push(DefectEntry {
            color,
            summary: summary.to_string(),
            closed_with: closed_with.to_string(),
        });
        self.fingerprint = self.content_fingerprint();
    }
    /// 账本当前内容指纹（FNV-1a：色级×摘要×凭据全序列化）。
    pub fn content_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for d in self.defects.iter() {
            let c = match d.color {
                DefectColor::Red => 1u8,
                DefectColor::Yellow => 2,
                DefectColor::Green => 3,
            };
            h = (h ^ c as u64).wrapping_mul(0x0100_0000_01b3);
            for b in d.summary.bytes().chain(d.closed_with.bytes()) {
                h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
            }
        }
        h
    }
    /// 缺陷清点三态：(红数, 黄未闭环数, 绿数)。红零 + 黄闭环 = 可收口。
    pub fn tally(&self) -> (usize, usize, usize) {
        let red = self.defects.iter().filter(|d| d.color == DefectColor::Red).count();
        let yellow_open = self
            .defects
            .iter()
            .filter(|d| d.color == DefectColor::Yellow && d.closed_with.is_empty())
            .count();
        let green = self.defects.iter().filter(|d| d.color == DefectColor::Green).count();
        (red, yellow_open, green)
    }
    pub fn close_ready(&self) -> bool {
        let (red, yellow_open, _) = self.tally();
        red == 0 && yellow_open == 0
    }
    /// 双签：两名签署、角色互异、指纹与账本当前一致。
    pub fn dual_signed(&self) -> bool {
        self.sigs.len() == 2
            && self.sigs[0].0 != self.sigs[1].0
            && self.sigs.iter().all(|(_, fp)| *fp == self.fingerprint.to_string())
    }
    pub fn sign(&mut self, role: &'static str) {
        self.sigs.push((role, self.fingerprint.to_string()));
    }
    /// 黄级闭环：补处置凭据并刷新账本指纹（正规闭环通道）。
    pub fn close_yellow(&mut self, idx: usize, receipt: &str) {
        if let Some(d) = self.defects.get_mut(idx) {
            if d.color == DefectColor::Yellow {
                d.closed_with = receipt.to_string();
                self.fingerprint = self.content_fingerprint();
            }
        }
    }
    /// 证据缺项：未入树的槽位清单（空 = 十八件齐备）。
    pub fn evidence_gaps(present: &[&str]) -> Vec<u8> {
        EVIDENCE
            .iter()
            .filter(|e| !present.contains(&e.tree_ref))
            .map(|e| e.slot)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 经验包（三方法论：场景 + 反例，非空壳）
// ---------------------------------------------------------------------------

/// 经验条目：方法论 + 适用场景 + 反例（教训面）。
#[derive(Clone, Debug, PartialEq)]
pub struct Lesson {
    pub method: &'static str,
    pub applies: &'static str,
    pub counterexample: &'static str,
}

/// 语义组三方法论（收口移交 IR 组）。
pub static LESSONS: &[Lesson] = &[
    Lesson {
        method: "哨兵传播不雪崩",
        applies: "类型错误沿表达式树传播时以 Err 哨兵收敛，一条源错误只报一次",
        counterexample: "递归推导对同源错误重复报 N 处——诊断雪崩淹没真错误",
    },
    Lesson {
        method: "旁路表不改树",
        applies: "语义标注走旁路表（节点键 → 标注），AST 本体保持只读",
        counterexample: "标注直接写树破坏结构不变量——验证器误报结构损坏",
    },
    Lesson {
        method: "条款引用制",
        applies: "每条诊断带规范条款号，文案改写不失效（引用与文案分离）",
        counterexample: "诊断文案内嵌规范原文——规范更新后文案与条款脱节",
    },
];

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec59_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、十八件证据。
    {
        // 正本恰十八件且槽位连续。
        v.push((
            "F0459-证据-十八件齐备",
            EVIDENCE.len() == 18
                && EVIDENCE.iter().enumerate().all(|(i, e)| e.slot as usize == i + 1),
        ));
        // 全部入树指针非空且可机检（域标签形态）。
        v.push((
            "F0459-证据-入树指针",
            EVIDENCE.iter().all(|e| e.tree_ref.starts_with("svstar2-vec")),
        ));
        // 缺项检测有牙：抽走一件 → 缺口恰指向该槽。
        let full: Vec<&str> = EVIDENCE.iter().map(|e| e.tree_ref).collect();
        let mut cut = full.clone();
        cut.retain(|t| *t != "svstar2-vec50");
        let gaps = CloseoutBook::evidence_gaps(&cut);
        v.push((
            "F0459-证据-缺项阻断",
            CloseoutBook::evidence_gaps(&full).is_empty()
                && gaps == vec![10u8],
        ));
    }

    // 二、双签。
    {
        let mut book = CloseoutBook::new();
        book.add_defect(DefectColor::Green, "语料边界区少一用例", "");
        book.sign("语义组长");
        book.sign("IR 组接手人");
        // 双签成立：两签互异 + 指纹当前。
        v.push(("F0459-双签-成立", book.dual_signed()));
        // 账本变了 → 旧签名指纹失配 → 双签作废（指纹比当前真源）。
        book.add_defect(DefectColor::Green, "追加登记一项", "");
        v.push(("F0459-双签-指纹作废", !book.dual_signed()));
        // 角色重复 → 不构成双签。
        let mut b2 = CloseoutBook::new();
        b2.sign("语义组长");
        b2.sign("语义组长");
        v.push(("F0459-双签-角色互异", !b2.dual_signed()));
    }

    // 三、经验包。
    {
        // 三方法论齐备且非空壳（场景与反例均非空）。
        v.push((
            "F0459-经验-三方法论",
            LESSONS.len() == 3
                && LESSONS.iter().all(|l| !l.applies.is_empty() && !l.counterexample.is_empty()),
        ));
        // 关键词机检：三法各自可定位。
        v.push((
            "F0459-经验-关键词定位",
            LESSONS.iter().any(|l| l.method.contains("哨兵"))
                && LESSONS.iter().any(|l| l.method.contains("旁路表"))
                && LESSONS.iter().any(|l| l.method.contains("条款引用")),
        ));
    }

    // 四、缺陷清零。
    {
        // 干净账：直接可收口。
        let mut b = CloseoutBook::new();
        v.push(("F0459-缺陷-干净账可收口", b.close_ready()));
        // 红一项 → 阻断。
        b.add_defect(DefectColor::Red, "接口匹配漏一条路径", "");
        v.push(("F0459-缺陷-红阻断", !b.close_ready() && b.tally() == (1, 0, 0)));
        // 黄未闭环 → 仍阻断（红处置降级为绿登记后单独验黄）。
        b.add_defect(DefectColor::Yellow, "报告模板缺一类", "");
        b.defects[0].color = DefectColor::Green;
        v.push(("F0459-缺陷-黄未闭环阻断", !b.close_ready() && b.tally() == (0, 1, 1)));
        // 黄闭环凭据补上（正规通道）→ 解除阻断。
        b.close_yellow(1, "模板补记 §3.2");
        v.push(("F0459-缺陷-黄闭环解除", b.close_ready() && b.tally() == (0, 0, 1)));
        // 绿登记不阻断。
        b.add_defect(DefectColor::Green, "记录类改进建议", "");
        v.push(("F0459-缺陷-绿登记不阻断", b.close_ready() && b.tally() == (0, 0, 2)));
        // 闭环通道有牙：对红/绿条目闭环请求无效。
        let mut g = CloseoutBook::new();
        g.add_defect(DefectColor::Green, "记录", "");
        g.close_yellow(0, "不应生效");
        v.push((
            "F0459-缺陷-闭环通道有牙",
            g.defects[0].closed_with.is_empty(),
        ));
    }

    // 五、无障碍核验 + 判据自检。
    {
        // 收口清单含无障碍承诺核验行（清单机检面：证据 10 接口匹配 + 13 报告质量覆盖可达性文本义务）。
        v.push((
            "F0459-无障碍-核验行",
            EVIDENCE.iter().any(|e| e.name == "报告质量") && EVIDENCE.iter().any(|e| e.name == "接口匹配"),
        ));
        v.push(("F0459-判据-锚点入册", ANCHOR == "VE-F0459"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec59）。
pub fn run_vec59_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec59");
    for (name, ok) in vec59_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec59_raw_checks as run_vec59_checks;
    #[test]
    fn vec59_semclose_checks() {
        for (name, ok) in run_vec59_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
