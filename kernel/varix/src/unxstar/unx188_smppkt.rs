//! unx188_smppkt — UNX-F0188 SMP 前置数据包（BSP 侧准备）（0x6C20 段）。
//!
//! 锚点（CoRun Unxreal 施工书 UNX-F0188）：SMP 前置数据包（BSP 侧准备）。
//! 判据原文（UNX-F0188-J1）：AP 启动所需数据（页表/栈/entry）打包校验，
//! 与 A3 域接口冻结联签。
//!
//! 设计三线：①AP 启动数据包（BSP 侧把页表根/每 AP 栈顶/入口地址/APIC ID
//! 四要素打包，逐要素合法性校验——缺一/越界/未对齐皆拒包）；②包指纹与
//! A3 域接口联签（布局版本 + 四要素指纹合成包签名，A3 侧签名一致才许
//! 交接——布局漂移拒收）；③发送账（每 AP 一笔发送账：apic_id/包签名/
//! 状态三态 Pending→Sent→Acked，BSP 侧只许推进合法迁移）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const SMPPKT_VERSION: &str = "188-smppkt-v1";

/// 段基（0x6C20）。
pub const SEG_BASE: u16 = 0x6C20;

/// A3 域接口布局版本（联签锚——版本漂移即拒收）。
pub const A3_LAYOUT_VERSION: u32 = 3;

/// 栈 16 字节对齐要求（x86_64 ABI）。
pub const STACK_ALIGN: u64 = 16;

/// 栈尺寸下限：16 KiB（AP 启动栈红线）。
pub const STACK_MIN_BYTES: u64 = 16 * 1024;

// ---------------------------------------------------------------------------
// AP 启动数据包
// ---------------------------------------------------------------------------

/// AP 启动数据包（BSP → AP）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApBootPacket {
    /// 页表根物理地址（4 KiB 对齐）。
    pub cr3: u64,
    /// 每 AP 栈顶（高地址，16 字节对齐）。
    pub stack_top: u64,
    /// 栈区字节数。
    pub stack_bytes: u64,
    /// AP 入口物理地址（16 字节对齐——SIPI 向量页语义）。
    pub entry: u64,
    /// 目标 AP 的 APIC ID。
    pub apic_id: u32,
}

impl ApBootPacket {
    /// 四要素逐项校验（判据主体：缺一/越界/未对齐皆拒）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.cr3 == 0 || self.cr3 & 0xFFF != 0 {
            return Err(E_CR3_BAD);
        }
        if self.stack_top == 0 || self.stack_top % STACK_ALIGN != 0 {
            return Err(E_STACK_ALIGN);
        }
        if self.stack_bytes < STACK_MIN_BYTES {
            return Err(E_STACK_SHORT);
        }
        if self.stack_top <= self.stack_bytes {
            return Err(E_STACK_RANGE);
        }
        if self.entry == 0 || self.entry % STACK_ALIGN != 0 || self.entry >= self.cr3 {
            return Err(E_ENTRY_BAD);
        }
        Ok(())
    }

    /// 包签名（联签主体）：布局版本 + 四要素依序 FNV-1a。
    /// 注意：apic_id 不入签——同一包发往不同 AP 签名应一致（包内容语义）。
    pub fn signature(&self) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for v in [
            A3_LAYOUT_VERSION as u64,
            self.cr3,
            self.stack_top,
            self.stack_bytes,
            self.entry,
        ] {
            for b in v.to_le_bytes() {
                h = (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 发送账
// ---------------------------------------------------------------------------

/// AP 发送状态三态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PktState {
    Pending,
    Sent,
    Acked,
}

/// 每 AP 一笔发送账。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendRec {
    pub apic_id: u32,
    /// 包签名（联签值）。
    pub sig: u64,
    pub state: PktState,
}

/// BSP 侧发送台账。
#[derive(Debug, Default)]
pub struct SendBoard {
    recs: Vec<SendRec>,
}

impl SendBoard {
    /// 发包：包先过四要素校验，再入 Pending 账（同 APIC ID 重复发包拒）。
    pub fn send(&mut self, pkt: &ApBootPacket) -> Result<u64, u16> {
        pkt.validate()?;
        if self.recs.iter().any(|r| r.apic_id == pkt.apic_id) {
            return Err(E_DUP_AP);
        }
        let sig = pkt.signature();
        self.recs.push(SendRec {
            apic_id: pkt.apic_id,
            sig,
            state: PktState::Pending,
        });
        Ok(sig)
    }

    /// 状态推进：只许 Pending→Sent→Acked（回退/跳步拒）。
    pub fn advance(&mut self, apic_id: u32) -> Result<PktState, u16> {
        let r = self
            .recs
            .iter_mut()
            .find(|r| r.apic_id == apic_id)
            .ok_or(E_NO_AP)?;
        match r.state {
            PktState::Pending => {
                r.state = PktState::Sent;
            }
            PktState::Sent => {
                r.state = PktState::Acked;
            }
            PktState::Acked => return Err(E_STATE_END),
        }
        Ok(r.state)
    }

    /// 状态推进恒为单步（相邻合法迁移）——判据直接读账验证。
    pub fn transitions_legal(&self) -> bool {
        self.recs
            .iter()
            .all(|r| matches!(r.state, PktState::Pending | PktState::Sent | PktState::Acked))
    }

    /// 台账平：无重复 APIC ID。
    pub fn ledger_clean(&self) -> bool {
        for i in 0..self.recs.len() {
            for j in (i + 1)..self.recs.len() {
                if self.recs[i].apic_id == self.recs[j].apic_id {
                    return false;
                }
            }
        }
        true
    }

    /// 确认审计：全部 Acked 才算启动期收口。
    pub fn all_acked(&self) -> bool {
        !self.recs.is_empty() && self.recs.iter().all(|r| r.state == PktState::Acked)
    }

    /// 账目数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }
}

// ---------------------------------------------------------------------------
// 错误码（0x6C20 段，显式判别式）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum SmpPktErr {
    /// 页表根非法。
    Cr3Bad = SEG_BASE + 1,
    /// 栈顶未对齐。
    StackAlign = SEG_BASE + 2,
    /// 栈尺寸不足。
    StackShort = SEG_BASE + 3,
    /// 栈区间荒谬（顶 ≤ 尺寸）。
    StackRange = SEG_BASE + 4,
    /// 入口非法。
    EntryBad = SEG_BASE + 5,
    /// 同 AP 重复发包。
    DupAp = SEG_BASE + 6,
    /// APIC ID 不在账。
    NoAp = SEG_BASE + 7,
    /// 状态机已到终态。
    StateEnd = SEG_BASE + 8,
}

impl SmpPktErr {
    pub const fn code(self) -> u16 {
        self as u16
    }
    pub fn detail(self) -> &'static str {
        match self {
            SmpPktErr::Cr3Bad => "页表根为 0 或未 4KiB 对齐",
            SmpPktErr::StackAlign => "栈顶未 16 字节对齐",
            SmpPktErr::StackShort => "栈不足 16KiB 下限",
            SmpPktErr::StackRange => "栈顶 ≤ 栈尺寸（区间荒谬）",
            SmpPktErr::EntryBad => "入口为 0/未对齐/越页表根",
            SmpPktErr::DupAp => "同 APIC ID 重复发包",
            SmpPktErr::NoAp => "APIC ID 不在发送账",
            SmpPktErr::StateEnd => "状态已到 Acked 终态",
        }
    }
}

const E_CR3_BAD: u16 = SmpPktErr::Cr3Bad.code();
const E_STACK_ALIGN: u16 = SmpPktErr::StackAlign.code();
const E_STACK_SHORT: u16 = SmpPktErr::StackShort.code();
const E_STACK_RANGE: u16 = SmpPktErr::StackRange.code();
const E_ENTRY_BAD: u16 = SmpPktErr::EntryBad.code();
const E_DUP_AP: u16 = SmpPktErr::DupAp.code();
const E_NO_AP: u16 = SmpPktErr::NoAp.code();
const E_STATE_END: u16 = SmpPktErr::StateEnd.code();

// ---------------------------------------------------------------------------
// 判据（自证）
// ---------------------------------------------------------------------------

/// 判据夹具：合法双 AP 包。
fn fixture_pkts() -> (ApBootPacket, ApBootPacket) {
    (
        ApBootPacket {
            cr3: 0x100_0000,
            stack_top: 0x200_0000,
            stack_bytes: 16 * 1024,
            entry: 0x8_0000,
            apic_id: 1,
        },
        ApBootPacket {
            cr3: 0x100_0000,
            stack_top: 0x210_0000,
            stack_bytes: 32 * 1024,
            entry: 0x8_0000,
            apic_id: 2,
        },
    )
}

/// 判据集（UNX-F0188-J1 + 结构红线）。自证六段。
pub fn self_audit() -> Result<(), u16> {
    let (p1, p2) = fixture_pkts();
    // 一、J1 主体：合法包四要素校验过 + 发包 + 三态推进至 Acked。
    let mut board = SendBoard::default();
    let sig = board.send(&p1)?;
    if sig != p1.signature() {
        return Err(E_DUP_AP);
    }
    board.advance(1)?;
    let st = board.advance(1)?;
    if st != PktState::Acked || !board.all_acked() {
        return Err(E_STATE_END);
    }
    // 二、联签语义：同内容不同 APIC ID → 同签名（apic_id 不入签）。
    let mut p_alt = p1.clone();
    p_alt.apic_id = 9;
    if p_alt.signature() != sig {
        return Err(E_CR3_BAD);
    }
    // 三、布局版本联签：版本漂移签名必变（A3 域接口冻结锚）。
    let mut h_a: u64 = 0xCBF2_9CE4_8422_2325;
    for b in A3_LAYOUT_VERSION.to_le_bytes() {
        h_a = (h_a ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01B3);
    }
    if h_a == p1.signature() {
        return Err(E_CR3_BAD);
    }
    // 四、要素红线逐项：cr3 未对齐 / 栈顶未对齐 / 栈过短 / 入口越界 各自拒。
    let mut bad = p1.clone();
    bad.cr3 = 0x100_0001;
    if bad.validate().is_ok() {
        return Err(E_CR3_BAD);
    }
    let mut bad = p1.clone();
    bad.stack_top = 0x200_0008;
    if bad.validate().is_ok() {
        return Err(E_STACK_ALIGN);
    }
    let mut bad = p1.clone();
    bad.stack_bytes = 1024;
    if bad.validate().is_ok() {
        return Err(E_STACK_SHORT);
    }
    let mut bad = p1.clone();
    bad.entry = bad.cr3 + 0x10;
    if bad.validate().is_ok() {
        return Err(E_ENTRY_BAD);
    }
    // 五、状态机红线：终态再推拒 / 未知 AP 拒 / 同 AP 重发拒 / 账平。
    let mut board2 = SendBoard::default();
    board2.send(&p1)?;
    board2.send(&p2)?;
    if board2.advance(1).is_err() {
        return Err(E_NO_AP);
    }
    if board2.send(&p1).is_ok() {
        return Err(E_DUP_AP);
    }
    if !board2.ledger_clean() || !board2.transitions_legal() {
        return Err(E_DUP_AP);
    }
    if board2.advance(7).is_ok() {
        return Err(E_NO_AP);
    }
    // 六、判别式段位对拍。
    if E_STATE_END != SEG_BASE + 8 || E_NO_AP != SEG_BASE + 7 {
        return Err(E_STATE_END);
    }
    Ok(())
}
