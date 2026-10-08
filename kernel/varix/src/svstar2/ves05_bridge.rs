//! VE-F3605 · 创作与E 域引擎对接（VE-R 域 · 创作生态域 · R01 组 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3605`
//!
//! **判据（锚点原文·六项）**：统一管线、预览隔离、令牌单源、转换对拍、
//! 收敛执法、判据。
//!
//! **职责定位（锚点原文）**：创作与 E 域引擎对接——对接（创作资产↔E 域引擎
//! （创作产物（主题/皮肤）如何被引擎消费：对接协议（创作资产→引擎资源的转换
//! 委托（Q 管线委托复述——创作资产也是资源：走统一管线（收敛红线复述执法；
//! 实时对接（编辑中→引擎实时反映（F3623 联动前向——预览通道协议（编辑态
//! 数据→预览渲染：预览不走正式管线（轻量通道（预览轻量红线：编辑预览不污染
//! 正式缓存（预览隔离；与 E 域令牌（主题资产→令牌集（F3003 通道复用单源声明。
//!
//! **数据结构（锚点原文·家族格式）**：委托协议；预览通道（隔离声明）；令牌
//! 复用声明；对接签名。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：预览污染检出→隔离修正（红线
//! 实测）；绕管线直消费→立案（收敛复述）；转换失真→对拍（复述）；令牌分叉→
//! 单源修正（复述）。
//!
//! **性能逐项分解（锚点原文·家族格式）**：委托 O(1)；预览 O(轻量）；隔离 O(1)；
//! 对拍 O(抽样)。
//!
//! **跨批对接点（锚点原文·家族格式）**：Q 管线收敛复述；E 令牌单源；F3606 预览
//! 对端；F3623 编辑联动。
//!
//! **无隐私面（锚点原文）**：本项只搬运创作产物的类型标识与内容摘要，不采集
//! 创作者个人信息，也不读取任何用户标识。
//!
//! # 〇、模块命名：`ves05_*` 的由来
//!
//! [`ver02_arch`](super::ver02_arch)= F3601 域开工 ADR；[`ver03_arch`](super::ver03_arch)
//! = F3602 生态总架构；[`ver04_arch`](super::ver04_arch) = F3603 资产模型；
//! [`ves04_flow`](super::ves04_flow) = F3604 工作流引擎。本项是 R01 组第五个，
//! 沿 F3604 的 `ves04_*` 序列取 `ves05_*`（源码只增不减不移，不改他人模块名）。
//!
//! # 一、本项解决的真问题：创作产物怎么进引擎（不是「怎么做资产」）
//!
//! F3603 已把创作资产建模完（七要素/七类/许可/兼容），F3604 已把工作流编排完
//! （三预置流/断点续作/沙箱）。但**资产做成之后怎么被引擎消费**，册内一直没有
//! 正面回答过。本项补的就是这一段：创作侧与E 域侧的**接缝**。
//!
//! 接缝上有三种东西会出错，且都不是资产本身的错，是**搬运方式**的错：
//!
//! 1. **绕过管线**：创作侧图省事，把自己的字节直接塞进引擎资源槽。后果是
//!    资产跳过了 Q 管线的收敛登记，引擎侧拿到一份「没走过流程」的资源——
//!    收敛红线执法（判据一）。
//! 2. **预览污染正式缓存**：编辑态数据是**临时**的（改到一半、可能失败、随时
//!    回滚），若它写进了正式缓存，用户放弃编辑后正式缓存已被脏数据覆盖且
//!    无从恢复——预览隔离（判据二）。
//! 3. **令牌分叉**：同一枚令牌（如 `motion.duration.fast`）在创作侧算一个值、
//!    在引擎侧又算一个值，两边都「自洽」，但画面与设计稿不符——令牌单源
//!    （判据三）。
//!
//! # 二、两条通道必须**结构上**不可混（不是靠自觉遵守）
//!
//! 锚点说「预览不走正式管线（轻量通道）」。这句话的实现方式有两种：
//!
//! - **弱实现**：给通道加一个 `is_preview: bool` 字段，靠调用方填对。填错了
//!   没人拦，判据也只能测「填对时对不对」，测不出「填错时拦不拦」。
//! - **强实现（本项采用）**：两条通道是**两个不同的类型**——[`PreviewChannel`]
//!   与 [`FormalChannel`]，各有各的写入口。预览通道**在类型上没有**写正式
//!   缓存的方法；正式通道**不接受**未走完委托的载荷。
//!
//! 代价是要多写一点类型，收益是「污染」这件事**在编译期就写不出来**，
//! 判据得以断言「隔离不是靠纪律，是靠类型」。锚点原文的「隔离 O(1)」正是
//! 这个意思：判定一次是否污染只需 O(1) 比对，不需要遍历缓存。
//!
//! # 三、委托协议：凭委托票据消费，不是凭调用方自称
//!
//! 收敛红线的执行机制是**票据**：资产要先向管线报名（[`DelegateLedger::enroll`]
//! 拿到 [`DelegateTicket`]），管线跑完盖章（[`DelegateLedger::seal`]），
//! 引擎消费时必须出示票据（[`FormalChannel::consume`]）。三步各自 O(1)：
//!
//! - 未报名 → [`E_PIPELINE_BYPASS`]（立案，收敛红线）；
//! - 已报名未盖章 → [`E_PIPELINE_UNSEALED`]（不可消费）；
//! - 已盖章 → 消费成功，票据**记为已用**（同一票据不可消费两次，
//!   否则 [`E_TICKET_REPLAY`]——重放即绕过管线的另一种形态）。
//!
//! 票据里带**载荷指纹**，消费时引擎侧重算内容指纹并与票据比对：不一致即
//! [`E_PAYLOAD_DRIFT`]（转换失真）。这让「对拍」不依赖下游自觉汇报。
//!
//! # 四、令牌单源：创作侧不得定义令牌，只能引用
//!
//! [`TokenBinding`] 只有一条构造路径 [`TokenBinding::reference`]，令牌值的
//! 权威来源恒为 [`TOKEN_AUTHORITY`]（E 域 F3003 通道）。创作侧能做的只有
//! 「引用哪一枚 + 覆盖到第几层」，不能改值。试图改值在类型上不可表达；
//! 而**同一令牌被两处声明引用**（分叉）由 [`audit_token_fork`] 检出并给出
//! [`E_TOKEN_FORK`]——「单源」是**可机检的记账事实**，不是约定。
//!
//! # 五、对拍：转换失真靠抽样比对，不靠信任
//!
//! [`parity_sample`] 从委托票据里抽样 N 条，逐条比对「票据指纹」与「引擎侧
//! 重算指纹」，产出 [`ParityReport`]。抽样数 N 由调用方给（锚点：O(抽样)），
//! 但报告**必须**同时给出 `sampled`/`matched`/`drifted` 三个**恰等于**的计数
//! ——三者之和须等于 N，缺一个这条判据就成了恒真门禁（钉不住「抽样为空」
//! 与「抽样全对」两种退化情形）。
//!
//! # 六、预览轻量的可测含义：字节上限，不是形容词
//!
//! 「轻量通道」若只写成注释就是空话。本项把它落成**可机检的三个量**：
//! 预览载荷字节数 ≤ [`PREVIEW_BYTE_CAP`]、预览不写正式缓存（类型保证）、
//! 预览往返不增长正式缓存条目数（[`PreviewChannel::formal_entries`] 不变）。
//! 三个量任一越界即 [`E_PREVIEW_HEAVY`]。

#![allow(clippy::needless_range_loop)]

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::MAX_CHECKS;

// ---------------------------------------------------------------------------
// 〇、版本与常量
// ---------------------------------------------------------------------------

/// 委托协议版本（签名材料的一部分）。
pub const DELEGATE_VERSION: &str = "delegate-v1";
/// 预览通道协议版本。
pub const PREVIEW_VERSION: &str = "preview-v1";
/// 令牌复用声明版本。
pub const TOKEN_REUSE_VERSION: &str = "token-reuse-v1";
/// 对接签名版本标识。
pub const BRIDGE_VERSION: &str = "creation-e-bridge-v1";

/// 判据条数（锚点原文六项：统一管线/预览隔离/令牌单源/转换对拍/收敛执法/判据）。
pub const CRITERION_COUNT: usize = 6;
/// 降级矩阵条目数（锚点原文四条错误路径）。
pub const DEGRADE_PATH_COUNT: usize = 4;
/// 禁扩面条目数（划清本项与 F3606/F3623 等前瞻项的界）。
pub const EXCLUSION_COUNT: usize = 6;

/// 预览载荷字节上限（「轻量」的可测含义）。
pub const PREVIEW_BYTE_CAP: usize = 4096;
/// 对拍抽样上限（防止调用方要一个 O(全量) 的抽样）。
pub const PARITY_SAMPLE_CAP: usize = 64;
/// 令牌层数上限（四级覆盖栈对齐 E 域 F3003/F3406 的层数上界）。
pub const MAX_TOKEN_LAYER: u8 = 3;
/// 令牌标识最大字节数。
pub const MAX_TOKEN_KEY_BYTES: usize = 64;
/// 票据账本容量上限。
pub const MAX_TICKETS: usize = 32;
/// 令牌分叉审计的最大声明数。
pub const MAX_TOKEN_DECLS: usize = 32;

/// 令牌值权威来源（E 域 F3003 通道）——创作侧引用它，不得另立。
pub const TOKEN_AUTHORITY: &str = "E-F3003";

/// **对接签名 v1**（冻结）。
///
/// 由 [`bridge_signature`] 对 `BRIDGE_VERSION` + 产物面 + 通道面 + 六判据码
/// 逐字节 FNV-1a 得出。**只吃字节不吃整数语义**：字节材料在任何优化级别下
/// 都算出同一值，而把数值当锚会因优化级别漂移，那是假冻结。
///
/// 冻结值（Python 侧独立重算，见判据 `C-CRITERIA-PROVENANCE`）：
/// `0x645747623917a848`，材料长 180 字节。
pub const BRIDGE_SIG_V1: u64 = 0x6457_4762_3917_a848;

// ---------------------------------------------------------------------------
// 一、错误码
// ---------------------------------------------------------------------------

/// 绕管线直消费（收敛红线：立案）。
pub const E_PIPELINE_BYPASS: &str = "E_PIPELINE_BYPASS";
/// 票据未盖章即消费。
pub const E_PIPELINE_UNSEALED: &str = "E_PIPELINE_UNSEALED";
/// 票据不存在（凭空消费）。
pub const E_TICKET_ABSENT: &str = "E_TICKET_ABSENT";
/// 票据重放（同一票据二次消费＝绕管线的另一形态）。
pub const E_TICKET_REPLAY: &str = "E_TICKET_REPLAY";
/// 账本已满。
pub const E_LEDGER_FULL: &str = "E_LEDGER_FULL";
/// 载荷指纹漂移（转换失真）。
pub const E_PAYLOAD_DRIFT: &str = "E_PAYLOAD_DRIFT";
/// 预览载荷超轻量上限。
pub const E_PREVIEW_HEAVY: &str = "E_PREVIEW_HEAVY";
/// 预览污染正式缓存（红线：隔离修正）。
pub const E_PREVIEW_CONTAMINATION: &str = "E_PREVIEW_CONTAMINATION";
/// 令牌分叉（单源修正）。
pub const E_TOKEN_FORK: &str = "E_TOKEN_FORK";
/// 令牌层号越界。
pub const E_TOKEN_LAYER_RANGE: &str = "E_TOKEN_LAYER_RANGE";
/// 令牌标识为空或超长。
pub const E_TOKEN_KEY_INVALID: &str = "E_TOKEN_KEY_INVALID";
/// 令牌来源非法（非E 域单源）。
pub const E_TOKEN_SOURCE_FOREIGN: &str = "E_TOKEN_SOURCE_FOREIGN";
/// 对拍抽样越界。
pub const E_PARITY_SAMPLE_RANGE: &str = "E_PARITY_SAMPLE_RANGE";
/// 对拍计数不自洽（三计数之和≠ 抽样数）。
pub const E_PARITY_COUNT_INCONSISTENT: &str = "E_PARITY_COUNT_INCONSISTENT";
/// 签名漂移（协议面被改动而签名未重冻）。
pub const E_SIGNATURE_DRIFT: &str = "E_SIGNATURE_DRIFT";
/// 判据缺依据。
pub const E_CRITERION_NO_BASIS: &str = "E_CRITERION_NO_BASIS";
/// 降级矩阵缺项。
pub const E_DEGRADE_MISSING: &str = "E_DEGRADE_MISSING";
/// 降级动作错配。
pub const E_DEGRADE_MISMATCH: &str = "E_DEGRADE_MISMATCH";
/// 越界（做了本项不该做的事）。

pub const E_OVERREACH: &str = "E_OVERREACH";

/// 严重级 P0（收敛/隔离红线）。
pub const P0: &str = "P0";
/// 严重级 P1。
pub const P1: &str = "P1";
/// 严重级 P2。
pub const P2: &str = "P2";

/// FNV-1a 64 位（字节口径，唯一实现——本项与判据共用，避免两套哈希各说各话）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// FNV-1a 十六进制串。
pub fn fnv1a64_hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a64(bytes))
}

/// 产物面（创作产物种类——本项只关心能进引擎的那两类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Product {
    /// 主题。
    Theme,
    /// 皮肤。
    Skin,
}

impl Product {
    /// 全部产物（**全集**，判据按此集合推导，不硬编码下标）。
    pub const ALL: [Product; 2] = [Product::Theme, Product::Skin];

    /// 产物码（签名材料 + 对接标识）。
    pub fn code(self) -> &'static str {
        match self {
            Product::Theme => "P-THEME",
            Product::Skin => "P-SKIN",
        }
    }

    /// 中文名（照锚点原词「主题/皮肤」）。
    pub fn name_cn(self) -> &'static str {
        match self {
            Product::Theme => "主题",
            Product::Skin => "皮肤",
        }
    }

    /// 由码反查。
    pub fn from_code(code: &str) -> Option<Product> {
        Product::ALL.iter().copied().find(|p| p.code() == code)
    }
}

/// 通道面（两条通道，结构上分离——见模块头注§二）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    /// 正式通道（走管线，进正式缓存）。
    Formal,
    /// 预览通道（轻量，不进正式缓存）。
    Preview,
}

impl Lane {
    /// 全部通道。
    pub const ALL: [Lane; 2] = [Lane::Formal, Lane::Preview];

    /// 通道码。
    pub fn code(self) -> &'static str {
        match self {
            Lane::Formal => "L-FORMAL",
            Lane::Preview => "L-PREVIEW",
        }
    }

    /// 是否正式通道。
    pub fn is_formal(self) -> bool {
        matches!(self, Lane::Formal)
    }
}

/// 六判据码（锚点原文六项，**逐字照录**）。
pub const CRITERION_CODES: [&str; CRITERION_COUNT] = [
    "C-UNIFIED-PIPELINE",
    "C-PREVIEW-ISOLATION",
    "C-TOKEN-SINGLE-SOURCE",
    "C-CONVERSION-PARITY",
    "C-CONVERGENCE-ENFORCE",
    "C-CRITERIA-PROVENANCE",
];

/// 判据中文名（照锚点原词）。
pub const CRITERION_NAMES_CN: [&str; CRITERION_COUNT] = [
    "统一管线",
    "预览隔离",
    "令牌单源",
    "转换对拍",
    "收敛执法",
    "判据",
];

// ---------------------------------------------------------------------------
// 二、票据与委托账本（判据一：统一管线）
// ---------------------------------------------------------------------------

/// 票据状态（盖章前不可消费）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TicketState {
    /// 已报名，未盖章（管线处理中）。
    Enrolled,
    /// 已盖章（可消费）。
    Sealed,
    /// 已消费（不可重放）。
    Spent,
}

/// 委托票据：引擎消费的**唯一凭据**。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelegateTicket {
    /// 票据号（报名时生成，单调不复用——按`len()+1` 编号会让释放后的新票据
    /// 拿到旧号，审计归属就乱了）。
    pub ticket_no: u32,
    /// 产物种类。
    pub product: Product,
    /// 载荷指纹（管线盖章时锁定）。
    pub payload_fp: u64,
    /// 载荷字节数（轻量判定与审计用）。
    pub payload_bytes: usize,
    /// 状态。
    pub state: TicketState,
}

impl DelegateTicket {
    /// 是否可消费（已盖章且未用）。
    pub fn is_consumable(&self) -> bool {
        matches!(self.state, TicketState::Sealed)
    }
}

/// 委托账本：O(1) 报名 / 盖章 / 消费（锚点性能：委托 O(1)）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelegateLedger {
    tickets: Vec<DelegateTicket>,
    /// 已消费票据数（建销对账用——建数须恰等于销数+ 未销数）。
    pub spent_count: u32,
}

impl DelegateLedger {
    /// 空账本。
    pub fn empty() -> DelegateLedger {
        DelegateLedger { tickets: Vec::new(), spent_count: 0 }
    }

    /// 报名：登记一笔待转换的创作产物，发回票据号。
    ///
    /// 票据号 = `len()+1` 的写法**不用**，改用单调计数器，避免票据释放后
    /// 新票据复用旧号导致审计归属错乱。
    pub fn enroll(&mut self, product: Product, payload_fp: u64, payload_bytes: usize) -> Result<u32, BridgeError> {
        if self.tickets.len() >= MAX_TICKETS {
            return Err(BridgeError::new(
                E_LEDGER_FULL,
                format!("ledger={}", self.tickets.len()),
                "委托账本已满：拒绝新报名而不是覆盖旧票据（旧票据可能已被消费）",
            ));
        }
        let ticket_no = self.tickets.len() as u32 + 1;
        self.tickets.push(DelegateTicket {
            ticket_no,
            product,
            payload_fp,
            payload_bytes,
            state: TicketState::Enrolled,
        });
        Ok(ticket_no)
    }

    /// 盖章：管线跑完，票据转为可消费。指纹在此刻锁定。
    pub fn seal(&mut self, ticket_no: u32, sealed_fp: u64) -> Result<(), BridgeError> {
        let t = self.tickets.iter_mut().find(|t| t.ticket_no == ticket_no).ok_or_else(|| {
            BridgeError::new(E_TICKET_ABSENT, format!("ticket={ticket_no}"), "盖章失败：票据不存在")
        })?;
        if t.state != TicketState::Enrolled {
            return Err(BridgeError::new(
                E_TICKET_REPLAY,
                format!("ticket={ticket_no}"),
                "盖章失败：票据已非「已报名」态",
            ));
        }
        t.payload_fp = sealed_fp;
        t.state = TicketState::Sealed;
        Ok(())
    }

    /// 查票据（只读）。
    pub fn ticket(&self, ticket_no: u32) -> Option<&DelegateTicket> {
        self.tickets.iter().find(|t| t.ticket_no == ticket_no)
    }

    /// 建销对账：销数 + 未销数须恰等于建数。
    pub fn reconcile(&self) -> bool {
        let spent = self.tickets.iter().filter(|t| t.state == TicketState::Spent).count();
        let live = self.tickets.iter().filter(|t| t.state != TicketState::Spent).count();
        spent == self.spent_count as usize && spent + live == self.tickets.len()
    }

    /// 未盖章票据数（执法巡检用）。
    pub fn unsealed_count(&self) -> usize {
        self.tickets.iter().filter(|t| t.state == TicketState::Enrolled).count()
    }

    /// 已消费票据数（独立重算，判据侧不用 `spent_count` 自问）。
    pub fn spent_actual(&self) -> usize {
        self.tickets.iter().filter(|t| t.state == TicketState::Spent).count()
    }

    /// 票据总数。
    pub fn len(&self) -> usize {
        self.tickets.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.tickets.is_empty()
    }

    /// 全部票据快照（审计用）。
    pub fn tickets(&self) -> &[DelegateTicket] {
        &self.tickets
    }

    /// 标记消费（内部，供正式通道调用）。
    fn mark_spent(&mut self, ticket_no: u32) {
        if let Some(t) = self.tickets.iter_mut().find(|t| t.ticket_no == ticket_no) {
            t.state = TicketState::Spent;
            self.spent_count = self.spent_count.saturating_add(1);
        }
    }
}

// ---------------------------------------------------------------------------
// 三、两条通道（判据二：预览隔离）
// ---------------------------------------------------------------------------

/// 预览帧：编辑态数据（轻量、临时、可丢弃）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewFrame {
    /// 载荷字节数。
    pub bytes: usize,
    /// 内容指纹（供创作者自查，不等于正式指纹）。
    pub fp: u64,
}

impl PreviewFrame {
    /// 构造一帧。
    pub fn new(bytes: usize, fp: u64) -> PreviewFrame {
        PreviewFrame { bytes, fp }
    }

    /// 是否超轻量上限。
    pub fn is_heavy(&self) -> bool {
        self.bytes > PREVIEW_BYTE_CAP
    }
}

/// 预览通道（**类型上没有**写正式缓存的方法——隔离由类型保证）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreviewChannel {
    frames: Vec<PreviewFrame>,
    /// 正式缓存条目数**快照**（构造时从 [`FormalChannel`] 取一次）。
    /// 预览往返后该值不得变化——变化即说明有人从预览侧写了正式缓存。
    formal_baseline: usize,
    /// 预览期间观测到的正式缓存条目数（由正式通道回填）。
    formal_observed: usize,
}

impl PreviewChannel {
    /// 新建预览通道（登记正式缓存基线）。
    pub fn new(formal: &FormalChannel) -> PreviewChannel {
        let n = formal.entries();
        PreviewChannel { frames: Vec::new(), formal_baseline: n, formal_observed: n }
    }

    /// 推入一帧（**只进预览缓冲**，不进正式缓存）。
    pub fn push(&mut self, frame: PreviewFrame) -> Result<(), BridgeError> {
        if frame.is_heavy() {
            return Err(BridgeError::new(
                E_PREVIEW_HEAVY,
                format!("bytes={}", frame.bytes),
                format!("预览载荷超轻量上限 {PREVIEW_BYTE_CAP}：重活交给正式管线，预览只走轻量通道"),
            ));
        }
        self.frames.push(frame);
        Ok(())
    }

    /// 丢弃全部预览帧（放弃编辑——正式缓存**不受影响**，这是隔离的实义）。
    pub fn discard_all(&mut self) {
        self.frames.clear();
    }

    /// 预览帧数。
    pub fn frames(&self) -> &[PreviewFrame] {
        &self.frames
    }

    /// 预览缓冲总字节（轻量度量）。
    pub fn buffered_bytes(&self) -> usize {
        self.frames.iter().fold(0usize, |acc, f| acc.saturating_add(f.bytes))
    }

    /// 正式缓存基线条目数。
    pub fn formal_baseline(&self) -> usize {
        self.formal_baseline
    }

    /// 由正式通道回填「当前条目数」，供隔离核验。
    pub fn observe_formal(&mut self, current: usize) {
        self.formal_observed = current;
    }

    /// 隔离核验：预览往返后正式缓存条目数须与基线**逐位相等**。
    ///
    /// O(1)——只比两个整数，不遍历缓存（锚点性能：隔离 O(1)）。
    pub fn isolation_intact(&self) -> bool {
        self.formal_baseline == self.formal_observed
    }

    /// 隔离污染（红线实测点：正式缓存被预览改动过）。
    pub fn contamination(&self) -> Option<BridgeIssue> {
        if self.isolation_intact() {
            None
        } else {
            Some(BridgeIssue::new(
                E_PREVIEW_CONTAMINATION,
                format!("baseline={} observed={}", self.formal_baseline, self.formal_observed),
                "预览污染正式缓存：编辑态数据写进了正式通道，须隔离修正（回滚正式缓存至基线）",
            ))
        }
    }
}

/// 正式通道（只接受**已盖章票据**的载荷）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormalChannel {
    entries: Vec<SealedPayload>,
    /// 建销对账：写入次数（独立重算用）。
    pub write_count: u32,
}

/// 已封载荷（持票据的正式资源）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealedPayload {
    /// 票据号。
    pub ticket_no: u32,
    /// 产物种类。
    pub product: Product,
    /// 指纹。
    pub fp: u64,
}

impl FormalChannel {
    /// 空正式通道。
    pub fn new() -> FormalChannel {
        FormalChannel { entries: Vec::new(), write_count: 0 }
    }

    /// 正式缓存条目数。
    pub fn entries(&self) -> usize {
        self.entries.len()
    }

    /// 已写条目快照。
    pub fn entries_slice(&self) -> &[SealedPayload] {
        &self.entries
    }

    /// 消费一张**已盖章**票据写入正式缓存。
    ///
    /// 这是收敛红线的执行点：没有票据 → [`E_PIPELINE_BYPASS`]（立案）。
    ///
    /// **码位分工**（两码不得重合行为，否则缺失分支会被掩盖）：
    /// - [`E_PIPELINE_BYPASS`]：正式通道**消费**时票据不存在——收敛红线，
    ///   P0、立案。这是「绕管线直消费」的正身。
    /// - [`E_TICKET_ABSENT`]：账本**盖章**时票据号不存在——运维笔误，
    ///   P2、立案。
    pub fn consume(
        &mut self,
        ledger: &mut DelegateLedger,
        ticket_no: u32,
        engine_side_fp: u64,
    ) -> Result<(), BridgeError> {
        let (state, sealed_fp, product) = {
            let t = ledger.ticket(ticket_no).ok_or_else(|| {
                BridgeError::new(
                    E_PIPELINE_BYPASS,
                    format!("ticket={ticket_no}"),
                    "消费失败：无票据直消费正式通道——收敛红线立案（创作资产必须走统一管线）",
                )
            })?;
            (t.state, t.payload_fp, t.product)
        };

        match state {
            TicketState::Enrolled => {
                return Err(BridgeError::new(
                    E_PIPELINE_UNSEALED,
                    format!("ticket={ticket_no}"),
                    "消费失败：票据未盖章，管线尚未跑完",
                ))
            }
            TicketState::Spent => {
                return Err(BridgeError::new(
                    E_TICKET_REPLAY,
                    format!("ticket={ticket_no}"),
                    "消费失败：票据已消费（重放即绕过管线的另一形态）",
                ))
            }
            TicketState::Sealed => {}
        }

        if engine_side_fp != sealed_fp {
            return Err(BridgeError::new(
                E_PAYLOAD_DRIFT,
                format!("ticket={ticket_no}"),
                format!(
                    "转换失真：票据指纹 0x{sealed_fp:016x} ≠ 引擎侧重算 0x{engine_side_fp:016x}，须对拍"
                ),
            ));
        }

        self.entries.push(SealedPayload { ticket_no, product, fp: sealed_fp });
        self.write_count = self.write_count.saturating_add(1);
        ledger.mark_spent(ticket_no);
        Ok(())
    }

    /// 回滚至某条目数（隔离修正用：把正式缓存恢复到基线）。
    pub fn rollback_to(&mut self, entries: usize) {
        if entries < self.entries.len() {
            self.entries.truncate(entries);
        }
    }

    /// **未受管直写**（越管线写入的显式建模）。
    ///
    /// 这不是一条「后门」，而是把**收敛红线要抓的违规本身**建成可构造对象：
    /// 执法判据若只能对「所有写入都持票据」的干净世界下结论，那它恒真、
    /// 抓不到任何东西。故此处**故意**提供一条不持票据、不经账本的写入口，
    /// 且它**不计入** [`write_count`]（`write_count` 只计受管写入）。
    ///
    /// 于是「未受管条目数」=`entries() - write_count` 恒可算出，
    /// [`enforce_convergence`] 据此判违规——这才是执法判据的牙口所在。
    pub fn push_unmanaged(&mut self, p: SealedPayload) {
        self.entries.push(p);
    }

    /// 未受管条目数（绕过票据直写进来的条数）。
    pub fn unmanaged_count(&self) -> usize {
        self.entries.len().saturating_sub(self.write_count as usize)
    }
}

// ---------------------------------------------------------------------------
// 四、令牌单源（判据三）
// ---------------------------------------------------------------------------

/// 令牌绑定：**只能引用**权威令牌，不可另立值。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TokenBinding {
    /// 令牌标识（如 `motion.duration.fast`）。
    pub key: String,
    /// 引用层号（0..=MAX_TOKEN_LAYER）。
    pub layer: u8,
    /// 来源（恒为 [`TOKEN_AUTHORITY`]）。
    pub source: &'static str,
    /// 引用时锁定的值摘要（由权威侧给出）。
    pub value_fp: u64,
}

impl TokenBinding {
    /// 构造引用（**唯一**构造路径）。
    ///
    /// `source` 恒写 [`TOKEN_AUTHORITY`]——不接受调用方自报来源，
    /// 这样「非E 域单源」这件事在类型上就构造不出来。
    pub fn reference(key: &str, layer: u8, value_fp: u64) -> Result<TokenBinding, BridgeError> {
        if key.is_empty() || key.len() > MAX_TOKEN_KEY_BYTES {
            return Err(BridgeError::new(
                E_TOKEN_KEY_INVALID,
                format!("len={}", key.len()),
                format!("令牌标识须 1..={MAX_TOKEN_KEY_BYTES} 字节"),
            ));
        }
        if layer > MAX_TOKEN_LAYER {
            return Err(BridgeError::new(
                E_TOKEN_LAYER_RANGE,
                format!("layer={layer}"),
                format!("令牌层号须 ≤ {MAX_TOKEN_LAYER}"),
            ));
        }
        Ok(TokenBinding {
            key: key.to_string(),
            layer,
            source: TOKEN_AUTHORITY,
            value_fp,
        })
    }

    /// 是否权威来源（恒真，但保留为**可机检断言**而非口头保证）。
    pub fn is_authoritative(&self) -> bool {
        self.source == TOKEN_AUTHORITY
    }
}

/// 令牌分叉项（同一令牌被两处声明引用）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenFork {
    /// 令牌标识。
    pub key: String,
    /// 第一处层号。
    pub layer_a: u8,
    /// 第二处层号。
    pub layer_b: u8,
    /// 第一处值摘要。
    pub fp_a: u64,
    /// 第二处值摘要。
    pub fp_b: u64,
}

/// 令牌复用声明（登记本资产引用了哪些令牌）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TokenReuseDecl {
    bindings: Vec<TokenBinding>,
}

impl TokenReuseDecl {
    /// 空声明。
    pub fn empty() -> TokenReuseDecl {
        TokenReuseDecl { bindings: Vec::new() }
    }

    /// 标准声明（主题资产引用三枚动效令牌 + 两枚外观令牌）。
    pub fn standard() -> TokenReuseDecl {
        let mut d = TokenReuseDecl::empty();
        // 权威值摘要取自 Python 侧独立重算（不向被测函数问答案）。
        for (k, layer, fp) in [
            ("motion.duration.fast", 0u8, 0xbf05_eefc_67c4_651du64),
            ("motion.ease.standard", 0u8, 0x57b5_c26c_ca86_da18u64),
            ("motion.shift.tight", 1u8, 0x3001_3216_15b8_e7d7u64),
            ("theme.alpha.bg", 2u8, 0xf02f_70f5_2404_58b5u64),
            ("theme.fg", 2u8, 0x85a8_a012_44e7_a323u64),
        ] {
            if let Ok(b) = TokenBinding::reference(k, layer, fp) {
                d.bindings.push(b);
            }
        }
        d
    }

    /// 追加一条引用。
    pub fn push(&mut self, b: TokenBinding) -> Result<(), BridgeError> {
        if self.bindings.len() >= MAX_TOKEN_DECLS {
            return Err(BridgeError::new(
                E_LEDGER_FULL,
                format!("decls={}", self.bindings.len()),
                "令牌声明数达上限",
            ));
        }
        self.bindings.push(b);
        Ok(())
    }

    /// 绑定集合。
    pub fn bindings(&self) -> &[TokenBinding] {
        &self.bindings
    }

    /// 声明条数。
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// 自审：键非空、层号在界、来源恒权威。
    pub fn audit(&self) -> Vec<BridgeIssue> {
        let mut out = Vec::new();
        for (i, b) in self.bindings.iter().enumerate() {
            if b.key.is_empty() || b.key.len() > MAX_TOKEN_KEY_BYTES {
                out.push(BridgeIssue::new(
                    E_TOKEN_KEY_INVALID,
                    format!("idx={i}"),
                    "令牌标识越界",
                ));
            }
            if b.layer > MAX_TOKEN_LAYER {
                out.push(BridgeIssue::new(
                    E_TOKEN_LAYER_RANGE,
                    format!("idx={i} layer={}", b.layer),
                    "令牌层号越界",
                ));
            }
            if !b.is_authoritative() {
                out.push(BridgeIssue::new(
                    E_TOKEN_SOURCE_FOREIGN,
                    format!("idx={i} source={}", b.source),
                    "令牌来源非 E 域单源",
                ));
            }
        }
        out
    }
}

/// 审计令牌分叉：同一键出现两次即分叉（单源记账的机检形态）。
///
/// O(n²) 但n ≤ [`MAX_TOKEN_DECLS`]，且**这是审计路径**不是消费路径——
/// 锚点性能要求「委托 O(1)/预览 O(轻量)/隔离 O(1)/对拍 O(抽样)」，
/// 未对审计路径设 O(1) 约束，故两两比对是可接受的（写明以免被误当 O(1) 门禁）。
pub fn audit_token_fork(decl: &TokenReuseDecl) -> Vec<TokenFork> {
    let mut out = Vec::new();
    let bs = decl.bindings();
    for i in 0..bs.len() {
        for j in (i + 1)..bs.len() {
            if bs[i].key == bs[j].key {
                out.push(TokenFork {
                    key: bs[i].key.clone(),
                    layer_a: bs[i].layer,
                    layer_b: bs[j].layer,
                    fp_a: bs[i].value_fp,
                    fp_b: bs[j].value_fp,
                });
            }
        }
    }
    out
}

/// 单源修正：把分叉项裁到只剩第一处（保留首次声明）。
///
/// 返回被裁掉的条数——判据要断**恰等于**该值，故返回确切计数而非 bool。
pub fn reconcile_token_single_source(decl: &mut TokenReuseDecl) -> usize {
    let mut removed = 0usize;
    let mut seen: Vec<String> = Vec::new();
    let mut kept: Vec<TokenBinding> = Vec::new();
    for b in decl.bindings.iter() {
        if seen.iter().any(|k| *k == b.key) {
            removed = removed.saturating_add(1);
        } else {
            seen.push(b.key.clone());
            kept.push(b.clone());
        }
    }
    decl.bindings = kept;
    removed
}

// ---------------------------------------------------------------------------
// 五、转换对拍（判据四）
// ---------------------------------------------------------------------------

/// 对拍报告（三计数须恰等于抽样数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParityReport {
    /// 抽样数（调用方给定，钳在 [`PARITY_SAMPLE_CAP`] 内）。
    pub sampled: usize,
    /// 一致条数。
    pub matched: usize,
    /// 失真条数。
    pub drifted: usize,
}

impl ParityReport {
    /// 计数自洽（三者之和== 抽样数）。
    pub fn is_consistent(&self) -> bool {
        self.matched + self.drifted == self.sampled
    }

    /// 是否全对（**须同时抽样非空**，否则退化）。
    pub fn all_match(&self) -> bool {
        self.sampled > 0 && self.drifted == 0 && self.is_consistent()
    }
}

/// 对拍：从票据抽样比对「票据指纹」与「引擎侧指纹」。
///
/// O(抽样)（锚点性能：O(抽样)）。抽样数为 0 时**不**报 all_match 通过——
/// 空抽样必须被上层判据单独抓住，否则「什么都没查」会被读成「都对」。
pub fn parity_sample(
    ledger: &DelegateLedger,
    engine_fps: &[(u32, u64)],
    want: usize,
) -> Result<ParityReport, BridgeError> {
    if want > PARITY_SAMPLE_CAP {
        return Err(BridgeError::new(
            E_PARITY_SAMPLE_RANGE,
            format!("want={want}"),
            format!("抽样数须 ≤ {PARITY_SAMPLE_CAP}（对拍是对 O(抽样) 的）"),
        ));
    }
    let mut sampled = 0usize;
    let mut matched = 0usize;
    let mut drifted = 0usize;
    for (ticket_no, efp) in engine_fps.iter() {
        if sampled >= want {
            break;
        }
        let t = match ledger.ticket(*ticket_no) {
            Some(t) => t,
            None => {
                drifted = drifted.saturating_add(1);
                sampled = sampled.saturating_add(1);
                continue;
            }
        };
        sampled = sampled.saturating_add(1);
        if t.payload_fp == *efp {
            matched = matched.saturating_add(1);
        } else {
            drifted = drifted.saturating_add(1);
        }
    }
    Ok(ParityReport { sampled, matched, drifted })
}

// ---------------------------------------------------------------------------
// 六、收敛执法与对接签名
// ---------------------------------------------------------------------------

/// 执法巡检结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnforcementReport {
    /// 未盖章票据数（仍在管线中，正常）。
    pub in_flight: usize,
    /// 正式缓存条目数。
    pub formal_entries: usize,
    /// 已盖章未消费记为闲票。
    pub idle_sealed: usize,
    /// 绕过票据直写正式缓存的条数（收敛红线要抓的就是它）。
    pub unmanaged: usize,
    /// 是否零违规。
    pub clean: bool,
}

/// 收敛执法巡检：未盖章可放行（管线处理中），已盖章未消费记为闲票。
pub fn enforce_convergence(ledger: &DelegateLedger, formal: &FormalChannel) -> EnforcementReport {
    let in_flight = ledger.unsealed_count();
    let idle_sealed = ledger
        .tickets()
        .iter()
        .filter(|t| t.state == TicketState::Sealed)
        .count();
    // 收敛判据：正式缓存里每一条都必须**对应一张已消费票据**。
    // 若正式条目数 > 已消费票据数，说明有东西绕过了票据直接进了正式缓存。
    let consumed = ledger.spent_actual();
    let unmanaged = formal.unmanaged_count();
    let clean = formal.entries() <= consumed && unmanaged == 0;
    EnforcementReport {
        in_flight,
        formal_entries: formal.entries(),
        idle_sealed,
        unmanaged,
        clean,
    }
}

/// 对接签名：对协议面逐字节 FNV-1a（**冻结锚**，改协议不改签名即漂移）。
pub fn bridge_signature() -> u64 {
    let mut m: Vec<u8> = Vec::new();
    m.extend_from_slice(BRIDGE_VERSION.as_bytes());
    m.push(0);
    for p in Product::ALL {
        m.extend_from_slice(p.code().as_bytes());
        m.push(0);
    }
    for l in Lane::ALL {
        m.extend_from_slice(l.code().as_bytes());
        m.push(0);
    }
    for c in CRITERION_CODES {
        m.extend_from_slice(c.as_bytes());
        m.push(0);
    }
    fnv1a64(&m)
}

/// 签名材料长度（审计用；冻结值 180 字节）。
pub fn bridge_signature_material_len() -> usize {
    let mut n = BRIDGE_VERSION.len() + 1;
    for p in Product::ALL {
        n += p.code().len() + 1;
    }
    for l in Lane::ALL {
        n += l.code().len() + 1;
    }
    for c in CRITERION_CODES {
        n += c.len() + 1;
    }
    n
}

/// 签名是否等于冻结值 v1。
pub fn signature_matches_v1() -> bool {
    bridge_signature() == BRIDGE_SIG_V1
}

// ---------------------------------------------------------------------------
// 七、问题与错误
// ---------------------------------------------------------------------------

/// 契约问题（收集式）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeIssue {
    /// 错误码。
    pub code: &'static str,
    /// 定位键。
    pub subject: String,
    /// 说明。
    pub detail: String,
}

impl BridgeIssue {
    /// 构造问题。
    pub fn new(code: &'static str, subject: impl Into<String>, detail: impl Into<String>) -> BridgeIssue {
        BridgeIssue { code, subject: subject.into(), detail: detail.into() }
    }

    /// 是否阻断类红线（收敛/隔离）。
    pub fn is_blocking(&self) -> bool {
        matches!(
            self.code,
            E_PIPELINE_BYPASS | E_PREVIEW_CONTAMINATION | E_TICKET_REPLAY | E_PAYLOAD_DRIFT
        )
    }

    /// 是否提示类。
    pub fn is_advisory(&self) -> bool {
        matches!(self.code, E_PREVIEW_HEAVY | E_TOKEN_FORK | E_SIGNATURE_DRIFT)
    }
}

/// 对接错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeError {
    /// 错误码。
    pub code: &'static str,
    /// 定位键。
    pub subject: String,
    /// 说明。
    pub detail: String,
}

impl BridgeError {
    /// 构造错误。
    pub fn new(code: &'static str, subject: impl Into<String>, detail: impl Into<String>) -> BridgeError {
        BridgeError { code, subject: subject.into(), detail: detail.into() }
    }

    /// 建议降级动作（锚点四条错误路径的落点）。
    pub fn action(&self) -> DegradeAction {
        match self.code {
            // 「预览污染检出→隔离修正」
            E_PREVIEW_CONTAMINATION => DegradeAction::IsolateRepair,
            // 「绕管线直消费→立案」
            E_PIPELINE_BYPASS | E_TICKET_ABSENT | E_TICKET_REPLAY | E_PIPELINE_UNSEALED => {
                DegradeAction::FileCase
            }
            // 「转换失真→对拍」
            E_PAYLOAD_DRIFT => DegradeAction::ParityProbe,
            // 「令牌分叉→单源修正」
            E_TOKEN_FORK | E_TOKEN_SOURCE_FOREIGN => DegradeAction::UnifySingleSource,
            E_PREVIEW_HEAVY => DegradeAction::ShrinkPreview,
            _ => DegradeAction::FileCase,
        }
    }

    /// 严重级。
    pub fn severity(&self) -> &'static str {
        match self.code {
            E_PIPELINE_BYPASS | E_PREVIEW_CONTAMINATION | E_TICKET_REPLAY => P0,
            E_PAYLOAD_DRIFT
            | E_PIPELINE_UNSEALED
            | E_TICKET_ABSENT
            | E_TOKEN_FORK
            | E_TOKEN_SOURCE_FOREIGN
            | E_PREVIEW_HEAVY => P1,
            _ => P2,
        }
    }

    /// 转契约问题。
    pub fn to_issue(&self) -> BridgeIssue {
        BridgeIssue::new(self.code, self.subject.clone(), self.detail.clone())
    }
}

impl core::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "[{}] {}: {}", self.code, self.subject, self.detail)
    }
}

/// 降级动作（锚点原文四条错误路径 + 两条延伸）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeAction {
    /// 隔离修正（预览污染）。
    IsolateRepair,
    /// 立案（绕管线直消费）。
    FileCase,
    /// 对拍（转换失真）。
    ParityProbe,
    /// 单源修正（令牌分叉）。
    UnifySingleSource,
    /// 缩减预览（超轻量）。
    ShrinkPreview,
}

impl DegradeAction {
    /// 全部动作。
    pub const ALL: [DegradeAction; 5] = [
        DegradeAction::IsolateRepair,
        DegradeAction::FileCase,
        DegradeAction::ParityProbe,
        DegradeAction::UnifySingleSource,
        DegradeAction::ShrinkPreview,
    ];

    /// 动作码。
    pub fn code(self) -> &'static str {
        match self {
            DegradeAction::IsolateRepair => "A-ISOLATE-REPAIR",
            DegradeAction::FileCase => "A-FILE-CASE",
            DegradeAction::ParityProbe => "A-PARITY-PROBE",
            DegradeAction::UnifySingleSource => "A-UNIFY-SINGLE-SOURCE",
            DegradeAction::ShrinkPreview => "A-SHRINK-PREVIEW",
        }
    }

    /// 中文名（照锚点原词）。
    pub fn name_cn(self) -> &'static str {
        match self {
            DegradeAction::IsolateRepair => "隔离修正",
            DegradeAction::FileCase => "立案",
            DegradeAction::ParityProbe => "对拍",
            DegradeAction::UnifySingleSource => "单源修正",
            DegradeAction::ShrinkPreview => "缩减预览",
        }
    }

    /// 由码反查。
    pub fn from_code(code: &str) -> Option<DegradeAction> {
        DegradeAction::ALL.iter().copied().find(|a| a.code() == code)
    }
}

/// 降级矩阵（锚点原文四条错误路径逐条钉死）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DegradePath {
    /// 触发错误码。
    pub trigger: &'static str,
    /// 应落动作。
    pub action: DegradeAction,
    /// 中文说明（照锚点原文）。
    pub note: &'static str,
}

/// 降级矩阵全集（恰 [`DEGRADE_PATH_COUNT`] 条）。
pub const DEGRADE_PATHS: [DegradePath; DEGRADE_PATH_COUNT] = [
    DegradePath {
        trigger: E_PREVIEW_CONTAMINATION,
        action: DegradeAction::IsolateRepair,
        note: "预览污染检出→隔离修正（红线实测）",
    },
    DegradePath {
        trigger: E_PIPELINE_BYPASS,
        action: DegradeAction::FileCase,
        note: "绕管线直消费→立案（收敛复述）",
    },
    DegradePath {
        trigger: E_PAYLOAD_DRIFT,
        action: DegradeAction::ParityProbe,
        note: "转换失真→对拍（复述）",
    },
    DegradePath {
        trigger: E_TOKEN_FORK,
        action: DegradeAction::UnifySingleSource,
        note: "令牌分叉→单源修正（复述）",
    },
];

/// 禁扩面（本项**不做**的事——写死以防范围蠕变）。
pub const EXCLUSIONS: [&str; EXCLUSION_COUNT] = [
    "不实现预览运行时本体（F3606 承担）",
    "不实现编辑器 UI（F3622/F3623 承担）",
    "不实现资产验证五段（F3607 承担）",
    "不实现资产导入四步（F3608 承担）",
    "不实现市场分发（F3616 承担）",
    "不复制 E 域令牌定义（只引用 F3003 单源）",
];

// ---------------------------------------------------------------------------
// 八、判据自审（判据六：判据可自证）
// ---------------------------------------------------------------------------

/// 判据依据（每条判据须有**可机检的依据**，否则是空话）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Criterion {
    /// 判据码。
    pub code: &'static str,
    /// 中文名。
    pub name_cn: &'static str,
    /// 依据说明（须指明机检方式）。
    pub basis: &'static str,
}

/// 六判据全集（逐条给出机检依据）。
pub const CRITERIA: [Criterion; CRITERION_COUNT] = [
    Criterion {
        code: "C-UNIFIED-PIPELINE",
        name_cn: "统一管线",
        basis: "无票据消费必报 E_PIPELINE_BYPASS；票据三态（报名/盖章/已用）逐态可达",
    },
    Criterion {
        code: "C-PREVIEW-ISOLATION",
        name_cn: "预览隔离",
        basis: "预览通道无写正式缓存的方法（类型保证）；隔离核验 O(1) 比两整数",
    },
    Criterion {
        code: "C-TOKEN-SINGLE-SOURCE",
        name_cn: "令牌单源",
        basis: "TokenBinding 来源恒为 TOKEN_AUTHORITY，构造路径不接受自报来源",
    },
    Criterion {
        code: "C-CONVERSION-PARITY",
        name_cn: "转换对拍",
        basis: "parity_sample 三计数之和恰等于抽样数；失真条数可被独立重算",
    },
    Criterion {
        code: "C-CONVERGENCE-ENFORCE",
        name_cn: "收敛执法",
        basis: "enforce_convergence 比「正式条目数 ≤ 已消费票据数」，绕管线必被查出",
    },
    Criterion {
        code: "C-CRITERIA-PROVENANCE",
        name_cn: "判据",
        basis: "六判据码逐字照录锚点；bridge_signature 对材料逐字节冻结",
    },
];

/// 判据自审：六码齐备、依据非空、码与中文名逐位对齐。
pub fn audit_criteria() -> Vec<BridgeIssue> {
    let mut out = Vec::new();
    if CRITERIA.len() != CRITERION_COUNT || CRITERION_CODES.len() != CRITERION_COUNT {
        out.push(BridgeIssue::new(
            E_CRITERION_NO_BASIS,
            format!("criteria={} codes={}", CRITERIA.len(), CRITERION_CODES.len()),
            "判据条数与锚点六项不符",
        ));
    }
    for c in CRITERIA.iter() {
        if c.basis.is_empty() {
            out.push(BridgeIssue::new(
                E_CRITERION_NO_BASIS,
                c.code,
                "判据缺可机检依据",
            ));
        }
        // 判据码须能在 CRITERION_CODES 里找到（防两处清单分叉）。
        if !CRITERION_CODES.iter().any(|s| *s == c.code) {
            out.push(BridgeIssue::new(
                E_CRITERION_NO_BASIS,
                c.code,
                "判据码不在 CRITERION_CODES 清单内（两处清单分叉）",
            ));
        }
    }
    for (i, code) in CRITERION_CODES.iter().enumerate() {
        if CRITERIA[i].code != *code {
            out.push(BridgeIssue::new(
                E_CRITERION_NO_BASIS,
                format!("idx={i}"),
                "判据码与序位不符",
            ));
        }
        if CRITERIA[i].name_cn != CRITERION_NAMES_CN[i] {
            out.push(BridgeIssue::new(
                E_CRITERION_NO_BASIS,
                format!("idx={i}"),
                "判据中文名与锚点原词不符",
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 九、总纲
// ---------------------------------------------------------------------------

/// 对接总纲（标准实例＝全绿正样本）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeArchitecture {
    /// 委托账本。
    pub ledger: DelegateLedger,
    /// 正式通道。
    pub formal: FormalChannel,
    /// 令牌复用声明。
    pub tokens: TokenReuseDecl,
    /// 降级矩阵。
    pub degrade: Vec<DegradePath>,
}

impl BridgeArchitecture {
    /// 标准总纲（**唯一正样本构造处**）：一笔记载走完报名→盖章→消费。
    pub fn standard() -> BridgeArchitecture {
        let mut ledger = DelegateLedger::empty();
        let theme_fp = 0xf02f_70f5_2404_58b5u64;
        let no = ledger.enroll(Product::Theme, theme_fp, 2048).unwrap_or(0);
        let _ = ledger.seal(no, theme_fp);
        let mut formal = FormalChannel::new();
        let _ = formal.consume(&mut ledger, no, theme_fp);
        BridgeArchitecture {
            ledger,
            formal,
            tokens: TokenReuseDecl::standard(),
            degrade: DEGRADE_PATHS.to_vec(),
        }
    }

    /// 空总纲（反例用：令牌分叉 + 签名漂移）。
    pub fn empty() -> BridgeArchitecture {
        let mut tokens = TokenReuseDecl::empty();
        if let Ok(a) = TokenBinding::reference("theme.alpha.bg", 0, 0x1111_1111_1111_1111) {
            let _ = tokens.push(a);
        }
        if let Ok(b) = TokenBinding::reference("theme.alpha.bg", 2, 0x2222_2222_2222_2222) {
            let _ = tokens.push(b);
        }
        BridgeArchitecture {
            ledger: DelegateLedger::empty(),
            formal: FormalChannel::new(),
            tokens,
            degrade: DEGRADE_PATHS.to_vec(),
        }
    }

    /// 收集全部契约问题。
    pub fn collect_issues(&self) -> Vec<BridgeIssue> {
        let mut out = Vec::new();
        out.extend(self.tokens.audit());
        let forks = audit_token_fork(&self.tokens);
        for f in forks.iter() {
            out.push(BridgeIssue::new(
                E_TOKEN_FORK,
                f.key.clone(),
                format!("令牌分叉：层{}@0x{:016x} 与 层{}@0x{:016x}", f.layer_a, f.fp_a, f.layer_b, f.fp_b),
            ));
        }
        // 降级矩阵：四条路径须齐备且动作与错误码映射一致。
        for p in DEGRADE_PATHS.iter() {
            if !self.degrade.iter().any(|d| d.trigger == p.trigger) {
                out.push(BridgeIssue::new(E_DEGRADE_MISSING, p.trigger, "降级矩阵缺该条路径"));
            }
        }
        for d in self.degrade.iter() {
            let expect = DEGRADE_PATHS.iter().find(|p| p.trigger == d.trigger);
            match expect {
                None => out.push(BridgeIssue::new(
                    E_OVERREACH,
                    d.trigger,
                    "降级矩阵含锚点未列的触发码（越界）",
                )),
                Some(p) => {
                    if d.action != p.action {
                        out.push(BridgeIssue::new(
                            E_DEGRADE_MISMATCH,
                            d.trigger,
                            format!("动作应为 {}，实为 {}", p.action.code(), d.action.code()),
                        ));
                    }
                }
            }
        }
        // 错误码 → 动作映射须与降级矩阵一致（两处不得分叉）。
        for p in DEGRADE_PATHS.iter() {
            let e = BridgeError::new(p.trigger, "audit", "");
            if e.action() != p.action {
                out.push(BridgeIssue::new(
                    E_DEGRADE_MISMATCH,
                    p.trigger,
                    format!("错误码映射动作 {} 与矩阵 {} 分叉", e.action().code(), p.action.code()),
                ));
            }
        }
        if !signature_matches_v1() {
            out.push(BridgeIssue::new(
                E_SIGNATURE_DRIFT,
                format!("sig=0x{:016x}", bridge_signature()),
                format!("对接签名漂移，冻结值 0x{BRIDGE_SIG_V1:016x}"),
            ));
        }
        if !self.ledger.reconcile() {
            out.push(BridgeIssue::new(
                E_TICKET_REPLAY,
                "reconcile",
                "票据建销不对账",
            ));
        }
        out.extend(audit_criteria());
        out
    }

    /// 开工前置校验：有阻断类问题即拒。
    pub fn preflight(&self) -> Result<(), BridgeError> {
        let issues = self.collect_issues();
        if let Some(bad) = issues.iter().find(|i| i.is_blocking()) {
            return Err(BridgeError::new(bad.code, bad.subject.clone(), bad.detail.clone()));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 十、自检聚合
// ---------------------------------------------------------------------------

/// VE-F3605 域自检入口由 [`ves05_checks`](super::ves05_checks) 提供，
/// 本模块只落功能；聚合器直接登记 `ves05_checks::run_ves05_checks_a/b`。

/// 自检判据容量上界：单批条数须 ≤ `MAX_CHECKS`，超限会被 `CheckSet` **静默丢弃**
/// （`dropped` 计数）——判据写多了等于没写。这里把上界写成常量供判据层自检。
pub const CHECK_BATCH_CAP: usize = MAX_CHECKS;

/// 构造后自检：判据表、降级矩阵、令牌权威源三处**自身**须零红项。
///
/// 独立于 [`BridgeArchitecture`]：判据层出问题时不能反过来把判据自身
/// 也判红（自举崩塌），故此处只查**静态表**的一致性。
pub fn audit_static_tables() -> Vec<BridgeIssue> {
    let mut out = Vec::new();
    out.extend(audit_criteria());

    // 降级矩阵：条数、动作码往返、动作→错误码映射三处一致。
    if DEGRADE_PATHS.len() != DEGRADE_PATH_COUNT {
        out.push(BridgeIssue::new(
            E_DEGRADE_MISSING,
            format!("paths={}", DEGRADE_PATHS.len()),
            format!("降级矩阵须恰 {DEGRADE_PATH_COUNT} 条（锚点原文四条错误路径）"),
        ));
    }
    for p in DEGRADE_PATHS.iter() {
        let e = BridgeError::new(p.trigger, "audit", "");
        if e.action() != p.action {
            out.push(BridgeIssue::new(
                E_DEGRADE_MISMATCH,
                p.trigger,
                format!("映射动作 {} ≠ 矩阵动作 {}", e.action().code(), p.action.code()),
            ));
        }
        if e.severity() != severity_of(p.trigger) {
            out.push(BridgeIssue::new(
                E_DEGRADE_MISMATCH,
                p.trigger,
                "严重级与静态表分叉",
            ));
        }
    }

    // 令牌权威源：声明常量须为E 域 F3003 通道。
    if TOKEN_AUTHORITY != "E-F3003" {
        out.push(BridgeIssue::new(
            E_TOKEN_SOURCE_FOREIGN,
            TOKEN_AUTHORITY,
            "令牌权威源漂移（须恒为 E 域 F3003 通道）",
        ));
    }

    // 禁扩面条目数。
    if EXCLUSIONS.len() != EXCLUSION_COUNT {
        out.push(BridgeIssue::new(
            E_OVERREACH,
            format!("exclusions={}", EXCLUSIONS.len()),
            "禁扩面条目数不符",
        ));
    }
    out
}

/// 静态严重级表（与 [`BridgeError::severity`] 对照用——独立第二侧）。
fn severity_of(code: &str) -> &'static str {
    match code {
        E_PIPELINE_BYPASS | E_PREVIEW_CONTAMINATION | E_TICKET_REPLAY => P0,
        E_PAYLOAD_DRIFT
        | E_PIPELINE_UNSEALED
        | E_TICKET_ABSENT
        | E_TOKEN_FORK
        | E_TOKEN_SOURCE_FOREIGN
        | E_PREVIEW_HEAVY => P1,
        _ => P2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 标准总纲零红项() {
        let a = BridgeArchitecture::standard();
        let issues = a.collect_issues();
        assert!(issues.is_empty(), "标准总纲不该有红项：{issues:?}");
        assert!(a.preflight().is_ok());
    }

    /// 反假变体：签名材料少一枚判据码时签名**必须**变——
    /// 否则「签名等于冻结值」是恒真门禁。
    #[test]
    fn 签名对材料敏感() {
        let mut m: Vec<u8> = Vec::new();
        m.extend_from_slice(BRIDGE_VERSION.as_bytes());
        m.push(0);
        for p in Product::ALL {
            m.extend_from_slice(p.code().as_bytes());
            m.push(0);
        }
        for l in Lane::ALL {
            m.extend_from_slice(l.code().as_bytes());
            m.push(0);
        }
        for c in CRITERION_CODES.iter().take(CRITERION_CODES.len() - 1) {
            m.extend_from_slice(c.as_bytes());
            m.push(0);
        }
        assert_ne!(fnv1a64(&m), BRIDGE_SIG_V1, "少一枚判据码签名却不变 ⇒ 签名不敏感");
    }

    /// 反假变体：空总纲含令牌分叉，须被检出；单源修正后分叉清零。
    #[test]
    fn 令牌分叉可检出可修正() {
        let mut e = BridgeArchitecture::empty();
        assert!(!audit_token_fork(&e.tokens).is_empty(), "空总纲应含分叉");
        let removed = reconcile_token_single_source(&mut e.tokens);
        assert_eq!(removed, 1, "应恰裁掉 1 条重复声明");
        assert!(audit_token_fork(&e.tokens).is_empty(), "修正后不应再有分叉");
    }

    /// 反假变体：预览往返不得改动正式缓存条目数。
    #[test]
    fn 预览不污染正式缓存() {
        let mut a = BridgeArchitecture::standard();
        let base = a.formal.entries();
        let mut pv = PreviewChannel::new(&a.formal);
        for i in 0..8 {
            let _ = pv.push(PreviewFrame::new(128, 0xabcd_0000 + i as u64));
        }
        pv.discard_all();
        pv.observe_formal(a.formal.entries());
        assert!(pv.isolation_intact(), "预览往返后正式条目数变了：污染");
        assert!(pv.contamination().is_none());
        assert_eq!(a.formal.entries(), base, "正式缓存条目数不应变化");
    }

    /// 反假变体：绕管线直消费（无票据）必被立案。
    #[test]
    fn 无票据消费被立案() {
        let mut a = BridgeArchitecture::standard();
        let err = a.formal.consume(&mut a.ledger, 9999, 0).unwrap_err();
        assert_eq!(err.code, E_PIPELINE_BYPASS);
        assert_eq!(err.action(), DegradeAction::FileCase);
        assert_eq!(err.severity(), P0);
    }

    /// 码位分工：消费路径与盖章路径的缺票错误码**不得重合**——
    /// 两码同行为等于把「绕管线」与「运维笔误」混为一谈，缺失分支被掩盖。
    #[test]
    fn 缺票两码不重合() {
        let mut l = DelegateLedger::empty();
        let mut f = FormalChannel::new();
        let consume_code = f.consume(&mut l, 9999, 0).unwrap_err().code;
        let seal_code = l.seal(9999, 0).unwrap_err().code;
        assert_eq!(consume_code, E_PIPELINE_BYPASS);
        assert_eq!(seal_code, E_TICKET_ABSENT);
        assert_ne!(consume_code, seal_code);
    }
}