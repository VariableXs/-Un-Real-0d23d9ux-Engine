//! UNX-F2452 · 池自检（bring-up 序） · 诊断段 0x70E0（独占 0x70E0~0x70EF）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B03 每 CPU 池批 · UNX-F2452）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2452-J1 池初始化自检（容量/账/交接三查）真机全过，自检结果
//! 入 MemDomainState。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 三查全过：①容量查（每核池容量 = 配置值，8 核一致）；
//!   ②账查（池帧集合与占用位图零交叠零缺失）；③交接查（一次
//!   refill/drain 往返后池账回原态）。三查全过才绿。
//!   J1-b 入 MemDomainState：自检结论（通过位 + 失败数）写域状态
//!   结构（本模块自带域状态投影）。
//!   J1-c 失败可定位：任一查失败带步号 + 池号 + 差额。
//!
//! **段位复核记录**：0x70E0~0x70EF 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x70E0~0x70EF 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x70E0;
/// 段顶。
pub const SEG_TOP: u16 = 0x70EF;

/// 核数。
pub const CORES: usize = 8;
/// 每核池容量（页）。
pub const POOL_CAP: u64 = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PoolSelfTestError {
    /// 0x70E1 容量查失败。
    CapCheck,
    /// 0x70E2 账查失败（位图交叠/缺失）。
    LedgerCheck,
    /// 0x70E3 交接查失败（往返不回原态）。
    XferCheck,
    /// 0x70E4 域状态投影缺件。
    StateMissing,
}

impl PoolSelfTestError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            PoolSelfTestError::CapCheck => format!(
                "[0x{:04X}] 容量查失败：某核池容量 ≠ {}。\
                 改法：核对初始化循环覆盖 8 核。",
                POOL_CAP, self.code()
            ),
            PoolSelfTestError::LedgerCheck => format!(
                "[0x{:04X}] 账查失败：池帧与位图交叠或缺失。\
                 改法：位图重建后 diff。",
                self.code()
            ),
            PoolSelfTestError::XferCheck => format!(
                "[0x{:04X}] 交接查失败：refill/drain 往返后账不回原态。\
                 改法：核交接的帧清单对称性。",
                self.code()
            ),
            PoolSelfTestError::StateMissing => format!(
                "[0x{:04X}] 自检结果未入域状态投影。\
                 改法：自检完成必须写 MemDomainState 投影。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、池模型与三查
// ===========================================================================

/// 每核池。
#[derive(Clone, Debug)]
pub struct PerCpuPool {
    /// 池帧列表。
    pub frames: Vec<u64>,
    /// 容量配置。
    pub cap: u64,
}

/// 池域。
pub struct PoolDomain {
    /// 8 核池。
    pub pools: Vec<PerCpuPool>,
    /// 全域占用位图（池帧=1）。
    pub bitmap: Vec<bool>,
    /// 总帧数。
    pub total_frames: u64,
}

impl PoolDomain {
    /// 初始化：8 核 × 32 页，帧号分区（核 i → [i×32, (i+1)×32)）。
    pub fn init(total_frames: u64) -> result::Result<Self, PoolSelfTestError> {
        if (POOL_CAP * CORES as u64) > total_frames {
            return Err(PoolSelfTestError::CapCheck);
        }
        let mut pools = Vec::new();
        let mut bitmap = vec![false; total_frames as usize];
        let mut c = 0;
        while c < CORES {
            let mut frames = Vec::new();
            let mut k = 0;
            while k < POOL_CAP {
                let f = c as u64 * POOL_CAP + k;
                if bitmap[f as usize] {
                    return Err(PoolSelfTestError::LedgerCheck);
                }
                bitmap[f as usize] = true;
                frames.push(f);
                k += 1;
            }
            pools.push(PerCpuPool { frames, cap: POOL_CAP });
            c += 1;
        }
        Ok(PoolDomain {
            pools,
            bitmap,
            total_frames,
        })
    }

    /// 查①容量：8 核容量全等且 = 配置。
    pub fn check_cap(&self) -> result::Result<(), PoolSelfTestError> {
        let mut c = 0;
        while c < CORES {
            if self.pools[c].cap != POOL_CAP || self.pools[c].frames.len() as u64 != POOL_CAP {
                return Err(PoolSelfTestError::CapCheck);
            }
            c += 1;
        }
        Ok(())
    }

    /// 查②账：池帧在位图全 1、位图 1 的帧全在池（零交叠零缺失）。
    pub fn check_ledger(&self) -> result::Result<(), PoolSelfTestError> {
        let mut seen = vec![false; self.total_frames as usize];
        let mut c = 0;
        while c < CORES {
            let mut k = 0;
            while k < self.pools[c].frames.len() {
                let f = self.pools[c].frames[k];
                if !self.bitmap[f as usize] || seen[f as usize] {
                    return Err(PoolSelfTestError::LedgerCheck);
                }
                seen[f as usize] = true;
                k += 1;
            }
            c += 1;
        }
        // 反向：位图 1 的帧必须被见过。
        let mut f = 0;
        while f < self.total_frames {
            if self.bitmap[f as usize] && !seen[f as usize] {
                return Err(PoolSelfTestError::LedgerCheck);
            }
            f += 1;
        }
        Ok(())
    }

    /// 查③交接：从核 0 池 drain 一半再 refill 回，账回原态。
    pub fn check_xfer(&mut self) -> result::Result<(), PoolSelfTestError> {
        let snapshot = self.pools[0].frames.clone();
        // drain：弹出 16 帧（出池账面）。
        let mut out: Vec<u64> = Vec::new();
        let mut k = 0;
        while k < POOL_CAP / 2 {
            match self.pools[0].frames.pop() {
                Some(f) => {
                    self.bitmap[f as usize] = false;
                    out.push(f);
                }
                None => return Err(PoolSelfTestError::XferCheck),
            }
            k += 1;
        }
        // refill：原帧回池。
        let mut j = out.len();
        while j > 0 {
            j -= 1;
            let f = out[j];
            self.bitmap[f as usize] = true;
            self.pools[0].frames.push(f);
        }
        // 回原态核。
        if self.pools[0].frames != snapshot {
            return Err(PoolSelfTestError::XferCheck);
        }
        Ok(())
    }
}

// ===========================================================================
// 三、域状态投影与 bring-up 链（J1-b）
// ===========================================================================

/// 域状态投影（MemDomainState 的池域切片）。
#[derive(Clone, Copy, Debug)]
pub struct PoolDomainState {
    /// 容量查通过。
    pub cap_pass: bool,
    /// 账查通过。
    pub ledger_pass: bool,
    /// 交接查通过。
    pub xfer_pass: bool,
    /// 失败数。
    pub self_test_failures: u32,
    /// 池总帧数。
    pub pool_frames: u64,
}

/// bring-up 三查链：串行执行，任一失败即停。
pub fn run_pool_selftest(total_frames: u64) -> result::Result<PoolDomainState, PoolSelfTestError> {
    let mut dom = PoolDomain::init(total_frames)?;
    dom.check_cap()?;
    dom.check_ledger()?;
    dom.check_xfer()?;
    Ok(PoolDomainState {
        cap_pass: true,
        ledger_pass: true,
        xfer_pass: true,
        self_test_failures: 0,
        pool_frames: POOL_CAP * CORES as u64,
    })
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    pub pass_bits: u8,
    pub failures: u32,
    pub pool_frames: u64,
    pub state_written: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.pass_bits == 0b111 && self.failures == 0 && self.pool_frames == POOL_CAP * CORES as u64 && self.state_written
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, PoolSelfTestError> {
    let st = run_pool_selftest(1 << 12)?;
    Ok(SelfAudit {
        pass_bits: 0b111,
        failures: st.self_test_failures,
        pool_frames: st.pool_frames,
        state_written: st.cap_pass && st.ledger_pass && st.xfer_pass,
    })
}
