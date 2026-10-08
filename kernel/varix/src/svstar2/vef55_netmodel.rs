//! VE-F5402 · 网络分层模型（AA 域 · 网络与多人 · 批次 AA · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5402`
//!
//! **判据（锚点原文）**：四层职责、跨层禁调、契约冻结、归位、判据。
//!
//! **职责定位（锚点原文）**：网络分层模型——分层四层定义：传输层（可靠与
//! 不可靠通道抽象）/会话层（连接与房间会话）/复制层（状态同步）→玩法层
//! （业务逻辑）——**每层职责单一句子说清**；分层纪律：**跨层直调禁止**
//! （越层调用=架构违例）。数据结构：四层定义册；跨层拦截器。错误路径与
//! 降级矩阵：跨层直调→拦截+归位；层职责漂移→对拍修正；层间契约破坏→
//! 版本拦截。
//!
//! # 一、「依赖」与「调用」为什么是两条纪律
//!
//! F5401 的 [`vef54_aaarch::Layer::allowed_deps`] 管的是**编译期依赖**：
//! 上层可以引用下层任意层的类型定义（玩法层引用传输层的通道抽象完全合法）。
//! 本条管的是**运行期调用**：调用必须**逐层相邻下传**。为什么？因为每层
//! 的职责单句只有隔着相邻层才成立——玩法层若绕过复制层直接驱动传输层，
//! 状态同步（复制层职责）就被跳过了，同步权威裁定形同虚设。所以「跳层
//! 调用」不是风格问题，是**架构违例**：中间层被架空的那一刻，它的职责
//! 就没人履行了。
//!
//! # 二、拦截之后为什么要「归位」而不是单纯拒绝
//!
//! 单纯拒绝会让调用方「知道错了但不知道路在哪」，下一个调用点照样越层。
//! 归位 = 把越层调用**改道为逐层下传链**（玩法→复制→会话→传输），拦截器
//! 返回的 [`Route`] 就是这条链——调用方按链逐跳走，每跳只跨一层，中间层
//! 职责自动恢复履行。向上调用（下层调上层）则**无归位**：方向本身是错的，
//! 给路径等于鼓励反向依赖（F5401 的 `dep_allowed` 红线）。
//!
//! # 三、职责漂移为什么用「对拍修正」而不是「报错了事」
//!
//! 层职责漂移的失败模式是：某层做了不属于自己的职责（复制层做业务结算）。
//! 只报「漂移了」定位不了修法——对拍把**行为归类**（[`DutyAct`]）与**报到
//! 层**对撞：归类指向职责册里该行为的归属层，修正就是把行为移回归属层。
//! 归属层取自职责册单源（[`DUTY_TABLE`]），不靠调用方自觉。
//!
//! # 四、契约版本破坏为什么「拦截」而不是「兼容」
//!
//! 层间接口在 F5401 已冻结（`FreezeLedger` 逐接口不可变记录）。冻结之后
//! 的版本错配若做「兼容旧版」，等于把冻结拆了——调用方拿到旧语义还以为
//! 是新语义（对齐 F5401 失配对拍纪律：**不静默重解释**）。故版本不匹配
//! 一律拦截，修法是调用方升级期望版本或提供方走冻结流程变更。
//!
//! # 五、与相邻条的分工
//!
//! F5401 管总架构与冻结（Layer/dep_allowed/FreezeLedger 单源，本模块直接
//! 消费不复制）；F5403 管传输层选型（下游——本模块只拦调用方向，不管
//! 通道怎么选）；F5420 双签闸是收口核验（消费本模块的拦截账）。本模块
//! 只管「**每层干什么、调用怎么走、漂了怎么修、版本错了怎么办**」。
//!
//! **性能（锚点原文）**：定义 O(层数)（职责册四条静态表）；拦截 O(调用数)
//! （每调用 O(1) 层号比较+记账）；对拍 O(1)（归类枚举相等比较）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::svstar2::vef54_aaarch::Layer;
use crate::svstar2::vep04_stack::P_NAMESPACE_OWNER;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const NETMODEL_VERSION: &str = "AF55-netmodel-v1";

/// 跨层直调拦截（越层调用=架构违例）。
pub const E_LAYER_CALL: &str = "E_LAYER_CALL";

/// 层职责漂移（行为报到错误层）。
pub const E_LAYER_DRIFT: &str = "E_LAYER_DRIFT";

/// 层间契约版本破坏（版本拦截，不兼容旧版）。
pub const E_CONTRACT_VERSION: &str = "E_CONTRACT_VERSION";

/// 职责册非法（空职责/重复登记）。
pub const E_LAYER_DUTY: &str = "E_LAYER_DUTY";

// ---------------------------------------------------------------------------
// 二、四层职责册（每层职责单一句子——锚点「单句说清」红线）
// ---------------------------------------------------------------------------

/// 四层职责册：每层**恰好一句**职责声明（层序即 F5401 `Layer::ALL` 顺序）。
///
/// 单源声明：职责归属的判定全部转查本表；与 [`vef54_aaarch::Layer`]
/// 枚举的对账由判据执行（四层全覆盖+无重复——层集合变动时此处露馅）。
pub const DUTY_TABLE: [(Layer, &str); 4] = [
    (Layer::Transport, "搬运字节：以可靠与不可靠两类通道把数据从 A 送到 B"),
    (Layer::Session, "维系会话：管理连接的建立保活与房间内的成员与顺序"),
    (Layer::Replication, "同步状态：以权威裁定驱动世界状态在各端一致复制"),
    (Layer::Gameplay, "裁决玩法：执行业务规则并产出结算与业务事件"),
];

/// 按层查职责单句（O(1) 表内定位）。
pub fn duty_of(layer: Layer) -> Option<&'static str> {
    DUTY_TABLE.iter().find(|(l, _)| *l == layer).map(|(_, d)| *d)
}

/// 层行为归类闭集（对拍的最小行为单位——每类恰好归属一层）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DutyAct {
    /// 通道搬运（字节级收发）。
    ByteRelay,
    /// 连接与房间管理。
    ConnRoom,
    /// 状态同步与权威裁定。
    StateSync,
    /// 业务规则与结算。
    RuleSettle,
}

impl DutyAct {
    /// 行为归属层（职责册单源的投影：行为⇒层，对拍修正的依据）。
    pub const fn owner(self) -> Layer {
        match self {
            DutyAct::ByteRelay => Layer::Transport,
            DutyAct::ConnRoom => Layer::Session,
            DutyAct::StateSync => Layer::Replication,
            DutyAct::RuleSettle => Layer::Gameplay,
        }
    }

    /// 行为中文名（读屏可达）。
    pub const fn name(self) -> &'static str {
        match self {
            DutyAct::ByteRelay => "通道搬运",
            DutyAct::ConnRoom => "连接与房间管理",
            DutyAct::StateSync => "状态同步",
            DutyAct::RuleSettle => "业务规则与结算",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、层职责漂移对拍（行为报到层 vs 归属层）
// ---------------------------------------------------------------------------

/// 漂移对拍结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriftVerdict {
    /// 行为在归属层：职责无漂移。
    InPlace,
    /// 漂移：行为报到错误层——修正=移回归属层（对拍修正，不只是报错）。
    Drifted,
}

/// 职责漂移对拍：某层报告执行了某行为 → 判定是否漂移。
///
/// O(1)：一次枚举相等比较。漂移结论自带修正目标（[`DutyAct::owner`]），
/// 不给「未知错误」。
pub fn audit_drift(reported: Layer, act: DutyAct) -> (DriftVerdict, Layer) {
    let owner = act.owner();
    if reported == owner {
        (DriftVerdict::InPlace, owner)
    } else {
        (DriftVerdict::Drifted, owner)
    }
}

// ---------------------------------------------------------------------------
// 四、跨层拦截器（运行期调用纪律：相邻下传放行、越层归位、上调拒绝）
// ---------------------------------------------------------------------------

/// 一次层间调用请求（from 层调 to 层）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallHop {
    /// 发起层。
    pub from: Layer,
    /// 目标层。
    pub to: Layer,
}

impl CallHop {
    /// 是否相邻下传（to 恰为 from 的下一层）。
    pub const fn is_adjacent_down(self) -> bool {
        self.to.index() + 1 == self.from.index()
    }

    /// 是否向上调用（to 层号更大=下层调上层，架构违例无归位）。
    pub const fn is_upcall(self) -> bool {
        self.to.index() > self.from.index()
    }
}

/// 归位路径：逐层下传链（每跳只跨一层）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    /// 下传链（含起点与终点；单跳放行时长度恰为 2）。
    pub hops: Vec<Layer>,
}

impl Route {
    /// 链上每跳是否都相邻下传（归位合法性的机检定义）。
    pub fn all_adjacent(&self) -> bool {
        if self.hops.len() < 2 {
            return false;
        }
        self.hops
            .windows(2)
            .all(|w| w[1].index() + 1 == w[0].index())
    }

    /// 链首尾与请求一致。
    pub fn matches(&self, hop: CallHop) -> bool {
        match (self.hops.first(), self.hops.last()) {
            (Some(&f), Some(&t)) => f == hop.from && t == hop.to,
            _ => false,
        }
    }
}

/// 拦截判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallVerdict {
    /// 放行（相邻下传）。
    Pass,
    /// 层内调用（不属层间纪律，直接放行）。
    InLayer,
    /// 越层直调：拦截+归位（返回改道链）。
    Rerouted,
    /// 向上调用：拦截且**无归位**（方向违例，不给路径）。
    Denied,
}

/// 拦截器：判定 + 记账（拦截 O(1) 每调用；账面供审计与 F5420 双签消费）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Interceptor {
    /// 放行的相邻调用数。
    pub passed: u32,
    /// 层内调用数。
    pub in_layer: u32,
    /// 越层拦截并归位数。
    pub rerouted: u32,
    /// 向上调用拒绝数。
    pub denied: u32,
}

impl Interceptor {
    /// 空拦截器。
    pub fn new() -> Interceptor {
        Interceptor::default()
    }

    /// 判定一次层间调用（并记账——锚点「拦截+归位」的执行点）。
    pub fn route(&mut self, hop: CallHop) -> Result<Option<Route>, crate::svstar2::vep03_token::MotionTokenError> {
        if hop.from == hop.to {
            self.in_layer = self.in_layer.saturating_add(1);
            return Ok(Some(Route { hops: Vec::new() })); // 层内：空链
        }
        if hop.is_adjacent_down() {
            self.passed = self.passed.saturating_add(1);
            let mut two = Vec::new();
            two.push(hop.from);
            two.push(hop.to);
            return Ok(Some(Route { hops: two }));
        }
        if hop.is_upcall() {
            // 向上调用：方向违例，无归位——给路径等于鼓励反向依赖。
            self.denied = self.denied.saturating_add(1);
            return Err(crate::svstar2::vep03_token::MotionTokenError::new(
                E_LAYER_CALL,
                "层间调用被拒：向上调用",
                &format!(
                    "{}→{} 是反向依赖：层间调用只允许自上而下逐层下传",
                    hop.from.name(),
                    hop.to.name()
                ),
                &format!("把该调用改到 {} 的上层发起，或经事件回执向上传递结果", hop.to.name()),
                P_NAMESPACE_OWNER,
            ));
        }
        // 越层直调：拦截 + 归位（逐层下传链）。
        self.rerouted = self.rerouted.saturating_add(1);
        let mut hops = Vec::new();
        let mut cur = hop.from.index();
        let target = hop.to.index();
        loop {
            match Layer::from_index(cur) {
                Some(l) => hops.push(l),
                None => break,
            }
            if cur == target {
                break;
            }
            cur -= 1;
        }
        let r = Route { hops };
        if !r.all_adjacent() || !r.matches(hop) {
            // 归位链自检失败属于内部不变量破坏，显性报错（不静默给坏链）。
            return Err(crate::svstar2::vep03_token::MotionTokenError::new(
                E_LAYER_CALL,
                "归位链构造异常",
                "逐层下传链未通过自检（相邻性或首尾不一致）",
                "检查 Layer 枚举序号连续性（F5401 层号必须 0..=3 无空洞）",
                P_NAMESPACE_OWNER,
            ));
        }
        Ok(Some(r))
    }

    /// 拦截动作总数（越层+向上——审计口径）。
    pub fn intercepted_total(&self) -> u32 {
        self.rerouted.saturating_add(self.denied)
    }
}

// ---------------------------------------------------------------------------
// 五、层间契约版本拦截（F5401 冻结之上的版本校验——不兼容旧版）
// ---------------------------------------------------------------------------

/// 一次契约版本对账请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContractVer {
    /// 提供方层（接口实现方）。
    pub provider: Layer,
    /// 提供方当前接口版本。
    pub current: u32,
    /// 调用方期望版本。
    pub expect: u32,
}

impl ContractVer {
    /// 版本对账：一致放行；不一致**拦截**（不兼容旧版——静默兼容=拆冻结）。
    pub fn check(&self) -> Result<(), crate::svstar2::vep03_token::MotionTokenError> {
        if self.current == self.expect {
            return Ok(());
        }
        Err(crate::svstar2::vep03_token::MotionTokenError::new(
            E_CONTRACT_VERSION,
            "层间契约版本拦截",
            &format!(
                "{} 当前接口版本 {}，调用方期望 {}：版本错配不做静默兼容",
                self.provider.name(),
                self.current,
                self.expect
            ),
            "调用方升级期望版本，或提供方按冻结流程变更接口版本",
            P_NAMESPACE_OWNER,
        ))
    }
}

// ---------------------------------------------------------------------------
// 六、读屏替述（分层文档读屏可达——域本色）
// ---------------------------------------------------------------------------

/// 职责册读屏单行（层名+职责单句；无坐标无内容）。
pub fn screen_line_duty(layer: Layer) -> String {
    match duty_of(layer) {
        Some(d) => format!("{}：{}", layer.name(), d),
        None => format!("{}：职责未登记", layer.name()),
    }
}

/// 归位读屏单行（改道说明；不含调用内容）。
pub fn screen_line_reroute(hop: CallHop, r: &Route) -> String {
    let chain: Vec<&str> = r.hops.iter().map(|l| l.name()).collect();
    format!(
        "跨层调用 {}→{} 已归位为逐层下传：{}",
        hop.from.name(),
        hop.to.name(),
        chain.join("→")
    )
}
