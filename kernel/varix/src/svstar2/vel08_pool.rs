//! VE-F2208 · 粒子池与内存（VE-L 域 · 粒子与物理域 · 批次 L01 第 8 项 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2208`
//!
//! **判据（锚点原文五条）**：双配额、三阈值、降级通知可查、泄漏防线、判据。
//! 逐条落位：
//! - **总量+单发射器双配额**：[`PoolQuota`] 声明**池总配额**（`bytes() =
//!   capacity × stride`，CPU 池字节上限与 GPU 池字节上限**各自独立**，
//!   见 [`PoolKind`]）与**单发射器上限**（[`EmitterBudget::cap`]，默认池容量
//!   [`DEFAULT_EMITTER_SHARE_PCT`]%可配）。两者是**不同维度**的约束：总量管
//!   「池装不装得下」，单发射器管「**一个发射器能不能吞掉整个池**」——
//!   后者不设时，一个发射器就能把池吃满、其余发射器全部饿死，而池水位
//!   看着「正常」（没超总量），故这条配额**不可省**。
//! - **三阈值水位**：沿用 F1776 范式三档（[`WM_WARN_PCT`] 70 预警 /
//!   [`WM_DEGRADE_PCT`] 85 降级 / [`WM_REJECT_PCT`] 95 拒绝），判据用
//!   **夹逼对**钉死界位置（69/70/71、84/85/86、94/95/96 各点分别落在
//!   哪一档），而不是只断「>某值成立」——后者漏掉「差一档」的错位。
//! - **降级通知可查**：水位超 85 → 按发射率**反比**缩减新发射
//!   （[`emission_scale`]），缩减因子**广播**给全部发射器
//!   （[`ParticlePool::broadcast_degrade`]，**每帧至多一次**），降级原因
//!   可经 [`PoolPressureEvent::reason`] 查得。
//! - **泄漏防线**：死亡未回收累计进 [`LeakLedger`]，分配/回收**恒等式**
//!   可断（[`LeakLedger::residue`] 恒为 0 是**不变量**，不为 0 即泄漏）。
//! - **判据**：`vel08_checks.rs` 逐条映射，**双向验证**（基线绿 + 变体红）。
//!
//! **滞回是硬要求，不是修饰（锚点「降级震荡→滞回缓冲」F1637 口径）**：
//! 三阈值若升降同值，水位在阈值上抖动时档位**每帧翻转**，发射率因子随之
//! 阶跃，视觉上是**频闪**。故升档阈值与降档阈值**分离**
//! （[`WM_WARN_PCT`]/[`HYST_WARN_OFF`] = 70/65 等三对），滞回区间内档位
//! **保持不变**。判据在每个滞回区间的**中点**与**两端**各取样，
//! 并与「无滞回」变体对拍（删掉滞回即红）。
//!
//! **降级因子为什么是反比线性而不是阶跃**：[`emission_scale`] 以 85% 为
//! 因子 1.0 的锚、95% 为因子 0 的锚线性反比，即
//! `scale = (100 - pct) / (100 - 85)`。这样 90% 水位得到因子 0.5（半速），
//! 是**连续**降级；若改成「超 85 就砍半」这类阶跃，水位在 85 附近抖动时
//! 发射率会 1.0↔0.5 跳变，**比不改更难看**。
//!
//! **空闲链表与批量回收的顺序语义（本域最易出静默错处）**：
//! [`ParticlePool`] 用**侵入式**空闲链表（槽位自身存next，零额外分配）。
//! 批量回收 [`ParticlePool::reclaim`] 把整条死亡链**一次性头插**到
//! free head，代价O(k)（k = 本批死亡数），故**摊还O(1)**（k 批共O(总死亡数)）。
//! 顺序语义是**头插 ⇒ 链表序 = 批次序 + 原free 序**，判据直接断链表
//! 取出序列为 `[0,1,2, …原free]`，**不是** `[2,1,0, …]`——逐个 push 会
//! 得到逆序，而逆序会让「回收顺序 = 死亡顺序」这条契约静默失效
//! （表现是粒子被复用时的属性错乱，不是崩溃）。
//!
//! **拒绝必须显性且带三要素**：超 [`WM_REJECT_PCT`] 新分配**拒绝**
//! （[`Rejection`] = 当前水位 / 上限 / 建议），**不静默截断**。静默截断
//! 的后果是发射器以为自己在正常发射、画面上粒子就是不出现，且**无任何
//! 线索可查**——这正是本域最恶劣的缺陷形态。
//!
//! **单发射器超限只截自己，不挤占他者**：请求超出本发射器上限时截断到
//! 上限并**告警**（[`PoolDiag::EmitterTruncated`]），但**不**动别的发射器
//! 已占的槽位，也不因此拒绝——否则一个发射器的配置错误会连坐全场。
//!
//! **与F2207 的分工**：F2207 决定「已存在的粒子按什么次序画」，本模块
//! 决定「**槽位从哪来、什么时候还回去、装不下怎么办**」。本模块消费
//! F2205 的回收信号（[`super::vel05_lifetime::LifeAdvance::recycled`]
//! 明示「池回收信号归 F2208 消费」），**不反写** F2205 一个字节。
//!
//! **降级矩阵（锚点原文五条→ 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 超 95 拒绝未显性 → 核验不过（显性是被测不变量，F1833 同规） | [`ParticlePool::try_alloc`] 返[`Rejection`] 且三要素齐备，判据逐要素断非空 |
//! | 降级震荡（压力边界反复）→ 滞回缓冲（F1637 口径） | [`ParticlePool::observe`] 升降阈值分离，判据断滞回区间内档位保持 |
//! | 单发射器超上限 → 该发射器发射截断+ 告警（不挤占他者） | [`ParticlePool::alloc_for`] 截到本发射器上限并告警，别家计数**逐项不变** |
//! | 池泄漏（死亡未回收累计）→ 泄漏账本（F2003 同款） | [`LeakLedger`] 双恒等式：分配-回收-池实测在用 ≡ 0、死亡标记-已回收 ≡ 0|
//! | 降级原因不可查 → 事件缺字段 | [`PoolPressureEvent`] 携水位/因子/原因，判据断三字段齐备且因子与水位**自洽** |
//!
//! **实测教训（写进代码而非只写进提交记录，因为它们会在改代码时重现）**：
//! ① **`observe`的升档循环查错了对象**：初版查「当前档的进入点」，
//!    而 [`PressureLevel::Normal`] 的进入点是 `None` ⇒ `break`，
//!    于是水位再高也**永不升档**（实测 70% 停在 Normal）。修正为查
//!    「**下一档**的进入点」。教训：**状态机的档位枚举里，`Normal`
//!    天然没有「进入点」，任何`enter_threshold(current)` 式的循环
//!    都会在起点自杀**。
//! ② **账本恒等式不能用账本自比**：初版把残差定义为
//!    `|alloc − freed|`，实测「3 分配 2 回收」判成泄漏——但**健康池里
//!    本就有在用粒子**，`alloc != freed` 是常态。正确口径是拿
//!    **池侧实测 `live`** 对账（[`LeakLedger::residue`] 的形参）。
//!    教训：**守恒式必须有一端来自被测系统的外部实测值**，
//!    两端都取自账本自己就退化成「账本自证为空」。
//! ③ **滞回回落是严格小于**：恰在回落点（如 80）**仍保持原档**。
//!    我曾把「85→80 须退出降级」写成判据，实测把**正确**的实现判成红。
//!    教训：**滞回死区含端点**（`[回落点, 进入点)` 半开区间），
//!    写判据与写文档时都要把这一点写明，否则后来者会照着错的改回去。
//! ④ **两级防护会互相掩盖**：`collect_dead` 与 `reclaim` 各有一层
//!    越界过滤，删掉任一层，另一层兜住 ⇒ 变异全绿。教训：**判据要
//!    钉「两层共同的可观测效果」（如 [`LeakLedger`] 的 `dead_marked`
//!    记的是**批长**），而不是钉某���层的内部行为**。
//! ⑤ **真等价变异不是门禁漏网**（三条，均已从变异表移除并写明理由）：
//!    - `water-pct-floor`：`(l*100)/cap` → `l*100/cap`，整数乘除结合律，
//!      语义完全等价；
//!    - `scale-no-clamp-low`：`pct <= 85` → `pct < 85`，在唯一的分歧点
//!      `pct == 85` 上两条路径**数值重合**（`<=` 给 1.0，`<` 算得
//!      `(95-85)/(95-85) = 1.0`）；
//!    - `quota-bytes-wrap` / `validate-swallow-overflow`：`capacity` 与
//!      `stride` 都是 `u32`，`(2^32-1)^2 < 2^64` 故**恒不溢出**，
//!      `checked_mul` 的 `None` 分支不可达（见 [`PoolQuota::bytes`]）。
//!    按纪律「MISSED 先怀疑变异选错」，这三项**改产出侧**（记录不可达性）
//!    而非加抓不到的假判据——**假判据比没判据更坏**，它给人虚假保障。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码与诊断袋（自建，不扩下游封闭枚举）
// ---------------------------------------------------------------------------

/// 本域诊断码。
///
/// **自建而非复用 `DiagCode`**：下游枚举是封闭的、无权加变体，把池域的
/// 「拒绝/截断/泄漏」塞进别人的语义里会让读屏播报出错误的类别名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolDiag {
    /// 水位进入预警档（70）。
    WatermarkWarn,
    /// 水位进入降级档（85），已广播缩减因子。
    WatermarkDegrade,
    /// 水位进入拒绝档（95），新分配被拒。
    WatermarkReject,
    /// 单发射器请求超其上限，已截断。
    EmitterTruncated,
    /// 空闲链表空（池满但未到拒绝档）。
    PoolExhausted,
    /// 泄漏检出：死亡标记与已回收数不匹配。
    LeakDetected,
    /// 配额声明非法（容量/stride/占比为0 或溢出）。
    QuotaRejected,
}

impl PoolDiag {
    /// 中文标签（**读屏可达**，F1764 纪律：诊断码须有中文标签）。
    pub fn zh(self) -> &'static str {
        match self {
            PoolDiag::WatermarkWarn => "池水位预警",
            PoolDiag::WatermarkDegrade => "池水位降级",
            PoolDiag::WatermarkReject => "池水位拒绝",
            PoolDiag::EmitterTruncated => "单发射器截断",
            PoolDiag::PoolExhausted => "池槽位耗尽",
            PoolDiag::LeakDetected => "池泄漏检出",
            PoolDiag::QuotaRejected => "池配额声明拒绝",
        }
    }

    /// 该码是否表示「新分配被拒」（区别于「被截断」「被钳制」）。
    ///
    /// 三者语义不同，故用**三个独立谓词**而不是一个 bool：调用方要能
    /// 区分「拒绝（要重试）」「截断（按截断值继续）」「耗尽（等回收）」。
    pub fn is_reject(self) -> bool {
        matches!(self, PoolDiag::WatermarkReject | PoolDiag::QuotaRejected)
    }
}

/// 诊断条目：码 + 现象 + 建议。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolDiagnostic {
    pub code: PoolDiag,
    pub message: String,
    pub hint: String,
}

/// 诊断袋：池域诊断出口。
#[derive(Clone, Debug, Default)]
pub struct PoolBag {
    items: Vec<PoolDiagnostic>,
}

impl PoolBag {
    pub fn new() -> Self {
        PoolBag { items: Vec::new() }
    }

    pub fn push(&mut self, d: PoolDiagnostic) {
        self.items.push(d);
    }

    pub fn note(&mut self, code: PoolDiag, message: String, hint: String) {
        self.items.push(PoolDiagnostic { code, message, hint });
    }

    pub fn all(&self) -> &[PoolDiagnostic] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 该码是否出现过。
    pub fn has(&self, code: PoolDiag) -> bool {
        self.items.iter().any(|d| d.code == code)
    }

    /// 某码的诊断条数（**独立计数**用：合并成一个总数会被别的码掩护）。
    pub fn count(&self, code: PoolDiag) -> usize {
        self.items.iter().filter(|d| d.code == code).count()
    }

    /// 取该码最后一条的消息（断言「须写明某值」用）。
    pub fn last_msg(&self, code: PoolDiag) -> Option<&str> {
        self.items.iter().rev().find(|d| d.code == code).map(|d| d.message.as_str())
    }
}

// ---------------------------------------------------------------------------
// 二、配额声明（锚点：池总配额 + 单发射器上限）
// ---------------------------------------------------------------------------

/// 池种类：CPU 池与 GPU 池**配额各自独立**（GPU 池配额单位同为字节，
/// 但与 CPU 池不共享——共享即等于只有一个总闸，GPU 超额会连带拒掉 CPU 分配）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolKind {
    Cpu,
    Gpu,
}

impl PoolKind {
    pub fn zh(self) -> &'static str {
        match self {
            PoolKind::Cpu => "CPU 池",
            PoolKind::Gpu => "GPU 池",
        }
    }
}

/// 池总配额声明。
///
/// 「池总内存 = 容量 × stride」是**声明**而非运行时统计：stride（单粒子
/// 字节数）在池创建时固定，故总字节是**编译期常量**，水位计算不必读
/// 字节表（读表会让每次水位查询变成 O(N)，而水位查询在热路径上）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolQuota {
    pub kind: PoolKind,
    /// 槽位容量（粒子个数）。
    pub capacity: u32,
    /// 单粒子字节数（stride）。
    pub stride: u32,
    /// 单发射器上限占池容量的百分比（默认 [`DEFAULT_EMITTER_SHARE_PCT`]）。
    pub emitter_share_pct: u32,
    /// 池总内存**硬顶**（字节）。与 `capacity × stride` 是**两个独立闸**：
    /// 前者是「我申请多少」，后者是「这个池实际占多少」。
    pub bytes_cap: u64,
}

/// 单发射器默认上限占比（池容量的 10%）。
pub const DEFAULT_EMITTER_SHARE_PCT: u32 = 10;

/// 单发射器占比上限（不允许配到 100%——那等于取消单发射器配额）。
pub const MAX_EMITTER_SHARE_PCT: u32 = 100;

impl PoolQuota {
    /// 池总内存字节数 = 容量 × stride（**checked_mul**：溢出即拒绝）。
    ///
    /// **实测记录的不可达性（如实记录，不遮掩）**：`capacity` 与 `stride`
    /// **都是 `u32`**，故 `capacity × stride ≤ (2^32-1)^2 ≈ 2^64 - 2^33 + 1`，
    /// **恒不溢出 `u64`** ⇒本函数的 `None` 分支在**当前类型下不可达**
    /// （实测 `u32::MAX × 64` 得 `Some(274877906880)`，与裸 `*` 逐位相同）。
    /// 换言之 `checked_mul` 在此是**防御性冗余**：防的是日后有人把字段
    /// 改成 `u64`/`usize` 而没同步复核乘法。
    ///
    /// **为什么仍保留**而不是换成裸 `*`：裸 `*` 在类型放宽后会**静默回绕**
    /// 成一个比真实值小得多的总配额——那时「池声明装得下」的判断完全失效，
    /// 且**无任何诊断**。保留 `checked_mul` + `None => Err` 是为了让那次
    /// 类型放宽变成**一次显式拒绝**而不是一个跨版本的静默缺陷。
    /// 代价是当前有一条走不到的分支，如实记此。
    pub fn bytes(&self) -> Option<u64> {
        (self.capacity as u64).checked_mul(self.stride as u64)
    }

    /// 配额校验。失败返回中文原因（读屏可达）。
    ///
    /// 四条非法各自**独立**判：容量 0（池不存在）、stride 0（总字节恒0，
    /// 水位分母为 0）、占比 0 或超 100（单发射器配额形同虚设）、
    /// 总量超硬顶（声明了装不下的池）。
    pub fn validate(&self) -> Result<u64, String> {
        if self.capacity == 0 {
            return Err(format!("池容量为 0（{}）：池不存在，无法建空闲链表", self.kind.zh()));
        }
        if self.stride == 0 {
            return Err(format!("单粒子字节数为 0（{}）：总字节恒 0，水位分母为零", self.kind.zh()));
        }
        if self.emitter_share_pct == 0 || self.emitter_share_pct > MAX_EMITTER_SHARE_PCT {
            return Err(format!(
                "单发射器上限占比 {} 越界（须在 1..={}）：等于取消单发射器配额，\
                 一个发射器即可吞掉整池",
                self.emitter_share_pct, MAX_EMITTER_SHARE_PCT
            ));
        }
        let total = match self.bytes() {
            Some(b) => b,
            None => {
                return Err(format!(
                    "{}总量溢出（容量 {} × stride {}）：配额声明不可信",
                    self.kind.zh(),
                    self.capacity,
                    self.stride
                ))
            }
        };
        if total > self.bytes_cap {
            return Err(format!(
                "{}声明总量 {} 字节超硬顶 {} 字节：池建起来就装不下",
                self.kind.zh(),
                total,
                self.bytes_cap
            ));
        }
        Ok(total)
    }

    /// 单发射器上限（槽位数）。占比 100% 时等于池容量（配者显式取消该约束）。
    pub fn emitter_cap(&self) -> u32 {
        let raw = (self.capacity as u64 * self.emitter_share_pct as u64) / 100;
        // 至少 1 槽：小池（如 capacity=4、share=10%）算得 0，若返回 0 则
        // 「上限 0」会让任何分配都被判超限——一个都发不出去，比不设配额更糟。
        if raw == 0 {
            1
        } else {
            raw as u32
        }
    }
}

// ---------------------------------------------------------------------------
// 三、三阈值水位 + 滞回
// ---------------------------------------------------------------------------

/// 水位档位（**四档**，不是 bool——三阈值自然给出四段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressureLevel {
    Normal,
    Warn,
    Degrade,
    Reject,
}

impl PressureLevel {
    pub fn zh(self) -> &'static str {
        match self {
            PressureLevel::Normal => "正常",
            PressureLevel::Warn => "预警",
            PressureLevel::Degrade => "降级",
            PressureLevel::Reject => "拒绝",
        }
    }
}

/// 预警档起点（百分位）。
pub const WM_WARN_PCT: u64 = 70;
/// 降级档起点（百分位）。
pub const WM_DEGRADE_PCT: u64 = 85;
/// 拒绝档起点（百分位）。
pub const WM_REJECT_PCT: u64 = 95;

/// 滞回落点：预警档回落（**低于** 70 才退出预警，故留 5 个点死区）。
pub const HYST_WARN_OFF: u64 = 65;
/// 滞回落点：降级档回落。
pub const HYST_DEGRADE_OFF: u64 = 80;
/// 滞回落点：拒绝档回落。
pub const HYST_REJECT_OFF: u64 = 90;

/// 水位百分比（整数运算，**无浮点**）。
///
/// 整数而非 `f32`：水位在阈值上做**精确比较**，浮点会让 `0.1+0.2` 这类
/// 误差把 85% 判成 84.99% 而漏掉降级。`capacity >= 1` 已由
/// [`PoolQuota::validate`] 保证，故除法安全。
pub fn water_pct(live: u32, capacity: u32) -> u64 {
    if capacity == 0 {
        return 100;
    }
    // live 超过 capacity 是**逻辑矛盾**（分配不可能超过槽位数），但若
    // 发生则水位钉到 100（钳到顶），绝不返回 >100 的荒谬值。
    let l = if live > capacity { capacity as u64 } else { live as u64 };
    (l * 100) / (capacity as u64)
}

/// 降级发射率因子：水位 pct 处的发射率比例（**反比线性**，85% → 1.0，95% → 0.0）。
///
/// 「反比」而非阶跃的理由见头注：阶跃会让发射率在阈值附近跳变，
/// 比不改更难看（频闪）。
pub fn emission_scale(pct: u64) -> f32 {
    if pct <= WM_DEGRADE_PCT {
        return 1.0;
    }
    if pct >= WM_REJECT_PCT {
        return 0.0;
    }
    let span = (WM_REJECT_PCT - WM_DEGRADE_PCT) as f32;
    ((WM_REJECT_PCT - pct) as f32) / span
}

/// 降档水位（三阈值按档位给出**回落**点，与升档点分离）。
fn exit_threshold(level: PressureLevel) -> u64 {
    match level {
        PressureLevel::Normal => 0,
        PressureLevel::Warn => HYST_WARN_OFF,
        PressureLevel::Degrade => HYST_DEGRADE_OFF,
        PressureLevel::Reject => HYST_REJECT_OFF,
    }
}

/// 升档水位（三阈值按档位给出**进入**点）。
fn enter_threshold(level: PressureLevel) -> Option<u64> {
    match level {
        PressureLevel::Normal => None,
        PressureLevel::Warn => Some(WM_WARN_PCT),
        PressureLevel::Degrade => Some(WM_DEGRADE_PCT),
        PressureLevel::Reject => Some(WM_REJECT_PCT),
    }
}

// ---------------------------------------------------------------------------
// 四、降级通知事件（入 F1408 总线，与 F2204 事件体系同构）
// ---------------------------------------------------------------------------

/// 池压力事件：水位值 + 缩减因子 + 降级原因。
///
/// 三字段**齐备**是契约：只有「降级了」而查不到「降到多少、为什么」的
/// 事件，排查时无法区分「池压高了」与「别的发射器把池吃满了」。
#[derive(Clone, Debug, PartialEq)]
pub struct PoolPressureEvent {
    /// 事件产生时的水位（百分位）。
    pub water_pct: u64,
    /// 事件产生时的发射率缩减因子。
    pub scale: f32,
    /// 降级原因（读屏可达的人话，非枚举名）。
    pub reason: String,
    /// 本帧内该事件的序号（去抖/去重用）。
    pub seq: u32,
}

/// 无事件（占位，供"本帧无降级"时返回，避免 `Option` 到处传）。
pub const NO_PRESSURE_REASON: &str = "本帧无降级：水位未过降级阈值";

/// 构造池压力事件。
pub fn pressure_event(pct: u64, seq: u32) -> PoolPressureEvent {
    let scale = emission_scale(pct);
    let reason = if pct >= WM_REJECT_PCT {
        format!(
            "池水位 {}% 已达拒绝档{}%：新分配被拒，已在制粒子继续跑到自然消亡",
            pct,
            WM_REJECT_PCT
        )
    } else if pct >= WM_DEGRADE_PCT {
        format!(
            "池水位 {}% 已过降级档 {}%：全部发射器发射率按反比因子 {:.3} 缩减",
            pct,
            WM_DEGRADE_PCT,
            scale
        )
    } else if pct >= WM_WARN_PCT {
        format!("池水位 {}% 已过预警档 {}%：暂不降级，仅告警", pct, WM_WARN_PCT)
    } else {
        NO_PRESSURE_REASON.to_string()
    };
    PoolPressureEvent { water_pct: pct, scale, reason, seq }
}

// ---------------------------------------------------------------------------
// 五、空闲链表（侵入式，零额外分配）
// ---------------------------------------------------------------------------

/// 槽位链表终止标记（也是"无槽位"哨兵）。
pub const NO_SLOT: u32 = u32::MAX;

/// 粒子池：侵入式空闲链表 + 在用计数 + 滞回档位。
///
/// `next_free[i]` 存槽位 i 的下一个空闲槽（**侵入式**：链表存在槽位数组
/// 自身里，不另分配`Vec<u32>`），故池的空闲链表**零额外内存**。
#[derive(Clone, Debug)]
pub struct ParticlePool {
    /// 侵入式 next 指针。
    next_free: Vec<u32>,
    /// 空闲链表头（无空闲时 [`NO_SLOT`]）。
    free_head: u32,
    /// 槽位容量。
    capacity: u32,
    /// 在用槽位数。
    live: u32,
    /// 当前档位（带滞回）。
    level: PressureLevel,
    /// 本帧已广播次数（**每帧至多一次**的守门计数）。
    broadcast_count: u32,
}

impl ParticlePool {
    /// 按容量建池：全部槽位入链，头为 0（链表序 = 槽位序升序）。
    pub fn new(quota: &PoolQuota) -> Result<ParticlePool, String> {
        quota.validate()?;
        let n = quota.capacity as usize;
        let mut next_free: Vec<u32> = vec![NO_SLOT; n];
        // 头插建链会得到逆序；这里从尾往头插，使链表序 == 槽位序升序。
        for i in (0..n).rev() {
            next_free[i] = if i + 1 < n { (i + 1) as u32 } else { NO_SLOT };
        }
        Ok(ParticlePool {
            next_free,
            free_head: 0,
            capacity: quota.capacity,
            live: 0,
            level: PressureLevel::Normal,
            broadcast_count: 0,
        })
    }

    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    pub fn live(&self) -> u32 {
        self.live
    }

    pub fn free_count(&self) -> u32 {
        self.capacity - self.live
    }

    pub fn level(&self) -> PressureLevel {
        self.level
    }

    /// 当前水位（百分位）。
    pub fn water_pct(&self) -> u64 {
        water_pct(self.live, self.capacity)
    }

    /// 当前发射率缩减因子。
    pub fn scale(&self) -> f32 {
        emission_scale(self.water_pct())
    }

    /// 采样水位并**带滞回**更新档位，产出本帧档位变化事件。
    ///
    /// **滞回语义**：`observe` 每帧调用一次；升档看进入点、降档看回落点，
    /// 两组阈值**不同**，故水位在死区内**档位保持**。
    pub fn observe(&mut self, bag: &mut PoolBag) -> Option<PressureLevel> {
        let pct = self.water_pct();
        let before = self.level;
        // 升档：查「**下一档**」的进入点，逐档爬（不跳档——跳档会让
        // 「85 一帧内到 96」跳过降级通知的语义边界）。
        //
        // **注意此处查的是 higher档而非当前档**：查当前档会在 Normal 时
        // 取 `enter_threshold(Normal) == None` 直接 break，于是水位再高
        // 也永不升档（实测缺陷：70% 停在 Normal）。
        let mut next = before;
        loop {
            let higher = match next {
                PressureLevel::Normal => PressureLevel::Warn,
                PressureLevel::Warn => PressureLevel::Degrade,
                PressureLevel::Degrade => PressureLevel::Reject,
                PressureLevel::Reject => break,
            };
            let t = match enter_threshold(higher) {
                Some(t) => t,
                None => break,
            };
            if pct < t {
                break;
            }
            next = higher;
        }
        // 降档：水位跌破当前档的回落点才降（死区内保持）。
        while next != PressureLevel::Normal && pct < exit_threshold(next) {
            next = match next {
                PressureLevel::Reject => PressureLevel::Degrade,
                PressureLevel::Degrade => PressureLevel::Warn,
                PressureLevel::Warn => PressureLevel::Normal,
                PressureLevel::Normal => break,
            };
        }
        self.level = next;
        if next == before {
            return None;
        }
        let code = match next {
            PressureLevel::Normal => {
                bag.note(
                    PoolDiag::WatermarkWarn,
                    format!("池水位回落至 {}%：档位解除，当前正常", pct),
                    String::from("回落经滞回死区判定，避免阈值附近档位抖动"),
                );
                PoolDiag::WatermarkWarn
            }
            PressureLevel::Warn => PoolDiag::WatermarkWarn,
            PressureLevel::Degrade => PoolDiag::WatermarkDegrade,
            PressureLevel::Reject => PoolDiag::WatermarkReject,
        };
        let ev = pressure_event(pct, 0);
        bag.note(code, ev.reason.clone(), String::from("详见池压力事件：水位值/缩减因子/原因三字段齐备"));
        Some(next)
    }

    /// 分配一个槽位（**O(1)**）。拒绝档返回 [`Rejection`]。
    ///
    /// **两条拒绝路径的可达性必须如实声明**：水位拒绝
    /// （`pct >= WM_REJECT_PCT`）在正常路径**总是先命中**——因为
    /// 「槽位全满」等价于 `live == capacity`，即 `pct == 100 >= 95`。
    /// 故 [`PoolDiag::PoolExhausted`] 分支（`free_head == NO_SLOT` 且
    /// `live < capacity`）在**账实相符时不可达**，它是**防御分支**：
    /// 只在链表与计数**不一致**（链表被外部逻辑损坏、重复回收致链 shortening）
    /// 时才命中，此时报「账实不符」而不是「水位高」，因为后者会让排查
    /// 走向错误方向（去调配额，而真实原因是链表损坏）。
    ///
    /// 写明「不可达」而非删掉该分支：删掉后链表损坏会退化成
    /// 「返回 NO_SLOT 且不记诊断」——那才是真正的静默。
    pub fn try_alloc(&mut self, bag: &mut PoolBag) -> Result<u32, Rejection> {
        let pct = self.water_pct();
        if self.level == PressureLevel::Reject || pct >= WM_REJECT_PCT {
            let r = Rejection::new(pct, self.capacity, self.live);
            bag.note(PoolDiag::WatermarkReject, r.message.clone(), r.hint.clone());
            return Err(r);
        }
        if self.free_head == NO_SLOT {
            // 防御分支：水位未过拒绝档却无槽位 ⇒ 链表与计数不一致。
            let r = Rejection::new(pct, self.capacity, self.live);
            bag.note(
                PoolDiag::PoolExhausted,
                format!(
                    "池账实不符：水位 {}%（在用 {}/{}）未过拒绝档，但空闲链表已空\
                     ——链表长度与在用计数不一致（链损坏或重复回收致链缩短）",
                    r.water_pct,
                    r.live,
                    r.capacity
                ),
                String::from("查回收路径是否重复归还同一槽位；本项不是配额问题，调配额无用"),
            );
            return Err(r);
        }
        let slot = self.free_head;
        self.free_head = self.next_free[slot as usize];
        self.next_free[slot as usize] = NO_SLOT;
        self.live += 1;
        Ok(slot)
    }

    /// 回收一批死亡槽位（**摊还 O(1)**／粒子）。
    ///
    /// **头插语义**：整条死亡链一次性接到 free head 之前，故链表取出序 =
    /// `batch` 序 + 原free 序（**非逆序**）。这是契约，判据直接断序。
    ///
    /// **先检后改**：一次 walk 建「当前在链上」集合（O(n)），筛出批中
    /// **合法且不在链上**的槽位，再整体头插。半批入链会让链表进入
    /// 「有槽位但指向非法下标」的状态，比拒绝更糟，故不做部分提交。
    pub fn reclaim(&mut self, batch: &[u32]) -> usize {
        if batch.is_empty() {
            return 0;
        }
        let n = self.capacity as usize;
        // 一次 walk 建在链集合。
        let mut on_chain = vec![false; n];
        let mut c = self.free_head;
        while c != NO_SLOT {
            let i = c as usize;
            if i >= n {
                break;
            }
            on_chain[i] = true;
            c = self.next_free[i];
        }
        // 筛出本批**真正可回收**者：下标合法、不在链上（幂等）。
        let mut fresh: Vec<u32> = Vec::new();
        for &s in batch.iter() {
            if s == NO_SLOT || (s as usize) >= n {
                continue;
            }
            if on_chain[s as usize] {
                continue;
            }
            fresh.push(s);
        }
        if fresh.is_empty() {
            return 0;
        }
        // 头插建批链（批内序 == fresh 序）。
        for i in 0..fresh.len() {
            let s = fresh[i] as usize;
            self.next_free[s] = if i + 1 < fresh.len() { fresh[i + 1] } else { NO_SLOT };
        }
        // 批尾接原 free 链：O(k) walk，不触碰原链内容。
        let tail = fresh[fresh.len() - 1] as usize;
        self.next_free[tail] = self.free_head;
        self.free_head = fresh[0];
        self.live = if (self.live as usize) >= fresh.len() {
            self.live - fresh.len() as u32
        } else {
            0
        };
        fresh.len()
    }

    /// 槽位是否在空闲链上（**O(n)** —— 仅诊断/判据路径用）。
    pub fn on_free_list(&self, s: u32) -> bool {
        let n = self.next_free.len();
        if s as usize >= n {
            return false;
        }
        let mut c = self.free_head;
        while c != NO_SLOT {
            if c == s {
                return true;
            }
            let i = c as usize;
            if i >= n {
                return false;
            }
            c = self.next_free[i];
        }
        false
    }

    /// 回收单个槽位（幂等）。
    pub fn reclaim_one(&mut self, slot: u32) -> bool {
        let before = self.free_count();
        let one = [slot];
        if self.reclaim(&one) == 0 {
            return false;
        }
        self.free_count() > before
    }

    /// 从空闲链表**按序取出前 k 个槽位**（诊断/判据用，O(k)）。
    pub fn peek_free(&self, k: usize) -> Vec<u32> {
        let mut out = Vec::new();
        let mut c = self.free_head;
        while c != NO_SLOT && out.len() < k {
            out.push(c);
            let i = c as usize;
            if i >= self.next_free.len() {
                break;
            }
            c = self.next_free[i];
        }
        out
    }

    /// 遍历整条空闲链（判据断**内容守恒**用：链表长度必须等于 free_count）。
    pub fn walk_all(&self) -> Vec<u32> {
        let mut out = Vec::new();
        let mut c = self.free_head;
        while c != NO_SLOT {
            out.push(c);
            let i = c as usize;
            if i >= self.next_free.len() {
                break;
            }
            c = self.next_free[i];
        }
        out
    }

    /// 降级广播：**每帧至多一次**，产出给全部发射器的事件。
    ///
    /// 「每帧一次」是**性能契约**（锚点：降级广播每帧一次）：广播 N 次
    /// 会让 N 个发射器各做一次事件解析，在发射器上千时是纯开销。
    pub fn broadcast_degrade(&mut self) -> Option<PoolPressureEvent> {
        if self.broadcast_count > 0 {
            return None;
        }
        let pct = self.water_pct();
        if pct < WM_WARN_PCT {
            return None;
        }
        self.broadcast_count += 1;
        Some(pressure_event(pct, self.broadcast_count))
    }

    /// 帧边界：重置广播计数（回收批处理与观测都在帧边界做）。
    pub fn end_frame(&mut self) {
        self.broadcast_count = 0;
    }
}

// ---------------------------------------------------------------------------
// 六、拒绝三要素
// ---------------------------------------------------------------------------

/// 拒绝三要素：**当前水位 / 上限 / 建议**。
///
/// 三者齐备才叫「显性」：缺水位则不知道多满，缺上限则不知道差多少，
/// 缺建议则使用者只能猜。缺任何一项的拒绝在画面上都表现为
/// 「粒子不出现」，排查时无线索。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    pub water_pct: u64,
    pub capacity: u32,
    pub live: u32,
    pub message: String,
    pub hint: String,
    pub pool_kind_zh: String,
}

impl Rejection {
    pub fn new(water_pct: u64, capacity: u32, live: u32) -> Rejection {
        let message = format!(
            "新分配被拒：当前水位 {}%（在用 {}/{} 槽位），已达拒绝档 {}%",
            water_pct,
            live,
            capacity,
            WM_REJECT_PCT
        );
        let hint = format!(
            "建议：① 等本帧批量回收释放槽位后重试；② 调低发射率使水位降过拒绝档回落点 {}%；\
             ③ 若水位长期不降，查泄漏账本（死亡未回收累计）与单发射器上限是否失效",
            HYST_REJECT_OFF
        );
        Rejection {
            water_pct,
            capacity,
            live,
            message,
            hint,
            pool_kind_zh: "粒子池".to_string(),
        }
    }

    /// 三要素是否齐备（**判据逐项断非空**，不合并成一个 bool 计数——
    /// 合并计数会被「另一项非空」掩护）。
    pub fn has_all_elements(&self) -> bool {
        !self.message.is_empty()
            && !self.hint.is_empty()
            && self.water_pct <= 100
            && self.capacity > 0
            && self.live <= self.capacity
    }
}

// ---------------------------------------------------------------------------
// 七、单发射器配额（防单发射器吞池）
// ---------------------------------------------------------------------------

/// 单发射器在用计数与其上限。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmitterBudget {
    /// 在用槽位。
    pub live: u32,
    /// 本发射器上限（槽位）。
    pub cap: u32,
}

impl EmitterBudget {
    pub fn new(cap: u32) -> EmitterBudget {
        EmitterBudget { live: 0, cap }
    }

    /// 余量（`saturating_sub`：余量是「还能发多少」，负数无意义）。
    pub fn remaining(&self) -> u32 {
        if self.live > self.cap {
            0
        } else {
            self.cap - self.live
        }
    }

    /// 已用百分比（**整数**，与水位同一口径）。
    pub fn used_pct(&self) -> u64 {
        if self.cap == 0 {
            return 100;
        }
        let l = if self.live > self.cap { self.cap as u64 } else { self.live as u64 };
        (l * 100) / (self.cap as u64)
    }
}

/// 分配结果：实际拿到的槽位与是否被截断。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllocResult {
    pub slot: u32,
    /// 本次是否因单发射器上限被截断。
    pub truncated: bool,
}

/// 为某发射器分配一个槽位：**超其上限则截断 + 告警，但不挤占他者**。
///
/// 「不挤占他者」的实现要点：本函数只查**本发射器**的 `live` 与 `cap`，
/// **不看也不改**池的 `free_head` 之外任何发射器的记账——故别家的
/// `EmitterBudget::live` 逐项不变（判据直接断这一点）。
pub fn alloc_for(pool: &mut ParticlePool, me: &mut EmitterBudget, bag: &mut PoolBag) -> Result<AllocResult, Rejection> {
    // 先看**本发射器**是否已满：已满即截断为 0（连一个槽都不给），
    // 但**不拒绝**——拒绝会连坐（池还有空槽却拒绝，是配额语义越权）。
    if me.live >= me.cap {
        bag.note(
            PoolDiag::EmitterTruncated,
            format!(
                "发射器已达单发射器上限（{}/{}槽），本次发射截断为 0",
                me.live, me.cap
            ),
            String::from("单发射器上限用于防单发射器吞池；提额请改配额声明的占比（池的总量水位此时可能仍正常）"),
        );
        return Ok(AllocResult { slot: NO_SLOT, truncated: true });
    }
    match pool.try_alloc(bag) {
        Ok(slot) => {
            me.live += 1;
            Ok(AllocResult { slot, truncated: false })
        }
        Err(r) => Err(r),
    }
}

/// 按发射率因子缩减**请求的发射数**（发射器侧消费降级广播）。
///
/// 缩减在**请求侧**而非池侧：池不该替发射器决定发多少（那是发射率语义，
/// 归 F2203），池只提供因子。
pub fn scaled_request(requested: u32, scale: f32) -> u32 {
    if !(scale.is_finite()) || scale >= 1.0 {
        return requested;
    }
    if scale <= 0.0 {
        return 0;
    }
    let r = (requested as f32 * scale).floor();
    if r < 0.0 {
        0
    } else if r > requested as f32 {
        requested
    } else {
        r as u32
    }
}

// ---------------------------------------------------------------------------
// 八、泄漏账本（F2003 同款防线）
// ---------------------------------------------------------------------------

/// 泄漏账本：死亡未回收累计 + 分配/回收恒等式。
///
/// **两个恒等式**（不是一件事）：
/// ① `alloc - freed == pool.live` —— 槽位守恒（丢槽/重复回收都会破）。
///    **必须拿池侧实际 live 来对账**，不能账本自比：健康池里「有在用
///    粒子」本就使 `alloc != freed`（3 分配 2 回收 ⇒ 差1 是正常的），
///    账本自比会把正常状态判成泄漏（实测教训，已修）。
/// ② `dead_marked - dead_reclaimed == pending` —— 死亡未回收累计（泄漏本体）。
///    只断①会漏「死亡了但没还回链表」——槽位总数对得上，可用槽位却在少。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeakLedger {
    /// 累计分配次数。
    pub alloc_count: u64,
    /// 累计回收次数。
    pub freed_count: u64,
    /// 死亡标记累计。
    pub dead_marked: u64,
    /// 已回收（死亡且还链）累计。
    pub dead_reclaimed: u64,
}

impl LeakLedger {
    pub fn new() -> LeakLedger {
        LeakLedger { alloc_count: 0, freed_count: 0, dead_marked: 0, dead_reclaimed: 0 }
    }

    pub fn on_alloc(&mut self) {
        self.alloc_count += 1;
    }

    pub fn on_free(&mut self) {
        self.freed_count += 1;
    }

    /// 登记死亡标记（**必须**与实际回收分开记，否则漏的是「标记了没回收」）。
    pub fn mark_dead(&mut self, n: u64) {
        self.dead_marked += n;
    }

    pub fn mark_reclaimed(&mut self, n: u64) {
        self.dead_reclaimed += n;
    }

    /// 恒等式①的残差：`|alloc - freed - pool_live|`（**恒为 0**）。
    ///
    /// 形参 `pool_live` 是**池侧实测在用数**（`ParticlePool::live()`），
    /// 判据侧必须传真值——传 0 或传账本自算值都会让这条恒等式恒成立。
    pub fn residue(&self, pool_live: u32) -> u64 {
        let expect = self.alloc_count as i128 - self.freed_count as i128;
        let actual = pool_live as i128;
        let diff = expect - actual;
        if diff < 0 {
            (-diff) as u64
        } else {
            diff as u64
        }
    }

    /// 恒等式②的残差：死亡未回收累计（**泄漏量**，健康时为 0）。
    pub fn pending_dead(&self) -> u64 {
        self.dead_marked.saturating_sub(self.dead_reclaimed)
    }

    /// 在用槽位（账本口径：分配-回收）。
    pub fn live(&self) -> u64 {
        self.alloc_count.saturating_sub(self.freed_count)
    }

    /// 账本自检：**必须**传池侧实测 live（理由见 [`LeakLedger::residue`]）。
    /// 两个残差都为 0 才健康。
    pub fn audit(&self, pool_live: u32, bag: &mut PoolBag) -> bool {
        let r1 = self.residue(pool_live);
        let r2 = self.pending_dead();
        if r1 == 0 && r2 == 0 {
            return true;
        }
        bag.note(
            PoolDiag::LeakDetected,
            format!(
                "池泄漏检出：账本在用 {} 对池实测 {}（残差 {}），死亡未回收累计 {}",
                self.live(),
                pool_live,
                r1,
                r2
            ),
            String::from("残差非零即为泄漏：查是否标记死亡后未走回收路径，或回收被跳过（回收只在帧边界做）"),
        );
        false
    }
}

// ---------------------------------------------------------------------------
// 九、与 F2205 的对接（回收信号消费）
// ---------------------------------------------------------------------------

/// 帧边界批量回收：从寿命推进结果里挑出 `recycled` 的槽位，一次性还链。
///
/// F2205 的 [`super::vel05_lifetime::LifeAdvance::recycled`] 明示
/// 「池回收信号归 F2208 消费」，本函数就是那个消费者。**批量**是性能
/// 契约：逐个还链虽然也是O(1)/个，但要走 N 次链表头插且要 N 次水位重算。
pub fn collect_dead(recycled: &[bool], pool: &mut ParticlePool, ledger: &mut LeakLedger) -> usize {
    let mut batch: Vec<u32> = Vec::new();
    for (i, &r) in recycled.iter().enumerate() {
        if r && (i as u32) < pool.capacity() {
            batch.push(i as u32);
        }
    }
    let n = batch.len() as u64;
    ledger.mark_dead(n);
    let got = pool.reclaim(&batch);
    ledger.mark_reclaimed(got as u64);
    for _ in 0..got {
        ledger.on_free();
    }
    got
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quota(n: u32, share: u32) -> PoolQuota {
        PoolQuota {
            kind: PoolKind::Cpu,
            capacity: n,
            stride: 64,
            emitter_share_pct: share,
            bytes_cap: 1 << 30,
        }
    }

    #[test]
    fn free_list_is_ascending() {
        let mut p = ParticlePool::new(&quota(8, 10)).unwrap();
        assert_eq!(p.peek_free(3), vec![0, 1, 2]);
    }

    #[test]
    fn reclaim_head_insert_keeps_batch_order() {
        let mut p = ParticlePool::new(&quota(16, 50)).unwrap();
        let mut b = PoolBag::new();
        // 先占掉 0,1,2,3。
        let mut got = Vec::new();
        for _ in 0..4 {
            got.push(p.try_alloc(&mut b).unwrap());
        }
        assert_eq!(got, vec![0, 1, 2, 3]);
        // 批量回收 [1,0,2]（乱序死亡）。
        let n = p.reclaim(&[1, 0, 2]);
        assert_eq!(n, 3);
        // 头插 => 链表序 = 批次序 + 原 free 序。
        assert_eq!(p.peek_free(3), vec![1, 0, 2]);
        // 容量 16，回收 3 后在用 1、空闲 15。
        assert_eq!(p.live(), 1);
        assert_eq!(p.free_count(), 15);
        // 幂等：重复回收同一批不再计数（在链上的跳过）。
        assert_eq!(p.reclaim(&[1, 0, 2]), 0);
        assert_eq!(p.free_count(), 15);
        // 越界槽位不入链。
        assert_eq!(p.reclaim(&[99]), 0);
        assert_eq!(p.free_count(), 15);
    }

    #[test]
    fn reject_has_three_elements() {
        let mut p = ParticlePool::new(&quota(4, 100)).unwrap();
        let mut b = PoolBag::new();
        let mut live = Vec::new();
        for _ in 0..4 {
            let s = p.try_alloc(&mut b);
            live.push(s.is_ok());
        }
        assert!(live.iter().all(|x| *x), "前 4 次分配应全部成功");
        p.observe(&mut b);
        assert_eq!(p.level(), PressureLevel::Reject);
        let e = p.try_alloc(&mut b).unwrap_err();
        assert!(e.has_all_elements());
        assert_eq!(e.water_pct, 100);
        assert!(b.has(PoolDiag::WatermarkReject));
        // 拒绝三要素逐项非空（不合并成一个 bool 计数）。
        assert!(!e.message.is_empty());
        assert!(!e.hint.is_empty());
        assert_eq!(e.capacity, 4);
        assert_eq!(e.live, 4);
    }

    #[test]
    fn hysteresis_holds_in_dead_zone() {
        let mut p = ParticlePool::new(&quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        // 占到 70 -> Warn。
        for _ in 0..70 {
            let _ = p.try_alloc(&mut b);
        }
        p.observe(&mut b);
        assert_eq!(p.level(), PressureLevel::Warn);
        // 回落到 67（70 与 65 之间的死区）：档位保持。
        p.reclaim(&[0, 1, 2]);
        p.observe(&mut b);
        assert_eq!(p.water_pct(), 67);
        assert_eq!(p.level(), PressureLevel::Warn);
        // 回落到 64：退出预警。
        p.reclaim(&[3, 4, 5]);
        p.observe(&mut b);
        assert_eq!(p.level(), PressureLevel::Normal);
    }

    #[test]
    fn emitter_cap_truncates_only_self() {
        let mut p = ParticlePool::new(&quota(100, 10)).unwrap();
        let mut b = PoolBag::new();
        let mut me = EmitterBudget::new(10);
        let other = EmitterBudget::new(10);
        for _ in 0..10 {
            let _ = alloc_for(&mut p, &mut me, &mut b).unwrap();
        }
        assert_eq!(me.live, 10);
        let r = alloc_for(&mut p, &mut me, &mut b).unwrap();
        assert!(r.truncated);
        assert_eq!(r.slot, NO_SLOT);
        assert_eq!(other.live, 0, "别家计数不得被连坐");
        assert!(b.has(PoolDiag::EmitterTruncated));
    }

    #[test]
    fn leak_ledger_two_identities() {
        let mut p = ParticlePool::new(&quota(8, 100)).unwrap();
        let mut b = PoolBag::new();
        let mut l = LeakLedger::new();
        for _ in 0..3 {
            p.try_alloc(&mut b).unwrap();
            l.on_alloc();
        }
        let flags = vec![true, true, false];
        let got = collect_dead(&flags, &mut p, &mut l);
        assert_eq!(got, 2);
        // 恒等式①必须拿池侧实测 live 对账：3 分配 2 回收 => 在用 1。
        assert_eq!(l.residue(p.live()), 0);
        assert_eq!(l.live(), 1);
        // 恒等式②：2 死 2 回收 => 死亡未回收 0。
        assert_eq!(l.pending_dead(), 0);
        assert!(l.audit(p.live(), &mut b));
        // 只标死不回收 => 泄漏。
        let mut l2 = LeakLedger::new();
        l2.mark_dead(5);
        assert_eq!(l2.pending_dead(), 5);
        assert!(!l2.audit(0, &mut b));
        assert!(b.has(PoolDiag::LeakDetected));
        // 恒等式①抓丢槽：账本说在用 2，池实测 1。
        let mut l3 = LeakLedger::new();
        l3.alloc_count = 3;
        l3.freed_count = 1;
        assert_eq!(l3.residue(1), 1);
    }

    #[test]
    fn broadcast_once_per_frame() {
        let mut p = ParticlePool::new(&quota(100, 100)).unwrap();
        let mut b = PoolBag::new();
        for _ in 0..90 {
            let _ = p.try_alloc(&mut b);
        }
        assert!(p.broadcast_degrade().is_some());
        assert!(p.broadcast_degrade().is_none(), "同帧第二次必须被拒");
        p.end_frame();
        assert!(p.broadcast_degrade().is_some());
    }

    #[test]
    fn gpu_and_cpu_quota_independent() {
        let cpu = PoolQuota { kind: PoolKind::Cpu, capacity: 10, stride: 64, emitter_share_pct: 10, bytes_cap: 1 << 20 };
        let gpu = PoolQuota { kind: PoolKind::Gpu, capacity: 10, stride: 64, emitter_share_pct: 10, bytes_cap: 8 << 20 };
        assert!(cpu.validate().is_ok());
        assert!(gpu.validate().is_ok());
        assert_ne!(cpu.kind, gpu.kind);
    }

    #[test]
    fn scaled_request_respects_factor() {
        assert_eq!(scaled_request(100, 1.0), 100);
        assert_eq!(scaled_request(100, 0.5), 50);
        assert_eq!(scaled_request(100, 0.0), 0);
        assert_eq!(scaled_request(100, -1.0), 0);
    }
}