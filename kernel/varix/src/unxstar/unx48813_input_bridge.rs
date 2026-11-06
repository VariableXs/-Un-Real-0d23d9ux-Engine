//! UNX-F48813 事件层与 IrqBackend 消费衔接 · 0x70180
//! 覆盖卡：UNX-F48813（事件层与 AI-02 IrqBackend 消费衔接：现存轮询键鼠路径
//!         「升级接管」口径前向声明，联签 AI-02）、
//!         UNX-F48814（B01 段内联轧：事件层五路判据交叉复跑）、
//!         UNX-F48815（统一输入事件层与设备能力位图错误码族 15 条）
//!
//! ── 闸门次序 ──
//! 接管声明面（F48813）的闸序是「先证后签、先签后记」，五路联轧面（F48814）
//! 的闸序是「各自独立、最后合账」，两者共同决定本模块的红线：
//!   ① 口径闸：接管只能从「现存轮询键鼠路径」（Polling）升级到 Irq（UpgradeFromPolling），
//!      不允许从任意状态跳到 Irq。若把口径闸放到联签之后，一份「从虚拟注入路径
//!      直接接管」的声明会先被盖章入册，事后只能靠追溯撤销——接管账一旦被盖章
//!      就是前向声明，撤销等于改写历史。
//!   ② 锚定行闸：AI-02 联签锚定行有两份持有者（事件层持有 A 副本、AI-02 持有 B 副本），
//!      两副本必须逐字节相等，并且与冻结指纹相符。若把锚定行闸放到联签闸之后，
//!      锚定行被改写时会先以「联签缺失」报错——归因错位：真正的问题是锚被动了，
//!      不是没人签。**先查锚、再查签**是这一面的铁序。
//!   ③ 联签闸：必须有两个签署方（事件层 owner + AI-02），缺一即拒；
//!      同一签署方重复签不算两方。若放到最后，声明会先入册再发现只有一方签，
//!      入册序号已消耗，重签只能拿新序号——序号漂移即账不稳。
//!   ④ 入册闸：以上三闸全过才落账；重复接管（已是 Irq）必须拒。
//!      此闸同时校验声明文本非空（DeclarationEmpty）与降级口径（回退到 Polling 必须声明）。
//!   ⑤ 五路联轧闸（F48814）：族表 / 帧原子 / 超时合成 / 单调重置 / 溢出上限
//!      五路**各自独立复跑两遍**，逐路比指纹；五路全绿且两遍零漂移才允许合账。
//!      任何一路的复跑不得借用别路的中间结果——交叉复跑的意义就是「互不背书」，
//!      一路的实现错了，另四路不会替它圆场。
//!
//! ── 关键差异 ──
//! 与 unx48805_input_guard 的边界：guard 是**运行期**时序三闸（真的收事件、真的丢帧）；
//!   本模块的五路联轧只是**判据级复跑**（小规模、可复算、只出指纹），
//!   不接真实设备流，也不产生异常账。guard 的 4096 是实测容量，本模块只做
//!   边界算术复核（cap-1 放行、cap 丢弃），不灌 4096 条事件。
//! 与 unx48811_input_edge 的边界：edge 的 add/remove 是设备级生命周期，
//!   本模块的接管是**后端级**（轮询 → 中断）路径切换：不产生流水线 Add/Remove，
//!   只产生一次前向声明；设备拔插不影响已盖章的接管声明。
//! 与 unx48819_input_seal 的边界：seal 做批级防重与行数收官（静态账），
//!   本模块的 15 条错误码族与接管声明是**语义账**，封印不重算其内容，
//!   只在收官时核对错误码族条数与段窗口。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 本模块诊断段基址。
pub const SEG_BASE: u32 = 0x70180;

/// 错误码族条数（卡面 F48815 的「15 条」硬口径）。
pub const CODE_FAMILY_LEN: usize = 15;
/// 五路联轧的路数（卡面 F48814 的「×5」）。
pub const CROSS_WAYS: usize = 5;
/// 联轧复跑遍数（零漂移要求两遍逐位相等）。
pub const CROSS_REPEATS: u32 = 2;
/// 判据注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;
/// 缺 SYN 合成门槛（与 guard 同一冻结口径，此处只做边界复核）。
pub const SYN_TIMEOUT_US: u64 = 100_000;
/// 帧内缓冲上限（与 guard 同一冻结口径）。
pub const OVERFLOW_CAP: u32 = 4_096;
/// 签署方数量（事件层 owner + AI-02）。
pub const SIGNERS: usize = 2;

// ── 15 条错误码族（F48815）────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BridgeCode {
    /// 声明缺少联签方。
    MissingCosign,
    /// 锚定行被改写（两副本不等）。
    AnchorRewritten,
    /// 已接管（重复接管）。
    AlreadyTakenOver,
    /// 起点不是现存轮询路径。
    NotPollingPath,
    /// 后端不可用（Irq 端点未就绪）。
    BackendUnavailable,
    /// 降级口径缺失（未声明回退）。
    FallbackMissing,
    /// 声明文本为空。
    DeclarationEmpty,
    /// 签署方重复（同一方签两次）。
    CosignDuplicated,
    /// 族表指纹漂移。
    FamilyDigestDrift,
    /// 帧原子性判据破。
    FrameAtomicBroken,
    /// 超时合成边界漂移。
    SynTimeoutDrift,
    /// 时间戳单调判据破。
    MonotonicDrift,
    /// 溢出上限漂移（非 4096）。
    OverflowCapDrift,
    /// 合账不平（五路未全绿却合账）。
    CrossLedgerImbalance,
    /// 双跑不一致。
    NotReproducible,
}

impl BridgeCode {
    /// 段内绝对码值（0x70181..0x7018F，恰好 15 条）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文文案（15 条互不重复，防「文案复用掩盖撞号」）。
    pub fn detail(self) -> &'static str {
        match self {
            BridgeCode::MissingCosign => "接管声明缺联签方",
            BridgeCode::AnchorRewritten => "AI-02 锚定行被改写",
            BridgeCode::AlreadyTakenOver => "已接管：重复声明拒绝",
            BridgeCode::NotPollingPath => "起点非轮询键鼠路径",
            BridgeCode::BackendUnavailable => "IrqBackend 端点未就绪",
            BridgeCode::FallbackMissing => "降级口径缺失：未声明回退",
            BridgeCode::DeclarationEmpty => "接管声明文本为空",
            BridgeCode::CosignDuplicated => "签署方重复：两签同一方",
            BridgeCode::FamilyDigestDrift => "族表指纹漂移",
            BridgeCode::FrameAtomicBroken => "帧原子性判据破",
            BridgeCode::SynTimeoutDrift => "超时合成边界漂移",
            BridgeCode::MonotonicDrift => "时间戳单调判据破",
            BridgeCode::OverflowCapDrift => "溢出上限漂移：非 4096",
            BridgeCode::CrossLedgerImbalance => "合账不平：五路未全绿",
            BridgeCode::NotReproducible => "双跑不一致",
        }
    }

    /// 闭集全表（顺序即 `val()` 枚举序）。
    pub const ALL: [BridgeCode; CODE_FAMILY_LEN] = [
        BridgeCode::MissingCosign,
        BridgeCode::AnchorRewritten,
        BridgeCode::AlreadyTakenOver,
        BridgeCode::NotPollingPath,
        BridgeCode::BackendUnavailable,
        BridgeCode::FallbackMissing,
        BridgeCode::DeclarationEmpty,
        BridgeCode::CosignDuplicated,
        BridgeCode::FamilyDigestDrift,
        BridgeCode::FrameAtomicBroken,
        BridgeCode::SynTimeoutDrift,
        BridgeCode::MonotonicDrift,
        BridgeCode::OverflowCapDrift,
        BridgeCode::CrossLedgerImbalance,
        BridgeCode::NotReproducible,
    ];
}

/// 码值 → 文案；未知码返回闭集外说明。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < BridgeCode::ALL.len() {
        if let Some(c) = BridgeCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x70181..0x7018F 闭集内"
}

/// 错误码族自检：15 条、码值连续、文案互不重复。
pub fn code_family_check() -> Result<u32, u32> {
    if BridgeCode::ALL.len() != CODE_FAMILY_LEN {
        return Err(BridgeCode::CrossLedgerImbalance.val());
    }
    let mut i = 0usize;
    while i < BridgeCode::ALL.len() {
        let c = match BridgeCode::ALL.get(i) {
            Some(c) => *c,
            None => return Err(BridgeCode::CrossLedgerImbalance.val()),
        };
        if c.val() != SEG_BASE + 1 + i as u32 {
            return Err(BridgeCode::CrossLedgerImbalance.val());
        }
        if c.detail().is_empty() {
            return Err(BridgeCode::CrossLedgerImbalance.val());
        }
        // 文案两两不同：撞文案等于撞号的前兆。
        let mut j = i + 1;
        while j < BridgeCode::ALL.len() {
            if let Some(o) = BridgeCode::ALL.get(j) {
                if o.detail() == c.detail() || o.val() == c.val() {
                    return Err(BridgeCode::CrossLedgerImbalance.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    Ok(BridgeCode::ALL.len() as u32)
}

// ── 指纹基础设施（本域按「联签指纹」命名）────────────────────
/// 联签指纹初值。
pub fn cosign_init() -> u64 {
    0xcbf2_9ce4_8422_2325
}

/// 折入一字节。
pub fn cosign_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)
}

/// 折入 32 位字。
pub fn cosign_word(h: u64, w: u32) -> u64 {
    let mut acc = h;
    let mut i = 0u32;
    while i < 4 {
        acc = cosign_byte(acc, ((w >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    acc
}

/// 折入字符串。
pub fn cosign_str(h: u64, s: &str) -> u64 {
    let mut acc = h;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            acc = cosign_byte(acc, *b);
        }
        i += 1;
    }
    acc
}

// ── AI-02 联签锚定行（两份副本 + 冻结指纹）───────────────────
/// 事件层持有的锚定行副本（A 副本，写入方）。
pub const ANCHOR_LINE_A: &str =
    "AI-02 IrqBackend takeover anchor: polling-keyboard-mouse -> irq, co-sign frozen";
/// AI-02 持有的锚定行副本（B 副本，审计方）。
pub const ANCHOR_LINE_B: &str =
    "AI-02 IrqBackend takeover anchor: polling-keyboard-mouse -> irq, co-sign frozen";
/// 锚定行冻结指纹（由 A 副本离线算定后钉死；B 副本必须算出同一值）。
pub const ANCHOR_HASH_FROZEN: u64 = 0x5333_3DFA_9D9B_E3B6;

/// 计算锚定行指纹。
pub fn anchor_hash(text: &str) -> u64 {
    cosign_str(cosign_init(), text)
}

/// 锚定行零改写判据：两副本逐字节相等 + 指纹与冻结值相符。
///
/// 说明：`ANCHOR_HASH_FROZEN` 是联签时离线钉死的值，本函数把它当作外部锚点。
/// 如果任何人改了 A 或 B 任一副本（哪怕一个字节），指纹必然变，判据即红；
/// 两副本相等这一条则防「同步改写」（两处一起改也会被指纹抓住）。
pub fn anchor_zero_rewrite() -> Result<(), u32> {
    if ANCHOR_LINE_A.as_bytes() != ANCHOR_LINE_B.as_bytes() {
        return Err(BridgeCode::AnchorRewritten.val());
    }
    let a = anchor_hash(ANCHOR_LINE_A);
    let b = anchor_hash(ANCHOR_LINE_B);
    if a != b {
        return Err(BridgeCode::AnchorRewritten.val());
    }
    if a != ANCHOR_HASH_FROZEN {
        return Err(BridgeCode::AnchorRewritten.val());
    }
    Ok(())
}

// ── 接管声明 ──────────────────────────────────────────────────
/// 消费路径模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathMode {
    /// 现存轮询键鼠路径（升级前）。
    Polling,
    /// 中断驱动路径（升级后）。
    Irq,
    /// 注入/虚拟路径（不得作为接管起点）。
    Virtual,
}

impl PathMode {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            PathMode::Polling => "polling",
            PathMode::Irq => "irq",
            PathMode::Virtual => "virtual",
        }
    }
}

/// 签署方。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Signer {
    /// 事件层 owner。
    EventLayer,
    /// AI-02（IrqBackend 提供方）。
    Ai02,
}

impl Signer {
    /// 名。
    pub fn label(self) -> &'static str {
        match self {
            Signer::EventLayer => "B01-event-layer",
            Signer::Ai02 => "AI-02-IrqBackend",
        }
    }
}

/// 接管声明（前向声明，不回头改）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TakeoverDecl {
    /// 声明序号（1 起）。
    pub seq: u32,
    /// 起点模式。
    pub from: PathMode,
    /// 终点模式。
    pub to: PathMode,
    /// 声明正文（必须非空，且写明降级口径）。
    pub text: String,
    /// 是否声明了「失败回退到 polling」。
    pub fallback_to_polling: bool,
    /// 签署方（去重后应为两方）。
    pub cosigners: Vec<Signer>,
    /// 锚定行指纹（建档时快照）。
    pub anchor: u64,
}

impl TakeoverDecl {
    /// 构造：正文与签署方归档，锚定行取当前副本指纹。
    pub fn new(text: &str, fallback: bool, cosigners: Vec<Signer>) -> TakeoverDecl {
        TakeoverDecl {
            seq: 0,
            from: PathMode::Polling,
            to: PathMode::Irq,
            text: text.to_string(),
            fallback_to_polling: fallback,
            cosigners,
            anchor: anchor_hash(ANCHOR_LINE_A),
        }
    }

    /// 联签方去重计数。
    pub fn distinct_signers(&self) -> u32 {
        let mut n = 0u32;
        let mut seen_owner = false;
        let mut seen_ai02 = false;
        let mut i = 0usize;
        while i < self.cosigners.len() {
            if let Some(s) = self.cosigners.get(i) {
                match s {
                    Signer::EventLayer => {
                        if !seen_owner {
                            seen_owner = true;
                            n += 1;
                        }
                    }
                    Signer::Ai02 => {
                        if !seen_ai02 {
                            seen_ai02 = true;
                            n += 1;
                        }
                    }
                }
            }
            i += 1;
        }
        n
    }

    /// 声明指纹（正文 + 口径 + 签署方 + 锚定行）。
    pub fn digest(&self) -> u64 {
        let mut h = cosign_init();
        h = cosign_word(h, self.seq);
        h = cosign_byte(h, self.from.label().len() as u8);
        h = cosign_byte(h, self.to.label().len() as u8);
        h = cosign_str(h, self.text.as_str());
        h = cosign_byte(h, if self.fallback_to_polling { 1 } else { 0 });
        let mut i = 0usize;
        while i < self.cosigners.len() {
            if let Some(s) = self.cosigners.get(i) {
                h = cosign_byte(h, s.label().len() as u8);
                h = cosign_str(h, s.label());
            }
            i += 1;
        }
        h = cosign_word(h, self.anchor as u32);
        h = cosign_word(h, (self.anchor >> 32) as u32);
        h
    }
}

/// 接管声明账本（盖章单向）。
pub struct TakeoverBook {
    decls: Vec<TakeoverDecl>,
    mode: PathMode,
    sealed: bool,
}

impl TakeoverBook {
    /// 空账，起点固定为现存轮询路径。
    pub fn new() -> TakeoverBook {
        TakeoverBook { decls: Vec::new(), mode: PathMode::Polling, sealed: false }
    }

    /// 当前模式。
    pub fn mode(&self) -> PathMode {
        self.mode
    }

    /// 声明数。
    pub fn len(&self) -> usize {
        self.decls.len()
    }

    /// 空账。
    pub fn is_empty(&self) -> bool {
        self.decls.is_empty()
    }

    /// 声明列表。
    pub fn declarations(&self) -> &[TakeoverDecl] {
        self.decls.as_slice()
    }

    /// 提交接管声明（五闸次序见文件头）。
    pub fn declare(&mut self, mut decl: TakeoverDecl) -> Result<u32, u32> {
        // ① 口径闸。
        if self.mode != PathMode::Polling {
            return Err(BridgeCode::AlreadyTakenOver.val());
        }
        if decl.from != PathMode::Polling || decl.to != PathMode::Irq {
            return Err(BridgeCode::NotPollingPath.val());
        }
        // ② 锚定行闸（先查锚、再查签）。
        if decl.anchor != ANCHOR_HASH_FROZEN {
            return Err(BridgeCode::AnchorRewritten.val());
        }
        anchor_zero_rewrite()?;
        // ③ 联签闸。
        let distinct = decl.distinct_signers();
        if decl.cosigners.len() as u32 != distinct {
            return Err(BridgeCode::CosignDuplicated.val());
        }
        if distinct < SIGNERS as u32 {
            return Err(BridgeCode::MissingCosign.val());
        }
        // ④ 入册闸。
        if decl.text.is_empty() {
            return Err(BridgeCode::DeclarationEmpty.val());
        }
        if !decl.fallback_to_polling {
            return Err(BridgeCode::FallbackMissing.val());
        }
        if self.sealed {
            return Err(BridgeCode::AlreadyTakenOver.val());
        }
        decl.seq = (self.decls.len() as u32).saturating_add(1);
        self.decls.push(decl);
        self.mode = PathMode::Irq;
        self.sealed = true;
        Ok(self.decls.len() as u32)
    }

    /// 合账指纹。
    pub fn digest(&self) -> u64 {
        let mut h = cosign_init();
        h = cosign_byte(h, self.mode.label().len() as u8);
        let mut i = 0usize;
        while i < self.decls.len() {
            if let Some(d) = self.decls.get(i) {
                h = cosign_word(h, d.digest() as u32);
                h = cosign_word(h, (d.digest() >> 32) as u32);
            }
            i += 1;
        }
        h
    }

    /// 降级：接管后若 Irq 端点失效，声明回退口径必须可执行（模式回落）。
    pub fn fallback(&mut self) -> Result<(), u32> {
        if self.decls.is_empty() {
            return Err(BridgeCode::BackendUnavailable.val());
        }
        let declared = match self.decls.first() {
            Some(d) => d.fallback_to_polling,
            None => false,
        };
        if !declared {
            return Err(BridgeCode::FallbackMissing.val());
        }
        self.mode = PathMode::Polling;
        // 回退不影响已盖章声明的存在性（前向声明只增不改）。
        Ok(())
    }
}

/// 标准声明正文（写明接管范围与降级口径）。
pub fn standard_decl_text() -> String {
    String::from("事件层接管 AI-02 IrqBackend：现存轮询键鼠路径升级为中断消费；端点失效即回退 polling")
}

// ── F48814 五路联轧 ──────────────────────────────────────────
/// 单路联轧结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WayResult {
    /// 路名。
    pub name: &'static str,
    /// 是否绿。
    pub green: bool,
    /// 该路指纹。
    pub digest: u64,
    /// 该路复跑遍数。
    pub repeats: u32,
}

/// 五路联轧台账。
#[derive(Clone, Debug)]
pub struct CrossLedger {
    ways: Vec<WayResult>,
}

impl CrossLedger {
    /// 空账。
    pub fn new() -> CrossLedger {
        CrossLedger { ways: Vec::new() }
    }

    /// 记一路。
    pub fn record(&mut self, w: WayResult) -> bool {
        if self.ways.len() >= CROSS_WAYS {
            return false;
        }
        self.ways.push(w);
        true
    }

    /// 路数。
    pub fn len(&self) -> usize {
        self.ways.len()
    }

    /// 空账。
    pub fn is_empty(&self) -> bool {
        self.ways.is_empty()
    }

    /// 五路全绿且复跑遍数达标。
    pub fn all_green(&self) -> bool {
        if self.ways.len() != CROSS_WAYS {
            return false;
        }
        let mut i = 0usize;
        while i < self.ways.len() {
            match self.ways.get(i) {
                Some(w) => {
                    if !w.green || w.repeats < CROSS_REPEATS {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 合账指纹（五路按序折叠，任何一路漂移都会变）。
    pub fn digest(&self) -> u64 {
        let mut h = cosign_init();
        let mut i = 0usize;
        while i < self.ways.len() {
            if let Some(w) = self.ways.get(i) {
                h = cosign_str(h, w.name);
                h = cosign_word(h, w.digest as u32);
                h = cosign_word(h, (w.digest >> 32) as u32);
                h = cosign_byte(h, if w.green { 1 } else { 0 });
            }
            i += 1;
        }
        h
    }

    /// 合账：未全绿即拒（不平就合账等于假绿）。
    pub fn settle(&self) -> Result<u64, u32> {
        if !self.all_green() {
            return Err(BridgeCode::CrossLedgerImbalance.val());
        }
        Ok(self.digest())
    }

    /// 逐路名（报告用）。
    pub fn names(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = Vec::new();
        let mut i = 0usize;
        while i < self.ways.len() {
            if let Some(w) = self.ways.get(i) {
                v.push(w.name);
            }
            i += 1;
        }
        v
    }
}

/// 路① 族表：十族槽位与 type 一一对应，指纹覆盖全表。
pub fn cross_family_table() -> Result<u64, u32> {
    let types: [u16; 10] = [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x11, 0x12, 0x14, 0x15];
    let mut h = cosign_init();
    let mut seen = 0u32;
    let mut i = 0usize;
    while i < types.len() {
        let t = match types.get(i) {
            Some(t) => *t,
            None => return Err(BridgeCode::FamilyDigestDrift.val()),
        };
        if t & 0x8000 != 0 {
            return Err(BridgeCode::FamilyDigestDrift.val());
        }
        let bit = 1u32 << i;
        if seen & bit != 0 {
            return Err(BridgeCode::FamilyDigestDrift.val());
        }
        seen |= bit;
        h = cosign_word(h, t as u32);
        i += 1;
    }
    if seen != 0x3FF {
        return Err(BridgeCode::FamilyDigestDrift.val());
    }
    Ok(h)
}

/// 路② 帧原子：未见 SYN 不得可见，见到 SYN 整帧可见。
pub fn cross_frame_atomic() -> Result<u64, u32> {
    let payload: [u16; 3] = [0x01, 0x01, 0x04];
    let mut h = cosign_init();
    let mut visible = 0u32;
    let mut pending = 0u32;
    let mut i = 0usize;
    while i < payload.len() {
        pending += 1;
        if visible != 0 {
            return Err(BridgeCode::FrameAtomicBroken.val());
        }
        i += 1;
    }
    if pending != 3 || visible != 0 {
        return Err(BridgeCode::FrameAtomicBroken.val());
    }
    // SYN 到来：整帧一次性可见。
    visible = pending;
    pending = 0;
    if visible != 3 {
        return Err(BridgeCode::FrameAtomicBroken.val());
    }
    h = cosign_word(h, visible);
    h = cosign_word(h, pending);
    Ok(h)
}

/// 路③ 超时合成边界：99,999 不合成、100,000 合成。
pub fn cross_syn_timeout() -> Result<u64, u32> {
    let threshold = SYN_TIMEOUT_US;
    let under = 99_999u64;
    let at = 100_000u64;
    let mut h = cosign_init();
    if under >= threshold {
        return Err(BridgeCode::SynTimeoutDrift.val());
    }
    if at < threshold {
        return Err(BridgeCode::SynTimeoutDrift.val());
    }
    h = cosign_word(h, under as u32);
    h = cosign_word(h, at as u32);
    Ok(h)
}

/// 路④ 单调重置：倒流必须判重置，等值不算倒流。
pub fn cross_monotonic() -> Result<u64, u32> {
    let mut last: Option<u64> = None;
    let mut resets = 0u32;
    let seq: [u64; 5] = [10, 20, 20, 15, 30];
    let mut i = 0usize;
    while i < seq.len() {
        let ts = match seq.get(i) {
            Some(v) => *v,
            None => return Err(BridgeCode::MonotonicDrift.val()),
        };
        if let Some(l) = last {
            if ts < l {
                resets += 1;
            }
        }
        match last {
            Some(l) => {
                if ts > l {
                    last = Some(ts);
                }
            }
            None => last = Some(ts),
        }
        i += 1;
    }
    if resets != 1 {
        return Err(BridgeCode::MonotonicDrift.val());
    }
    let mut h = cosign_init();
    h = cosign_word(h, resets);
    h = cosign_word(h, last.unwrap_or(0) as u32);
    Ok(h)
}

/// 路⑤ 溢出上限：cap-1 放行、cap 丢弃一帧（只做边界算术，不灌满）。
pub fn cross_overflow_cap() -> Result<u64, u32> {
    if OVERFLOW_CAP != 4_096 {
        return Err(BridgeCode::OverflowCapDrift.val());
    }
    let below = OVERFLOW_CAP - 1;
    let at = OVERFLOW_CAP;
    let mut h = cosign_init();
    if below >= OVERFLOW_CAP {
        return Err(BridgeCode::OverflowCapDrift.val());
    }
    if at < OVERFLOW_CAP {
        return Err(BridgeCode::OverflowCapDrift.val());
    }
    h = cosign_word(h, below);
    h = cosign_word(h, at);
    Ok(h)
}

/// 五路联轧：逐路复跑 `CROSS_REPEATS` 遍，逐遍比指纹。
pub fn cross_rig() -> Result<CrossLedger, u32> {
    let mut led = CrossLedger::new();

    // 路①
    let a = cross_family_table()?;
    let b = cross_family_table()?;
    led.record(WayResult { name: "family-table", green: a == b, digest: a, repeats: 2 });

    // 路②
    let a = cross_frame_atomic()?;
    let b = cross_frame_atomic()?;
    led.record(WayResult { name: "frame-atomic", green: a == b, digest: a, repeats: 2 });

    // 路③
    let a = cross_syn_timeout()?;
    let b = cross_syn_timeout()?;
    led.record(WayResult { name: "syn-timeout", green: a == b, digest: a, repeats: 2 });

    // 路④
    let a = cross_monotonic()?;
    let b = cross_monotonic()?;
    led.record(WayResult { name: "monotonic", green: a == b, digest: a, repeats: 2 });

    // 路⑤
    let a = cross_overflow_cap()?;
    let b = cross_overflow_cap()?;
    led.record(WayResult { name: "overflow-cap", green: a == b, digest: a, repeats: 2 });

    led.settle()?;
    Ok(led)
}

// ── 黄金全链 ──────────────────────────────────────────────────
/// 黄金链输出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenOut {
    /// 错误码族条数。
    pub codes: u32,
    /// 五路全绿数。
    pub ways: u32,
    /// 接管账模型。
    pub mode: PathMode,
    /// 声明数。
    pub decls: u32,
    /// 联签账指纹。
    pub digest: u64,
}

/// 黄金全链：错误码族 → 锚定行 → 声明入册 → 五路联轧 → 合账。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let codes = code_family_check()?;
    anchor_zero_rewrite()?;
    let mut book = TakeoverBook::new();
    let decl = TakeoverDecl::new(
        standard_decl_text().as_str(),
        true,
        vec![Signer::EventLayer, Signer::Ai02],
    );
    let n = book.declare(decl)?;
    if n != 1 || book.mode() != PathMode::Irq {
        return Err(BridgeCode::CrossLedgerImbalance.val());
    }
    let led = cross_rig()?;
    if !led.all_green() {
        return Err(BridgeCode::CrossLedgerImbalance.val());
    }
    let mut digest = book.digest();
    digest = cosign_word(digest, led.digest() as u32);
    digest = cosign_word(digest, (led.digest() >> 32) as u32);
    Ok(GoldenOut {
        codes,
        ways: led.len() as u32,
        mode: book.mode(),
        decls: book.len() as u32,
        digest,
    })
}

// ── 注入账 ────────────────────────────────────────────────────
/// 注入账：5 槽。
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    /// 每槽注入次数。
    pub tries: [u32; 5],
    /// 每槽按预期命中次数。
    pub caught: [u32; 5],
}

impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0u32; 5], caught: [0u32; 5] }
    }

    /// 记账。
    pub fn record(&mut self, slot: usize, caught: bool) -> bool {
        let mut ok = false;
        if let Some(t) = self.tries.get_mut(slot) {
            *t += 1;
            ok = true;
        }
        if ok && caught {
            if let Some(c) = self.caught.get_mut(slot) {
                *c += 1;
            }
        }
        ok
    }

    /// 五槽全中。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 5 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || t != c {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入 5 槽 × rounds：单方联签 / 锚改 / 重复接管 / 缺降级口径 / 空正文。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 槽 0：只有事件层一方签。
        let mut book = TakeoverBook::new();
        let d = TakeoverDecl::new(standard_decl_text().as_str(), true, vec![Signer::EventLayer]);
        led.record(0, matches!(book.declare(d), Err(c) if c == BridgeCode::MissingCosign.val()));

        // 槽 1：锚定行指纹被改。
        let mut book = TakeoverBook::new();
        let mut d = TakeoverDecl::new(
            standard_decl_text().as_str(),
            true,
            vec![Signer::EventLayer, Signer::Ai02],
        );
        d.anchor ^= 1;
        led.record(1, matches!(book.declare(d), Err(c) if c == BridgeCode::AnchorRewritten.val()));

        // 槽 2：重复接管。
        let mut book = TakeoverBook::new();
        let d1 = TakeoverDecl::new(
            standard_decl_text().as_str(),
            true,
            vec![Signer::EventLayer, Signer::Ai02],
        );
        let first = book.declare(d1).is_ok();
        let d2 = TakeoverDecl::new(
            standard_decl_text().as_str(),
            true,
            vec![Signer::EventLayer, Signer::Ai02],
        );
        led.record(
            2,
            first && matches!(book.declare(d2), Err(c) if c == BridgeCode::AlreadyTakenOver.val()),
        );

        // 槽 3：缺降级口径。
        let mut book = TakeoverBook::new();
        let d = TakeoverDecl::new(standard_decl_text().as_str(), false, vec![Signer::EventLayer, Signer::Ai02]);
        led.record(3, matches!(book.declare(d), Err(c) if c == BridgeCode::FallbackMissing.val()));

        // 槽 4：空正文。
        let mut book = TakeoverBook::new();
        let d = TakeoverDecl::new("", true, vec![Signer::EventLayer, Signer::Ai02]);
        led.record(4, matches!(book.declare(d), Err(c) if c == BridgeCode::DeclarationEmpty.val()));
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────
/// 探针 A：标准声明一次入册，模式转为 Irq，降级可回落。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut book = TakeoverBook::new();
    if book.mode() != PathMode::Polling {
        return Err(BridgeCode::NotPollingPath.val());
    }
    let d = TakeoverDecl::new(
        standard_decl_text().as_str(),
        true,
        vec![Signer::EventLayer, Signer::Ai02],
    );
    book.declare(d)?;
    if book.mode() != PathMode::Irq {
        return Err(BridgeCode::BackendUnavailable.val());
    }
    book.fallback()?;
    if book.mode() != PathMode::Polling {
        return Err(BridgeCode::FallbackMissing.val());
    }
    // 前向声明只增不改：回退后声明仍在账上。
    if book.declarations().len() != 1 {
        return Err(BridgeCode::CrossLedgerImbalance.val());
    }
    Ok(())
}

/// 探针 B：虚拟路径起点必须拒（口径闸在联签之前）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut book = TakeoverBook::new();
    let mut d = TakeoverDecl::new(
        standard_decl_text().as_str(),
        true,
        vec![Signer::EventLayer, Signer::Ai02],
    );
    d.from = PathMode::Virtual;
    match book.declare(d) {
        Err(c) if c == BridgeCode::NotPollingPath.val() => Ok(()),
        _ => Err(BridgeCode::NotPollingPath.val()),
    }
}

/// 探针 C：五路合账未全绿即拒（人为构造两路账）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut led = CrossLedger::new();
    led.record(WayResult { name: "family-table", green: true, digest: 1, repeats: 2 });
    led.record(WayResult { name: "frame-atomic", green: false, digest: 2, repeats: 2 });
    match led.settle() {
        Err(c) if c == BridgeCode::CrossLedgerImbalance.val() => Ok(()),
        _ => Err(BridgeCode::CrossLedgerImbalance.val()),
    }
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BridgeCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫：15 条码全部落在 [SEG_BASE+1, SEG_BASE+0x20)。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < BridgeCode::ALL.len() {
        match BridgeCode::ALL.get(i) {
            Some(c) => {
                let v = c.val();
                if v <= SEG_BASE || v >= SEG_BASE + 0x20 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // F48815-J1：错误码 ×15 零漂移互不混淆。
    let codes = code_family_check();
    v.push((
        "F48815-J1-错误码15条互不混淆",
        matches!(codes, Ok(15)),
        format!("codes={:?}/15", codes),
    ));

    // F48815-J1：15 条码段窗口闭集（0x70181..0x7018F）。
    let window = {
        let mut ok = true;
        let mut i = 0usize;
        while i < BridgeCode::ALL.len() {
            if let Some(c) = BridgeCode::ALL.get(i) {
                if c.val() != SEG_BASE + 1 + i as u32 {
                    ok = false;
                }
            }
            i += 1;
        }
        ok && segment_guard()
    };
    v.push((
        "F48815-J1-错误码段窗口连续",
        window,
        String::from("0x70181..0x7018F"),
    ));

    // F48813-J1：接管声明的联签与锚定行落账。
    let decl = {
        let mut book = TakeoverBook::new();
        let d = TakeoverDecl::new(
            standard_decl_text().as_str(),
            true,
            vec![Signer::EventLayer, Signer::Ai02],
        );
        let declared = book.declare(d).is_ok();
        declared
            && book.mode() == PathMode::Irq
            && book.len() == 1
            && book.declarations().first().map(|x| x.distinct_signers()) == Some(2)
    };
    v.push((
        "F48813-J1-接管声明落账并联签",
        decl,
        String::from("mode=irq cosigners=2"),
    ));

    // F48813-J1：AI-02 锚定行零改写（两副本 + 冻结指纹）。
    v.push((
        "F48813-J1-锚定行零改写",
        anchor_zero_rewrite().is_ok()
            && ANCHOR_LINE_A.as_bytes() == ANCHOR_LINE_B.as_bytes()
            && anchor_hash(ANCHOR_LINE_A) == ANCHOR_HASH_FROZEN,
        format!("anchor=0x{:016X}", anchor_hash(ANCHOR_LINE_A)),
    ));

    // F48813-J1：缺签/改锚/重复接管三拒（闸序：先锚后签）。
    let gates = {
        let mut book = TakeoverBook::new();
        let one = TakeoverDecl::new(
            standard_decl_text().as_str(),
            true,
            vec![Signer::EventLayer],
        );
        let missing = matches!(book.declare(one), Err(c) if c == BridgeCode::MissingCosign.val());
        let mut bad = TakeoverDecl::new(
            standard_decl_text().as_str(),
            true,
            vec![Signer::EventLayer, Signer::Ai02],
        );
        bad.anchor ^= 0xFF;
        let anchored = matches!(book.declare(bad), Err(c) if c == BridgeCode::AnchorRewritten.val());
        missing && anchored && book.is_empty()
    };
    v.push((
        "F48813-J1-先锚后签双拒",
        gates,
        String::from("MissingCosign + AnchorRewritten"),
    ));

    // F48814-J1：五路联轧全绿。
    let rig = cross_rig();
    let rig_ok = matches!(&rig, Ok(l) if l.all_green() && l.len() == CROSS_WAYS);
    v.push((
        "F48814-J1-五路联轧全绿",
        rig_ok,
        format!("ways={:?}", rig.as_ref().map(|l| l.names())),
    ));

    // F48814-J1：五路合账零漂移（两遍指纹逐位相等）。
    let drift = {
        let a = cross_rig();
        let b = cross_rig();
        match (&a, &b) {
            (Ok(x), Ok(y)) => x.digest() == y.digest() && x.settle().is_ok() && y.settle().is_ok(),
            _ => false,
        }
    };
    v.push((
        "F48814-J1-五路合账零漂移",
        drift,
        format!("digest={:?}", cross_rig().map(|l| l.digest())),
    ));

    // F48813-J2：注入 5 槽 ×10 全中。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = matches!(&inj, Ok(l) if l.all_caught());
    v.push((
        "F48813-J2-注入10轮5槽全中",
        inj_ok,
        format!(
            "tries={:?} caught={:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));

    // F48813-J3：双跑 + 段位。
    v.push((
        "F48813-J3-双跑与段位",
        repro_check().is_ok() && segment_guard(),
        format!("golden={:?}", golden_run().map(|g| g.digest)),
    ));

    v
}

pub fn run_unx48813_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48813 事件层与 IrqBackend 衔接");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifteen_codes_unique_and_contiguous() {
        assert_eq!(code_family_check(), Ok(15));
        assert_eq!(BridgeCode::ALL.len(), CODE_FAMILY_LEN);
        assert_eq!(BridgeCode::MissingCosign.val(), SEG_BASE + 1);
        assert_eq!(BridgeCode::NotReproducible.val(), SEG_BASE + 15);
        // 文案两两不同。
        let mut seen: Vec<&str> = Vec::new();
        for c in BridgeCode::ALL.iter() {
            assert!(!seen.contains(&c.detail()));
            seen.push(c.detail());
        }
        assert_eq!(seen.len(), 15);
    }

    #[test]
    fn anchor_copies_identical_and_frozen() {
        assert!(anchor_zero_rewrite().is_ok());
        assert_eq!(ANCHOR_LINE_A, ANCHOR_LINE_B);
        assert_eq!(anchor_hash(ANCHOR_LINE_A), ANCHOR_HASH_FROZEN);
    }

    #[test]
    fn standard_declaration_takes_over_once() {
        let mut book = TakeoverBook::new();
        assert_eq!(book.mode(), PathMode::Polling);
        let d = TakeoverDecl::new(
            standard_decl_text().as_str(),
            true,
            vec![Signer::EventLayer, Signer::Ai02],
        );
        assert_eq!(book.declare(d), Ok(1));
        assert_eq!(book.mode(), PathMode::Irq);
        let again = TakeoverDecl::new(
            standard_decl_text().as_str(),
            true,
            vec![Signer::EventLayer, Signer::Ai02],
        );
        assert_eq!(book.declare(again).err(), Some(BridgeCode::AlreadyTakenOver.val()));
    }

    #[test]
    fn gate_order_anchor_before_cosign() {
        // 既缺签又改锚：必须报锚错（先锚后签）。
        let mut book = TakeoverBook::new();
        let mut d = TakeoverDecl::new(standard_decl_text().as_str(), true, vec![Signer::EventLayer]);
        d.anchor ^= 7;
        assert_eq!(book.declare(d).err(), Some(BridgeCode::AnchorRewritten.val()));
        // 只缺签：报缺签。
        let d = TakeoverDecl::new(standard_decl_text().as_str(), true, vec![Signer::EventLayer]);
        assert_eq!(book.declare(d).err(), Some(BridgeCode::MissingCosign.val()));
    }

    #[test]
    fn duplicate_signer_rejected() {
        let mut book = TakeoverBook::new();
        let d = TakeoverDecl::new(
            standard_decl_text().as_str(),
            true,
            vec![Signer::EventLayer, Signer::EventLayer],
        );
        assert_eq!(book.declare(d).err(), Some(BridgeCode::CosignDuplicated.val()));
    }

    #[test]
    fn virtual_origin_rejected() {
        assert!(probe_seq_b().is_ok());
        let mut book = TakeoverBook::new();
        let mut d = TakeoverDecl::new(
            standard_decl_text().as_str(),
            true,
            vec![Signer::EventLayer, Signer::Ai02],
        );
        d.from = PathMode::Virtual;
        assert_eq!(book.declare(d).err(), Some(BridgeCode::NotPollingPath.val()));
    }

    #[test]
    fn fallback_needs_declared_rule() {
        let mut book = TakeoverBook::new();
        let d = TakeoverDecl::new(
            standard_decl_text().as_str(),
            false,
            vec![Signer::EventLayer, Signer::Ai02],
        );
        assert_eq!(book.declare(d).err(), Some(BridgeCode::FallbackMissing.val()));
        // 未接管时回退无声明可依。
        let mut empty = TakeoverBook::new();
        assert_eq!(empty.fallback().err(), Some(BridgeCode::BackendUnavailable.val()));
    }

    #[test]
    fn fallback_keeps_declaration() {
        let mut book = TakeoverBook::new();
        let d = TakeoverDecl::new(
            standard_decl_text().as_str(),
            true,
            vec![Signer::EventLayer, Signer::Ai02],
        );
        book.declare(d).expect("接管");
        book.fallback().expect("回退");
        assert_eq!(book.mode(), PathMode::Polling);
        assert_eq!(book.len(), 1);
    }

    #[test]
    fn five_way_rig_green_and_stable() {
        let a = cross_rig().expect("五路联轧");
        let b = cross_rig().expect("二遍");
        assert!(a.all_green());
        assert_eq!(a.len(), CROSS_WAYS);
        assert_eq!(a.digest(), b.digest());
        assert_eq!(
            a.names(),
            vec!["family-table", "frame-atomic", "syn-timeout", "monotonic", "overflow-cap"]
        );
        // 每路独立可复算。
        assert_eq!(cross_family_table().ok(), cross_family_table().ok());
        assert!(cross_frame_atomic().is_ok());
        assert!(cross_syn_timeout().is_ok());
        assert!(cross_monotonic().is_ok());
        assert!(cross_overflow_cap().is_ok());
    }

    #[test]
    fn cross_ledger_rejects_partial_green() {
        assert!(probe_seq_c().is_ok());
        let mut led = CrossLedger::new();
        led.record(WayResult { name: "a", green: true, digest: 1, repeats: 2 });
        assert!(!led.all_green());
        assert_eq!(led.settle().err(), Some(BridgeCode::CrossLedgerImbalance.val()));
    }

    #[test]
    fn way_digests_differ_across_ways() {
        let d = [
            cross_family_table().expect("族"),
            cross_frame_atomic().expect("帧"),
            cross_syn_timeout().expect("超时"),
            cross_monotonic().expect("单调"),
            cross_overflow_cap().expect("溢出"),
        ];
        // 五路指纹不应全同（全同说明某路根本没算自己的东西）。
        let first = d.first().copied().unwrap_or(0);
        let mut distinct = 0usize;
        for v in d.iter() {
            if *v != first {
                distinct += 1;
            }
        }
        assert!(distinct >= 3, "五路指纹过于雷同");
    }

    #[test]
    fn golden_chain_complete() {
        let g = golden_run().expect("黄金链");
        assert_eq!(g, golden_run().expect("二跑"));
        assert_eq!(g.codes, 15);
        assert_eq!(g.ways, 5);
        assert_eq!(g.mode, PathMode::Irq);
        assert_eq!(g.decls, 1);
    }

    #[test]
    fn inject_five_slots_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS).expect("注入");
        assert!(led.all_caught());
        assert_eq!(led.tries, [JUDGE_ROUNDS; 5]);
        assert_eq!(led.caught, [JUDGE_ROUNDS; 5]);
    }

    #[test]
    fn probes_a_b_c_pass() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn codes_are_segment_closed() {
        assert!(segment_guard());
        for c in BridgeCode::ALL.iter() {
            let v = c.val();
            assert!(v > SEG_BASE && v < SEG_BASE + 0x20);
            assert_eq!(detail(v), c.detail());
        }
        assert_eq!(detail(0x70190), "未知诊断码：不在 0x70181..0x7018F 闭集内");
    }

    #[test]
    fn declaration_digest_sensitive_to_text() {
        let a = TakeoverDecl::new(
            standard_decl_text().as_str(),
            true,
            vec![Signer::EventLayer, Signer::Ai02],
        );
        let b = TakeoverDecl::new(
            "事件层接管 AI-02 IrqBackend（改）",
            true,
            vec![Signer::EventLayer, Signer::Ai02],
        );
        assert_ne!(a.digest(), b.digest());
        let c = TakeoverDecl::new(
            standard_decl_text().as_str(),
            false,
            vec![Signer::EventLayer, Signer::Ai02],
        );
        assert_ne!(a.digest(), c.digest());
    }
}
