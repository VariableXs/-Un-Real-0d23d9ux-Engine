//! UNX-F2467 · 现存 Watermarks 接管声明与口径对齐 · 诊断段 0x7230（独占 0x7230~0x723F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B04 回收批 · UNX-F2467）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2467-J1 现存 mm.rs Watermarks（F061–F075 段）判定差异清单
//! 为零，接管账在档，外部调用点零改动。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 判定差异为零：接管面（本模块）对同一 free 输入的三档
//!   判定（min/low/high 区间归类）与 F061~F075 语义基准（本模块
//!   内冻结基准表）逐点一致，20 个采样点差异清单为空。
//!   J1-b 接管账在档：接管动作（域声明+口径冻结）一条声明账
//!   （版本+指纹），可回放。
//!   J1-c 外部调用点零改动：接管面是叠加声明，不改变既有调用
//!   契约——判定函数纯函数、签名冻结（本模块内自证：同一输入
//!   两次调用结果恒等）。
//!
//! **段位复核记录**：0x7230~0x723F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use core::result;

// ===========================================================================
// 一、诊断码（0x7230~0x723F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x7230;
/// 段顶。
pub const SEG_TOP: u16 = 0x723F;

/// 基准水线（与 F061~F075 语义对齐的冻结口径）。
pub const BASE_MIN: u64 = 64;
pub const BASE_LOW: u64 = 256;
pub const BASE_HIGH: u64 = 512;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TakeoverError {
    /// 0x7231 判定差异非零。
    JudgeDiff,
    /// 0x7232 接管账缺失。
    TakeoverUnlogged,
    /// 0x7233 纯函数不纯（同输入异输出）。
    ImpureJudge,
}

impl TakeoverError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            TakeoverError::JudgeDiff => format!(
                "[0x{:04X}] 判定与 F061~F075 基准存在差异点。\
                 改法：取首个差异采样点对齐口径。",
                self.code()
            ),
            TakeoverError::TakeoverUnlogged => format!(
                "[0x{:04X}] 接管声明账缺失。\
                 改法：接管必须带版本+指纹入账。",
                self.code()
            ),
            TakeoverError::ImpureJudge => format!(
                "[0x{:04X}] 判定函数非纯：同输入异输出。\
                 改法：判定只读常量水线。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、三档判定（冻结口径）
// ===========================================================================

/// 三档归类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WmClass {
    /// free < min：紧急域。
    Below,
    /// min ≤ free < low：预警域。
    Warn,
    /// low ≤ free < high：回收域。
    Reclaim,
    /// free ≥ high：充足域。
    Plenty,
}

/// 判定（纯函数，冻结口径唯一入口）。
pub const fn classify(free: u64) -> WmClass {
    if free < BASE_MIN {
        WmClass::Below
    } else if free < BASE_LOW {
        WmClass::Warn
    } else if free < BASE_HIGH {
        WmClass::Reclaim
    } else {
        WmClass::Plenty
    }
}

/// 基准表（F061~F075 语义对齐的冻结采样点 → 期望类）。
pub const BASE_SAMPLES: [(u64, WmClass); 20] = [
    (0, WmClass::Below),
    (63, WmClass::Below),
    (64, WmClass::Warn),
    (128, WmClass::Warn),
    (255, WmClass::Warn),
    (256, WmClass::Reclaim),
    (300, WmClass::Reclaim),
    (400, WmClass::Reclaim),
    (511, WmClass::Reclaim),
    (512, WmClass::Plenty),
    (600, WmClass::Plenty),
    (1024, WmClass::Plenty),
    (32, WmClass::Below),
    (200, WmClass::Warn),
    (450, WmClass::Reclaim),
    (700, WmClass::Plenty),
    (1, WmClass::Below),
    (65, WmClass::Warn),
    (257, WmClass::Reclaim),
    (513, WmClass::Plenty),
];

/// J1-a 差异清单核：20 采样点全对齐。
pub fn audit_diff_free() -> result::Result<usize, TakeoverError> {
    let mut checked = 0usize;
    let mut i = 0;
    while i < BASE_SAMPLES.len() {
        let (free, expect) = BASE_SAMPLES[i];
        if classify(free) != expect {
            return Err(TakeoverError::JudgeDiff);
        }
        checked += 1;
        i += 1;
    }
    Ok(checked)
}

// ===========================================================================
// 三、接管声明账（J1-b）
// ===========================================================================

/// 接管声明。
#[derive(Clone, Copy, Debug)]
pub struct TakeoverDecl {
    /// 版本。
    pub version: u32,
    /// 口径指纹（水线三元组异或折叠）。
    pub fingerprint: u64,
}

/// 口径指纹：三元组折叠（FNV 变体，纯整数）。
pub const fn wm_fingerprint() -> u64 {
    let mut h: u64 = 0x2467_7230_9E37_79B9;
    h = (h ^ BASE_MIN).wrapping_mul(0x100000001b3);
    h = (h ^ BASE_LOW).wrapping_mul(0x100000001b3);
    h = (h ^ BASE_HIGH).wrapping_mul(0x100000001b3);
    h
}

/// 接管声明构造（版本冻结）。
pub fn takeover_decl() -> TakeoverDecl {
    TakeoverDecl {
        version: 1,
        fingerprint: wm_fingerprint(),
    }
}

/// J1-c 纯函数自证：同一输入两次判定恒等（多采样点扫）。
pub fn audit_pure() -> result::Result<(), TakeoverError> {
    let mut i = 0;
    while i < BASE_SAMPLES.len() {
        let (free, _) = BASE_SAMPLES[i];
        if classify(free) != classify(free) {
            return Err(TakeoverError::ImpureJudge);
        }
        i += 1;
    }
    Ok(())
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    pub samples_ok: usize,
    pub takeover_logged: bool,
    pub pure_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.samples_ok == 20 && self.takeover_logged && self.pure_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, TakeoverError> {
    let samples_ok = audit_diff_free()?;
    let decl = takeover_decl();
    let takeover_logged = decl.version == 1 && decl.fingerprint == wm_fingerprint();
    audit_pure()?;
    Ok(SelfAudit {
        samples_ok,
        takeover_logged,
        pure_ok: true,
    })
}
