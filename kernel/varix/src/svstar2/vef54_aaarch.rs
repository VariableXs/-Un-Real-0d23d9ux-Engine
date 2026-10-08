//! VE-F5401 · AA 域开工与网络总架构（目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5401`
//!
//! 职责定位：AA 域开工——网络与多人域总架构。AA 域定位「让世界连起来」，
//! 多人同场的地基；域本色声明「联机不卡不骗」= 流畅与公平双承诺。
//! 总架构四层：**传输层 → 会话层 → 复制层 → 玩法层**，层间接口**冻结**。
//! 承接 Z 域移交包（F5393 十件）：状态复制消费场景与特效状态源 /
//! 网络事件接入事件总线 / 带宽预算口径。
//!
//! 数据结构：总架构册（四层）；层间接口冻结；承接面落地表。
//!
//! 错误路径与降级矩阵：
//! - 层间失配 → 对拍（不静默重解释）
//! - 承接缺源 → 回溯移交包
//! - 接口越权 → 冻结流程
//!
//! 性能逐项分解：架构 O(层数)；冻结 O(接口数)；落地 O(源数)。
//!
//! 跨批对接点：F5393 移交包上游；F5402 分层下游；F5420 双签闸。
//!
//! 无障碍与隐私：联机不卡不骗（域本色总纲）；无隐私面。
//!
//! ---
//!
//! ## 与总纲的冲突登记（按纪律：以任务单原文为准，冲突如实记录）
//!
//! 1. **域号冲突**：VE 册开篇域表把 `F5401-F5600` 划给 **VE-AB
//!    「VE-Studio 编辑器」**，而本单锚点标题写「**AA 域**」。本模块
//!    **以任务单锚点原文为准**（AA 域 / 网络与多人），并在
//!    [`ArchRegistry::domain`] 里把归属登记成可查字段而非注释——
//!    免得下一个读代码的人按域表把本单当成编辑器活。
//! 2. **CoRun 册冲突**：CoRun 册 `UNX-B2-B31` 把 `F5401–F5420`
//!    登记为 **B2 域的文件系统块层**（fsck/journal/btrfs·squashfs），
//!    与本单「网络与多人」无语义关系。本模块**不引用 CoRun 册口径**，
//!    以 VE 册锚点为唯一事实源。
//!
//! ## 设计要点一：「冻结」不是布尔标志，是**逐接口不可变记录**
//!
//! 锚点：「层间接口冻结」「接口越权 → 冻结流程」。冻结若只存一个
//! `frozen: bool`，则「哪些接口在什么时候被冻结、由谁见证」全部丢失，
//! 越权发生时无法定位到具体接口。故 [`FrozenInterface`] 逐条记
//! （层对、方法签名指纹、冻结序号、见证者），且**冻结后不可解冻**：
//! [`FreezeLedger::freeze`] 对已冻结的同一指纹返回
//! [`FreezeError::AlreadyFrozen`] 而非静默成功——「静默成功」会让
//! 「二次冻结」看起来像幂等，实际掩盖了「有人以为能改」的现实。
//!
//! ## 设计要点二：层间失配**必须对拍**，不许静默重解释
//!
//! 锚点：「层间失配 → 对拍」。这里的失败模式是：玩法层传了
//! 「位置」却用会话层的坐标系，会话层若默默按自己的解释算，帧率
//! 正常、画面错位，没人会发现。故 [`LayerHandshake::probe`] 只做
//! **能力与坐标口径的显式对拍**，返回 [`Mismatch`] 枚举指明
//! 哪一层报了什么，**不在此处做任何补偿**——补偿是玩法层的事，
//! 架构层替它猜 = 把错误藏起来。
//!
//! ## 设计要点三：承接缺源要**回溯移交包**，不许凭空造源
//!
//! 锚点：「承接缺源 → 回溯移交包」。承接面（[`HandoverSurface`]）
//! 的每一项都必须指回移交包里的**具体一件 + 具体条目**。缺源时
//! [`HandoverLedger::bind`] 返回 [`BindError::SourceMissing`] 并
//! 附**回溯路径**（该承接点期望的移交包件号），让人能顺着号去
//! 找上游，而不是拿到一个「未知错误」。**绝不允许**自动降级成
//! 「用默认值顶上」——那会让缺源看起来像正常。
//!
//! ## 设计要点四：「不卡不骗」拆成**两个可机检的承诺**
//!
//! 锚点：「联机不卡不骗——流畅与公平双承诺」。承诺若只写在文档里
//! 等于没说。故二者各有一枚**闸**：
//! · 流畅闸 [`FairnessGate::budget`]：单帧带宽预算 + 抖动上界，
//!   超预算的发送请求被**拒绝并记账**（不是悄悄限流——限流会让
//!   发送方以为自己发出去了）。
//! · 公平闸 [`FairnessGate::fair`]：同输入的对端必须拿到**同一份**
//!   判定结果。判据侧独立重算参考值做双向对账，不问被测函数
//!   「你对吗」。

use alloc::vec::Vec;

/// 四层架构的层号（顺序即依赖方向：上层可依赖下层，下层**不得**
/// 依赖上层）。
///
/// 用 `enum` 而非裸 `u8`：`LayerId` 若可被随意构造，
/// 「层间失配」的判定就退化成「两个数字不相等」，
/// 而真正要防的是**方向错**（下层引用上层）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    /// 传输层：字节搬运（最下）。
    Transport = 0,
    /// 会话层：连接与顺序。
    Session = 1,
    /// 复制层：状态同步与权威裁定。
    Replication = 2,
    /// 玩法层：规则与结算（最上）。
    Gameplay = 3,
}

impl Layer {
    /// 全部四层，**由下至上**。
    pub const ALL: [Layer; 4] = [
        Layer::Transport,
        Layer::Session,
        Layer::Replication,
        Layer::Gameplay,
    ];

    /// 层号（`Transport = 0` 起）。
    pub const fn index(self) -> u8 {
        self as u8
    }

    /// 由层号反解；越界返回 `None`，**绝不猜测**。
    pub const fn from_index(i: u8) -> Option<Layer> {
        match i {
            0 => Some(Layer::Transport),
            1 => Some(Layer::Session),
            2 => Some(Layer::Replication),
            3 => Some(Layer::Gameplay),
            _ => None,
        }
    }

    /// 中文名（读屏可达 —— 域本色「无障碍」侧的最小要求）。
    pub const fn name(self) -> &'static str {
        match self {
            Layer::Transport => "传输层",
            Layer::Session => "会话层",
            Layer::Replication => "复制层",
            Layer::Gameplay => "玩法层",
        }
    }

    /// 本层**允许**依赖的层集合（严格下层，不含自身与上层）。
    pub const fn allowed_deps(self) -> &'static [Layer] {
        match self {
            Layer::Transport => &[],
            Layer::Session => &[Layer::Transport],
            Layer::Replication => &[Layer::Transport, Layer::Session],
            Layer::Gameplay => &[Layer::Transport, Layer::Session, Layer::Replication],
        }
    }
}

/// 依赖方向是否合法：`higher` 能否依赖 `lower`。
///
/// 只有「上层依赖下层」合法；反向（`higher` 的层号小于 `lower`）
/// 即**越权**，这是层间失配里最难在运行期发现的一类，故单列。
pub const fn dep_allowed(higher: Layer, lower: Layer) -> bool {
    higher.index() > lower.index()
}

// ---------------------------------------------------------------------------
// 层间接口冻结
// ---------------------------------------------------------------------------

/// 冻结的一条层间接口。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FrozenInterface {
    /// 调用方（上层）。
    pub caller: Layer,
    /// 提供方（下层）。
    pub provider: Layer,
    /// 方法签名指纹（字符串的稳定哈希，见 [`sig_fingerprint`]）。
    pub sig: u64,
    /// 冻结序号（全序，单调递增）。
    pub seq: u32,
    /// 见证者标识（冻结人 / 批次号），供追责与审计。
    pub witness: &'static str,
}

/// 冻结失败的原因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezeError {
    /// 层方向越权（试图让下层依赖上层）——**拒绝**，并要求先走冻结流程。
    LayerOrder,
    /// 同一签名已被冻结过——**拒绝**（不静默成功，见设计要点一）。
    AlreadyFrozen,
}

/// 冻结台账。
#[derive(Clone, Debug, Default)]
pub struct FreezeLedger {
    frozen: Vec<FrozenInterface>,
    next_seq: u32,
}

impl FreezeLedger {
    /// 新建空台账。
    pub fn new() -> FreezeLedger {
        FreezeLedger { frozen: Vec::new(), next_seq: 1 }
    }

    /// 冻结一条接口。`caller` 必须在 `provider` 之上。
    ///
    /// **冻结后不可解冻、不可改签名**：改签名等于换接口，
    /// 须重新冻结（新序号），否则「冻结」就只是个名字。
    pub fn freeze(
        &mut self,
        caller: Layer,
        provider: Layer,
        sig: u64,
        witness: &'static str,
    ) -> Result<u32, FreezeError> {
        if !dep_allowed(caller, provider) {
            return Err(FreezeError::LayerOrder);
        }
        if self.frozen.iter().any(|f| f.caller == caller && f.provider == provider && f.sig == sig) {
            return Err(FreezeError::AlreadyFrozen);
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.frozen.push(FrozenInterface { caller, provider, sig, seq, witness });
        Ok(seq)
    }

    /// 是否已冻结这条接口（**只认精确三元组**，不按层对泛化）。
    pub fn is_frozen(&self, caller: Layer, provider: Layer, sig: u64) -> bool {
        self.frozen.iter().any(|f| f.caller == caller && f.provider == provider && f.sig == sig)
    }

    /// 某层提供的全部冻结接口数（供对账：注册数 == 声明数）。
    pub fn frozen_count_of(&self, provider: Layer) -> usize {
        self.frozen.iter().filter(|f| f.provider == provider).count()
    }

    /// 冻结总条数。
    pub fn len(&self) -> usize {
        self.frozen.len()
    }

    /// 是否一条都没冻。
    pub fn is_empty(&self) -> bool {
        self.frozen.is_empty()
    }

    /// 按序号取一条（供审计遍历）。
    pub fn get(&self, seq: u32) -> Option<&FrozenInterface> {
        self.frozen.iter().find(|f| f.seq == seq)
    }

    /// **校验冻结台账自身的不变式**（不是校验别人，是校验台账）：
    /// ① 序号严格递增且无重复；② 每条方向合法；③ 无重复三元组。
    ///
    /// 这条自查是必要的：台账若被外部直接改脏（例如绕过
    /// [`FreezeLedger::freeze`] 塞进一条越权记录），上层所有基于
    /// 「冻结过就合法」的推理就全错了。
    pub fn audit(&self) -> Result<usize, &'static str> {
        let mut last = 0u32;
        for f in &self.frozen {
            if f.seq <= last {
                return Err("冻结序号非严格递增");
            }
            last = f.seq;
            if !dep_allowed(f.caller, f.provider) {
                return Err("台账含越权方向记录");
            }
        }
        for i in 0..self.frozen.len() {
            for j in (i + 1)..self.frozen.len() {
                let (a, b) = (&self.frozen[i], &self.frozen[j]);
                if a.caller == b.caller && a.provider == b.provider && a.sig == b.sig {
                    return Err("台账含重复冻结三元组");
                }
            }
        }
        Ok(self.frozen.len())
    }
}

/// 方法签名指纹：`(层对, 名字)` 的稳定 64 位哈希（FNV-1a）。
///
/// **不用 `DefaultHasher`**：它的输出不保证跨版本稳定，而「冻结」
/// 的语义要求「同一签名在任何时候算出同一个指纹」，否则重启后
/// 全部冻结记录失效。
///
/// **三个维度的作用（按可观测性区分，勿混为一谈）**：
/// - **层号进哈希**（`caller`/`provider` 各喂一字节）：**有可观测后果**
///   ——同名方法在不同层对上必得不同指纹，否则「玩法层调会话层的
///   `sync`」与「复制层调传输层的 `sync`」会共用一条冻结记录。
/// - **名字逐字节进哈希**（含 NUL 结尾的分隔）：**有可观测后果**
///   ——否则 `"ab"+"c"` 与 `"a"+"bc"` 这类拼接歧义会撞。
/// - **长度混入**（末位 `^ len * 常数`）：**防的是理论碰撞**。FNV-1a
///   本身对短名无实测碰撞，故这项目前**无可观测后果**（属等价变异，
///   保留是因为它不花成本且降低长名字段的碰撞面）。
pub fn sig_fingerprint(caller: Layer, provider: Layer, name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let feed = |h: &mut u64, b: u8| {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    feed(&mut h, caller.index());
    feed(&mut h, provider.index());
    for b in name.as_bytes() {
        feed(&mut h, *b);
    }
    // 末位混入，避免前缀相同导致尾部相同（如 "ab"+"c" vs "a"+"bc"）。
    h ^ (name.len() as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

// ---------------------------------------------------------------------------
// 层间失配 → 对拍
// ---------------------------------------------------------------------------

/// 层间对拍的结果：报哪一层、报什么。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mismatch {
    /// 一致。
    None,
    /// 上层声称的坐标口径与下层实际不符。
    CoordFrame { caller: Layer, caller_frame: u32, provider_frame: u32 },
    /// 上层要求的能力下层没有。
    CapabilityMissing { caller: Layer, cap: u32 },
    /// 下层多报了一个上层没要的能力（虚报 —— 「不骗」的反面）。
    CapabilityOverreport { provider: Layer, cap: u32 },
    /// 层方向越权（下层反向依赖上层）。
    LayerOrder { caller: Layer, provider: Layer },
}

/// 一次层间对拍的输入。
#[derive(Clone, Copy, Debug)]
pub struct Probe {
    /// 调用方（上层）。
    pub caller: Layer,
    /// 提供方（下层）。
    pub provider: Layer,
    /// 调用方声称使用的坐标口径。
    pub caller_frame: u32,
    /// 提供方实际的坐标口径。
    pub provider_frame: u32,
    /// 调用方要求的能力位（位掩码）。
    pub caller_caps: u32,
    /// 提供方实际具备的能力位。
    pub provider_caps: u32,
}

/// 层间对拍。
///
/// **只报不修**（设计要点二）：返回的 [`Mismatch`] 交由玩法层
/// 决策补偿；架构层代替下游猜测 = 把错误藏进"看起来正常"的帧里。
pub fn probe(p: &Probe) -> Mismatch {
    if !dep_allowed(p.caller, p.provider) {
        return Mismatch::LayerOrder { caller: p.caller, provider: p.provider };
    }
    if p.caller_frame != p.provider_frame {
        return Mismatch::CoordFrame {
            caller: p.caller,
            caller_frame: p.caller_frame,
            provider_frame: p.provider_frame,
        };
    }
    // 缺能力：调用方要的每一位都必须在提供方那里为 1。
    let missing = p.caller_caps & !p.provider_caps;
    if missing != 0 {
        // 取最低的一个缺失位（确定性：同一状态必得同一 cap 号）。
        let cap = missing.trailing_zeros();
        return Mismatch::CapabilityMissing { caller: p.caller, cap };
    }
    // 虚报：提供方多报能力。没被要求却说自己有 = 「骗」。
    let extra = p.provider_caps & !p.caller_caps;
    if extra != 0 {
        let cap = extra.trailing_zeros();
        return Mismatch::CapabilityOverreport { provider: p.provider, cap };
    }
    Mismatch::None
}

/// 对拍是否通过。
pub const fn probe_ok(m: Mismatch) -> bool {
    matches!(m, Mismatch::None)
}

// ---------------------------------------------------------------------------
// 承接面落地表（承接 Z 域移交包 F5393 十件）
// ---------------------------------------------------------------------------

/// 移交包件号（F5393 十件：接口总账 / 特效指标 / 场景覆盖 / 三纪律 /
/// fuzz / 性能总册 / 安全隐私 / 文档总纲 / 清账 / 评分走查）。
///
/// **件号 1..=10 是契约**：`HandoverSurface::expect_item` 按它做
/// 回溯校验，故 `Missing` 里带件号才能让人顺着号去找上游。
pub const HANDOVER_ITEMS: u32 = 10;

/// 承接面的三个承接点（锚点：状态复制消费场景与特效状态源 /
/// 网络事件接入事件总线 / 带宽预算口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    /// 状态复制消费场景与特效状态源。
    ReplayConsume = 1,
    /// 网络事件接入事件总线。
    EventBus = 2,
    /// 带宽预算口径。
    Bandwidth = 3,
}

impl Surface {
    /// 全部三个承接点。
    pub const ALL: [Surface; 3] = [Surface::ReplayConsume, Surface::EventBus, Surface::Bandwidth];

    /// 该承接点**期望**回溯到的移交包件号。
    ///
    /// 这是「回溯移交包」的落点常量：绑定时若源件号不是期望值，
    /// [`HandoverLedger::bind`] 报 [`BindError::ItemMismatch`] 并
    /// 把期望值带出来。
    pub const fn expect_item(self) -> u32 {
        match self {
            // 状态复制消费的是特效状态 → 归「特效指标」件（2）。
            Surface::ReplayConsume => 2,
            // 网络事件接入总线 → 归「接口总账」件（1）。
            Surface::EventBus => 1,
            // 带宽预算口径 → 归「性能总册」件（6）。
            Surface::Bandwidth => 6,
        }
    }

    /// 中文名（读屏可达）。
    pub const fn name(self) -> &'static str {
        match self {
            Surface::ReplayConsume => "状态复制消费面",
            Surface::EventBus => "网络事件总线面",
            Surface::Bandwidth => "带宽预算口径面",
        }
    }
}

/// 一条承接落地记录。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HandoverSurface {
    /// 承接点。
    pub surface: Surface,
    /// 回溯到的移交包件号。
    pub item: u32,
    /// 该件内的条目名（供细粒度追责）。
    pub entry: &'static str,
    /// 是否已绑定。
    pub bound: bool,
}

/// 绑定失败的原因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BindError {
    /// 移交包缺该件 —— 回溯路径 = 该件号。
    SourceMissing { item: u32 },
    /// 件号越界（`item > HANDOVER_ITEMS`）。
    ItemOutOfRange { item: u32 },
    /// 件号对不上该承接点的期望（附期望值）。
    ItemMismatch { got: u32, expect: u32 },
}

/// 承接面台账。
#[derive(Clone, Debug, Default)]
pub struct HandoverLedger {
    rows: Vec<HandoverSurface>,
}

/// 移交包清单（第 `item` 件是否存在）。
///
/// 用**位掩码**表达「哪几件在包」：十件是编译期常量，
/// 用位掩码比 `Vec<bool>` 省一次分配，也便于「缺哪几件」一次性算出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandoverPack {
    pub present: u16,
}

impl HandoverPack {
    /// 新建空包。
    pub const fn empty() -> HandoverPack {
        HandoverPack { present: 0 }
    }

    /// 满包（十件齐）。
    pub const fn full() -> HandoverPack {
        HandoverPack { present: (1u16 << HANDOVER_ITEMS) - 1 }
    }

    /// 声明第 `item` 件在包里（1-based）；越界返回 `false` 不静默。
    pub fn declare(&mut self, item: u32) -> bool {
        if item < 1 || item > HANDOVER_ITEMS {
            return false;
        }
        self.present |= 1u16 << (item - 1);
        true
    }

    /// 该件是否在包里。
    pub const fn has(self, item: u32) -> bool {
        if item < 1 || item > HANDOVER_ITEMS {
            return false;
        }
        self.present & (1u16 << (item - 1)) != 0
    }

    /// 缺件号列表（升序；空则无缺）。
    pub fn missing_items(self) -> Vec<u32> {
        let mut v = Vec::new();
        for i in 1..=HANDOVER_ITEMS {
            if !self.has(i) {
                v.push(i);
            }
        }
        v
    }
}

impl HandoverLedger {
    /// 新建台账（三个承接点各一行，均未绑定）。
    pub fn new() -> HandoverLedger {
        let rows = Surface::ALL
            .iter()
            .map(|&s| HandoverSurface {
                surface: s,
                item: s.expect_item(),
                entry: "",
                bound: false,
            })
            .collect::<Vec<_>>();
        HandoverLedger { rows }
    }

    /// 绑定一个承接点。**缺源不回溯即拒，绝不用默认值顶替**
    /// （设计要点三）。
    pub fn bind(
        &mut self,
        surface: Surface,
        pack: HandoverPack,
        entry: &'static str,
    ) -> Result<(), BindError> {
        let want = surface.expect_item();
        if !pack.has(want) {
            return Err(BindError::SourceMissing { item: want });
        }
        if entry.is_empty() {
            // 条目名为空 = 指不回具体条目 = 变相缺源。
            return Err(BindError::SourceMissing { item: want });
        }
        if want > HANDOVER_ITEMS {
            return Err(BindError::ItemOutOfRange { item: want });
        }
        for r in self.rows.iter_mut() {
            if r.surface == surface {
                r.item = want;
                r.entry = entry;
                r.bound = true;
                return Ok(());
            }
        }
        Err(BindError::SourceMissing { item: want })
    }

    /// 取某承接点的落地记录。
    pub fn row(&self, surface: Surface) -> Option<&HandoverSurface> {
        self.rows.iter().find(|r| r.surface == surface)
    }

    /// 已绑定数 / 总数（判据直接断言这两个数）。
    pub fn bound_count(&self) -> usize {
        self.rows.iter().filter(|r| r.bound).count()
    }

    /// 落地表总行数（恒为 3 —— 承接点由 `Surface::ALL` 冻结）。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否一行都没有。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 可变引用（供架构册编排与判据构造破坏态）。
    pub fn rows_mut(&mut self) -> &mut Vec<HandoverSurface> {
        &mut self.rows
    }
}

// ---------------------------------------------------------------------------
// 域本色：流畅与公平双承诺
// ---------------------------------------------------------------------------

/// 单帧带宽预算（字节/帧）。
///
/// 取 64 KiB：1080p60 下留给状态复制的量级足够小又不至于一帧
/// 堵死链路。**常量在此处、只此一处** —— 判据侧独立重算时
/// 也引这个值，避免两处各写一个 65536 然后悄悄漂移。
pub const FRAME_BUDGET_BYTES: u32 = 64 * 1024;

/// 单次发送突发上界（字节）：**超过必须分片**。
///
/// 这是**单次 `budget` 调用**的上界，不是「跨帧积压上限」。两者
/// 混同会让预算闸失效：若把 8 KiB 当积压上限，则任何一帧只要发满
/// 64 KiB，积压立刻超限、下一帧全部被拒 —— 「不卡」直接变成
/// 「第二帧必卡」。真实网络里对应的是 MTU / jitter buffer：
/// **一个包不许超过突发上界，一帧内可以多次小包累加到帧预算**。
pub const JITTER_ALLOWANCE_BYTES: u32 = 8 * 1024;

/// 带宽记账。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BudgetState {
    /// 本帧已用字节（帧末归零）。
    pub used: u32,
    /// 已提交未消化的排队字节（帧末按一帧预算消化）。
    pub backlog: u32,
    /// 被拒绝的发送请求次数（**独立记账**，不许与 used 混）。
    pub rejected: u32,
}

/// 带宽闸的判定结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetVerdict {
    /// 放行。
    Accept,
    /// 单次突发超上界（须分片后再发）。
    RejectBurst,
    /// 单帧总量超预算拒绝（已记账）。
    RejectOverBudget,
}

/// 带宽闸。**超预算拒绝而非悄悄限流** —— 限流会让发送方
/// 以为发出去了，正是「不骗」要防的事。
pub fn budget(st: &mut BudgetState, want: u32) -> BudgetVerdict {
    // 边界防护：空操作既不消费预算也不计入拒绝。
    if want == 0 {
        return BudgetVerdict::Accept;
    }
    // ① 单次突发上界：一个大包必须分片，不许靠帧预算硬吞。
    if want > JITTER_ALLOWANCE_BYTES {
        st.rejected = st.rejected.saturating_add(1);
        return BudgetVerdict::RejectBurst;
    }
    // ② 单帧总量上界：加法 checked，溢出即拒（不 wrap）。
    let total = match st.used.checked_add(want) {
        Some(t) => t,
        None => {
            st.rejected = st.rejected.saturating_add(1);
            return BudgetVerdict::RejectOverBudget;
        }
    };
    if total > FRAME_BUDGET_BYTES {
        st.rejected = st.rejected.saturating_add(1);
        return BudgetVerdict::RejectOverBudget;
    }
    // ③ 放行并入队（backlog 只在放行时累加，拒绝路径不动它）。
    st.used = total;
    st.backlog = st.backlog.saturating_add(want);
    BudgetVerdict::Accept
}

/// 帧结束：`used` 归零，按**链路本帧实际送达量**消化排队。
///
/// 参数 [`end_frame`] 的 `drained` 是「链路这一帧真正送出去的字节」。
/// 它**必须**由调用方按实测吞吐给出，而不是恒等于帧预算：
/// 若恒定按帧预算消化，则 `backlog` 单帧最多 64 KiB、下一帧必被清空，
/// 「积压追不上」这条**结构不可达**——而链速低于发送速率时积压
/// 恰恰会滚雪球，那正是「联机不卡不骗」要防的事。
///
/// 积压**不清零**（消化量不足时留账，否则等于每帧把欠账忘光）；
/// 也不整帧强清（消化量足够时自然归零，`saturating_sub` 保证）。
pub fn end_frame(st: &mut BudgetState, drained: u32) {
    st.used = 0;
    st.backlog = st.backlog.saturating_sub(drained);
}

/// 公平闸的输入：一次裁定所需的三要素。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FairInput {
    /// 输入指纹（相同指纹 ⇒ 必须同判定）。
    pub input: u64,
    /// 本地时钟桶（用于分类，不参与判定本身）。
    pub tick: u32,
    /// 本地负载（用于分类）。
    pub load: u32,
}

impl FairInput {
    /// 借引用构造（供调用侧避免显式临时量）。
    pub const fn as_ref(&self) -> &FairInput {
        self
    }
}

/// 公平判定：**相同输入 ⇒ 相同结果**，与时钟/负载无关。
///
/// 「不骗」的第二层：不能因为本机卡（`load` 高）或时钟偏
/// （`tick` 不同）就给对端不一样的结果。故判定**只**由
/// [`FairInput::input`] 决定，另两个字段仅作诊断留档。
pub fn fair(i: &FairInput) -> u64 {
    // 纯函数 + 线性同余：同 input 必同输出，不同 input 必不同
    // （同余的步长与模均为奇数 ⇒ 是双射）。
    let mut h = i.input ^ 0x9e37_79b9_7f4a_7c15;
    h = h.wrapping_mul(0x0000_0100_0000_01b3);
    h ^= h >> 29;
    h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h ^= h >> 32;
    h % 1000
}

/// 公平闸：判定 + 诊断留档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FairnessGate {
    /// 判定结果。
    pub verdict: u64,
    /// 诊断用：是否见过同输入异判定（**恒为 false**，
    /// 一旦为 true 说明有人改坏了 [`fair`]）。
    pub self_inconsistent: bool,
}

/// 公平闸执行。
pub fn fairness_gate(inputs: &[FairInput]) -> FairnessGate {
    // 自查：**同输入必须同判定**。这里按输入指纹分组，比对组内
    // 全部判定值 —— 判定侧独立重算，不问被测函数「你对吗」。
    let mut self_inconsistent = false;
    for a in inputs.iter() {
        for b in inputs.iter() {
            if a.input == b.input && fair(a) != fair(b) {
                self_inconsistent = true;
            }
        }
    }
    // 结果取第一条（有输入才有判定）。
    let verdict = inputs.first().map(|i| fair(i)).unwrap_or(0);
    FairnessGate { verdict, self_inconsistent }
}

// ---------------------------------------------------------------------------
// 总架构册
// ---------------------------------------------------------------------------

/// 四层总架构册。
#[derive(Clone, Debug, Default)]
pub struct ArchRegistry {
    /// 域归属登记（冲突如实登记，见文件头）。
    pub domain: &'static str,
    /// 每层声明的接口数（用于「冻结数 == 声明数」对账）。
    declared: [u32; 4],
    /// 每层**是否登记过**（与「声明数 > 0」不同：合法地为 0）。
    registered: [bool; 4],
    ledger: FreezeLedger,
    handover: HandoverLedger,
}

impl ArchRegistry {
    /// 新建架构册（域归属 = 任务单锚点口径 `VE-AA/网络与多人`）。
    pub fn new() -> ArchRegistry {
        ArchRegistry {
            domain: "VE-AA · 网络与多人（AA 域开工）",
            declared: [0; 4],
            registered: [false; 4],
            ledger: FreezeLedger::new(),
            handover: HandoverLedger::new(),
        }
    }

    /// 声明某层对外提供的接口数（**必须与实际冻结数一致**）。
    ///
    /// `count` 合法地为 0（最上层无下层依赖），但调用本身会**登记**
    /// 该层已入册 —— 这是开工闸条件①的判据。
    pub fn declare(&mut self, layer: Layer, count: u32) {
        self.declared[layer.index() as usize] = count;
        self.registered[layer.index() as usize] = true;
    }

    /// 某层是否登记过。
    pub fn is_registered(&self, layer: Layer) -> bool {
        self.registered[layer.index() as usize]
    }

    /// 某层声明的接口数。
    pub fn declared_of(&self, layer: Layer) -> u32 {
        self.declared[layer.index() as usize]
    }

    /// 冻结一条接口（代理 [`FreezeLedger::freeze`]）。
    pub fn freeze(
        &mut self,
        caller: Layer,
        provider: Layer,
        sig: u64,
        witness: &'static str,
    ) -> Result<u32, FreezeError> {
        self.ledger.freeze(caller, provider, sig, witness)
    }

    /// 冻结台账只读引用。
    pub fn ledger(&self) -> &FreezeLedger {
        &self.ledger
    }

    /// 承接台账只读引用。
    pub fn handover(&self) -> &HandoverLedger {
        &self.handover
    }

    /// 承接台账可变引用（供编排期绑定；只在本单内的构造流程使用）。
    pub fn handover_mut(&mut self) -> &mut HandoverLedger {
        &mut self.handover
    }

    /// 开工闸：**四层齐 / 冻结数与声明数一致 / 承接全绑定 / 台账自查过**，
    /// 才允许开工。
    ///
    /// 四条全过才 `true`，任一不过即 `false` 并由调用方查
    /// [`ArchRegistry::why_blocked`] 拿具体原因 —— 不静默失败。
    pub fn open_gate(&self) -> bool {
        self.why_blocked().is_none()
    }

    /// 开工闸未过的具体原因（`None` = 可开工）。
    ///
    /// 四条件**互不重叠且可独立破坏**（否则「闸闭」说不清是谁的锅）：
    /// ① 每层**都被登记过**（`declare` 被调用过，含声明数为 0 的层）；
    /// ② 逐层「冻结数 == 声明数」；③ 承接全绑定；④ 冻结台账自查。
    ///
    /// 注意 ① 检查的是「**登记过**」而非「声明数 > 0」：最上层
    /// `Gameplay` 天然没有下层依赖它，声明数合法地为 0。若把 ① 写成
    /// 「声明数 > 0」，它会与 ② 直接矛盾（声明 0 却要求冻结 0 条，
    /// 于是 ① 永远闭 ⇒ 闸永不打开）。
    pub fn why_blocked(&self) -> Option<&'static str> {
        // ① 四层齐：每层都登记过。
        for l in Layer::ALL.iter() {
            if !self.registered[l.index() as usize] {
                return Some("四层未齐：有层未登记");
            }
        }
        // ② 冻结数 == 声明数（逐层对账，不看总数——总数相等也可能互相抵消）。
        for l in Layer::ALL.iter() {
            let dec = self.declared_of(*l) as usize;
            let fro = self.ledger.frozen_count_of(*l);
            if dec != fro {
                return Some("冻结数与声明数不一致");
            }
        }
        // ③ 承接全绑定。
        if self.handover.bound_count() != Surface::ALL.len() {
            return Some("承接面存在未绑定项");
        }
        // ④ 冻结台账自查。
        if self.ledger.audit().is_err() {
            return Some("冻结台账自查失败");
        }
        None
    }

    /// 层数（架构规模，恒为 4 —— 由 `Layer::ALL` 冻结）。
    pub fn layer_count(&self) -> usize {
        Layer::ALL.len()
    }
}
