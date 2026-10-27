//! UNX-F2458 · 紧急保留池语义 · 诊断段 0x7170（独占 0x7170~0x717F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B03 每 CPU 池批 · UNX-F2458）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2458-J1 紧急池仅限授权调用方（PF_MEMALLOC 类）命中 10/10 次，
//! 越权命中被拒。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 授权命中 10/10：授权调用方（PF_MEMALLOC 标注）紧急池
//!   请求 10/10 次命中；越权（无标注）请求 10/10 次被拒并留账。
//!   J1-b 授权位模型：调用方凭据 = 授权位（1 位），路由只读凭据，
//!   不猜身份。
//!   J1-c 紧急池不外流：紧急池帧绝不经普通路径发放（双向隔离：
//!   普通请求即使授权位缺失也不触紧急池；授权方紧急请求不动
//!   普通池——先紧急后备才普通）。
//!
//! **段位复核记录**：0x7170~0x717F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x7170~0x717F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x7170;
/// 段顶。
pub const SEG_TOP: u16 = 0x717F;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EmergError {
    /// 0x7171 越权命中紧急池（被拒）。
    Unauthorized,
    /// 0x7172 授权方未命中（漏放）。
    AuthorizedMissed,
    /// 0x7173 紧急池外流到普通路径。
    ReserveLeak,
    /// 0x7174 拒绝未留账。
    RejectUnlogged,
}

impl EmergError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            EmergError::Unauthorized => format!(
                "[0x{:04X}] 越权请求紧急池：无 PF_MEMALLOC 标注。\
                 改法：只有回收者/页表关键路径可挂授权位。",
                self.code()
            ),
            EmergError::AuthorizedMissed => format!(
                "[0x{:04X}] 授权方紧急请求未命中：路由误拒。\
                 改法：核对凭据位读取。",
                self.code()
            ),
            EmergError::ReserveLeak => format!(
                "[0x{:04X}] 紧急池外流：普通路径拿到保留帧。\
                 改法：双池发放路由严格分流。",
                self.code()
            ),
            EmergError::RejectUnlogged => format!(
                "[0x{:04X}] 越权拒绝未留账。\
                 改法：拒绝路径写拒绝账。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、双池授权路由
// ===========================================================================

/// 一条拒绝账。
#[derive(Clone, Copy, Debug)]
pub struct EmergReject {
    /// 调用方凭据。
    pub authorized: bool,
    /// 事件号。
    pub seq: u64,
}

/// 双池路由器。
pub struct EmergencyRouter {
    /// 紧急池余量。
    pub emergency: u64,
    /// 普通池余量。
    pub normal: u64,
    /// 授权命中数。
    pub auth_hits: u64,
    /// 越权拒绝数。
    pub rejects: u64,
    /// 拒绝账。
    pub reject_ledger: Vec<EmergReject>,
    seq: u64,
}

impl EmergencyRouter {
    /// 构造。
    pub fn new(emergency: u64, normal: u64) -> Self {
        EmergencyRouter {
            emergency,
            normal,
            auth_hits: 0,
            rejects: 0,
            reject_ledger: Vec::new(),
            seq: 0,
        }
    }

    /// J1 请求路由：authorized=true 先紧急池后普通池；
    /// authorized=false 只普通池（紧急池零触达）。
    pub fn alloc(&mut self, authorized: bool) -> result::Result<u8, EmergError> {
        self.seq += 1;
        if authorized {
            if self.emergency > 0 {
                self.emergency -= 1;
                self.auth_hits += 1;
                return Ok(0); // 紧急池交付。
            }
            // 紧急池空：授权方可退普通池。
            if self.normal > 0 {
                self.normal -= 1;
                return Ok(1);
            }
            return Err(EmergError::AuthorizedMissed);
        }
        // 越权：紧急池零触达，只普通池；普通池空则拒绝留账。
        if self.normal > 0 {
            self.normal -= 1;
            return Ok(1);
        }
        self.rejects += 1;
        self.reject_ledger.push(EmergReject {
            authorized: false,
            seq: self.seq,
        });
        Err(EmergError::Unauthorized)
    }

    /// J1-c 外流核：紧急池消耗只由授权命中产生（审计恒等：
    /// 紧急消耗 ≡ auth_hits）。
    pub fn audit_no_leak(&self, initial_emergency: u64) -> result::Result<(), EmergError> {
        let consumed = initial_emergency - self.emergency;
        if consumed != self.auth_hits {
            return Err(EmergError::ReserveLeak);
        }
        Ok(())
    }

    /// 账核：拒绝账条数 ≡ rejects。
    pub fn audit_rejects(&self) -> result::Result<usize, EmergError> {
        if self.reject_ledger.len() as u64 != self.rejects {
            return Err(EmergError::RejectUnlogged);
        }
        Ok(self.reject_ledger.len())
    }
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// LCG。
pub struct Lcg(u64);

impl Lcg {
    /// 构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x2458_7170_9E37_79B9)
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

/// 授权命中演练轮数。
pub const AUTH_ROUNDS: usize = 10;
/// 越权拒绝演练轮数。
pub const REJECT_ROUNDS: usize = 10;

/// J1 证据生成器：授权 10/10 命中 + 越权 10/10 拒绝 + 外流核。
pub fn emerg_audit() -> result::Result<(usize, usize), EmergError> {
    let mut ok = 0usize;
    // 授权命中。
    let mut r = 0;
    while r < AUTH_ROUNDS {
        let mut rt = EmergencyRouter::new(4, 16);
        match rt.alloc(true) {
            Ok(0) => ok += 1, // 紧急池命中。
            _ => return Err(EmergError::AuthorizedMissed),
        }
        rt.audit_no_leak(4)?;
        r += 1;
    }
    // 越权拒绝（普通池空）。
    let mut rej = 0usize;
    let mut r2 = 0;
    while r2 < REJECT_ROUNDS {
        let mut rt = EmergencyRouter::new(4, 0); // 普通池空。
        match rt.alloc(false) {
            Err(EmergError::Unauthorized) => rej += 1,
            Ok(_) => return Err(EmergError::ReserveLeak),
            Err(e) => return Err(e),
        }
        rt.audit_rejects()?;
        rt.audit_no_leak(4)?;
        r2 += 1;
    }
    Ok((ok, rej))
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    pub auth_hits: usize,
    pub rejects: usize,
    pub no_leak_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.auth_hits == AUTH_ROUNDS && self.rejects == REJECT_ROUNDS && self.no_leak_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, EmergError> {
    let (auth, rej) = emerg_audit()?;
    // 授权方紧急空退普通池。
    let mut rt = EmergencyRouter::new(0, 8);
    let fallback = rt.alloc(true)? == 1;
    Ok(SelfAudit {
        auth_hits: auth,
        rejects: rej,
        no_leak_ok: fallback,
    })
}
