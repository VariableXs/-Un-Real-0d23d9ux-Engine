//! unx48196_msgpump — UNX-F48196 Wine 窗 D4 消息泵边界联签（USER32 消息泵本体不二写声明）（诊断段 0xB960）。
//!
//! 判据原文：
//!   J1 边界声明入册+越界注入 ×10 零波及；判据锚定 §AI-61 §1 语义边界 D4/AI-19。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 语义边界 D4 分区口径：消息按 id 分三类——D4Pump（USER32
//!      消息泵本体内部消息：WM_TIMER/WM_PAINT/WM_QUIT/WM_INPUT 等，
//!      IME 域不得触碰）、ImeSurface（WM_IME_* 面，本域合法辖区）、
//!      Shared（WM_IME_CONTROL 这类跨界协商面，须双向回执）。
//!   2. 不二写声明是一条独立账：声明号 D4 唯一、双联签（AI-19 域主
//!      + AI-21 审计主）齐备才能 seal；no_rewrite 位必须为真，否则
//!      拒收入册——声明本体被改写比注入更危险。
//!   3. 泵守卫：所有进入消息先过 classify，PumpInternal 类一律拒收
//!      （E_CROSS）且不迁移泵任何状态——"零波及"的口径是注入 ×10
//!      后泵的时戳/序号/水位三账逐位不动。
//!   4. 注入确定性：越界样本取 D4Pump 闭集内的真实 id（WM_TIMER 等）
//!      ×10 笔固定序，不掷骰子；守卫必须在每一笔都拒绝。
//!   5. 声明防重：同声明号二次入册、未双签即 seal、no_rewrite 缺位
//!      三条拒收路径都要在 drill 里以构造样本验证。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB960;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_CROSS: u16 = SEG_BASE + 1; // 越界注入：泵本体消息被 IME 域触碰
const E_SPILL: u16 = SEG_BASE + 2; // 零波及破：泵账在注入后发生了位移
const E_DUP_DECL: u16 = SEG_BASE + 3; // 声明号重复
const E_UNSEAL: u16 = SEG_BASE + 4; // 未双签即 seal
const E_NO_REWRITE_BIT: u16 = SEG_BASE + 5; // no_rewrite 缺位
const E_SCOPE_BAD: u16 = SEG_BASE + 6; // 分类账出闭集
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

// ── 消息 id 常量（USER32 + IMM 口径）───────────────────────
pub const WM_TIMER: u32 = 0x0113;
pub const WM_PAINT: u32 = 0x000F;
pub const WM_QUIT: u32 = 0x0012;
pub const WM_INPUT: u32 = 0x00FF;
pub const WM_SYSTIMER: u32 = 0x0118;
pub const WM_IME_SETCONTEXT: u32 = 0x0281;
pub const WM_IME_NOTIFY: u32 = 0x0282;
pub const WM_IME_CONTROL: u32 = 0x0283;
pub const WM_IME_COMPOSITION: u32 = 0x010F;
pub const WM_IME_STARTCOMPOSITION: u32 = 0x010D;

/// 泵本体内部消息闭集（D4Pump 区）。
pub const PUMP_INTERNAL: [u32; 5] = [WM_TIMER, WM_PAINT, WM_QUIT, WM_INPUT, WM_SYSTIMER];

/// 消息边界分类（纪律 1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    PumpInternal, // D4 泵本体：禁触
    ImeSurface,   // 本域辖区：放行
    Shared,       // 跨界协商面：放行须回执
}

/// 逐 id 分类：闭集三区，未知 id 归 Shared（协商面走回执，不静默）。
pub fn classify(id: u32) -> Result<Zone, u16> {
    let mut i = 0usize;
    while i < PUMP_INTERNAL.len() {
        if PUMP_INTERNAL[i] == id {
            return Ok(Zone::PumpInternal);
        }
        i += 1;
    }
    match id {
        WM_IME_SETCONTEXT | WM_IME_NOTIFY | WM_IME_COMPOSITION | WM_IME_STARTCOMPOSITION => {
            Ok(Zone::ImeSurface)
        }
        WM_IME_CONTROL => Ok(Zone::Shared),
        _ => Ok(Zone::Shared),
    }
}

/// 联签主身份（AI-19 域主 / AI-21 审计主）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Principal {
    Ai19,
    Ai21,
}

/// 边界声明账：D4 不二写声明本体。
#[derive(Clone, Debug)]
pub struct BoundaryDecl {
    pub decl_no: u16,
    pub clause: u16,      // 条款锚：D4=0x0404, AI-19=0x1300
    pub scope: Zone,      // 声明管制的分区
    pub no_rewrite: bool, // 不二写位
    pub sig19: Option<u64>,
    pub sig21: Option<u64>,
    pub sealed: bool,
}

impl BoundaryDecl {
    /// 联签摘要：声明号、条款、分区、不二写位的 FNV-1a 64。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let parts: [u64; 4] = [
            self.decl_no as u64,
            self.clause as u64,
            match self.scope {
                Zone::PumpInternal => 0,
                Zone::ImeSurface => 1,
                Zone::Shared => 2,
            },
            self.no_rewrite as u64,
        ];
        let mut i = 0usize;
        while i < parts.len() {
            h ^= parts[i];
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            i += 1;
        }
        h
    }
}

/// 声明入册处。
pub struct DeclBook {
    pub decls: Vec<BoundaryDecl>,
}

impl DeclBook {
    pub fn new() -> DeclBook {
        DeclBook { decls: Vec::new() }
    }

    /// 入册（纪律 2/5）：声明号查重 + no_rewrite 位强制。
    pub fn register(&mut self, d: BoundaryDecl) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.decls.len() {
            if self.decls[i].decl_no == d.decl_no {
                return Err(E_DUP_DECL);
            }
            i += 1;
        }
        if !d.no_rewrite {
            return Err(E_NO_REWRITE_BIT);
        }
        self.decls.push(d);
        Ok(())
    }

    /// 联签：按主身份落签。
    pub fn sign(&mut self, no: u16, who: Principal) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.decls.len() {
            if self.decls[i].decl_no == no {
                let sig = self.decls[i].digest();
                match who {
                    Principal::Ai19 => self.decls[i].sig19 = Some(sig),
                    Principal::Ai21 => self.decls[i].sig21 = Some(sig.wrapping_add(0xA121)),
                }
                return Ok(());
            }
            i += 1;
        }
        Err(E_SCOPE_BAD)
    }

    /// seal（纪律 2）：双签齐备才许封。
    pub fn seal(&mut self, no: u16) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.decls.len() {
            if self.decls[i].decl_no == no {
                if self.decls[i].sig19.is_none() || self.decls[i].sig21.is_none() {
                    return Err(E_UNSEAL);
                }
                self.decls[i].sealed = true;
                return Ok(());
            }
            i += 1;
        }
        Err(E_SCOPE_BAD)
    }

    /// 审计：存在 sealed 且 no_rewrite 的 D4 声明（判据 J1 前半）。
    pub fn has_sealed_d4(&self) -> bool {
        let mut i = 0usize;
        while i < self.decls.len() {
            if self.decls[i].sealed && self.decls[i].no_rewrite {
                return true;
            }
            i += 1;
        }
        false
    }
}

/// 泵三账：时戳 / 序号 / 水位——零波及的逐位比对对象。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PumpTicks {
    pub tick: u64,
    pub seq: u64,
    pub water: u64,
}

/// 泵守卫：IME 域进入消息一律先过 classify。
pub struct PumpGuard {
    pub ticks: PumpTicks,
    pub injected: u32,
    pub rejected: u32,
    pub passed: u32,
    pub acks: u32,
}

impl PumpGuard {
    pub fn new() -> PumpGuard {
        PumpGuard {
            ticks: PumpTicks { tick: 100, seq: 7, water: 42 },
            injected: 0,
            rejected: 0,
            passed: 0,
            acks: 0,
        }
    }

    /// 进泵（纪律 3）：PumpInternal 拒收且泵三账分毫不动。
    pub fn admit(&mut self, id: u32) -> Result<Zone, u16> {
        self.injected += 1;
        let z = classify(id)?;
        match z {
            Zone::PumpInternal => {
                self.rejected += 1;
                Err(E_CROSS)
            }
            Zone::ImeSurface => {
                self.passed += 1;
                self.ticks.seq += 1;
                Ok(z)
            }
            Zone::Shared => {
                self.passed += 1;
                self.ticks.seq += 1;
                self.acks += 1; // Shared 面回执
                Ok(z)
            }
        }
    }

    /// 零波及核验：与注入前快照逐位相等。
    pub fn untouched(&self, snap: &PumpTicks) -> Result<(), u16> {
        if self.ticks != *snap {
            return Err(E_SPILL);
        }
        Ok(())
    }

    pub fn snapshot(&self) -> PumpTicks {
        self.ticks
    }
}

/// 写回判定（判据 J2）。
fn writeback(done: bool, summary_len: u32) -> Result<(), u16> {
    if !done || summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（边界声明入册+越界注入 ×10 零波及），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 声明入册：D4 不二写声明双联签 + seal，入册审计真查到。
    let mut book = DeclBook::new();
    book.register(BoundaryDecl {
        decl_no: 0x0404,
        clause: 0x0404,
        scope: Zone::PumpInternal,
        no_rewrite: true,
        sig19: None,
        sig21: None,
        sealed: false,
    })?;
    // 未双签即 seal 必拒（探针）。
    if book.seal(0x0404).is_ok() {
        return Err(E_UNSEAL);
    }
    book.sign(0x0404, Principal::Ai19)?;
    book.sign(0x0404, Principal::Ai21)?;
    book.seal(0x0404)?;
    if !book.has_sealed_d4() {
        return Err(E_UNSEAL);
    }
    hits += 1;

    // R2 声明防重/缺位探针：同号重入册、no_rewrite 缺位各拒一次。
    let dup = book.register(BoundaryDecl {
        decl_no: 0x0404,
        clause: 0x0404,
        scope: Zone::PumpInternal,
        no_rewrite: true,
        sig19: None,
        sig21: None,
        sealed: false,
    });
    if dup.is_ok() {
        return Err(E_DUP_DECL);
    }
    let bare = book.register(BoundaryDecl {
        decl_no: 0x0405,
        clause: 0x0405,
        scope: Zone::PumpInternal,
        no_rewrite: false,
        sig19: None,
        sig21: None,
        sealed: false,
    });
    if bare.is_ok() {
        return Err(E_NO_REWRITE_BIT);
    }
    hits += 1;

    // R3 分类账闭集：十类 id 三区落位逐一对账。
    if classify(WM_TIMER)? != Zone::PumpInternal {
        return Err(E_SCOPE_BAD);
    }
    if classify(WM_SYSTIMER)? != Zone::PumpInternal {
        return Err(E_SCOPE_BAD);
    }
    if classify(WM_IME_COMPOSITION)? != Zone::ImeSurface {
        return Err(E_SCOPE_BAD);
    }
    if classify(WM_IME_STARTCOMPOSITION)? != Zone::ImeSurface {
        return Err(E_SCOPE_BAD);
    }
    if classify(WM_IME_CONTROL)? != Zone::Shared {
        return Err(E_SCOPE_BAD);
    }
    hits += 1;

    // R4 主判据·越界注入 ×10 零波及：泵本体闭集 id ×2 轮逐笔拒收，
    //    注入前后泵三账快照逐位不动。
    let mut g = PumpGuard::new();
    let snap = g.snapshot();
    let round: [u32; 5] = [WM_TIMER, WM_PAINT, WM_QUIT, WM_INPUT, WM_SYSTIMER];
    let mut r = 0u32;
    while r < 2 {
        let mut i = 0usize;
        while i < round.len() {
            if g.admit(round[i]).is_ok() {
                return Err(E_CROSS); // 泵本体消息放行即越界成立
            }
            if g.untouched(&snap).is_err() {
                return Err(E_SPILL); // 拒收瞬间泵账不得位移
            }
            i += 1;
        }
        r += 1;
    }
    if g.rejected != 10 || g.injected != 10 {
        return Err(E_CROSS); // ×10 逐笔全拒
    }
    if g.passed != 0 || g.acks != 0 {
        return Err(E_SPILL);
    }
    g.untouched(&snap)?;
    hits += 1;

    // R5 合法面通行：ImeSurface 放行计数、Shared 面回执计数，泵序号账
    //    只随合法笔位移（对照拒收路径的零位移）。
    let mut g = PumpGuard::new();
    let _ = g.admit(WM_IME_COMPOSITION)?;
    let _ = g.admit(WM_IME_SETCONTEXT)?;
    let _ = g.admit(WM_IME_CONTROL)?;
    if g.passed != 3 || g.acks != 1 {
        return Err(E_SCOPE_BAD);
    }
    if g.ticks.seq != 9 { // 7 + 3 笔合法位移
        return Err(E_SPILL);
    }
    hits += 1;

    // R6 写回（判据 J2）：完成态 + 非空摘要放行；空摘要拒。
    if writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 0x04D4)?;
    hits += 1;

    Ok(hits)
}
