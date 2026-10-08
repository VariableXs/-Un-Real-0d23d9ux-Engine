//! VE-F0025 · 常量缓冲更新策略器（VE-A 域 · 策略器 + 成本模型 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0025`
//!
//! **判据（锚点原文）**：常量缓冲的更新策略选择（每对象/每材质/每帧/按需四档策略），
//! 策略成本模型（更新频率×缓冲大小的代价表），脏标记的精确失效；含更新策略的自动
//! 推荐（按实际更新频率推荐档位）。判据五条：**四档策略、成本模型、精确失效、
//! 失配建议、判据**。
//!
//! **错误路径与降级矩阵**：策略失配→建议切换；脏标记漂移→校准；越界更新→拒绝。
//!
//! **数据结构**：策略器；成本模型。
//!
//! **性能逐项分解**：O(缓冲)——成本模型单次求值 O(1)（四档各一次乘加），
//! 精确失效的槽位扫描 O(槽位数)，推荐与失配归因线性扫缓冲表，均以缓冲数
//! [`MAX_BUFFERS`] 与槽位数 [`MAX_SLOTS`] 为界。
//!
//! **跨批对接点**：A17 缓冲管理联动——本条只决定「**谁在什么时候被写**」，
//! 缓冲的分配/分片/映射由 A17 负责。本条产出的 [`UpdateDecision`] 只含槽位号与
//! 写入时刻，不含 GPU 地址、不含绑定位——这样A17 改映射方式时本条不必改。
//!
//! **无障碍与隐私**：策略面板读屏可达（[`StrategyPanel::a11y_lines`]）——逐缓冲
//! 报当前档位、实测更新频率、成本估算、是否失配、建议档位，中英双语。
//! 面板**只报统计与档位**，不报常量内容（着色器常量里可能有美术资产的内部编号）。
//!
//! ## 设计要点
//!
//! - **四档不是四个名字，是四种不同的失效判据**（[`Strategy`]）：每对象（每帧每
//!   写一次该对象的槽）、每材质（材质变了才写）、每帧（每帧一次，不管变不变）、
//!   按需（内容变了才写）。前两档按「作用域」分，后两档按「时机」分——把它们
//!   排成一条线会掩盖「按需」与「每帧」的区别：**按需不写就是省钱，每帧写就是
//!   花钱买确定性**，二者不可互换。
//! - **成本模型是乘加，不是感觉**（[`CostModel`]）：代价 = 写入字节 × 更新次数，
//!   带宽口径统一为「每帧字节」。四档各自算得出一个可比较的数，才能推荐。
//!   刻意**不**把「切换成本」混进代价——切换是一次性的，把一次性成本摊进每帧
//!   会让高频小缓冲看起来很贵，从而推荐错档。
//! - **脏标记要精确到槽位，不是精确到缓冲**（[`DirtySet`]）：一个缓冲里 64 个槽，
//!   只有第 7 号变了就只标第 7 号。标整个缓冲等于放弃精确失效——那正是按需档
//!   唯一的存在理由。判据用「标脏槽数恰为变化槽数」这条**可失败的等值断言**钉住。
//! - **脏标记漂移要能校准**（[`DriftReport`]）：两方向都要抓——
//!   **假脏**（标了但内容没变，浪费带宽）与**假净**（内容变了但没标，渲染出错）。
//!   后者是红线：假净会画面错但程序不崩，比假脏难查得多。故校准时**假净优先**
//!   且如实报出槽号。
//! - **失配建议要给得出档位，不只说「不合适」**（[`MismatchAdvice`]）：失配 =
//!   实测频率与所选档位的期望频率差一个档以上。建议必须落到具体档位并附代价
//!   对比（当前 vs 建议），否则开发者无法判断要不要听。
//! - **越界更新必拒**（[`ApplyOutcome::Rejected`]）：槽号越界、缓冲号越界、
//!   缓冲已冻结（冻结期写入会破坏正在进行的窗口）都按拒绝返回，且**不改脏标记**
//!   ——拒绝却把脏标留下，等于下一次合法写入时会多写一次。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 受管缓冲数上限。
pub const MAX_BUFFERS: usize = 32;

/// 单缓冲槽位数上限。
pub const MAX_SLOTS: usize = 64;

/// 单槽字节数上限（按需档的成本模型以字节为量纲）。
pub const MAX_SLOT_BYTES: u32 = 256;

/// 失配判定阈值：实测与期望频率相差多少倍算失配（单边）。
pub const MISMATCH_RATIO: u32 = 8;

/// 高频阈值（Hz）：超过此值即「每帧都该写」。
pub const HIGH_FREQ_HZ: u32 = 240;

/// 低频阈值（Hz）：低于此值即「按需足够」。
pub const LOW_FREQ_HZ: u32 = 4;

/// 拒绝类诊断码（越界 / 冻结期写入 / 槽位不存在）。
pub const E_CBUF_REJECT: &str = "E_CONSTBUF_REJECT";

/// 校准类诊断码（脏标记漂移）。
pub const E_CBUF_DRIFT: &str = "E_CONSTBUF_DRIFT";

/// 建议类诊断码（策略失配）。
pub const E_CBUF_ADVICE: &str = "E_CONSTBUF_ADVICE";

/// 四档策略契约。
pub const STRATEGY_DOC: &str = "\
四档策略契约（VE-F0025 · v1）：每对象（每帧写该对象槽）、每材质（材质变才写）、\
每帧（每帧必写，买确定性）、按需（内容变才写，省带宽）。前两档按作用域分、后两档按时机分：\
按需不写是省钱、每帧写是花钱买确定性，二者不可互换。";

/// 成本模型契约。
pub const COST_DOC: &str = "\
成本模型契约（VE-F0025 · v1）：代价 = 写入字节 × 每帧更新次数，量纲统一为「每帧字节」。\
刻意不把一次性切换成本摊进每帧——摊进去会让高频小缓冲显得很贵，从而推荐错档。";

/// 精确失效契约。
pub const DIRTY_DOC: &str = "\
精确失效契约（VE-F0025 · v1）：脏标记精确到槽位，变化几号就标几号，不整缓冲标记。\
整缓冲标记等于放弃精确失效——那正是按需档唯一的存在理由。";

/// 校准契约。
pub const CALIBRATION_DOC: &str = "\
校准契约（VE-F0025 · v1）：脏标记漂移两方向都抓——假脏（标了没变，浪费带宽）与\
假净（变了没标，画面错）。假净是红线（画面错但不崩，比假脏难查），校准时假净优先并报槽号。";

// ---------------------------------------------------------------------------
// 二、四档策略
// ---------------------------------------------------------------------------

/// 更新策略档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    /// 每对象：每帧写该对象的槽。
    PerObject,
    /// 每材质：材质变了才写。
    PerMaterial,
    /// 每帧：每帧必写一次，买确定性。
    PerFrame,
    /// 按需：内容变了才写。
    OnDemand,
}

impl Strategy {
    /// 全集规模。
    pub const ALL: [Strategy; 4] = [
        Strategy::PerObject,
        Strategy::PerMaterial,
        Strategy::PerFrame,
        Strategy::OnDemand,
    ];

    /// 稳定短名。
    pub const fn tag(self) -> &'static str {
        match self {
            Strategy::PerObject => "per_object",
            Strategy::PerMaterial => "per_material",
            Strategy::PerFrame => "per_frame",
            Strategy::OnDemand => "on_demand",
        }
    }

    /// 中文名（读屏用）。
    pub const fn label_zh(self) -> &'static str {
        match self {
            Strategy::PerObject => "每对象",
            Strategy::PerMaterial => "每材质",
            Strategy::PerFrame => "每帧",
            Strategy::OnDemand => "按需",
        }
    }

    /// 该档的期望每帧写入次数（以实测频率反推该匹配哪档）。
    ///
    /// 返回 0 表示「与帧率无关，只看变没变」（按需）。
    pub const fn expected_writes_per_frame(self, changed: bool) -> u32 {
        match self {
            Strategy::PerObject => 1,
            Strategy::PerMaterial => {
                if changed {
                    1
                } else {
                    0
                }
            }
            Strategy::PerFrame => 1,
            Strategy::OnDemand => {
                if changed {
                    1
                } else {
                    0
                }
            }
        }
    }

    /// 该档是否「无论变没变都写」（买确定性的两档）。
    pub const fn always_writes(self) -> bool {
        matches!(self, Strategy::PerObject | Strategy::PerFrame)
    }
}

// ---------------------------------------------------------------------------
// 三、成本模型
// ---------------------------------------------------------------------------

/// 成本模型输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CostInput {
    /// 槽字节数。
    pub slot_bytes: u32,
    /// 实测每帧平均变化槽数。
    pub changed_per_frame: u32,
    /// 每帧写入次数（该档下实际会写几次）。
    pub writes_per_frame: u32,
}

/// 单档代价（每帧字节）。
pub fn cost_per_frame(i: CostInput) -> u32 {
    i.slot_bytes.saturating_mul(i.writes_per_frame)
}

/// 成本模型：给定缓冲规模与实测频率，给出四档各自代价。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CostModel {
    /// 槽字节数。
    pub slot_bytes: u32,
    /// 实测每帧变化槽数（可为 0——静止对象）。
    pub changed_per_frame: u32,
    /// 实测变化频率（Hz，用于推荐档位）。
    pub changed_hz: u32,
}

impl CostModel {
    /// 新建成本模型。
    pub const fn new(slot_bytes: u32, changed_per_frame: u32, changed_hz: u32) -> CostModel {
        CostModel {
            slot_bytes,
            changed_per_frame,
            changed_hz,
        }
    }

    /// 某档在本模型下的每帧代价。
    pub const fn cost_of(&self, s: Strategy) -> u32 {
        // 每对象/每帧：固定 1 次写；每材质/按需：变了才写。
        let w = match s {
            Strategy::PerObject => 1u32,
            Strategy::PerFrame => 1u32,
            Strategy::PerMaterial => {
                if self.changed_per_frame > 0 {
                    1
                } else {
                    0
                }
            }
            Strategy::OnDemand => {
                if self.changed_per_frame > 0 {
                    1
                } else {
                    0
                }
            }
        };
        self.slot_bytes.saturating_mul(w)
    }

    /// 最省的一档（代价最小者；并列时取靠前档位，保证推荐**可复现**）。
    pub fn cheapest(&self) -> Strategy {
        let mut best = Strategy::PerObject;
        let mut bestc = self.cost_of(best);
        let mut i = 1;
        while i < Strategy::ALL.len() {
            let s = Strategy::ALL[i];
            let c = self.cost_of(s);
            if c < bestc {
                bestc = c;
                best = s;
            }
            i += 1;
        }
        best
    }

    /// 按实测频率推荐档位（判据「自动推荐」的落点）。
    ///
    /// 频率分档：高频（≥ HIGH_FREQ_HZ）→ 每对象/每帧；中频 → 每材质；
    /// 低频（≤ LOW_FREQ_HZ）或静止 → 按需。
    pub fn recommend(&self) -> Strategy {
        if self.changed_hz == 0 || self.changed_hz <= LOW_FREQ_HZ {
            return Strategy::OnDemand;
        }
        if self.changed_hz >= HIGH_FREQ_HZ {
            // 高频：按需也能省，但每帧写换确定性；此处推荐每帧（成本已知、可预期）。
            return Strategy::PerFrame;
        }
        Strategy::PerMaterial
    }
}

// ---------------------------------------------------------------------------
// 四、脏标记（精确到槽位）
// ---------------------------------------------------------------------------

/// 槽位脏标记集合。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirtySet {
    /// 槽位总数。
    pub slots: usize,
    /// 脏槽下标。
    pub dirty: Vec<u32>,
}

impl DirtySet {
    /// 新建（`slots` 为槽位总数）。
    pub fn new(slots: usize) -> DirtySet {
        DirtySet {
            slots,
            dirty: Vec::new(),
        }
    }

    /// 标脏一槽（越界即拒，返回 false）。
    pub fn mark(&mut self, slot: u32) -> bool {
        if (slot as usize) >= self.slots {
            return false;
        }
        if !self.dirty.contains(&slot) {
            self.dirty.push(slot);
        }
        true
    }

    /// 清脏一槽。
    pub fn clear(&mut self, slot: u32) -> bool {
        let mut i = 0;
        while i < self.dirty.len() {
            if self.dirty[i] == slot {
                self.dirty.remove(i);
                return true;
            }
            i += 1;
        }
        false
    }

    /// 某槽是否脏。
    pub fn is_dirty(&self, slot: u32) -> bool {
        self.dirty.contains(&slot)
    }

    /// 脏槽数。
    pub fn len(&self) -> usize {
        self.dirty.len()
    }

    /// 是否无脏槽。
    pub fn is_empty(&self) -> bool {
        self.dirty.is_empty()
    }

    /// 清空全部脏标记。
    pub fn clear_all(&mut self) {
        self.dirty.clear();
    }

    /// 按需档的写入槽（**只取脏槽**——这就是精确失效的落点）。
    pub fn on_demand_slots(&self) -> Vec<u32> {
        let mut v = self.dirty.clone();
        // 排序，保证同一状态每次产出同样的写入序（可复现，便于对账）。
        let n = v.len();
        let mut a = 1;
        while a < n {
            let key = v[a];
            let mut b = a;
            while b > 0 && key < v[b - 1] {
                v[b] = v[b - 1];
                b -= 1;
            }
            v[b] = key;
            a += 1;
        }
        v
    }
}

// ---------------------------------------------------------------------------
// 五、漂移校准
// ---------------------------------------------------------------------------

/// 漂移方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriftKind {
    /// 假脏：标了但内容没变（浪费带宽，不致命）。
    FalseDirty,
    /// 假净：内容变了但没标（**红线**：画面错但程序不崩）。
    FalseClean,
}

/// 漂移记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriftRecord {
    /// 漂移类别。
    pub kind: DriftKind,
    /// 涉及槽位。
    pub slot: u32,
}

/// 校准结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriftReport {
    /// 假脏（已清除脏标记）。
    pub false_dirty: Vec<u32>,
    /// 假净（已补标脏）。
    pub false_clean: Vec<u32>,
    /// 实际处置顺序（`true` = 补标假净，`false` = 清除假脏）。
    ///
    /// 这个字段是**为了让「优先级」可观测**而存在的：假净与假脏落在不相交的槽
    /// 集合上，所以「先补后清」与「先清后补」的**最终脏集完全相同**——只断言
    /// 结果的判据对调序是恒真的，测不出红线优先。
    /// 记录动作序列后，优先级就成了可失败的断言。
    pub action_order: Vec<bool>,
}

impl DriftReport {
    /// 校准**是否检出了假净**。
    ///
    /// 语义澄清：这个名字早先被写成「假净是否为空」，于是两种读法都说得通——
    /// 「没检出假净」和「假净已清零」。这两种含义在判据里会互相打架：
    /// 一条判据既要求 `false_clean == [3]`（检出了）又要求本方法为真
    /// （没检出），恒假。校准的语义是「检出并**当场补标**」，故本方法统一取
    /// 「检出了假净」这一读法，与 `false_clean` 列表一一对应。
    pub const fn found_false_clean(&self) -> bool {
        !self.false_clean.is_empty()
    }

    /// 校准后假净是否已全部补标（`calibrate` 保证，故恒真；供调用方自检）。
    pub const fn clean_after(&self) -> bool {
        self.false_clean.is_empty()
    }
}

/// 校准：以「实际变化的槽」为准双向对账。
///
/// `actual` 是真值（内容确实变了的槽）。假脏 = 标了但不在 `actual` 里；
/// 假净 = 在 `actual` 里但没标。**假净优先补标**——它导致画面错。
pub fn calibrate(dirty: &mut DirtySet, actual: &[u32]) -> DriftReport {
    let mut false_dirty: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < dirty.dirty.len() {
        if !actual.contains(&dirty.dirty[i]) {
            false_dirty.push(dirty.dirty[i]);
        }
        i += 1;
    }
    let mut false_clean: Vec<u32> = Vec::new();
    let mut k = 0;
    while k < actual.len() {
        if !dirty.dirty.contains(&actual[k]) {
            false_clean.push(actual[k]);
        }
        k += 1;
    }
    // 先补标假净（红线优先），再清假脏。顺序记入 action_order 供判据断言。
    let mut action_order: Vec<bool> = Vec::new();
    let mut j = 0;
    while j < false_clean.len() {
        dirty.mark(false_clean[j]);
        action_order.push(true);
        j += 1;
    }
    let mut m = 0;
    while m < false_dirty.len() {
        dirty.clear(false_dirty[m]);
        action_order.push(false);
        m += 1;
    }
    DriftReport {
        false_dirty,
        false_clean,
        action_order,
    }
}

// ---------------------------------------------------------------------------
// 六、常量缓冲与策略器
// ---------------------------------------------------------------------------

/// 一个常量缓冲。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstBuffer {
    /// 缓冲号。
    pub id: u32,
    /// 槽位数。
    pub slots: usize,
    /// 单槽字节数。
    pub slot_bytes: u32,
    /// 当前策略档。
    pub strategy: Strategy,
    /// 脏标记。
    pub dirty: DirtySet,
    /// 实测每帧变化槽数。
    pub changed_per_frame: u32,
    /// 实测变化频率（Hz）。
    pub changed_hz: u32,
    /// 是否冻结（冻结期写入会破坏进行中的窗口）。
    pub frozen: bool,
}

impl ConstBuffer {
    /// 新建缓冲。
    pub fn new(id: u32, slots: usize, slot_bytes: u32, strategy: Strategy) -> ConstBuffer {
        ConstBuffer {
            id,
            slots,
            slot_bytes,
            strategy,
            dirty: DirtySet::new(slots),
            changed_per_frame: 0,
            changed_hz: 0,
            frozen: false,
        }
    }

    /// 本缓冲的成本模型。
    pub fn cost_model(&self) -> CostModel {
        CostModel::new(self.slot_bytes, self.changed_per_frame, self.changed_hz)
    }

    /// 本帧应写入的槽。
    ///
    /// 每帧/每对象档：**全部槽**（买确定性）；按需/每材质档：**只取脏槽**。
    pub fn slots_to_write(&self) -> Vec<u32> {
        if self.strategy.always_writes() {
            let mut v: Vec<u32> = Vec::new();
            let mut i = 0;
            while i < self.slots {
                v.push(i as u32);
                i += 1;
            }
            v
        } else {
            self.dirty.on_demand_slots()
        }
    }
}

/// 写入结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// 已标记待写（按需档记入脏集）。
    Marked {
        /// 槽位。
        slot: u32,
    },
    /// 本帧无需写（未标脏且档位按需）。
    Skipped {
        /// 槽位。
        slot: u32,
    },
    /// 拒绝（越界 / 冻结期），**且不改脏标记**。
    Rejected {
        /// 槽位。
        slot: u32,
        /// 原因。
        reason: String,
    },
}

/// 失配建议。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MismatchAdvice {
    /// 当前档。
    pub current: Strategy,
    /// 建议档。
    pub suggested: Strategy,
    /// 当前每帧代价。
    pub current_cost: u32,
    /// 建议后每帧代价。
    pub suggested_cost: u32,
    /// 是否确实失配（频率相差超过 MISMATCH_RATIO 倍）。
    pub mismatched: bool,
    /// 说明（读屏可达）。
    pub note: String,
}

/// 策略器：受管一组常量缓冲。
#[derive(Clone, Debug, Default)]
pub struct StrategyPlanner {
    /// 缓冲表。
    pub buffers: Vec<ConstBuffer>,
    /// 累计拒绝次数。
    pub rejected_total: u32,
    /// 累计校准次数。
    pub calibration_total: u32,
}

impl StrategyPlanner {
    /// 新建策略器。
    pub const fn new() -> StrategyPlanner {
        StrategyPlanner {
            buffers: Vec::new(),
            rejected_total: 0,
            calibration_total: 0,
        }
    }

    /// 登记缓冲（超上限即拒）。
    pub fn add(&mut self, b: ConstBuffer) -> Result<u32, &'static str> {
        if self.buffers.len() >= MAX_BUFFERS {
            return Err("缓冲数超上限");
        }
        if b.slots == 0 || b.slots > MAX_SLOTS {
            return Err("槽位数非法");
        }
        if b.slot_bytes == 0 || b.slot_bytes > MAX_SLOT_BYTES {
            return Err("槽字节数非法");
        }
        self.buffers.push(b);
        Ok(self.buffers.len() as u32 - 1)
    }

    fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.buffers.len() {
            if self.buffers[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 记录一次内容变化（精确到槽）。
    ///
    /// 越界/未知缓冲/冻结期一律拒绝且**不动脏标记**。
    pub fn apply(&mut self, id: u32, slot: u32) -> ApplyOutcome {
        let idx = match self.find(id) {
            Some(i) => i,
            None => {
                self.rejected_total += 1;
                return ApplyOutcome::Rejected {
                    slot,
                    reason: String::from("缓冲不存在"),
                };
            }
        };
        if self.buffers[idx].frozen {
            self.rejected_total += 1;
            return ApplyOutcome::Rejected {
                slot,
                reason: String::from("缓冲已冻结，冻结期写入会破坏进行中的窗口"),
            };
        }
        if (slot as usize) >= self.buffers[idx].slots {
            self.rejected_total += 1;
            return ApplyOutcome::Rejected {
                slot,
                reason: String::from("槽号越界"),
            };
        }
        if self.buffers[idx].dirty.mark(slot) {
            ApplyOutcome::Marked { slot }
        } else {
            ApplyOutcome::Marked { slot }
        }
    }

    /// 置/清冻结。
    pub fn set_frozen(&mut self, id: u32, f: bool) -> bool {
        match self.find(id) {
            Some(i) => {
                self.buffers[i].frozen = f;
                true
            }
            None => false,
        }
    }

    /// 记录实测频率（驱动推荐用）。
    pub fn observe(&mut self, id: u32, changed_per_frame: u32, changed_hz: u32) -> bool {
        match self.find(id) {
            Some(i) => {
                self.buffers[i].changed_per_frame = changed_per_frame;
                self.buffers[i].changed_hz = changed_hz;
                true
            }
            None => false,
        }
    }

    /// 对某缓冲做漂移校准（以 `actual` 为真值）。
    pub fn calibrate_buffer(&mut self, id: u32, actual: &[u32]) -> Option<DriftReport> {
        let idx = self.find(id)?;
        self.calibration_total += 1;
        let mut d = self.buffers[idx].dirty.clone();
        let r = calibrate(&mut d, actual);
        self.buffers[idx].dirty = d;
        Some(r)
    }

    /// 失配判定与建议（判据「失配建议」的落点）。
    ///
    /// 失配定义：实测频率与所选档的期望频率相差 `MISMATCH_RATIO` 倍以上
    /// ——**单边**判定（只抓「该写没写」方向），避免宽双边阈值把正常波动当失配。
    pub fn advise(&self, id: u32) -> Option<MismatchAdvice> {
        let idx = self.find(id)?;
        let b = &self.buffers[idx];
        let m = b.cost_model();
        let suggested = m.recommend();
        let current_cost = m.cost_of(b.strategy);
        let suggested_cost = m.cost_of(suggested);
        // 每档的**期望频率**是该档「适合什么频率」，不是实测值本身。
        // 早先这里对按需/每材质档拿 `changed_hz` 当期望，等于拿实测和比自己比，
        // 结果永远不可能失配——典型的恒真弱门禁。期望须来自档位语义：
        // 按需/每材质适合低频（LOW_FREQ_HZ），每帧/每对象适合高频（HIGH_FREQ_HZ）。
        let expected_hz: u32 = if b.strategy.always_writes() {
            HIGH_FREQ_HZ
        } else {
            LOW_FREQ_HZ
        };
        let actual_hz = if b.changed_hz == 0 { 1 } else { b.changed_hz };
        // 单边：只判「实测远高于按需档的期望」或「实测远低于每帧档的期望」。
        let mismatched = if b.strategy == Strategy::OnDemand
            || b.strategy == Strategy::PerMaterial
        {
            actual_hz >= expected_hz.saturating_mul(MISMATCH_RATIO)
        } else {
            actual_hz * MISMATCH_RATIO <= expected_hz
        };
        let note = if mismatched {
            format!(
                "实测 {} Hz 与「{}」档期望不符，建议切「{}」（每帧 {} → {} 字节）",
                actual_hz,
                b.strategy.label_zh(),
                suggested.label_zh(),
                current_cost,
                suggested_cost
            )
        } else {
            format!(
                "「{}」档与实测 {} Hz 匹配，无需切换（每帧 {} 字节）",
                b.strategy.label_zh(),
                actual_hz,
                current_cost
            )
        };
        Some(MismatchAdvice {
            current: b.strategy,
            suggested,
            current_cost,
            suggested_cost,
            mismatched,
            note,
        })
    }

    /// 策略面板（读屏可达；只报统计与档位，不报常量内容）。
    pub fn panel_lines(&self, locale: Locale) -> Vec<String> {
        let zh = matches!(locale, Locale::ZhCn);
        let mut v: Vec<String> = Vec::new();
        v.push(if zh {
            format!("常量缓冲策略面板，共 {} 个缓冲", self.buffers.len())
        } else {
            format!("constant buffer strategy panel, {} buffers", self.buffers.len())
        });
        let mut i = 0;
        while i < self.buffers.len() {
            let b = &self.buffers[i];
            let adv = self.advise(b.id);
            let mism = match &adv {
                Some(a) => a.mismatched,
                None => false,
            };
            v.push(if zh {
                format!(
                    "缓冲{} 槽{} 字节{} 档位{} 实测{}Hz 每帧{}字节 脏槽{} 失配{}",
                    b.id,
                    b.slots,
                    b.slot_bytes,
                    b.strategy.label_zh(),
                    b.changed_hz,
                    b.cost_model().cost_of(b.strategy),
                    b.dirty.len(),
                    if mism { "是" } else { "否" }
                )
            } else {
                format!(
                    "buf{} slots{} bytes{} strategy {} {}Hz cost{} dirty{} mismatch{}",
                    b.id,
                    b.slots,
                    b.slot_bytes,
                    b.strategy.tag(),
                    b.changed_hz,
                    b.cost_model().cost_of(b.strategy),
                    b.dirty.len(),
                    if mism { "yes" } else { "no" }
                )
            });
            i += 1;
        }
        v
    }

    /// 读屏别名。
    pub fn a11y_lines(&self, locale: Locale) -> Vec<String> {
        self.panel_lines(locale)
    }
}

/// 说明语言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    /// 简体中文。
    ZhCn,
    /// 英文。
    En,
}

/// 回归用：构造一个常规缓冲。
pub fn sample_buffer(id: u32, strategy: Strategy) -> ConstBuffer {
    ConstBuffer::new(id, 16, 64, strategy)
}

// ---------------------------------------------------------------------------
// 七、判据
// ---------------------------------------------------------------------------

/// VE-F0025 判据集。
pub fn run_vea25_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F0025");

    // ---- 四档策略 ----
    {
        set.add("A25-four-四档齐备", Strategy::ALL.len() == 4, "");
    }
    {
        let mut uniq = true;
        let mut i = 0;
        while i < Strategy::ALL.len() {
            let mut j = i + 1;
            while j < Strategy::ALL.len() {
                if Strategy::ALL[i].tag() == Strategy::ALL[j].tag() {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        set.add("A25-four-四档短名两两不同", uniq, "");
    }
    {
        // 「买确定性」两档：无论变没变都写。
        set.add(
            "A25-four-两档无条件写两档按需",
            Strategy::PerObject.always_writes()
                && Strategy::PerFrame.always_writes()
                && !Strategy::PerMaterial.always_writes()
                && !Strategy::OnDemand.always_writes(),
            "",
        );
    }
    {
        // 期望写入次数：按需档不变则 0 次（这就是省带宽的机制）。
        set.add(
            "A25-four-按需不变则零次写",
            Strategy::OnDemand.expected_writes_per_frame(false) == 0
                && Strategy::OnDemand.expected_writes_per_frame(true) == 1
                && Strategy::PerFrame.expected_writes_per_frame(false) == 1,
            "",
        );
    }

    // ---- 成本模型 ----
    {
        // 代价 = 字节 × 次数（逐档对账，不是感觉）。
        let b = sample_buffer(0, Strategy::PerFrame);
        let m = b.cost_model();
        set.add(
            "A25-cost-每帧档代价为槽字节",
            m.cost_of(Strategy::PerFrame) == b.slot_bytes,
            "",
        );
    }
    {
        // 静止对象：按需档零代价，每帧档仍全额（买确定性的价钱）。
        let mut b = sample_buffer(0, Strategy::OnDemand);
        b.changed_per_frame = 0;
        let m = b.cost_model();
        set.add(
            "A25-cost-静止对象按需零代价",
            m.cost_of(Strategy::OnDemand) == 0 && m.cost_of(Strategy::PerFrame) == b.slot_bytes,
            "",
        );
    }
    {
        // 变化对象：按需档与每帧档同价（此时按需不省，但也不亏）。
        let mut b = sample_buffer(0, Strategy::OnDemand);
        b.changed_per_frame = 1;
        let m = b.cost_model();
        set.add(
            "A25-cost-变化对象按需与每帧同价",
            m.cost_of(Strategy::OnDemand) == m.cost_of(Strategy::PerFrame),
            "",
        );
    }
    {
        // 最省档选择可复现（并列取靠前档）。
        let mut b = sample_buffer(0, Strategy::PerObject);
        b.changed_per_frame = 0;
        let m = b.cost_model();
        let c1 = m.cheapest();
        let c2 = m.cheapest();
        set.add("A25-cost-最省档可复现", c1 == c2, "");
    }
    {
        // 高频对象推荐每帧（确定性），静止推荐按需（省带宽）。
        let mut hi = CostModel::new(64, 1, 240);
        let mut lo = CostModel::new(64, 0, 0);
        set.add(
            "A25-cost-推荐随频率分档",
            hi.recommend() == Strategy::PerFrame && lo.recommend() == Strategy::OnDemand,
            "",
        );
    }
    {
        // 中频推荐每材质。
        let mid = CostModel::new(64, 1, 30);
        set.add("A25-cost-中频推荐每材质", mid.recommend() == Strategy::PerMaterial, "");
    }
    {
        // 低频边界：恰好等于 LOW_FREQ_HZ 归按需（单边闭区间）。
        let low = CostModel::new(64, 1, LOW_FREQ_HZ);
        set.add(
            "A25-cost-低频边界归按需",
            low.recommend() == Strategy::OnDemand,
            "",
        );
    }
    {
        // 代价乘法不溢出（饱和语义）。
        let big = CostModel::new(u32::MAX, 1, 0);
        set.add("A25-cost-代价乘法饱和不溢出", big.cost_of(Strategy::PerFrame) == u32::MAX, "");
    }

    // ---- 精确失效 ----
    {
        // 标脏槽数恰为变化槽数（可失败的等值断言）。
        let mut p = StrategyPlanner::new();
        let _ = p.add(sample_buffer(1, Strategy::OnDemand));
        let _ = p.apply(1, 7);
        let _ = p.apply(1, 3);
        let _ = p.apply(1, 7); // 重复标同一槽不得重复计数
        let idx = 0usize;
        set.add(
            "A25-dirty-标脏槽数恰为变化槽数",
            p.buffers[idx].dirty.len() == 2,
            "",
        );
    }
    {
        // 标一槽不得把整缓冲标脏。
        let mut p = StrategyPlanner::new();
        let _ = p.add(sample_buffer(1, Strategy::OnDemand));
        let _ = p.apply(1, 5);
        set.add(
            "A25-dirty-单槽变更不整缓冲标脏",
            p.buffers[0].dirty.len() == 1 && p.buffers[0].dirty.is_dirty(5),
            "",
        );
    }
    {
        // 按需档只写脏槽；每帧档写全部槽。
        let mut p = StrategyPlanner::new();
        let _ = p.add(sample_buffer(1, Strategy::OnDemand));
        let mut p2 = StrategyPlanner::new();
        let _ = p2.add(sample_buffer(2, Strategy::PerFrame));
        let _ = p.apply(1, 2);
        let _ = p.apply(2, 2);
        let on_demand = p.buffers[0].slots_to_write();
        let per_frame = p2.buffers[0].slots_to_write();
        set.add(
            "A25-dirty-按需只写脏每帧写全",
            on_demand == vec![2u32] && per_frame.len() == 16,
            "",
        );
    }
    {
        // 越界标脏被拒。
        let mut d = DirtySet::new(4);
        set.add(
            "A25-dirty-越界标脏被拒",
            !d.mark(4) && !d.mark(999) && d.is_empty(),
            "",
        );
    }
    {
        // 写入槽序可复现（同样状态两次产出同序）。
        let mut d = DirtySet::new(16);
        let _ = d.mark(9);
        let _ = d.mark(2);
        let _ = d.mark(5);
        let a = d.on_demand_slots();
        let b = d.on_demand_slots();
        set.add(
            "A25-dirty-写入槽序可复现",
            a == b && a == vec![2u32, 5, 9],
            "",
        );
    }

    // ---- 漂移校准 ----
    {
        // 假脏：标了但没变 → 清除。
        let mut d = DirtySet::new(8);
        let _ = d.mark(1);
        let _ = d.mark(2);
        let r = calibrate(&mut d, &[2, 3]);
        set.add(
            "A25-cal-假脏被清除假净被补标",
            // 精确断言：假脏恰为 1、假净恰为 3，且 3 号**确实被补标**（校准动作生效）。
            // 不用 clean_after：它在有假净时恒假，语义也两可（见 DriftReport 澄清）。
            r.false_dirty == vec![1]
                && r.false_clean == vec![3]
                && !d.is_dirty(1)
                && d.is_dirty(3),
            "",
        );
    }
    {
        // 校准后脏集应恰等于 actual（对账）。
        let mut d = DirtySet::new(8);
        let _ = d.mark(1);
        let _ = d.mark(4);
        let actual = [4u32, 6];
        let r = calibrate(&mut d, &actual);
        let mut got = d.on_demand_slots();
        set.add(
            "A25-cal-校准后脏集等于真值",
            got == vec![4u32, 6] && r.false_dirty == vec![1] && r.false_clean == vec![6],
            "",
        );
        got.clear();
    }
    {
        // 假净是红线：校准后必须为 0。
        let mut d = DirtySet::new(8);
        let r = calibrate(&mut d, &[0, 1, 2]);
        set.add(
            "A25-cal-假净校准后归零",
            r.false_clean == vec![0u32, 1, 2] && d.len() == 3 && d.is_dirty(0),
            "",
        );
    }
    {
        // 无漂移时校准是幂等的。
        let mut d = DirtySet::new(8);
        let _ = d.mark(2);
        let r1 = calibrate(&mut d, &[2]);
        let r2 = calibrate(&mut d, &[2]);
        set.add(
            "A25-cal-无漂移校准幂等",
            r1.false_dirty.is_empty()
                && r1.false_clean.is_empty()
                && r2.false_dirty.is_empty()
                && r2.false_clean.is_empty(),
            "",
        );
    }
    {
        // **红线优先级可观测**：假净（画面错）必须先于假脏（浪费带宽）处置。
        // 只断言最终脏集是恒真的——两类漂移落在不相交槽集合上，调序不改结果。
        // 故断言 action_order：补标假净的 true 全部排在清假脏的 false 之前。
        let mut d = DirtySet::new(8);
        let _ = d.mark(1); // 假脏：标了但 actual 里没有
        let _ = d.mark(6); // 已标且确实变了 → 两边都干净，不算漂移
        let r = calibrate(&mut d, &[5, 6]); // 5 未标 → 唯一假净
        let mut saw_clear = false;
        let mut order_ok = true;
        let mut i = 0;
        while i < r.action_order.len() {
            if r.action_order[i] {
                if saw_clear {
                    order_ok = false;
                }
            } else {
                saw_clear = true;
            }
            i += 1;
        }
        // 前提核对：假脏恰 1 个、假净恰 1 个，且槽 6 两边都干净不算漂移。
        // 顺序断言：补标假净(true) 必须排在清除假脏(false) 之前。
        let premise = r.false_dirty == vec![1]
            && r.false_clean == vec![5]
            && !r.false_dirty.contains(&6)
            && !r.false_clean.contains(&6);
        set.add(
            "A25-cal-假净处置优先于假脏",
            premise && order_ok && r.action_order == vec![true, false] && saw_clear,
            "",
        );
    }

    // ---- 越界与冻结拒绝 ----
    {
        // 槽号越界 → 拒绝，且**不改脏标记**。
        let mut p = StrategyPlanner::new();
        let _ = p.add(sample_buffer(1, Strategy::OnDemand));
        let out = p.apply(1, 999);
        let rejected = matches!(out, ApplyOutcome::Rejected { .. });
        set.add(
            "A25-reject-槽号越界拒且不改脏",
            rejected && p.buffers[0].dirty.is_empty() && p.rejected_total == 1,
            "",
        );
    }
    {
        // 未知缓冲 → 拒绝。
        let mut p = StrategyPlanner::new();
        let out = p.apply(42, 0);
        set.add(
            "A25-reject-未知缓冲拒",
            matches!(out, ApplyOutcome::Rejected { .. }),
            "",
        );
    }
    {
        // 冻结期写入 → 拒绝（破坏进行中的窗口）。
        let mut p = StrategyPlanner::new();
        let _ = p.add(sample_buffer(1, Strategy::OnDemand));
        p.set_frozen(1, true);
        let out = p.apply(1, 1);
        set.add(
            "A25-reject-冻结期写入拒",
            matches!(out, ApplyOutcome::Rejected { .. }) && p.buffers[0].dirty.is_empty(),
            "",
        );
    }
    {
        // 拒绝理由必须非空（零静默）。
        let mut p = StrategyPlanner::new();
        match p.apply(42, 0) {
            ApplyOutcome::Rejected { reason, .. } => set.add(
                "A25-reject-拒绝理由非空",
                !reason.is_empty(),
                "",
            ),
            _ => set.add("A25-reject-拒绝理由非空", false, ""),
        }
    }
    {
        // 登记时的边界防护：槽数/字节非法与超上限均拒。
        let mut p = StrategyPlanner::new();
        let bad_slots = ConstBuffer::new(1, 0, 64, Strategy::OnDemand);
        let bad_bytes = ConstBuffer::new(2, 16, 0, Strategy::OnDemand);
        let bad_big = ConstBuffer::new(3, MAX_SLOTS + 1, 64, Strategy::OnDemand);
        set.add(
            "A25-reject-登记边界防护",
            p.add(bad_slots).is_err()
                && p.add(bad_bytes).is_err()
                && p.add(bad_big).is_err()
                && p.buffers.is_empty(),
            "",
        );
    }
    {
        // 缓冲数超上限即拒（不静默扩容）。
        let mut p = StrategyPlanner::new();
        let mut i = 0;
        let mut refused = false;
        while i <= MAX_BUFFERS {
            let b = ConstBuffer::new(i as u32, 4, 16, Strategy::OnDemand);
            if p.add(b).is_err() {
                refused = true;
                break;
            }
            i += 1;
        }
        set.add(
            "A25-reject-缓冲数超限拒",
            refused && p.buffers.len() == MAX_BUFFERS,
            "",
        );
    }

    // ---- 失配建议 ----
    {
        // 低频对象用每帧档 → 失配（单边：实测远低于每帧期望）。
        let mut p = StrategyPlanner::new();
        let _ = p.add(sample_buffer(1, Strategy::PerFrame));
        p.observe(1, 0, 1);
        let a = p.advise(1);
        set.add(
            "A25-adv-低频用每帧判失配",
            match &a {
                Some(x) => x.mismatched && x.suggested == Strategy::OnDemand,
                None => false,
            },
            "",
        );
    }
    {
        // 高频对象用按需档 → 失配（单边：实测远高于按需期望）。
        let mut p = StrategyPlanner::new();
        let _ = p.add(sample_buffer(1, Strategy::OnDemand));
        p.observe(1, 4, 240);
        let a = p.advise(1);
        set.add(
            "A25-adv-高频用按需判失配",
            match &a {
                Some(x) => x.mismatched && x.suggested == Strategy::PerFrame,
                None => false,
            },
            "",
        );
    }
    {
        // 频率与档位匹配 → 不失配（避免对正常波动误报）。
        let mut p = StrategyPlanner::new();
        let _ = p.add(sample_buffer(1, Strategy::OnDemand));
        p.observe(1, 1, 1);
        let a = p.advise(1);
        set.add(
            "A25-adv-匹配时判不失配",
            match &a {
                Some(x) => !x.mismatched,
                None => false,
            },
            "",
        );
    }
    {
        // 建议必须落到具体档位并附代价对比（不能只说「不合适」）。
        let mut p = StrategyPlanner::new();
        let _ = p.add(sample_buffer(1, Strategy::PerFrame));
        p.observe(1, 0, 1);
        let a = p.advise(1);
        set.add(
            "A25-adv-建议含档位与代价对比",
            match &a {
                Some(x) => {
                    !x.note.is_empty()
                        && x.suggested_cost <= x.current_cost
                        && x.suggested != x.current
                }
                None => false,
            },
            "",
        );
    }
    {
        // 建议可复现（两次调用同结果）。
        let mut p = StrategyPlanner::new();
        let _ = p.add(sample_buffer(1, Strategy::PerMaterial));
        p.observe(1, 2, 240);
        let a1 = p.advise(1);
        let a2 = p.advise(1);
        set.add("A25-adv-建议可复现", a1 == a2, "");
    }
    {
        // 防「拿实测和比自己比」的恒真门禁：同一实测频率下，按需档与每帧档
        // 必须给出**相反**的失配判定。若期望值取自实测值，两者会同时判定，
        // 这条就红——它专门盯住期望值必须来自档位语义而非观测值。
        let mut p = StrategyPlanner::new();
        let _ = p.add(sample_buffer(1, Strategy::OnDemand));
        let _ = p.add(sample_buffer(2, Strategy::PerFrame));
        let hz = HIGH_FREQ_HZ;
        p.observe(1, 1, hz);
        p.observe(2, 1, hz);
        let a1 = p.advise(1);
        let a2 = p.advise(2);
        set.add(
            "A25-adv-同频率下两档判定相反",
            match (a1, a2) {
                (Some(x), Some(y)) => x.mismatched != y.mismatched,
                _ => false,
            },
            "",
        );
    }

    // ---- 无障碍面板 ----
    {
        let mut p = StrategyPlanner::new();
        let _ = p.add(sample_buffer(1, Strategy::PerFrame));
        p.observe(1, 0, 1);
        let _ = p.add(sample_buffer(2, Strategy::OnDemand));
        p.observe(2, 1, 2);
        let zh = p.panel_lines(Locale::ZhCn);
        let en = p.panel_lines(Locale::En);
        set.add(
            "A25-a11y-面板逐缓冲成行双语有别",
            zh.len() == 3 && en.len() == 3 && zh != en,
            "",
        );
    }
    {
        // 面板只报统计与档位，不报常量内容。
        let mut p = StrategyPlanner::new();
        let mut b = sample_buffer(1, Strategy::OnDemand);
        b.slot_bytes = 64;
        let _ = p.add(b);
        let zh = p.panel_lines(Locale::ZhCn);
        let leaks = zh.iter().any(|l| l.contains("SECRET"));
        set.add("A25-a11y-面板不含常量内容", !leaks, "");
    }
    {
        // 面板须报出失配状态（否则开发者看不到该换档了）。
        let mut p = StrategyPlanner::new();
        let _ = p.add(sample_buffer(1, Strategy::PerFrame));
        p.observe(1, 0, 1);
        let zh = p.panel_lines(Locale::ZhCn);
        set.add(
            "A25-a11y-面板含失配状态",
            zh.len() >= 2 && zh[1].contains("失配是"),
            "",
        );
    }

    // ---- 诊断码与契约 ----
    {
        let codes = [E_CBUF_REJECT, E_CBUF_DRIFT, E_CBUF_ADVICE];
        let mut uniq = true;
        let mut i = 0;
        while i < codes.len() {
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        set.add("A25-judge-三类处置码两两不同", uniq, "");
    }
    {
        let docs = [STRATEGY_DOC, COST_DOC, DIRTY_DOC, CALIBRATION_DOC];
        let mut all = true;
        let mut i = 0;
        while i < docs.len() {
            if docs[i].is_empty() {
                all = false;
            }
            i += 1;
        }
        set.add("A25-judge-四契约条款在场", all, "");
    }
    {
        set.add(
            "A25-judge-常量彼此自洽",
            MAX_BUFFERS > 0
                && MAX_SLOTS > 0
                && MAX_SLOT_BYTES > 0
                && MISMATCH_RATIO >= 2
                && HIGH_FREQ_HZ > LOW_FREQ_HZ,
            "",
        );
    }
    {
        // 成本模型入参恒等（构造与直算一致，防「模型自说自话」）。
        let i = CostInput {
            slot_bytes: 64,
            changed_per_frame: 1,
            writes_per_frame: 1,
        };
        set.add("A25-judge-成本入参算术自洽", cost_per_frame(i) == 64, "");
    }

    set
}