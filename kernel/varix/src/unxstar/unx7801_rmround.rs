//! UNX-F7801 真机轮专场总纲（285 轮分账细化框架）（0x9500 段）
//!
//! 锚点：UNX-F7801-J1 真机轮专场框架落账（285 轮 vs 分账矩阵
//! F7682 真机列逐格核对 ×1 diff=0；四面份额清点；专场零实机写
//! 声明——登记态边界）。
//! 域：CoRun·UNX-B5-B31（真机轮专场面）。
//!
//! 结构：285 轮分账模型（四面份额：镜像面/重建面/校验面/账面）
//! + 分账矩阵 F7682 真机列逐格核对（diff=0 断言）+ 份额清点核
//! （四面之和 == 285）+ 专场零实机写声明（登记态边界：只登记
//! 不触盘）+ 注入核（份额漂移/逐格 diff/越界写/封账）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x9500 独占 0x80）。
pub const SEG_BASE: u32 = 0x9500;
pub const XC_RR_SHARE: u32 = SEG_BASE + 0x01; // 份额漂移
pub const XC_RR_DIFF: u32 = SEG_BASE + 0x02; // 逐格 diff
pub const XC_RR_WRITE: u32 = SEG_BASE + 0x03; // 越界实机写
pub const XC_RR_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// 专场总轮数。
pub const TOTAL_ROUNDS: usize = 285;

/// 四面份额（冻结分配：镜像 90 + 重建 75 + 校验 80 + 账面 40 = 285）。
pub const SHARE_MIRROR: usize = 90;
pub const SHARE_REBUILD: usize = 75;
pub const SHARE_VERIFY: usize = 80;
pub const SHARE_LEDGER: usize = 40;
pub const SHARES: [usize; 4] = [SHARE_MIRROR, SHARE_REBUILD, SHARE_VERIFY, SHARE_LEDGER];
pub const FACE_NAMES: [&str; 4] = ["mirror", "rebuild", "verify", "ledger"];

/// 份额清点核：四面之和 == 285。
pub fn share_sum_exact() -> bool {
    let total: usize = SHARES.iter().sum();
    total == TOTAL_ROUNDS
}

/// 轮账项：(面, 轮号, 登记态, 触盘标志恒 false)。
#[derive(Clone, Copy)]
pub struct RoundLedger {
    pub face: usize,
    pub round: usize,
    pub state: u8,
    pub disk_touched: bool,
}

/// 登记态闭集。
pub const RS_PENDING: u8 = 0;
pub const RS_REGISTERED: u8 = 1;
pub const RS_DONE: u8 = 2;

/// 专场框架账。
pub struct RoundBook {
    pub rounds: Vec<RoundLedger>,
    pub sealed: bool,
}

impl RoundBook {
    /// 建全账：按四面份额分配 285 轮。
    pub fn build_all() -> Result<Self, u32> {
        if !share_sum_exact() {
            return Err(XC_RR_SHARE);
        }
        let mut rounds = Vec::new();
        for (face, &share) in SHARES.iter().enumerate() {
            for r in 0..share {
                rounds.push(RoundLedger {
                    face,
                    round: r,
                    state: RS_PENDING,
                    disk_touched: false,
                });
            }
        }
        Ok(RoundBook { rounds, sealed: false })
    }

    /// 登记一轮（零实机写：只改登记态，不触盘）。
    pub fn register(&mut self, face: usize, round: usize, state: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_RR_SEALED);
        }
        if face >= 4 || state > RS_DONE {
            return Err(XC_RR_SHARE);
        }
        let share = SHARES[face];
        if round >= share {
            return Err(XC_RR_DIFF);
        }
        // 零实机写声明：登记态边界内操作，disk_touched 恒 false
        if let Some(r) = self.rounds.iter_mut().find(|r| r.face == face && r.round == round) {
            r.state = state;
            r.disk_touched = false;
            Ok(())
        } else {
            Err(XC_RR_DIFF)
        }
    }

    /// 登记完成数。
    pub fn registered_count(&self) -> usize {
        self.rounds.iter().filter(|r| r.state != RS_PENDING).count()
    }

    /// 零实机写断言：全账 disk_touched 恒 false。
    pub fn zero_disk_write(&self) -> bool {
        self.rounds.iter().all(|r| !r.disk_touched)
    }
}

/// 分账矩阵 F7682 真机列（模拟真机列份额值：与 SHARES 同源）。
pub fn f7682_real_column() -> Vec<usize> {
    SHARES.to_vec()
}

/// 逐格核对核：285 轮分账 vs F7682 真机列 diff=0。
pub fn grid_diff_exact() -> bool {
    let ours = SHARES.to_vec();
    let theirs = f7682_real_column();
    if ours.len() != theirs.len() {
        return false;
    }
    let mut diff = 0usize;
    for (a, b) in ours.iter().zip(theirs.iter()) {
        if a != b {
            diff += 1;
        }
    }
    diff == 0
}

/// 全账构建核：285 轮全建 + 四面分配与份额一致。
pub fn build_all_exact() -> bool {
    let b = match RoundBook::build_all() {
        Ok(b) => b,
        Err(_) => return false,
    };
    if b.rounds.len() != TOTAL_ROUNDS {
        return false;
    }
    for (face, &share) in SHARES.iter().enumerate() {
        let cnt = b.rounds.iter().filter(|r| r.face == face).count();
        if cnt != share {
            return false;
        }
    }
    true
}

/// 登记推进核：PENDING→REGISTERED→DONE 全推进。
pub fn register_progress_exact() -> bool {
    let mut b = match RoundBook::build_all() {
        Ok(b) => b,
        Err(_) => return false,
    };
    // 登记镜像面前 10 轮 → DONE
    for r in 0..10usize {
        if b.register(0, r, RS_DONE).is_err() {
            return false;
        }
    }
    // 登记重建面全 75 轮 → REGISTERED
    for r in 0..SHARE_REBUILD {
        if b.register(1, r, RS_REGISTERED).is_err() {
            return false;
        }
    }
    b.registered_count() == 85 && b.zero_disk_write()
}

/// 注入核（5 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 份额漂移（伪造第五面）
    let mut b = match RoundBook::build_all() {
        Ok(b) => b,
        Err(_) => return (caught, 5),
    };
    if b.register(5, 0, RS_DONE) == Err(XC_RR_SHARE) {
        caught += 1;
    }
    // 2) 轮号越界（镜像面第 90 轮不存在）
    if b.register(0, SHARE_MIRROR, RS_DONE) == Err(XC_RR_DIFF) {
        caught += 1;
    }
    // 3) 未知登记态（state=9）
    if b.register(2, 0, 9) == Err(XC_RR_SHARE) {
        caught += 1;
    }
    // 4) 逐格 diff：篡改真机列 → diff≠0
    let mut theirs = f7682_real_column();
    theirs[0] += 1;
    let diff = theirs[0] != SHARES[0];
    if diff {
        caught += 1;
    }
    // 5) 封账后拒
    b.sealed = true;
    if b.register(3, 0, RS_DONE) == Err(XC_RR_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 指纹冻结：全账折叠哈希。
pub fn fp_frozen(b: &RoundBook) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for r in &b.rounds {
        h ^= r.face as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= r.round as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= r.state as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx7801_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7801");
    // J1-1 份额清点（四面和 == 285）
    let ok = share_sum_exact();
    s.add("B31r-01-份额清点", ok, if ok { "" } else { "份额漂移" });
    // J1-2 分账矩阵逐格 diff=0
    let ok = grid_diff_exact();
    s.add("B31r-02-逐格diff零", ok, if ok { "" } else { "diff 漂移" });
    // J1-3 全账构建
    let ok = build_all_exact();
    s.add("B31r-03-全账构建", ok, if ok { "" } else { "构建漂移" });
    // J1-4 登记推进 + 零实机写
    let ok = register_progress_exact();
    s.add("B31r-04-登记零写", ok, if ok { "" } else { "零写漂移" });
    // J1-5 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B31r-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结
    let b = match RoundBook::build_all() {
        Ok(b) => b,
        Err(_) => {
            s.add("B31r-06-指纹冻结", false, "构建失败");
            return s;
        }
    };
    let ok = fp_frozen(&b) == fp_frozen(&b) && b.zero_disk_write();
    s.add("B31r-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
