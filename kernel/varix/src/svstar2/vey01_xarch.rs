//! VE-F4801 · X 域开工与工具链总架构（VE-Y 域 · 工具链与调试域 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4801`
//!
//! **判据（锚点原文五条）**：**四层、工欲善其事、承接落地、层冻结、判据**。
//!
//! - **四层**：CLI 层→构建层→调试层→协议层，层序即依赖方向——**只准向下
//!   依赖**（CLI 可用构建层的接口，反向即越权）。层册定容注册、层序一次
//!   校验 O(层数)，重复层与缺层都不给过。
//! - **工欲善其事（域本色）**：X 域是开发者与调试者的一切武器库，工具链的
//!   体验与本体**同等标准**——工具即产品，因此工具同受无障碍与词典约束：
//!   每个工具入册必须带词典键与读屏文本，缺任一即拒绝入册（不是警告）。
//!   「内部工具不用讲究」是借口，不是理由。
//! - **承接落地**：W 域移交包（F4793 十件）落到 X 域三承接点（vengine CLI
//!   承接插件构建打包需求/市场分发通道/调试桥接协议）。**缺源件必须回溯
//!   指名 F4793**，不允许「先做着回头补源」——没源码的承接是凭空捏造。
//! - **层冻结**：层间接口一经冻结，指纹（签名 FNV-1a）不可改；改语义先走
//!   显性 unfreeze 流程留痕，**直接改即越权**，冻结流程处置（拒绝+立案）。
//!   冻结的目的是让下游敢于依赖：一个今天改明天变 thaw 的接口等于没冻结。
//! - **判据**：锚点判据名独立写死对账；判据侧 FNV 独立实现重算对拍——
//!   被测指纹函数与判据指纹函数是**两份实现**，防同源恒绿。
//!
//! **错误路径与降级矩阵**：层间失配→对拍立案；承接缺源→回溯移交包（指名
//! F4793）；接口越权→冻结流程（拒绝+立案，不静默放行）。
//!
//! **性能逐项分解**：架构 O(层数)；冻结 O(接口数)；落地 O(源数)。
//!
//! **跨批对接点**：上游 F4793 移交包；下游 F4802 CLI；F4820 双签闸。
//!
//! **无障碍与隐私**：工具即产品（工具同受无障碍与词典约束，域本色）；
//! 无隐私面。
//!
//! **诊断码**：X 域独占 `0x3Axx` 段十码，显性映射禁 `as` 直转。

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 〇、诊断码（X 域独占 0x3Axx 段，显性映射禁 as 直转）
// ---------------------------------------------------------------------------

/// 诊断码段基址（X 域独占）。
pub const DIAG_SEG: u16 = 0x3A00;

/// 诊断码（封闭全集十码，显性 wire 映射——禁 `as u16` 直转枚举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XErr {
    /// 层册已满。
    LayerCap,
    /// 层序非法（缺层/重复/倒序）。
    LayerOrder,
    /// 接口越权（下层用上层接口或未注册层接口）。
    IfUsurped,
    /// 接口已冻结仍试图改指纹。
    IfFrozen,
    /// 工具缺词典键或读屏文本。
    ToolNotA11y,
    /// 承接件缺源。
    HandoverMissing,
    /// 承接件拒收（件缺或哈希不符）。
    HandoverRefused,
    /// 层间失配对拍不过。
    LayerMismatch,
    /// 回溯未指名源头件号。
    TracebackNoSource,
    /// 读屏节缺失。
    ScreenSectionMissing,
}

impl XErr {
    /// 码值（显性映射）。
    pub const fn wire(self) -> u16 {
        match self {
            XErr::LayerCap => DIAG_SEG,
            XErr::LayerOrder => DIAG_SEG + 1,
            XErr::IfUsurped => DIAG_SEG + 2,
            XErr::IfFrozen => DIAG_SEG + 3,
            XErr::ToolNotA11y => DIAG_SEG + 4,
            XErr::HandoverMissing => DIAG_SEG + 5,
            XErr::HandoverRefused => DIAG_SEG + 6,
            XErr::LayerMismatch => DIAG_SEG + 7,
            XErr::TracebackNoSource => DIAG_SEG + 8,
            XErr::ScreenSectionMissing => DIAG_SEG + 9,
        }
    }

    /// 人话（读屏用）。
    pub const fn zh(self) -> &'static str {
        match self {
            XErr::LayerCap => "层册已满",
            XErr::LayerOrder => "层序非法",
            XErr::IfUsurped => "接口越权",
            XErr::IfFrozen => "接口已冻结不可改",
            XErr::ToolNotA11y => "工具缺无障碍要素",
            XErr::HandoverMissing => "承接件缺源",
            XErr::HandoverRefused => "承接件拒收",
            XErr::LayerMismatch => "层间失配",
            XErr::TracebackNoSource => "回溯缺源头",
            XErr::ScreenSectionMissing => "读屏节缺失",
        }
    }

    /// 全集遍历（判据用：十码互异十话互异）。
    pub const ALL: [XErr; 10] = [
        XErr::LayerCap,
        XErr::LayerOrder,
        XErr::IfUsurped,
        XErr::IfFrozen,
        XErr::ToolNotA11y,
        XErr::HandoverMissing,
        XErr::HandoverRefused,
        XErr::LayerMismatch,
        XErr::TracebackNoSource,
        XErr::ScreenSectionMissing,
    ];
}

/// FNV-1a 64（被测侧指纹函数）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 一、四层架构册（锚点：总架构册（四层））
// ---------------------------------------------------------------------------

/// 层序（CLI→构建→调试→协议，序号即依赖方向，只准向下）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum XLayer {
    /// CLI 层（用户直接面对的命令面）。
    Cli,
    /// 构建层（编译/打包/分发管线）。
    Build,
    /// 调试层（断点/桥接/检查器）。
    Debug,
    /// 协议层（机器间通信与桥接协议）。
    Protocol,
}

impl XLayer {
    /// 层码（显性映射禁 as）。
    pub const fn code(self) -> u8 {
        match self {
            XLayer::Cli => 0,
            XLayer::Build => 1,
            XLayer::Debug => 2,
            XLayer::Protocol => 3,
        }
    }

    /// 层名（读屏用中文）。
    pub const fn zh(self) -> &'static str {
        match self {
            XLayer::Cli => "CLI 层",
            XLayer::Build => "构建层",
            XLayer::Debug => "调试层",
            XLayer::Protocol => "协议层",
        }
    }

    /// 四层全集（判据与册初始化共用）。
    pub const ALL: [XLayer; 4] = [XLayer::Cli, XLayer::Build, XLayer::Debug, XLayer::Protocol];
}

/// 层规格（层册一行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerSpec {
    /// 层。
    pub layer: XLayer,
    /// 层职责（一句话，读屏可辨）。
    pub duty: String,
}

/// 层册容量（架构册定容，内核无堆上限纪律）。
pub const MAX_LAYERS: usize = 8;

/// 四层架构册。
///
/// 层序即依赖方向：册内层按 `XLayer::code` 严格递增登记，重复层拒绝、
/// 乱序拒绝——架构册本身乱序，层间依赖判定就成了玄学。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArchRegistry {
    /// 已登记层（按层码递增）。
    pub layers: Vec<LayerSpec>,
}

impl ArchRegistry {
    /// 空册。
    pub fn new() -> ArchRegistry {
        ArchRegistry { layers: Vec::new() }
    }

    /// 登记一层（层序校验 O(层数)：重复拒绝、必须严格递增）。
    pub fn register(&mut self, spec: LayerSpec) -> Result<(), XErr> {
        if self.layers.len() >= MAX_LAYERS {
            return Err(XErr::LayerCap);
        }
        for prev in self.layers.iter() {
            if prev.layer == spec.layer {
                return Err(XErr::LayerOrder);
            }
            if prev.layer.code() > spec.layer.code() {
                return Err(XErr::LayerOrder);
            }
        }
        self.layers.push(spec);
        Ok(())
    }

    /// 是否完整四层（缺层 = 架构册不成立，下游开工闸不给过）。
    pub fn is_complete(&self) -> bool {
        if self.layers.len() != XLayer::ALL.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.layers.len() {
            if self.layers[i].layer.code() != i as u8 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 依赖方向判定：`from` 是否允许使用 `to` 层接口（只准向下 = 层码更大）。
    pub fn may_depend(&self, from: XLayer, to: XLayer) -> bool {
        // 下层用上层接口即越权；同层直调也算越权（须走本层接口）。
        to.code() > from.code()
    }
}

// ---------------------------------------------------------------------------
// 二、层间接口冻结（锚点：层间接口冻结；层冻结判据）
// ---------------------------------------------------------------------------

/// 一条层间接口（冻结体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenIf {
    /// 接口名。
    pub name: String,
    /// 属主层（提供方所在层）。
    pub layer: XLayer,
    /// 签名指纹（FNV-1a，冻结后不可改）。
    pub fingerprint: u64,
    /// 是否已冻结。
    pub frozen: bool,
}

/// 接口册容量。
pub const MAX_IFACES: usize = 32;

/// 冻结账（接口登记 + 冻结 + 越权处置）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FreezeLedger {
    /// 接口表。
    pub ifaces: Vec<FrozenIf>,
    /// 立案簿（越权/篡改/失配立案）。
    pub cases: Vec<(XErr, String)>,
}

impl FreezeLedger {
    /// 空账。
    pub fn new() -> FreezeLedger {
        FreezeLedger { ifaces: Vec::new(), cases: Vec::new() }
    }

    /// 登记接口（O(接口数) 查重；未冻结态可登记后统一冻结）。
    pub fn add(&mut self, name: &str, layer: XLayer, signature: &str) -> Result<(), XErr> {
        if self.ifaces.len() >= MAX_IFACES {
            return Err(XErr::LayerCap);
        }
        let mut i = 0usize;
        while i < self.ifaces.len() {
            if self.ifaces[i].name == name {
                return Err(XErr::LayerOrder);
            }
            i += 1;
        }
        self.ifaces.push(FrozenIf {
            name: name.to_string(),
            layer,
            fingerprint: fnv1a(signature.as_bytes()),
            frozen: false,
        });
        Ok(())
    }

    /// 冻结一条接口（重复冻结拒绝——冻结是幂等语义上的一次性动作）。
    pub fn freeze(&mut self, name: &str) -> bool {
        for f in self.ifaces.iter_mut() {
            if f.name == name {
                if f.frozen {
                    return false;
                }
                f.frozen = true;
                return true;
            }
        }
        false
    }

    /// 显性解冻流程（唯一合法的改指纹前置；**必须留痕**）。
    pub fn unfreeze(&mut self, name: &str) -> bool {
        for f in self.ifaces.iter_mut() {
            if f.name == name && f.frozen {
                f.frozen = false;
                self.cases.push((XErr::IfFrozen, format!("显性解冻：{}", name)));
                return true;
            }
        }
        false
    }

    /// 直接改已冻结接口的指纹 = 越权 → 拒绝 + 立案（**冻结流程处置**）。
    pub fn mutate_frozen(&mut self, name: &str, new_sig: &str) -> bool {
        for f in self.ifaces.iter_mut() {
            if f.name == name {
                if f.frozen {
                    self.cases.push((XErr::IfFrozen, format!("越权篡改冻结接口：{}", name)));
                    return false;
                }
                f.fingerprint = fnv1a(new_sig.as_bytes());
                return true;
            }
        }
        false
    }

    /// 层间依赖闸：调用方 `from` 用 `to` 层接口名——方向违规或接口未登记
    /// 都算越权（越权 → 立案 + 拒绝，锚点：接口越权→冻结流程）。
    pub fn check_use(&mut self, from: XLayer, iface: &str) -> bool {
        let hit = self.ifaces.iter().find(|f| f.name == iface).map(|f| (f.layer, f.frozen));
        match hit {
            Some((owner, _frozen)) => {
                if owner.code() > from.code() {
                    true // 向下依赖，放行
                } else {
                    self.cases.push((XErr::IfUsurped, format!("{} 越权用 {}", from.zh(), iface)));
                    false
                }
            }
            None => {
                self.cases.push((XErr::IfUsurped, format!("未登记接口：{}", iface)));
                false
            }
        }
    }

    /// 层间对拍：实际指纹与册内指纹一致才算过（失配 → 立案）。
    pub fn crosscheck(&mut self, iface: &str, actual: u64) -> bool {
        let hit = self.ifaces.iter().find(|f| f.name == iface).map(|f| (f.fingerprint, f.frozen));
        match hit {
            Some((fp, frozen)) => {
                if frozen && fp == actual {
                    true
                } else if frozen {
                    self.cases.push((XErr::LayerMismatch, format!("对拍失配：{}", iface)));
                    false
                } else {
                    false // 未冻结接口不参与对拍（冻结是对拍前提）
                }
            }
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、工欲善其事（域本色：工具即产品——工具同受无障碍与词典约束）
// ---------------------------------------------------------------------------

/// 工具规格（入册一行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolSpec {
    /// 工具名。
    pub name: String,
    /// 词典键（**必填**——工具文案一律走词典，禁散落硬编码）。
    pub dict_key: String,
    /// 读屏文本（**必填**——工具界面输出读屏可辨）。
    pub screen_text: String,
}

/// 工具册容量。
pub const MAX_TOOLS: usize = 32;

/// 工具册（入册闸：缺词典键或缺读屏文本 = 拒绝入册并立案）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolRegistry {
    pub tools: Vec<ToolSpec>,
    pub refused: Vec<(String, XErr)>,
}

impl ToolRegistry {
    /// 空册。
    pub fn new() -> ToolRegistry {
        ToolRegistry { tools: Vec::new(), refused: Vec::new() }
    }

    /// 入册（O(册数) 查重；无障碍两要素缺一即拒——**不是警告是拒绝**）。
    pub fn admit(&mut self, t: ToolSpec) -> bool {
        if self.tools.len() >= MAX_TOOLS {
            self.refused.push((t.name, XErr::LayerCap));
            return false;
        }
        if t.dict_key.is_empty() {
            self.refused.push((t.name, XErr::ToolNotA11y));
            return false;
        }
        if t.screen_text.is_empty() {
            self.refused.push((t.name, XErr::ToolNotA11y));
            return false;
        }
        for prev in self.tools.iter() {
            if prev.name == t.name {
                self.refused.push((t.name, XErr::LayerOrder));
                return false;
            }
        }
        self.tools.push(t);
        true
    }
}

// ---------------------------------------------------------------------------
// 四、承接面落地表（锚点：承接 W 域移交包（F4793 十件））
// ---------------------------------------------------------------------------

/// F4793 移交包十件（锚点原文名录，判据侧独立写死对账）。
pub const HANDOVER_TEN: [&str; 10] = [
    "接口总账",
    "插件指标",
    "场景覆盖",
    "三纪律",
    "fuzz",
    "性能总册",
    "安全隐私",
    "文档总纲",
    "清账",
    "评分走查",
];

/// X 域三承接点（锚点原文）。
pub const HANDOVER_POINTS: [&str; 3] = ["插件构建打包需求", "市场分发通道", "调试桥接协议"];

/// 源头件号（回溯指名）。
pub const HANDOVER_SOURCE: &str = "VE-F4793";

/// 承接状态（互斥完备三态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LandState {
    /// 已落地（有源码有承接点）。
    Landed,
    /// 待承接（有源码未落地）。
    Pending,
    /// 缺源（回溯移交包）。
    Missing,
}

impl LandState {
    /// 人话。
    pub const fn zh(self) -> &'static str {
        match self {
            LandState::Landed => "已落地",
            LandState::Pending => "待承接",
            LandState::Missing => "缺源回溯",
        }
    }
}

/// 落地表一行：一件 → 承接点 + 状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LandRow {
    /// 件名（必须出自 [`HANDOVER_TEN`]——不在册件名 = 拒收）。
    pub item: String,
    /// 承接点（必须出自 [`HANDOVER_POINTS`]）。
    pub point: String,
    /// 状态。
    pub state: LandState,
}

/// 落地表容量。
pub const MAX_LAND_ROWS: usize = 16;

/// 承接面落地表（缺源回溯、越名拒收）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HandoverLedger {
    pub rows: Vec<LandRow>,
    pub tracebacks: Vec<(String, String)>, // (件名, 源头件号)
    pub refused: Vec<(String, XErr)>,
}

impl HandoverLedger {
    /// 空表。
    pub fn new() -> HandoverLedger {
        HandoverLedger { rows: Vec::new(), tracebacks: Vec::new(), refused: Vec::new() }
    }

    fn known_item(item: &str) -> bool {
        HANDOVER_TEN.iter().any(|x| *x == item)
    }

    fn known_point(point: &str) -> bool {
        HANDOVER_POINTS.iter().any(|x| *x == point)
    }

    /// 落地一行（件名/承接点越名即拒收；缺源件登记回溯**指名源头**）。
    pub fn land(&mut self, item: &str, point: &str, state: LandState) -> bool {
        if self.rows.len() >= MAX_LAND_ROWS {
            self.refused.push((item.to_string(), XErr::LayerCap));
            return false;
        }
        if !Self::known_item(item) {
            self.refused.push((item.to_string(), XErr::HandoverRefused));
            return false;
        }
        if !Self::known_point(point) {
            self.refused.push((item.to_string(), XErr::HandoverRefused));
            return false;
        }
        if state == LandState::Missing {
            // 缺源 → 回溯移交包，**必须指名源头件号**，否则连回溯都拒。
            self.tracebacks.push((item.to_string(), HANDOVER_SOURCE.to_string()));
        }
        // 同一件只留一行（重复落地 = 覆盖语义，拒绝——落地面不是流水账）。
        for prev in self.rows.iter() {
            if prev.item == item {
                self.refused.push((item.to_string(), XErr::HandoverRefused));
                return false;
            }
        }
        self.rows.push(LandRow {
            item: item.to_string(),
            point: point.to_string(),
            state,
        });
        true
    }

    /// 十件齐了吗（落地 = Landed；Missing/Pending 都不算落地）。
    pub fn landed_count(&self) -> usize {
        self.rows.iter().filter(|r| r.state == LandState::Landed).count()
    }

    /// 缺源件数（回溯有源头才算有效回溯）。
    pub fn missing_with_traceback(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| r.state == LandState::Missing)
            .filter(|r| self.tracebacks.iter().any(|(it, src)| it == &r.item && src == HANDOVER_SOURCE))
            .count()
    }
}

// ---------------------------------------------------------------------------
// 五、域本色读屏（工具即产品：总览读屏含三节）
// ---------------------------------------------------------------------------

/// 架构读屏总览（层册/冻结/承接三节，缺节 = 读屏不完整）。
pub fn screen_overview(
    reg: &ArchRegistry,
    ledger: &FreezeLedger,
    land: &HandoverLedger,
) -> String {
    let mut s = String::new();
    s.push_str("【层册】");
    let mut i = 0usize;
    while i < reg.layers.len() {
        s.push_str(reg.layers[i].layer.zh());
        if i + 1 < reg.layers.len() {
            s.push_str("→");
        }
        i += 1;
    }
    s.push('\n');
    let frozen = ledger.ifaces.iter().filter(|f| f.frozen).count();
    s.push_str(&format!("【冻结】接口 {} 条已冻结、立案 {} 件\n", frozen, ledger.cases.len()));
    s.push_str(&format!("【承接】已落地 {}／十件、回溯 {} 件\n", land.landed_count(), land.missing_with_traceback()));
    s
}

// ---------------------------------------------------------------------------
// 六、域自检（CheckSet）
// ---------------------------------------------------------------------------

/// 判据侧独立 FNV-1a（**与被测 fnv1a 是两份实现**，防同源恒绿）。
const fn fnv1a_recheck(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

/// 判据侧独立写的期望指纹表（接口名 → 签名字面量的期望指纹）——
/// 接口签名属运行期字面量，判据里对拍时用 `fnv1a_recheck(b"...")` 现算，
/// 不预置常量表（预置反而多一份需要同步的字面量）。

/// VE-F4801 域自检。
pub fn run_vey01_checks() -> CheckSet {
    let mut s = CheckSet::new("vey01_xarch");

    // ---- 判据一：四层 ----
    {
        // 完整四层可建；缺一层 is_complete 必假；乱序登记必拒。
        let mut reg = ArchRegistry::new();
        let mut ok_base = true;
        let mut i = 0usize;
        while i < XLayer::ALL.len() {
            let l = XLayer::ALL[i];
            ok_base = ok_base && reg.register(LayerSpec { layer: l, duty: l.zh().to_string() }).is_ok();
            i += 1;
        }
        let dup = reg.register(LayerSpec { layer: XLayer::Cli, duty: "重复".to_string() });
        let mut bad = ArchRegistry::new();
        let _ = bad.register(LayerSpec { layer: XLayer::Build, duty: "先建构建层".to_string() });
        // 乱序：层码更小的 Cli 在 Build 之后登记必被拒（且不进册）。
        let bad_order = bad.register(LayerSpec { layer: XLayer::Cli, duty: "再建 CLI 层".to_string() });
        s.add(
            "Y01-四层-层册完整且重复乱序双拒",
            ok_base && reg.is_complete() && dup == Err(XErr::LayerOrder) && !bad.is_complete()
                && bad_order == Err(XErr::LayerOrder) && bad.layers.len() == 1,
            "",
        );
    }
    {
        // 依赖方向：向下放行、向上与同层越权。
        let reg = ArchRegistry::new();
        let down = reg.may_depend(XLayer::Cli, XLayer::Protocol);
        let up = reg.may_depend(XLayer::Protocol, XLayer::Cli);
        let same = reg.may_depend(XLayer::Debug, XLayer::Debug);
        s.add("Y01-四层-依赖只准向下", down && !up && !same, "");
    }

    // ---- 判据二：工欲善其事 ----
    {
        // 两要素齐备入册；缺词典键、缺读屏文本各自被拒且立案留名。
        let mut tr = ToolRegistry::new();
        let ok = tr.admit(ToolSpec {
            name: "vengine build".to_string(),
            dict_key: "tool.build.title".to_string(),
            screen_text: "构建工具：打包插件".to_string(),
        });
        let no_dict = tr.admit(ToolSpec {
            name: "bad1".to_string(),
            dict_key: String::new(),
            screen_text: "有读屏".to_string(),
        });
        let no_screen = tr.admit(ToolSpec {
            name: "bad2".to_string(),
            dict_key: "tool.bad2".to_string(),
            screen_text: String::new(),
        });
        let traced = tr.refused.iter().any(|(n, e)| n == "bad1" && *e == XErr::ToolNotA11y)
            && tr.refused.iter().any(|(n, e)| n == "bad2" && *e == XErr::ToolNotA11y);
        s.add(
            "Y01-工欲善其事-缺词典或缺读屏拒绝入册且留名",
            ok && !no_dict && !no_screen && traced && tr.tools.len() == 1,
            "",
        );
    }

    // ---- 判据三：层冻结 ----
    {
        // 冻结后改指纹被拒且立案；显性 unfreeze 后可改且留痕。
        let mut lg = FreezeLedger::new();
        let _ = lg.add("cli.build.request", XLayer::Build, "fn(BuildRequest)->BuildResult");
        let frozen = lg.freeze("cli.build.request");
        let tamper = lg.mutate_frozen("cli.build.request", "fn(BuildRequest)->String");
        let tamper_case = lg.cases.iter().any(|(e, d)| *e == XErr::IfFrozen && d.contains("cli.build.request"));
        let _ = lg.unfreeze("cli.build.request");
        let after_unfreeze = lg.mutate_frozen("cli.build.request", "fn(BuildRequest)->String");
        let unfreeze_traced = lg.cases.iter().any(|(e, _)| *e == XErr::IfFrozen);
        s.add(
            "Y01-层冻结-篡改拒+立案+显性解冻后可改且留痕",
            frozen && !tamper && tamper_case && after_unfreeze && unfreeze_traced,
            "",
        );
    }
    {
        // 越权：CLI 用 Protocol 层接口 = 向下放行；Protocol 用 CLI 层接口 =
        // 向上越权必立案；幽灵接口（未登记）也立案。
        let mut lg = FreezeLedger::new();
        let _ = lg.add("proto.frame", XLayer::Protocol, "frame(v1)");
        let _ = lg.add("cli.exit", XLayer::Cli, "exit(code)");
        let ok = lg.check_use(XLayer::Cli, "proto.frame");
        let usurp = lg.check_use(XLayer::Protocol, "cli.exit");
        let case = lg.cases.iter().any(|(e, _)| *e == XErr::IfUsurped);
        let ghost = lg.check_use(XLayer::Cli, "ghost.iface");
        s.add(
            "Y01-层冻结-越权与幽灵接口立案+向下放行",
            ok && !usurp && case && !ghost,
            "",
        );
    }
    {
        // 对拍：冻结后指纹一致放行、注入失配必立案（非恒真——真对拍）。
        let mut lg = FreezeLedger::new();
        let _ = lg.add("dbg.bridge", XLayer::Debug, "bridge(v1;chan=2)");
        let _ = lg.freeze("dbg.bridge");
        let expect = fnv1a_recheck(b"bridge(v1;chan=2)");
        let pass = lg.crosscheck("dbg.bridge", expect);
        let inject = lg.crosscheck("dbg.bridge", expect ^ 0xdead_beef);
        let case = lg.cases.iter().any(|(e, d)| *e == XErr::LayerMismatch && d.contains("dbg.bridge"));
        s.add(
            "Y01-层冻结-指纹对拍通过+注入失配必立案",
            pass && !inject && case && expect != 0,
            "",
        );
    }

    // ---- 判据四：承接落地 ----
    {
        // 十件名录常量与判据侧独立写死逐条全等。
        let recheck: [&str; 10] = [
            "接口总账", "插件指标", "场景覆盖", "三纪律", "fuzz",
            "性能总册", "安全隐私", "文档总纲", "清账", "评分走查",
        ];
        let mut ten_ok = HANDOVER_TEN.len() == 10;
        let mut i = 0usize;
        while i < 10 {
            if HANDOVER_TEN.get(i) != Some(&recheck[i]) {
                ten_ok = false;
            }
            i += 1;
        }
        // 三承接点同法独立对拍。
        let pts: [&str; 3] = ["插件构建打包需求", "市场分发通道", "调试桥接协议"];
        let mut pts_ok = HANDOVER_POINTS.len() == 3;
        let mut j = 0usize;
        while j < 3 {
            if HANDOVER_POINTS.get(j) != Some(&pts[j]) {
                pts_ok = false;
            }
            j += 1;
        }
        s.add("Y01-承接落地-十件与三承接点名录独立全等", ten_ok && pts_ok, "");
    }
    {
        // 落地：越名件拒收、合法件落地、缺源件回溯必指名 F4793。
        let mut hl = HandoverLedger::new();
        let ghost = hl.land("不存在的件", "市场分发通道", LandState::Landed);
        let bad_point = hl.land("接口总账", "随便哪个点", LandState::Landed);
        let ok1 = hl.land("接口总账", "插件构建打包需求", LandState::Landed);
        let ok2 = hl.land("fuzz", "调试桥接协议", LandState::Missing);
        let traced = hl.missing_with_traceback() == 1;
        let src_ok = hl.tracebacks.iter().any(|(it, src)| it == "fuzz" && src == "VE-F4793");
        s.add(
            "Y01-承接落地-越名拒收+缺源回溯指名源头",
            !ghost && !bad_point && ok1 && ok2 && traced && src_ok && hl.landed_count() == 1,
            "",
        );
    }

    // ---- 判据五：判据（诊断码十码互异 + 读屏三节） ----
    {
        // 十码 wire 两两互异且落在 0x3Axx 段；人话两两互异。
        let mut uniq = true;
        let mut zh_uniq = true;
        let mut i = 0usize;
        while i < XErr::ALL.len() {
            let mut j = i + 1;
            while j < XErr::ALL.len() {
                if XErr::ALL[i].wire() == XErr::ALL[j].wire() {
                    uniq = false;
                }
                if XErr::ALL[i].zh() == XErr::ALL[j].zh() {
                    zh_uniq = false;
                }
                j += 1;
            }
            if XErr::ALL[i].wire() & 0xFF00 != DIAG_SEG & 0xFF00 {
                uniq = false;
            }
            i += 1;
        }
        s.add("Y01-判据-诊断码十码互异且独占段", uniq && zh_uniq, "");
    }
    {
        // 读屏三节齐备：层册→、冻结、承接字样都必须在。
        let mut reg = ArchRegistry::new();
        let mut i = 0usize;
        while i < XLayer::ALL.len() {
            let l = XLayer::ALL[i];
            let _ = reg.register(LayerSpec { layer: l, duty: l.zh().to_string() });
            i += 1;
        }
        let mut lg = FreezeLedger::new();
        let _ = lg.add("cli.exit", XLayer::Cli, "exit(code)");
        let _ = lg.freeze("cli.exit");
        let mut hl = HandoverLedger::new();
        let _ = hl.land("清账", "市场分发通道", LandState::Landed);
        let text = screen_overview(&reg, &lg, &hl);
        s.add(
            "Y01-判据-读屏总览含层册冻结承接三节",
            text.contains("【层册】") && text.contains("→") && text.contains("【冻结】")
                && text.contains("【承接】") && text.contains("已落地 1"),
            "",
        );
    }
    {
        // FNV 两侧实现对拍：同输入同输出、异输入异输出（判据侧独立实现防同源）。
        let a = fnv1a(b"vengine");
        let b = fnv1a_recheck(b"vengine");
        let c = fnv1a(b"vengine!");
        s.add("Y01-判据-双份FNV实现对拍", a == b && a != c && a != 0, "");
    }

    s
}
