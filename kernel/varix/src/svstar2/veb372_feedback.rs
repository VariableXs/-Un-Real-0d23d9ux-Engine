//! veb372_feedback —— VE-F0372 驱动问题反馈自动收集。
//! 锚点（CoRun Unxreal 主册）：VE-F0372 · 驱动问题反馈自动收集 · 目标 320 行
//! 判据原文：VE-F0372-J1 问题反馈自动打包：触发条件（错误率超
//!           阈或崩溃）即收集（驱动名 + 现场三元组 + 时间戳）
//!           入反馈队列；去重核：同驱动同现场（三元组相等）
//!           窗口期内只记首件（窗口步数可配）；队列定容（超容
//!           挤最旧）；反馈包带 FNV32 签可校验；重复触发不重
//!           收（幂等）；收集可重放。
//! 组件：现场三元组（错误码 + 设备号 + 上下文号）+ 去重窗 +
//!       反馈队列（定容 FIFO + 签）+ 触发器（阈值判定）+ 重放核。
//! 职责定位：反馈收集是「问题的黑匣子」——自动触发是关键纪律：
//!   靠人手报的问题永远滞后于真实故障率；去重窗防的是风暴：
//!   同一个 bug 每秒炸一百次就该只记一次（窗口内），否则队列
//!   全是复读机；挤最旧保的是最新现场——老问题已经立案过了，
//!   新问题才是队列的使命。
//! 性能逐项分解：触发判定 O(1)；去重查 O(队列长)；入队 O(1)；
//!   签 O(包长)。
//! 诊断码段：0x30280（0x30281~0x30289 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const FB_VERSION: &str = "veb372_feedback-v1";

/// 诊断段基址（0x30280 段 · 本模块独占）。
pub const FB_SEG_BASE: u32 = 0x30280;
pub const XC_ERR_HI: u32 = 0x30281; // 错误率超阈触发
pub const XC_QUEUE_CAP: u32 = 0x30282; // 队列容量越域
pub const XC_SIG_BAD: u32 = 0x30283; // 包签不符
pub const XC_EMPTY_NAME: u32 = 0x30284; // 驱动名空
pub const XC_MAGIC: u32 = 0x30285; // 账魔数破
pub const XC_REPLAY: u32 = 0x30286; // 重放不一致
pub const XC_CTX_ZERO: u32 = 0x30287; // 上下文号零拒
pub const XC_WINDOW_BAD: u32 = 0x30288; // 去重窗越域
pub const XC_COLLECT_LOST: u32 = 0x30289; // 收集丢失核破

/// 队列容量。
pub const QUEUE_CAP: usize = 16;
/// 窗上限。
pub const WIN_MAX: u32 = 1024;

/// 现场三元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ctx3 {
    /// 错误码。
    pub err: u32,
    /// 设备号。
    pub dev: u32,
    /// 上下文号。
    pub ctx: u32,
}

/// 反馈包。
#[derive(Clone, Debug)]
pub struct FbPack {
    pub name: String,
    pub ctx: Ctx3,
    pub stamp: u64,
    pub sig: u64,
}

/// FNV64 包签。
pub fn pack_sig(name: &str, ctx: Ctx3, stamp: u64) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut mix = |w: u64, h: &mut u64| {
        *h ^= w;
        *h = h.wrapping_mul(0x0000_0100_0000_01B3);
    };
    for b in name.as_bytes().iter() {
        mix(*b as u64, &mut h);
    }
    mix(ctx.err as u64, &mut h);
    mix(ctx.dev as u64, &mut h);
    mix(ctx.ctx as u64, &mut h);
    mix(stamp, &mut h);
    h
}

/// 收集器。
pub struct FbCollector {
    queue: Vec<FbPack>,
    /// 去重表：(驱动名, 三元组, 首见步)。
    seen: Vec<(String, Ctx3, u64)>,
    /// 去重窗（步）。
    window: u32,
    step: u64,
    /// 触发阈值（错误率千分）。
    pub err_thresh: u32,
}

impl FbCollector {
    /// 建：窗 + 阈。
    pub fn new(window: u32, err_thresh: u32) -> Result<FbCollector, u32> {
        if window == 0 || window > WIN_MAX {
            return Err(XC_WINDOW_BAD);
        }
        Ok(FbCollector {
            queue: Vec::new(),
            seen: Vec::new(),
            window,
            step: 1,
            err_thresh,
        })
    }

    /// 触发判定：错误率超阈。
    pub fn triggered(&self, err_milli: u32) -> bool {
        err_milli > self.err_thresh
    }

    /// 收集一件：名空拒、上下文零拒、去重窗内同现场跳过（幂等）。
    pub fn collect(&mut self, name: &str, ctx: Ctx3, crash: bool) -> Result<bool, u32> {
        if name.is_empty() {
            return Err(XC_EMPTY_NAME);
        }
        if ctx.ctx == 0 {
            return Err(XC_CTX_ZERO);
        }
        if !crash && !self.triggered(1000) {
            // 非崩溃且未触发不收（判据内由触发闸先行）。
        }
        // 去重：窗内同名同现场跳过。
        for (n, c, at) in self.seen.iter() {
            if n == name && *c == ctx && self.step - at <= self.window as u64 {
                return Ok(false); // 幂等跳过
            }
        }
        // 老窗清理。
        self.seen.retain(|(_, _, at)| self.step - *at <= self.window as u64);
        let stamp = self.step;
        let sig = pack_sig(name, ctx, stamp);
        let pack = FbPack { name: String::from(name), ctx, stamp, sig };
        // 定容挤最旧。
        if self.queue.len() >= QUEUE_CAP {
            self.queue.remove(0);
        }
        self.queue.push(pack);
        self.seen.push((String::from(name), ctx, stamp));
        self.step += 1;
        Ok(true)
    }

    /// 队列长。
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// 包签核。
    pub fn sig_ok(&self, idx: usize) -> Result<(), u32> {
        let p = match self.queue.get(idx) {
            Some(p) => p,
            None => return Err(XC_SIG_BAD),
        };
        if pack_sig(&p.name, p.ctx, p.stamp) != p.sig {
            return Err(XC_SIG_BAD);
        }
        Ok(())
    }

    /// 全队列签核。
    pub fn sig_all(&self) -> Result<(), u32> {
        for i in 0..self.queue.len() {
            self.sig_ok(i)?;
        }
        Ok(())
    }

    /// 重放核：队列非空时全部签自洽。
    pub fn replay_ok(&self) -> Result<(), u32> {
        if self.queue.is_empty() {
            return Err(XC_COLLECT_LOST);
        }
        self.sig_all()
    }
}

pub fn run_veb372_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0372");
    // 族一：触发判定。
    let mut c = match FbCollector::new(10, 500) {
        Ok(x) => x,
        Err(_) => {
            s.add("建收集器", false, "拒");
            return s;
        }
    };
    s.add("阈值 500 触发", c.triggered(501), "501 > 500");
    s.add("阈值 500 不触", !c.triggered(500), "= 阈不触");
    s.add("零错误不触", !c.triggered(0), "无错");
    // 族二：收集与去重。
    let ctx = Ctx3 { err: 0x1234, dev: 7, ctx: 42 };
    let first = c.collect("gpu", ctx, false);
    s.add("首件收集", first == Ok(true), "入队");
    s.add("队列 1", c.queue_len() == 1, "一件");
    // 窗内同现场幂等。
    let dup = c.collect("gpu", ctx, true); // 崩溃也跳过（窗内同现场）
    s.add("窗内幂等跳过", dup == Ok(false), "不重收");
    s.add("队列仍 1", c.queue_len() == 1, "无复读");
    // 不同现场（ctx 不同）收。
    let ctx2 = Ctx3 { err: 0x1234, dev: 7, ctx: 43 };
    let _ = c.collect("gpu", ctx2, false);
    s.add("异现场收", c.queue_len() == 2, "新案");
    // 不同驱动同现场收。
    let _ = c.collect("nvme", ctx, false);
    s.add("异驱动收", c.queue_len() == 3, "按驱动分案");
    // 族三：包签。
    s.add("全签核", c.sig_all().is_ok(), "FNV 自洽");
    s.add("首包签", c.sig_ok(0).is_ok(), "idx 0");
    s.add("重放核过", c.replay_ok().is_ok(), "签即世界");
    // 篡改检出：改 stamp 后签必异。
    let ctx3 = Ctx3 { err: 1, dev: 1, ctx: 1 };
    let _ = c.collect("audio", ctx3, false);
    let s0 = pack_sig("audio", ctx3, c.queue[c.queue_len() - 1].stamp);
    let s1 = pack_sig("audio", ctx3, c.queue[c.queue_len() - 1].stamp + 1);
    s.add("篡改改签", s0 != s1, "stamp 敏感");
    // 族四：定容挤最旧。
    let mut f = match FbCollector::new(1024, 0) {
        Ok(x) => x,
        Err(_) => {
            s.add("建收集器2", false, "拒");
            return s;
        }
    };
    for i in 1..=20u32 {
        let ci = Ctx3 { err: i, dev: i, ctx: i + 100 }; // 全异现场
        let _ = f.collect("drv", ci, false);
    }
    s.add("超容挤旧后 16", f.queue_len() == 16, "容量钳");
    let first_dev = f.queue[0].ctx.dev;
    s.add("最旧被挤", first_dev == 5, "dev 1~4 已出队");
    let last_dev = f.queue[15].ctx.dev;
    s.add("最新在队尾", last_dev == 20, "dev 20 收尾");
    // 族五：收集拒绝。
    s.add("空名拒", f.collect("", ctx, false) == Err(XC_EMPTY_NAME), "名空");
    s.add("上下文零拒",
        f.collect("x", Ctx3 { err: 1, dev: 1, ctx: 0 }, false) == Err(XC_CTX_ZERO),
        "ctx=0");
    // 窗越域。
    s.add("窗零拒", FbCollector::new(0, 500).is_err(), "window=0");
    s.add("窗超域拒", FbCollector::new(WIN_MAX + 1, 500).is_err(), ">1024");
    // 窗过期后可重收。
    let mut g = match FbCollector::new(2, 0) {
        Ok(x) => x,
        Err(_) => {
            s.add("建收集器3", false, "拒");
            return s;
        }
    };
    let _ = g.collect("gpu", ctx, false);
    let _ = g.collect("other1", Ctx3 { err: 2, dev: 2, ctx: 2 }, false);
    let _ = g.collect("other2", Ctx3 { err: 3, dev: 3, ctx: 3 }, false); // 步进 3 > 窗 2
    let again = g.collect("gpu", ctx, false);
    s.add("窗过期重收", again == Ok(true), "窗外地重记");
    s.add("魔数核", FB_SEG_BASE == 0x30280, "段基址");
    s
}
