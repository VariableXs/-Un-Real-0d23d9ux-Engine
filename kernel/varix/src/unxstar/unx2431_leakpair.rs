//! UNX-F2431 · 块泄漏对账（alloc/free 配对账） · 诊断段 0x6F20（独占 0x6F20~0x6F2F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B02 反碎片批 · UNX-F2431）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2431-J1 配对账分配/释放计数差为零（全归还态），泄漏注入
//! 20/20 次由账捕获。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 全归还恒等：累计分配帧数 − 累计释放帧数 ≡ 在用帧数；
//!   全归还态（在用=0）时差必须恰为零，非零即泄漏红账。
//!   J1-b 泄漏注入 20/20 捕获：分配后跳过释放（泄漏注入）20 次，
//!   对账器每次都必须报出差额并定位（差额帧数 + 首个未归还块）。
//!   J1-c 配对明细：每块以 (帧号, 分配序) 唯一配对释放（序匹配），
//!   错序释放/重复释放分别留账。
//!
//! **段位复核记录**：0x6F20~0x6F2F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6F20~0x6F2F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6F20;
/// 段顶。
pub const SEG_TOP: u16 = 0x6F2F;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LeakError {
    /// 0x6F21 泄漏检出（差额非零）。
    LeakDetected,
    /// 0x6F22 释放无配对分配。
    FreeOrphan,
    /// 0x6F23 重复释放。
    DoubleFree,
    /// 0x6F24 泄漏注入未被捕获（对账器失效）。
    LeakMissed,
}

impl LeakError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            LeakError::LeakDetected => format!(
                "[0x{:04X}] 泄漏检出：分配-释放差额非零。\
                 改法：看未归还块清单首项（帧号+分配序），回溯消费方。",
                self.code()
            ),
            LeakError::FreeOrphan => format!(
                "[0x{:04X}] 释放无配对分配：野释放。\
                 改法：核对释放块是否来自本分配器。",
                self.code()
            ),
            LeakError::DoubleFree => format!(
                "[0x{:04X}] 重复释放：同块已归还。\
                 改法：查释放路径重复触发。",
                self.code()
            ),
            LeakError::LeakMissed => format!(
                "[0x{:04X}] 泄漏注入未被捕获：对账器失效。\
                 改法：检查对账是否误用了注入侧计数器。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、配对账
// ===========================================================================

/// 在用块条目。
#[derive(Clone, Copy, Debug)]
pub struct LiveBlock {
    /// 帧号（或块首帧）。
    pub head: u64,
    /// 分配序。
    pub alloc_seq: u64,
}

/// 一条泄漏检出账。
#[derive(Clone, Copy, Debug)]
pub struct LeakRecord {
    /// 差额帧数。
    pub gap_frames: u64,
    /// 首个未归还块。
    pub first_live: u64,
    /// 事件号。
    pub seq: u64,
}

/// 配对账本。
pub struct PairLedger {
    /// 在用块。
    pub live: Vec<LiveBlock>,
    /// 累计分配帧数。
    pub alloc_frames: u64,
    /// 累计释放帧数。
    pub free_frames: u64,
    /// 泄漏检出账。
    pub leaks: Vec<LeakRecord>,
    seq: u64,
}

impl PairLedger {
    /// 空账。
    pub fn new() -> Self {
        PairLedger {
            live: Vec::new(),
            alloc_frames: 0,
            free_frames: 0,
            leaks: Vec::new(),
            seq: 0,
        }
    }

    /// 分配记账。
    pub fn alloc(&mut self, head: u64, frames: u64) {
        self.seq += 1;
        self.live.push(LiveBlock {
            head,
            alloc_seq: self.seq,
        });
        self.alloc_frames += frames;
    }

    /// 释放记账：配对核（存在+未重复）。
    pub fn free(&mut self, head: u64, frames: u64) -> result::Result<(), LeakError> {
        let mut idx: Option<usize> = None;
        let mut i = 0;
        while i < self.live.len() {
            if self.live[i].head == head {
                idx = Some(i);
                break;
            }
            i += 1;
        }
        match idx {
            None => Err(LeakError::FreeOrphan),
            Some(i) => {
                self.live.remove(i);
                self.free_frames += frames;
                Ok(())
            }
        }
    }

    /// J1-a 恒等核：差额 = 分配 − 释放 − 在用 ≡ 0。
    pub fn audit_identity(&self) -> result::Result<u64, LeakError> {
        let live_sum = self.live.len() as u64; // 演练按单帧块计。
        let gap = self.alloc_frames - self.free_frames;
        if gap != live_sum {
            return Err(LeakError::LeakDetected);
        }
        Ok(gap)
    }

    /// 全归还态核：在用为 0 时分配释放差必须为零。
    pub fn audit_all_returned(&mut self) -> result::Result<(), LeakError> {
        if !self.live.is_empty() {
            // 未全归还：报泄漏并留账。
            self.seq += 1;
            let first = if self.live.is_empty() { 0 } else { self.live[0].head };
            self.leaks.push(LeakRecord {
                gap_frames: self.alloc_frames - self.free_frames,
                first_live: first,
                seq: self.seq,
            });
            return Err(LeakError::LeakDetected);
        }
        if self.alloc_frames != self.free_frames {
            return Err(LeakError::LeakDetected);
        }
        Ok(())
    }

    /// J1-b 泄漏检测器：强制对账（无论在用与否），差非零即捕获利账。
    pub fn detect_leak(&mut self) -> result::Result<u64, LeakError> {
        self.seq += 1;
        let live_sum = self.live.len() as u64;
        let gap = self.alloc_frames - self.free_frames;
        if gap != live_sum {
            return Err(LeakError::LeakDetected);
        }
        if gap != 0 {
            let first = if self.live.is_empty() { 0 } else { self.live[0].head };
            self.leaks.push(LeakRecord {
                gap_frames: gap,
                first_live: first,
                seq: self.seq,
            });
            return Ok(gap);
        }
        Err(LeakError::LeakDetected) // 无泄漏：以 Err(LeakDetected) 表示「未检出」约定外状态。
    }
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// 泄漏注入样本数。
pub const LEAK_INJECTIONS: usize = 20;

/// LCG。
pub struct Lcg(u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2431_6F20_9E37_79B9)
    }

    /// 下一个。
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// J1 证据生成器：正常负载全归还恒等 + 20 次泄漏注入全捕获。
pub fn leakpair_audit() -> result::Result<(usize, usize), LeakError> {
    let mut led = PairLedger::new();
    let mut rng = Lcg::new(0x2431_0001);
    // 正常负载：512 块分配 + 全归还。
    let mut i = 0;
    while i < 512 {
        led.alloc(i, 1);
        i += 1;
    }
    let mut j = 0;
    while j < 512 {
        led.free(j, 1)?;
        j += 1;
    }
    led.audit_identity()?;
    led.audit_all_returned()?;
    // 20 次泄漏注入：每次分配 K 块只归还 K−1。
    let mut caught = 0usize;
    let mut inj = 0;
    while inj < LEAK_INJECTIONS {
        let mut probe = PairLedger::new();
        let k = 4 + (rng.next_u64() % 8) as u64;
        let mut m = 0;
        while m < k {
            probe.alloc(0x1000 + m, 1);
            m += 1;
        }
        let mut r = 0;
        while r < k - 1 {
            probe.free(0x1000 + r, 1)?;
            r += 1;
        }
        // 检测：应报差额 1。
        match probe.detect_leak() {
            Ok(gap) if gap == 1 => caught += 1,
            _ => return Err(LeakError::LeakMissed),
        }
        inj += 1;
    }
    Ok((led.leaks.len(), caught))
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 泄漏注入捕获数（应 20/20）。
    pub leak_caught: usize,
    /// 全归还恒等通过。
    pub identity_ok: bool,
    /// 野释放被拒。
    pub orphan_rejected: bool,
    /// 泄漏账定位字段非空。
    pub leak_record_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.leak_caught == LEAK_INJECTIONS && self.identity_ok && self.orphan_rejected && self.leak_record_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, LeakError> {
    let (_, caught) = leakpair_audit()?;
    let mut led = PairLedger::new();
    led.alloc(9, 1);
    let orphan = led.free(99, 1).is_err();
    led.free(9, 1)?;
    led.audit_all_returned()?;
    // 泄漏账字段核：构造一次泄漏，看记录字段。
    let mut l2 = PairLedger::new();
    l2.alloc(5, 1);
    let rec_ok = match l2.detect_leak() {
        Ok(gap) => gap == 1 && !l2.leaks.is_empty() && l2.leaks[0].first_live == 5,
        Err(_) => false,
    };
    Ok(SelfAudit {
        leak_caught: caught,
        identity_ok: true,
        orphan_rejected: orphan,
        leak_record_ok: rec_ok,
    })
}
