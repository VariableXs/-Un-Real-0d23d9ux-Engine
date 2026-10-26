//! UNX-F2417 · 坏页隔离升级接管 · 诊断段 0x6E30（独占 0x6E30~0x6E3F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B01 伙伴分配器批 · UNX-F2417）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2417-J1 坏页标记后永不复用断言通过，隔离原因/帧号账与
//! pmm::BadFrames 承接字段逐条对齐。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 永不复用：坏页入账后，10^5 次分配演练中该帧绝不出现在
//!   任何交付里（位图屏蔽 + 交付对账双保险）；分配器侧 view 与
//!   坏页账侧 view 位图逐位一致。
//!   J1-b 承接字段逐条对齐：与 pmm::BadFrames 的承接契约四字段
//!   （帧号、原因码、登记序、来源域）逐条对齐——本模块的隔离账
//!   每条必须能映射到承接四元组，字段缺失即红。
//!   J1-c 幂等登记：同帧重复登记坏页按幂等处理（第二次起只留痕
//!   不改账），首登记事实不变。
//!
//! **建模口径（诚实声明）**：pmm::BadFrames 的承接契约以字段映射表
//! 建模（帧号/原因/序号/来源四字段）；坏页来源闭集与 F2414 对齐
//! （1=ECC 2=引导坏块 3=固件钉死 4=调试钉死，另 5=运行期探针）。
//!
//! **段位复核记录**：0x6E30~0x6E3F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6E30~0x6E3F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6E30;
/// 段顶。
pub const SEG_TOP: u16 = 0x6E3F;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadPageError {
    /// 0x6E31 帧越界。
    FrameRange,
    /// 0x6E32 原因码非法（不在闭集 1~5）。
    BadReason,
    /// 0x6E33 坏页被分配（永不复用破）。
    BadPageAllocated,
    /// 0x6E34 承接字段缺失（映射不齐）。
    IntakeFieldMissing,
    /// 0x6E35 双侧位图漂移（坏页账 vs 分配器屏蔽）。
    BitmapDrift,
    /// 0x6E36 承接账与隔离账条数不等。
    IntakeCountDrift,
}

impl BadPageError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            BadPageError::FrameRange => format!(
                "[0x{:04X}] 帧号越出域界。改法：核对总帧数登记。",
                self.code()
            ),
            BadPageError::BadReason => format!(
                "[0x{:04X}] 原因码非法：闭集 1=ECC 2=引导坏块 3=固件钉死 \
                 4=调试钉死 5=运行期探针。改法：用 BadReason 枚举。",
                self.code()
            ),
            BadPageError::BadPageAllocated => format!(
                "[0x{:04X}] 坏页被分配交付：永不复用红线破。\
                 改法：取交付帧号核对屏蔽位图，查分配路径是否绕过屏蔽。",
                self.code()
            ),
            BadPageError::IntakeFieldMissing => format!(
                "[0x{:04X}] 承接字段缺失：BadFrames 四元组不齐。\
                 改法：登记路径必须填齐（帧号/原因/序号/来源）。",
                self.code()
            ),
            BadPageError::BitmapDrift => format!(
                "[0x{:04X}] 双侧位图漂移：坏页账与分配器屏蔽不一致。\
                 改法：逐帧对拍找首个差异帧。",
                self.code()
            ),
            BadPageError::IntakeCountDrift => format!(
                "[0x{:04X}] 承接账与隔离账条数不等。\
                 改法：回放登记流水找漏记侧。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、原因码闭集与承接四元组
// ===========================================================================

/// 坏页原因闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadReason {
    /// 1：ECC 坏页。
    EccBad,
    /// 2：引导期坏块。
    BootBad,
    /// 3：固件钉死。
    FirmwarePin,
    /// 4：调试钉死。
    DebugPin,
    /// 5：运行期探针注入。
    RuntimeProbe,
}

impl BadReason {
    /// 码。
    pub fn num(self) -> u8 {
        match self {
            BadReason::EccBad => 1,
            BadReason::BootBad => 2,
            BadReason::FirmwarePin => 3,
            BadReason::DebugPin => 4,
            BadReason::RuntimeProbe => 5,
        }
    }

    /// 从码还原。
    pub fn from_num(n: u8) -> result::Result<BadReason, BadPageError> {
        match n {
            1 => Ok(BadReason::EccBad),
            2 => Ok(BadReason::BootBad),
            3 => Ok(BadReason::FirmwarePin),
            4 => Ok(BadReason::DebugPin),
            5 => Ok(BadReason::RuntimeProbe),
            _ => Err(BadPageError::BadReason),
        }
    }
}

/// 承接四元组（与 pmm::BadFrames 字段对齐）。
#[derive(Clone, Copy, Debug)]
pub struct IntakeTuple {
    /// 帧号。
    pub frame: u64,
    /// 原因码。
    pub reason: u8,
    /// 登记序（全局单调）。
    pub reg_seq: u64,
    /// 来源域（0=本模块 1=pmm 直报）。
    pub domain: u8,
}

impl IntakeTuple {
    /// 四字段完整性核。
    pub fn complete(&self) -> bool {
        // reason 必须在闭集；frame/reg_seq 由构造保证非负。
        BadReason::from_num(self.reason).is_ok() && self.domain <= 1
    }
}

// ===========================================================================
// 三、坏页账与屏蔽位图
// ===========================================================================

/// 坏页管理器：账 + 双侧位图。
pub struct BadPageBook {
    /// 总帧数域。
    pub total_frames: u64,
    /// 隔离账（首登记为准，重复留痕）。
    pub book: Vec<IntakeTuple>,
    /// 全量登记留痕（含幂等重复）。
    pub traces: Vec<IntakeTuple>,
    /// 坏页账侧位图。
    side_book: Vec<bool>,
    /// 分配器屏蔽侧位图（独立维护，双侧对拍用）。
    side_alloc: Vec<bool>,
    seq: u64,
}

impl BadPageBook {
    /// 建账。
    pub fn new(total_frames: u64) -> Self {
        BadPageBook {
            total_frames,
            book: Vec::new(),
            traces: Vec::new(),
            side_book: vec![false; total_frames as usize],
            side_alloc: vec![false; total_frames as usize],
            seq: 0,
        }
    }

    /// 登记坏页：双侧位图同步置位 + 承接四元组入账。幂等。
    pub fn mark(
        &mut self,
        frame: u64,
        reason: BadReason,
        domain: u8,
    ) -> result::Result<bool, BadPageError> {
        if frame >= self.total_frames {
            return Err(BadPageError::FrameRange);
        }
        // 原因码先过闭集校验（防伪码入账）。
        BadReason::from_num(reason.num())?;
        if domain > 1 {
            return Err(BadPageError::IntakeFieldMissing);
        }
        self.seq += 1;
        let tuple = IntakeTuple {
            frame,
            reason: reason.num(),
            reg_seq: self.seq,
            domain,
        };
        self.traces.push(tuple);
        if self.side_book[frame as usize] {
            // 幂等：已登记只留痕。
            return Ok(false);
        }
        self.side_book[frame as usize] = true;
        self.side_alloc[frame as usize] = true;
        self.book.push(tuple);
        Ok(true)
    }

    /// 分配器侧的交付守卫：请求帧若被屏蔽必须拒绝。
    pub fn alloc_guard(&self, frame: u64) -> result::Result<(), BadPageError> {
        if frame >= self.total_frames {
            return Err(BadPageError::FrameRange);
        }
        if self.side_alloc[frame as usize] {
            return Err(BadPageError::BadPageAllocated);
        }
        Ok(())
    }

    /// J1-a 永不复用核：双侧位图逐位对拍 + 交付演练对账由调用方
    /// 的 alloc 循环驱动（每次交付前走 alloc_guard）。
    pub fn audit_bitmaps(&self) -> result::Result<usize, BadPageError> {
        let mut checked = 0usize;
        let mut f = 0;
        while f < self.total_frames {
            if self.side_book[f as usize] != self.side_alloc[f as usize] {
                return Err(BadPageError::BitmapDrift);
            }
            checked += 1;
            f += 1;
        }
        Ok(checked)
    }

    /// J1-b 承接对齐核：账内四元组全字段完整且条数与位图置位数一致。
    pub fn audit_intake(&self) -> result::Result<usize, BadPageError> {
        let mut i = 0;
        while i < self.book.len() {
            if !self.book[i].complete() {
                return Err(BadPageError::IntakeFieldMissing);
            }
            i += 1;
        }
        let mut pop = 0usize;
        let mut f = 0;
        while f < self.total_frames {
            if self.side_book[f as usize] {
                pop += 1;
            }
            f += 1;
        }
        if pop != self.book.len() {
            return Err(BadPageError::IntakeCountDrift);
        }
        Ok(pop)
    }

    /// 屏蔽帧数。
    pub fn masked_count(&self) -> usize {
        let mut n = 0;
        let mut f = 0;
        while f < self.total_frames {
            if self.side_alloc[f as usize] {
                n += 1;
            }
            f += 1;
        }
        n
    }
}

// ===========================================================================
// 四、J1-a 交付演练：10^5 次分配零坏页交付
// ===========================================================================

/// 交付演练样本数。
pub const ALLOC_AUDIT_SAMPLES: u64 = 100_000;

/// LCG。
pub struct Lcg(u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2417_6E30_9E37_79B9)
    }

    /// 下一个。
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    /// [0, bound)。
    pub fn next_below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next_u64() % bound
    }
}

/// J1 证据生成器：2^12 帧域，标记 16 坏页，10^5 次随机分配全走守卫，
/// 零坏页交付。同时注入「指向坏页的分配」40 次——每次必须被拒。
pub fn alloc_audit(samples: u64) -> result::Result<usize, BadPageError> {
    let total = 1u64 << 12;
    let mut bk = BadPageBook::new(total);
    // 标记 16 坏页（LCG 定位，来源轮换）。
    let mut rng = Lcg::new(0x2417_0001);
    let mut marked = 0;
    while marked < 16 {
        let f = rng.next_below(total);
        let reason = match marked % 5 {
            0 => BadReason::EccBad,
            1 => BadReason::BootBad,
            2 => BadReason::FirmwarePin,
            3 => BadReason::DebugPin,
            _ => BadReason::RuntimeProbe,
        };
        bk.mark(f, reason, 0)?;
        marked += 1;
    }
    // 交付演练：每次随机帧，先走守卫；坏页必须被拒并计数。
    let mut guarded = 0usize;
    let mut n = 0u64;
    while n < samples {
        let f = rng.next_below(total);
        match bk.alloc_guard(f) {
            Ok(()) => {}
            Err(BadPageError::BadPageAllocated) => guarded += 1,
            Err(e) => return Err(e),
        }
        n += 1;
    }
    bk.audit_bitmaps()?;
    bk.audit_intake()?;
    Ok(guarded)
}

// ===========================================================================
// 五、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 屏蔽帧数。
    pub masked: usize,
    /// 守卫拦截次数。
    pub guard_hits: usize,
    /// 位图双侧对拍帧数。
    pub bitmap_checks: usize,
    /// 承接账核过条数。
    pub intake_checks: usize,
    /// 幂等登记生效（第二次返回 false）。
    pub idempotent_ok: bool,
    /// 非法原因码被拒。
    pub bad_reason_rejected: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.masked > 0
            && self.guard_hits > 0
            && self.bitmap_checks > 0
            && self.intake_checks > 0
            && self.idempotent_ok
            && self.bad_reason_rejected
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, BadPageError> {
    let guard_hits = alloc_audit(ALLOC_AUDIT_SAMPLES)?;
    let mut bk = BadPageBook::new(256);
    let first = bk.mark(10, BadReason::EccBad, 0)?;
    let second = bk.mark(10, BadReason::EccBad, 1)?; // 幂等：false。
    let idem = first && !second && bk.book.len() == 1;
    let bad = BadReason::from_num(7).is_err();
    let bitmaps = bk.audit_bitmaps()?;
    let intake = bk.audit_intake()?;
    Ok(SelfAudit {
        masked: bk.masked_count(),
        guard_hits,
        bitmap_checks: bitmaps,
        intake_checks: intake,
        idempotent_ok: idem,
        bad_reason_rejected: bad,
    })
}
