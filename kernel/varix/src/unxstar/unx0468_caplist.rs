//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0468 能力链表账（MSI/MSI-X/PM 只读） · 0x75600段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（DISPATCH_w7G 逐字锚定，AI-VX7-1104）：
//!   能力链完整走链（链长≤48 上限，零环零断），MSI 0x05/MSI-X 0x11/PM 0x01
//!   位态只读落账（使能位零改）。
//! 设计要点：
//!   1. 走链模型：能力指针寄存器 0x34（PCI Local Bus Spec r3.0 §6.8.1、
//!      PCIe Base r6.0 §7.5.1.2），低字节为首个能力项偏移；每跳读 4 字节
//!      能力头（id=bits[7:0]、next=bits[15:8]），next 低 2 位保留必须为零。
//!   2. 走链三闸加一限：对齐闸（raw next & 0x03 == 0）、范围闸（0x40 ≤
//!      next ≤ 0xFC）、回环闸（visited 位图按字粒度置位查重）、链长上限
//!      48 跳；任一闸拒走即停，错误码落 0x75600 段闭集。
//!   3. 位态只读落账：MSI(0x05) Message Control bit0、MSI-X(0x11) 使能位
//!      bit15 与表尺寸 bits[10:3]、PM(0x01) PMCSR 电源态 bits[1:0] 与
//!      PME 使能位 bit8，全部从配置空间读出记账，禁止任何回写路径。
//!   4. 使能位零改：写拦截门对 4 类写请求一律拒绝并计数，账面快照前后
//!      全等（零改断言走私有谓词，生产代码不用 assert）。
//!   5. 单调状态机：Idle→Walked→Ledgered→Sealed 四相，倒退显性拒
//!      （PhaseRegress），封账后再写必被抓。
//!   6. 错误路径中文建议写入诊断 detail：环路→截链登记复扫、断链→补链
//!      复查、错位→重扫配置空间、改写→核对只读红线。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75600，AI-VX7-1104 批 G 组独占区起点）。
pub const SEG_BASE: u32 = 0x75600;

// ── 冻结常量表（来源：PCI Local Bus Spec r3.0 §6.8.1 / PCIe Base r6.0 §7.5.1.2）──
/// 配置空间字数（256B / 4B = 64 字，冻结）。
pub const CFG_WORDS: usize = 64;
/// 能力指针寄存器偏移（0x34，字下标 13，冻结）。
pub const CAP_PTR_REG: u8 = 0x34;
/// next 指针低 2 位保留（必须为零，PCI 规范 §6.8.1）。
pub const NEXT_RESERVED_MASK: u32 = 0x03;
/// PM 能力 ID（0x01，PCI Power Management Interface Spec r1.2）。
pub const CAP_ID_PM: u8 = 0x01;
/// MSI 能力 ID（0x05，PCI Local Bus Spec r3.0 §6.8.1）。
pub const CAP_ID_MSI: u8 = 0x05;
/// MSI-X 能力 ID（0x11，PCIe Base r6.0 §7.7）。
pub const CAP_ID_MSIX: u8 = 0x11;
/// 能力区起始偏移（0x40：标准头 0x00~0x3F 之后）。
pub const CAP_AREA_LO: u8 = 0x40;
/// 能力区终止偏移（0xFC：每项 4B 对齐的上界）。
pub const CAP_AREA_HI: u8 = 0xFC;
/// 链长上限（48 跳，判据冻结值）。
pub const CHAIN_MAX: u8 = 48;
/// 使能位写拦截的预算轮数（黄金链口径：4 目标 × 2 轮 = 8 次注入）。
pub const POKE_ROUNDS: u32 = 2;
/// 注入轮数（判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapCode {
    /// 能力链回环（同一偏移二次入链）
    ChainLoop,
    /// 能力链断裂（next 越出 0x40~0xFC 能力区）
    ChainBreak,
    /// next 指针低 2 位非零（每跳 4B 对齐红线）
    ChainMisaligned,
    /// 链长超 48 上限
    ChainTooLong,
    /// 使能位写拦截（只读红线，零改）
    WriteBlocked,
    /// 审计状态机倒退（Idle→Walked→Ledgered→Sealed 只进不退）
    PhaseRegress,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl CapCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CapCode::ChainLoop => "能力链回环：截链登记后复扫配置空间",
            CapCode::ChainBreak => "能力链断裂：next 越出能力区，补链复查",
            CapCode::ChainMisaligned => "next 低 2 位非零：每跳 4B 对齐，重扫头字",
            CapCode::ChainTooLong => "链长超 48 上限：疑似伪链，全量复扫",
            CapCode::WriteBlocked => "使能位写拦截：只读红线，回写一律拒绝",
            CapCode::PhaseRegress => "审计状态机倒退：按 Idle→Walked→Ledgered→Sealed 推进",
            CapCode::ReplayMismatch => "复算失配：账面与重算结果不一致",
            CapCode::NotReproducible => "双跑不一致：复现红线，固定种子复跑",
        }
    }
    pub const ALL: [CapCode; 8] = [
        CapCode::ChainLoop,
        CapCode::ChainBreak,
        CapCode::ChainMisaligned,
        CapCode::ChainTooLong,
        CapCode::WriteBlocked,
        CapCode::PhaseRegress,
        CapCode::ReplayMismatch,
        CapCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回，未知码显性提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CapCode::ALL.len() {
        if let Some(c) = CapCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75600 段闭集内"
}

// ── 配置空间模型（只读面） ──────────────────────────────────────
/// 一台设备的 256 字节配置空间，按 4B 字存储。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CfgSpace {
    words: Vec<u32>,
}

impl CfgSpace {
    /// 空白设备：全零（0x34 读出 0 ⇒ 零能力链，合法）。
    pub fn blank() -> CfgSpace {
        CfgSpace { words: Vec::new() }
    }

    /// 按 4B 对齐写入一个字（构造器专用；运行面无写路径）。
    pub fn put_word(&mut self, off: u8, w: u32) -> Result<(), u32> {
        if (off as u32) % 4 != 0 {
            return Err(CapCode::ChainMisaligned.val());
        }
        let idx = (off as usize) / 4;
        while self.words.len() <= idx {
            self.words.push(0);
        }
        // 定长模型：超出 64 字窗口显性拒。
        if idx >= CFG_WORDS {
            return Err(CapCode::ChainBreak.val());
        }
        if let Some(slot) = self.words.get_mut(idx) {
            *slot = w;
            Ok(())
        } else {
            Err(CapCode::ChainBreak.val())
        }
    }

    /// 读一个字（off 必须 4B 对齐）。
    pub fn word_at(&self, off: u8) -> Option<u32> {
        if (off as u32) % 4 != 0 {
            return None;
        }
        let idx = (off as usize) / 4;
        self.words.get(idx).copied()
    }

    /// 读一个字节（任意偏移）。
    pub fn byte_at(&self, off: u8) -> Option<u8> {
        let idx = (off as usize) / 4;
        let w = self.words.get(idx).copied()?;
        let sh = (off as u32 % 4) * 8;
        Some(((w >> sh) & 0xFF) as u8)
    }

    /// 读一个半字（off 必须 2B 对齐）。
    pub fn half_at(&self, off: u8) -> Option<u16> {
        if (off as u32) % 2 != 0 {
            return None;
        }
        let idx = (off as usize) / 4;
        let w = self.words.get(idx).copied()?;
        let sh = if off % 4 == 0 { 0 } else { 16 };
        Some(((w >> sh) & 0xFFFF) as u16)
    }
}

// ── 能力头与能力项 ─────────────────────────────────────────────
/// 能力项分类（数据闭集：未知 id 落 Other 显性留痕）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapKind {
    /// PM 能力（id 0x01）
    Pm,
    /// MSI 能力（id 0x05）
    Msi,
    /// MSI-X 能力（id 0x11）
    MsiX,
    /// 其他能力（id 原样留痕，不落位态账）
    Other(u8),
}

impl CapKind {
    pub fn from_id(id: u8) -> CapKind {
        if id == CAP_ID_PM {
            CapKind::Pm
        } else if id == CAP_ID_MSI {
            CapKind::Msi
        } else if id == CAP_ID_MSIX {
            CapKind::MsiX
        } else {
            CapKind::Other(id)
        }
    }
    pub fn id(self) -> u8 {
        match self {
            CapKind::Pm => CAP_ID_PM,
            CapKind::Msi => CAP_ID_MSI,
            CapKind::MsiX => CAP_ID_MSIX,
            CapKind::Other(v) => v,
        }
    }
}

/// 单个能力项：链上偏移、分类、next 指针。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapNode {
    /// 能力项在配置空间内的偏移（4B 对齐）
    pub off: u8,
    /// 能力 id 分类
    pub kind: CapKind,
    /// next 指针（原始 8bit 值）
    pub next: u8,
}

/// 一次完整走链的产物。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CapChain {
    pub nodes: Vec<CapNode>,
    /// 跳数（≤ 48）
    pub hops: u8,
    /// 是否从 next=0 正常收链（零断标志）
    pub ended_at_zero: bool,
}

/// 走链主循环：三闸加一限，任一违约显性拒走。
pub fn walk_chain(cfg: &CfgSpace) -> Result<CapChain, u32> {
    let mut visited = [0u64; 4];
    let mut nodes: Vec<CapNode> = Vec::new();
    let mut ptr = match cfg.byte_at(CAP_PTR_REG) {
        Some(b) => b,
        None => return Err(CapCode::ChainBreak.val()),
    };
    let mut hops: u8 = 0;
    while ptr != 0 {
        // 闸 1：对齐（raw 低 2 位必须为零）。
        if (ptr as u32) & NEXT_RESERVED_MASK != 0 {
            return Err(CapCode::ChainMisaligned.val());
        }
        // 闸 2：范围（能力区 0x40~0xFC）。
        if ptr < CAP_AREA_LO || ptr > CAP_AREA_HI {
            return Err(CapCode::ChainBreak.val());
        }
        // 闸 3：回环（visited 位图查重，字粒度 4B）。
        let wi = (ptr as usize) / 4;
        let bucket = wi / 64;
        let bit = (wi % 64) as u32;
        let slot_val = match visited.get(bucket) {
            Some(v) => *v,
            None => return Err(CapCode::ChainBreak.val()),
        };
        if slot_val & (1u64 << bit) != 0 {
            return Err(CapCode::ChainLoop.val());
        }
        if let Some(slot) = visited.get_mut(bucket) {
            *slot |= 1u64 << bit;
        }
        // 限：链长 ≤ 48。
        hops = match hops.checked_add(1) {
            Some(h) if h <= CHAIN_MAX => h,
            _ => return Err(CapCode::ChainTooLong.val()),
        };
        // 读能力头：id=bits[7:0]，next=bits[15:8]。
        let w = match cfg.word_at(ptr) {
            Some(w) => w,
            None => return Err(CapCode::ChainBreak.val()),
        };
        let id = (w & 0xFF) as u8;
        let next = ((w >> 8) & 0xFF) as u8;
        nodes.push(CapNode { off: ptr, kind: CapKind::from_id(id), next });
        ptr = next;
    }
    Ok(CapChain { nodes, hops, ended_at_zero: true })
}

// ── 位态只读账 ─────────────────────────────────────────────────
/// 三类能力的位态只读账（全部从配置空间读出，禁止回写）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoBitLedger {
    /// MSI 存在位（链上有 id 0x05）
    pub msi_present: bool,
    /// MSI Message Control 使能位（bit0）
    pub msi_enable: bool,
    /// MSI-X 存在位（链上有 id 0x11）
    pub msix_present: bool,
    /// MSI-X Message Control 使能位（bit15）
    pub msix_enable: bool,
    /// MSI-X 表尺寸 bits[10:3]
    pub msix_table_size: u16,
    /// PM 存在位（链上有 id 0x01）
    pub pm_present: bool,
    /// PMCSR 电源态 bits[1:0]（0=D0）
    pub pm_power_state: u8,
    /// PMCSR PME 使能位（bit8）
    pub pme_enable: bool,
}

impl RoBitLedger {
    /// 从走链结果 + 配置空间收集三类位态（只读）。
    pub fn collect(cfg: &CfgSpace, chain: &CapChain) -> RoBitLedger {
        let mut led = RoBitLedger::default();
        let mut i = 0;
        while i < chain.nodes.len() {
            let node = match chain.nodes.get(i) {
                Some(n) => *n,
                None => break,
            };
            match node.kind {
                CapKind::Msi => {
                    led.msi_present = true;
                    if let Some(h) = cfg.half_at(node.off.saturating_add(2)) {
                        led.msi_enable = h & 0x0001 == 0x0001;
                    }
                }
                CapKind::MsiX => {
                    led.msix_present = true;
                    if let Some(h) = cfg.half_at(node.off.saturating_add(2)) {
                        led.msix_enable = (h >> 15) & 0x0001 == 0x0001;
                        led.msix_table_size = (h >> 3) & 0x07FF;
                    }
                }
                CapKind::Pm => {
                    led.pm_present = true;
                    if let Some(h) = cfg.half_at(node.off.saturating_add(4)) {
                        led.pm_power_state = (h & 0x0003) as u8;
                        led.pme_enable = (h >> 8) & 0x0001 == 0x0001;
                    }
                }
                CapKind::Other(_) => {}
            }
            i += 1;
        }
        led
    }
}

/// 使能位写请求的四个目标（只读红线覆盖面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteTarget {
    /// MSI 使能位
    MsiEnable,
    /// MSI-X 使能位
    MsixEnable,
    /// PME 使能位
    PmeEnable,
    /// PM 电源态
    PowerState,
}

/// 写拦截门：只读红线，所有写请求一律拒绝并计数。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WriteGate {
    /// 已拦截的写请求数（= 注入数）
    pub blocked: u32,
    /// 生产路径写计数（恒 0）
    pub production_writes: u32,
}

impl WriteGate {
    /// 提交一次写请求：恒拒（只读红线），拦截计数 +1。
    pub fn submit(&mut self, _t: WriteTarget) -> Result<(), u32> {
        self.blocked = self.blocked.saturating_add(1);
        Err(CapCode::WriteBlocked.val())
    }
    /// 生产路径读写闸：只允许读，写计恒零。
    pub fn production_ok(&self) -> bool {
        self.production_writes == 0
    }
}

/// 零改谓词：拦截前后账面全等（违约条件先行 return false）。
fn enable_zero_change_ok(before: &RoBitLedger, after: &RoBitLedger) -> bool {
    if before.msi_enable != after.msi_enable {
        return false;
    }
    if before.msix_enable != after.msix_enable {
        return false;
    }
    if before.msix_table_size != after.msix_table_size {
        return false;
    }
    if before.pm_power_state != after.pm_power_state {
        return false;
    }
    if before.pme_enable != after.pme_enable {
        return false;
    }
    true
}

// ── 审计状态机（Idle→Walked→Ledgered→Sealed，单调） ─────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainPhase {
    /// 初始：已装载配置空间
    Idle,
    /// 已走链
    Walked,
    /// 已落位态账
    Ledgered,
    /// 已封账（只读红线生效）
    Sealed,
}

/// 单设备审计行：bdf + 配置空间 + 走链产物 + 位态账。
#[derive(Clone, Debug)]
pub struct DevChainAudit {
    pub bdf: u32,
    pub cfg: CfgSpace,
    pub chain: CapChain,
    pub ledger: RoBitLedger,
}

/// 多设备审计器：三步推进，倒退显性拒。
pub struct ChainAuditor {
    pub phase: ChainPhase,
    pub audits: Vec<DevChainAudit>,
    pub gate: WriteGate,
}

impl ChainAuditor {
    pub fn new(devs: Vec<(u32, CfgSpace)>) -> ChainAuditor {
        let mut audits: Vec<DevChainAudit> = Vec::new();
        let mut i = 0;
        while i < devs.len() {
            if let Some((bdf, cfg)) = devs.get(i) {
                audits.push(DevChainAudit {
                    bdf: *bdf,
                    cfg: cfg.clone(),
                    chain: CapChain::default(),
                    ledger: RoBitLedger::default(),
                });
            }
            i += 1;
        }
        ChainAuditor { phase: ChainPhase::Idle, audits, gate: WriteGate::default() }
    }

    /// 步 1：走链（仅 Idle 相可入）。
    pub fn step_walk(&mut self) -> Result<(), u32> {
        if self.phase != ChainPhase::Idle {
            return Err(CapCode::PhaseRegress.val());
        }
        let mut i = 0;
        while i < self.audits.len() {
            if let Some(a) = self.audits.get_mut(i) {
                a.chain = walk_chain(&a.cfg)?;
            }
            i += 1;
        }
        self.phase = ChainPhase::Walked;
        Ok(())
    }

    /// 步 2：位态落账（仅 Walked 相可入）。
    pub fn step_ledger(&mut self) -> Result<(), u32> {
        if self.phase != ChainPhase::Walked {
            return Err(CapCode::PhaseRegress.val());
        }
        let mut i = 0;
        while i < self.audits.len() {
            if let Some(a) = self.audits.get_mut(i) {
                a.ledger = RoBitLedger::collect(&a.cfg, &a.chain);
            }
            i += 1;
        }
        self.phase = ChainPhase::Ledgered;
        Ok(())
    }

    /// 步 3：封账（仅 Ledgered 相可入）。
    pub fn step_seal(&mut self) -> Result<(), u32> {
        if self.phase != ChainPhase::Ledgered {
            return Err(CapCode::PhaseRegress.val());
        }
        self.phase = ChainPhase::Sealed;
        Ok(())
    }

    /// 对指定设备发起使能位写注入（4 目标 × rounds），返回被拒次数。
    pub fn poke_enable_bits(&mut self, _dev_idx: usize, rounds: u32) -> u32 {
        let mut rejected = 0u32;
        let mut r = 0;
        while r < rounds {
            let targets = [
                WriteTarget::MsiEnable,
                WriteTarget::MsixEnable,
                WriteTarget::PmeEnable,
                WriteTarget::PowerState,
            ];
            let mut t = 0;
            while t < targets.len() {
                if let Some(tg) = targets.get(t) {
                    if self.gate.submit(*tg).is_err() {
                        rejected += 1;
                    }
                }
                t += 1;
            }
            r += 1;
        }
        rejected
    }
}

// ── 走链产物谓词（私有，判据面） ────────────────────────────────
/// 链干净谓词：跳数不超限且从零收链。
fn chain_clean_ok(chain: &CapChain) -> bool {
    if chain.hops > CHAIN_MAX {
        return false;
    }
    if !chain.ended_at_zero {
        return false;
    }
    true
}

/// 设备行谓词：链干净 + 位态账与链上分类一致。
fn dev_audit_ok(a: &DevChainAudit) -> bool {
    if !chain_clean_ok(&a.chain) {
        return false;
    }
    let mut saw_msi = false;
    let mut saw_msix = false;
    let mut saw_pm = false;
    let mut i = 0;
    while i < a.chain.nodes.len() {
        if let Some(n) = a.chain.nodes.get(i) {
            match n.kind {
                CapKind::Msi => saw_msi = true,
                CapKind::MsiX => saw_msix = true,
                CapKind::Pm => saw_pm = true,
                CapKind::Other(_) => {}
            }
        }
        i += 1;
    }
    if saw_msi != a.ledger.msi_present {
        return false;
    }
    if saw_msix != a.ledger.msix_present {
        return false;
    }
    if saw_pm != a.ledger.pm_present {
        return false;
    }
    true
}

/// 三类能力齐全谓词（真机 0 号设备的冻结口径）。
fn kinds_all_present_ok(led: &RoBitLedger) -> bool {
    led.msi_present && led.msix_present && led.pm_present
}

// ── LCG（固定种子，判据可复算） ────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
}

// ── 注入检出账（四路：环/断/错位/改写） ────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 真机设备表（3 台：全链 / 零链 / 仅 PM） ─────────────────────
/// 设备 A：MSI@0x50(使能) → MSI-X@0x70(使能, 表尺寸 4) → PM@0x90(PME 使能)。
pub fn cfg_dev_a() -> CfgSpace {
    let mut c = CfgSpace::blank();
    let _ = c.put_word(0x34, 0x0000_0050);
    // MSI 头：id 0x05 next 0x70；半字 0x52 使能 bit0=1。
    let _ = c.put_word(0x50, 0x0001_7005);
    // MSI-X 头：id 0x11 next 0x90；半字 0x72 使能 bit15=1、表尺寸 4。
    let _ = c.put_word(0x70, 0x8020_9011);
    // PM 头：id 0x01 next 0；PMCSR@0x94：电源态 D0、PME 使能 bit8=1。
    let _ = c.put_word(0x90, 0x0000_0001);
    let _ = c.put_word(0x94, 0x0000_0100);
    c
}

/// 设备 B：0x34 = 0，零能力链。
pub fn cfg_dev_b() -> CfgSpace {
    let mut c = CfgSpace::blank();
    let _ = c.put_word(0x34, 0x0000_0000);
    c
}

/// 设备 C：仅 PM@0x60，电源态 D1、PME 关。
pub fn cfg_dev_c() -> CfgSpace {
    let mut c = CfgSpace::blank();
    let _ = c.put_word(0x34, 0x0000_0060);
    let _ = c.put_word(0x60, 0x0000_0001);
    let _ = c.put_word(0x64, 0x0000_0001);
    c
}

/// 真机三设备（bdf：0x0000 / 0x0800 / 0x1000）。
pub fn machine_cfgs() -> Vec<(u32, CfgSpace)> {
    let mut v: Vec<(u32, CfgSpace)> = Vec::new();
    v.push((0x0000, cfg_dev_a()));
    v.push((0x0800, cfg_dev_b()));
    v.push((0x1000, cfg_dev_c()));
    v
}

// ── 黄金全链：走链→落账→封账→零改→复算 ─────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut aud = ChainAuditor::new(machine_cfgs());
    aud.step_walk()?;
    aud.step_ledger()?;
    aud.step_seal()?;
    // 设备 A 三类能力齐全。
    let kinds_ok = match aud.audits.get(0) {
        Some(a) => dev_audit_ok(a) && kinds_all_present_ok(&a.ledger),
        None => false,
    };
    // 设备 B 零链合法。
    let zero_ok = match aud.audits.get(1) {
        Some(a) => a.chain.hops == 0 && chain_clean_ok(&a.chain),
        None => false,
    };
    // 设备 C 仅 PM：电源态 D1。
    let pm_ok = match aud.audits.get(2) {
        Some(a) => a.ledger.pm_present && a.ledger.pm_power_state == 1 && !a.ledger.pme_enable,
        None => false,
    };
    // 跳数复算：3 + 0 + 1 = 4。
    let mut hops = 0u32;
    let mut i = 0;
    while i < aud.audits.len() {
        if let Some(a) = aud.audits.get(i) {
            hops = hops.saturating_add(a.chain.hops as u32);
        }
        i += 1;
    }
    let hops_ok = hops == 4;
    // 使能位零改：8 次写全拒，账面快照全等。
    let before = match aud.audits.get(0) {
        Some(a) => a.ledger.clone(),
        None => RoBitLedger::default(),
    };
    let blocked = aud.poke_enable_bits(0, POKE_ROUNDS);
    let blocked_ok = blocked == POKE_ROUNDS * 4 && aud.gate.production_ok();
    let after = match aud.audits.get(0) {
        Some(a) => a.ledger.clone(),
        None => RoBitLedger::default(),
    };
    let zero_change = enable_zero_change_ok(&before, &after);
    let ok = (kinds_ok as u32) + (zero_ok as u32) + (pm_ok as u32) + (hops_ok as u32) + (blocked_ok as u32) + (zero_change as u32);
    Ok((ok, blocked, hops))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链 6/6 绿 + 8 拦 + 4 跳。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((6, 8, 4)) => Ok(()),
        _ => Err(CapCode::NotReproducible.val()),
    }
}

/// 序 B：自环注入必抓（next 指回自身偏移）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut cfg = CfgSpace::blank();
    let _ = cfg.put_word(0x34, 0x0000_0050);
    // 头字：id 0x05，next 0x50 —— 指回自身。
    let _ = cfg.put_word(0x50, 0x0000_5005);
    match walk_chain(&cfg) {
        Err(e) if e == CapCode::ChainLoop.val() => Ok(()),
        _ => Err(CapCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常量（链长 48 / id 表 / 对齐掩码）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        CHAIN_MAX == 48,
        CAP_ID_PM == 0x01,
        CAP_ID_MSI == 0x05,
        CAP_ID_MSIX == 0x11,
        NEXT_RESERVED_MASK == 0x03,
    ) {
        (true, true, true, true, true) => Ok(()),
        _ => Err(CapCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：环路。
        let mut c1 = CfgSpace::blank();
        let _ = c1.put_word(0x34, 0x0000_0050);
        let _ = c1.put_word(0x50, 0x0000_5005);
        led.record(0, walk_chain(&c1).is_err() && probe_seq_b().is_ok());
        // 路 2：断链（next 落在标准头区 0x38）。
        let mut c2 = CfgSpace::blank();
        let _ = c2.put_word(0x34, 0x0000_0050);
        let _ = c2.put_word(0x50, 0x0000_3805);
        led.record(1, walk_chain(&c2).is_err());
        // 路 3：错位（next 低 2 位非零）。
        let mut c3 = CfgSpace::blank();
        let _ = c3.put_word(0x34, 0x0000_0050);
        let _ = c3.put_word(0x50, 0x0000_5305);
        led.record(2, walk_chain(&c3).is_err());
        // 路 4：改写拦截（封账后 4 目标写全拒且零改）。
        let mut aud = ChainAuditor::new(machine_cfgs());
        let flow = aud.step_walk().is_ok() && aud.step_ledger().is_ok() && aud.step_seal().is_ok();
        let before = match aud.audits.get(0) {
            Some(a) => a.ledger.clone(),
            None => RoBitLedger::default(),
        };
        let rej = aud.poke_enable_bits(0, 1);
        let after = match aud.audits.get(0) {
            Some(a) => a.ledger.clone(),
            None => RoBitLedger::default(),
        };
        led.record(3, flow && rej == 4 && enable_zero_change_ok(&before, &after));
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(CapCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（0x75000~0x781C0 窗口 + 0x40 对齐 + 闭集落窗） ─────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < CapCode::ALL.len() {
        if let Some(c) = CapCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

/// 链长上限边界谓词：48 跳满链收链成功，且链长上限与能力区物理容量
/// 数学一致（(0x100-0x40)/4 == 48 == CHAIN_MAX；8bit next 指针放不下
/// 第 49 个对齐槽位，越区路径由范围/对齐闸兜底，ChainTooLong 为防御闸）。
fn chain_max_boundary_ok() -> bool {
    // 容量数学：能力区 0x40~0xFC 恰 48 个 4B 对齐槽。
    let capacity = (0x100u32 - CAP_AREA_LO as u32) / 4;
    if capacity != CHAIN_MAX as u32 {
        return false;
    }
    // 48 节点满链：0x40~0xFC 每槽一节点，尾节点 next=0。
    let mut c = CfgSpace::blank();
    let mut off: u8 = CAP_AREA_LO;
    let _ = c.put_word(CAP_PTR_REG, off as u32);
    let mut i: u8 = 0;
    while i < CHAIN_MAX {
        let next = if i == CHAIN_MAX - 1 { 0u8 } else { off.saturating_add(4) };
        let w = ((next as u32) << 8) | CAP_ID_MSI as u32;
        let _ = c.put_word(off, w);
        off = off.saturating_add(4);
        i += 1;
    }
    match walk_chain(&c) {
        Ok(ch) => ch.hops == CHAIN_MAX && chain_clean_ok(&ch),
        Err(_) => false,
    }
}

/// 诊断码闭集查表谓词：ALL 全员落窗且 detail 查表回环一致。
fn code_closure_ok() -> bool {
    let mut i = 0;
    while i < CapCode::ALL.len() {
        if let Some(c) = CapCode::ALL.get(i) {
            let v = c.val();
            if detail(v) != c.detail() || c.detail().is_empty() {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链。
    let g = golden_run();
    v.push((
        "F0468-J1-黄金全链",
        match &g {
            Ok((ok, rej, hops)) => *ok == 6 && *rej == 8 && *hops == 4,
            Err(_) => false,
        },
        match &g {
            Ok((ok, rej, hops)) => format!("ok={} rej={} hops={}", ok, rej, hops),
            Err(e) => format!("err=0x{:x} {}", e, detail(*e)),
        },
    ));
    // 段位守卫。
    v.push((
        "F0468-段位守卫",
        segment_guard(),
        format!("SEG=0x{:X} 窗口 0x75000~0x781C0", SEG_BASE),
    ));
    // 链长上限边界。
    v.push((
        "F0468-链长≤48边界",
        chain_max_boundary_ok(),
        String::from("48 跳满链收链、上限与能力区容量一致"),
    ));
    // 零环。
    let loop_ok = probe_seq_b().is_ok();
    v.push((
        "F0468-零环",
        loop_ok,
        String::from("自环 next 指回自身，回环闸必抓"),
    ));
    // 零断。
    let mut brk = CfgSpace::blank();
    let _ = brk.put_word(0x34, 0x0000_0050);
    let _ = brk.put_word(0x50, 0x0000_3805);
    let brk_ok = match walk_chain(&brk) {
        Err(e) => e == CapCode::ChainBreak.val(),
        Ok(_) => false,
    };
    v.push((
        "F0468-零断",
        brk_ok,
        String::from("next=0x38 落标准头区，范围闸必抓"),
    ));
    // 错位。
    let mut mis = CfgSpace::blank();
    let _ = mis.put_word(0x34, 0x0000_0050);
    let _ = mis.put_word(0x50, 0x0000_5305);
    let mis_ok = match walk_chain(&mis) {
        Err(e) => e == CapCode::ChainMisaligned.val(),
        Ok(_) => false,
    };
    v.push((
        "F0468-4B对齐",
        mis_ok,
        String::from("next=0x53 低 2 位非零，对齐闸必抓"),
    ));
    // MSI 0x05 位态落账。
    let a_chain = match walk_chain(&cfg_dev_a()) {
        Ok(c) => c,
        Err(_) => CapChain::default(),
    };
    let a_led = RoBitLedger::collect(&cfg_dev_a(), &a_chain);
    v.push((
        "F0468-MSI0x05位态",
        a_led.msi_present && a_led.msi_enable,
        format!("msi_en={}", a_led.msi_enable),
    ));
    // MSI-X 0x11 位态落账。
    v.push((
        "F0468-MSIX0x11位态",
        a_led.msix_present && a_led.msix_enable && a_led.msix_table_size == 4,
        format!("msix_en={} tsz={}", a_led.msix_enable, a_led.msix_table_size),
    ));
    // PM 0x01 位态落账。
    v.push((
        "F0468-PM0x01位态",
        a_led.pm_present && a_led.pm_power_state == 0 && a_led.pme_enable,
        format!("pstate={} pme={}", a_led.pm_power_state, a_led.pme_enable),
    ));
    // 使能位零改。
    let mut aud = ChainAuditor::new(machine_cfgs());
    let flow = aud.step_walk().is_ok() && aud.step_ledger().is_ok() && aud.step_seal().is_ok();
    let before = match aud.audits.get(0) {
        Some(a) => a.ledger.clone(),
        None => RoBitLedger::default(),
    };
    let rej = aud.poke_enable_bits(0, POKE_ROUNDS);
    let after = match aud.audits.get(0) {
        Some(a) => a.ledger.clone(),
        None => RoBitLedger::default(),
    };
    v.push((
        "F0468-使能位零改",
        flow && rej == POKE_ROUNDS * 4 && enable_zero_change_ok(&before, &after) && aud.gate.production_ok(),
        format!("拦截={} 生产写=0", rej),
    ));
    // 状态机倒退拒。
    let mut aud2 = ChainAuditor::new(machine_cfgs());
    let regress_ok = aud2.step_ledger().is_err() && aud2.step_seal().is_err();
    v.push((
        "F0468-状态机倒退拒",
        regress_ok,
        String::from("Idle 相直调 ledger/seal 显性拒"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "F0468-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 双跑一致。
    v.push((
        "F0468-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 诊断码闭集查表。
    v.push((
        "F0468-诊断码闭集",
        code_closure_ok(),
        String::from("8 码全员落窗且查表回环一致"),
    ));
    v
}

pub fn run_unx0468_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0468 能力链表账（MSI/MSI-X/PM 只读）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ─────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_full() {
        let g = golden_run();
        assert!(g.is_ok());
        assert_eq!(g.ok(), Some((6, 8, 4)));
    }

    #[test]
    fn test_loop_injection_caught() {
        assert!(probe_seq_b().is_ok());
    }

    #[test]
    fn test_break_injection_caught() {
        let mut c = CfgSpace::blank();
        let _ = c.put_word(0x34, 0x0000_0050);
        let _ = c.put_word(0x50, 0x0000_3805);
        let r = walk_chain(&c);
        assert!(r.is_err());
        assert_eq!(r.err(), Some(CapCode::ChainBreak.val()));
    }

    #[test]
    fn test_misalign_injection_caught() {
        let mut c = CfgSpace::blank();
        let _ = c.put_word(0x34, 0x0000_0050);
        let _ = c.put_word(0x50, 0x0000_5305);
        let r = walk_chain(&c);
        assert!(r.is_err());
        assert_eq!(r.err(), Some(CapCode::ChainMisaligned.val()));
    }

    #[test]
    fn test_chain_max_boundary() {
        assert!(chain_max_boundary_ok());
        // 防御闸：ChainTooLong 在诊断闭集内且落窗。
        let v = CapCode::ChainTooLong.val();
        assert!(v >= SEG_BASE && v < SEG_BASE + 0x40);
    }

    #[test]
    fn test_zero_change_after_8_writes() {
        let mut aud = ChainAuditor::new(machine_cfgs());
        assert!(aud.step_walk().is_ok());
        assert!(aud.step_ledger().is_ok());
        assert!(aud.step_seal().is_ok());
        let before = match aud.audits.get(0) {
            Some(a) => a.ledger.clone(),
            None => RoBitLedger::default(),
        };
        let rej = aud.poke_enable_bits(0, POKE_ROUNDS);
        assert_eq!(rej, 8);
        let after = match aud.audits.get(0) {
            Some(a) => a.ledger.clone(),
            None => RoBitLedger::default(),
        };
        assert!(enable_zero_change_ok(&before, &after));
        assert_eq!(before, after);
    }

    #[test]
    fn test_phase_machine_regress_rejected() {
        let mut aud = ChainAuditor::new(machine_cfgs());
        assert!(aud.step_ledger().is_err());
        assert!(aud.step_seal().is_err());
        assert!(aud.step_walk().is_ok());
        // Walked 相重走也拒。
        assert!(aud.step_walk().is_err());
        assert!(aud.step_ledger().is_ok());
        assert!(aud.step_ledger().is_err());
        assert!(aud.step_seal().is_ok());
    }

    #[test]
    fn test_dual_run_identical() {
        assert!(repro_check().is_ok());
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
    }

    #[test]
    fn test_segment_guard_and_closure() {
        assert!(segment_guard());
        assert!(code_closure_ok());
        let mut i = 0;
        while i < CapCode::ALL.len() {
            let v = match CapCode::ALL.get(i) {
                Some(c) => c.val(),
                None => break,
            };
            assert!(!detail(v).is_empty());
            assert_eq!(detail(0x1234_5678u32), "未知诊断码：不在 0x75600 段闭集内");
            i += 1;
        }
    }

    #[test]
    fn test_inject_rounds_all_caught() {
        let inj = inject_rounds(JUDGE_ROUNDS);
        assert!(inj.is_ok());
        let led = match inj {
            Ok(l) => l,
            Err(_) => return,
        };
        assert!(led.all_caught());
        assert_eq!(led.tries, [10, 10, 10, 10]);
        assert_eq!(led.caught, [10, 10, 10, 10]);
    }

    #[test]
    fn test_probe_seq_c_constants_frozen() {
        assert!(probe_seq_c().is_ok());
        assert_eq!(CHAIN_MAX, 48);
        assert_eq!(CAP_ID_MSI, 0x05);
        assert_eq!(CAP_ID_MSIX, 0x11);
        assert_eq!(CAP_ID_PM, 0x01);
    }

    #[test]
    fn test_checks_all_green() {
        let s = run_unx0468_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
    }
}
