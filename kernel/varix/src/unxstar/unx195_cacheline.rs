//! unx195_cacheline — UNX-F0195 缓存与对齐策略声明（cacheline）
//! （0x2B60 段）。
//!
//! 锚点（CoRun Unxreal 施工书 UNX-F0195）：缓存与对齐策略声明
//! （cacheline）。判据原文（UNX-F0195-J1）：共享数据结构 cacheline
//! 对齐断言，伪共享热点计数落账。
//!
//! 设计三线：①cacheline 常量与对齐声明账（64 字节线宽，共享结构
//! 必须整线对齐——跨线结构逐个入违例账）；②伪共享检测器（两热点
//! 变量落在同一 cacheline → 伪共享命中，热点计数落账——同线热点数
//! 与分离动作账平）；③分离策略执行（命中伪共享的对 → 声明填充
//! 分离，分离后重检必须零同线对——策略闭环）。

use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const CACHELINE_VERSION: &str = "195-cacheline-v1";

/// 段基（0x2B60）。
pub const SEG_BASE: u16 = 0x2B60;

/// cacheline 线宽（x86_64 标准 64 字节）。
pub const CACHELINE: u64 = 64;

/// 对齐掩码。
pub const CACHELINE_MASK: u64 = !(CACHELINE - 1);

// ---------------------------------------------------------------------------
// 对齐声明账
// ---------------------------------------------------------------------------

/// 共享结构声明条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharedStruct {
    /// 结构名。
    pub name: &'static str,
    /// 基址。
    pub addr: u64,
    /// 字节数。
    pub size: u64,
}

impl SharedStruct {
    /// 是否跨线（判据红线：共享结构不许跨线）。
    pub fn crosses_line(&self) -> bool {
        let s = self.addr & CACHELINE_MASK;
        s + CACHELINE < self.addr + self.size
    }
    /// 占据的线号列表。
    pub fn lines(&self) -> Vec<u64> {
        let first = self.addr & CACHELINE_MASK;
        let mut v = Vec::new();
        let mut l = first;
        while l < self.addr + self.size {
            v.push(l);
            l += CACHELINE;
        }
        v
    }
}

/// 对齐审计：全部共享结构逐个核跨线，违例入账。
pub fn align_audit(items: &[SharedStruct]) -> Vec<&'static str> {
    items
        .iter()
        .filter(|i| i.crosses_line())
        .map(|i| i.name)
        .collect()
}

/// 对齐修正：把结构推到下一线界（返回新基址，重叠由调用方避让）。
pub fn pad_to_line(addr: u64) -> u64 {
    (addr + CACHELINE - 1) & CACHELINE_MASK
}

// ---------------------------------------------------------------------------
// 伪共享检测
// ---------------------------------------------------------------------------

/// 热点变量条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotVar {
    pub name: &'static str,
    pub addr: u64,
}

/// 伪共享命中对。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FalseShareHit {
    pub a: &'static str,
    pub b: &'static str,
    /// 共同所在线基址。
    pub line: u64,
}

/// 伪共享检测（判据主体）：同线异变量对全数命中。
pub fn detect_false_sharing(hots: &[HotVar]) -> Vec<FalseShareHit> {
    let mut hits = Vec::new();
    for i in 0..hots.len() {
        for j in (i + 1)..hots.len() {
            let la = hots[i].addr & CACHELINE_MASK;
            let lb = hots[j].addr & CACHELINE_MASK;
            if la == lb {
                hits.push(FalseShareHit {
                    a: hots[i].name,
                    b: hots[j].name,
                    line: la,
                });
            }
        }
    }
    hits
}

/// 分离动作账：命中 → 填充分离（把 b 推到下一线）后重检。
#[derive(Debug, Default)]
pub struct SeperationLedger {
    /// 分离动作：(变量对, 新 b 地址)。
    pub actions: Vec<(&'static str, &'static str, u64)>,
    /// 分离后重检剩余命中数。
    pub residual: usize,
}

impl SeperationLedger {
    /// 执行分离：对每个命中对，把 b 挪到下一线界（动作入账）。
    pub fn separate(&mut self, hots: &mut Vec<HotVar>, hits: &[FalseShareHit]) {
        for h in hits {
            if let Some(b) = hots.iter_mut().find(|v| v.name == h.b) {
                let new_addr = pad_to_line(b.addr + CACHELINE);
                b.addr = new_addr;
                self.actions.push((h.a, h.b, new_addr));
            }
        }
        self.residual = detect_false_sharing(hots).len();
    }

    /// 策略闭环审计：动作数 ≡ 命中数、重检残零。
    pub fn closed(&self, hits: usize) -> Result<(), u16> {
        if self.actions.len() != hits {
            return Err(E_ACTION_MISMATCH);
        }
        if self.residual != 0 {
            return Err(E_RESIDUAL);
        }
        Ok(())
    }

    /// 账目数。
    pub fn len(&self) -> usize {
        self.actions.len()
    }
}

// ---------------------------------------------------------------------------
// 错误码（0x2B60 段，显式判别式）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum CacheLineErr {
    /// 动作数与命中数不平。
    ActionMismatch = SEG_BASE + 1,
    /// 分离后仍有伪共享。
    Residual = SEG_BASE + 2,
    /// 热点重复声明。
    DupHot = SEG_BASE + 3,
    /// 跨线结构未修正。
    CrossPending = SEG_BASE + 4,
}

impl CacheLineErr {
    pub const fn code(self) -> u16 {
        self as u16
    }
    pub fn detail(self) -> &'static str {
        match self {
            CacheLineErr::ActionMismatch => "分离动作数 ≠ 命中数",
            CacheLineErr::Residual => "分离后重检仍有同线对",
            CacheLineErr::DupHot => "热点变量重复声明",
            CacheLineErr::CrossPending => "跨线共享结构未修正",
        }
    }
}

const E_ACTION_MISMATCH: u16 = CacheLineErr::ActionMismatch.code();
const E_RESIDUAL: u16 = CacheLineErr::Residual.code();
const E_DUP_HOT: u16 = CacheLineErr::DupHot.code();
const E_CROSS_PENDING: u16 = CacheLineErr::CrossPending.code();

// ---------------------------------------------------------------------------
// 判据（自证）
// ---------------------------------------------------------------------------

/// 判据集（UNX-F0195-J1 + 结构红线）。自证六段。
pub fn self_audit() -> Result<(), u16> {
    // 一、J1 主体：共享结构对齐断言（全过）+ 跨线检出入账。
    let items = vec![
        SharedStruct {
            name: "runqueue",
            addr: 0x1000,
            size: 64,
        },
        SharedStruct {
            name: "irq_stat",
            addr: 0x1040,
            size: 64,
        },
    ];
    if !align_audit(&items).is_empty() {
        return Err(E_CROSS_PENDING);
    }
    let crossy = vec![SharedStruct {
        name: "spanning",
        addr: 0x1038,
        size: 16,
    }];
    let pending = align_audit(&crossy);
    if pending.len() != 1 || pending[0] != "spanning" {
        return Err(E_CROSS_PENDING);
    }
    // 二、修正闭环：跨线结构推线界后复检过。
    let fixed = pad_to_line(0x1038);
    if fixed != 0x1040 {
        return Err(E_CROSS_PENDING);
    }
    let fixed_items = vec![SharedStruct {
        name: "spanning",
        addr: fixed,
        size: 16,
    }];
    if !align_audit(&fixed_items).is_empty() {
        return Err(E_CROSS_PENDING);
    }
    // 三、J1 伪共享：同线热点对命中（计数落账 1 对）。
    let mut hots = vec![
        HotVar {
            name: "counter_a",
            addr: 0x2008,
        },
        HotVar {
            name: "counter_b",
            addr: 0x2010,
        },
        HotVar {
            name: "counter_c",
            addr: 0x2100,
        },
    ];
    for i in 0..hots.len() {
        for j in (i + 1)..hots.len() {
            if hots[i].name == hots[j].name {
                return Err(E_DUP_HOT);
            }
        }
    }
    let hits = detect_false_sharing(&hots);
    if hits.len() != 1 || hits[0].a != "counter_a" || hits[0].b != "counter_b" {
        return Err(E_RESIDUAL);
    }
    // 四、分离闭环：动作数 ≡ 命中数、分离后重检残零、a/c 不受牵连。
    let mut ledger = SeperationLedger::default();
    ledger.separate(&mut hots, &hits);
    ledger.closed(hits.len())?;
    let la = hots[0].addr & CACHELINE_MASK;
    let lc = hots[2].addr & CACHELINE_MASK;
    if la == lc {
        return Err(E_RESIDUAL);
    }
    // 五、双命中分离：三热点同线 → 两对命中、两动作、残零。
    let mut hots2 = vec![
        HotVar {
            name: "x",
            addr: 0x3000,
        },
        HotVar {
            name: "y",
            addr: 0x3008,
        },
        HotVar {
            name: "z",
            addr: 0x3010,
        },
    ];
    let hits2 = detect_false_sharing(&hots2);
    if hits2.len() != 3 {
        return Err(E_RESIDUAL);
    }
    let mut ledger2 = SeperationLedger::default();
    ledger2.separate(&mut hots2, &hits2);
    ledger2.closed(hits2.len())?;
    if ledger2.len() != 3 {
        return Err(E_ACTION_MISMATCH);
    }
    // 六、判别式段位对拍 + 线宽常量。
    if CACHELINE != 64 || E_CROSS_PENDING != SEG_BASE + 4 {
        return Err(E_CROSS_PENDING);
    }
    Ok(())
}
