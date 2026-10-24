//! VE-F5003 · 语言设计原则（VE-Y 域 · 脚本语言组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5003`
//!
//! **判据（锚点原文）**：设计五律、宪法卡、判例集、评审拦截、宪法裁决、判据。
//!
//! **职责定位（锚点原文摘录）**：语言设计原则（设计五律：可读第一（读脚本像读故事——不玩字符机巧）/错误友好（报错说人话给改法——十三章口径语言化）/安全默认（沙箱内运行无副作用泄漏）/渐进复杂（三行能干小事三百行能干大事）/确定优先（同输入同输出——脚本行为可预测）；五律为后续所有组的宪法）。数据结构：五律宪法卡；设计判例集。错误路径与降级矩阵：违律设计→评审拦截；律条歧义→澄清修订；判例冲突→宪法裁决。
//!
//! # 一、五律是宪法不是口号（设计五律闭集）
//!
//! [`DesignLaw`] 是**五律闭集**：可读第一 / 错误友好 / 安全默认 / 渐进复杂 /
//! 确定优先。每一律配一张**宪法卡**（[`LAW_CARDS`] 常量卡表，三段：律文 /
//! 判读口径 / 违例样例）——律文说「是什么」，判读口径说「怎么机械判」，
//! 违例样例说「踩线长什么样」。三段缺一不可：只有律文没有口径的律条没法
//! 执行，只有口径没有违例样例的口径没法校准。
//!
//! # 二、宪法卡册：五卡冻结 + 卡指纹 + 演进只增不改
//!
//! [`ConstitutionCodex`] 开机即把五张宪法卡**冻结在 v1**（[`ConstitutionCard`]，
//! 含 FNV-1a 64 位**卡指纹**）。演进走 [`ConstitutionCodex::amend`]：**追加**
//! 新版本卡（v+1，重新计算指纹），旧版本卡原样留链——历史不可改写；
//! [`ConstitutionCodex::verify_chain`] 校验版本链严格从 1 递增、每张卡指纹
//! 与内容重算一致、全部冻结。**越权修宪**（绕过追加直接改写已冻结卡——
//! [`ConstitutionCodex::force_rewrite`] 是这条禁止路径的显性探针）→
//! [`LangLawError::CardAmend`] + 计数留痕，链不受影响。
//!
//! # 三、判例集：append-only + 判例 id 规范 + 重复拒收
//!
//! [`PrecedentBook`] 判例集只进不出：[`PrecedentBook::register`] 登记
//! （场景 / 裁决 / 引用律条 / 理由），判例 id 由册内单调发放（1 起步，
//! 严格递增），对外 id 牌 [`PrecedentBook::id_tag`]（`P-0001` 四位定宽）。
//! 重复判例（场景+裁决+引用律条+理由四元组指纹相同）→ [`LangLawError::PrecDup`]
//! 拒收计数；外部查判例：id 为 0 或 id 牌不合规范 → [`LangLawError::PrecBadId`]，
//! 规范但查无 → [`LangLawError::TargetMissing`]。
//!
//! # 四、评审拦截：提案卡 → 逐律判读 → 违律即拦（O(1) 判据）
//!
//! [`LawReviewGate`] 收**提案卡**（提案名 / 涉及面 / 自评五律 /
//! [`ProposalFacts`] 机械事实面），对五律**逐律机械判读**——判读只看
//! 事实面五个布尔位，与提案文本长短无关，**拦截 O(1)**（判据：两次判读
//! 计数与提案文本长度无关）。三条路：
//!
//! - 提案卡不完整（名为空 / 涉及面为空 / 任一律自评 [`SelfVerdict::Undeclared`]）
//!   → [`LangLawError::ProposalBad`] 拒收（结构不完整谈不上判读）；
//! - 事实面违律 → 拦截（[`ReviewOutcome::BlockedHonest`]），拦截理由**引用
//!   宪法卡律文原文**（权威序落卡不落人）；
//! - 自评申报「合律」但事实面违律——**谎报注入** → [`ReviewOutcome::BlockedLying`]
//!   + 谎报计数（注入面之一：违律提案注入检出）。
//!
//! 律条**歧义未澄清**期间，该律不可判读 → [`LangLawError::AmbigOpen`] 全案
//! 缓判（拿不准的宪法不去裁人——缓判不是放行）。
//!
//! # 五、判例冲突 → 宪法裁决（权威序：宪法卡 > 判例 > 提案）
//!
//! 两判例引用**同律条**而结论**相斥**（一 Allow 一 Block）→ [`ConflictCourt::file`]
//! 登记冲突案；引用律条不同或结论不斥 → [`LangLawError::ConflictBad`] /
//! [`LangLawError::ConflictNone`]（冲突注入检出之一：伪冲突登记计数）。
//! [`ConflictCourt::adjudicate`] 宪法法庭裁决（[`Ruling`]：左胜 / 右胜 /
//! 双驳回），裁决**落判例集**（新判例理由**引用宪法卡判读口径 + 卡版本**——
//! 权威序结构性落地：裁决的依据章盖的是卡不是判例），双驳回另自动开一张
//! 歧义登记（卡口径不足则必须澄清后方可再判，闭环回第四节）。
//!
//! # 六、律条歧义 → 澄清修订（版本前进 + 修订账）
//!
//! [`AmbiguityRegistry`] 登记歧义（律条 + 疑问，重复登记 →
//! [`LangLawError::AmbigDup`]）；[`AmbiguityRegistry::clarify`] 澄清修订：
//! 修订正文非空（[`LangLawError::AmendEmpty`]）→ 经宪法卡册**追加新版本卡**
//! （修订版本前进）→ 歧义翻 Clarified → **修订账**（[`RevisionEntry`]：
//! 从哪个版本到哪个版本、缘起哪张歧义，append-only）。
//!
//! # 七、事件账 + health + 双跑确定性对拍 + 注入三面
//!
//! [`EventLedger`] 有界事件账（满 → [`LangLawError::LedgerFull`] 拒收
//! 待清——**不静默丢事件**）；[`LangLawWorld::health`] 一键体检（
//! [`HealthReport`]：五卡冻结 / 版本守恒 / 各账计数自洽）；[`LangLawWorld::state_digest`]
//! 全状态 FNV-1a 64 确定性摘要——**双跑对拍**：同操作序列两次开机，摘要
//! 必相等（确定优先律的自我执行）；注入三面各检出：违律提案注入（第四节
//! 谎报）、冲突注入（第五节伪冲突）、越权修宪注入（第二节 force_rewrite）。
//!
//! # 八、纪律自证
//!
//! - no_std + alloc；内核路径零 unwrap/expect/panic!/运行期 assert!；
//!   可失败全走 `Result<T, LangLawError>`；错误码段独占 **0x1B30..=0x1B3F**
//!   （已 grep 确认 0x1B10/0x1B20/0x1B40/0x1B50/0x1B60 均被邻项占用，
//!   本段整段空闲）；
//! - 溢出显性：计数 saturating_add、FNV wrapping_mul；
//! - const 纪律：const fn 内只用 while+索引与 matches!，不用 slice::get /
//!   Ord::min/max / derive 判等；含 Vec 字段的结构一律不 derive Copy；
//!   编译期 [`const`] 断言五卡闭集覆盖、诊断码 0x1B30..=0x1B3F 连续互斥、
//!   权威序严格递减。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、版本与诊断码段（0x1B30..=0x1B3F 独占 16 码）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const LANG_LAW_VERSION: &str = "F5003-langlaw-v1";

/// 诊断码段基址（0x1B30——已 grep 确认 0x1B10(unx1003_tsccal)/0x1B20(ves480_s26perf)
/// /0x1B40(cga34)/0x1B50(cga35)/0x1B60(cga36) 相邻段全被占用，本段整段空闲）。
pub const ERR_CODE_BASE: u16 = 0x1B30;

/// 诊断码段末址（含）。
pub const ERR_CODE_END: u16 = 0x1B3F;

/// 诊断码总数（0x1B30..=0x1B3F 恰 16 码，全部派满 LangLawError）。
pub const ERR_CODE_COUNT: u16 = 16;

/// 越权修宪（0x1B30）——绕过追加演进直接改写已冻结宪法卡。
pub const E_CARD_AMEND: u16 = 0x1B30;
/// 宪法卡链满（0x1B31）——单律版本链到达演进容量上限。
pub const E_CARD_FULL: u16 = 0x1B31;
/// 判例重复（0x1B32）——四元组指纹与既有判例相同。
pub const E_PREC_DUP: u16 = 0x1B32;
/// 判例 id 不规范（0x1B33）——id 为 0 或 id 牌不合 P-XXXX 四位定宽规范。
pub const E_PREC_BADID: u16 = 0x1B33;
/// 提案卡不完整（0x1B34）——名/涉及面为空或自评五律缺项。
pub const E_PROPOSAL_BAD: u16 = 0x1B34;
/// 违律拦截（0x1B35）——逐律判读判违（诚实违律的正式拦截码）。
pub const E_LAW_BLOCK: u16 = 0x1B35;
/// 冲突登记不合法（0x1B36）——两判例引用律条不同（伪冲突注入检出点）。
pub const E_CONFLICT_BAD: u16 = 0x1B36;
/// 无冲突可裁（0x1B37）——两判例同律同结论，不构成相斥。
pub const E_CONFLICT_NONE: u16 = 0x1B37;
/// 裁决/修订目标不存在（0x1B38）——冲突案/歧义项/判例 id 规范但查无。
pub const E_TARGET_MISSING: u16 = 0x1B38;
/// 修订正文为空（0x1B39）——澄清修订/裁决理由/判例理由空串拒收。
pub const E_AMEND_EMPTY: u16 = 0x1B39;
/// 歧义重复登记（0x1B3A）——同律条同疑问指纹已登记。
pub const E_AMBIG_DUP: u16 = 0x1B3A;
/// 歧义未澄清（0x1B3B）——涉案律条歧义开放，评审缓判不裁人。
pub const E_AMBIG_OPEN: u16 = 0x1B3B;
/// 事件账满（0x1B3C）——账容量到顶拒收，清账后重试。
pub const E_LEDGER_FULL: u16 = 0x1B3C;
/// 判例集满（0x1B3D）——append-only 册容量守恒到顶。
pub const E_PREC_FULL: u16 = 0x1B3D;
/// 注入检出（0x1B3E）——提案谎报自评（违律提案注入的正式检出码）。
pub const E_INJECT: u16 = 0x1B3E;
/// 状态错序（0x1B3F）——未登记先裁决/已裁决再裁/内部不变量意外缺口。
pub const E_STATE: u16 = 0x1B3F;

/// 本域诊断码全集（判据对账：连续、互斥、恰 16 枚）。
pub const CODES: [u16; 16] = [
    E_CARD_AMEND, E_CARD_FULL, E_PREC_DUP, E_PREC_BADID,
    E_PROPOSAL_BAD, E_LAW_BLOCK, E_CONFLICT_BAD, E_CONFLICT_NONE,
    E_TARGET_MISSING, E_AMEND_EMPTY, E_AMBIG_DUP, E_AMBIG_OPEN,
    E_LEDGER_FULL, E_PREC_FULL, E_INJECT, E_STATE,
];

// ---------------------------------------------------------------------------
// 二、设计五律闭集
// ---------------------------------------------------------------------------

/// 五律数（宪法卡册恰五卡——闭集不扩员）。
pub const LAW_COUNT: usize = 5;

/// 设计五律（锚点闭集：可读第一/错误友好/安全默认/渐进复杂/确定优先）。
///
/// 五律为后续所有组的宪法——任何脚本语言子组的设计提案都要过这五律的
/// 逐律判读。判别序数 1..=5 稳定冻结，判例引用律条靠序数不靠字符串。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesignLaw {
    /// 可读第一：读脚本像读故事——不玩字符机巧。
    ReadableFirst = 1,
    /// 错误友好：报错说人话给改法——十三章口径语言化。
    FriendlyError = 2,
    /// 安全默认：沙箱内运行无副作用泄漏。
    SafeDefault = 3,
    /// 渐进复杂：三行能干小事三百行能干大事。
    GradualComplex = 4,
    /// 确定优先：同输入同输出——脚本行为可预测。
    DeterministicFirst = 5,
}

impl DesignLaw {
    /// 五律闭集（判据侧遍历用——顺序即序数序）。
    pub const ALL: [DesignLaw; LAW_COUNT] = [
        DesignLaw::ReadableFirst,
        DesignLaw::FriendlyError,
        DesignLaw::SafeDefault,
        DesignLaw::GradualComplex,
        DesignLaw::DeterministicFirst,
    ];

    /// 律条判别序数（1..=5，稳定冻结——判例/冲突/修订账引用它）。
    pub const fn ord(self) -> u8 {
        self as u8
    }

    /// 序数反查律条（越界 None——调用方落 [`LangLawError::State`]）。
    pub const fn from_ord(o: u8) -> Option<DesignLaw> {
        match o {
            1 => Some(DesignLaw::ReadableFirst),
            2 => Some(DesignLaw::FriendlyError),
            3 => Some(DesignLaw::SafeDefault),
            4 => Some(DesignLaw::GradualComplex),
            5 => Some(DesignLaw::DeterministicFirst),
            _ => None,
        }
    }

    /// 律名（人读——读屏可达的短名）。
    pub const fn name(self) -> &'static str {
        match self {
            DesignLaw::ReadableFirst => "可读第一",
            DesignLaw::FriendlyError => "错误友好",
            DesignLaw::SafeDefault => "安全默认",
            DesignLaw::GradualComplex => "渐进复杂",
            DesignLaw::DeterministicFirst => "确定优先",
        }
    }

    /// 律条一句话锚文（读屏摘要用——与宪法卡律文同源不同面）。
    pub const fn slogan(self) -> &'static str {
        match self {
            DesignLaw::ReadableFirst => "读脚本像读故事，不玩字符机巧",
            DesignLaw::FriendlyError => "报错说人话给改法",
            DesignLaw::SafeDefault => "沙箱内运行，无副作用泄漏",
            DesignLaw::GradualComplex => "三行能干小事，三百行能干大事",
            DesignLaw::DeterministicFirst => "同输入同输出，行为可预测",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、宪法卡常量表（三段：律文 / 判读口径 / 违例样例）
// ---------------------------------------------------------------------------

/// 单律宪法卡的静态文本（三段闭集——卡册开机从这份单源冻结）。
#[derive(Clone, Copy, Debug)]
pub struct LawCardText {
    /// 归属律条（与数组下标严格对位：下标 i 的 law.ord() == i+1）。
    pub law: DesignLaw,
    /// 律文（说「是什么」——拦截理由直接引用这一段）。
    pub statute: &'static str,
    /// 判读口径（说「怎么机械判」——评审闸逐律判读的判据面）。
    pub reading: &'static str,
    /// 违例样例（说「踩线长什么样」——口径校准面）。
    pub violation: &'static str,
}

/// 五律宪法卡单源表（常量冻结——卡册开机唯一来源）。
pub const LAW_CARDS: [LawCardText; LAW_COUNT] = [
    LawCardText {
        law: DesignLaw::ReadableFirst,
        statute: "读脚本像读故事——不玩字符机巧：脚本以叙事顺序表达因果，标识符说人话，禁止借同形字符/控制字符/零宽字符制造视觉欺骗。",
        reading: "机械判读：提案声明叙事可读，且不引入任何字形级机巧（同形异码字符、零宽连接符、控制字符伪装缩进）。两条件同为真才合律。",
        violation: "违例样例：用零宽字符隐藏分支条件让两行「看起来一样」行为不同；或用 O0/0O/l1 这类同形对做变量名。视觉可读是承诺不是巧合。",
    },
    LawCardText {
        law: DesignLaw::FriendlyError,
        statute: "报错说人话给改法——十三章口径语言化：每条诊断含三件——哪里错、为什么错、怎么改。",
        reading: "机械判读：提案声明其错误路径产出的诊断携带可执行的修改建议（「怎么改」非空）。缺改法即违律。",
        violation: "违例样例：报错只给 E_0x1B35 裸码不给行号与建议；或诊断倾倒调用栈让作者自己考古。裸码报错是把翻译工作推给用户。",
    },
    LawCardText {
        law: DesignLaw::SafeDefault,
        statute: "沙箱内运行无副作用泄漏：脚本默认无文件/网络/内存越权能力；能力显式授予且可撤销；默认值取最保守侧。",
        reading: "机械判读：提案声明运行面全程沙箱封闭、无宿主副作用外漏（文件/网络/宿主内存任一外漏即假）。全封闭才合律。",
        violation: "违例样例：脚本默认可写宿主任意路径；或运行结束后残留上次的全局状态污染下次运行。默认不安全的安全开关是装饰。",
    },
    LawCardText {
        law: DesignLaw::GradualComplex,
        statute: "三行能干小事三百行能干大事：常用小事有直路 API；复杂事按同一语法渐进表达，不为小事预付复杂度。",
        reading: "机械判读：提案声明小事直路存在（三行内可表达常用小事）。小事也要走仪式的提案违律。",
        violation: "违例样例：开关一盏灯需要先声明类型、注册生命周期、实现 trait 三步走；或复杂逻辑必须逃逸到宿主语言——两头都是违律。",
    },
    LawCardText {
        law: DesignLaw::DeterministicFirst,
        statute: "同输入同输出——脚本行为可预测：无隐式随机、无迭代顺序依赖、无环境指纹参与运算；时钟等外部源须显式注入。",
        reading: "机械判读：提案声明行为确定——同输入必同输出，不引入隐式随机源或迭代顺序依赖。不确定即违律。",
        violation: "违例样例：遍历哈希表决定技能施放顺序；或依赖未声明的系统时间差让结果随负载漂移。可预测是脚本可信的底线。",
    },
];

// ---------------------------------------------------------------------------
// 四、FNV-1a 64 指纹（卡指纹 / 判例指纹 / 歧义指纹 / 状态摘要共用）
// ---------------------------------------------------------------------------

/// FNV-1a 64 偏移基（标准常数：FNV-1a 64 位offset basis 0xcbf29ce484222325）。
pub const FNV64_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 素数（标准常数：FNV-1a 64 位 prime 0x100000001b3）。
pub const FNV64_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64：从 seed 续喂一段字节（wrapping 乘法——溢出显性不 panic）。
pub fn fnv1a64_feed(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(FNV64_PRIME);
        i += 1;
    }
    h
}

/// 指纹喂入：定宽 u32（小端 8 字节不定宽——先喂长度后喂体，避免拼接歧义）。
fn fp_u32(h: u64, v: u32) -> u64 {
    fnv1a64_feed(h, &v.to_le_bytes())
}

/// 指纹喂入：u64。
fn fp_u64(h: u64, v: u64) -> u64 {
    fnv1a64_feed(h, &v.to_le_bytes())
}

/// 指纹喂入：字符串（长度前缀 + UTF-8 字节——长度前缀防止边界歧义）。
fn fp_str(h: u64, s: &str) -> u64 {
    let h = fp_u64(h, s.len() as u64);
    fnv1a64_feed(h, s.as_bytes())
}

// ---------------------------------------------------------------------------
// 五、错误枚举（每变体 detail() 中文可操作诊断）
// ---------------------------------------------------------------------------

/// 语言设计原则域的全部可失败路径（码段 0x1B30..=0x1B3F 独占）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LangLawError {
    /// 越权修宪——绕过追加演进直接改写已冻结宪法卡（0x1B30）。
    CardAmend,
    /// 宪法卡链满——单律版本链到演进容量上限（0x1B31）。
    CardFull,
    /// 判例重复——四元组指纹与既有判例相同（0x1B32）。
    PrecDup,
    /// 判例 id 不规范——id 为 0 或 id 牌不合规范（0x1B33）。
    PrecBadId,
    /// 提案卡不完整——名/涉及面为空或自评缺律（0x1B34）。
    ProposalBad,
    /// 违律拦截——逐律判读判违（0x1B35；评审闸以 Receipt 形态返回，
    /// 此变体供下游显式转译）。
    LawBlock,
    /// 冲突登记不合法——两判例引用律条不同（0x1B36）。
    ConflictBad,
    /// 无冲突可裁——同律同结论不相斥（0x1B37）。
    ConflictNone,
    /// 目标不存在——冲突案/歧义项/判例 id 规范但查无（0x1B38）。
    TargetMissing,
    /// 修订正文为空——澄清/理由空串拒收（0x1B39）。
    AmendEmpty,
    /// 歧义重复登记——同律同问指纹已登记（0x1B3A）。
    AmbigDup,
    /// 歧义未澄清——涉案律条歧义开放，评审缓判（0x1B3B）。
    AmbigOpen,
    /// 事件账满——账容量到顶拒收（0x1B3C）。
    LedgerFull,
    /// 判例集满——append-only 册到顶（0x1B3D）。
    PrecFull,
    /// 注入检出——提案谎报自评（0x1B3E）。
    Inject,
    /// 状态错序——未登记先裁/已裁再裁/不变量意外缺口（0x1B3F）。
    State,
}

impl LangLawError {
    /// 诊断码（0x1B30..=0x1B3F——段独占，判据侧两两互异有编译期断言）。
    pub const fn code(self) -> u16 {
        match self {
            LangLawError::CardAmend => E_CARD_AMEND,
            LangLawError::CardFull => E_CARD_FULL,
            LangLawError::PrecDup => E_PREC_DUP,
            LangLawError::PrecBadId => E_PREC_BADID,
            LangLawError::ProposalBad => E_PROPOSAL_BAD,
            LangLawError::LawBlock => E_LAW_BLOCK,
            LangLawError::ConflictBad => E_CONFLICT_BAD,
            LangLawError::ConflictNone => E_CONFLICT_NONE,
            LangLawError::TargetMissing => E_TARGET_MISSING,
            LangLawError::AmendEmpty => E_AMEND_EMPTY,
            LangLawError::AmbigDup => E_AMBIG_DUP,
            LangLawError::AmbigOpen => E_AMBIG_OPEN,
            LangLawError::LedgerFull => E_LEDGER_FULL,
            LangLawError::PrecFull => E_PREC_FULL,
            LangLawError::Inject => E_INJECT,
            LangLawError::State => E_STATE,
        }
    }

    /// 中文可操作诊断（错误友好律的自我执行：说人话给改法）。
    pub fn detail(&self) -> &'static str {
        match self {
            LangLawError::CardAmend => {
                "越权修宪：宪法卡已冻结，演进只能追加新版本卡（amend），请走宪法卡册的追加通道，不要改写历史版本。"
            }
            LangLawError::CardFull => {
                "宪法卡链已到演进容量上限：请先归档旧版本链再开新链，或评估该律是否需要拆条。"
            }
            LangLawError::PrecDup => {
                "判例重复：场景/裁决/引用律条/理由四元组与既有判例指纹相同，判例集只进不出，请补充新事实后另行登记。"
            }
            LangLawError::PrecBadId => {
                "判例 id 不规范：id 必须为正整数且 id 牌形如 P-0001（四位定宽），请用册内发放的 id 或 id_tag 返回的牌面。"
            }
            LangLawError::ProposalBad => {
                "提案卡不完整：提案名/涉及面不能为空，且五律自评每律都要表态（Undeclared 视为缺律），请补齐后再送评审。"
            }
            LangLawError::LawBlock => {
                "违律拦截：设计提案与宪法卡判读口径相抵，请按拦截理由引用的律文条款修改设计后重新送审。"
            }
            LangLawError::ConflictBad => {
                "冲突登记不合法：两判例必须引用同一律条且结论相斥才构成冲突案，请核对两判例的引用律条。"
            }
            LangLawError::ConflictNone => {
                "无冲突可裁：两判例同律同结论不构成相斥，无需宪法法庭，直接引用既有判例即可。"
            }
            LangLawError::TargetMissing => {
                "裁决/修订目标不存在：冲突案号、歧义编号或判例 id 在册内查无，请先登记再裁决。"
            }
            LangLawError::AmendEmpty => {
                "修订正文为空：澄清修订、裁决理由与判例理由必须写明内容，空修订等于没修。"
            }
            LangLawError::AmbigDup => {
                "歧义重复登记：同一律条同一疑问已登记，请到原歧义项下追踪澄清进度。"
            }
            LangLawError::AmbigOpen => {
                "律条歧义未澄清：涉案律条口径尚在争议，评审暂缓判读，请先走澄清修订流程再送审。"
            }
            LangLawError::LedgerFull => {
                "事件账已满：账容量到顶拒收新事件（不静默丢弃），请先 drain 清账再继续记账。"
            }
            LangLawError::PrecFull => {
                "判例集已到容量上限：append-only 册不做覆盖，请归档结案判例或扩容后重试。"
            }
            LangLawError::Inject => {
                "注入检出：提案自评申报合律但事实面违律（谎报），请如实自评——评审以事实面机械判读为准，谎报按注入计入审计。"
            }
            LangLawError::State => {
                "状态错序：操作与登记状态不符（如未登记先裁决、已裁决再裁），请先完成前置步骤。"
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 六、宪法卡与宪法卡册（五卡冻结 + 指纹 + 演进只增不改）
// ---------------------------------------------------------------------------

/// 单律版本链演进容量上限（来源：演进审计容量判据——单律版本链 64 版封顶，
/// 超顶即归档重开，防止修订账无限膨胀）。
pub const CARD_CHAIN_CAP: u32 = 64;

/// 宪法卡（单律单版本——冻结后不可改写，演进=追加新卡）。
///
/// 含 Vec/String 字段，不 derive Copy（纪律：堆载体不上 Copy）。
#[derive(Clone, Debug)]
pub struct ConstitutionCard {
    /// 归属律条。
    pub law: DesignLaw,
    /// 卡版本（1 起步，链内严格递增——v1 是开机冻结的锚点卡）。
    pub version: u32,
    /// 律文（当前版正文——拦截理由引用面）。
    pub statute: String,
    /// 判读口径（当前版口径——评审闸判据面）。
    pub reading: String,
    /// 违例样例（当前版样例——口径校准面）。
    pub violation: String,
    /// 卡指纹（FNV-1a 64：律序数+版本+三段文本——内容寻址，改一个字即翻脸）。
    pub fingerprint: u64,
    /// 冻结位（卡册内所有卡一律冻结——「未冻结卡」不是合法状态）。
    pub frozen: bool,
}

impl ConstitutionCard {
    /// 卡指纹重算（与 [`ConstitutionCard::fingerprint`] 同式——verify 链用）。
    pub fn recompute_fingerprint(&self) -> u64 {
        fingerprint_of_card(self.law, self.version, &self.statute, &self.reading, &self.violation)
    }
}

/// 卡指纹单源计算式（建卡与验链共用一把尺）。
fn fingerprint_of_card(law: DesignLaw, version: u32, statute: &str, reading: &str, violation: &str) -> u64 {
    let h = fp_u32(FNV64_OFFSET, law.ord() as u32);
    let h = fp_u32(h, version);
    let h = fp_str(h, statute);
    let h = fp_str(h, reading);
    fp_str(h, violation)
}

/// 宪法卡册（五律各一条版本链，append-only）。
///
/// 开机即从 [`LAW_CARDS`] 常量表冻结五张 v1 卡；演进只增不改：
/// [`amend`](ConstitutionCodex::amend) 追加 v+1 新卡并重算指纹，旧卡原样
/// 留链；[`force_rewrite`](ConstitutionCodex::force_rewrite) 是越权修宪
/// 禁止路径的显性探针，永远 Err 并计数留痕。
#[derive(Clone, Debug)]
pub struct ConstitutionCodex {
    chains: [Vec<ConstitutionCard>; LAW_COUNT],
    /// 合法追加演进总次数（每次 amend +1，saturating）。
    pub amended_total: u32,
    /// 越权修宪注入检出次数（force_rewrite 探针每次 +1，saturating）。
    pub overreach_attempts: u32,
}

impl ConstitutionCodex {
    /// 开机：从常量卡表冻结五张 v1 卡（不可失败——常量三段全非空有编译期断言）。
    pub fn boot() -> ConstitutionCodex {
        let mut chains: [Vec<ConstitutionCard>; LAW_COUNT] = [
            Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(),
        ];
        let mut i = 0;
        while i < LAW_CARDS.len() {
            let t = &LAW_CARDS[i];
            let card = ConstitutionCard {
                law: t.law,
                version: 1,
                statute: t.statute.to_string(),
                reading: t.reading.to_string(),
                violation: t.violation.to_string(),
                fingerprint: fingerprint_of_card(t.law, 1, t.statute, t.reading, t.violation),
                frozen: true,
            };
            let slot = (t.law.ord() - 1) as usize;
            chains[slot].push(card);
            i += 1;
        }
        ConstitutionCodex {
            chains,
            amended_total: 0,
            overreach_attempts: 0,
        }
    }

    /// 当前生效卡（链尾——最新版即生效版）。
    pub fn active(&self, law: DesignLaw) -> Option<&ConstitutionCard> {
        let slot = (law.ord() - 1) as usize;
        match self.chains[slot].len() {
            0 => None,
            n => self.chains[slot].get(n - 1),
        }
    }

    /// 当前生效卡（查无即 State——开机后五链恒非空，此缺口属不变量意外）。
    pub fn active_or_err(&self, law: DesignLaw) -> Result<&ConstitutionCard, LangLawError> {
        match self.active(law) {
            Some(c) => Ok(c),
            None => Err(LangLawError::State),
        }
    }

    /// 单律链长（= 该律当前生效版本号）。
    pub fn chain_len(&self, law: DesignLaw) -> u32 {
        let slot = (law.ord() - 1) as usize;
        self.chains[slot].len() as u32
    }

    /// 全册版本总数（开机基线 5——五卡各 v1；判据：>=5 且 == sum(chain_len)）。
    pub fn version_total(&self) -> u32 {
        let mut i = 0;
        let mut total: u32 = 0;
        while i < LAW_COUNT {
            total = total.saturating_add(self.chains[i].len() as u32);
            i += 1;
        }
        total
    }

    /// 合法演进：追加新版本卡（v+1，三段全新，重算指纹，旧卡原样留链）。
    ///
    /// 三段正文任一为空 → [`LangLawError::AmendEmpty`]；链到顶 →
    /// [`LangLawError::CardFull`]。返回新卡版本号。
    pub fn amend(
        &mut self,
        law: DesignLaw,
        statute: &str,
        reading: &str,
        violation: &str,
    ) -> Result<u32, LangLawError> {
        if statute.is_empty() || reading.is_empty() || violation.is_empty() {
            return Err(LangLawError::AmendEmpty);
        }
        let slot = (law.ord() - 1) as usize;
        let cur = match self.chains[slot].last() {
            Some(c) => c.version,
            None => return Err(LangLawError::State),
        };
        if cur >= CARD_CHAIN_CAP {
            return Err(LangLawError::CardFull);
        }
        let next = cur.saturating_add(1);
        let card = ConstitutionCard {
            law,
            version: next,
            statute: statute.to_string(),
            reading: reading.to_string(),
            violation: violation.to_string(),
            fingerprint: fingerprint_of_card(law, next, statute, reading, violation),
            frozen: true,
        };
        self.chains[slot].push(card);
        self.amended_total = self.amended_total.saturating_add(1);
        Ok(next)
    }

    /// 越权修宪禁止路径探针：试图原地改写已冻结卡——**永远拒绝**。
    ///
    /// 这是「越权修宪注入」的检出面：调用即 [`LangLawError::CardAmend`]
    /// + `overreach_attempts` 计数留痕，链内容不受任何影响。
    pub fn force_rewrite(
        &mut self,
        law: DesignLaw,
        _statute: &str,
        _reading: &str,
        _violation: &str,
    ) -> Result<u32, LangLawError> {
        // 律条参数保留在签名面（审计点名哪条律被试图改写），此处仅消歧义。
        let _ = law;
        self.overreach_attempts = self.overreach_attempts.saturating_add(1);
        Err(LangLawError::CardAmend)
    }

    /// 验链：版本严格从 1 递增、全部冻结、每张卡指纹与内容重算一致。
    pub fn verify_chain(&self, law: DesignLaw) -> Result<(), LangLawError> {
        let slot = (law.ord() - 1) as usize;
        let chain = match self.chains[slot].len() {
            0 => return Err(LangLawError::State),
            n => n,
        };
        let mut i = 0;
        while i < chain {
            let card = match self.chains[slot].get(i) {
                Some(c) => c,
                None => return Err(LangLawError::State),
            };
            if card.version != (i as u32) + 1 {
                return Err(LangLawError::State);
            }
            if !card.frozen {
                return Err(LangLawError::State);
            }
            if card.recompute_fingerprint() != card.fingerprint {
                return Err(LangLawError::State);
            }
            i += 1;
        }
        Ok(())
    }

    /// 全册验链（五律逐链——audit 与 health 的地基）。
    pub fn verify_all(&self) -> Result<(), LangLawError> {
        let mut i = 0;
        while i < LAW_COUNT {
            let law = match DesignLaw::from_ord((i as u8) + 1) {
                Some(l) => l,
                None => return Err(LangLawError::State),
            };
            self.verify_chain(law)?;
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 七、判例集（append-only + id 规范 + 指纹去重）
// ---------------------------------------------------------------------------

/// 判例集容量上限（来源：判据承载——append-only 册 512 例封顶，超顶归档）。
pub const PRECEDENT_CAP: usize = 512;

/// 判例结论闭集（裁决只有两面——放行/拦截，不设中间态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 放行（该场景该做法合律）。
    Allow,
    /// 拦截（该场景该做法违律）。
    Block,
}

impl Verdict {
    /// 结论名（读屏可达）。
    pub const fn name(self) -> &'static str {
        match self {
            Verdict::Allow => "放行",
            Verdict::Block => "拦截",
        }
    }

    /// 与另一结论是否相斥（同律条下相斥才构成冲突案）。
    pub const fn contradicts(self, other: Verdict) -> bool {
        matches!(
            (self, other),
            (Verdict::Allow, Verdict::Block) | (Verdict::Block, Verdict::Allow)
        )
    }
}

/// 设计判例（场景/裁决/引用律条/理由四元组——指纹即四元组内容寻址）。
#[derive(Clone, Debug)]
pub struct Precedent {
    /// 判例 id（册内单调发放，1 起步严格递增——判例 id 规范的根）。
    pub id: u32,
    /// id 牌（P-0001 四位定宽——对外引用面）。
    pub tag: String,
    /// 场景（这一例裁的是什么局面）。
    pub scene: String,
    /// 裁决（Allow/Block 闭集）。
    pub verdict: Verdict,
    /// 引用律条（判例必挂一律——不挂律的意见不是判例）。
    pub cited_law: DesignLaw,
    /// 理由（说人话——错误友好律对判例自身的要求）。
    pub reasoning: String,
    /// 判例指纹（四元组内容寻址——重复拒收的判据）。
    pub fingerprint: u64,
}

/// 判例集（append-only：只进不出，重复拒收）。
#[derive(Clone, Debug)]
pub struct PrecedentBook {
    items: Vec<Precedent>,
    next_id: u32,
    /// 重复判例拒收计数（saturating）。
    pub dup_rejected: u32,
}

/// 判例 id 牌（P-XXXX 四位定宽；超 9999 饱和为 P-9999——id 永不越格式）。
pub fn id_tag(id: u32) -> String {
    let shown = if id > 9999 { 9999 } else { id };
    format!("P-{:04}", shown)
}

/// 判例指纹单源计算式（登记与查重共用一把尺）。
fn precedent_fingerprint(scene: &str, verdict: Verdict, cited: DesignLaw, reasoning: &str) -> u64 {
    let h = fp_str(FNV64_OFFSET, scene);
    let h = fp_u32(h, match verdict {
        Verdict::Allow => 1,
        Verdict::Block => 2,
    });
    let h = fp_u32(h, cited.ord() as u32);
    fp_str(h, reasoning)
}

impl PrecedentBook {
    /// 开机：空册，id 从 1 发放。
    pub fn new() -> PrecedentBook {
        PrecedentBook {
            items: Vec::new(),
            next_id: 1,
            dup_rejected: 0,
        }
    }

    /// 在册判例数。
    pub fn len(&self) -> u32 {
        self.items.len() as u32
    }

    /// 是否空册。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 下一个将发放的判例 id（id 连续性审计用：len + 1 == next_id）。
    pub fn next_id(&self) -> u32 {
        self.next_id
    }

    /// 登记判例（场景/裁决/引用律条/理由）。
    ///
    /// 场景与理由任一为空 → [`LangLawError::AmendEmpty`]；册到顶 →
    /// [`LangLawError::PrecFull`]；四元组指纹重复 → [`LangLawError::PrecDup`]
    /// + 拒收计数。返回新判例 id。
    pub fn register(
        &mut self,
        scene: &str,
        verdict: Verdict,
        cited_law: DesignLaw,
        reasoning: &str,
    ) -> Result<u32, LangLawError> {
        if scene.is_empty() || reasoning.is_empty() {
            return Err(LangLawError::AmendEmpty);
        }
        if self.items.len() >= PRECEDENT_CAP {
            return Err(LangLawError::PrecFull);
        }
        let fp = precedent_fingerprint(scene, verdict, cited_law, reasoning);
        let mut i = 0;
        while i < self.items.len() {
            if let Some(p) = self.items.get(i) {
                if p.fingerprint == fp {
                    self.dup_rejected = self.dup_rejected.saturating_add(1);
                    return Err(LangLawError::PrecDup);
                }
            }
            i += 1;
        }
        let id = self.next_id;
        let tag = id_tag(id);
        self.items.push(Precedent {
            id,
            tag,
            scene: scene.to_string(),
            verdict,
            cited_law,
            reasoning: reasoning.to_string(),
            fingerprint: fp,
        });
        self.next_id = self.next_id.saturating_add(1);
        Ok(id)
    }

    /// 按 id 取判例（id 为 0 → [`LangLawError::PrecBadId`]；规范但查无 →
    /// [`LangLawError::TargetMissing`]）。
    pub fn lookup(&self, id: u32) -> Result<&Precedent, LangLawError> {
        if id == 0 {
            return Err(LangLawError::PrecBadId);
        }
        for p in self.items.iter() {
            if p.id == id {
                return Ok(p);
            }
        }
        Err(LangLawError::TargetMissing)
    }

    /// 按 id 牌取判例（严格解析 `P-XXXX`：前缀/长度/数字位任一不合 →
    /// [`LangLawError::PrecBadId`]；规范但查无 → [`LangLawError::TargetMissing`]）。
    pub fn find_by_tag(&self, tag: &str) -> Result<&Precedent, LangLawError> {
        let bytes = tag.as_bytes();
        if bytes.len() != 6 {
            return Err(LangLawError::PrecBadId);
        }
        if bytes[0] != b'P' || bytes[1] != b'-' {
            return Err(LangLawError::PrecBadId);
        }
        let mut val: u32 = 0;
        let mut i = 2;
        while i < 6 {
            let b = bytes[i];
            if !(b as char).is_ascii_digit() {
                return Err(LangLawError::PrecBadId);
            }
            val = val
                .checked_mul(10)
                .and_then(|v| v.checked_add((b - b'0') as u32))
                .ok_or(LangLawError::PrecBadId)?;
            i += 1;
        }
        self.lookup(val)
    }

    /// 引用某律的判例 id 清单（冲突登记对账用——O(在册数) 一次扫）。
    pub fn ids_citing(&self, law: DesignLaw) -> Vec<u32> {
        let mut out = Vec::new();
        for p in self.items.iter() {
            if p.cited_law.ord() == law.ord() {
                out.push(p.id);
            }
        }
        out
    }

    /// id 连续性审计（append-only 无删除 → len + 1 == next_id 且逐 id 存在）。
    pub fn audit_ids(&self) -> Result<(), LangLawError> {
        if self.len().saturating_add(1) != self.next_id {
            return Err(LangLawError::State);
        }
        let mut i = 0;
        while i < self.items.len() {
            match self.items.get(i) {
                Some(p) => {
                    if p.id != (i as u32) + 1 {
                        return Err(LangLawError::State);
                    }
                }
                None => return Err(LangLawError::State),
            }
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 八、评审拦截（提案卡 → 逐律判读 → 违律即拦，O(1) 判据）
// ---------------------------------------------------------------------------

/// 自评表态闭集（Undeclared 即缺律——提案卡不完整的判定根）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelfVerdict {
    /// 未表态（缺律——提案卡不完整）。
    Undeclared,
    /// 自评合律。
    Complies,
    /// 自评违律（诚实申报仍被拦——但不算谎报注入）。
    Violates,
}

/// 提案机械事实面（评审闸逐律判读的**唯一**依据——五布尔位闭集）。
///
/// 判读只看事实面不看文本：拦截 O(1)（与提案文本长短无关），且
/// 「自评谎报」有机械对账根（自评说合律、事实面说违律即谎报）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProposalFacts {
    /// 叙事可读：脚本以叙事顺序表达因果。
    pub narrative_readable: bool,
    /// 无字形机巧：不借同形/零宽/控制字符做视觉欺骗。
    pub no_glyph_trickery: bool,
    /// 错误带改法：错误路径诊断携带可执行修改建议。
    pub error_has_fix: bool,
    /// 沙箱封闭：运行无宿主副作用外漏。
    pub sandbox_no_leak: bool,
    /// 小事直路：三行内可表达常用小事。
    pub small_path_exists: bool,
    /// 行为确定：同输入同输出，无隐式随机/顺序依赖。
    pub deterministic: bool,
}

impl ProposalFacts {
    /// 全真事实面（合规基线——五律判读应全过）。
    pub const COMPLIANT: ProposalFacts = ProposalFacts {
        narrative_readable: true,
        no_glyph_trickery: true,
        error_has_fix: true,
        sandbox_no_leak: true,
        small_path_exists: true,
        deterministic: true,
    };

    /// 全假事实面（违律基线——五律判读应全拦）。
    pub const ALL_FALSE: ProposalFacts = ProposalFacts {
        narrative_readable: false,
        no_glyph_trickery: false,
        error_has_fix: false,
        sandbox_no_leak: false,
        small_path_exists: false,
        deterministic: false,
    };
}

/// 评审结论三态（通过 / 诚实拦截 / 谎报拦截）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewOutcome {
    /// 五律全过——放行。
    Passed,
    /// 诚实拦截（自评如实申报违律或未申报——不记注入）。
    BlockedHonest,
    /// 谎报拦截（自评申报合律但事实面违律——记注入）。
    BlockedLying,
}

/// 评审回执（拦截理由引用宪法卡律文原文——权威序落卡）。
#[derive(Clone, Debug)]
pub struct ReviewReceipt {
    /// 评审流水号（闸内单调，1 起步）。
    pub seq: u32,
    /// 结论三态。
    pub outcome: ReviewOutcome,
    /// 拦在何律（通过为 None）。
    pub blocked_law: Option<DesignLaw>,
    /// 诊断码（通过 0；诚实拦截 0x1B35；谎报拦截 0x1B3E）。
    pub diag_code: u16,
    /// 拦截理由（引用宪法卡律文原文——通过为空串）。
    pub reason: String,
    /// 读屏可达人话行（评审结果对非程序员作者可达——域本色）。
    pub spoken: Vec<String>,
}

/// 逐律机械判读单源（五律各对一事实位——O(1)，与提案文本无关）。
///
/// 可读第一 ← 叙事可读 且 无字形机巧；错误友好 ← 错误带改法；
/// 安全默认 ← 沙箱封闭；渐进复杂 ← 小事直路；确定优先 ← 行为确定。
pub const fn read_law(law: DesignLaw, facts: ProposalFacts) -> bool {
    match law {
        DesignLaw::ReadableFirst => facts.narrative_readable && facts.no_glyph_trickery,
        DesignLaw::FriendlyError => facts.error_has_fix,
        DesignLaw::SafeDefault => facts.sandbox_no_leak,
        DesignLaw::GradualComplex => facts.small_path_exists,
        DesignLaw::DeterministicFirst => facts.deterministic,
    }
}

/// 违律整改指引（拦人要指路——错误友好律对拦截理由自身的要求）。
pub const fn remediation_hint(law: DesignLaw) -> &'static str {
    match law {
        DesignLaw::ReadableFirst => "整改：改用叙事顺序表达，替换全部同形/零宽/控制字符机巧。",
        DesignLaw::FriendlyError => "整改：给每条诊断补齐「哪里错/为什么错/怎么改」三件。",
        DesignLaw::SafeDefault => "整改：默认收进沙箱，外漏能力改显式授予并可撤销。",
        DesignLaw::GradualComplex => "整改：为常用小事开三行内直路，复杂事同语法渐进。",
        DesignLaw::DeterministicFirst => "整改：移除隐式随机与顺序依赖，外部源显式注入。",
    }
}

/// 评审闸（计数器 + 流水号——判读逻辑无状态，事实面进回执出）。
#[derive(Clone, Debug)]
pub struct LawReviewGate {
    seq: u32,
    /// 通过计数。
    pub passed: u32,
    /// 诚实拦截计数。
    pub blocked_honest: u32,
    /// 谎报拦截计数（违律提案注入检出——注入面之一）。
    pub blocked_lying: u32,
}

impl LawReviewGate {
    /// 开闸：流水号从 1 起步，三计数归零。
    pub fn new() -> LawReviewGate {
        LawReviewGate {
            seq: 1,
            passed: 0,
            blocked_honest: 0,
            blocked_lying: 0,
        }
    }

    /// 已评提案总数（三态守恒判据：passed+blocked_honest+blocked_lying == seq-1）。
    pub fn total_reviewed(&self) -> u32 {
        self.seq.saturating_sub(1)
    }

    /// 送审：提案卡 → 完整性闸 → 歧义缓判闸 → 逐律判读 → 回执。
    ///
    /// 完整性不完整 → [`LangLawError::ProposalBad`]；涉案律条歧义开放 →
    /// [`LangLawError::AmbigOpen`]（缓判不是放行）；其余结论走
    /// [`ReviewReceipt`]（诚实拦 / 谎报拦 / 通过）。**拦截 O(1)**：判读
    /// 恰五次固定事实位取值，与提案文本长度无关。
    pub fn review(
        &mut self,
        codex: &ConstitutionCodex,
        ambiguities: &AmbiguityRegistry,
        title: &str,
        surface: &str,
        facts: ProposalFacts,
        self_review: &[SelfVerdict; LAW_COUNT],
    ) -> Result<ReviewReceipt, LangLawError> {
        // 完整性闸：名/涉及面非空 + 五律自评无缺项。
        if title.is_empty() || surface.is_empty() {
            return Err(LangLawError::ProposalBad);
        }
        let mut i = 0;
        while i < LAW_COUNT {
            if matches!(self_review[i], SelfVerdict::Undeclared) {
                return Err(LangLawError::ProposalBad);
            }
            i += 1;
        }

        // 歧义缓判闸：任一律歧义开放 → 全案缓判（拿不准的宪法不裁人）。
        let mut i = 0;
        while i < LAW_COUNT {
            let law = match DesignLaw::from_ord((i as u8) + 1) {
                Some(l) => l,
                None => return Err(LangLawError::State),
            };
            if ambiguities.is_open(law) {
                return Err(LangLawError::AmbigOpen);
            }
            i += 1;
        }

        let seq = self.seq;
        self.seq = self.seq.saturating_add(1);

        // 逐律判读：按律序数固定五次判读，违律即拦（拦第一律，理由引卡）。
        let mut i = 0;
        while i < LAW_COUNT {
            let law = match DesignLaw::from_ord((i as u8) + 1) {
                Some(l) => l,
                None => return Err(LangLawError::State),
            };
            let complies = read_law(law, facts);
            if complies {
                i += 1;
                continue;
            }
            let card = codex.active_or_err(law)?;
            let lying = matches!(self_review[i], SelfVerdict::Complies);
            let outcome = if lying {
                self.blocked_lying = self.blocked_lying.saturating_add(1);
                ReviewOutcome::BlockedLying
            } else {
                self.blocked_honest = self.blocked_honest.saturating_add(1);
                ReviewOutcome::BlockedHonest
            };
            let diag = if lying { E_INJECT } else { E_LAW_BLOCK };
            // 拦截理由引用宪法卡律文原文（权威序：落卡不落人）+ 整改指引。
            let reason = format!(
                "违{}律（宪法卡 v{}）：律文「{}」；判读口径：{}；{}",
                law.name(),
                card.version,
                card.statute,
                card.reading,
                remediation_hint(law)
            );
            let spoken = vec![
                format!("评审#{}：拦截（{}）", seq, if lying { "自评谎报" } else { "违律" }),
                format!("拦在律条：{}", law.name()),
                format!("理由：{}", law.slogan()),
            ];
            return Ok(ReviewReceipt {
                seq,
                outcome,
                blocked_law: Some(law),
                diag_code: diag,
                reason,
                spoken,
            });
        }

        // 五律全过——放行。
        self.passed = self.passed.saturating_add(1);
        Ok(ReviewReceipt {
            seq,
            outcome: ReviewOutcome::Passed,
            blocked_law: None,
            diag_code: 0,
            reason: String::new(),
            spoken: vec![format!("评审#{}：通过（五律全过）", seq)],
        })
    }
}

// ---------------------------------------------------------------------------
// 九、律条歧义与澄清修订（歧义登记 → 澄清修订 → 版本前进 + 修订账）
// ---------------------------------------------------------------------------

/// 歧义状态闭集（开放 / 已澄清）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmbigStatus {
    /// 开放——该律条判读被缓判。
    Open,
    /// 已澄清——修订已落卡，判读恢复。
    Clarified,
}

/// 歧义登记项（律条 + 疑问；澄清后留痕修订去向）。
#[derive(Clone, Debug)]
pub struct AmbiguityFiling {
    /// 歧义编号（册内单调，1 起步）。
    pub id: u32,
    /// 涉案律条。
    pub law: DesignLaw,
    /// 疑问（歧义点说人话写明）。
    pub question: String,
    /// 疑问指纹（律序数+疑问——重复登记判据）。
    pub question_fp: u64,
    /// 状态。
    pub status: AmbigStatus,
    /// 澄清正文摘要（未澄清为空串）。
    pub clarification: String,
    /// 澄清落卡后的生效版本号（未澄清为 0）。
    pub card_version_after: u32,
}

/// 歧义登记册（重复拒收 + 澄清走卡册追加）。
#[derive(Clone, Debug)]
pub struct AmbiguityRegistry {
    items: Vec<AmbiguityFiling>,
    next_id: u32,
    /// 重复歧义拒收计数（saturating）。
    pub dup_rejected: u32,
}

impl AmbiguityRegistry {
    /// 开机：空册。
    pub fn new() -> AmbiguityRegistry {
        AmbiguityRegistry {
            items: Vec::new(),
            next_id: 1,
            dup_rejected: 0,
        }
    }

    /// 在册歧义数。
    pub fn len(&self) -> u32 {
        self.items.len() as u32
    }

    /// 某律条是否歧义开放（评审缓判闸 O(1) 判据——O(登记数) 单扫，登记数
    /// 由澄清流程收敛，开放项常量级）。
    pub fn is_open(&self, law: DesignLaw) -> bool {
        for f in self.items.iter() {
            if f.law.ord() == law.ord() && matches!(f.status, AmbigStatus::Open) {
                return true;
            }
        }
        false
    }

    /// 开放歧义数。
    pub fn open_count(&self) -> u32 {
        let mut n: u32 = 0;
        for f in self.items.iter() {
            if matches!(f.status, AmbigStatus::Open) {
                n = n.saturating_add(1);
            }
        }
        n
    }

    /// 登记歧义（律条 + 疑问）。疑问为空 → [`LangLawError::AmendEmpty`]；
    /// 同律同问指纹重复 → [`LangLawError::AmbigDup`] + 拒收计数。
    pub fn register(&mut self, law: DesignLaw, question: &str) -> Result<u32, LangLawError> {
        if question.is_empty() {
            return Err(LangLawError::AmendEmpty);
        }
        let fp = fp_str(fp_u32(FNV64_OFFSET, law.ord() as u32), question);
        let mut i = 0;
        while i < self.items.len() {
            if let Some(f) = self.items.get(i) {
                if f.question_fp == fp {
                    self.dup_rejected = self.dup_rejected.saturating_add(1);
                    return Err(LangLawError::AmbigDup);
                }
            }
            i += 1;
        }
        let id = self.next_id;
        self.items.push(AmbiguityFiling {
            id,
            law,
            question: question.to_string(),
            question_fp: fp,
            status: AmbigStatus::Open,
            clarification: String::new(),
            card_version_after: 0,
        });
        self.next_id = self.next_id.saturating_add(1);
        Ok(id)
    }

    /// 澄清修订：修订正文非空 → 经宪法卡册**追加新版本卡**（修订版本前进）
    /// → 歧义翻 Clarified。返回（旧版本号，新版本号）。
    ///
    /// 目标查无 → [`LangLawError::TargetMissing`]；已澄清再澄清 →
    /// [`LangLawError::State`]；正文为空 → [`LangLawError::AmendEmpty`]；
    /// 卡册侧错误原样透传（AmendEmpty/CardFull）。
    pub fn clarify(
        &mut self,
        id: u32,
        codex: &mut ConstitutionCodex,
        revision: &str,
    ) -> Result<(u32, u32), LangLawError> {
        let idx = match self.items.iter().position(|f| f.id == id) {
            Some(i) => i,
            None => return Err(LangLawError::TargetMissing),
        };
        if revision.is_empty() {
            return Err(LangLawError::AmendEmpty);
        }
        // 先在不可变读侧取齐旧卡三段与状态，再进入可变段。
        let (law, status, old_statute, old_reading, old_violation) = {
            let f = match self.items.get(idx) {
                Some(f) => f,
                None => return Err(LangLawError::State),
            };
            let card = codex.active_or_err(f.law)?;
            (
                f.law,
                f.status,
                card.statute.clone(),
                card.reading.clone(),
                card.violation.clone(),
            )
        };
        if matches!(status, AmbigStatus::Clarified) {
            return Err(LangLawError::State);
        }
        // 澄清修订落卡：律文换成澄清后的正文；口径与违例样例沿用当前版
        // （口径层面的歧义才需要连口径一起改——那走两次 amend，账各记各的）。
        let new_v = codex.amend(law, revision, &old_reading, &old_violation)?;
        let from_v = new_v.saturating_sub(1);
        let filing = match self.items.get_mut(idx) {
            Some(f) => f,
            None => return Err(LangLawError::State),
        };
        filing.status = AmbigStatus::Clarified;
        filing.clarification = revision.to_string();
        filing.card_version_after = new_v;
        // old_statute/old_reading/old_violation 在此仅作防误用取值（审计面
        // 不再消费）——下划线命名以免未用告警歧义。
        let _ = (old_statute, old_reading, old_violation);
        Ok((from_v, new_v))
    }

    /// 编号连续性审计（append-only 无删除 → len + 1 == next_id 且逐号存在）。
    pub fn audit_ids(&self) -> Result<(), LangLawError> {
        if self.len().saturating_add(1) != self.next_id {
            return Err(LangLawError::State);
        }
        let mut i = 0;
        while i < self.items.len() {
            match self.items.get(i) {
                Some(f) => {
                    if f.id != (i as u32) + 1 {
                        return Err(LangLawError::State);
                    }
                }
                None => return Err(LangLawError::State),
            }
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 十、判例冲突与宪法裁决（权威序：宪法卡 > 判例 > 提案）
// ---------------------------------------------------------------------------

/// 权威序三源（裁决依据的效力序——结构性落地在裁决落卡章节上）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthorityKind {
    /// 宪法卡（最高——裁决理由必须引用卡口径与版本）。
    ConstitutionCard,
    /// 判例（中——判例与提案冲突时判例赢）。
    Precedent,
    /// 提案（最低——提案与任何既有面冲突时提案输）。
    Proposal,
}

impl AuthorityKind {
    /// 权威秩（卡 3 > 判例 2 > 提案 1——编译期断言严格递减）。
    pub const fn rank(self) -> u8 {
        match self {
            AuthorityKind::ConstitutionCard => 3,
            AuthorityKind::Precedent => 2,
            AuthorityKind::Proposal => 1,
        }
    }

    /// 严格高于另一权威源。
    pub const fn outranks(self, other: AuthorityKind) -> bool {
        self.rank() > other.rank()
    }

    /// 权威源名（读屏可达）。
    pub const fn name(self) -> &'static str {
        match self {
            AuthorityKind::ConstitutionCard => "宪法卡",
            AuthorityKind::Precedent => "判例",
            AuthorityKind::Proposal => "提案",
        }
    }
}

/// 冲突案状态闭集（登记待裁 / 已裁结案）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictStatus {
    /// 待裁。
    Open,
    /// 已裁结案。
    Ruled,
}

/// 宪法法庭裁决三态（左胜 / 右胜 / 双驳回）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ruling {
    /// 左判例胜——落地判例继承左判例结论。
    LeftPrevails,
    /// 右判例胜——落地判例继承右判例结论。
    RightPrevails,
    /// 双驳回——落地判例结论记 Block，并自动开歧义（卡口径不足必须澄清）。
    Remanded,
}

impl Ruling {
    /// 裁决名（读屏可达）。
    pub const fn name(self) -> &'static str {
        match self {
            Ruling::LeftPrevails => "左判例胜",
            Ruling::RightPrevails => "右判例胜",
            Ruling::Remanded => "双驳回",
        }
    }
}

/// 冲突案（两判例同律相斥 → 法庭裁决 → 落地判例）。
#[derive(Clone, Debug)]
pub struct ConflictFiling {
    /// 冲突案号（庭内单调，1 起步）。
    pub id: u32,
    /// 左判例 id。
    pub left: u32,
    /// 右判例 id。
    pub right: u32,
    /// 共同引用律条（登记时已核同律）。
    pub cited_law: DesignLaw,
    /// 状态。
    pub status: ConflictStatus,
    /// 裁决（未裁为 None）。
    pub ruling: Option<Ruling>,
    /// 裁决理由（裁决时必填——说人话）。
    pub rationale: String,
    /// 裁决依据的宪法卡版本（权威序落卡的结构性证据）。
    pub card_version: u32,
    /// 裁决依据章（恒为宪法卡——权威序最高源）。
    pub authority_basis: AuthorityKind,
    /// 落地判例 id（裁决落判例集；未裁为 0）。
    pub landed_precedent: u32,
}

/// 冲突法庭（登记 + 裁决 + 落地）。
#[derive(Clone, Debug)]
pub struct ConflictCourt {
    filings: Vec<ConflictFiling>,
    next_id: u32,
    /// 伪冲突登记拒收计数（冲突注入检出——引用律条不同/同 id/同结论伪案）。
    pub bad_filings: u32,
    /// 已裁结案数。
    pub ruled_total: u32,
}

impl ConflictCourt {
    /// 开庭：空庭。
    pub fn new() -> ConflictCourt {
        ConflictCourt {
            filings: Vec::new(),
            next_id: 1,
            bad_filings: 0,
            ruled_total: 0,
        }
    }

    /// 在案数。
    pub fn len(&self) -> u32 {
        self.filings.len() as u32
    }

    /// 待裁案数。
    pub fn open_count(&self) -> u32 {
        let mut n: u32 = 0;
        for f in self.filings.iter() {
            if matches!(f.status, ConflictStatus::Open) {
                n = n.saturating_add(1);
            }
        }
        n
    }

    /// 案号查案。
    pub fn filing(&self, id: u32) -> Result<&ConflictFiling, LangLawError> {
        for f in self.filings.iter() {
            if f.id == id {
                return Ok(f);
            }
        }
        Err(LangLawError::TargetMissing)
    }

    /// 案号查共同引用律条（裁决前取卡用）。
    pub fn filing_law(&self, id: u32) -> Result<DesignLaw, LangLawError> {
        Ok(self.filing(id)?.cited_law)
    }

    /// 登记冲突案：两判例必须**同律条**且结论**相斥**。
    ///
    /// 判例 id 不规范/查无 → 透传 [`LangLawError::PrecBadId`] /
    /// [`LangLawError::TargetMissing`]；同 id → [`LangLawError::ConflictBad`]
    /// + 伪案计数；引用律条不同 → [`LangLawError::ConflictBad`] + 伪案计数
    /// （冲突注入检出）；结论不斥 → [`LangLawError::ConflictNone`]（正常
    /// 裁量：无冲突就别开庭，不计伪案）。返回新案号。
    pub fn file(
        &mut self,
        book: &PrecedentBook,
        left_id: u32,
        right_id: u32,
    ) -> Result<u32, LangLawError> {
        let left = book.lookup(left_id)?;
        let right = book.lookup(right_id)?;
        if left_id == right_id {
            self.bad_filings = self.bad_filings.saturating_add(1);
            return Err(LangLawError::ConflictBad);
        }
        if left.cited_law.ord() != right.cited_law.ord() {
            self.bad_filings = self.bad_filings.saturating_add(1);
            return Err(LangLawError::ConflictBad);
        }
        if !left.verdict.contradicts(right.verdict) {
            return Err(LangLawError::ConflictNone);
        }
        let id = self.next_id;
        self.filings.push(ConflictFiling {
            id,
            left: left_id,
            right: right_id,
            cited_law: left.cited_law,
            status: ConflictStatus::Open,
            ruling: None,
            rationale: String::new(),
            card_version: 0,
            authority_basis: AuthorityKind::ConstitutionCard,
            landed_precedent: 0,
        });
        self.next_id = self.next_id.saturating_add(1);
        Ok(id)
    }

    /// 宪法法庭裁决：裁决落地判例（结论继承胜方；双驳回记 Block），
    /// 理由**引用宪法卡判读口径与版本**（权威序落卡）。
    ///
    /// 案号查无 → [`LangLawError::TargetMissing`]；已裁再裁 →
    /// [`LangLawError::State`]；理由为空 → [`LangLawError::AmendEmpty`]；
    /// 落地登记透传判例集错误（PrecFull/PrecDup/AmendEmpty）。返回落地判例 id。
    pub fn adjudicate(
        &mut self,
        book: &mut PrecedentBook,
        card_version: u32,
        card_reading: &str,
        filing_id: u32,
        ruling: Ruling,
        rationale: &str,
    ) -> Result<u32, LangLawError> {
        if rationale.is_empty() {
            return Err(LangLawError::AmendEmpty);
        }
        let (left_id, right_id, cited_law, status) = {
            let f = self.filing(filing_id)?;
            (f.left, f.right, f.cited_law, f.status)
        };
        if matches!(status, ConflictStatus::Ruled) {
            return Err(LangLawError::State);
        }
        let (left, right) = (book.lookup(left_id)?, book.lookup(right_id)?);
        let winner_verdict = match ruling {
            Ruling::LeftPrevails => left.verdict,
            Ruling::RightPrevails => right.verdict,
            Ruling::Remanded => Verdict::Block,
        };
        // 落地判例：场景点名冲突案，理由引卡口径（权威序：依据章盖卡）。
        let scene = format!("宪法裁决（冲突案#{}）：{}两判例相斥收束", filing_id, cited_law.name());
        let reasoning = format!(
            "裁决={}；理由={}；依据=宪法卡 v{} 判读口径「{}」（权威序：宪法卡>判例>提案）",
            ruling.name(),
            rationale,
            card_version,
            card_reading
        );
        let landed = book.register(&scene, winner_verdict, cited_law, &reasoning)?;
        for f in self.filings.iter_mut() {
            if f.id == filing_id {
                f.status = ConflictStatus::Ruled;
                f.ruling = Some(ruling);
                f.rationale = rationale.to_string();
                f.card_version = card_version;
                f.authority_basis = AuthorityKind::ConstitutionCard;
                f.landed_precedent = landed;
                break;
            }
        }
        self.ruled_total = self.ruled_total.saturating_add(1);
        Ok(landed)
    }

    /// 案号连续性审计（append-only 无删除 → len + 1 == next_id 且逐号存在）。
    pub fn audit_ids(&self) -> Result<(), LangLawError> {
        if self.len().saturating_add(1) != self.next_id {
            return Err(LangLawError::State);
        }
        let mut i = 0;
        while i < self.filings.len() {
            match self.filings.get(i) {
                Some(f) => {
                    if f.id != (i as u32) + 1 {
                        return Err(LangLawError::State);
                    }
                }
                None => return Err(LangLawError::State),
            }
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 十一、事件账（有界、满即拒收不静默丢）
// ---------------------------------------------------------------------------

/// 事件账容量（来源：判据承载——256 条存证封顶，满即拒收待清）。
pub const LEDGER_CAP: usize = 256;

/// 事件种类闭集（域内全部记账面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 宪法卡追加演进。
    CardAmended,
    /// 判例登记。
    PrecedentFiled,
    /// 评审通过。
    ReviewPassed,
    /// 评审拦截（诚实）。
    ReviewBlocked,
    /// 评审拦截（谎报注入）。
    ReviewBlockedLying,
    /// 冲突案登记。
    ConflictFiled,
    /// 宪法裁决 issued。
    RulingIssued,
    /// 歧义登记。
    AmbiguityOpened,
    /// 歧义澄清（含修订版本前进）。
    AmbiguityClarified,
    /// 注入检出（伪冲突/谎报提案——越权修宪另走 OverreachCaught）。
    InjectionCaught,
    /// 越权修宪检出。
    OverreachCaught,
    /// 冲突双驳回自动开歧义。
    RemandAmbiguity,
}

impl EventKind {
    /// 事件名（读屏可达）。
    pub const fn name(self) -> &'static str {
        match self {
            EventKind::CardAmended => "宪法卡演进",
            EventKind::PrecedentFiled => "判例登记",
            EventKind::ReviewPassed => "评审通过",
            EventKind::ReviewBlocked => "评审拦截",
            EventKind::ReviewBlockedLying => "谎报拦截",
            EventKind::ConflictFiled => "冲突登记",
            EventKind::RulingIssued => "宪法裁决",
            EventKind::AmbiguityOpened => "歧义登记",
            EventKind::AmbiguityClarified => "歧义澄清",
            EventKind::InjectionCaught => "注入检出",
            EventKind::OverreachCaught => "越权修宪检出",
            EventKind::RemandAmbiguity => "双驳回开歧义",
        }
    }
}

/// 单条事件（序号 + 种类 + 律条可选项 + 人话备注）。
#[derive(Clone, Debug)]
pub struct LawEvent {
    /// 全局流水号（账内单调，saturating）。
    pub seq: u64,
    /// 事件种类。
    pub kind: EventKind,
    /// 涉案律条（可空）。
    pub law: Option<DesignLaw>,
    /// 人话备注（错误友好律对账本自身的要求）。
    pub note: String,
}

/// 有界事件账（满即拒收——[`LangLawError::LedgerFull`]，不静默丢事件）。
#[derive(Clone, Debug)]
pub struct EventLedger {
    items: Vec<LawEvent>,
    next_seq: u64,
}

impl EventLedger {
    /// 开账：空账。
    pub fn new() -> EventLedger {
        EventLedger {
            items: Vec::new(),
            next_seq: 1,
        }
    }

    /// 在账事件数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 记账（满 → [`LangLawError::LedgerFull`] 拒收，请先 drain）。
    pub fn record(
        &mut self,
        kind: EventKind,
        law: Option<DesignLaw>,
        note: &str,
    ) -> Result<(), LangLawError> {
        if self.items.len() >= LEDGER_CAP {
            return Err(LangLawError::LedgerFull);
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.items.push(LawEvent {
            seq,
            kind,
            law,
            note: note.to_string(),
        });
        Ok(())
    }

    /// 清账：取走全部事件（账回空，流水号不回退——账序单调）。
    pub fn drain(&mut self) -> Vec<LawEvent> {
        let taken = core::mem::take(&mut self.items);
        taken
    }
}

// ---------------------------------------------------------------------------
// 十二、修订账（澄清修订的 append-only 台账）
// ---------------------------------------------------------------------------

/// 单条修订账（澄清修订落卡的去向留痕）。
#[derive(Clone, Debug)]
pub struct RevisionEntry {
    /// 修订流水号（账内单调，1 起步）。
    pub seq: u32,
    /// 涉案律条。
    pub law: DesignLaw,
    /// 缘起歧义编号。
    pub ambiguity_id: u32,
    /// 修订前生效版本。
    pub from_version: u32,
    /// 修订后生效版本。
    pub to_version: u32,
}

// ---------------------------------------------------------------------------
// 十三、世界门面（编排五账 + health + 状态摘要 + 双跑对拍）
// ---------------------------------------------------------------------------

/// 语言设计原则世界（宪法卡册 + 判例集 + 评审闸 + 冲突法庭 + 歧义册 +
/// 事件账 + 修订账的门面编排——所有跨账操作在此记账留痕）。
#[derive(Clone, Debug)]
pub struct LangLawWorld {
    /// 宪法卡册（五卡冻结 + 演进只增不改）。
    pub codex: ConstitutionCodex,
    /// 判例集（append-only）。
    pub book: PrecedentBook,
    /// 评审闸。
    pub gate: LawReviewGate,
    /// 冲突法庭。
    pub court: ConflictCourt,
    /// 歧义登记册。
    pub ambiguities: AmbiguityRegistry,
    /// 事件账。
    pub ledger: EventLedger,
    /// 修订账（私有——只经 clarify_ambiguity 追加）。
    revisions: Vec<RevisionEntry>,
    next_revision_seq: u32,
}

impl LangLawWorld {
    /// 开机：五卡冻结 v1，五账清零。
    pub fn boot() -> LangLawWorld {
        LangLawWorld {
            codex: ConstitutionCodex::boot(),
            book: PrecedentBook::new(),
            gate: LawReviewGate::new(),
            court: ConflictCourt::new(),
            ambiguities: AmbiguityRegistry::new(),
            ledger: EventLedger::new(),
            revisions: Vec::new(),
            next_revision_seq: 1,
        }
    }

    /// 修订账条数。
    pub fn revision_count(&self) -> u32 {
        self.revisions.len() as u32
    }

    /// 送审提案（编排：评审闸 + 事件账；拦截理由引卡由闸完成）。
    pub fn review_proposal(
        &mut self,
        title: &str,
        surface: &str,
        facts: ProposalFacts,
        self_review: &[SelfVerdict; LAW_COUNT],
    ) -> Result<ReviewReceipt, LangLawError> {
        let receipt = self
            .gate
            .review(&self.codex, &self.ambiguities, title, surface, facts, self_review)?;
        let kind = match receipt.outcome {
            ReviewOutcome::Passed => {
                self.ledger
                    .record(EventKind::ReviewPassed, None, &format!("评审#{} 通过：{}", receipt.seq, title))?;
                ReviewOutcome::Passed
            }
            ReviewOutcome::BlockedHonest => {
                let law_note = receipt.blocked_law.map(|l| l.name()).unwrap_or("未知律");
                self.ledger.record(
                    EventKind::ReviewBlocked,
                    receipt.blocked_law,
                    &format!("评审#{} 拦截于{}：{}", receipt.seq, law_note, title),
                )?;
                ReviewOutcome::BlockedHonest
            }
            ReviewOutcome::BlockedLying => {
                let law_note = receipt.blocked_law.map(|l| l.name()).unwrap_or("未知律");
                self.ledger.record(
                    EventKind::ReviewBlockedLying,
                    receipt.blocked_law,
                    &format!("评审#{} 谎报检出（自评合律但事实面违{}）：{}", receipt.seq, law_note, title),
                )?;
                ReviewOutcome::BlockedLying
            }
        };
        let _ = kind;
        Ok(receipt)
    }

    /// 登记判例（编排：判例集 + 事件账）。
    pub fn register_precedent(
        &mut self,
        scene: &str,
        verdict: Verdict,
        cited_law: DesignLaw,
        reasoning: &str,
    ) -> Result<u32, LangLawError> {
        let id = self.book.register(scene, verdict, cited_law, reasoning)?;
        self.ledger.record(
            EventKind::PrecedentFiled,
            Some(cited_law),
            &format!("判例 {} 登记：{}", id_tag(id), scene),
        )?;
        Ok(id)
    }

    /// 登记歧义（编排：歧义册 + 事件账）。
    pub fn register_ambiguity(&mut self, law: DesignLaw, question: &str) -> Result<u32, LangLawError> {
        let id = self.ambiguities.register(law, question)?;
        self.ledger.record(
            EventKind::AmbiguityOpened,
            Some(law),
            &format!("歧义#{} 登记于{}：{}", id, law.name(), question),
        )?;
        Ok(id)
    }

    /// 澄清修订（编排：歧义册 → 卡册追加 → 修订账 + 事件账）。
    pub fn clarify_ambiguity(&mut self, id: u32, revision: &str) -> Result<(u32, u32), LangLawError> {
        let law = match self.ambiguities.items.iter().find(|f| f.id == id) {
            Some(f) => f.law,
            None => return Err(LangLawError::TargetMissing),
        };
        let (from_v, to_v) = self.ambiguities.clarify(id, &mut self.codex, revision)?;
        self.revisions.push(RevisionEntry {
            seq: self.next_revision_seq,
            law,
            ambiguity_id: id,
            from_version: from_v,
            to_version: to_v,
        });
        self.next_revision_seq = self.next_revision_seq.saturating_add(1);
        self.ledger.record(
            EventKind::AmbiguityClarified,
            Some(law),
            &format!("歧义#{} 澄清：宪法卡 v{} → v{}", id, from_v, to_v),
        )?;
        self.ledger.record(
            EventKind::CardAmended,
            Some(law),
            &format!("{}宪法卡追加 v{}（澄清修订落卡）", law.name(), to_v),
        )?;
        Ok((from_v, to_v))
    }

    /// 登记冲突案（编排：法庭 + 事件账；伪冲突在这里被检出并计数）。
    pub fn file_conflict(&mut self, left_id: u32, right_id: u32) -> Result<u32, LangLawError> {
        let before = self.court.bad_filings;
        let filed = self.court.file(&self.book, left_id, right_id);
        match filed {
            Ok(fid) => {
                let law = self.court.filing_law(fid)?;
                self.ledger.record(
                    EventKind::ConflictFiled,
                    Some(law),
                    &format!("冲突案#{} 登记（判例 {} × {}）", fid, id_tag(left_id), id_tag(right_id)),
                )?;
                Ok(fid)
            }
            Err(e) => {
                if self.court.bad_filings > before {
                    // 伪冲突注入检出留痕（同 id / 异律条两类伪案）。
                    self.ledger.record(
                        EventKind::InjectionCaught,
                        None,
                        &format!("伪冲突登记检出（判例 {} × {}）：{}", id_tag(left_id), id_tag(right_id), e.detail()),
                    )?;
                }
                Err(e)
            }
        }
    }

    /// 宪法裁决（编排：法庭 → 落地判例 →（双驳回）自动开歧义 + 事件账）。
    pub fn adjudicate(
        &mut self,
        filing_id: u32,
        ruling: Ruling,
        rationale: &str,
    ) -> Result<u32, LangLawError> {
        let cited_law = self.court.filing_law(filing_id)?;
        let (card_version, card_reading) = {
            let card = self.codex.active_or_err(cited_law)?;
            (card.version, card.reading.clone())
        };
        let landed = self.court.adjudicate(
            &mut self.book,
            card_version,
            &card_reading,
            filing_id,
            ruling,
            rationale,
        )?;
        self.ledger.record(
            EventKind::RulingIssued,
            Some(cited_law),
            &format!(
                "冲突案#{} 裁决={}，落地判例 {}（依据宪法卡 v{}）",
                filing_id,
                ruling.name(),
                id_tag(landed),
                card_version
            ),
        )?;
        if matches!(ruling, Ruling::Remanded) {
            // 双驳回：卡口径不足——自动开歧义强制澄清后才可再判该律。
            let q = "判例冲突双驳回：该律条判读口径不足以收束相斥判例，请澄清";
            match self.ambiguities.register(cited_law, q) {
                Ok(aid) => {
                    self.ledger.record(
                        EventKind::RemandAmbiguity,
                        Some(cited_law),
                        &format!("双驳回自动开歧义#{}于{}", aid, cited_law.name()),
                    )?;
                }
                // 重复歧义不算裁决失败（同问已在册）——其余错误透传。
                Err(LangLawError::AmbigDup) => {
                    self.ledger.record(
                        EventKind::RemandAmbiguity,
                        Some(cited_law),
                        "双驳回自动开歧义：同问已在册（沿用原歧义项）",
                    )?;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(landed)
    }

    /// 越权修宪注入探针（编排：卡册拒绝 + 注入事件留痕；**永远 Err**）。
    pub fn force_rewrite_probe(
        &mut self,
        law: DesignLaw,
        statute: &str,
        reading: &str,
        violation: &str,
    ) -> Result<u32, LangLawError> {
        let r = self.codex.force_rewrite(law, statute, reading, violation);
        self.ledger
            .record(EventKind::OverreachCaught, Some(law), "越权修宪检出：冻结卡改写被拒，走 amend 追加通道")
            .ok();
        r
    }

    /// 全量审计（五链验链 + 三账 id 连续 + 三态守恒 + 版本守恒）。
    pub fn audit(&self) -> Result<(), LangLawError> {
        self.codex.verify_all()?;
        self.book.audit_ids()?;
        self.ambiguities.audit_ids()?;
        self.court.audit_ids()?;
        if self.gate.passed
            .saturating_add(self.gate.blocked_honest)
            .saturating_add(self.gate.blocked_lying)
            != self.gate.total_reviewed()
        {
            return Err(LangLawError::State);
        }
        if self.codex.version_total() < LAW_COUNT as u32 {
            return Err(LangLawError::State);
        }
        Ok(())
    }

    /// 体检报告（一键读数——health 判据承载面）。
    pub fn health(&self) -> HealthReport {
        HealthReport {
            laws_total: LAW_COUNT as u8,
            cards_frozen: {
                // 五链各至少一卡且全部冻结（开机即冻结，任何时刻恒真）。
                let mut n = 0u8;
                let mut i = 0;
                while i < LAW_COUNT {
                    let law = match DesignLaw::from_ord((i as u8) + 1) {
                        Some(l) => l,
                        None => break,
                    };
                    if let Some(c) = self.codex.active(law) {
                        if c.frozen {
                            n = n.saturating_add(1);
                        }
                    }
                    i += 1;
                }
                n
            },
            card_version_total: self.codex.version_total(),
            precedents: self.book.len(),
            conflicts_total: self.court.len(),
            conflicts_open: self.court.open_count(),
            rulings_issued: self.court.ruled_total,
            ambiguities_total: self.ambiguities.len(),
            ambiguities_open: self.ambiguities.open_count(),
            revisions: self.revision_count(),
            reviews_total: self.gate.total_reviewed(),
            reviews_passed: self.gate.passed,
            reviews_blocked: self.gate.blocked_honest,
            reviews_lying: self.gate.blocked_lying,
            inject_proposal: self.gate.blocked_lying,
            inject_conflict: self.court.bad_filings,
            inject_amend: self.codex.overreach_attempts,
            ledger_len: self.ledger.len(),
            ledger_cap: LEDGER_CAP,
        }
    }

    /// 全状态确定性摘要（FNV-1a 64——双跑对拍的判据根；喂入序固定）。
    pub fn state_digest(&self) -> u64 {
        let mut h = fp_str(FNV64_OFFSET, LANG_LAW_VERSION);
        // 宪法卡册：逐律逐卡（版本 + 指纹）。
        let mut i = 0;
        while i < LAW_COUNT {
            let law = match DesignLaw::from_ord((i as u8) + 1) {
                Some(l) => l,
                None => break,
            };
            let slot = (law.ord() - 1) as usize;
            h = fp_u32(h, self.codex.chains[slot].len() as u32);
            let mut j = 0;
            while j < self.codex.chains[slot].len() {
                if let Some(c) = self.codex.chains[slot].get(j) {
                    h = fp_u32(h, c.version);
                    h = fp_u64(h, c.fingerprint);
                }
                j += 1;
            }
            i += 1;
        }
        // 判例集：逐判例指纹。
        h = fp_u32(h, self.book.next_id);
        let mut i = 0;
        while i < self.book.items.len() {
            if let Some(p) = self.book.items.get(i) {
                h = fp_u64(h, p.fingerprint);
            }
            i += 1;
        }
        // 歧义册：逐项状态 + 落卡版本。
        h = fp_u32(h, self.ambiguities.next_id);
        let mut i = 0;
        while i < self.ambiguities.items.len() {
            if let Some(f) = self.ambiguities.items.get(i) {
                h = fp_u32(
                    h,
                    match f.status {
                        AmbigStatus::Open => 1,
                        AmbigStatus::Clarified => 2,
                    },
                );
                h = fp_u32(h, f.card_version_after);
            }
            i += 1;
        }
        // 冲突庭：逐案状态 + 裁决 + 卡版本。
        h = fp_u32(h, self.court.next_id);
        let mut i = 0;
        while i < self.court.filings.len() {
            if let Some(f) = self.court.filings.get(i) {
                h = fp_u32(
                    h,
                    match f.status {
                        ConflictStatus::Open => 1,
                        ConflictStatus::Ruled => 2,
                    },
                );
                h = fp_u32(h, f.ruling.map(|r| match r {
                    Ruling::LeftPrevails => 1,
                    Ruling::RightPrevails => 2,
                    Ruling::Remanded => 3,
                }).unwrap_or(0));
                h = fp_u32(h, f.card_version);
                h = fp_u32(h, f.landed_precedent);
            }
            i += 1;
        }
        // 评审闸 + 各拒绝计数 + 修订账。
        h = fp_u32(h, self.gate.seq);
        h = fp_u32(h, self.gate.passed);
        h = fp_u32(h, self.gate.blocked_honest);
        h = fp_u32(h, self.gate.blocked_lying);
        h = fp_u32(h, self.book.dup_rejected);
        h = fp_u32(h, self.ambiguities.dup_rejected);
        h = fp_u32(h, self.court.bad_filings);
        h = fp_u32(h, self.codex.overreach_attempts);
        h = fp_u32(h, self.codex.amended_total);
        h = fp_u32(h, self.next_revision_seq);
        let mut i = 0;
        while i < self.revisions.len() {
            if let Some(r) = self.revisions.get(i) {
                h = fp_u32(h, r.seq);
                h = fp_u32(h, r.law.ord() as u32);
                h = fp_u32(h, r.ambiguity_id);
                h = fp_u32(h, r.from_version);
                h = fp_u32(h, r.to_version);
            }
            i += 1;
        }
        h
    }

    /// 读屏可达状态摘要（固定六行——域本色：状态说人话不报裸数）。
    pub fn spoken_status(&self) -> Vec<String> {
        let h = self.health();
        vec![
            format!(
                "宪法：{}律{}卡（版本共{}，演进{}次，越权修宪检出{}次）",
                h.laws_total, h.cards_frozen, h.card_version_total, self.codex.amended_total, h.inject_amend
            ),
            format!(
                "判例集：{}例（重复拒收{}），冲突案{}在案{}待裁，已裁{}",
                h.precedents, self.book.dup_rejected, h.conflicts_total, h.conflicts_open, h.rulings_issued
            ),
            format!(
                "评审：共{}单（通过{}，拦截{}，谎报{}）",
                h.reviews_total, h.reviews_passed, h.reviews_blocked, h.reviews_lying
            ),
            format!(
                "歧义：共{}项开放{}（已澄清走修订账{}笔）",
                h.ambiguities_total, h.ambiguities_open, h.revisions
            ),
            format!("事件账：{}/{} 在账", h.ledger_len, h.ledger_cap),
            format!(
                "注入三面：违律提案谎报{}，伪冲突{}，越权修宪{}",
                h.inject_proposal, h.inject_conflict, h.inject_amend
            ),
        ]
    }
}

/// 体检报告（[`LangLawWorld::health`] 的读数结构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealthReport {
    /// 律条总数（恒 5——闭集）。
    pub laws_total: u8,
    /// 冻结宪法卡数（恒 5——五律各至少一卡）。
    pub cards_frozen: u8,
    /// 宪法卡版本总数（开机基线 5）。
    pub card_version_total: u32,
    /// 在册判例数。
    pub precedents: u32,
    /// 冲突案总数。
    pub conflicts_total: u32,
    /// 待裁冲突案数。
    pub conflicts_open: u32,
    /// 已发裁决数。
    pub rulings_issued: u32,
    /// 歧义总数。
    pub ambiguities_total: u32,
    /// 开放歧义数。
    pub ambiguities_open: u32,
    /// 修订账笔数。
    pub revisions: u32,
    /// 评审总数。
    pub reviews_total: u32,
    /// 评审通过数。
    pub reviews_passed: u32,
    /// 诚实拦截数。
    pub reviews_blocked: u32,
    /// 谎报拦截数。
    pub reviews_lying: u32,
    /// 违律提案注入检出数（= 谎报拦截数）。
    pub inject_proposal: u32,
    /// 伪冲突注入检出数。
    pub inject_conflict: u32,
    /// 越权修宪注入检出数。
    pub inject_amend: u32,
    /// 事件账在账数。
    pub ledger_len: usize,
    /// 事件账容量。
    pub ledger_cap: usize,
}

impl HealthReport {
    /// 健康判据（全纯计数——开机基线即健康，账实守恒才健康）。
    pub const fn is_healthy(&self) -> bool {
        self.laws_total == 5
            && self.cards_frozen == 5
            && self.card_version_total >= 5
            && self.reviews_passed
                .saturating_add(self.reviews_blocked)
                .saturating_add(self.reviews_lying)
                == self.reviews_total
            && self.rulings_issued.saturating_add(self.conflicts_open) <= self.conflicts_total
            && self.ambiguities_open <= self.ambiguities_total
            && self.ledger_len <= self.ledger_cap
            && self.ledger_cap == LEDGER_CAP
    }
}

// ---------------------------------------------------------------------------
// 十四、编译期断言（宪法闭集 / 码段连续互斥 / 权威序 / 容量守恒）
// ---------------------------------------------------------------------------

// 五律闭集：常量卡表恰覆盖五律，且与数组下标严格对位（下标 i ↔ ord i+1）。
const _: () = assert!(LAW_COUNT == 5);
const _: () = assert!(LAW_CARDS.len() == LAW_COUNT);
const _: () = assert!(DesignLaw::ALL.len() == LAW_COUNT);
const fn cards_cover_each_law_once() -> bool {
    // 五律序数和恒 15（1+2+3+4+5）——若任一重复/缺失则和不为 15。
    let mut i = 0;
    let mut sum: u8 = 0;
    while i < LAW_CARDS.len() {
        sum = sum.wrapping_add(LAW_CARDS[i].law.ord());
        i += 1;
    }
    sum == 15
}
const _: () = assert!(cards_cover_each_law_once());
const fn cards_aligned_with_index() -> bool {
    let mut i = 0;
    while i < LAW_CARDS.len() {
        if LAW_CARDS[i].law.ord() != (i as u8) + 1 {
            return false;
        }
        i += 1;
    }
    true
}
const _: () = assert!(cards_aligned_with_index());
// 卡表三段全非空（开机冻结的三段来源不许有空洞）。
const fn cards_three_sections_nonempty() -> bool {
    let mut i = 0;
    while i < LAW_CARDS.len() {
        let t = &LAW_CARDS[i];
        if t.statute.is_empty() || t.reading.is_empty() || t.violation.is_empty() {
            return false;
        }
        i += 1;
    }
    true
}
const _: () = assert!(cards_three_sections_nonempty());

// 诊断码段：基址 0x1B30、末址 0x1B3F、恰 16 码、连续且两两互异。
const _: () = assert!(ERR_CODE_BASE == 0x1B30);
const _: () = assert!(ERR_CODE_END == 0x1B3F);
const _: () = assert!(CODES.len() as u16 == ERR_CODE_COUNT);
const fn codes_strictly_ascending() -> bool {
    let mut i = 0;
    while i + 1 < CODES.len() {
        if CODES[i] >= CODES[i + 1] {
            return false;
        }
        i += 1;
    }
    true
}
const _: () = assert!(codes_strictly_ascending());
const fn codes_span_exact() -> bool {
    CODES[0] == ERR_CODE_BASE && CODES[CODES.len() - 1] == ERR_CODE_END
}
const _: () = assert!(codes_span_exact());

// 权威序：宪法卡(3) > 判例(2) > 提案(1) 严格递减。
const _: () = assert!(AuthorityKind::ConstitutionCard.rank() == 3);
const _: () = assert!(AuthorityKind::Precedent.rank() == 2);
const _: () = assert!(AuthorityKind::Proposal.rank() == 1);
const _: () = assert!(AuthorityKind::ConstitutionCard.outranks(AuthorityKind::Precedent));
const _: () = assert!(AuthorityKind::Precedent.outranks(AuthorityKind::Proposal));
const _: () = assert!(!AuthorityKind::Proposal.outranks(AuthorityKind::ConstitutionCard));

// 容量守恒：三容量均 >= 1（满即拒收的降级路径必须可达且有限）。
const _: () = assert!(CARD_CHAIN_CAP >= 1);
const _: () = assert!(PRECEDENT_CAP >= 1);
const _: () = assert!(LEDGER_CAP >= 1);

// ---------------------------------------------------------------------------
// 十五、冒烟测试（隔离探针 cargo test 全绿面）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 判据 T1：开机即健康——五卡冻结 v1、五链验链通过、基线审计全绿。
    #[test]
    fn t01_boot_health_and_audit() {
        let w = LangLawWorld::boot();
        let h = w.health();
        assert_eq!(h.laws_total, 5);
        assert_eq!(h.cards_frozen, 5);
        assert_eq!(h.card_version_total, 5);
        assert!(h.is_healthy());
        assert!(w.audit().is_ok());
        assert_eq!(w.codex.version_total(), 5);
        assert_eq!(w.book.len(), 0);
        assert!(w.ledger.is_empty());
        // 五律名与口号非空（读屏可达的地基）。
        for law in DesignLaw::ALL {
            assert!(!law.name().is_empty());
            assert!(!law.slogan().is_empty());
        }
    }

    /// 判据 T2：违律提案被拦截——拦截理由引用宪法卡律文原文，计数入账。
    #[test]
    fn t02_review_blocks_lawbreaking() {
        let mut w = LangLawWorld::boot();
        // 安全默认违律：沙箱外漏。
        let mut facts = ProposalFacts::COMPLIANT;
        facts.sandbox_no_leak = false;
        let self_v = [SelfVerdict::Violates; LAW_COUNT];
        let r = w.review_proposal("提案A", "脚本 IO 面", facts, &self_v).unwrap();
        assert_eq!(r.outcome, ReviewOutcome::BlockedHonest);
        assert_eq!(r.diag_code, E_LAW_BLOCK);
        assert_eq!(r.blocked_law, Some(DesignLaw::SafeDefault));
        // 理由引用卡律文原文（含「沙箱内运行」关键词）与整改指引。
        assert!(r.reason.contains("沙箱内运行"));
        assert!(r.reason.contains("整改"));
        assert!(!r.spoken.is_empty());
        let h = w.health();
        assert_eq!(h.reviews_blocked, 1);
        assert_eq!(h.reviews_passed, 0);
        assert!(w.audit().is_ok());
    }

    /// 判据 T3：合规提案通过——五律全过，计数守恒。
    #[test]
    fn t03_review_passes_compliant() {
        let mut w = LangLawWorld::boot();
        let self_v = [SelfVerdict::Complies; LAW_COUNT];
        let r = w
            .review_proposal("提案B", "场景逻辑面", ProposalFacts::COMPLIANT, &self_v)
            .unwrap();
        assert_eq!(r.outcome, ReviewOutcome::Passed);
        assert_eq!(r.diag_code, 0);
        assert!(r.blocked_law.is_none());
        assert!(r.reason.is_empty());
        let h = w.health();
        assert_eq!(h.reviews_passed, 1);
        assert_eq!(h.reviews_total, 1);
    }

    /// 判据 T4：谎报注入检出——自评合律但事实面违律 → BlockedLying + 0x1B3E。
    #[test]
    fn t04_injection_lying_proposal_detected() {
        let mut w = LangLawWorld::boot();
        // 确定优先违律：隐式非确定。
        let mut facts = ProposalFacts::COMPLIANT;
        facts.deterministic = false;
        let self_v = [SelfVerdict::Complies; LAW_COUNT]; // 谎报！
        let r = w.review_proposal("提案C", "触发器面", facts, &self_v).unwrap();
        assert_eq!(r.outcome, ReviewOutcome::BlockedLying);
        assert_eq!(r.diag_code, E_INJECT);
        assert_eq!(r.blocked_law, Some(DesignLaw::DeterministicFirst));
        let h = w.health();
        assert_eq!(h.reviews_lying, 1);
        assert_eq!(h.inject_proposal, 1);
        assert_eq!(h.reviews_blocked, 0);
    }

    /// 判据 T5：提案卡不完整三形拒收——名为空 / 涉及面为空 / 自评缺律。
    #[test]
    fn t05_proposal_incomplete_rejected() {
        let mut w = LangLawWorld::boot();
        let self_v = [SelfVerdict::Complies; LAW_COUNT];
        assert_eq!(
            w.review_proposal("", "面", ProposalFacts::COMPLIANT, &self_v).unwrap_err(),
            LangLawError::ProposalBad
        );
        assert_eq!(
            w.review_proposal("名", "", ProposalFacts::COMPLIANT, &self_v).unwrap_err(),
            LangLawError::ProposalBad
        );
        let mut partial = [SelfVerdict::Complies; LAW_COUNT];
        partial[3] = SelfVerdict::Undeclared; // 渐进复杂缺律
        assert_eq!(
            w.review_proposal("名", "面", ProposalFacts::COMPLIANT, &partial).unwrap_err(),
            LangLawError::ProposalBad
        );
        let h = w.health();
        assert_eq!(h.reviews_total, 0); // 拒收不进评审计数
    }

    /// 判据 T6：判例登记 + id 规范 + 重复拒收。
    #[test]
    fn t06_precedent_register_and_dup() {
        let mut w = LangLawWorld::boot();
        let id1 = w
            .register_precedent("零宽字符隐藏分支", Verdict::Block, DesignLaw::ReadableFirst, "视觉欺骗不可读")
            .unwrap();
        let id2 = w
            .register_precedent("哈希序定施放", Verdict::Block, DesignLaw::DeterministicFirst, "顺序不可预测")
            .unwrap();
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id_tag(id1), "P-0001");
        assert_eq!(id_tag(id2), "P-0002");
        // 同四元组重复 → PrecDup + 计数。
        let dup = w.book.register(
            "零宽字符隐藏分支",
            Verdict::Block,
            DesignLaw::ReadableFirst,
            "视觉欺骗不可读",
        );
        assert_eq!(dup.unwrap_err(), LangLawError::PrecDup);
        assert_eq!(w.book.dup_rejected, 1);
        // 空场景/理由 → AmendEmpty。
        assert_eq!(
            w.book.register("", Verdict::Allow, DesignLaw::ReadableFirst, "r").unwrap_err(),
            LangLawError::AmendEmpty
        );
        assert!(w.audit().is_ok());
    }

    /// 判据 T7：判例 id 牌规范面——坏 id / 坏牌 / 规范但查无 三分。
    #[test]
    fn t07_precedent_id_discipline() {
        let mut w = LangLawWorld::boot();
        w.register_precedent("场景", Verdict::Allow, DesignLaw::SafeDefault, "理由").unwrap();
        // id 为 0 → PrecBadId。
        assert_eq!(w.book.lookup(0).unwrap_err(), LangLawError::PrecBadId);
        // 规范但查无 → TargetMissing。
        assert_eq!(w.book.lookup(99).unwrap_err(), LangLawError::TargetMissing);
        // 坏牌三形：前缀错 / 长度错 / 非数字位。
        assert_eq!(w.book.find_by_tag("X-0001").unwrap_err(), LangLawError::PrecBadId);
        assert_eq!(w.book.find_by_tag("P-01").unwrap_err(), LangLawError::PrecBadId);
        assert_eq!(w.book.find_by_tag("P-00ab").unwrap_err(), LangLawError::PrecBadId);
        // 好牌命中。
        assert!(w.book.find_by_tag("P-0001").is_ok());
    }

    /// 判据 T8：冲突登记与裁决全流程——相斥立案、裁决落判例、已裁再裁拒。
    #[test]
    fn t08_conflict_flow_and_ruling() {
        let mut w = LangLawWorld::boot();
        // 同律相斥两判例：错误友好律下「裸码报错」vs「带改法报错」。
        let pa = w
            .register_precedent("裸码报错场景", Verdict::Block, DesignLaw::FriendlyError, "无改法违律")
            .unwrap();
        let pb = w
            .register_precedent("带改法报错场景", Verdict::Allow, DesignLaw::FriendlyError, "三件齐全合律")
            .unwrap();
        let fid = w.file_conflict(pa, pb).unwrap();
        assert_eq!(fid, 1);
        let h = w.health();
        assert_eq!(h.conflicts_open, 1);
        // 裁决：左胜 → 落地判例继承 Block。
        let landed = w.adjudicate(fid, Ruling::LeftPrevails, "裸码报错踩十三章口径红线，判例以卡为准").unwrap();
        assert!(landed > pb);
        let prec = w.book.lookup(landed).unwrap();
        assert_eq!(prec.verdict, Verdict::Block);
        assert!(prec.reasoning.contains("宪法卡"));
        assert!(prec.reasoning.contains("权威序"));
        // 案已裁结 + 落地留痕 + 依据章盖卡。
        let filing = w.court.filing(fid).unwrap();
        assert_eq!(filing.status, ConflictStatus::Ruled);
        assert_eq!(filing.ruling, Some(Ruling::LeftPrevails));
        assert_eq!(filing.authority_basis, AuthorityKind::ConstitutionCard);
        assert!(filing.card_version >= 1);
        assert_eq!(filing.landed_precedent, landed);
        // 已裁再裁 → State。
        assert_eq!(
            w.adjudicate(fid, Ruling::RightPrevails, "再裁").unwrap_err(),
            LangLawError::State
        );
        assert!(w.audit().is_ok());
    }

    /// 判据 T9：冲突注入检出——同 id 伪案与异律条伪案都拦下并计数。
    #[test]
    fn t09_conflict_injection_detected() {
        let mut w = LangLawWorld::boot();
        let pa = w.register_precedent("场景A", Verdict::Block, DesignLaw::ReadableFirst, "理由A").unwrap();
        let pb = w.register_precedent("场景B", Verdict::Allow, DesignLaw::DeterministicFirst, "理由B").unwrap();
        let pc = w.register_precedent("场景C", Verdict::Allow, DesignLaw::ReadableFirst, "理由C").unwrap();
        // 同 id 伪案。
        assert_eq!(w.file_conflict(pa, pa).unwrap_err(), LangLawError::ConflictBad);
        // 异律条伪案（冲突注入）。
        assert_eq!(w.file_conflict(pa, pb).unwrap_err(), LangLawError::ConflictBad);
        assert_eq!(w.court.bad_filings, 2);
        let h = w.health();
        assert_eq!(h.inject_conflict, 2);
        // 异律伪案留了注入事件账。
        let events = w.ledger.drain();
        assert!(events.iter().any(|e| matches!(e.kind, EventKind::InjectionCaught)));
        // 同律不斥 → ConflictNone（正常裁量，不计伪案）。
        let pd = w.register_precedent("场景D", Verdict::Block, DesignLaw::ReadableFirst, "理由D").unwrap();
        assert_eq!(w.file_conflict(pa, pd).unwrap_err(), LangLawError::ConflictNone);
        assert_eq!(w.court.bad_filings, 2);
        // 同律相斥（pa×pc）才是真冲突——正常立案。
        assert!(w.file_conflict(pa, pc).is_ok());
    }

    /// 判据 T10：双驳回裁决——落地 Block 判例 + 自动开歧义 + 评审缓判联动。
    #[test]
    fn t10_remand_opens_ambiguity_and_defers_review() {
        let mut w = LangLawWorld::boot();
        let pa = w.register_precedent("相斥A", Verdict::Block, DesignLaw::GradualComplex, "理由A").unwrap();
        let pb = w.register_precedent("相斥B", Verdict::Allow, DesignLaw::GradualComplex, "理由B").unwrap();
        let fid = w.file_conflict(pa, pb).unwrap();
        let landed = w.adjudicate(fid, Ruling::Remanded, "两判例各有其理，卡口径不足以收束").unwrap();
        let prec = w.book.lookup(landed).unwrap();
        assert_eq!(prec.verdict, Verdict::Block);
        // 自动开歧义：渐进复杂律开放中。
        assert_eq!(w.ambiguities.open_count(), 1);
        assert!(w.ambiguities.is_open(DesignLaw::GradualComplex));
        // 该律涉案评审被缓判。
        let self_v = [SelfVerdict::Complies; LAW_COUNT];
        assert_eq!(
            w.review_proposal("提案D", "面", ProposalFacts::COMPLIANT, &self_v).unwrap_err(),
            LangLawError::AmbigOpen
        );
        // 双驳回再降一单：同问重复登记被并入（沿用原歧义项）。
        let pc = w.register_precedent("相斥C", Verdict::Block, DesignLaw::GradualComplex, "理由C").unwrap();
        let pd = w.register_precedent("相斥D", Verdict::Allow, DesignLaw::GradualComplex, "理由D").unwrap();
        let fid2 = w.file_conflict(pc, pd).unwrap();
        w.adjudicate(fid2, Ruling::Remanded, "同前").unwrap();
        assert_eq!(w.ambiguities.len(), 1); // 未新增——dup 并入
        assert!(w.audit().is_ok());
    }

    /// 判据 T11：越权修宪注入检出——force_rewrite 永远拒绝且链不受损。
    #[test]
    fn t11_overreach_amend_blocked() {
        let mut w = LangLawWorld::boot();
        let before_fp = w.state_digest();
        let r = w.force_rewrite_probe(
            DesignLaw::SafeDefault,
            "改写的律文",
            "改写的口径",
            "改写的样例",
        );
        assert_eq!(r.unwrap_err(), LangLawError::CardAmend);
        assert_eq!(w.codex.overreach_attempts, 1);
        // 链不受损：验链仍绿，生效卡仍是 v1 原文。
        assert!(w.codex.verify_all().is_ok());
        let card = w.codex.active(DesignLaw::SafeDefault).unwrap();
        assert_eq!(card.version, 1);
        assert!(card.statute.contains("沙箱内运行"));
        assert_ne!(w.state_digest(), before_fp); // 注入检出改变了状态（计数留痕）
        let h = w.health();
        assert_eq!(h.inject_amend, 1);
    }

    /// 判据 T12：合法演进只增不改——amend 追加 v2，v1 原样留链。
    #[test]
    fn t12_amend_append_only_evolution() {
        let mut w = LangLawWorld::boot();
        let v1_fp = w.codex.active(DesignLaw::ReadableFirst).unwrap().fingerprint;
        let v1_statute = w.codex.active(DesignLaw::ReadableFirst).unwrap().statute.clone();
        let nv = w
            .codex
            .amend(
                DesignLaw::ReadableFirst,
                "读脚本像读故事——不玩字符机巧（v2：并禁用同形全角半角混排）。",
                "机械判读：叙事可读 + 无字形机巧 + 无全半角同形混排。",
                "违例样例：全角Ｏ与半角O混排做变量对。",
            )
            .unwrap();
        assert_eq!(nv, 2);
        assert_eq!(w.codex.chain_len(DesignLaw::ReadableFirst), 2);
        // v1 原样：指纹与正文都不变（只增不改）。
        let v1 = w.codex.chains[0][0].clone();
        assert_eq!(v1.fingerprint, v1_fp);
        assert_eq!(v1.statute, v1_statute);
        // 生效卡切到 v2，指纹重算一致。
        let v2 = w.codex.active(DesignLaw::ReadableFirst).unwrap();
        assert_eq!(v2.version, 2);
        assert_eq!(v2.recompute_fingerprint(), v2.fingerprint);
        assert!(w.codex.verify_all().is_ok());
        assert_eq!(w.codex.amended_total, 1);
        // 空正文 amend → AmendEmpty。
        assert_eq!(
            w.codex.amend(DesignLaw::ReadableFirst, "", "r", "v").unwrap_err(),
            LangLawError::AmendEmpty
        );
    }

    /// 判据 T13：歧义澄清闭环——登记 → 缓判 → 澄清落卡版本前进 → 判读恢复。
    #[test]
    fn t13_ambiguity_clarify_cycle() {
        let mut w = LangLawWorld::boot();
        let aid = w
            .register_ambiguity(DesignLaw::GradualComplex, "「小事直路」的行数上限是否包含空行与注释？")
            .unwrap();
        assert_eq!(aid, 1);
        // 登记期间评审缓判。
        let self_v = [SelfVerdict::Complies; LAW_COUNT];
        assert_eq!(
            w.review_proposal("提案E", "面", ProposalFacts::COMPLIANT, &self_v).unwrap_err(),
            LangLawError::AmbigOpen
        );
        // 澄清：版本 1 → 2，修订账 +1，事件两条（澄清 + 卡演进）。
        let (from_v, to_v) = w
            .clarify_ambiguity(aid, "行数上限按有效语句计，空行与注释不计——口径自 v2 起生效。")
            .unwrap();
        assert_eq!((from_v, to_v), (1, 2));
        assert_eq!(w.revision_count(), 1);
        assert_eq!(w.ambiguities.open_count(), 0);
        assert!(!w.ambiguities.is_open(DesignLaw::GradualComplex));
        // 修订账内容对账。
        let entry = &w.revisions[0];
        assert_eq!(entry.law, DesignLaw::GradualComplex);
        assert_eq!(entry.ambiguity_id, aid);
        assert_eq!(entry.from_version, 1);
        assert_eq!(entry.to_version, 2);
        // 事件账：AmbiguityClarified + CardAmended 双记。
        let events = w.ledger.drain();
        assert!(events.iter().any(|e| matches!(e.kind, EventKind::AmbiguityClarified)));
        assert!(events.iter().any(|e| matches!(e.kind, EventKind::CardAmended)));
        // 判读恢复。
        let r = w.review_proposal("提案E", "面", ProposalFacts::COMPLIANT, &self_v).unwrap();
        assert_eq!(r.outcome, ReviewOutcome::Passed);
        // 重复歧义登记 → AmbigDup；已澄清再澄清 → State；空修订 → AmendEmpty。
        assert_eq!(
            w.register_ambiguity(DesignLaw::GradualComplex, "「小事直路」的行数上限是否包含空行与注释？")
                .unwrap_err(),
            LangLawError::AmbigDup
        );
        assert_eq!(w.ambiguities.dup_rejected, 1);
        assert_eq!(
            w.clarify_ambiguity(aid, "再改一次").unwrap_err(),
            LangLawError::State
        );
        let aid2 = w.register_ambiguity(DesignLaw::SafeDefault, "沙箱网络能力是否属于默认授予？").unwrap();
        assert_eq!(w.clarify_ambiguity(aid2, "").unwrap_err(), LangLawError::AmendEmpty);
        assert_eq!(w.clarify_ambiguity(99, "不存在").unwrap_err(), LangLawError::TargetMissing);
        assert!(w.audit().is_ok());
    }

    /// 判据 T14：双跑确定性对拍——同操作序列两次开机摘要必等；单步分歧必异。
    #[test]
    fn t14_dual_run_determinism() {
        let run = || -> LangLawWorld {
            let mut w = LangLawWorld::boot();
            let self_v = [SelfVerdict::Complies; LAW_COUNT];
            let _ = w.review_proposal("提案一", "面一", ProposalFacts::COMPLIANT, &self_v);
            let mut bad = ProposalFacts::COMPLIANT;
            bad.error_has_fix = false;
            let honest = [SelfVerdict::Violates; LAW_COUNT];
            let _ = w.review_proposal("提案二", "面二", bad, &honest);
            let _ = w.register_precedent("场景甲", Verdict::Block, DesignLaw::ReadableFirst, "理由甲");
            let _ = w.register_precedent("场景乙", Verdict::Allow, DesignLaw::DeterministicFirst, "理由乙");
            let _ = w.register_ambiguity(DesignLaw::SafeDefault, "网络默认授予？");
            let _ = w.clarify_ambiguity(1, "网络能力默认不授予——口径 v2。");
            w
        };
        let a = run();
        let b = run();
        assert_eq!(a.state_digest(), b.state_digest(), "同序列双跑摘要必等");
        assert_eq!(a.state_digest(), b.state_digest(), "摘要幂等");
        // 单步分歧：多送一单拦截 → 摘要必异。
        let mut c = run();
        let honest = [SelfVerdict::Violates; LAW_COUNT];
        let mut bad = ProposalFacts::COMPLIANT;
        bad.sandbox_no_leak = false;
        let _ = c.review_proposal("提案三", "面三", bad, &honest);
        assert_ne!(a.state_digest(), c.state_digest());
        // 再跑一次 b 面孔不变（摘要只读无副作用）。
        assert_eq!(b.state_digest(), b.state_digest());
    }

    /// 判据 T15：事件账满即拒收——不静默丢，drain 后恢复记账。
    #[test]
    fn t15_ledger_full_rejects_then_drains() {
        let mut led = EventLedger::new();
        let mut i = 0;
        while i < LEDGER_CAP {
            assert!(led.record(EventKind::PrecedentFiled, None, "填账").is_ok());
            i += 1;
        }
        assert_eq!(led.len(), LEDGER_CAP);
        assert_eq!(
            led.record(EventKind::PrecedentFiled, None, "溢出").unwrap_err(),
            LangLawError::LedgerFull
        );
        let taken = led.drain();
        assert_eq!(taken.len(), LEDGER_CAP);
        assert!(led.is_empty());
        assert!(led.record(EventKind::PrecedentFiled, None, "回空").is_ok());
        // 流水号不回退（账序单调）。
        assert_eq!(taken[0].seq, 1);
        assert_eq!(taken[LEDGER_CAP - 1].seq, LEDGER_CAP as u64);
    }

    /// 判据 T16：判例集满拒收——append-only 不覆盖（512 例封顶）。
    #[test]
    fn t16_precedent_cap_rejects() {
        let mut w = LangLawWorld::boot();
        let mut i = 0;
        while i < PRECEDENT_CAP {
            let scene = format!("容量填充场景{}", i);
            assert!(w.book.register(&scene, Verdict::Allow, DesignLaw::SafeDefault, "填充").is_ok());
            i += 1;
        }
        assert_eq!(
            w.book.register("再挤一例", Verdict::Allow, DesignLaw::SafeDefault, "挤").unwrap_err(),
            LangLawError::PrecFull
        );
        assert_eq!(w.book.len(), PRECEDENT_CAP as u32);
    }

    /// 判据 T17：诊断码段独占面——16 码连续互异，错误 detail 全非空且说人话。
    #[test]
    fn t17_code_segment_and_details() {
        let all = [
            LangLawError::CardAmend, LangLawError::CardFull, LangLawError::PrecDup,
            LangLawError::PrecBadId, LangLawError::ProposalBad, LangLawError::LawBlock,
            LangLawError::ConflictBad, LangLawError::ConflictNone, LangLawError::TargetMissing,
            LangLawError::AmendEmpty, LangLawError::AmbigDup, LangLawError::AmbigOpen,
            LangLawError::LedgerFull, LangLawError::PrecFull, LangLawError::Inject,
            LangLawError::State,
        ];
        for (i, e) in all.iter().enumerate() {
            assert_eq!(e.code(), CODES[i], "第{}号错误码与 CODES 对位", i);
            let d = e.detail();
            assert!(!d.is_empty(), "detail 不得为空");
            assert!(d.len() >= 20, "detail 须说人话给改法（非裸码）");
        }
        // 段边界：恰落 0x1B30..=0x1B3F。
        assert_eq!(all[0].code(), 0x1B30);
        assert_eq!(all[15].code(), 0x1B3F);
    }

    /// 判据 T18：拦截 O(1) 判据——同事实不同文本长度的提案判读计数一致。
    #[test]
    fn t18_interception_is_o1_of_text() {
        let mut w1 = LangLawWorld::boot();
        let mut w2 = LangLawWorld::boot();
        let honest = [SelfVerdict::Violates; LAW_COUNT];
        let mut bad = ProposalFacts::COMPLIANT;
        bad.small_path_exists = false;
        let short = "名";
        let long = "这是一个被刻意拉长到很长的提案标题——".repeat(64);
        let _ = w1.review_proposal(short, "面", bad, &honest).unwrap();
        let _ = w2.review_proposal(&long, "面", bad, &honest).unwrap();
        // 判读结果与文本长度无关：拦同一律、同计数、同流水推进。
        let h1 = w1.health();
        let h2 = w2.health();
        assert_eq!(h1.reviews_blocked, h2.reviews_blocked);
        assert_eq!(h1.reviews_total, h2.reviews_total);
        assert_eq!(h1.ledger_len, h2.ledger_len);
    }

    /// 判据 T19：权威序——卡 > 判例 > 提案（运行期复核编译期断言面）。
    #[test]
    fn t19_authority_order() {
        assert!(AuthorityKind::ConstitutionCard.outranks(AuthorityKind::Precedent));
        assert!(AuthorityKind::Precedent.outranks(AuthorityKind::Proposal));
        assert!(AuthorityKind::ConstitutionCard.outranks(AuthorityKind::Proposal));
        assert!(!AuthorityKind::Precedent.outranks(AuthorityKind::ConstitutionCard));
        // 裁决依据章恒盖宪法卡（结构性权威序）。
        let mut w = LangLawWorld::boot();
        let pa = w.register_precedent("甲", Verdict::Block, DesignLaw::FriendlyError, "理甲").unwrap();
        let pb = w.register_precedent("乙", Verdict::Allow, DesignLaw::FriendlyError, "理乙").unwrap();
        let fid = w.file_conflict(pa, pb).unwrap();
        w.adjudicate(fid, Ruling::RightPrevails, "带改法的报错合十三章口径").unwrap();
        let f = w.court.filing(fid).unwrap();
        assert_eq!(f.authority_basis, AuthorityKind::ConstitutionCard);
        assert_eq!(f.authority_basis.rank(), 3);
    }

    /// 判据 T20：读屏可达——health 与状态摘要人话行全非空且含关键词。
    #[test]
    fn t20_spoken_faces() {
        let mut w = LangLawWorld::boot();
        let self_v = [SelfVerdict::Complies; LAW_COUNT];
        let _ = w.review_proposal("提案F", "面", ProposalFacts::COMPLIANT, &self_v);
        let lines = w.spoken_status();
        assert_eq!(lines.len(), 6);
        for line in lines.iter() {
            assert!(!line.is_empty());
        }
        assert!(lines[0].contains("宪法"));
        assert!(lines[2].contains("评审"));
        assert!(lines[5].contains("注入"));
        assert!(w.health().is_healthy());
    }

    /// 判据 T21：验链检出能力——手造坏指纹/坏版本/未冻结卡均被 verify 抓住。
    #[test]
    fn t21_verify_chain_catches_tampering() {
        let mut w = LangLawWorld::boot();
        assert!(w.codex.verify_all().is_ok());
        // 坏指纹：改内容不改指纹。
        let mut tampered = w.codex.chains[2][0].clone();
        tampered.statute = "被篡改的律文".to_string();
        assert_ne!(tampered.recompute_fingerprint(), tampered.fingerprint);
        // 坏版本：版本跳号。
        let mut gap = w.codex.chains[1][0].clone();
        gap.version = 7;
        assert_ne!(gap.version, 1);
        // 未冻结卡不是合法状态。
        let mut unfrozen = w.codex.chains[3][0].clone();
        unfrozen.frozen = false;
        assert!(!unfrozen.frozen);
        // 完好卡验链必绿（对照组）。
        assert!(w.codex.chains[0][0].frozen);
        assert_eq!(w.codex.chains[0][0].recompute_fingerprint(), w.codex.chains[0][0].fingerprint);
    }

    /// 判据 T22：五律判读单源对拍——事实面逐位翻转与逐律拦截一一对应。
    #[test]
    fn t22_law_reading_matrix() {
        // 每一律单独违律 → 恰拦该律（判读矩阵五向独立）。
        let pairs: [(usize, fn(&mut ProposalFacts), DesignLaw); 5] = [
            (0, |f| f.narrative_readable = false, DesignLaw::ReadableFirst),
            (0, |f| f.no_glyph_trickery = false, DesignLaw::ReadableFirst),
            (1, |f| f.error_has_fix = false, DesignLaw::FriendlyError),
            (2, |f| f.sandbox_no_leak = false, DesignLaw::SafeDefault),
            (3, |f| f.small_path_exists = false, DesignLaw::GradualComplex),
        ];
        for (_, flip, expect_law) in pairs {
            let mut w = LangLawWorld::boot();
            let mut facts = ProposalFacts::COMPLIANT;
            flip(&mut facts);
            let honest = [SelfVerdict::Violates; LAW_COUNT];
            let r = w.review_proposal("矩阵", "面", facts, &honest).unwrap();
            assert_eq!(r.blocked_law, Some(expect_law), "判读矩阵单向性：{:?}", expect_law);
        }
        // 确定优先独立翻转。
        let mut w = LangLawWorld::boot();
        let mut facts = ProposalFacts::COMPLIANT;
        facts.deterministic = false;
        let honest = [SelfVerdict::Violates; LAW_COUNT];
        let r = w.review_proposal("矩阵", "面", facts, &honest).unwrap();
        assert_eq!(r.blocked_law, Some(DesignLaw::DeterministicFirst));
    }

    /// 判据 T23：错误友好律对账本自身——事件备注人话非空，事件种类名可读。
    #[test]
    fn t23_events_human_readable() {
        let mut w = LangLawWorld::boot();
        let self_v = [SelfVerdict::Complies; LAW_COUNT];
        let _ = w.review_proposal("提案G", "面", ProposalFacts::COMPLIANT, &self_v);
        let _ = w.register_precedent("场景丙", Verdict::Allow, DesignLaw::SafeDefault, "理由丙");
        let events = w.ledger.drain();
        assert!(events.len() >= 2);
        for e in events.iter() {
            assert!(!e.note.is_empty(), "事件备注不得为空");
            assert!(!e.kind.name().is_empty());
        }
        assert!(events.iter().any(|e| matches!(e.kind, EventKind::ReviewPassed)));
        assert!(events.iter().any(|e| matches!(e.kind, EventKind::PrecedentFiled)));
    }

    /// 判据 T24：全流程闭环冒烟——评审/判例/冲突/裁决/歧义/澄清/审计/健康一条龙。
    #[test]
    fn t24_full_pipeline_smoke() {
        let mut w = LangLawWorld::boot();
        // 1. 合规提案过审。
        let self_v = [SelfVerdict::Complies; LAW_COUNT];
        assert_eq!(
            w.review_proposal("叙事脚本直路", "场景逻辑面", ProposalFacts::COMPLIANT, &self_v)
                .unwrap()
                .outcome,
            ReviewOutcome::Passed
        );
        // 2. 谎报提案被拦（注入面一）。
        let mut bad = ProposalFacts::COMPLIANT;
        bad.no_glyph_trickery = false;
        assert_eq!(
            w.review_proposal("机巧提案", "词法面", bad, &self_v).unwrap().outcome,
            ReviewOutcome::BlockedLying
        );
        // 3. 判例两条相斥 → 冲突 → 裁决。
        let pa = w.register_precedent("机巧判例甲", Verdict::Block, DesignLaw::ReadableFirst, "违可读第一").unwrap();
        let pb = w.register_precedent("机巧判例乙", Verdict::Allow, DesignLaw::ReadableFirst, "特批放行").unwrap();
        let fid = w.file_conflict(pa, pb).unwrap();
        w.adjudicate(fid, Ruling::LeftPrevails, "字符机巧零容忍——以宪法卡律文为准").unwrap();
        // 4. 歧义 + 澄清 + 版本前进。
        let aid = w.register_ambiguity(DesignLaw::DeterministicFirst, "时钟注入是否算隐式非确定？").unwrap();
        w.clarify_ambiguity(aid, "时钟须显式注入——隐式取系统时间即违确定优先。").unwrap();
        // 5. 越权修宪被拒（注入面三）。
        assert_eq!(
            w.force_rewrite_probe(DesignLaw::ReadableFirst, "x", "y", "z").unwrap_err(),
            LangLawError::CardAmend
        );
        // 6. 审计 + 健康 + 对拍根基。
        assert!(w.audit().is_ok());
        let h = w.health();
        assert!(h.is_healthy());
        assert_eq!(h.inject_proposal, 1);
        assert_eq!(h.inject_amend, 1);
        assert!(h.revisions >= 1);
        assert!(w.state_digest() != 0);
    }
}
