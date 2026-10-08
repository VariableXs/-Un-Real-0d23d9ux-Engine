//! VE-F6201 · 输入域总架构（AE 域 · 输入与交互域 · 批次 AE01 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F6201`
//!
//! **判据（锚点原文）**：四层架构、输入如呼吸、三律、通道承诺、判据。
//!
//! **职责定位（锚点原文）**：输入与交互域的域级总纲（四层：设备抽象/事件层/
//! 映射层/消费层），域本色：**输入如呼吸**——好的输入用户感觉不到它的存在；
//! 输入三律：**可达**（每个功能键盘可触达）/**一致**（同操作处处同响应）/
//! **可重绑**（所有映射用户可改）。输入是用户与世界的唯一通道——**通道卡
//! 一帧，世界就假一帧**；含与 U 域交互词典的联动声明——输入三律与词典
//! 第十章同源同判。数据结构：四层架构；三律声明。错误路径与降级矩阵：
//! 层间耦合违规→拒绝注册；三律违例→整改；注册越权→审计。
//!
//! # 一、「输入如呼吸」为什么是域本色而不是口号
//!
//! 呼吸不会被注意，直到它出问题——输入也一样：用户「感觉不到输入层」
//! 的前提是每一次按键都恰好产生预期响应。这条本色不能靠自觉维护，本模块
//! 把它拆成两个可机检的构件：**四层依赖方向**（[`InLayer::may_depend_on`]
//! ——注册时耦合违规即拒，[`E_COUPLE`]）与**通道零丢帧账**（[`Channel
//! Ledger`]——输入事件只有「已处理/已排队」两种合法去向，`dropped` 计数
//! 判据钉死恒 0：通道卡一帧，世界就假一帧，丢帧=架构违例而非实现细节）。
//!
//! # 二、三律为什么逐条带「违例→整改」而不是只做宣言
//!
//! 三律若只是文档，违例会在每个新功能里悄悄复发。故每律可**立违规单**
//! （[`LawViolation`]）且违规单必须走**整改闭环**（未整改的违规持续在册
//! ——[`LawBoard::clean`] 只在零未整改时为真）。整改不是删除记录：违规
//! 单保留 `remediated` 标记，审计能看到「违过什么、改没改」——被删掉的
//! 违规史等于没违过，那才是真正的红线。
//!
//! # 三、「同操作处处同响应」为什么与 U 域词典第十章同源
//!
//! 「一致」律的判定口径（什么算同一操作、什么算同一响应）若在输入域和
//! 交互词典各写一份，两份口径迟早漂移——同一操作在两处得到不同结论。
//! 故本模块钉下**同源声明**（[`U_DICT_SAME_JUDGE`]）：三律的判定码
//! （[`InputLaw::code`]）与词典第十章条目共享同一编号体系，词典落库后
//! 本域判定改为引用其条目，不复制口径（同源同判红线的前向契约位）。
//!
//! # 四、注册越权为什么是「审计」而不是「报错拉倒」
//!
//! 重复注册/越层注册被拒绝只是第一步——**谁在什么时候越了权**必须留下
//! 审计账（[`Registry::audit_count`]），否则每一次越权尝试都是一次无痕
//! 的架构试探。拒绝+记账双动作，缺一不可。
//!
//! # 五、与相邻条的分工
//!
//! AD10 移交承接是上游（承接声明见 [`AD10_HANDOVER`]）；U 域词典是三律
//! 口径的对端（同源声明）；本域后续条目（事件/映射/消费细则）消费本条
//! 的层枚举与三律板。本条只管「**层怎么分、律怎么判、通道承诺怎么守**」，
//! 不实现具体输入设备与手势识别。
//!
//! **性能（锚点原文）**：O(1) 编排——注册/判定的账面动作均为常数操作
//! （追加一条记录/翻转一个标记），无查表扫描。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const INPUTARCH_VERSION: &str = "AE01-inputarch-v1";

/// 域标识。
pub const INPUT_DOMAIN: &str = "VE-AE";

/// 层间耦合违规（依赖方向错/依赖自身）。
pub const E_COUPLE: &str = "E_COUPLE";

/// 注册越权（重复注册/同名冲突）。
pub const E_SQUAT: &str = "E_SQUAT";

/// 三律未全声明/违规未整改。
pub const E_LAW_OPEN: &str = "E_LAW_OPEN";

/// 通道丢帧（架构违例级——通道承诺红线）。
pub const E_CHANNEL_DROP: &str = "E_CHANNEL_DROP";

/// 输入三律数。
pub const LAW_COUNT: usize = 3;

/// 架构层数。
pub const LAYER_COUNT: usize = 4;

/// U 域交互词典·第十章（输入三律所在章）——同源同判的契约位。
pub const U_DICT_CHAPTER: u8 = 10;

/// 同源同判声明（词典未落库前的前向契约位：三律判定码与词典第十章
/// 条目共享编号体系；落库后本域判定改为引用其条目，不复制口径）。
pub const U_DICT_SAME_JUDGE: &str = "U-dict-ch10-same-judge";

/// AD10 移交承接声明（上游契约位：AD10 移交包的输入侧承接面由本条
/// 接收——承接明细随 AD10 落库回填，位钉在此防漏接）。
pub const AD10_HANDOVER: &str = "AD10-handover-input-surface";

// ---------------------------------------------------------------------------
// 二、四层架构（设备抽象/事件层/映射层/消费层）
// ---------------------------------------------------------------------------

/// 输入域四层（顺序即依赖方向：上层可依赖下层，反向=耦合违规）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InLayer {
    /// 设备抽象：把硬件差异收敛为统一输入原语（最下）。
    Device = 0,
    /// 事件层：原语流→带语义的输入事件。
    Event = 1,
    /// 映射层：事件→动作的绑定（用户可重绑的落点）。
    Mapping = 2,
    /// 消费层：动作驱动业务（最上）。
    Consume = 3,
}

impl InLayer {
    /// 全部四层，**由下至上**。
    pub fn all() -> [InLayer; LAYER_COUNT] {
        [
            InLayer::Device,
            InLayer::Event,
            InLayer::Mapping,
            InLayer::Consume,
        ]
    }

    /// 层号。
    pub const fn ordinal(self) -> usize {
        self as usize
    }

    /// 层短码。
    pub const fn tag(self) -> &'static str {
        match self {
            InLayer::Device => "device",
            InLayer::Event => "event",
            InLayer::Mapping => "mapping",
            InLayer::Consume => "consume",
        }
    }

    /// 层中文名（读屏可达）。
    pub const fn label(self) -> &'static str {
        match self {
            InLayer::Device => "设备抽象层",
            InLayer::Event => "事件层",
            InLayer::Mapping => "映射层",
            InLayer::Consume => "消费层",
        }
    }

    /// 依赖方向是否合法：上层依赖下层合法；依赖自身/上层=耦合违规。
    pub const fn may_depend_on(self, target: InLayer) -> bool {
        self.ordinal() > target.ordinal()
    }
}

/// 待注册模块（架构成员的最小声明）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchModule {
    /// 模块名（域内唯一）。
    pub name: &'static str,
    /// 所属层。
    pub layer: InLayer,
    /// 声明依赖的层集合（含自身/上层即耦合违规——注册入口拒绝）。
    pub deps: Vec<InLayer>,
}

/// 架构注册表：注册+耦合拦截+越权审计（锚点数据结构一）。
#[derive(Clone, Debug, Default)]
pub struct Registry {
    modules: Vec<ArchModule>,
    /// 越权尝试审计计数（拒绝+记账双动作）。
    audit_count: u32,
}

impl Registry {
    /// 空注册表。
    pub fn new() -> Registry {
        Registry { modules: Vec::new(), audit_count: 0 }
    }

    /// 注册模块：依赖方向合法且名字唯一才收；违规拒绝并**审计入账**。
    ///
    /// O(1) 摊销（追加记账）；耦合检查只扫声明的 deps（边界防护）。
    pub fn register(&mut self, m: ArchModule) -> Result<(), &'static str> {
        let mut bad = false;
        for d in m.deps.iter() {
            if !m.layer.may_depend_on(*d) {
                bad = true;
            }
        }
        let dup = self.modules.iter().any(|x| x.name == m.name);
        if bad {
            self.audit_count = self.audit_count.saturating_add(1);
            return Err(E_COUPLE);
        }
        if dup {
            self.audit_count = self.audit_count.saturating_add(1);
            return Err(E_SQUAT);
        }
        self.modules.push(m);
        Ok(())
    }

    /// 在册模块数。
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// 越权审计计数（拒绝之外必须留痕）。
    pub fn audit_count(&self) -> u32 {
        self.audit_count
    }

    /// 某层在册模块数（架构图读屏的分层数据）。
    pub fn count_of(&self, layer: InLayer) -> usize {
        self.modules.iter().filter(|m| m.layer == layer).count()
    }
}

// ---------------------------------------------------------------------------
// 三、输入三律（可达/一致/可重绑 + 违例整改闭环）
// ---------------------------------------------------------------------------

/// 输入三律（锚点闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputLaw {
    /// 可达：每个功能键盘可触达。
    Reachable,
    /// 一致：同操作处处同响应。
    Consistent,
    /// 可重绑：所有映射用户可改。
    Rebindable,
}

impl InputLaw {
    /// 三律全集。
    pub fn all() -> [InputLaw; LAW_COUNT] {
        [InputLaw::Reachable, InputLaw::Consistent, InputLaw::Rebindable]
    }

    /// 律短码。
    pub const fn tag(self) -> &'static str {
        match self {
            InputLaw::Reachable => "reachable",
            InputLaw::Consistent => "consistent",
            InputLaw::Rebindable => "rebindable",
        }
    }

    /// 律中文名。
    pub const fn label(self) -> &'static str {
        match self {
            InputLaw::Reachable => "可达",
            InputLaw::Consistent => "一致",
            InputLaw::Rebindable => "可重绑",
        }
    }

    /// 判定码（与 U 域词典第十章条目同源——[`U_DICT_SAME_JUDGE`]）。
    pub const fn code(self) -> u8 {
        match self {
            InputLaw::Reachable => 1,
            InputLaw::Consistent => 2,
            InputLaw::Rebindable => 3,
        }
    }

    /// 判定码 → 律（同源双向可走——词典落库后按码互查）。
    pub const fn of_code(c: u8) -> Option<InputLaw> {
        match c {
            1 => Some(InputLaw::Reachable),
            2 => Some(InputLaw::Consistent),
            3 => Some(InputLaw::Rebindable),
            _ => None,
        }
    }
}

/// 三律违规单（违例→整改：记录保留，整改打标——违规史不可抹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LawViolation {
    /// 违反的三律。
    pub law: InputLaw,
    /// 违例描述短句。
    pub what: &'static str,
    /// 是否已整改。
    pub remediated: bool,
}

/// 三律声明板（锚点数据结构二）。
#[derive(Clone, Debug, Default)]
pub struct LawBoard {
    declared: u32,
    violations: Vec<LawViolation>,
}

impl LawBoard {
    /// 空板。
    pub fn new() -> LawBoard {
        LawBoard { declared: 0, violations: Vec::new() }
    }

    /// 声明一律（重复声明拒绝——三律各声明一次）。
    pub fn declare(&mut self, law: InputLaw) -> Result<(), &'static str> {
        let already = self.declared & (1u32 << law.code());
        if already != 0 {
            return Err(E_LAW_OPEN);
        }
        self.declared |= 1u32 << law.code();
        Ok(())
    }

    /// 三律是否全部声明。
    pub fn fully_declared(&self) -> bool {
        let mut all = 0u32;
        for l in InputLaw::all().iter() {
            all |= 1u32 << l.code();
        }
        self.declared & all == all
    }

    /// 立违规单（违例→整改闭环入口）。
    pub fn violate(&mut self, law: InputLaw, what: &'static str) {
        self.violations.push(LawViolation { law, what, remediated: false });
    }

    /// 整改：把该律最早一条未整改违规打标（无未整改违规时报错——账实相符）。
    pub fn remediate(&mut self, law: InputLaw) -> Result<(), &'static str> {
        match self.violations.iter_mut().find(|v| v.law == law && !v.remediated) {
            Some(v) => {
                v.remediated = true;
                Ok(())
            }
            None => Err(E_LAW_OPEN),
        }
    }

    /// 未整改违规数（`clean` 只在零未整改时为真）。
    pub fn open_violations(&self) -> usize {
        self.violations.iter().filter(|v| !v.remediated).count()
    }

    /// 架构是否干净（三律全声明 + 零未整改违规）。
    pub fn clean(&self) -> bool {
        self.fully_declared() && self.open_violations() == 0
    }

    /// 违规史总条数（整改过的也在册——违规史不可抹）。
    pub fn total_violations(&self) -> usize {
        self.violations.len()
    }
}

// ---------------------------------------------------------------------------
// 四、通道承诺账（卡一帧=假一帧：零丢帧可机检）
// ---------------------------------------------------------------------------

/// 输入帧处理三态（账面的最小动作单位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameOutcome {
    /// 本帧已处理。
    Handled,
    /// 本帧显性排队（合法去向之二——排队的也要有主）。
    Queued,
    /// 丢帧（**违例**——只应出现在账面被违例打红时，实现不得产生）。
    Dropped,
}

/// 通道账：每帧入账，`dropped` 恒 0 是架构承诺（判据钉死）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChannelLedger {
    pub handled: u32,
    pub queued: u32,
    pub dropped: u32,
}

impl ChannelLedger {
    /// 空账。
    pub fn new() -> ChannelLedger {
        ChannelLedger { handled: 0, queued: 0, dropped: 0 }
    }

    /// 一帧入账（O(1)）。
    pub fn account(&mut self, o: FrameOutcome) {
        match o {
            FrameOutcome::Handled => self.handled = self.handled.saturating_add(1),
            FrameOutcome::Queued => self.queued = self.queued.saturating_add(1),
            FrameOutcome::Dropped => self.dropped = self.dropped.saturating_add(1),
        }
    }

    /// 通道承诺是否守住（dropped 恒 0——卡一帧=假一帧红线）。
    pub fn promise_holds(&self) -> bool {
        self.dropped == 0
    }

    /// 总帧数（三账守恒：handled+queued+dropped）。
    pub fn total_frames(&self) -> u32 {
        self.handled
            .saturating_add(self.queued)
            .saturating_add(self.dropped)
    }
}

// ---------------------------------------------------------------------------
// 五、读屏替述（架构图读屏替代——域本色）
// ---------------------------------------------------------------------------

/// 架构读屏单行（四层一行：层名+在册模块数；无内部细节）。
pub fn screen_line_arch(r: &Registry) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for l in InLayer::all().iter() {
        parts.push(l.label());
    }
    format!(
        "输入域四层（下→上）：{}；在册模块 {} 个，越权审计 {} 次",
        parts.join("→"),
        r.len(),
        r.audit_count()
    )
}

/// 三律读屏单行（三律+整改态）。
pub fn screen_line_laws(b: &LawBoard) -> String {
    let st = if b.clean() {
        "全部声明，零未整改违规"
    } else {
        "存在未整改违规"
    };
    format!(
        "输入三律：可达/一致/可重绑（判定码同源 U 域词典第 {} 章）；{}",
        U_DICT_CHAPTER, st
    )
}
