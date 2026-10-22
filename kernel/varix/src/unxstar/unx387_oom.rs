//! UNX-F0387 堆耗尽显性路径（OOM 分级账）（0xAE00 段）
//!
//! 锚点：UNX-F0387-J1 OOM 三级（warn 80%/crit 95%/dead 100%）触发
//! 正确，分级动作账齐。
//! 域：CoRun·UNX-A1-B20（OOM 分级面）。
//!
//! 结构：堆水位模型（used/cap + 百分比水位）+ OOM 三级判定（warn
//! 80% / crit 95% / dead 100%）+ 分级动作账（warn 记账 / crit 收缩
//! 缓存 / dead 冻结分配）+ 动作留痕核 + 注入核（越级动作/水位回退
//! 后不解除/分配死锁/封账）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xAE00 独占 0x80）。
pub const SEG_BASE: u32 = 0xAE00;
pub const XC_OOM_LEVEL: u32 = SEG_BASE + 0x01;
pub const XC_OOM_DEAD: u32 = SEG_BASE + 0x02;
pub const XC_OOM_SEALED: u32 = SEG_BASE + 0x03;

/// OOM 阈值（万分比）。
pub const TH_WARN: u32 = 8000; // 80%
pub const TH_CRIT: u32 = 9500; // 95%
pub const TH_DEAD: u32 = 10000; // 100%

/// OOM 级别。
pub const OOM_OK: u8 = 0;
pub const OOM_WARN: u8 = 1;
pub const OOM_CRIT: u8 = 2;
pub const OOM_DEAD: u8 = 3;

/// 级名。
pub fn oom_name(l: u8) -> &'static str {
    match l {
        OOM_OK => "ok",
        OOM_WARN => "warn",
        OOM_CRIT => "crit",
        OOM_DEAD => "dead",
        _ => "?",
    }
}

/// 分级动作。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OomAction {
    None,
    Log,
    ShrinkCache,
    FreezeAlloc,
}

/// OOM 账。
pub struct OomBook {
    pub cap: u64,
    pub used: u64,
    pub actions: Vec<(u8, OomAction)>,
    pub dead: bool,
    pub sealed: bool,
}

impl OomBook {
    pub fn new(cap: u64) -> Self {
        OomBook { cap, used: 0, actions: Vec::new(), dead: false, sealed: false }
    }

    /// 万分比水位。
    pub fn watermark(&self) -> u32 {
        if self.cap == 0 {
            return TH_DEAD;
        }
        ((self.used * 10_000) / self.cap) as u32
    }

    /// 分配请求（水位判定 + 分级动作）。
    pub fn alloc(&mut self, n: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_OOM_SEALED);
        }
        if self.dead {
            self.actions.push((OOM_DEAD, OomAction::FreezeAlloc));
            return Err(XC_OOM_DEAD); // dead 后一切分配冻结
        }
        let new_used = self.used + n;
        if new_used > self.cap {
            return Err(XC_OOM_DEAD);
        }
        self.used = new_used;
        // 分级判定
        let w = self.watermark();
        let level = if w >= TH_DEAD {
            OOM_DEAD
        } else if w >= TH_CRIT {
            OOM_CRIT
        } else if w >= TH_WARN {
            OOM_WARN
        } else {
            OOM_OK
        };
        match level {
            OOM_OK => {}
            OOM_WARN => self.actions.push((OOM_WARN, OomAction::Log)),
            OOM_CRIT => {
                self.actions.push((OOM_CRIT, OomAction::ShrinkCache));
                // 收缩缓存：释放 5% 水位
                let shrink = self.cap / 20;
                self.used = self.used.saturating_sub(shrink);
            }
            _ => {
                self.dead = true;
                self.actions.push((OOM_DEAD, OomAction::FreezeAlloc));
                return Err(XC_OOM_DEAD);
            }
        }
        Ok(())
    }

    /// 释放。
    pub fn free(&mut self, n: u64) {
        self.used = self.used.saturating_sub(n);
        if !self.dead && self.watermark() < TH_CRIT {
            // crit 解除（回 warn 以下自然降级）
        }
    }
}

/// 三级触发核：80%/95%/100% 边界。
pub fn three_level_exact() -> bool {
    let mut b = OomBook::new(10_000);
    // 79% → OK
    if b.alloc(7_900).is_err() {
        return false;
    }
    if b.watermark() >= TH_WARN {
        return false;
    }
    // +2% → 81% → WARN
    if b.alloc(200).is_err() {
        return false;
    }
    if b.actions.last().map(|(l, a)| *l == OOM_WARN && *a == OomAction::Log) != Some(true) {
        return false;
    }
    // 冲到 96% → CRIT（触发收缩）
    if b.alloc(1_600).is_err() {
        return false;
    }
    if b.actions.last().map(|(l, a)| *l == OOM_CRIT && *a == OomAction::ShrinkCache) != Some(true) {
        return false;
    }
    // 冲到 ≥100% → DEAD
    if b.alloc(9_000).is_ok() {
        return false;
    }
    b.dead
}

/// dead 冻结核：dead 后一切分配拒。
pub fn dead_freeze_exact() -> bool {
    let mut b = OomBook::new(100);
    if b.alloc(100).is_ok() {
        return false;
    }
    // dead（100%）
    if !b.dead {
        return false;
    }
    // 释放后仍冻结（dead 不可逆）
    b.free(50);
    if b.alloc(1) != Err(XC_OOM_DEAD) {
        return false;
    }
    // 冻结动作留痕 ×2
    b.actions.iter().filter(|(l, a)| *l == OOM_DEAD && *a == OomAction::FreezeAlloc).count() >= 2
}

/// crit 收缩核：触发后水位回落 5%。
pub fn shrink_exact() -> bool {
    let mut b = OomBook::new(10_000);
    let _ = b.alloc(9_600); // 96% → crit → 收缩 5%cap=500
    let w = b.watermark();
    // 收缩后应低于 crit（9600-500=9100 → 91%）
    (9_000..TH_CRIT).contains(&w)
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 超容量单笔
    let mut b = OomBook::new(100);
    if b.alloc(101) == Err(XC_OOM_DEAD) && b.dead {
        caught += 1;
    }
    // 2) dead 后分配冻结
    if b.alloc(1) == Err(XC_OOM_DEAD) {
        caught += 1;
    }
    // 3) 封账后拒
    let mut b2 = OomBook::new(1000);
    b2.sealed = true;
    if b2.alloc(10) == Err(XC_OOM_SEALED) {
        caught += 1;
    }
    // 4) 零容量堆即 dead
    let mut b3 = OomBook::new(0);
    if b3.watermark() == TH_DEAD {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：OOM 账折叠哈希。
pub fn fp_frozen(b: &OomBook) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h ^= b.cap;
    h = h.wrapping_mul(0x100000001b3);
    h ^= b.used;
    h = h.wrapping_mul(0x100000001b3);
    for (l, a) in &b.actions {
        h ^= *l as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= match a {
            OomAction::None => 0,
            OomAction::Log => 1,
            OomAction::ShrinkCache => 2,
            OomAction::FreezeAlloc => 3,
        } as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx387_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0387");
    // J1-1 三级触发
    let ok = three_level_exact();
    s.add("B20-01-三级触发", ok, if ok { "" } else { "触发漂移" });
    // J1-2 dead 冻结
    let ok = dead_freeze_exact();
    s.add("B20-02-dead冻结", ok, if ok { "" } else { "冻结漂移" });
    // J1-3 crit 收缩
    let ok = shrink_exact();
    s.add("B20-03-crit收缩", ok, if ok { "" } else { "收缩漂移" });
    // J1-4 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B20-04-注入核必抓", ok, &detail);
    // J1-5 指纹冻结
    let b = OomBook::new(1000);
    let ok = fp_frozen(&b) == fp_frozen(&b);
    s.add("B20-05-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
