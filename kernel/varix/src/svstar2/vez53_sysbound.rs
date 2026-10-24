//! VE-F5004 · 与既有系统关系（VE-Y 域 · 脚本语言组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5004`
//!
//! **判据（锚点原文）**：四定边界、编排不侵入、互补不混岗、预留、判据。
//!
//! **职责定位（锚点原文全文）**：与既有系统关系——与既有系统关系（关系四定：与引擎系统（脚本调用引擎 API——单向编排不反向侵入）/与插件 SDK（脚本能力与插件互补——轻逻辑用脚本重扩展用插件边界清晰）/与配置系统（配置管数据脚本管行为——不混岗）/与 W 域市场（脚本包未来可上架——预留不承诺））。数据结构：关系四定卡；边界清单。错误路径与降级矩阵：边界混淆（脚本干插件的活）→引导归位；关系失配→对拍修正；预留私启→拦截。性能逐项分解：边界 O(1)；对拍 O(1)；拦截 O(1)。跨批对接点：W01 插件口径上游；配置系统衔接；F5020 双签闸。无障碍与隐私：边界文档读屏可达（域本色）；无隐私面。工程量：核心逻辑约 150 行（五类要点判定与主流程编排）、边界防护约 40 行（输入校验与阈值防护）、错误路径约 35 行（降级与回滚处置）、测试支撑约 75 行（回归用例与断言），合计约 300 行。判据：四定边界、编排不侵入、互补不混岗、预留、判据。
//!
//! # 一、关系四定：脚本与既有系统的关系闭集（边界 O(1) 判读）
//!
//! [`RelationKind`] 是**关系四定闭集**：引擎系统 / 插件 SDK / 配置系统 /
//! W 域市场。每一关系配一张**四定卡**（[`QUAD_CARDS`] 常量卡表，三段：
//! 关系定文 / 判读口径 / 违例样例）——定文说「这条关系是什么」，判读口径
//! 说「怎么机械判」，违例样例说「越界长什么样」。三段缺一不可：只有定文
//! 没有口径的四定没法执行，只有口径没有违例样例的口径没法校准。
//!
//! [`QuadGate`] 收**关系申报卡**（能力名 / 申报关系 / [`RelationFacts`]
//! 机械事实面），对四定**逐关系机械判读**——判读只看事实面三个布尔位，
//! 与申报文本长短无关，**边界 O(1)**（判据：同事实不同文本长度的判读
//! 计数一致）。三条路：
//!
//! - 事实面与申报关系**安于本位** → [`QuadOutcome::AtHome`]；
//! - **边界混淆**（脚本干插件的活 / 轻逻辑塞进插件 / 行为混进配置）→
//!   [`QuadOutcome::GuidedHome`] **引导归位**（指出该归哪条关系）；
//! - **预留私启**（W 域市场面提前开启上架通路）→
//!   [`QuadOutcome::Intercepted`] **拦截**——预留不承诺是结构性纪律，
//!   私启零容忍。
//!
//! **关系失配 → 对拍修正**：同一能力重复申报且判读结论变位 → 修正式
//! 申报（[`QuadReceipt::mismatch`] 置位 + 计数留痕）——四定是活边界不是
//! 一次性登记。
//!
//! # 二、既有系统边界闭集：四域边界卡（可调用面 / 禁入面 / 数据交换协议）
//!
//! [`SysDomain`] 是**脚本面对的既有系统域闭集**：宿主内核域 / 图形域 /
//! 输入域 / 文件域。每域一张**边界卡**（[`BOUNDARY_CARDS`] 常量卡表，
//! 三段：可调用面锚名单 / 禁入面锚名单 / 数据交换协议）——脚本可调什么、
//! 绝不碰什么、跨界面怎么传数据，三段都**写死在 const 卡上**：
//! 开机即冻结，运行期只读不改。
//!
//! 开机白名单种子 [`CANONICAL_APIS`]：每域预置的规范 API（引擎域的实体
//! 查询 / 实体创建，图形域的绘制提交 / 贴图快照，输入域的按键状态读，
//! 文件域的受控落盘）——**每条规范 API 的名字都经编译期断言落在对应域
//! 边界卡的可调用面名单内**（白名单与边界清单结构性对齐，不是两套皮）。
//!
//! # 三、互操作面：白名单注册制 + 签名指纹 + 越白名单调用拦截
//!
//! [`InteropSurface`] 是脚本可调宿主 API 的**唯一合法通道**：
//! [`InteropSurface::register`] 注册（名字 / 所属域 / 读写种类 /
//! [`SigSpec`] 签名），每条注册算 **FNV-1a 64 位签名指纹**
//! （[`InteropSurface::fingerprint_of`]——名字 + 域 + 种类 + 参数型闭集 +
//! 返回型，一体入指纹），指纹可重算可验链
//! （[`InteropSurface::verify_all`]——指纹被改即验出）。三种拦截：
//!
//! - **越白名单调用**（查无此名）→ [`SysBoundError::ApiNotFound`]
//!   （注入面之一：越界调用注入的正式检出码）；
//! - **签名失配**（实参个数与注册签名不符）→
//!   [`SysBoundError::SigMismatch`]；
//! - **重复注册** → [`SysBoundError::ApiDup`]；容量到顶 →
//!   [`SysBoundError::ApiFull`]。
//!
//! 白名单查表面容量写死（[`API_CAP`] = 48）——查表面有界，**查名代价
//! 恒定上界，边界 O(1) 的结构性保证**。
//!
//! # 四、数据交换协议：跨边界值类型闭集 + 句柄生命周期 + 类型不匹配拦截
//!
//! 跨边界只许传四类值（[`SysValue`] **类型闭集**）：数值 / 字符串 /
//! 句柄 / 只读视图——不传裸指针、不传闭包、不传宿主内部结构体
//! （协议口径与边界卡第三段同源）。参数型闭集 [`ArgType`] 与值闭集
//! 一一对位，**类型不匹配 → [`SysBoundError::TypeMismatch`] 拦截**
//! （协议即闸门：错了拒收，不静默转换）。
//!
//! [`HandleTable`] 管**句柄生命周期语义闭集**：借出（[`HandleState::Lent`]）
//! / 转移（[`HandleState::Transferred`]）/ 到期回收
//! （[`HandleState::Recycled`]）。语义纪律：
//!
//! - 借出态可作实参、可转移（一次性：转移后不得再转）；
//! - **只读视图不可转移**（借出语义——视图天生不拥有）、不得冒充实体的
//!   句柄使用，实体句柄也不得冒充视图；
//! - 到期回收后句柄**永久封闭**（id 不复用）——再用即
//!   [`SysBoundError::HandleBad`]（注入面之二：句柄滥用注入检出）；
//! - 结构性不可能状态（已发放区间内查到 Free 槽）→
//!   [`SysBoundError::Inject`]（注入兜底正式码）。
//!
//! # 五、边界审计：跨界调用入账 + 高频调用熔断
//!
//! [`BoundaryAudit`] 每次**跨界调用入账**（无论成败：流水号 / 调用方 /
//! 被调面槽位 / 参数摘要指纹 / 成败位）——**定长环形账**
//! （[`AUDIT_RING_CAP`] = 64，满则覆盖最旧，流水号单调不回退），审计面
//! 永远有最近一段完整轨迹。同账挂**每脚本调用计量器**：单脚本调用计数
//! 达阈值（[`CALL_BREAKER_THRESHOLD`] = 96——口径注：单帧脚本预算以内
//! 的保守上限）即**熔断**（[`SysBoundError::BreakerTripped`]），熔断后
//! 连续 [`BREAKER_COOLDOWN`] = 8 次拒唤自动冷却复路——高频洪水杀得掉、
//! 且冷却路径确定可对拍。
//!
//! # 六、安全默认律在边界上落地：默认无副作用泄漏——授权位 + 授权账
//!
//! 与 F5003 五律的衔接点：**安全默认律**（沙箱内运行无副作用泄漏）在
//! 边界上的落地件是 [`AuthLedger`]——API 分两类（[`ApiKind::ReadOnly`]
//! / [`ApiKind::Effect`]），**一切写操作（Effect 面）默认拒绝**，只有
//! 宿主显式签发的**授权位**（[`AuthLedger::grant`]：脚本 × API 槽位 ×
//! 次数预算）在预算内才放行；预算耗尽 → [`SysBoundError::AuthSpent`]；
//! 无授权 → [`SysBoundError::AuthRequired`]（注入面之三：未授权写注入
//! 的正式检出码）。授权签发入**授权账**（append-only，签发不可静默撤销，
//! 回收只走预算耗尽的自然到期）。
//!
//! # 七、事件账 + health + FNV 指纹 + 双跑确定性对拍 + 注入三面
//!
//! [`EventLedger`] 有界事件账（满 → [`SysBoundError::LedgerFull`] 拒收
//! 待清——**不静默丢事件**）；[`SysBoundWorld::health`] 一键体检
//! （[`HealthReport`]：白名单 / 调用计数自洽 / 三注入计数 / 授权与熔断
//! 状态）；[`SysBoundWorld::state_digest`] 全状态 FNV-1a 64 确定性摘要
//! ——**双跑对拍**：同操作序列两次开机，摘要必相等（确定优先律在边界
//! 的自我执行）；注入三面各检出：越界调用注入（第三节 ApiNotFound）、
//! 句柄滥用注入（第四节 HandleBad）、未授权写注入（第六节
//! AuthRequired），外加结构性注入兜底（第四节 Inject）。
//!
//! # 八、纪律自证
//!
//! - no_std + alloc；内核路径零 unwrap/expect/panic!/运行期 assert!；
//!   可失败全走 `Result<T, SysBoundError>`；错误码段独占
//!   **0x1D50..=0x1D5F**（开工前已 grep 全仓 `0x1D5` 前缀确认整段空闲）；
//! - 错误枚举每变体带 `detail(&self) -> &'static str` 中文可操作诊断；
//! - 溢出显性：计数 saturating_add、FNV wrapping_mul；
//! - const 纪律：const fn 内只用 while+索引与 as u8 比较，不用
//!   slice::get / Ord::min/max / derive 判等；含 Vec 字段的结构一律不
//!   derive Copy；>64KB 结构一律不上栈（世界体走堆构造）；
//! - 编译期 [`const`] 断言：诊断码 0x1D50..=0x1D5F 连续互斥、四定卡
//!   闭集覆盖、四域边界卡三段齐备、规范 API 名与域卡可调用面对齐。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、版本与诊断码段（0x1D50..=0x1D5F 独占 16 码）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const SYS_BOUND_VERSION: &str = "F5004-sysbound-v1";

/// 诊断码段基址（0x1D50——开工前已 grep 全仓 `0x1D5` 前缀，确认整段空闲）。
pub const ERR_CODE_BASE: u16 = 0x1D50;

/// 诊断码段末址（含）。
pub const ERR_CODE_END: u16 = 0x1D5F;

/// 诊断码总数（0x1D50..=0x1D5F 恰 16 码，全部派满 SysBoundError）。
pub const ERR_CODE_COUNT: u16 = 16;

/// 关系四定违定（0x1D50）——边界混淆引导归位后仍越位 / 预留私启拦截的正式码。
pub const E_QUAD_VIOL: u16 = 0x1D50;
/// 关系申报卡不完整（0x1D51）——能力名为空或超长，结构不完整谈不上判读。
pub const E_QUAD_BAD: u16 = 0x1D51;
/// 边界域序数不合法（0x1D52）——域序数须落 1..=4 闭集内。
pub const E_DOMAIN_BAD: u16 = 0x1D52;
/// 白名单重复注册（0x1D53）——同名 API 已在白名单内。
pub const E_API_DUP: u16 = 0x1D53;
/// 白名单容量满（0x1D54）——API_CAP=48 到顶，查表面有界不可破。
pub const E_API_FULL: u16 = 0x1D54;
/// 越白名单调用（0x1D55）——查无此名，越界调用注入的正式检出码。
pub const E_API_NOT_FOUND: u16 = 0x1D55;
/// 签名失配（0x1D56）——实参个数与注册签名不符。
pub const E_SIG_MISMATCH: u16 = 0x1D56;
/// 跨界值类型不匹配（0x1D57）——实参类型与签名参数型不对位，拒收不转换。
pub const E_TYPE_MISMATCH: u16 = 0x1D57;
/// 句柄滥用（0x1D58）——句柄不存在/已到期回收/已转移再转/视图越权冒充。
pub const E_HANDLE_BAD: u16 = 0x1D58;
/// 未授权写操作（0x1D59）——Effect 面调用无授权位，未授权写注入正式检出码。
pub const E_AUTH_REQUIRED: u16 = 0x1D59;
/// 授权额度耗尽（0x1D5A）——授权位在但预算为零，须宿主重新签发。
pub const E_AUTH_SPENT: u16 = 0x1D5A;
/// 高频调用熔断中（0x1D5B）——单脚本调用计数达阈值，冷却后复路。
pub const E_BREAKER_TRIPPED: u16 = 0x1D5B;
/// 事件账满（0x1D5C）——账容量到顶拒收，drain 后重试，不静默丢事件。
pub const E_LEDGER_FULL: u16 = 0x1D5C;
/// 参数不合法（0x1D5D）——空名/超长串/调用方 id 越界，跨界值必须小而有界。
pub const E_PARAM_BAD: u16 = 0x1D5D;
/// 结构性注入兜底（0x1D5E）——内部不变量意外缺口（不可能状态被触达）。
pub const E_INJECT: u16 = 0x1D5E;
/// 状态错序（0x1D5F）——槽位/账目/时序错乱或容量缺口，先查次序与容量。
pub const E_STATE: u16 = 0x1D5F;

/// 本域诊断码全集（判据对账：连续、互斥、恰 16 枚）。
pub const CODES: [u16; 16] = [
    E_QUAD_VIOL, E_QUAD_BAD, E_DOMAIN_BAD, E_API_DUP,
    E_API_FULL, E_API_NOT_FOUND, E_SIG_MISMATCH, E_TYPE_MISMATCH,
    E_HANDLE_BAD, E_AUTH_REQUIRED, E_AUTH_SPENT, E_BREAKER_TRIPPED,
    E_LEDGER_FULL, E_PARAM_BAD, E_INJECT, E_STATE,
];

// ---------------------------------------------------------------------------
// 二、FNV-1a 64 指纹（自包含指纹件——签名/摘要/审计共用一源）
// ---------------------------------------------------------------------------

/// FNV-1a 64 标准偏移基值（FNV 参考实现常量——来源写死不复算）。
pub const FNV_SEED: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 素数（同上——来源写死）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64 逐字节喂入（wrapping 乘法显性——不隐式溢出）。
pub fn fnv1a64_feed(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 喂入一个 u32（定宽小端——指纹不受平台字节序影响）。
fn fp_u32(h: u64, v: u32) -> u64 {
    let mut hh = h;
    let mut i = 0;
    while i < 4 {
        hh ^= ((v >> (i * 8)) & 0xFF) as u64;
        hh = hh.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hh
}

/// 喂入一个 u64（定宽小端）。
fn fp_u64(h: u64, v: u64) -> u64 {
    let mut hh = h;
    let mut i = 0;
    while i < 8 {
        hh ^= ((v >> (i * 8)) & 0xFF) as u64;
        hh = hh.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hh
}

/// 喂入一个字符串（长度前缀 + 字节——前缀防拼接歧义："ab"+"c" ≠ "a"+"bc"）。
fn fp_str(h: u64, s: &str) -> u64 {
    let hh = fp_u64(h, s.len() as u64);
    fnv1a64_feed(hh, s.as_bytes())
}

// ---------------------------------------------------------------------------
// 三、关系四定闭集与四定卡（与既有系统关系的第一层）
// ---------------------------------------------------------------------------

/// 关系四定数（脚本与既有系统的关系闭集恰四条——不多不少）。
pub const QUAD_COUNT: usize = 4;

/// 关系四定（锚点闭集：引擎系统 / 插件 SDK / 配置系统 / W 域市场）。
///
/// 判别序数 1..=4 稳定冻结，申报记录靠序数不靠字符串。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelationKind {
    /// 与引擎系统：脚本调用引擎 API——单向编排不反向侵入。
    EngineSystem = 1,
    /// 与插件 SDK：脚本能力与插件互补——轻逻辑用脚本重扩展用插件。
    PluginSdk = 2,
    /// 与配置系统：配置管数据脚本管行为——不混岗。
    ConfigSystem = 3,
    /// 与 W 域市场：脚本包未来可上架——预留不承诺。
    WMarket = 4,
}

impl RelationKind {
    /// 四定闭集（判据侧遍历用——顺序即序数序）。
    pub const ALL: [RelationKind; QUAD_COUNT] = [
        RelationKind::EngineSystem,
        RelationKind::PluginSdk,
        RelationKind::ConfigSystem,
        RelationKind::WMarket,
    ];

    /// 关系判别序数（1..=4，稳定冻结——申报记录引用它）。
    pub const fn ord(self) -> u8 {
        self as u8
    }

    /// 序数反查关系（越界 None——调用方落 [`SysBoundError::DomainBad`]）。
    pub const fn from_ord(o: u8) -> Option<RelationKind> {
        match o {
            1 => Some(RelationKind::EngineSystem),
            2 => Some(RelationKind::PluginSdk),
            3 => Some(RelationKind::ConfigSystem),
            4 => Some(RelationKind::WMarket),
            _ => None,
        }
    }

    /// 关系名（人读——读屏可达的短名）。
    pub const fn name(self) -> &'static str {
        match self {
            RelationKind::EngineSystem => "引擎系统",
            RelationKind::PluginSdk => "插件SDK",
            RelationKind::ConfigSystem => "配置系统",
            RelationKind::WMarket => "W域市场",
        }
    }

    /// 引导归位口径（判读出混淆时给申报人的一句话改法——错误友好律落地）。
    pub const fn guide_hint(self) -> &'static str {
        match self {
            RelationKind::EngineSystem => "轻逻辑与编排归脚本——以引擎 API 调用实现，勿动插件面",
            RelationKind::PluginSdk => "重扩展归插件 SDK——脚本只留轻逻辑薄壳",
            RelationKind::ConfigSystem => "数据归配置行为归脚本——数据表移出行为面",
            RelationKind::WMarket => "市场面只许登记预留位——上架通路未来再启",
        }
    }
}

/// 关系四定卡（三段卡：定文 / 判读口径 / 违例样例——三段缺一不可）。
#[derive(Clone, Copy)]
pub struct QuadCard {
    /// 本卡约束的关系。
    pub relation: RelationKind,
    /// 关系定文（这条关系是什么——锚点原文口径）。
    pub charter: &'static str,
    /// 判读口径（怎么机械判——判读只看事实面布尔位）。
    pub reading: &'static str,
    /// 违例样例（越界长什么样——口径校准面）。
    pub violation: &'static str,
}

/// 关系四定卡册（四卡冻结——const 卡，开机即冻结运行期只读）。
pub const QUAD_CARDS: [QuadCard; QUAD_COUNT] = [
    QuadCard {
        relation: RelationKind::EngineSystem,
        charter: "与引擎系统：脚本调用引擎 API——单向编排不反向侵入。",
        reading: "申报引擎关系的能力必须是编排面（引擎 API 调用组合），不得是重扩展；反向侵入（引擎回调里注册脚本 API）一律拦截。",
        violation: "引擎回调里反过来注册脚本 API——反向侵入。",
    },
    QuadCard {
        relation: RelationKind::PluginSdk,
        charter: "与插件 SDK：脚本能力与插件互补——轻逻辑用脚本重扩展用插件边界清晰。",
        reading: "申报插件关系的能力必须是重扩展面（重逻辑/重资源）；轻逻辑报成插件关系即边界混淆，引导归位到脚本编排。",
        violation: "三行能干完的轻逻辑包成插件——脚本干插件的活反向版，边界混淆。",
    },
    QuadCard {
        relation: RelationKind::ConfigSystem,
        charter: "与配置系统：配置管数据脚本管行为——不混岗。",
        reading: "申报配置关系的能力必须携带数据表面且不含重扩展逻辑；行为报成配置即混岗，引导归位；重扩展塞进配置域引导归插件。",
        violation: "脚本里写死整张配置数据表——数据混进行为面，混岗。",
    },
    QuadCard {
        relation: RelationKind::WMarket,
        charter: "与 W 域市场：脚本包未来可上架——预留不承诺。",
        reading: "申报市场关系只许登记预留位；事实面出现开启上架通路的即预留私启，拦截零容忍。",
        violation: "借预留名义私启上架通道——预留私启，拦截。",
    },
];

/// 关系判读事实面（机械事实——判读只看这三个布尔位，与文本无关）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelationFacts {
    /// 该能力是重扩展面（重逻辑/重资源/重依赖——插件的活）。
    pub is_heavy_extension: bool,
    /// 该能力携带配置数据表面（配置的活）。
    pub carries_config_data: bool,
    /// 该能力开启了市场上架通路（预留私启的信号位）。
    pub opens_market_channel: bool,
}

impl RelationFacts {
    /// 全安分事实面（无重扩展/无数据表/无市场通路）。
    pub const PLAIN: RelationFacts = RelationFacts {
        is_heavy_extension: false,
        carries_config_data: false,
        opens_market_channel: false,
    };

    /// 事实面指纹（对拍修正用——事实变了结论才许变）。
    pub fn fingerprint(self) -> u64 {
        let mut h = fp_u32(FNV_SEED, 0xF00D_0001);
        h = fp_u32(h, self.is_heavy_extension as u32);
        h = fp_u32(h, self.carries_config_data as u32);
        fp_u32(h, self.opens_market_channel as u32)
    }
}

/// 关系判读结论（安于本位 / 引导归位 / 拦截——三路闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuadOutcome {
    /// 安于本位：事实面与申报关系相合。
    AtHome,
    /// 边界混淆 → 引导归位（附该归哪条关系）。
    GuidedHome(RelationKind),
    /// 预留私启 → 拦截（零容忍——不改道不留情面）。
    Intercepted,
}

impl QuadOutcome {
    /// 结论码（对拍存储用——0 安位 / 1 引导 / 2 拦截）。
    pub const fn code(self) -> u8 {
        match self {
            QuadOutcome::AtHome => 0,
            QuadOutcome::GuidedHome(_) => 1,
            QuadOutcome::Intercepted => 2,
        }
    }

    /// 引导目标（仅 GuidedHome 有意义；余者 None）。
    pub const fn guided_to(self) -> Option<RelationKind> {
        match self {
            QuadOutcome::AtHome => None,
            QuadOutcome::GuidedHome(r) => Some(r),
            QuadOutcome::Intercepted => None,
        }
    }
}

/// 关系四定的机械判读（纯函数——事实面三布尔位 → 三路结论，O(1)）。
///
/// 判读矩阵：
/// - 引擎关系：不得是重扩展（重扩展报引擎 → 引导归插件）；
/// - 插件关系：必须是重扩展（轻逻辑报插件 → 引导归引擎编排）；
/// - 配置关系：必须携带数据表面且非重扩展（行为报配置 → 引导归引擎；
///   重扩展报配置 → 引导归插件）；
/// - 市场关系：开了上架通路即拦截（预留私启）；纯预留登记合法安位。
pub const fn judge_quad(declared: RelationKind, facts: RelationFacts) -> QuadOutcome {
    match declared {
        RelationKind::EngineSystem => {
            if facts.is_heavy_extension {
                QuadOutcome::GuidedHome(RelationKind::PluginSdk)
            } else {
                QuadOutcome::AtHome
            }
        }
        RelationKind::PluginSdk => {
            if facts.is_heavy_extension {
                QuadOutcome::AtHome
            } else {
                QuadOutcome::GuidedHome(RelationKind::EngineSystem)
            }
        }
        RelationKind::ConfigSystem => {
            if facts.is_heavy_extension {
                QuadOutcome::GuidedHome(RelationKind::PluginSdk)
            } else if facts.carries_config_data {
                QuadOutcome::AtHome
            } else {
                QuadOutcome::GuidedHome(RelationKind::EngineSystem)
            }
        }
        RelationKind::WMarket => {
            if facts.opens_market_channel {
                QuadOutcome::Intercepted
            } else {
                QuadOutcome::AtHome
            }
        }
    }
}

/// 关系申报回执（判读结论 + 对拍修正位 + 能力指纹）。
#[derive(Clone, Debug)]
pub struct QuadReceipt {
    /// 能力名指纹（对拍键——名字不入存储只入指纹）。
    pub capability_fp: u64,
    /// 本次判读结论。
    pub outcome: QuadOutcome,
    /// 对拍修正位：与同能力既往判读结论失配时置位（关系失配 → 对拍修正）。
    pub mismatch: bool,
}

/// 关系申报存储项（append-only——四定是活边界，历史申报留痕）。
#[derive(Clone, Debug)]
pub struct QuadRecord {
    /// 能力名指纹。
    pub capability_fp: u64,
    /// 申报关系（序数）。
    pub declared_ord: u8,
    /// 判读结论码（0 安位 / 1 引导 / 2 拦截）。
    pub outcome_code: u8,
    /// 引导目标序数（未引导为 0）。
    pub guide_to_ord: u8,
    /// 申报时事实面指纹。
    pub facts_fp: u64,
}

/// 关系四定闸（判读计数 + 申报存储 + 对拍修正）。
///
/// 容量有界（[`QUAD_RECORD_CAP`]）——申报账到顶走 [`SysBoundError::State`]
/// 拒收（容量缺口显性报，不静默挤兑）。
pub struct QuadGate {
    /// 申报存储（append-only，有界）。
    pub records: Vec<QuadRecord>,
    /// 累计申报数。
    pub declared_total: u32,
    /// 安于本位数。
    pub at_home: u32,
    /// 引导归位数。
    pub guided: u32,
    /// 预留私启拦截数。
    pub intercepted: u32,
    /// 对拍修正数（同能力结论失配）。
    pub mismatched: u32,
}

/// 申报存储容量（128 条——四定申报有界，防申报洪水撑账）。
pub const QUAD_RECORD_CAP: usize = 128;

/// 能力名上限（64 字节——申报卡是卡不是文档，名字超长即拒）。
pub const QUAD_NAME_MAX: usize = 64;

impl QuadGate {
    /// 开机空闸。
    pub fn new() -> QuadGate {
        QuadGate {
            records: Vec::new(),
            declared_total: 0,
            at_home: 0,
            guided: 0,
            intercepted: 0,
            mismatched: 0,
        }
    }

    /// 判读并登记一次关系申报（边界 O(1)；重复能力走对拍修正）。
    ///
    /// - 能力名为空或超长 → [`SysBoundError::QuadBad`]（申报卡不完整）；
    /// - 申报账满 → [`SysBoundError::State`]；
    /// - 同能力指纹既往已有判读且本次结论变位 → 回执置
    ///   [`QuadReceipt::mismatch`] + 修正计数（关系失配 → 对拍修正）。
    pub fn declare(
        &mut self,
        capability: &str,
        declared: RelationKind,
        facts: RelationFacts,
    ) -> Result<QuadReceipt, SysBoundError> {
        if capability.is_empty() || capability.len() > QUAD_NAME_MAX {
            return Err(SysBoundError::QuadBad);
        }
        if self.records.len() >= QUAD_RECORD_CAP {
            return Err(SysBoundError::State);
        }
        let cap_fp = fp_str(FNV_SEED, capability);
        let outcome = judge_quad(declared, facts);
        // 对拍：查同能力既往判读结论是否变位。
        let mut mismatch = false;
        let mut i = 0;
        while i < self.records.len() {
            if self.records[i].capability_fp == cap_fp {
                let prior_code = self.records[i].outcome_code;
                if prior_code != outcome.code() {
                    mismatch = true;
                }
            }
            i += 1;
        }
        if mismatch {
            self.mismatched = self.mismatched.saturating_add(1);
        }
        match outcome {
            QuadOutcome::AtHome => self.at_home = self.at_home.saturating_add(1),
            QuadOutcome::GuidedHome(_) => self.guided = self.guided.saturating_add(1),
            QuadOutcome::Intercepted => self.intercepted = self.intercepted.saturating_add(1),
        }
        self.declared_total = self.declared_total.saturating_add(1);
        self.records.push(QuadRecord {
            capability_fp: cap_fp,
            declared_ord: declared.ord(),
            outcome_code: outcome.code(),
            guide_to_ord: match outcome.guided_to() {
                Some(r) => r.ord(),
                None => 0,
            },
            facts_fp: facts.fingerprint(),
        });
        Ok(QuadReceipt {
            capability_fp: cap_fp,
            outcome,
            mismatch,
        })
    }

    /// 查同能力最近一次判读结论（无记录 None）。
    pub fn last_outcome_code(&self, capability: &str) -> Option<u8> {
        let cap_fp = fp_str(FNV_SEED, capability);
        let mut found: Option<u8> = None;
        let mut i = 0;
        while i < self.records.len() {
            if self.records[i].capability_fp == cap_fp {
                found = Some(self.records[i].outcome_code);
            }
            i += 1;
        }
        found
    }

    /// 申报账自检：计数自洽（总量 = 三路之和 + 修正重复计入修正账）。
    pub fn audit(&self) -> Result<(), SysBoundError> {
        if self.records.len() > QUAD_RECORD_CAP {
            return Err(SysBoundError::State);
        }
        let three = self.at_home.saturating_add(self.guided).saturating_add(self.intercepted);
        if three != self.declared_total {
            return Err(SysBoundError::State);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、既有系统边界闭集：四域边界卡（可调用面 / 禁入面 / 数据交换协议）
// ---------------------------------------------------------------------------

/// 边界域数（脚本面对的既有系统域闭集恰四域）。
pub const DOMAIN_COUNT: usize = 4;

/// 既有系统域闭集（宿主内核域 / 图形域 / 输入域 / 文件域）。
///
/// 判别序数 1..=4 稳定冻结，白名单注册与句柄归属都靠序数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SysDomain {
    /// 宿主内核域：实体/场景/时钟/事件——编排的主面。
    HostKernel = 1,
    /// 图形域：绘制/贴图/材质——只读视图优先。
    Graphics = 2,
    /// 输入域：按键/指针——数值快照。
    Input = 3,
    /// 文件域：受控读写——授权写。
    File = 4,
}

impl SysDomain {
    /// 四域闭集。
    pub const ALL: [SysDomain; DOMAIN_COUNT] = [
        SysDomain::HostKernel,
        SysDomain::Graphics,
        SysDomain::Input,
        SysDomain::File,
    ];

    /// 域判别序数（1..=4）。
    pub const fn ord(self) -> u8 {
        self as u8
    }

    /// 序数反查域（越界 None）。
    pub const fn from_ord(o: u8) -> Option<SysDomain> {
        match o {
            1 => Some(SysDomain::HostKernel),
            2 => Some(SysDomain::Graphics),
            3 => Some(SysDomain::Input),
            4 => Some(SysDomain::File),
            _ => None,
        }
    }

    /// 域名（人读——读屏可达的短名）。
    pub const fn name(self) -> &'static str {
        match self {
            SysDomain::HostKernel => "宿主内核域",
            SysDomain::Graphics => "图形域",
            SysDomain::Input => "输入域",
            SysDomain::File => "文件域",
        }
    }

    /// 域在边界卡册中的下标（序数 - 1——const 对齐用）。
    pub const fn card_index(self) -> usize {
        (self as u8 - 1) as usize
    }
}

/// 域序数合法化（越界 → [`SysBoundError::DomainBad`]）。
pub fn domain_of_ord(o: u8) -> Result<SysDomain, SysBoundError> {
    match SysDomain::from_ord(o) {
        Some(d) => Ok(d),
        None => Err(SysBoundError::DomainBad),
    }
}

/// 边界卡（三段：可调用面 / 禁入面 / 数据交换协议——三段都写死 const）。
#[derive(Clone, Copy)]
pub struct BoundaryCard {
    /// 本卡约束的域。
    pub domain: SysDomain,
    /// 可调用面锚名单（白名单种子必须从此名单内取——编译期对齐断言）。
    pub callable: &'static [&'static str],
    /// 禁入面锚名单（脚本绝不碰——写了白名单也只开可调用面）。
    pub forbidden: &'static [&'static str],
    /// 数据交换协议（跨界面怎么传数据——与值闭集口径同源）。
    pub protocol: &'static str,
}

/// 四域边界卡册（const 冻结——开机即冻结运行期只读不改）。
pub const BOUNDARY_CARDS: [BoundaryCard; DOMAIN_COUNT] = [
    BoundaryCard {
        domain: SysDomain::HostKernel,
        callable: &["实体查询", "实体创建", "场景查询", "时钟读取", "事件投递"],
        forbidden: &["内存直接分配", "线程派生", "内核句柄直取", "驱动直写"],
        protocol: "值闭集传递：数值/字符串/句柄/只读视图四类——不传裸指针、不传闭包、不传宿主内部结构体。",
    },
    BoundaryCard {
        domain: SysDomain::Graphics,
        callable: &["绘制指令提交", "贴图快照", "材质参数设置"],
        forbidden: &["渲染管线重建", "着色器热替换", "帧缓冲直写"],
        protocol: "只读视图优先：贴图与网格以只读视图借给脚本——所有权不转移、到期回收。",
    },
    BoundaryCard {
        domain: SysDomain::Input,
        callable: &["按键状态读", "指针位置读"],
        forbidden: &["输入事件注入", "输入流劫持", "外设枚举直调"],
        protocol: "数值快照：跨界只传数值快照——不传事件对象本体、不订阅原始流。",
    },
    BoundaryCard {
        domain: SysDomain::File,
        callable: &["受控落盘", "受控读档"],
        forbidden: &["直接写盘", "路径自由拼接", "进程调用"],
        protocol: "授权写：一切持久化持显式授权位走引擎显式通道——默认无副作用泄漏（安全默认律落地）。",
    },
];

/// const 内字符串相等（逐字节——const fn 纪律：不用 derive 判等）。
const fn str_eq(a: &str, b: &str) -> bool {
    let ba = a.as_bytes();
    let bb = b.as_bytes();
    if ba.len() != bb.len() {
        return false;
    }
    let mut i = 0;
    while i < ba.len() {
        if ba[i] != bb[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// const 判定名字是否在某域卡可调用面名单内（编译期对齐断言用）。
const fn card_has_callable(card: &BoundaryCard, name: &str) -> bool {
    let list = card.callable;
    let mut i = 0;
    while i < list.len() {
        if str_eq(list[i], name) {
            return true;
        }
        i += 1;
    }
    false
}

/// const 判定名字是否在某域卡禁入面名单内（注册防线复查用）。
pub const fn card_forbids(domain: SysDomain, name: &str) -> bool {
    let list = BOUNDARY_CARDS[domain.card_index()].forbidden;
    let mut i = 0;
    while i < list.len() {
        if str_eq(list[i], name) {
            return true;
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// 五、错误枚举（诊断码 0x1D50..=0x1D5F，detail 全说人话给改法）
// ---------------------------------------------------------------------------

/// 边界域错误全集（16 变体——恰对位 0x1D50..=0x1D5F 十六码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SysBoundError {
    /// 关系四定违定（预留私启拦截的正式错误码）。
    QuadViol,
    /// 关系申报卡不完整（能力名为空或超长）。
    QuadBad,
    /// 边界域序数不合法。
    DomainBad,
    /// 白名单重复注册。
    ApiDup,
    /// 白名单容量满。
    ApiFull,
    /// 越白名单调用（查无此名——越界调用注入正式检出码）。
    ApiNotFound,
    /// 签名失配（实参个数与注册签名不符）。
    SigMismatch,
    /// 跨界值类型不匹配。
    TypeMismatch,
    /// 句柄滥用（不存在/已回收/再转移/视图冒充）。
    HandleBad,
    /// 未授权写操作（无授权位——未授权写注入正式检出码）。
    AuthRequired,
    /// 授权额度耗尽（授权位在预算为零）。
    AuthSpent,
    /// 高频调用熔断中。
    BreakerTripped,
    /// 事件账满。
    LedgerFull,
    /// 参数不合法（空名/超长串/调用方 id 越界）。
    ParamBad,
    /// 结构性注入兜底（不可能状态被触达）。
    Inject,
    /// 状态错序（槽位/账目/时序错乱或容量缺口）。
    State,
}

impl SysBoundError {
    /// 诊断码（与 [`CODES`] 逐位对位——编译期与运行期双断言）。
    pub const fn code(self) -> u16 {
        match self {
            SysBoundError::QuadViol => E_QUAD_VIOL,
            SysBoundError::QuadBad => E_QUAD_BAD,
            SysBoundError::DomainBad => E_DOMAIN_BAD,
            SysBoundError::ApiDup => E_API_DUP,
            SysBoundError::ApiFull => E_API_FULL,
            SysBoundError::ApiNotFound => E_API_NOT_FOUND,
            SysBoundError::SigMismatch => E_SIG_MISMATCH,
            SysBoundError::TypeMismatch => E_TYPE_MISMATCH,
            SysBoundError::HandleBad => E_HANDLE_BAD,
            SysBoundError::AuthRequired => E_AUTH_REQUIRED,
            SysBoundError::AuthSpent => E_AUTH_SPENT,
            SysBoundError::BreakerTripped => E_BREAKER_TRIPPED,
            SysBoundError::LedgerFull => E_LEDGER_FULL,
            SysBoundError::ParamBad => E_PARAM_BAD,
            SysBoundError::Inject => E_INJECT,
            SysBoundError::State => E_STATE,
        }
    }

    /// 中文可操作诊断（说人话给改法——错误友好律在本域的落地）。
    pub fn detail(&self) -> &'static str {
        match self {
            SysBoundError::QuadViol => {
                "关系四定违定：预留私启零容忍。把该能力改回预留登记（不开任何上架通路），或改报引擎/插件/配置关系后重新申报。"
            }
            SysBoundError::QuadBad => {
                "关系申报卡不完整：能力名为空或超长（上限 64 字节）。给能力一个非空短名再申报，名字只作指纹键不作文档。"
            }
            SysBoundError::DomainBad => {
                "边界域序数不合法：域序数须落 1..=4（1 宿主内核/2 图形/3 输入/4 文件）。用 SysDomain::from_ord 先验再传。"
            }
            SysBoundError::ApiDup => {
                "白名单重复注册：同名 API 已存在。先 lookup 确认是否已注册；确属升级改签名请走新名注册，不改旧面。"
            }
            SysBoundError::ApiFull => {
                "白名单容量满（API_CAP=48）：查表面有界不可破。合并同域细粒度 API 或下线旧面后再注册。"
            }
            SysBoundError::ApiNotFound => {
                "越白名单调用：查无此名已拦截。只许调 InteropSurface::register 注册过的面；新能力先走关系四定申报再注册。"
            }
            SysBoundError::SigMismatch => {
                "签名失配：实参个数与注册签名不符。查该 API 的 SigSpec 参数个数，补齐或缺省化实参后再调。"
            }
            SysBoundError::TypeMismatch => {
                "跨界值类型不匹配：实参类型与签名参数型不对位。跨边界只传数值/字符串/句柄/只读视图四类，按签名逐位对型，不做静默转换。"
            }
            SysBoundError::HandleBad => {
                "句柄滥用：句柄不存在、已到期回收、已转移再转、或只读视图越权冒充实体的句柄。查句柄当前态（借出/转移/回收），回收后的句柄永久封闭须重新获取。"
            }
            SysBoundError::AuthRequired => {
                "未授权写操作：Effect 面调用默认拒绝（安全默认律）。由宿主先 grant_write 签发授权位（脚本×API×预算），再重发该调用。"
            }
            SysBoundError::AuthSpent => {
                "授权额度耗尽：授权位在但预算为零。请宿主重新 grant_write 续签；预算耗尽是授权的自然到期，不作静默续期。"
            }
            SysBoundError::BreakerTripped => {
                "高频调用熔断中：单脚本调用计数达阈值。等冷却（连续 8 次拒唤自动复路）或由宿主拆分调用批；熔断是为保帧预算。"
            }
            SysBoundError::LedgerFull => {
                "事件账满：账容量到顶拒收待清。调用 EventLedger::drain 清账后重试；事件不静默丢弃。"
            }
            SysBoundError::ParamBad => {
                "参数不合法：空名/超长串/调用方 id 越界。跨边界值必须小而有界（串上限 96 字节），调用方 id 须落 1..=4096。"
            }
            SysBoundError::Inject => {
                "结构性注入兜底：触达了内部不变量定义的不可能状态（如已发放区间内出现空槽）。立即立案排查，视为注入攻击面处理。"
            }
            SysBoundError::State => {
                "状态错序：槽位/账目/时序错乱或容量缺口。先查调用次序（未注册先调用/账满再入账）与容量常量，再核对审计账自检输出。"
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 六、数据交换协议：跨边界值类型闭集 + 参数型闭集 + 签名
// ---------------------------------------------------------------------------

/// 跨边界字符串值上限（96 字节——跨界值是摘要级数据不是载荷，小而有界）。
pub const PARAM_STR_MAX: usize = 96;

/// API 名上限（48 字节——白名单条目名短而有界）。
pub const API_NAME_MAX: usize = 48;

/// 签名参数槽上限（4 参——跨界调用面窄接口纪律：多于 4 参请打包为只读视图）。
pub const SIG_ARGS_MAX: usize = 4;

/// 参数型闭集（与 [`SysValue`] 值闭集一一对位——序数即类型对位键）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgType {
    /// 数值（i64）。
    Number = 1,
    /// 字符串（有界 UTF-8）。
    Str = 2,
    /// 句柄（实体所有权/使用权凭证——可借出可转移）。
    Handle = 3,
    /// 只读视图（借用语义凭证——不可转移不可冒充实体的句柄）。
    View = 4,
}

impl ArgType {
    /// 型序数（1..=4——与 SysValue::tag 对位）。
    pub const fn tag(self) -> u8 {
        self as u8
    }

    /// 型名（人读——类型不匹配报错给改法用）。
    pub const fn name(self) -> &'static str {
        match self {
            ArgType::Number => "数值",
            ArgType::Str => "字符串",
            ArgType::Handle => "句柄",
            ArgType::View => "只读视图",
        }
    }
}

/// 跨边界值闭集（数值 / 字符串 / 句柄 / 只读视图——四类之外一律拒收）。
///
/// 协议口径与边界卡第三段同源：不传裸指针、不传闭包、不传宿主内部
/// 结构体。字符串值入闭集时做有界校验（超 [`PARAM_STR_MAX`] 拒收）。
#[derive(Clone, Debug)]
pub enum SysValue {
    /// 数值（i64——跨界数值统一 i64 定宽，确定性对拍要求）。
    Num(i64),
    /// 字符串（有界——构造走 [`SysValue::str_checked`] 校验长度）。
    Str(String),
    /// 句柄 id（>0——句柄表发放的实体凭证）。
    Hnd(u32),
    /// 只读视图 id（>0——句柄表发放的借用凭证）。
    View(u32),
}

impl SysValue {
    /// 有界字符串值构造（空串与超长拒收——协议闸门拒收不截断）。
    pub fn str_checked(s: &str) -> Result<SysValue, SysBoundError> {
        if s.is_empty() || s.len() > PARAM_STR_MAX {
            return Err(SysBoundError::ParamBad);
        }
        Ok(SysValue::Str(s.to_string()))
    }

    /// 值型序数（与 ArgType::tag 对位——类型匹配的机械判据）。
    pub fn tag(&self) -> u8 {
        match self {
            SysValue::Num(_) => 1,
            SysValue::Str(_) => 2,
            SysValue::Hnd(_) => 3,
            SysValue::View(_) => 4,
        }
    }

    /// 值的句柄 id（Hnd/View 有值，余者 None——句柄校验遍历用）。
    pub fn handle_id(&self) -> Option<u32> {
        match self {
            SysValue::Num(_) | SysValue::Str(_) => None,
            SysValue::Hnd(id) | SysValue::View(id) => Some(*id),
        }
    }

    /// 值摘要喂入（参数摘要指纹的单值面——类型序数 + 定宽数据）。
    fn fp_into(&self, h: u64) -> u64 {
        match self {
            SysValue::Num(v) => {
                let hh = fp_u32(h, 1);
                fp_u64(hh, *v as u64)
            }
            SysValue::Str(s) => {
                let hh = fp_u32(h, 2);
                fp_str(hh, s)
            }
            SysValue::Hnd(id) => {
                let hh = fp_u32(h, 3);
                fp_u32(hh, *id)
            }
            SysValue::View(id) => {
                let hh = fp_u32(h, 4);
                fp_u32(hh, *id)
            }
        }
    }
}

/// 签名（参数型槽 + 实参个数 + 返回型——白名单注册的签名面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SigSpec {
    /// 参数型槽（前 argn 个有效，余位填 Number 占位但不参与校验）。
    pub args: [ArgType; SIG_ARGS_MAX],
    /// 实参个数（0..=SIG_ARGS_MAX）。
    pub argn: u8,
    /// 返回型。
    pub ret: ArgType,
}

impl SigSpec {
    /// 签名构造（argn 超 SIG_ARGS_MAX 返回 None——窄接口纪律）。
    pub fn from_parts(args: [ArgType; SIG_ARGS_MAX], argn: u8, ret: ArgType) -> Option<SigSpec> {
        if argn as usize > SIG_ARGS_MAX {
            return None;
        }
        Some(SigSpec { args, argn, ret })
    }

    /// 签名指纹喂入（API 签名指纹的签名面）。
    fn fp_into(&self, h: u64) -> u64 {
        let hh = fp_u32(h, self.argn as u32);
        let mut i = 0;
        let mut out = hh;
        while i < SIG_ARGS_MAX {
            out = fp_u32(out, self.args[i].tag() as u32);
            i += 1;
        }
        fp_u32(out, self.ret.tag() as u32)
    }
}

// ---------------------------------------------------------------------------
// 七、互操作面：白名单注册制 + 签名指纹 + 越白名单调用拦截
// ---------------------------------------------------------------------------

/// API 读写种类闭集（只读 / 效果——效果面即写操作，默认拒绝需授权位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiKind {
    /// 只读面：无副作用，白名单内即放行。
    ReadOnly = 1,
    /// 效果面（写操作）：白名单之外还须显式授权位——安全默认律落地。
    Effect = 2,
}

impl ApiKind {
    /// 种类序数。
    pub const fn ord(self) -> u8 {
        self as u8
    }

    /// 种类名（人读——审计与读屏用）。
    pub const fn name(self) -> &'static str {
        match self {
            ApiKind::ReadOnly => "只读面",
            ApiKind::Effect => "效果面",
        }
    }
}

/// 白名单容量（48 条——查表面有界，边界 O(1) 的结构性保证）。
pub const API_CAP: usize = 48;

/// 白名单条目（名字 / 域 / 种类 / 签名 / 签名指纹——指纹可重算可验链）。
#[derive(Clone, Debug)]
pub struct ApiEntry {
    /// API 名（白名单唯一键）。
    pub name: String,
    /// 所属既有系统域。
    pub domain: SysDomain,
    /// 读写种类。
    pub kind: ApiKind,
    /// 签名。
    pub sig: SigSpec,
    /// 签名指纹（FNV-1a 64：名字+域+种类+签名一体入指纹）。
    pub fingerprint: u64,
}

/// 规范 API 种子卡（开机白名单的 const 种源——名字必须落在对应域卡
/// 可调用面名单内，编译期断言对齐）。
pub struct CanonicalApi {
    /// API 名（= 域卡可调用面名单内的一项）。
    pub name: &'static str,
    /// 所属域。
    pub domain: SysDomain,
    /// 读写种类。
    pub kind: ApiKind,
    /// 参数型（前 argn 个有效）。
    pub args: [ArgType; SIG_ARGS_MAX],
    /// 实参个数。
    pub argn: u8,
    /// 返回型。
    pub ret: ArgType,
}

/// 开机白名单种子（六条规范 API——四域全覆盖，引擎域两条）：
///
/// | 名 | 域 | 种类 | 签名 | 说明 |
/// |---|---|---|---|---|
/// | 实体查询 | 内核 | 只读 | (串)->数 | 按名查实体计数 |
/// | 实体创建 | 内核 | 效果 | (串)->句柄 | 写操作：创建实体发句柄 |
/// | 绘制指令提交 | 图形 | 只读 | (视图)->数 | 提交绘制引用 |
/// | 贴图快照 | 图形 | 只读 | (数)->视图 | 取只读视图 |
/// | 按键状态读 | 输入 | 只读 | (数)->数 | 数值快照 |
/// | 受控落盘 | 文件 | 效果 | (串)->数 | 写操作：授权写显式通道 |
pub const CANONICAL_APIS: [CanonicalApi; 6] = [
    CanonicalApi {
        name: "实体查询",
        domain: SysDomain::HostKernel,
        kind: ApiKind::ReadOnly,
        args: [ArgType::Str, ArgType::Number, ArgType::Number, ArgType::Number],
        argn: 1,
        ret: ArgType::Number,
    },
    CanonicalApi {
        name: "实体创建",
        domain: SysDomain::HostKernel,
        kind: ApiKind::Effect,
        args: [ArgType::Str, ArgType::Number, ArgType::Number, ArgType::Number],
        argn: 1,
        ret: ArgType::Handle,
    },
    CanonicalApi {
        name: "绘制指令提交",
        domain: SysDomain::Graphics,
        kind: ApiKind::ReadOnly,
        args: [ArgType::View, ArgType::Number, ArgType::Number, ArgType::Number],
        argn: 1,
        ret: ArgType::Number,
    },
    CanonicalApi {
        name: "贴图快照",
        domain: SysDomain::Graphics,
        kind: ApiKind::ReadOnly,
        args: [ArgType::Number, ArgType::Number, ArgType::Number, ArgType::Number],
        argn: 1,
        ret: ArgType::View,
    },
    CanonicalApi {
        name: "按键状态读",
        domain: SysDomain::Input,
        kind: ApiKind::ReadOnly,
        args: [ArgType::Number, ArgType::Number, ArgType::Number, ArgType::Number],
        argn: 1,
        ret: ArgType::Number,
    },
    CanonicalApi {
        name: "受控落盘",
        domain: SysDomain::File,
        kind: ApiKind::Effect,
        args: [ArgType::Str, ArgType::Number, ArgType::Number, ArgType::Number],
        argn: 1,
        ret: ArgType::Number,
    },
];

/// 互操作面（脚本可调宿主 API 的唯一合法通道——白名单注册制）。
pub struct InteropSurface {
    /// 白名单条目（有界 Vec——容量 API_CAP）。
    pub entries: Vec<ApiEntry>,
    /// 重复注册拒收计数。
    pub dup_rejected: u32,
    /// 注册总数（含后续成功——与 entries.len() 差即下线数，本版无下线）。
    pub registered_total: u32,
}

impl InteropSurface {
    /// 开机空面（种子由 [`SysBoundWorld::boot`] 按 [`CANONICAL_APIS`] 灌入）。
    pub fn new() -> InteropSurface {
        InteropSurface {
            entries: Vec::new(),
            dup_rejected: 0,
            registered_total: 0,
        }
    }

    /// API 签名指纹（名字+域+种类+签名一体入指纹——可重算可验链）。
    pub fn fingerprint_of(name: &str, domain: SysDomain, kind: ApiKind, sig: &SigSpec) -> u64 {
        let h = fp_str(FNV_SEED, name);
        let h = fp_u32(h, domain.ord() as u32);
        let h = fp_u32(h, kind.ord() as u32);
        sig.fp_into(h)
    }

    /// 白名单注册（名字非空不超长 / 不与禁入面撞名 / 不重复 / 容量有界）。
    ///
    /// - 空名或超长 → [`SysBoundError::ParamBad`]；
    /// - 与所属域卡**禁入面**撞名 → [`SysBoundError::QuadViol`]
    ///   （禁入面是 const 卡写死的——白名单注册也开不进禁入面）；
    /// - 同名重复 → [`SysBoundError::ApiDup`]（拒收计数）；
    /// - 容量满 → [`SysBoundError::ApiFull`]；
    /// - 成功返回**槽位 id**（0 起单调——授权位/审计账引用它）。
    pub fn register(
        &mut self,
        name: &str,
        domain: SysDomain,
        kind: ApiKind,
        sig: SigSpec,
    ) -> Result<u32, SysBoundError> {
        if name.is_empty() || name.len() > API_NAME_MAX {
            return Err(SysBoundError::ParamBad);
        }
        if card_forbids(domain, name) {
            return Err(SysBoundError::QuadViol);
        }
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].name == name {
                self.dup_rejected = self.dup_rejected.saturating_add(1);
                return Err(SysBoundError::ApiDup);
            }
            i += 1;
        }
        if self.entries.len() >= API_CAP {
            return Err(SysBoundError::ApiFull);
        }
        let fp = InteropSurface::fingerprint_of(name, domain, kind, &sig);
        let slot = self.entries.len() as u32;
        self.entries.push(ApiEntry {
            name: name.to_string(),
            domain,
            kind,
            sig,
            fingerprint: fp,
        });
        self.registered_total = self.registered_total.saturating_add(1);
        Ok(slot)
    }

    /// 查名取槽位（线性有界扫描——API_CAP 有界故 O(1) 上界恒定）。
    pub fn lookup(&self, name: &str) -> Option<u32> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].name == name {
                return Some(i as u32);
            }
            i += 1;
        }
        None
    }

    /// 槽位取条目（越界 None）。
    pub fn entry(&self, slot: u32) -> Option<&ApiEntry> {
        if (slot as usize) < self.entries.len() {
            Some(&self.entries[slot as usize])
        } else {
            None
        }
    }

    /// 验链：全表指纹重算比对（指纹被改即验出——白名单防篡改面）。
    pub fn verify_all(&self) -> Result<(), SysBoundError> {
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            let fp = InteropSurface::fingerprint_of(&e.name, e.domain, e.kind, &e.sig);
            if fp != e.fingerprint {
                return Err(SysBoundError::State);
            }
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 八、句柄生命周期语义闭集（借出 / 转移 / 到期回收）
// ---------------------------------------------------------------------------

/// 句柄表容量（256 槽——句柄面有界，id 单调发放不复用）。
pub const HANDLE_CAP: usize = 256;

/// 句柄状态闭集（空闲 / 借出 / 已转移 / 到期回收——生命周期语义四态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleState {
    /// 空闲（未发放）。
    Free = 0,
    /// 借出（脚本持有——可作实参、可一次性转移）。
    Lent = 1,
    /// 已转移（新持有方持有——不得再转，待到期回收）。
    Transferred = 2,
    /// 到期回收（永久封闭——id 不复用，再用即句柄滥用）。
    Recycled = 3,
}

impl HandleState {
    /// 态序数。
    pub const fn ord(self) -> u8 {
        self as u8
    }

    /// 态名（人读——句柄滥用报错给改法用）。
    pub const fn name(self) -> &'static str {
        match self {
            HandleState::Free => "空闲",
            HandleState::Lent => "借出",
            HandleState::Transferred => "已转移",
            HandleState::Recycled => "到期回收",
        }
    }
}

/// 句柄槽记录（状态 / 持有方 / 归属域 / 是否只读视图 / 发放流水）。
#[derive(Clone, Copy, Debug)]
pub struct HandleRecord {
    /// 生命周期状态。
    pub state: HandleState,
    /// 持有方脚本 id（0 = 宿主）。
    pub owner: u32,
    /// 归属既有系统域。
    pub domain: SysDomain,
    /// 是否只读视图（视图借用语义：不可转移、不得冒充实体的句柄）。
    pub is_view: bool,
    /// 发放时的世界操作流水（确定性对拍面）。
    pub born_seq: u64,
}

/// 句柄表（发放 / 转移 / 到期回收 / 实参校验——生命周期唯一管理件）。
pub struct HandleTable {
    /// 槽记录（恒 HANDLE_CAP 条，开机即 Free）。
    pub slots: Vec<HandleRecord>,
    /// 已发放数（槽位下标 0..issued；id = 下标 + 1，0 不是合法句柄）。
    pub issued: u32,
    /// 到期回收累计数。
    pub recycled_total: u32,
    /// 转移累计数。
    pub transferred_total: u32,
    /// 句柄滥用拒收计数（注入面之二的本表侧计数）。
    pub abuse_rejected: u32,
    /// 世界操作流水引用值（发放时盖章用——由世界体灌入）。
    pub op_seq: u64,
}

impl HandleTable {
    /// 开机空表（HANDLE_CAP 槽全 Free——容量预分配不逐次扩）。
    pub fn new() -> HandleTable {
        let mut slots = Vec::new();
        let mut i = 0;
        while i < HANDLE_CAP {
            slots.push(HandleRecord {
                state: HandleState::Free,
                owner: 0,
                domain: SysDomain::HostKernel,
                is_view: false,
                born_seq: 0,
            });
            i += 1;
        }
        HandleTable {
            slots,
            issued: 0,
            recycled_total: 0,
            transferred_total: 0,
            abuse_rejected: 0,
            op_seq: 0,
        }
    }

    /// 发放句柄（借出态归调用方——表满走 [`SysBoundError::State`]）。
    pub fn acquire(
        &mut self,
        owner: u32,
        domain: SysDomain,
        is_view: bool,
    ) -> Result<u32, SysBoundError> {
        if self.issued as usize >= HANDLE_CAP {
            return Err(SysBoundError::State);
        }
        let idx = self.issued as usize;
        let id = (idx + 1) as u32;
        self.slots[idx] = HandleRecord {
            state: HandleState::Lent,
            owner,
            domain,
            is_view,
            born_seq: self.op_seq,
        };
        self.issued = self.issued.saturating_add(1);
        Ok(id)
    }

    /// 槽位下标合法化（id 须落 1..=issued——越界即滥用）。
    fn idx_of(&mut self, id: u32) -> Result<usize, SysBoundError> {
        if id == 0 || (id as usize) > self.issued as usize {
            self.abuse_rejected = self.abuse_rejected.saturating_add(1);
            return Err(SysBoundError::HandleBad);
        }
        Ok((id - 1) as usize)
    }

    /// 实参句柄校验（跨界调用的句柄面闸门——类型之外的生命周期校验）。
    ///
    /// `want_view`：实参型是 [`ArgType::View`] 时置真。校验矩阵：
    /// - id 非法（0 或超已发放区间）→ [`SysBoundError::HandleBad`]；
    /// - 已到期回收 → [`SysBoundError::HandleBad`]（回收后永久封闭）；
    /// - 空闲态（结构性不可能）→ [`SysBoundError::Inject`]（注入兜底）；
    /// - 视图型实参撞上实体的句柄 / 实体型实参撞上视图 →
    ///   [`SysBoundError::HandleBad`]（冒充即滥用）；
    /// - 视图须处于借出态（转移态的视图不可再作实参——借用语义闭集）。
    pub fn validate_arg(&mut self, id: u32, want_view: bool) -> Result<(), SysBoundError> {
        let idx = self.idx_of(id)?;
        let rec = self.slots[idx];
        match rec.state {
            HandleState::Recycled => {
                self.abuse_rejected = self.abuse_rejected.saturating_add(1);
                Err(SysBoundError::HandleBad)
            }
            HandleState::Free => Err(SysBoundError::Inject),
            HandleState::Lent | HandleState::Transferred => {
                if want_view != rec.is_view {
                    self.abuse_rejected = self.abuse_rejected.saturating_add(1);
                    return Err(SysBoundError::HandleBad);
                }
                if want_view && rec.state != HandleState::Lent {
                    self.abuse_rejected = self.abuse_rejected.saturating_add(1);
                    return Err(SysBoundError::HandleBad);
                }
                Ok(())
            }
        }
    }

    /// 转移（一次性：借出 → 已转移；再转 / 视图转移 / 非持有方发起 → 滥用）。
    pub fn transfer(&mut self, id: u32, from: u32, to: u32) -> Result<(), SysBoundError> {
        if to == 0 || to == from {
            return Err(SysBoundError::ParamBad);
        }
        let idx = self.idx_of(id)?;
        let rec = self.slots[idx];
        if rec.is_view {
            self.abuse_rejected = self.abuse_rejected.saturating_add(1);
            return Err(SysBoundError::HandleBad);
        }
        if rec.state != HandleState::Lent {
            self.abuse_rejected = self.abuse_rejected.saturating_add(1);
            return Err(SysBoundError::HandleBad);
        }
        if rec.owner != from {
            self.abuse_rejected = self.abuse_rejected.saturating_add(1);
            return Err(SysBoundError::HandleBad);
        }
        self.slots[idx].state = HandleState::Transferred;
        self.slots[idx].owner = to;
        self.transferred_total = self.transferred_total.saturating_add(1);
        Ok(())
    }

    /// 到期回收（借出/已转移 → 回收；回收后的 id 永久封闭不复用）。
    pub fn recycle(&mut self, id: u32, by: u32) -> Result<(), SysBoundError> {
        let idx = self.idx_of(id)?;
        let rec = self.slots[idx];
        match rec.state {
            HandleState::Lent | HandleState::Transferred => {
                if rec.owner != by && by != 0 {
                    self.abuse_rejected = self.abuse_rejected.saturating_add(1);
                    return Err(SysBoundError::HandleBad);
                }
                self.slots[idx].state = HandleState::Recycled;
                self.recycled_total = self.recycled_total.saturating_add(1);
                Ok(())
            }
            HandleState::Free => Err(SysBoundError::Inject),
            HandleState::Recycled => {
                self.abuse_rejected = self.abuse_rejected.saturating_add(1);
                Err(SysBoundError::HandleBad)
            }
        }
    }

    /// 存活句柄数（已发放 − 已回收——health 与自检共用口径）。
    pub fn live(&self) -> u32 {
        self.issued.saturating_sub(self.recycled_total)
    }

    /// 表自检：状态闭集内 / 存活数对账 / 已发放区间无 Free（结构不变量）。
    pub fn audit(&self) -> Result<(), SysBoundError> {
        if self.slots.len() != HANDLE_CAP {
            return Err(SysBoundError::State);
        }
        let mut non_free = 0u32;
        let mut recycled = 0u32;
        let mut i = 0;
        while i < self.slots.len() {
            let st = self.slots[i].state;
            if i as u32 >= self.issued && st != HandleState::Free {
                return Err(SysBoundError::State);
            }
            match st {
                HandleState::Free => {}
                HandleState::Lent | HandleState::Transferred => {
                    non_free = non_free.saturating_add(1);
                }
                HandleState::Recycled => {
                    non_free = non_free.saturating_add(1);
                    recycled = recycled.saturating_add(1);
                }
            }
            i += 1;
        }
        if non_free != self.issued || recycled != self.recycled_total {
            return Err(SysBoundError::State);
        }
        if self.live() != self.issued.saturating_sub(self.recycled_total) {
            return Err(SysBoundError::State);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 九、授权位 + 授权账（安全默认律在边界上的落地件）
// ---------------------------------------------------------------------------

/// 授权位容量（32 位并存——授权面有界）。
pub const GRANT_CAP: usize = 32;

/// 授权账容量（append-only 64 条——签发历史不可静默撤销）。
pub const GRANT_LEDGER_CAP: usize = 64;

/// 授权位（脚本 × API 槽位 × 次数预算——预算耗尽即自然到期）。
#[derive(Clone, Copy, Debug)]
pub struct Grant {
    /// 授权的 API 槽位。
    pub api_slot: u32,
    /// 被授权脚本 id。
    pub script: u32,
    /// 剩余次数预算（0 即到期——不作静默续期）。
    pub budget: u32,
    /// 签发时的世界操作流水（确定性对拍面）。
    pub granted_seq: u64,
}

/// 授权账条目（append-only 签发留痕）。
#[derive(Clone, Copy, Debug)]
pub struct GrantEntry {
    /// 授权的 API 槽位。
    pub api_slot: u32,
    /// 被授权脚本 id。
    pub script: u32,
    /// 签发预算（签发瞬间的值——历史不改写）。
    pub budget_at_grant: u32,
    /// 签发流水。
    pub seq: u64,
}

/// 授权账（Effect 面调用的唯一放行依据——默认无授权即拒绝）。
pub struct AuthLedger {
    /// 在管授权位（有界 Vec——容量 GRANT_CAP）。
    pub grants: Vec<Grant>,
    /// 签发留痕账（append-only——容量 GRANT_LEDGER_CAP）。
    pub ledger: Vec<GrantEntry>,
    /// 累计签发数。
    pub issued_total: u32,
    /// 累计核销数（Effect 调用成功即核销一次预算）。
    pub spent_total: u32,
    /// 签发账满拒收计数。
    pub full_rejected: u32,
    /// 世界操作流水引用值（签发盖章用——由世界体灌入）。
    pub op_seq: u64,
}

impl AuthLedger {
    /// 开机空账（默认零授权——安全默认律：默认无副作用泄漏）。
    pub fn new() -> AuthLedger {
        AuthLedger {
            grants: Vec::new(),
            ledger: Vec::new(),
            issued_total: 0,
            spent_total: 0,
            full_rejected: 0,
            op_seq: 0,
        }
    }

    /// 宿主签发授权位（脚本 × API 槽位 × 预算——同键续签即刷新预算）。
    ///
    /// - 槽位非法 → [`SysBoundError::ApiNotFound`]；
    /// - 预算为 0 → [`SysBoundError::ParamBad`]（签零预算不是授权是空文）；
    /// - 同（脚本,槽位）已有授权位 → 刷新预算（续签语义，一条不重复占位）；
    /// - 新签且位满 → [`SysBoundError::State`]；
    /// - 留痕账满 → [`SysBoundError::LedgerFull`]（不静默丢签发史）。
    pub fn grant(
        &mut self,
        script: u32,
        api_slot: u32,
        budget: u32,
        api_kind: ApiKind,
    ) -> Result<(), SysBoundError> {
        if budget == 0 {
            return Err(SysBoundError::ParamBad);
        }
        if api_kind.ord() != 2 {
            return Err(SysBoundError::State);
        }
        // 原子性前置检查：留痕账满先拒收——签发不落任何一半（签发与留痕
        // 原子，账满时既不刷新预算也不发新位）。
        if self.ledger.len() >= GRANT_LEDGER_CAP {
            self.full_rejected = self.full_rejected.saturating_add(1);
            return Err(SysBoundError::LedgerFull);
        }
        let seq = self.op_seq;
        let mut i = 0;
        while i < self.grants.len() {
            if self.grants[i].script == script && self.grants[i].api_slot == api_slot {
                self.grants[i].budget = budget;
                self.grants[i].granted_seq = seq;
                self.issued_total = self.issued_total.saturating_add(1);
                return self.append_entry(api_slot, script, budget, seq);
            }
            i += 1;
        }
        if self.grants.len() >= GRANT_CAP {
            return Err(SysBoundError::State);
        }
        self.grants.push(Grant {
            api_slot,
            script,
            budget,
            granted_seq: seq,
        });
        self.issued_total = self.issued_total.saturating_add(1);
        self.append_entry(api_slot, script, budget, seq)
    }

    /// 留痕账追加（满 → [`SysBoundError::LedgerFull`]——签发已生效但史不
    /// 可记时按满拒收整笔，签发与留痕原子）。
    fn append_entry(
        &mut self,
        api_slot: u32,
        script: u32,
        budget: u32,
        seq: u64,
    ) -> Result<(), SysBoundError> {
        if self.ledger.len() >= GRANT_LEDGER_CAP {
            self.full_rejected = self.full_rejected.saturating_add(1);
            return Err(SysBoundError::LedgerFull);
        }
        self.ledger.push(GrantEntry {
            api_slot,
            script,
            budget_at_grant: budget,
            seq,
        });
        Ok(())
    }

    /// 查授权位（脚本 × 槽位——在管位下标；无授权 None）。
    pub fn find(&self, script: u32, api_slot: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.grants.len() {
            if self.grants[i].script == script && self.grants[i].api_slot == api_slot {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 在管授权位数。
    pub fn active(&self) -> u32 {
        self.grants.len() as u32
    }

    /// 核销一次预算（Effect 调用成功时调用——饱和减不回卷）。
    pub fn spend(&mut self, idx: usize) {
        if idx < self.grants.len() {
            self.grants[idx].budget = self.grants[idx].budget.saturating_sub(1);
            self.spent_total = self.spent_total.saturating_add(1);
        }
    }

    /// 账自检：位有界 / 留痕有界 / 签发数 ≥ 留痕数（续签同键不增位）。
    pub fn audit(&self) -> Result<(), SysBoundError> {
        if self.grants.len() > GRANT_CAP || self.ledger.len() > GRANT_LEDGER_CAP {
            return Err(SysBoundError::State);
        }
        if self.issued_total < self.ledger.len() as u32 {
            return Err(SysBoundError::State);
        }
        let mut i = 0;
        while i < self.grants.len() {
            if self.grants[i].script == 0 {
                return Err(SysBoundError::State);
            }
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 十、边界审计：定长环形入账 + 每脚本计量器 + 高频熔断
// ---------------------------------------------------------------------------

/// 环形审计账容量（64 条——审计面永远有最近一段完整轨迹）。
pub const AUDIT_RING_CAP: usize = 64;

/// 熔断阈值（96 次——口径注：单帧脚本预算以内的保守上限，写死不复算）。
pub const CALL_BREAKER_THRESHOLD: u64 = 96;

/// 熔断冷却拒唤数（连续 8 次拒唤自动复路——冷却路径确定可对拍）。
pub const BREAKER_COOLDOWN: u32 = 8;

/// 调用方 id 上限（4096——脚本 id 有界，计量器表有界）。
pub const SCRIPT_ID_MAX: u32 = 4096;

/// 计量器容量（128 个脚本位——超出走 State 显性报）。
pub const METER_CAP: usize = 128;

/// 审计账条目（流水号 / 调用方 / 被调面 / 参数摘要 / 成败位——跨界调用
/// 无论成败一律入账）。
#[derive(Clone, Copy, Debug)]
pub struct AuditEntry {
    /// 流水号（单调不回退——环形覆盖不改流水）。
    pub seq: u64,
    /// 调用方脚本 id。
    pub caller: u32,
    /// 被调面槽位（查无此名时为 0——名字本身已入事件账备注）。
    pub api_slot: u32,
    /// 参数摘要指纹（FNV-1a 64——只入指纹不入原文）。
    pub digest: u64,
    /// 成败位。
    pub ok: bool,
}

/// 每脚本调用计量器（计数 / 熔断态 / 冷却进度）。
#[derive(Clone, Copy, Debug)]
pub struct CallerMeter {
    /// 脚本 id。
    pub script: u32,
    /// 通过熔断闸的调用计数（达到阈值即熔断——饱和不回卷）。
    pub calls: u64,
    /// 累计拒绝总数（熔断期拒唤）。
    pub rejected: u64,
    /// 熔断态。
    pub tripped: bool,
    /// 熔断期拒唤进度（达冷却数自动复路）。
    pub trip_rejected: u32,
}

/// 边界审计（环形入账 + 计量器表 + 熔断器）。
pub struct BoundaryAudit {
    /// 环形账（定长 Vec——满则覆盖最旧）。
    pub ring: Vec<AuditEntry>,
    /// 环内有效条数（< 容量时即已写入数）。
    pub filled: usize,
    /// 环写指针（下一写入下标 = total % 容量）。
    pub head: usize,
    /// 累计入账数。
    pub total: u64,
    /// 下一流水号。
    pub next_seq: u64,
    /// 计量器表（按脚本 id 线性查——容量有界）。
    pub meters: Vec<CallerMeter>,
    /// 熔断触发累计数。
    pub trips: u32,
    /// 熔断冷却累计数。
    pub cools: u32,
}

impl BoundaryAudit {
    /// 开机空账（环形账预分配——不逐次扩）。
    pub fn new() -> BoundaryAudit {
        let mut ring = Vec::new();
        let mut i = 0;
        while i < AUDIT_RING_CAP {
            ring.push(AuditEntry {
                seq: 0,
                caller: 0,
                api_slot: 0,
                digest: 0,
                ok: false,
            });
            i += 1;
        }
        BoundaryAudit {
            ring,
            filled: 0,
            head: 0,
            total: 0,
            next_seq: 1,
            meters: Vec::new(),
            trips: 0,
            cools: 0,
        }
    }

    /// 入账一条（成败都记——满则覆盖最旧，流水号单调不回退）。
    pub fn push(&mut self, caller: u32, api_slot: u32, digest: u64, ok: bool) {
        let seq = self.next_seq;
        self.ring[self.head] = AuditEntry {
            seq,
            caller,
            api_slot,
            digest,
            ok,
        };
        self.head = (self.head + 1) % AUDIT_RING_CAP;
        if self.filled < AUDIT_RING_CAP {
            self.filled += 1;
        }
        self.total = self.total.saturating_add(1);
        self.next_seq = self.next_seq.saturating_add(1);
    }

    /// 按时间序读环（最旧在前——审计回放用）。
    pub fn iter_oldest_first(&self) -> Vec<AuditEntry> {
        let mut out = Vec::new();
        if self.filled < AUDIT_RING_CAP {
            let mut i = 0;
            while i < self.filled {
                out.push(self.ring[i]);
                i += 1;
            }
        } else {
            let mut i = self.head;
            let mut n = 0;
            while n < AUDIT_RING_CAP {
                out.push(self.ring[i]);
                i = (i + 1) % AUDIT_RING_CAP;
                n += 1;
            }
        }
        out
    }

    /// 取计量器（无则建——容量满走 [`SysBoundError::State`]）。
    pub fn meter_for(&mut self, script: u32) -> Result<usize, SysBoundError> {
        let mut i = 0;
        while i < self.meters.len() {
            if self.meters[i].script == script {
                return Ok(i);
            }
            i += 1;
        }
        if self.meters.len() >= METER_CAP {
            return Err(SysBoundError::State);
        }
        self.meters.push(CallerMeter {
            script,
            calls: 0,
            rejected: 0,
            tripped: false,
            trip_rejected: 0,
        });
        Ok(self.meters.len() - 1)
    }

    /// 熔断闸检（熔断中 → 拒唤计数 + 冷却进度；到冷却数自动复路）。
    ///
    /// 返回 Ok(()) 表示闸开可继续本次调用；Err(BreakerTripped) 表示本次
    /// 被熔断拒唤（复路进度已推进，到点自动冷却）。
    pub fn gate(&mut self, script: u32) -> Result<(), SysBoundError> {
        let idx = self.meter_for(script)?;
        if self.meters[idx].tripped {
            self.meters[idx].rejected = self.meters[idx].rejected.saturating_add(1);
            self.meters[idx].trip_rejected = self.meters[idx].trip_rejected.saturating_add(1);
            if self.meters[idx].trip_rejected >= BREAKER_COOLDOWN {
                self.meters[idx].tripped = false;
                self.meters[idx].calls = 0;
                self.meters[idx].trip_rejected = 0;
                self.cools = self.cools.saturating_add(1);
                return Ok(());
            }
            return Err(SysBoundError::BreakerTripped);
        }
        Ok(())
    }

    /// 记一次通过闸的调用（计数 +1；达阈值即熔断——本次调用仍正常处置）。
    pub fn count_call(&mut self, script: u32) -> Result<(), SysBoundError> {
        let idx = self.meter_for(script)?;
        self.meters[idx].calls = self.meters[idx].calls.saturating_add(1);
        if self.meters[idx].calls >= CALL_BREAKER_THRESHOLD {
            if !self.meters[idx].tripped {
                self.meters[idx].tripped = true;
                self.meters[idx].trip_rejected = 0;
                self.trips = self.trips.saturating_add(1);
            }
        }
        Ok(())
    }

    /// 账自检：环有界 / 流水覆盖一致 / 计量器有界。
    pub fn audit(&self) -> Result<(), SysBoundError> {
        if self.ring.len() != AUDIT_RING_CAP || self.meters.len() > METER_CAP {
            return Err(SysBoundError::State);
        }
        if self.filled > AUDIT_RING_CAP {
            return Err(SysBoundError::State);
        }
        let mut i = 0;
        while i < self.meters.len() {
            if self.meters[i].script == 0 || self.meters[i].script > SCRIPT_ID_MAX {
                return Err(SysBoundError::State);
            }
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 十一、有界事件账（满即拒收——不静默丢事件）
// ---------------------------------------------------------------------------

/// 事件账容量（128 条——满即拒收待清）。
pub const LEDGER_CAP: usize = 128;

/// 事件种类闭集（19 种——跨界生命周期的每一步都有名有姓）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 关系申报安于本位。
    QuadAtHome,
    /// 边界混淆引导归位。
    QuadGuided,
    /// 预留私启拦截。
    QuadIntercepted,
    /// 关系失配对拍修正。
    QuadMismatch,
    /// 白名单注册。
    WhitelistRegistered,
    /// 白名单重复注册拒收。
    WhitelistDup,
    /// 跨界调用成功。
    CallOk,
    /// 越白名单调用拦截（注入面一）。
    CallOutbound,
    /// 签名失配拦截。
    CallSigMismatch,
    /// 类型不匹配拦截。
    CallTypeMismatch,
    /// 句柄发放。
    HandleIssued,
    /// 句柄转移。
    HandleTransferred,
    /// 句柄到期回收。
    HandleRecycled,
    /// 句柄滥用拦截（注入面二）。
    HandleAbuse,
    /// 授权签发。
    WriteGranted,
    /// 授权核销（写操作成功）。
    WriteSpent,
    /// 未授权写拦截（注入面三）。
    WriteDenied,
    /// 熔断触发。
    BreakerTripped,
    /// 熔断冷却复路。
    BreakerCooled,
}

impl EventKind {
    /// 事件名（人读——读屏可达）。
    pub const fn name(self) -> &'static str {
        match self {
            EventKind::QuadAtHome => "关系安位",
            EventKind::QuadGuided => "边界混淆引导归位",
            EventKind::QuadIntercepted => "预留私启拦截",
            EventKind::QuadMismatch => "关系失配对拍修正",
            EventKind::WhitelistRegistered => "白名单注册",
            EventKind::WhitelistDup => "白名单重复注册拒收",
            EventKind::CallOk => "跨界调用成功",
            EventKind::CallOutbound => "越白名单调用拦截",
            EventKind::CallSigMismatch => "签名失配拦截",
            EventKind::CallTypeMismatch => "类型不匹配拦截",
            EventKind::HandleIssued => "句柄发放",
            EventKind::HandleTransferred => "句柄转移",
            EventKind::HandleRecycled => "句柄到期回收",
            EventKind::HandleAbuse => "句柄滥用拦截",
            EventKind::WriteGranted => "授权签发",
            EventKind::WriteSpent => "授权核销",
            EventKind::WriteDenied => "未授权写拦截",
            EventKind::BreakerTripped => "熔断触发",
            EventKind::BreakerCooled => "熔断冷却复路",
        }
    }
}

/// 事件条目（流水号 / 种类 / 目标 id 可选 / 人话备注）。
#[derive(Clone, Debug)]
pub struct Event {
    /// 流水号（账序单调）。
    pub seq: u64,
    /// 种类。
    pub kind: EventKind,
    /// 目标 id（句柄 id / 槽位 / 脚本 id——视种类而定）。
    pub target: Option<u32>,
    /// 人话备注（非空——错误友好律对账本自身的约束）。
    pub note: String,
}

/// 事件账（满即拒收——不静默丢事件；drain 清账复路）。
pub struct EventLedger {
    /// 账目（有界 Vec）。
    pub entries: Vec<Event>,
    /// 下一流水号。
    next_seq: u64,
    /// 满拒收计数。
    pub full_rejected: u32,
}

impl EventLedger {
    /// 开机空账。
    pub fn new() -> EventLedger {
        EventLedger {
            entries: Vec::new(),
            next_seq: 1,
            full_rejected: 0,
        }
    }

    /// 记事件（备注非空强制——空备注拒收 ParamBad；账满拒收 LedgerFull）。
    pub fn record(
        &mut self,
        kind: EventKind,
        target: Option<u32>,
        note: &str,
    ) -> Result<(), SysBoundError> {
        if note.is_empty() {
            return Err(SysBoundError::ParamBad);
        }
        if self.entries.len() >= LEDGER_CAP {
            self.full_rejected = self.full_rejected.saturating_add(1);
            return Err(SysBoundError::LedgerFull);
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.entries.push(Event {
            seq,
            kind,
            target,
            note: note.to_string(),
        });
        Ok(())
    }

    /// 账长。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 清账取走（流水号不回退——账序单调）。
    pub fn drain(&mut self) -> Vec<Event> {
        let out = core::mem::replace(&mut self.entries, Vec::new());
        out
    }
}

// ---------------------------------------------------------------------------
// 十二、世界体：开机种子 + 跨界调用全管线 + 体检 + 确定性摘要
// ---------------------------------------------------------------------------

/// 跨界调用回执（成功路径的返回面——流水号 / 槽位 / 指纹 / 返回值）。
#[derive(Clone, Debug)]
pub struct CallReceipt {
    /// 审计流水号（与环形账条目同号——对账键）。
    pub seq: u64,
    /// 调用方脚本 id。
    pub caller: u32,
    /// 被调面槽位。
    pub api_slot: u32,
    /// 被调面签名指纹。
    pub sig_fingerprint: u64,
    /// 返回值（规范值：数→0 / 串→"ok" / 句柄→新发句柄 / 视图→新发视图）。
    pub ret: SysValue,
}

/// 体检报告（一键体检面——计数自洽即健康）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealthReport {
    /// 白名单长度。
    pub whitelist_len: u32,
    /// 通过熔断闸的调用总数。
    pub calls_total: u64,
    /// 成功调用数。
    pub calls_ok: u64,
    /// 白名单外拦截数（注入面一）。
    pub inject_outbound: u32,
    /// 签名失配拦截数。
    pub sig_mismatch: u32,
    /// 类型不匹配拦截数。
    pub type_mismatch: u32,
    /// 句柄滥用拦截数（注入面二，含句柄表侧计数）。
    pub inject_handle: u32,
    /// 未授权写拦截数（注入面三）。
    pub inject_write: u32,
    /// 结构性注入兜底数。
    pub inject_struct: u32,
    /// 在管授权位数。
    pub grants_active: u32,
    /// 授权签发总数。
    pub write_granted: u32,
    /// 授权核销总数。
    pub write_spent: u32,
    /// 当前熔断中的脚本数。
    pub breaker_tripped: u32,
    /// 熔断触发累计。
    pub breaker_trips: u32,
    /// 关系安位数。
    pub quad_at_home: u32,
    /// 引导归位数。
    pub quad_guided: u32,
    /// 预留私启拦截数。
    pub quad_intercepted: u32,
    /// 对拍修正数。
    pub quad_mismatch: u32,
    /// 存活句柄数。
    pub handles_live: u32,
    /// 事件账长。
    pub ledger_len: u32,
}

impl HealthReport {
    /// 健康判据：调用计数自洽 + 各账有界（违一即不健康）。
    pub fn is_healthy(&self) -> bool {
        if self.calls_ok > self.calls_total {
            return false;
        }
        if self.whitelist_len > API_CAP as u32 {
            return false;
        }
        if self.ledger_len > LEDGER_CAP as u32 {
            return false;
        }
        if self.grants_active > GRANT_CAP as u32 {
            return false;
        }
        true
    }
}

/// 系统边界世界体（四定闸 + 互操作面 + 句柄表 + 授权账 + 审计 + 事件账
/// 六件一体——VE-F5004 的组装面）。
pub struct SysBoundWorld {
    /// 关系四定闸。
    pub quad: QuadGate,
    /// 互操作面（白名单）。
    pub surface: InteropSurface,
    /// 句柄表。
    pub handles: HandleTable,
    /// 授权账。
    pub auth: AuthLedger,
    /// 边界审计。
    pub audit_log: BoundaryAudit,
    /// 事件账。
    pub ledger: EventLedger,
    /// 越白名单调用注入计数（注入面一）。
    pub inject_outbound: u32,
    /// 句柄滥用注入计数（注入面二）。
    pub inject_handle: u32,
    /// 未授权写注入计数（注入面三）。
    pub inject_write: u32,
    /// 结构性注入兜底计数。
    pub inject_struct: u32,
    /// 成功调用数。
    pub calls_ok: u64,
    /// 签名失配拦截数。
    pub sig_mismatch: u32,
    /// 类型不匹配拦截数。
    pub type_mismatch: u32,
    /// 世界操作流水（单调——句柄/授权盖章用）。
    pub ops: u64,
}

impl SysBoundWorld {
    /// 开机：六件套空体 + 按 [`CANONICAL_APIS`] 灌规范白名单种子。
    ///
    /// 种子注册失败在开机路径即显性上抛（不可达：编译期断言已保证种子
    /// 名/签名合法且不撞禁入面——失败即不变量破，开机失败优于带病运行）。
    pub fn boot() -> Result<SysBoundWorld, SysBoundError> {
        let mut w = SysBoundWorld {
            quad: QuadGate::new(),
            surface: InteropSurface::new(),
            handles: HandleTable::new(),
            auth: AuthLedger::new(),
            audit_log: BoundaryAudit::new(),
            ledger: EventLedger::new(),
            inject_outbound: 0,
            inject_handle: 0,
            inject_write: 0,
            inject_struct: 0,
            calls_ok: 0,
            sig_mismatch: 0,
            type_mismatch: 0,
            ops: 0,
        };
        let mut i = 0;
        while i < CANONICAL_APIS.len() {
            let a = &CANONICAL_APIS[i];
            let sig = match SigSpec::from_parts(a.args, a.argn, a.ret) {
                Some(s) => s,
                None => return Err(SysBoundError::State),
            };
            let slot = w.surface.register(a.name, a.domain, a.kind, sig)?;
            let note = w.ledger_note_seed(a.name, a.domain);
            w.ledger.record(EventKind::WhitelistRegistered, Some(slot), &note)?;
            i += 1;
        }
        Ok(w)
    }

    /// 种子注册事件备注（开机路径专用——名字与域名人话拼接）。
    fn ledger_note_seed(&self, name: &str, domain: SysDomain) -> String {
        format!("注册规范 API「{}」至{}白名单", name, domain.name())
    }

    /// 推进世界流水（每个状态变更操作 +1——确定性对拍的时序骨）。
    fn tick(&mut self) -> u64 {
        self.ops = self.ops.saturating_add(1);
        self.handles.op_seq = self.ops;
        self.auth.op_seq = self.ops;
        self.ops
    }

    // -- 关系四定 ---------------------------------------------------------

    /// 关系申报（四定闸判读 + 事件账留痕）。
    pub fn declare_relation(
        &mut self,
        capability: &str,
        declared: RelationKind,
        facts: RelationFacts,
    ) -> Result<QuadReceipt, SysBoundError> {
        let r = self.quad.declare(capability, declared, facts)?;
        self.tick();
        let (kind, note) = match r.outcome {
            QuadOutcome::AtHome => (
                EventKind::QuadAtHome,
                format!("能力「{}」申报{}——安于本位", capability, declared.name()),
            ),
            QuadOutcome::GuidedHome(to) => (
                EventKind::QuadGuided,
                format!(
                    "能力「{}」申报{}——边界混淆，引导归位：{}",
                    capability,
                    declared.name(),
                    to.guide_hint()
                ),
            ),
            QuadOutcome::Intercepted => (
                EventKind::QuadIntercepted,
                format!("能力「{}」申报{}——预留私启，拦截", capability, declared.name()),
            ),
        };
        self.ledger.record(kind, None, &note)?;
        if r.mismatch {
            self.ledger.record(
                EventKind::QuadMismatch,
                None,
                &format!(
                    "关系失配对拍修正：能力「{}」判读结论变位（先判后改，账留痕）",
                    capability
                ),
            )?;
        }
        Ok(r)
    }

    // -- 白名单 -----------------------------------------------------------

    /// 白名单注册（互操作面注册 + 事件留痕；重复注册拒收留痕后原错误上抛）。
    pub fn register_api(
        &mut self,
        name: &str,
        domain: SysDomain,
        kind: ApiKind,
        sig: SigSpec,
    ) -> Result<u32, SysBoundError> {
        let slot = match self.surface.register(name, domain, kind, sig) {
            Ok(s) => s,
            Err(SysBoundError::ApiDup) => {
                self.ledger.record(
                    EventKind::WhitelistDup,
                    None,
                    &format!("白名单重复注册拒收：API「{}」已在册，升级改签名请走新名", name),
                )?;
                return Err(SysBoundError::ApiDup);
            }
            Err(e) => return Err(e),
        };
        self.tick();
        self.ledger.record(
            EventKind::WhitelistRegistered,
            Some(slot),
            &format!("注册 API「{}」至{}（{}）", name, domain.name(), kind.name()),
        )?;
        Ok(slot)
    }

    // -- 授权 -------------------------------------------------------------

    /// 宿主签发写授权位（安全默认律：写操作默认拒绝，签发是唯一入口）。
    pub fn grant_write(&mut self, script: u32, api_slot: u32, budget: u32) -> Result<(), SysBoundError> {
        if script == 0 || script > SCRIPT_ID_MAX {
            return Err(SysBoundError::ParamBad);
        }
        let entry = match self.surface.entry(api_slot) {
            Some(e) => e,
            None => return Err(SysBoundError::ApiNotFound),
        };
        let kind = entry.kind;
        let name = entry.name.clone();
        self.tick();
        self.auth.grant(script, api_slot, budget, kind)?;
        self.ledger.record(
            EventKind::WriteGranted,
            Some(api_slot),
            &format!(
                "签发写授权位：脚本#{} ×「{}」×预算{}",
                script, name, budget
            ),
        )?;
        Ok(())
    }

    // -- 句柄 -------------------------------------------------------------

    /// 句柄转移（一次性：借出 → 已转移；事件留痕）。
    pub fn transfer_handle(&mut self, id: u32, from: u32, to: u32) -> Result<(), SysBoundError> {
        self.tick();
        match self.handles.transfer(id, from, to) {
            Ok(()) => {
                self.ledger.record(
                    EventKind::HandleTransferred,
                    Some(id),
                    &format!("句柄#{} 由脚本#{} 转移至脚本#{}（一次性）", id, from, to),
                )?;
                Ok(())
            }
            Err(e) => {
                self.note_handle_rejection(e)?;
                Err(e)
            }
        }
    }

    /// 句柄到期回收（借出/已转移 → 回收；回收后永久封闭）。
    pub fn recycle_handle(&mut self, id: u32, by: u32) -> Result<(), SysBoundError> {
        self.tick();
        match self.handles.recycle(id, by) {
            Ok(()) => {
                self.ledger.record(
                    EventKind::HandleRecycled,
                    Some(id),
                    &format!("句柄#{} 到期回收（id 永久封闭不复用）", id),
                )?;
                Ok(())
            }
            Err(e) => {
                self.note_handle_rejection(e)?;
                Err(e)
            }
        }
    }

    /// 句柄拒绝的统一留痕（滥用计数 + 注入面二计数 + 事件账）。
    fn note_handle_rejection(&mut self, e: SysBoundError) -> Result<(), SysBoundError> {
        match e {
            SysBoundError::HandleBad => {
                self.inject_handle = self.inject_handle.saturating_add(1);
                self.ledger.record(
                    EventKind::HandleAbuse,
                    None,
                    "句柄滥用拦截：查句柄当前态，回收后的句柄永久封闭须重新获取",
                )?;
            }
            SysBoundError::Inject => {
                self.inject_struct = self.inject_struct.saturating_add(1);
                self.ledger.record(
                    EventKind::HandleAbuse,
                    None,
                    "结构性注入兜底：句柄表触达不可能状态，已立案",
                )?;
            }
            _ => {}
        }
        Ok(())
    }

    /// 调用计数 + 熔断触发留痕（成功路径与拒绝路径同口径——触发即记事件）。
    ///
    /// 返回 Ok(true) 表示本次计数恰达阈值（熔断刚触发，本次调用仍正常
    /// 处置）；Ok(false) 为常态。
    fn count_and_note_trip(&mut self, caller: u32, near_api: &str) -> Result<bool, SysBoundError> {
        self.audit_log.count_call(caller)?;
        let idx = self.audit_log.meter_for(caller)?;
        let m = self.audit_log.meters[idx];
        if m.tripped && m.calls == CALL_BREAKER_THRESHOLD {
            self.ledger.record(
                EventKind::BreakerTripped,
                Some(caller),
                &format!(
                    "熔断触发：脚本#{} 调用计数达 {}（近调「{}」），后续拒唤 {} 次后冷却复路",
                    caller, CALL_BREAKER_THRESHOLD, near_api, BREAKER_COOLDOWN
                ),
            )?;
            return Ok(true);
        }
        Ok(false)
    }

    // -- 跨界调用全管线 ---------------------------------------------------

    /// 跨界调用全管线（边界 O(1) 判据的主通道）：
    ///
    /// 1. 调用方 id 合法性（1..=4096）；
    /// 2. 熔断闸（熔断中拒唤；冷却到点自动复路并放行本次）；
    /// 3. 白名单查名（查无 → **越界调用注入**）；
    /// 4. 签名 arity 比对（不符 → 签名失配）；
    /// 5. 类型闭集逐位对型（不对 → 类型不匹配——拒收不转换）；
    /// 6. 句柄实参生命周期校验（滥用 → **句柄滥用注入**）；
    /// 7. Effect 面授权位检（无 → **未授权写注入**；预算零 → 授权耗尽）；
    /// 8. 熔断计数（达阈值即熔断——本次仍正常处置）；
    /// 9. 审计入账（成败都记）+ 事件留痕 + 规范返回值。
    ///
    /// 无论成败：通过闸的调用一律计入计量器与环形审计账（成功路径之外
    /// 的拒绝路径各自留事件）。
    pub fn call_api(
        &mut self,
        caller: u32,
        name: &str,
        args: &[SysValue],
    ) -> Result<CallReceipt, SysBoundError> {
        if caller == 0 || caller > SCRIPT_ID_MAX {
            return Err(SysBoundError::ParamBad);
        }
        // 2. 熔断闸（熔断中拒唤；冷却到点自动复路并放行本次）。
        //    拒唤不入环形账（保持 ∑计量器计数 == 入账总数的不变量）——
        //    拒唤轨迹由计量器 rejected 计数承载。
        let cools_before = self.audit_log.cools;
        if let Err(e) = self.audit_log.gate(caller) {
            return Err(e);
        }
        if self.audit_log.cools != cools_before {
            self.ledger.record(
                EventKind::BreakerCooled,
                Some(caller),
                &format!(
                    "熔断冷却复路：脚本#{} 拒唤满 {} 次，计数清零重开",
                    caller, BREAKER_COOLDOWN
                ),
            )?;
        }
        // 3. 白名单查名。
        let slot = match self.surface.lookup(name) {
            Some(s) => s,
            None => {
                self.tick();
                self.count_and_note_trip(caller, name)?;
                self.audit_log.push(caller, 0, fp_str(FNV_SEED, name), false);
                self.inject_outbound = self.inject_outbound.saturating_add(1);
                self.ledger.record(
                    EventKind::CallOutbound,
                    None,
                    "越白名单调用拦截：只许调注册过的面，新能力先走关系四定申报再注册",
                )?;
                return Err(SysBoundError::ApiNotFound);
            }
        };
        // 取签名快照（Copy——避开与后续 &mut 冲突）。
        let (sig, kind, domain, fp, api_name) = {
            let e = &self.surface.entries[slot as usize];
            (e.sig, e.kind, e.domain, e.fingerprint, e.name.clone())
        };
        // 4. 签名 arity。
        if args.len() as u8 != sig.argn {
            self.tick();
            self.count_and_note_trip(caller, &api_name)?;
            self.audit_log.push(caller, slot, 0, false);
            self.sig_mismatch = self.sig_mismatch.saturating_add(1);
            self.ledger.record(
                EventKind::CallSigMismatch,
                Some(slot),
                &format!(
                    "签名失配拦截：API「{}」注册 {} 参，实到 {} 参",
                    api_name, sig.argn, args.len()
                ),
            )?;
            return Err(SysBoundError::SigMismatch);
        }
        // 5. 类型闭集逐位对型。
        {
            let mut i = 0;
            while i < args.len() {
                if args[i].tag() != sig.args[i].tag() {
                    self.tick();
                    self.count_and_note_trip(caller, &api_name)?;
                    self.audit_log.push(caller, slot, 0, false);
                    self.type_mismatch = self.type_mismatch.saturating_add(1);
                    self.ledger.record(
                        EventKind::CallTypeMismatch,
                        Some(slot),
                        &format!(
                            "类型不匹配拦截：API「{}」第{}参应为{}，实到型序数 {}——拒收不转换",
                            api_name,
                            i + 1,
                            sig.args[i].name(),
                            args[i].tag()
                        ),
                    )?;
                    return Err(SysBoundError::TypeMismatch);
                }
                i += 1;
            }
        }
        // 6. 句柄实参生命周期校验。
        {
            let mut i = 0;
            while i < args.len() {
                let want_view = sig.args[i].tag() == 4;
                if let Some(id) = args[i].handle_id() {
                    if let Err(e) = self.handles.validate_arg(id, want_view) {
                        self.tick();
                        self.count_and_note_trip(caller, &api_name)?;
                        self.audit_log.push(caller, slot, 0, false);
                        self.note_handle_rejection(e)?;
                        return Err(e);
                    }
                }
                i += 1;
            }
        }
        // 7. Effect 面授权位检。
        let mut grant_idx: Option<usize> = None;
        if kind.ord() == 2 {
            match self.auth.find(caller, slot) {
                Some(idx) => {
                    if self.auth.grants[idx].budget == 0 {
                        self.tick();
                        self.count_and_note_trip(caller, &api_name)?;
                        self.audit_log.push(caller, slot, 0, false);
                        self.inject_write = self.inject_write.saturating_add(1);
                        self.ledger.record(
                            EventKind::WriteDenied,
                            Some(slot),
                            "写授权拦截：授权位在但预算为零，请宿主重新签发，不作静默续期",
                        )?;
                        return Err(SysBoundError::AuthSpent);
                    }
                    grant_idx = Some(idx);
                }
                None => {
                    self.tick();
                    self.count_and_note_trip(caller, &api_name)?;
                    self.audit_log.push(caller, slot, 0, false);
                    self.inject_write = self.inject_write.saturating_add(1);
                    self.ledger.record(
                        EventKind::WriteDenied,
                        Some(slot),
                        "未授权写拦截：Effect 面默认拒绝（安全默认律），由宿主先签发授权位再调",
                    )?;
                    return Err(SysBoundError::AuthRequired);
                }
            }
        }
        // 8. 熔断计数（达阈值即熔断——本次仍正常处置，触发已留痕）。
        let _tripped_now = self.count_and_note_trip(caller, &api_name)?;
        // 9. 核销授权（Effect 成功路径）。
        if let Some(idx) = grant_idx {
            self.auth.spend(idx);
        }
        // 规范返回值（数→0 / 串→"ok" / 句柄→新发 / 视图→新发视图）。
        let ret = match sig.ret {
            ArgType::Number => SysValue::Num(0),
            ArgType::Str => SysValue::Str("ok".to_string()),
            ArgType::Handle | ArgType::View => {
                let is_view = sig.ret.tag() == 4;
                let id = self.handles.acquire(caller, domain, is_view)?;
                if is_view {
                    SysValue::View(id)
                } else {
                    SysValue::Hnd(id)
                }
            }
        };
        self.tick();
        self.calls_ok = self.calls_ok.saturating_add(1);
        let digest = fp_args(FNV_SEED, args);
        let seq = self.audit_log.next_seq;
        self.audit_log.push(caller, slot, digest, true);
        // 事件留痕（核销先于发放留痕——账序即处置序）。
        if grant_idx.is_some() {
            self.ledger.record(
                EventKind::WriteSpent,
                Some(slot),
                &format!("写授权核销：脚本#{} 调用「{}」成功", caller, api_name),
            )?;
        }
        if let Some(id) = ret.handle_id() {
            self.ledger.record(
                EventKind::HandleIssued,
                Some(id),
                &format!(
                    "句柄发放：调用「{}」向脚本#{} 发放 {}#{}",
                    api_name,
                    caller,
                    if sig.ret.tag() == 4 { "只读视图" } else { "句柄" },
                    id
                ),
            )?;
        }
        self.ledger.record(
            EventKind::CallOk,
            Some(slot),
            &format!("跨界调用成功：脚本#{} →「{}」", caller, api_name),
        )?;
        Ok(CallReceipt {
            seq,
            caller,
            api_slot: slot,
            sig_fingerprint: fp,
            ret,
        })
    }

    // -- 体检 / 审计 / 摘要 / 读屏 ----------------------------------------

    /// 一键体检（计数快照——健康判据见 [`HealthReport::is_healthy`]）。
    pub fn health(&self) -> HealthReport {
        let mut breaker_tripped = 0u32;
        let mut i = 0;
        while i < self.audit_log.meters.len() {
            if self.audit_log.meters[i].tripped {
                breaker_tripped = breaker_tripped.saturating_add(1);
            }
            i += 1;
        }
        HealthReport {
            whitelist_len: self.surface.entries.len() as u32,
            calls_total: self.audit_log.total,
            calls_ok: self.calls_ok,
            inject_outbound: self.inject_outbound,
            sig_mismatch: self.sig_mismatch,
            type_mismatch: self.type_mismatch,
            inject_handle: self.inject_handle.saturating_add(self.handles.abuse_rejected),
            inject_write: self.inject_write,
            inject_struct: self.inject_struct,
            grants_active: self.auth.active(),
            write_granted: self.auth.issued_total,
            write_spent: self.auth.spent_total,
            breaker_tripped,
            breaker_trips: self.audit_log.trips,
            quad_at_home: self.quad.at_home,
            quad_guided: self.quad.guided,
            quad_intercepted: self.quad.intercepted,
            quad_mismatch: self.quad.mismatched,
            handles_live: self.handles.live(),
            ledger_len: self.ledger.len() as u32,
        }
    }

    /// 不变量审计（六件套各自自检 + 计量器总量对账——违一即 State）。
    pub fn audit(&self) -> Result<(), SysBoundError> {
        self.quad.audit()?;
        self.surface.verify_all()?;
        self.handles.audit()?;
        self.auth.audit()?;
        self.audit_log.audit()?;
        if self.ledger.len() > LEDGER_CAP {
            return Err(SysBoundError::State);
        }
        // 计量器总量对账：∑calls == 通过闸调用总数。
        let mut sum = 0u64;
        let mut i = 0;
        while i < self.audit_log.meters.len() {
            sum = sum.saturating_add(self.audit_log.meters[i].calls);
            i += 1;
        }
        if sum != self.audit_log.total {
            return Err(SysBoundError::State);
        }
        Ok(())
    }

    /// 全状态 FNV-1a 64 确定性摘要（双跑对拍键——同序列必等）。
    ///
    /// 覆盖面：白名单（名/指纹/域/种/签名）+ 句柄表全槽态 + 授权位与
    /// 留痕账 + 环形审计账 + 计量器 + 四定申报账 + 六件套计数器 + 事件账。
    pub fn state_digest(&self) -> u64 {
        let mut h = fp_str(FNV_SEED, SYS_BOUND_VERSION);
        // 白名单。
        h = fp_str(h, "surface");
        h = fp_u32(h, self.surface.entries.len() as u32);
        let mut i = 0;
        while i < self.surface.entries.len() {
            let e = &self.surface.entries[i];
            h = fp_str(h, &e.name);
            h = fp_u64(h, e.fingerprint);
            h = fp_u32(h, e.domain.ord() as u32);
            h = fp_u32(h, e.kind.ord() as u32);
            h = fp_u32(h, e.sig.argn as u32);
            h = fp_u32(h, e.sig.ret.tag() as u32);
            i += 1;
        }
        // 句柄表。
        h = fp_str(h, "handles");
        h = fp_u32(h, self.handles.issued);
        h = fp_u32(h, self.handles.recycled_total);
        h = fp_u32(h, self.handles.transferred_total);
        let mut i = 0;
        while i < self.handles.issued as usize {
            let r = self.handles.slots[i];
            h = fp_u32(h, r.state.ord() as u32);
            h = fp_u32(h, r.owner);
            h = fp_u32(h, r.domain.ord() as u32);
            h = fp_u32(h, r.is_view as u32);
            h = fp_u64(h, r.born_seq);
            i += 1;
        }
        // 授权。
        h = fp_str(h, "auth");
        h = fp_u32(h, self.auth.issued_total);
        h = fp_u32(h, self.auth.spent_total);
        h = fp_u32(h, self.auth.grants.len() as u32);
        let mut i = 0;
        while i < self.auth.grants.len() {
            let g = self.auth.grants[i];
            h = fp_u32(h, g.api_slot);
            h = fp_u32(h, g.script);
            h = fp_u32(h, g.budget);
            h = fp_u64(h, g.granted_seq);
            i += 1;
        }
        let mut i = 0;
        while i < self.auth.ledger.len() {
            let g = self.auth.ledger[i];
            h = fp_u32(h, g.api_slot);
            h = fp_u32(h, g.script);
            h = fp_u32(h, g.budget_at_grant);
            h = fp_u64(h, g.seq);
            i += 1;
        }
        // 审计环。
        h = fp_str(h, "audit");
        h = fp_u64(h, self.audit_log.total);
        let ring = self.audit_log.iter_oldest_first();
        let mut i = 0;
        while i < ring.len() {
            let a = ring[i];
            h = fp_u64(h, a.seq);
            h = fp_u32(h, a.caller);
            h = fp_u32(h, a.api_slot);
            h = fp_u64(h, a.digest);
            h = fp_u32(h, a.ok as u32);
            i += 1;
        }
        // 计量器。
        h = fp_str(h, "meters");
        let mut i = 0;
        while i < self.audit_log.meters.len() {
            let m = self.audit_log.meters[i];
            h = fp_u32(h, m.script);
            h = fp_u64(h, m.calls);
            h = fp_u64(h, m.rejected);
            h = fp_u32(h, m.tripped as u32);
            h = fp_u32(h, m.trip_rejected);
            i += 1;
        }
        // 四定账。
        h = fp_str(h, "quad");
        h = fp_u32(h, self.quad.declared_total);
        h = fp_u32(h, self.quad.at_home);
        h = fp_u32(h, self.quad.guided);
        h = fp_u32(h, self.quad.intercepted);
        h = fp_u32(h, self.quad.mismatched);
        let mut i = 0;
        while i < self.quad.records.len() {
            let r = self.quad.records[i];
            h = fp_u64(h, r.capability_fp);
            h = fp_u32(h, r.declared_ord as u32);
            h = fp_u32(h, r.outcome_code as u32);
            h = fp_u32(h, r.guide_to_ord as u32);
            h = fp_u64(h, r.facts_fp);
            i += 1;
        }
        // 计数器。
        h = fp_str(h, "counters");
        h = fp_u32(h, self.inject_outbound);
        h = fp_u32(h, self.inject_handle);
        h = fp_u32(h, self.inject_write);
        h = fp_u32(h, self.inject_struct);
        h = fp_u32(h, self.sig_mismatch);
        h = fp_u32(h, self.type_mismatch);
        h = fp_u64(h, self.calls_ok);
        h = fp_u64(h, self.ops);
        // 事件账。
        h = fp_str(h, "ledger");
        let mut i = 0;
        while i < self.ledger.entries.len() {
            let e = self.ledger.entries[i];
            h = fp_u64(h, e.seq);
            let k: u8 = match e.kind {
                EventKind::QuadAtHome => 0,
                EventKind::QuadGuided => 1,
                EventKind::QuadIntercepted => 2,
                EventKind::QuadMismatch => 3,
                EventKind::WhitelistRegistered => 4,
                EventKind::WhitelistDup => 5,
                EventKind::CallOk => 6,
                EventKind::CallOutbound => 7,
                EventKind::CallSigMismatch => 8,
                EventKind::CallTypeMismatch => 9,
                EventKind::HandleIssued => 10,
                EventKind::HandleTransferred => 11,
                EventKind::HandleRecycled => 12,
                EventKind::HandleAbuse => 13,
                EventKind::WriteGranted => 14,
                EventKind::WriteSpent => 15,
                EventKind::WriteDenied => 16,
                EventKind::BreakerTripped => 17,
                EventKind::BreakerCooled => 18,
            };
            h = fp_u32(h, k as u32);
            h = fp_u32(h, e.target.unwrap_or(0));
            h = fp_str(h, &e.note);
            i += 1;
        }
        h
    }

    /// 读屏可达状态行（8 行人话——边界文档读屏可达的运行期面）。
    pub fn spoken_status(&self) -> Vec<String> {
        let h = self.health();
        vec![
            format!("白名单：{} 条规范面在册（上限 {}）", h.whitelist_len, API_CAP),
            format!(
                "跨界调用：共 {} 次，成功 {} 次，拒绝 {} 次",
                h.calls_total,
                h.calls_ok,
                h.calls_total - h.calls_ok
            ),
            format!(
                "注入检出：越白名单 {} / 句柄滥用 {} / 未授权写 {} / 结构兜底 {}",
                h.inject_outbound, h.inject_handle, h.inject_write, h.inject_struct
            ),
            format!(
                "写授权：在管 {} 位，累计签发 {}，核销 {}",
                h.grants_active, h.write_granted, h.write_spent
            ),
            format!(
                "熔断：当前熔断 {} 个脚本，累计触发 {} 次",
                h.breaker_tripped, h.breaker_trips
            ),
            format!(
                "关系四定：安位 {} / 引导 {} / 拦截 {} / 对拍修正 {}",
                h.quad_at_home, h.quad_guided, h.quad_intercepted, h.quad_mismatch
            ),
            format!(
                "句柄：存活 {}（发放-回收口径），回收即永久封闭",
                h.handles_live
            ),
            format!("事件账：{} 条在账（上限 {}，满即拒收待清）", h.ledger_len, LEDGER_CAP),
        ]
    }
}

/// 参数摘要指纹（FNV-1a 64——只入指纹不入原文，审计隐私面）。
fn fp_args(seed: u64, args: &[SysValue]) -> u64 {
    let mut h = fp_u32(seed, args.len() as u32);
    let mut i = 0;
    while i < args.len() {
        h = args[i].fp_into(h);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 十三、编译期断言（const 纪律自证——判据在编译期先过关）
// ---------------------------------------------------------------------------

const _: () = assert!(ERR_CODE_BASE == 0x1D50);
const _: () = assert!(ERR_CODE_END == 0x1D5F);
const _: () = assert!(ERR_CODE_COUNT == 16);
const _: () = assert!(CODES.len() == 16);

/// 诊断码段连续互斥：0x1D50..=0x1D5F 恰 16 码无缺口无重号。
const _: () = {
    let mut ok = true;
    let mut i = 0;
    while i + 1 < CODES.len() {
        if CODES[i].wrapping_add(1) != CODES[i + 1] {
            ok = false;
        }
        i += 1;
    }
    if CODES[0] != ERR_CODE_BASE {
        ok = false;
    }
    if CODES[CODES.len() - 1] != ERR_CODE_END {
        ok = false;
    }
    assert!(ok);
};

/// 四定卡闭集覆盖：四卡各对一条关系、序数 1..=4 不重不漏、三段齐备。
const _: () = {
    let mut ok = true;
    let mut i = 0;
    while i < QUAD_CARDS.len() {
        let c = &QUAD_CARDS[i];
        if c.relation.ord() != (i + 1) as u8 {
            ok = false;
        }
        if c.charter.len() == 0 || c.reading.len() == 0 || c.violation.len() == 0 {
            ok = false;
        }
        i += 1;
    }
    assert!(ok);
};

/// 四域边界卡三段齐备：可调用面与禁入面非空、协议非空、域序数对位。
const _: () = {
    let mut ok = true;
    let mut i = 0;
    while i < BOUNDARY_CARDS.len() {
        let c = &BOUNDARY_CARDS[i];
        if c.domain.ord() != (i + 1) as u8 {
            ok = false;
        }
        if c.callable.len() == 0 || c.forbidden.len() == 0 || c.protocol.len() == 0 {
            ok = false;
        }
        let mut j = 0;
        while j < c.forbidden.len() {
            if card_has_callable(c, c.forbidden[j]) {
                ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    assert!(ok);
};

/// 规范 API 对齐断言：每条种子的名字都落在所属域卡可调用面名单内、
/// 签名参数个数合法、Effect 面种子确为效果类（白名单与边界清单结构性
/// 对齐——不是两套皮）。
const _: () = {
    let mut ok = true;
    let mut i = 0;
    while i < CANONICAL_APIS.len() {
        let a = &CANONICAL_APIS[i];
        let card = &BOUNDARY_CARDS[a.domain.card_index()];
        if !card_has_callable(card, a.name) {
            ok = false;
        }
        if a.argn as usize > SIG_ARGS_MAX {
            ok = false;
        }
        if card_forbids(a.domain, a.name) {
            ok = false;
        }
        i += 1;
    }
    assert!(ok);
};

/// 协议闭集对位：值闭集四型与参数型闭集四型序数一一对位。
const _: () = {
    let mut ok = true;
    let mut i = 1;
    while i <= 4 {
        let mut found = false;
        let mut j = 0;
        while j < SIG_ARGS_MAX {
            let probe: [ArgType; SIG_ARGS_MAX] = [
                ArgType::Number,
                ArgType::Str,
                ArgType::Handle,
                ArgType::View,
            ];
            if probe[j].tag() == i {
                found = true;
            }
            j += 1;
        }
        if !found {
            ok = false;
        }
        i += 1;
    }
    assert!(ok);
};
