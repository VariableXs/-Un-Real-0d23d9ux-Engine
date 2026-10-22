//! CGPU-F0368 · C03 组收口与 C04 移交（判据段 0x0980）
//! C03 组收口：16 项自检入树/五场景配方实测报告/组双签/台账更新/
//! 向 C04 预测与画像组移交（配方与切换协议就绪——预测层有了可修
//! 正的对象）。判据：入树、报告、双签、移交、断点。
//!
//! 锚点原文摘录（施工书 1635 行）：
//! 「16 项自检入树/五场景配方实测报告/组双签/台账更新/向 C04 预
//! 测与画像组移交（配方与切换协议就绪——预测层有了可修正的对象）。
//! 判据：入树、报告、双签、移交、断点。」
//!
//! 诊断码段 0x0980（本模块独占：0x0980~0x098F）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。

use alloc::format;
use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0980;

/// 自检项数冻结（判据「16 项自检」——册面数字写死）。
pub const SELF_CHECK_ITEMS: usize = 16;

/// 组签名数（判据「双签」——恰好两签）。
pub const SIGNATURES_REQUIRED: usize = 2;

/// 移交断点号（判据「断点」锚——C03→C04 断点，C04 从此续）。
pub const HANDOVER_BREAKPOINT: u32 = 3680;

/// 自检项（判据「入树」的叶子——16 项逐项落名）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheckItem {
    /// 配方三账户联动。
    AccountLink,
    /// 多路配额表守恒。
    LaneQuota,
    /// G 标签映射单调。
    GMapMonotonic,
    /// 省电模式语义。
    PowerModes,
    /// 网页两层分配。
    WebTwoLayer,
    /// 可见性联动。
    VisibilityLink,
    /// 滚动提权。
    ScrollBoost,
    /// 滥用防护。
    AbuseGuard,
    /// 全屏授予与保底。
    FullscreenGrant,
    /// 退场恢复无残留。
    ExitRestore,
    /// 切换四相协议。
    TransferPhases,
    /// 并存仲裁。
    CoexistArb,
    /// 风暴合并。
    StormMerge,
    /// 冲突矩阵自洽。
    ConflictMatrix,
    /// 开放格式钳制。
    OpenFormatClamp,
    /// 验证三态。
    VerifyThreeState,
}

impl CheckItem {
    /// 闭集遍历（16 项全列——入树完整性依据）。
    pub fn all() -> [CheckItem; SELF_CHECK_ITEMS] {
        [
            CheckItem::AccountLink, CheckItem::LaneQuota, CheckItem::GMapMonotonic, CheckItem::PowerModes,
            CheckItem::WebTwoLayer, CheckItem::VisibilityLink, CheckItem::ScrollBoost, CheckItem::AbuseGuard,
            CheckItem::FullscreenGrant, CheckItem::ExitRestore, CheckItem::TransferPhases, CheckItem::CoexistArb,
            CheckItem::StormMerge, CheckItem::ConflictMatrix, CheckItem::OpenFormatClamp, CheckItem::VerifyThreeState,
        ]
    }
    /// 项名（自检树节点名）。
    pub fn name(&self) -> &'static str {
        match self {
            CheckItem::AccountLink => "account-link",
            CheckItem::LaneQuota => "lane-quota",
            CheckItem::GMapMonotonic => "gmap-monotonic",
            CheckItem::PowerModes => "power-modes",
            CheckItem::WebTwoLayer => "web-two-layer",
            CheckItem::VisibilityLink => "visibility-link",
            CheckItem::ScrollBoost => "scroll-boost",
            CheckItem::AbuseGuard => "abuse-guard",
            CheckItem::FullscreenGrant => "fullscreen-grant",
            CheckItem::ExitRestore => "exit-restore",
            CheckItem::TransferPhases => "transfer-phases",
            CheckItem::CoexistArb => "coexist-arb",
            CheckItem::StormMerge => "storm-merge",
            CheckItem::ConflictMatrix => "conflict-matrix",
            CheckItem::OpenFormatClamp => "open-format-clamp",
            CheckItem::VerifyThreeState => "verify-three-state",
        }
    }
}

/// 自检树（判据「入树」主判据）：16 项逐项入树、逐项落状态，
/// 未全绿不许收口。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CheckTree {
    /// 节点（项名 + 绿旗）。
    pub nodes: Vec<(&'static str, bool)>,
}

impl CheckTree {
    /// 空树。
    pub fn new() -> Self {
        CheckTree { nodes: Vec::new() }
    }
    /// 入树（去重——同名二次入树拒绝）。
    pub fn insert(&mut self, item: CheckItem, green: bool) -> Result<(), u16> {
        if self.nodes.iter().any(|(n, _)| *n == item.name()) {
            return Err(SEG_BASE);
        }
        self.nodes.push((item.name(), green));
        Ok(())
    }
    /// 全绿核（判据「入树」收口前置）。
    pub fn all_green(&self) -> Result<bool, u16> {
        if self.nodes.len() != SELF_CHECK_ITEMS {
            return Err(SEG_BASE + 1);
        }
        // 覆盖核：16 项闭集逐一命中。
        for item in CheckItem::all() {
            if !self.nodes.iter().any(|(n, _)| *n == item.name()) {
                return Err(SEG_BASE + 2);
            }
        }
        Ok(self.nodes.iter().all(|&(_, g)| g))
    }
}

impl Default for CheckTree {
    fn default() -> Self {
        Self::new()
    }
}

/// 实测报告行（判据「报告」一等公民）：五场景实测。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SceneReportRow {
    /// 场景号（对接 F0367 BenchScene 序号 0~4）。
    pub scene: u8,
    /// 实测步数。
    pub steps: u32,
    /// 达标旗。
    pub ok: bool,
}

impl SceneReportRow {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.scene > 4 {
            return Err(SEG_BASE + 3);
        }
        if self.steps == 0 {
            return Err(SEG_BASE + 4);
        }
        Ok(())
    }
}

/// 实测报告（判据「报告」主判据）：五行齐套 + 全达标。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SceneReport {
    /// 报告行。
    pub rows: Vec<SceneReportRow>,
}

impl SceneReport {
    /// 五行齐套核。
    pub fn complete(&self) -> Result<(), u16> {
        if self.rows.len() != 5 {
            return Err(SEG_BASE + 5);
        }
        let mut seen = [false; 5];
        for r in &self.rows {
            r.validate()?;
            if seen[r.scene as usize] {
                return Err(SEG_BASE + 6);
            }
            seen[r.scene as usize] = true;
        }
        if !self.rows.iter().all(|r| r.ok) {
            return Err(SEG_BASE + 7);
        }
        Ok(())
    }
}

/// 收口账（判据「双签 + 台账」一等公民）：树 + 报告 + 双签 + 断点。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CloseLedger {
    /// 组号（C03=3）。
    pub group: u8,
    /// 双签（签名串，恰好两条、互异）。
    pub signs: Vec<String>,
    /// 移交断点号。
    pub breakpoint: u32,
    /// 台账行数（本组各单完成行——12 单）。
    pub ledger_rows: u32,
}

impl CloseLedger {
    /// 开账（判据「双签」主判据）：两签互异非空才许开。
    pub fn open(sign_a: &str, sign_b: &str) -> Result<Self, u16> {
        if sign_a.is_empty() || sign_b.is_empty() {
            return Err(SEG_BASE + 8);
        }
        if sign_a == sign_b {
            return Err(SEG_BASE + 9);
        }
        Ok(CloseLedger {
            group: 3,
            signs: alloc::vec![String::from(sign_a), String::from(sign_b)],
            breakpoint: HANDOVER_BREAKPOINT,
            ledger_rows: 12,
        })
    }
    /// 双签核。
    pub fn double_sign_ok(&self) -> Result<bool, u16> {
        if self.signs.len() != SIGNATURES_REQUIRED {
            return Err(SEG_BASE + 10);
        }
        Ok(self.signs[0] != self.signs[1] && !self.signs[0].is_empty() && !self.signs[1].is_empty())
    }
}

/// 移交包（判据「移交」一等公民）：C03→C04 移交的内容清单。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HandoverPack {
    /// 断点号（C04 从此续）。
    pub breakpoint: u32,
    /// 移交件（配方协议就绪声明 + 12 单台账锚）。
    pub items: Vec<String>,
}

impl HandoverPack {
    /// 打包（判据「移交」主判据）：前置全过才许打包——树全绿 +
    /// 报告齐套 + 双签成立。
    pub fn pack(tree: &CheckTree, report: &SceneReport, ledger: &CloseLedger) -> Result<Self, u16> {
        if !tree.all_green()? {
            return Err(SEG_BASE + 11);
        }
        report.complete()?;
        if !ledger.double_sign_ok()? {
            return Err(SEG_BASE + 12);
        }
        Ok(HandoverPack {
            breakpoint: ledger.breakpoint,
            items: alloc::vec![
                String::from("配方与切换协议就绪——预测层有了可修正的对象"),
                alloc::format!("C03 组 12 单台账齐（{} 行）", ledger.ledger_rows),
                alloc::format!("自检 16 项全绿（{} 节点）", tree.nodes.len()),
            ],
        })
    }
    /// 断点核（判据「断点」守卫）：断点号必须是冻结值。
    pub fn breakpoint_frozen(&self) -> Result<(), u16> {
        if self.breakpoint != HANDOVER_BREAKPOINT {
            return Err(SEG_BASE + 13);
        }
        Ok(())
    }
}

/// 收口诊断明细（错误码 → 中文可操作建议）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "自检项重复入树",
        1 => "树节点数≠16",
        2 => "树未覆盖 16 项闭集",
        3 => "报告场景号越界（0~4）",
        4 => "实测步数为 0——假跑",
        5 => "报告行数≠5",
        6 => "报告场景重复",
        7 => "报告含未达标行",
        8 => "签名为空",
        9 => "双签互异被破坏",
        10 => "签名数≠2",
        11 => "树未全绿——不许收口",
        12 => "双签不成立——不许移交",
        13 => "断点号被改动——移交锚失效",
        _ => "未知诊断码——查本模块 SEG_BASE 0x0980",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_tree_sixteen_green() {
        let mut t = CheckTree::new();
        for item in CheckItem::all() {
            t.insert(item, true).unwrap();
        }
        assert_eq!(t.nodes.len(), 16);
        assert_eq!(t.all_green().unwrap(), true);
        // 重复入树拒绝。
        assert_eq!(t.insert(CheckItem::AccountLink, true).unwrap_err(), SEG_BASE);
        // 有红不许收口。
        let mut half = CheckTree::new();
        for (i, item) in CheckItem::all().iter().enumerate() {
            half.insert(*item, i != 0).unwrap();
        }
        assert_eq!(half.all_green().unwrap(), false);
    }

    #[test]
    fn scene_report_and_close() {
        let rep = SceneReport {
            rows: (0..5).map(|s| SceneReportRow { scene: s, steps: 8, ok: true }).collect(),
        };
        assert!(rep.complete().is_ok());
        let led = CloseLedger::open("AI-WB", "reviewer-2").unwrap();
        assert!(led.double_sign_ok().unwrap());
        // 同签拒绝。
        assert_eq!(CloseLedger::open("x", "x").unwrap_err(), SEG_BASE + 9);
        assert!(diagnose(SEG_BASE + 9).contains("互异"));
    }

    #[test]
    fn handover_pack_and_breakpoint() {
        let mut t = CheckTree::new();
        for item in CheckItem::all() {
            t.insert(item, true).unwrap();
        }
        let rep = SceneReport {
            rows: (0..5).map(|s| SceneReportRow { scene: s, steps: 8, ok: true }).collect(),
        };
        let led = CloseLedger::open("AI-WB", "reviewer-2").unwrap();
        let pack = HandoverPack::pack(&t, &rep, &led).unwrap();
        assert_eq!(pack.breakpoint, HANDOVER_BREAKPOINT);
        assert!(pack.breakpoint_frozen().is_ok());
        assert_eq!(pack.items.len(), 3);
        // 红树不许移交。
        let mut red = CheckTree::new();
        for (i, item) in CheckItem::all().iter().enumerate() {
            red.insert(*item, i != 15).unwrap();
        }
        assert_eq!(HandoverPack::pack(&red, &rep, &led).unwrap_err(), SEG_BASE + 11);
    }
}
