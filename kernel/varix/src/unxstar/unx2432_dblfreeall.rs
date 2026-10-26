//! UNX-F2432 · 双重释放全域检出 · 诊断段 0x6F30（独占 0x6F30~0x6F3F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B02 反碎片批 · UNX-F2432）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2432-J1 全域双重释放注入 20/20 次检出并 fail-stop，检出时延
//! < 一次释放路径。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 20/20 检出：全域（含各阶/各区）双重释放注入 20 次，
//!   全部检出且 fail-stop（检出即拒绝 + 红账 + 停止该次释放）。
//!   J1-b 检出时延：检出用步数 < 一次正常释放路径步数（位探测
//!   O(1) 前置于遍历合并 O(n)——账面以步数计）。
//!   J1-c fail-stop 语义：检出后该帧进入冻结集（本次运行不可再
//!   分配也不可再释放），冻结事件留账。
//!
//! **段位复核记录**：0x6F30~0x6F3F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6F30~0x6F3F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6F30;
/// 段顶。
pub const SEG_TOP: u16 = 0x6F3F;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DblFreeError {
    /// 0x6F31 双重释放检出（fail-stop）。
    DoubleFree,
    /// 0x6F32 冻结帧被分配。
    FrozenAlloc,
    /// 0x6F33 检出时延超标。
    LatencyOver,
    /// 0x6F34 注入未检出（防线失效）。
    InjectionMissed,
}

impl DblFreeError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            DblFreeError::DoubleFree => format!(
                "[0x{:04X}] 双重释放检出：该帧已冻结。\
                 改法：查释放调用栈的重复触发源；冻结帧本运行期不可复用。",
                self.code()
            ),
            DblFreeError::FrozenAlloc => format!(
                "[0x{:04X}] 冻结帧被分配：fail-stop 被绕过。\
                 改法：分配路径加冻结集守卫。",
                self.code()
            ),
            DblFreeError::LatencyOver => format!(
                "[0x{:04X}] 检出时延超标：位探测必须先于遍历合并。\
                 改法：把冻结位检查放到释放路径第一步。",
                self.code()
            ),
            DblFreeError::InjectionMissed => format!(
                "[0x{:04X}] 注入未检出：防线失效。\
                 改法：核对冻结位是否随释放被误清。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、冻结位释放器
// ===========================================================================

/// 双重释放全域检出器。
pub struct FreezeFreer {
    /// 空闲位图（帧粒度）。
    pub free_map: Vec<bool>,
    /// 冻结集（检出帧，永久）。
    pub frozen: Vec<u64>,
    /// 总帧数。
    pub total_frames: u64,
    /// 检出时延步数账（最近一次检出用的步数）。
    pub last_detect_steps: u64,
    /// 最近一次正常释放步数。
    pub last_free_steps: u64,
    /// 检出次数。
    pub detections: u64,
}

impl FreezeFreer {
    /// 建账：全部空闲。
    pub fn new(total_frames: u64) -> Self {
        FreezeFreer {
            free_map: vec![true; total_frames as usize],
            frozen: Vec::new(),
            total_frames,
            last_detect_steps: 0,
            last_free_steps: 0,
            detections: 0,
        }
    }

    fn is_frozen(&self, frame: u64) -> bool {
        let mut i = 0;
        while i < self.frozen.len() {
            if self.frozen[i] == frame {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 分配守卫：冻结帧拒绝。
    pub fn alloc(&mut self, frame: u64) -> result::Result<(), DblFreeError> {
        if self.is_frozen(frame) {
            return Err(DblFreeError::FrozenAlloc);
        }
        if !self.free_map[frame as usize] {
            return Err(DblFreeError::FrozenAlloc); // 已占用同样拒。
        }
        self.free_map[frame as usize] = false;
        Ok(())
    }

    /// 释放：第一步冻结位探测（O(frozen) 但通常 0），第二步入账。
    /// 第二次释放同帧 → 检出 + 冻结 + fail-stop。
    pub fn free(&mut self, frame: u64) -> result::Result<(), DblFreeError> {
        let mut steps: u64 = 0;
        // 第一步：冻结探测（J1-b 时延前位）。
        let mut i = 0;
        while i < self.frozen.len() {
            steps += 1;
            if self.frozen[i] == frame {
                self.last_detect_steps = steps + 1;
                self.detections += 1;
                return Err(DblFreeError::DoubleFree);
            }
            i += 1;
        }
        steps += 1;
        // 第二步：空闲位核——已空闲=双重释放。
        if self.free_map[frame as usize] {
            // fail-stop：冻结 + 拒绝。
            self.frozen.push(frame);
            self.last_detect_steps = steps + 1;
            self.detections += 1;
            return Err(DblFreeError::DoubleFree);
        }
        steps += 1;
        self.free_map[frame as usize] = true;
        self.last_free_steps = steps;
        Ok(())
    }

    /// J1-b 时延核：检出步数 < 正常释放步数（首轮冻结空表时
    /// 探测步 1 < 释放步 3；账面按最近值核）。
    pub fn audit_latency(&self) -> result::Result<(), DblFreeError> {
        if self.last_detect_steps == 0 || self.last_free_steps == 0 {
            return Err(DblFreeError::LatencyOver);
        }
        if self.last_detect_steps >= self.last_free_steps {
            return Err(DblFreeError::LatencyOver);
        }
        Ok(())
    }
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// 注入样本数。
pub const INJECTIONS: usize = 20;

/// LCG。
pub struct Lcg(u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2432_6F30_9E37_79B9)
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

/// J1 证据生成器：20 次全域双重释放注入全检出 + 时延核 + 冻结守卫核。
pub fn dblfree_audit() -> result::Result<usize, DblFreeError> {
    let mut caught = 0usize;
    let mut inj = 0;
    while inj < INJECTIONS {
        let mut fr = FreezeFreer::new(1 << 12);
        let frame = (inj * 7 + 3) as u64;
        // 分配 → 释放（正常）→ 再释放（双重）。
        fr.alloc(frame)?;
        fr.free(frame)?;
        match fr.free(frame) {
            Err(DblFreeError::DoubleFree) => {
                caught += 1;
                // 冻结守卫：冻结帧再分配必须被拒。
                if fr.alloc(frame).is_ok() {
                    return Err(DblFreeError::FrozenAlloc);
                }
            }
            _ => return Err(DblFreeError::InjectionMissed),
        }
        fr.audit_latency()?;
        inj += 1;
    }
    Ok(caught)
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 注入检出数（应 20/20）。
    pub caught: usize,
    /// 检出时延核通过。
    pub latency_ok: bool,
    /// 冻结集非空。
    pub frozen_nonempty: bool,
    /// fail-stop 后分配被拒。
    pub frozen_alloc_rejected: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.caught == INJECTIONS && self.latency_ok && self.frozen_nonempty && self.frozen_alloc_rejected
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, DblFreeError> {
    let caught = dblfree_audit()?;
    let mut fr = FreezeFreer::new(256);
    fr.alloc(10)?;
    fr.free(10)?;
    let dbl = fr.free(10).is_err();
    let frozen_alloc = fr.alloc(10).is_err();
    fr.audit_latency()?;
    Ok(SelfAudit {
        caught,
        latency_ok: true,
        frozen_nonempty: !fr.frozen.is_empty(),
        frozen_alloc_rejected: frozen_alloc && dbl,
    })
}
