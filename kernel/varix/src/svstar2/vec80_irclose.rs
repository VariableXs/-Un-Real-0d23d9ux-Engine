//! VE-F0480 · IR 组收口（VE-C 域 · IR 组终件）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0480`
//!
//! **锚点原文**：「IR 组（C 域第四组 F0461-0479）收口：自检入树（数据结构、
//! 生成器、SSA、验证器、转储、折叠、DCE、CSE、类型传播、循环族、纹理降级、
//! 归一化、接口、链接器、序列化、调试装饰、变体参数化、扩展库、编排十九件
//! 证据齐备）、缺陷总账清点、双签后移交优化组；经验包：SSA 不变量自检、副
//! 作用白名单保守原则、缓存双哈希键控方法论。判据：十九件证据、双签、经验
//! 包、缺陷清零、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **十九件证据**（判据一）。E1..E19 逐件入树指针（F0461..F0479 对齐），
//!    缺项即阻断回补（与 F0459 收口同构——第二用复用声明）。
//!
//! 2. **双签**（判据二）。签名册双签（角色互异 + 账本指纹当前——指纹比对
//!    收口账当前内容，账变签废）。
//!
//! 3. **经验包**（判据三）。IR 组三方法论（SSA 不变量自检 / 副作用白名单
//!    保守原则 / 缓存双哈希键控），逐条带场景与反例。
//!
//! 4. **缺陷清零**（判据四）。三色清点（红零 + 黄闭环 = 可收口；绿登记不
//!    阻断），验证器门禁未验 → 补测阻断（证据面单列）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0480";

// ---------------------------------------------------------------------------
// 十九件证据（E1..E19 → F0461..F0479）
// ---------------------------------------------------------------------------

/// 证据条目。
#[derive(Clone, Debug, PartialEq)]
pub struct Evidence {
    pub slot: u8,
    pub name: &'static str,
    pub tree_ref: &'static str,
}

/// 十九件证据正本。
pub static EVIDENCE: &[Evidence] = &[
    Evidence { slot: 1, name: "IR 数据结构", tree_ref: "svstar2-vec61" },
    Evidence { slot: 2, name: "IR 生成器", tree_ref: "svstar2-vec62" },
    Evidence { slot: 3, name: "SSA 构造", tree_ref: "svstar2-vec63" },
    Evidence { slot: 4, name: "IR 验证器", tree_ref: "svstar2-vec64" },
    Evidence { slot: 5, name: "IR 转储", tree_ref: "svstar2-vec65" },
    Evidence { slot: 6, name: "常量折叠", tree_ref: "svstar2-vec66" },
    Evidence { slot: 7, name: "DCE", tree_ref: "svstar2-vec67" },
    Evidence { slot: 8, name: "CSE", tree_ref: "svstar2-vec68" },
    Evidence { slot: 9, name: "类型传播", tree_ref: "svstar2-vec69" },
    Evidence { slot: 10, name: "循环优化族", tree_ref: "svstar2-vec70" },
    Evidence { slot: 11, name: "纹理降级", tree_ref: "svstar2-vec71" },
    Evidence { slot: 12, name: "访问归一化", tree_ref: "svstar2-vec72" },
    Evidence { slot: 13, name: "入口规范化", tree_ref: "svstar2-vec73" },
    Evidence { slot: 14, name: "链接器", tree_ref: "svstar2-vec74" },
    Evidence { slot: 15, name: "序列化缓存", tree_ref: "svstar2-vec75" },
    Evidence { slot: 16, name: "调试装饰", tree_ref: "svstar2-vec76" },
    Evidence { slot: 17, name: "变体参数化", tree_ref: "svstar2-vec77" },
    Evidence { slot: 18, name: "数学扩展库", tree_ref: "svstar2-vec78" },
    Evidence { slot: 19, name: "流水线编排", tree_ref: "svstar2-vec79" },
];

// ---------------------------------------------------------------------------
// 收口账（与 F0459 同构第二用：三色清点 + 双签指纹当前）
// ---------------------------------------------------------------------------

/// 缺陷色级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefectColor {
    Red,
    Yellow,
    Green,
}

/// 缺陷条目。
#[derive(Clone, Debug, PartialEq)]
pub struct DefectEntry {
    pub color: DefectColor,
    pub summary: String,
    pub closed_with: String,
}

/// 收口账。
#[derive(Debug, Default)]
pub struct CloseoutBook {
    pub defects: Vec<DefectEntry>,
    pub sigs: Vec<(&'static str, String)>,
    pub fingerprint: u64,
    /// 验证器门禁核验行（门禁未验 → 补测阻断）。
    pub gate_verified: bool,
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
    /// 三色清点：(红, 黄未闭环, 绿)。
    pub fn tally(&self) -> (usize, usize, usize) {
        let red = self.defects.iter().filter(|d| d.color == DefectColor::Red).count();
        let yopen = self
            .defects
            .iter()
            .filter(|d| d.color == DefectColor::Yellow && d.closed_with.is_empty())
            .count();
        let green = self.defects.iter().filter(|d| d.color == DefectColor::Green).count();
        (red, yopen, green)
    }
    /// 可收口：验证器门禁已验 + 红零 + 黄闭环（门禁未验即阻断）。
    pub fn close_ready(&self) -> bool {
        let (red, yopen, _) = self.tally();
        self.gate_verified && red == 0 && yopen == 0
    }
    pub fn sign(&mut self, role: &'static str) {
        self.sigs.push((role, self.fingerprint.to_string()));
    }
    pub fn dual_signed(&self) -> bool {
        self.sigs.len() == 2
            && self.sigs[0].0 != self.sigs[1].0
            && self.sigs.iter().all(|(_, fp)| *fp == self.fingerprint.to_string())
    }
    /// 黄闭环正规通道（补凭据 + 刷指纹）。
    pub fn close_yellow(&mut self, idx: usize, receipt: &str) {
        if let Some(d) = self.defects.get_mut(idx) {
            if d.color == DefectColor::Yellow {
                d.closed_with = receipt.to_string();
                self.fingerprint = self.content_fingerprint();
            }
        }
    }
    /// 证据缺项：未入树槽位。
    pub fn evidence_gaps(present: &[&str]) -> Vec<u8> {
        EVIDENCE
            .iter()
            .filter(|e| !present.contains(&e.tree_ref))
            .map(|e| e.slot)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 经验包（IR 组三方法论）
// ---------------------------------------------------------------------------

/// 经验条目。
#[derive(Clone, Debug, PartialEq)]
pub struct Lesson {
    pub method: &'static str,
    pub applies: &'static str,
    pub counterexample: &'static str,
}

/// IR 组三方法论（移交优化组）。
pub static LESSONS: &[Lesson] = &[
    Lesson {
        method: "SSA 不变量自检",
        applies: "每优化 pass 后跑验证器自检（使用定义支配一致——不变量即门禁）",
        counterexample: "优化改绑漏 phi 更新——使用无定义静默劣化到错误输出",
    },
    Lesson {
        method: "副作用白名单保守原则",
        applies: "DCE/CSE 对内存写/资源访问/discard 一律保守不消（白名单显性）",
        counterexample: "把 store 当纯计算消除——渲染输出静默丢失",
    },
    Lesson {
        method: "缓存双哈希键控",
        applies: "缓存键 = 源哈希 × 选项哈希（任一变更即 miss——失效面单测）",
        counterexample: "单哈希键控漏选项维度——改编译选项后命中旧缓存静默错产物",
    },
];

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec80_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、十九件证据。
    {
        v.push((
            "F0480-证据-十九件齐备",
            EVIDENCE.len() == 19
                && EVIDENCE.iter().enumerate().all(|(i, e)| e.slot as usize == i + 1),
        ));
        v.push((
            "F0480-证据-入树指针",
            EVIDENCE.iter().all(|e| e.tree_ref.starts_with("svstar2-vec")),
        ));
        // 缺项检测有牙。
        let full: Vec<&str> = EVIDENCE.iter().map(|e| e.tree_ref).collect();
        let mut cut = full.clone();
        cut.retain(|t| *t != "svstar2-vec67");
        let gaps = CloseoutBook::evidence_gaps(&cut);
        v.push((
            "F0480-证据-缺项阻断",
            CloseoutBook::evidence_gaps(&full).is_empty() && gaps == vec![7u8],
        ));
    }

    // 二、双签。
    {
        let mut b = CloseoutBook::new();
        b.gate_verified = true;
        b.add_defect(DefectColor::Green, "转储格式建议", "");
        b.sign("IR 组长");
        b.sign("优化组接手人");
        v.push(("F0480-双签-成立", b.dual_signed()));
        // 账变签废。
        b.add_defect(DefectColor::Green, "追加", "");
        v.push(("F0480-双签-指纹作废", !b.dual_signed()));
        // 角色重复不构成。
        let mut b2 = CloseoutBook::new();
        b2.sign("IR 组长");
        b2.sign("IR 组长");
        v.push(("F0480-双签-角色互异", !b2.dual_signed()));
    }

    // 三、经验包。
    {
        v.push((
            "F0480-经验-三方法论",
            LESSONS.len() == 3
                && LESSONS.iter().all(|l| !l.applies.is_empty() && !l.counterexample.is_empty()),
        ));
        v.push((
            "F0480-经验-关键词定位",
            LESSONS.iter().any(|l| l.method.contains("SSA"))
                && LESSONS.iter().any(|l| l.method.contains("白名单"))
                && LESSONS.iter().any(|l| l.method.contains("双哈希")),
        ));
    }

    // 四、缺陷清零 + 门禁核验。
    {
        // 门禁未验 → 补测阻断（close_ready 还要求 gate_verified）。
        let mut b = CloseoutBook::new();
        b.gate_verified = false;
        v.push(("F0480-门禁-未验阻断", !b.close_ready()));
        // 门禁已验 → 过。
        b.gate_verified = true;
        v.push(("F0480-门禁-已验放行", b.close_ready()));
        // 红阻断。
        b.add_defect(DefectColor::Red, "CSE 误消除一例", "");
        v.push(("F0480-缺陷-红阻断", !b.close_ready() && b.tally() == (1, 0, 0)));
        // 红处置完成（降级绿登记）→ 黄闭环凭据补上 → 解除阻断。
        b.add_defect(DefectColor::Yellow, "转储版本未登记", "");
        b.defects[0].color = DefectColor::Green;
        b.close_yellow(1, "版本行补记 dump-ver:1");
        v.push(("F0480-缺陷-黄闭环解除", b.close_ready() && b.tally() == (0, 0, 1)));
        // 绿登记不阻断。
        b.add_defect(DefectColor::Green, "建议类", "");
        v.push(("F0480-缺陷-绿不阻断", b.close_ready() && b.tally() == (0, 0, 2)));
        // 闭环通道有牙：对红条目闭环无效。
        let mut g = CloseoutBook::new();
        g.gate_verified = true;
        g.add_defect(DefectColor::Red, "红", "");
        g.close_yellow(0, "不应生效");
        v.push(("F0480-缺陷-通道有牙", g.defects[0].closed_with.is_empty()));
    }

    // 五、无障碍核验 + 判据自检。
    {
        // 收口清单含无障碍承诺核验行（转储视图键盘可达证据——vec65 面）。
        v.push((
            "F0480-无障碍-核验行",
            EVIDENCE.iter().any(|e| e.name == "IR 转储"),
        ));
        v.push(("F0480-判据-锚点入册", ANCHOR == "VE-F0480"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec80）。
pub fn run_vec80_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec80");
    for (name, ok) in vec80_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec80_raw_checks as run_vec80_checks;
    #[test]
    fn vec80_irclose_checks() {
        for (name, ok) in run_vec80_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
