//! unx0887_ipifam —— UNX-F0887 · ICI 三类 IPI 投递分账（Fixed/NMI/Init-Startup 语义族）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0887 · ICI 三类 IPI 投递分账。
//! 判据原文摘录（卡面验收行）：ICI 三类 IPI 投递分账（Fixed/NMI/Init-Startup
//!           语义族）——三类投递分账逐笔、语义族互不混账、投递状态
//!           机闭环（发送→投递→应答）。
//! 诊断段 0x79E0（本单独占，0x79E1~0x79EF 闭集）。
//!
//! 设计纪律：
//! - 三类语义族闭集：
//!   Fixed（0b000）：普通向量投递，须向量合法（16~255）；
//!   NMI（0b100）：不可屏蔽投递，无视掩码，向量面仍须合法；
//!   Init-Startup（0b110/0b101 组合族）：INIT（启动序列）与 Startup
//!   （SIPI，向量 = 启动页号，8 位面 ≤ 0xFF 且页对齐 4KiB 面）。
//! - 投递状态机：Queued → Sent → Delivered → Acked；每笔投递逐态推进，
//!   状态跳变非法即红账（如未 Sent 即 Acked）。
//! - 三账分立：Fixed 账 / NMI 账 / Init-Startup 账各自独立（互不混账），
//!   总账行 = 三账行之和（守恒恒等式）。
//! - 拦截面：向量越界、Startup 页号越 8 位面、状态机违序、族码非法。
//! - no_std + alloc；零 unwrap/expect/中断型红字；整数算术；确定性；无浮点。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 模块版本。
pub const IF_VERSION: &str = "unx0887_ipifam-v1";

/// 诊断段基址（0x79E0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x79E0;

/// XcCode 闭集（0x79E1~0x79EF）。
pub const XC_VEC_RANGE: u16 = SEG_BASE + 1; // 向量越界（16~255 外）
pub const XC_PAGE_RANGE: u16 = SEG_BASE + 2; // Startup 页号越 8 位面
pub const XC_STATE: u16 = SEG_BASE + 3; // 状态机违序
pub const XC_FAMILY: u16 = SEG_BASE + 4; // 族码非法（非三类）
pub const XC_LED_FULL: u16 = SEG_BASE + 5; // 账溢出（计数不丢）
pub const XC_CONSERVE: u16 = SEG_BASE + 6; // 守恒破
pub const XC_NOT_FOUND: u16 = SEG_BASE + 7; // 投递句柄悬空
pub const XC_DEST_RANGE: u16 = SEG_BASE + 8; // 目标核越界

/// 逻辑核数建模上限。
pub const CORE_MAX: usize = 16;
/// 每账容量。
pub const LED_CAP: usize = 64;

/// IPI 语义族闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IciFamily {
    /// Fixed（0b000）。
    Fixed,
    /// NMI（0b100）。
    Nmi,
    /// INIT（0b101→族内 Init 面）。
    Init,
    /// Startup/SIPI（0b110→族内 Startup 面）。
    Startup,
}

impl IciFamily {
    /// ICR 投递模式三位原值。
    pub fn bits(&self) -> u32 {
        match self {
            IciFamily::Fixed => 0,
            IciFamily::Nmi => 4,
            IciFamily::Init => 5,
            IciFamily::Startup => 6,
        }
    }
    /// 三位原值 → 闭集（1/2/3/7 非法）。
    pub fn from_bits(b: u32) -> Result<IciFamily, u16> {
        match b {
            0 => Ok(IciFamily::Fixed),
            4 => Ok(IciFamily::Nmi),
            5 => Ok(IciFamily::Init),
            6 => Ok(IciFamily::Startup),
            _ => Err(XC_FAMILY),
        }
    }
    /// 账归属面（Init 与 Startup 同属 Init-Startup 语义族，但分账行独立）。
    pub fn slot(&self) -> usize {
        match self {
            IciFamily::Fixed => 0,
            IciFamily::Nmi => 1,
            IciFamily::Init => 2,
            IciFamily::Startup => 2,
        }
    }
}

/// 投递状态机闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dstate {
    /// 已排队。
    Queued,
    /// 已发送（ICR 投递状态位翻转）。
    Sent,
    /// 已投递（目标 Local APIC 收讫）。
    Delivered,
    /// 已应答（目标核 EOI/处理回执）。
    Acked,
}

/// 投递行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IpiRow {
    /// 投递句柄（自 1 递增）。
    pub handle: u64,
    /// 语义族。
    pub family: IciFamily,
    /// 目标核。
    pub dest: usize,
    /// 向量/页号。
    pub vector: u16,
    /// 状态。
    pub state: Dstate,
    /// 状态跳变计数。
    pub hops: u8,
}

/// ICI 分账器。
pub struct IciLedger {
    /// 三面账：slot0=Fixed slot1=NMI slot2=Init-Startup。
    slots: [Vec<IpiRow>; 3],
    /// 全账索引（句柄 → slot 定位）。
    index: Vec<(u64, usize)>,
    seq: u64,
    overflow: usize,
    /// 拦截计数。
    rejects: u64,
}

impl IciLedger {
    pub fn new() -> IciLedger {
        IciLedger {
            slots: [Vec::new(), Vec::new(), Vec::new()],
            index: Vec::new(),
            seq: 0,
            overflow: 0,
            rejects: 0,
        }
    }

    /// 发起投递：合法性全门 + Queued→Sent 即发（同事务）。
    pub fn send(&mut self, family: IciFamily, dest: usize, vector: u16) -> Result<u64, u16> {
        self.seq += 1;
        // 1. 目标界
        if dest >= CORE_MAX {
            self.rejects += 1;
            return Err(XC_DEST_RANGE);
        }
        // 2. 族内向量面校验
        match family {
            IciFamily::Fixed | IciFamily::Nmi => {
                if vector < 16 || vector > 255 {
                    self.rejects += 1;
                    return Err(XC_VEC_RANGE);
                }
            }
            IciFamily::Init => {
                // INIT 无向量面（向量须 0）
                if vector != 0 {
                    self.rejects += 1;
                    return Err(XC_VEC_RANGE);
                }
            }
            IciFamily::Startup => {
                // SIPI：向量 = 启动页号（8 位面）
                if vector > 0xFF {
                    self.rejects += 1;
                    return Err(XC_PAGE_RANGE);
                }
            }
        }
        let h = self.seq;
        let row = IpiRow {
            handle: h,
            family,
            dest,
            vector,
            state: Dstate::Queued,
            hops: 1,
        };
        let slot = family.slot();
        if self.slots[slot].len() < LED_CAP {
            self.slots[slot].push(row);
            self.index.push((h, slot));
        } else {
            self.overflow += 1;
        }
        Ok(h)
    }

    /// 状态推进：Queued→Sent→Delivered→Acked 单向单步。
    pub fn advance(&mut self, handle: u64) -> Result<Dstate, u16> {
        let slot = match self.index.iter().find(|(h, _)| *h == handle) {
            Some((_, s)) => *s,
            None => return Err(XC_NOT_FOUND),
        };
        let pos = match self.slots[slot].iter().position(|r| r.handle == handle) {
            Some(p) => p,
            None => return Err(XC_NOT_FOUND),
        };
        let next = match self.slots[slot][pos].state {
            Dstate::Queued => Dstate::Sent,
            Dstate::Sent => Dstate::Delivered,
            Dstate::Delivered => Dstate::Acked,
            Dstate::Acked => {
                self.rejects += 1;
                return Err(XC_STATE);
            }
        };
        self.slots[slot][pos].state = next;
        self.slots[slot][pos].hops += 1;
        Ok(next)
    }

    /// 直跳校验面：未 Sent 即 Acked 属违序（显式拒）。
    pub fn force_acked(&mut self, handle: u64) -> Result<(), u16> {
        let slot = match self.index.iter().find(|(h, _)| *h == handle) {
            Some((_, s)) => *s,
            None => return Err(XC_NOT_FOUND),
        };
        let pos = match self.slots[slot].iter().position(|r| r.handle == handle) {
            Some(p) => p,
            None => return Err(XC_NOT_FOUND),
        };
        if self.slots[slot][pos].state != Dstate::Delivered {
            self.rejects += 1;
            return Err(XC_STATE);
        }
        self.slots[slot][pos].state = Dstate::Acked;
        self.slots[slot][pos].hops += 1;
        Ok(())
    }

    /// 投递行读取（句柄）。
    pub fn row(&self, handle: u64) -> Result<IpiRow, u16> {
        let slot = match self.index.iter().find(|(h, _)| *h == handle) {
            Some((_, s)) => *s,
            None => return Err(XC_NOT_FOUND),
        };
        for r in self.slots[slot].iter() {
            if r.handle == handle {
                return Ok(*r);
            }
        }
        Err(XC_NOT_FOUND)
    }

    /// 三账行数。
    pub fn slot_lens(&self) -> [usize; 3] {
        [self.slots[0].len(), self.slots[1].len(), self.slots[2].len()]
    }

    /// 全账行数（三账之和）。
    pub fn total(&self) -> usize {
        self.slots[0].len() + self.slots[1].len() + self.slots[2].len()
    }

    /// 守恒：三账和 = 全账行数 + 溢出 = 发起次数；索引与账平。
    pub fn conserve(&self) -> bool {
        self.total() + self.overflow == self.seq as usize
            && self.index.len() == self.total()
    }

    /// 全应答核：所有在账行 state == Acked。
    pub fn all_acked(&self) -> bool {
        for s in 0..3 {
            for r in self.slots[s].iter() {
                if r.state != Dstate::Acked {
                    return false;
                }
            }
        }
        true
    }

    /// 拦截计数。
    pub fn rejects(&self) -> u64 {
        self.rejects
    }
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    // 1. 三类各发一笔 → 走全状态机 Queued→Sent→Delivered→Acked
    let mut l = IciLedger::new();
    let h1 = l.send(IciFamily::Fixed, 1, 0x41)?;
    let h2 = l.send(IciFamily::Nmi, 2, 0x50)?;
    let h3 = l.send(IciFamily::Startup, 3, 0x08)?; // SIPI 页 8（0x8000）
    let h4 = l.send(IciFamily::Init, 3, 0)?;
    if l.slot_lens() != [1, 1, 2] || l.total() != 4 {
        return Err(XC_CONSERVE);
    }
    for h in [h1, h2, h3, h4].iter() {
        l.advance(*h)?;
        l.advance(*h)?;
        l.advance(*h)?;
        if l.row(*h)?.state != Dstate::Acked {
            return Err(XC_STATE);
        }
    }
    if !l.all_acked() || !l.conserve() {
        return Err(XC_CONSERVE);
    }
    // 2. 终态再推进拒
    if l.advance(h1).is_ok() {
        return Err(XC_STATE);
    }
    // 3. 错误路径①：族码非法（1/2/3/7）
    if IciFamily::from_bits(1).is_ok() || IciFamily::from_bits(7).is_ok() {
        return Err(XC_FAMILY);
    }
    // 4. 错误路径②：Fixed 向量越界（15）
    if l.send(IciFamily::Fixed, 1, 15).is_ok() {
        return Err(XC_VEC_RANGE);
    }
    // 5. 错误路径③：SIPI 页号越 8 位面（0x100）
    if l.send(IciFamily::Startup, 1, 0x100).is_ok() {
        return Err(XC_PAGE_RANGE);
    }
    // 6. 错误路径④：INIT 带非零向量
    if l.send(IciFamily::Init, 1, 0x41).is_ok() {
        return Err(XC_VEC_RANGE);
    }
    // 7. 错误路径⑤：目标核越界
    if l.send(IciFamily::Fixed, CORE_MAX, 0x41).is_ok() {
        return Err(XC_DEST_RANGE);
    }
    // 8. 错误路径⑥：悬空句柄
    if l.advance(999).is_ok() || l.row(999).is_ok() {
        return Err(XC_NOT_FOUND);
    }
    // 9. force_acked：Delivered 前拒，Delivered 后可
    let h5 = l.send(IciFamily::Fixed, 0, 0x42)?;
    if l.force_acked(h5).is_ok() {
        return Err(XC_STATE);
    }
    l.advance(h5)?; // → Sent
    if l.force_acked(h5).is_ok() {
        return Err(XC_STATE);
    }
    l.advance(h5)?; // → Delivered
    l.force_acked(h5)?;
    if l.row(h5)?.state != Dstate::Acked {
        return Err(XC_STATE);
    }
    // 10. 语义族互不混账：slot 行数对拍（Fixed 2 / NMI 1 / Init-Startup 2）
    if l.slot_lens() != [2, 1, 2] {
        return Err(XC_CONSERVE);
    }
    if l.conserve() && l.rejects() >= 6 {
        return Ok(0);
    }
    Err(XC_CONSERVE)
}
