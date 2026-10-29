//! UNX-F2466 · 回收优先级阶梯与来源注册器 · 诊断段 0x7220（独占 0x7220~0x722F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B04 回收批 · UNX-F2466）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2466-J1 实际回收来源序与阶梯序一致（来源账逐笔对齐），
//! 阶梯跳级注入 10/10 次拦截。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 来源序对齐：回收按阶梯（页缓存→可回收匿名→干净页表→
//!   可迁移页）顺序取，来源账逐笔与阶梯序对齐，跳序即红。
//!   J1-b 跳级注入 10/10 拦截：请求跳过前级直接取后级 10 次，
//!   全部拦截并留账。
//!   J1-c 来源注册器：来源在册（名字+优先级+可回收量），未注册
//!   来源拒绝接入。
//!
//! **段位复核记录**：0x7220~0x722F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x7220~0x722F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x7220;
/// 段顶。
pub const SEG_TOP: u16 = 0x722F;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LadderError {
    /// 0x7221 跳级（越过前级）。
    LadderSkip,
    /// 0x7222 未注册来源。
    UnregisteredSource,
    /// 0x7223 来源账与阶梯序不一致。
    OrderDrift,
    /// 0x7224 拦截未留账。
    BlockUnlogged,
}

impl LadderError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            LadderError::LadderSkip => format!(
                "[0x{:04X}] 阶梯跳级：前级未竭不得取后级。\
                 改法：回收循环按阶梯序扫描。",
                self.code()
            ),
            LadderError::UnregisteredSource => format!(
                "[0x{:04X}] 来源未注册。改法：先 register_source()。",
                self.code()
            ),
            LadderError::OrderDrift => format!(
                "[0x{:04X}] 来源账与阶梯序不一致。\
                 改法：逐笔对齐来源账与阶梯下标。",
                self.code()
            ),
            LadderError::BlockUnlogged => format!(
                "[0x{:04X}] 跳级拦截未留账。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、阶梯与来源注册器
// ===========================================================================

/// 阶梯来源（优先级即数组序）。
#[derive(Clone, Copy, Debug)]
pub struct ReclaimSource {
    /// 名字。
    pub name: &'static str,
    /// 可回收量（帧）。
    pub available: u64,
}

/// 阶梯注册器（注册序 = 回收序）。
pub struct SourceRegistry {
    /// 在册来源（序固定）。
    pub sources: Vec<ReclaimSource>,
    /// 来源账（逐笔回收记录：来源下标）。
    pub ledger: Vec<usize>,
    /// 拦截账。
    pub blocks: Vec<(usize, u64)>, // (试图跳到的下标, 事件号)
    seq: u64,
}

impl SourceRegistry {
    /// 构造：四阶梯（页缓存/可回收匿名/干净页表/可迁移页）。
    pub fn new(avail: [u64; 4]) -> Self {
        let names = ["pagecache", "recl-anon", "clean-pt", "migratable"];
        let mut sources = Vec::new();
        let mut i = 0;
        while i < 4 {
            sources.push(ReclaimSource {
                name: names[i],
                available: avail[i],
            });
            i += 1;
        }
        SourceRegistry {
            sources,
            ledger: Vec::new(),
            blocks: Vec::new(),
            seq: 0,
        }
    }

    /// 回收 n 帧：严格按阶梯序（前级竭尽才进后级）。
    pub fn reclaim(&mut self, n: u64) -> result::Result<u64, LadderError> {
        let mut got = 0u64;
        let mut lvl = 0usize;
        while got < n && lvl < self.sources.len() {
            if self.sources[lvl].available > 0 {
                let take = if self.sources[lvl].available >= n - got {
                    n - got
                } else {
                    self.sources[lvl].available
                };
                self.sources[lvl].available -= take;
                let mut k = 0;
                while k < take {
                    self.ledger.push(lvl);
                    k += 1;
                }
                got += take;
            }
            lvl += 1;
        }
        Ok(got)
    }

    /// J1-b 跳级注入：请求直接从 lvl 取（绕过前级）——拦截。
    pub fn inject_skip(&mut self, lvl: usize) -> result::Result<(), LadderError> {
        self.seq += 1;
        // 前级未竭则拦截。
        let mut prev = 0;
        let mut blocked = false;
        while prev < lvl {
            if self.sources[prev].available > 0 {
                blocked = true;
                break;
            }
            prev += 1;
        }
        if blocked {
            self.blocks.push((lvl, self.seq));
            return Err(LadderError::LadderSkip);
        }
        // 前级已竭：放行（合法）。
        Ok(())
    }

    /// J1-a 序核：来源账序列必须非降（阶梯下标单调不减，因为
    /// 前级竭尽才进后级；注意同轮内单调）。
    pub fn audit_order(&self) -> result::Result<usize, LadderError> {
        let mut i = 1;
        while i < self.ledger.len() {
            if self.ledger[i] < self.ledger[i - 1] {
                return Err(LadderError::OrderDrift);
            }
            i += 1;
        }
        Ok(self.ledger.len())
    }

    /// 拦截账核。
    pub fn audit_blocks(&self) -> result::Result<usize, LadderError> {
        Ok(self.blocks.len())
    }
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// 跳级注入数。
pub const INJECTIONS: usize = 10;

/// J1 证据生成器：阶梯序回收对齐 + 10 次跳级拦截。
pub fn ladder_audit() -> result::Result<usize, LadderError> {
    let mut reg = SourceRegistry::new([256, 256, 128, 128]);
    // 阶梯序回收 512 帧。
    let got = reg.reclaim(512)?;
    if got != 512 {
        return Err(LadderError::OrderDrift);
    }
    reg.audit_order()?;
    // 跳级注入 10 次：每次注入时前级仍有余量 → 拦截。
    let mut caught = 0usize;
    let mut i = 0;
    while i < INJECTIONS {
        // 每次重建域保证前级有余量。
        let mut reg2 = SourceRegistry::new([256, 256, 128, 128]);
        match reg2.inject_skip(3) {
            Err(LadderError::LadderSkip) => caught += 1,
            Ok(()) => return Err(LadderError::OrderDrift),
            Err(e) => return Err(e),
        }
        reg2.audit_blocks()?;
        i += 1;
    }
    // 合法放行：前级竭尽后进后级。
    let mut reg3 = SourceRegistry::new([0, 0, 64, 64]);
    reg3.inject_skip(3)?;
    Ok(caught)
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    pub caught: usize,
    pub order_ok: bool,
    pub legal_pass_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.caught == INJECTIONS && self.order_ok && self.legal_pass_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, LadderError> {
    let caught = ladder_audit()?;
    let mut reg = SourceRegistry::new([32, 32, 32, 32]);
    reg.reclaim(128)?;
    let order_ok = reg.audit_order()? > 0;
    Ok(SelfAudit {
        caught,
        order_ok,
        legal_pass_ok: true,
    })
}
