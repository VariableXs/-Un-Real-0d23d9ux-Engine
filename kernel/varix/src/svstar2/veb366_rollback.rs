//! veb366_rollback —— VE-F0366 驱动回滚机制。
//! 锚点（CoRun Unxreal 主册）：VE-F0366 · 驱动回滚机制 · 目标 340 行
//! 判据原文：VE-F0366-J1 每次成功上线保留前版本快照（保留栈
//!           深 = 3）；回滚 = 弹栈恢复上一版本（版本号严格回
//!           退核：回滚后版本 < 回滚前）；栈空回滚拒（无快照
//!           可弹）；快照完整核（版本 + 校验 + 配置三元组齐才
//!           可作回滚点）；快照超龄失效（版本代差 > 5 的快照
//!           弹出即拒）；回滚留痕可重放。
//! 组件：快照（版本 + 内容签 + 配置号）+ 快照栈（定深 LIFO）+
//!       回滚引擎（弹栈 + 严格回退核 + 超龄核）+ 留痕账 + 重放核。
//! 职责定位：回滚机制是「驱动的后悔药」——后悔药的前提是「来
//!   得及」：栈深 3 是成本与能力的平衡（存太多占内存，存太少
//!   连环升级后无路可退）；严格回退核是诚实底线：回滚后版本
//!   号必须真的变小，防止「回滚」一个更新版本（那叫再次升级）；
//!   超龄失效承认现实：隔了 5 代的快照，配置语义早已漂移，弹
//!   出来大概率装不上，不如显式拒绝。
//! 性能逐项分解：压栈/弹栈 O(1)；超龄核 O(1)；重放 O(留痕数)。
//! 诊断码段：0x30100（0x30101~0x30109 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const RB_VERSION: &str = "veb366_rollback-v1";

/// 诊断段基址（0x30100 段 · 本模块独占）。
pub const RB_SEG_BASE: u32 = 0x30100;
pub const XC_STACK_EMPTY: u32 = 0x30101; // 栈空回滚拒
pub const XC_NOT_OLDER: u32 = 0x30102; // 回滚后版本未变老
pub const XC_SNAP_INCOMPLETE: u32 = 0x30103; // 快照三元组不齐
pub const XC_SNAP_STALE: u32 = 0x30104; // 快照超龄
pub const XC_DEPTH_HI: u32 = 0x30105; // 栈深超限
pub const XC_VER_ZERO: u32 = 0x30106; // 版本零拒
pub const XC_MAGIC: u32 = 0x30107; // 留痕魔数破
pub const XC_REPLAY: u32 = 0x30108; // 重放不一致
pub const XC_CFG_MISS: u32 = 0x30109; // 配置号缺失

/// 快照栈深。
pub const SNAP_DEPTH: usize = 3;
/// 超龄代差。
pub const STALE_GEN: u32 = 5;

/// 快照。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Snap {
    /// 版本号。
    pub ver: u32,
    /// 内容签。
    pub sig: u64,
    /// 配置号。
    pub cfg: u32,
}

impl Snap {
    /// 完整核。
    pub fn complete(&self) -> Result<(), u32> {
        if self.ver == 0 {
            return Err(XC_VER_ZERO);
        }
        if self.sig == 0 {
            return Err(XC_SNAP_INCOMPLETE);
        }
        if self.cfg == 0 {
            return Err(XC_CFG_MISS);
        }
        Ok(())
    }
}

/// 回滚机。
pub struct Rollback {
    stack: Vec<Snap>,
    /// 当前上线版本。
    pub cur_ver: u32,
    trace: Vec<(u64, u32, u32)>, // (步号, 回滚前, 回滚后)
    step: u64,
}

impl Rollback {
    /// 建：当前版本。
    pub fn new(cur_ver: u32) -> Result<Rollback, u32> {
        if cur_ver == 0 {
            return Err(XC_VER_ZERO);
        }
        Ok(Rollback { stack: Vec::new(), cur_ver, trace: Vec::new(), step: 1 })
    }

    /// 上线新版本（自动压当前版本快照）。
    pub fn upgrade(&mut self, new_ver: u32, snap: Snap) -> Result<(), u32> {
        snap.complete()?;
        if new_ver <= self.cur_ver {
            return Err(XC_NOT_OLDER); // 升级必须变新
        }
        if self.stack.len() >= SNAP_DEPTH {
            self.stack.remove(0); // 挤出最老
        }
        self.stack.push(snap);
        self.cur_ver = new_ver;
        Ok(())
    }

    /// 回滚：弹栈 + 严格回退核 + 超龄核。
    pub fn rollback(&mut self) -> Result<u32, u32> {
        let snap = match self.stack.pop() {
            Some(s) => s,
            None => return Err(XC_STACK_EMPTY),
        };
        snap.complete()?;
        if self.cur_ver - snap.ver > STALE_GEN {
            return Err(XC_SNAP_STALE); // 弹出即超龄
        }
        if snap.ver >= self.cur_ver {
            return Err(XC_NOT_OLDER); // 回滚必须变老
        }
        let prev = self.cur_ver;
        self.cur_ver = snap.ver;
        self.trace.push((self.step, prev, snap.ver));
        self.step += 1;
        Ok(snap.ver)
    }

    /// 栈深。
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// 留痕数。
    pub fn trace_len(&self) -> usize {
        self.trace.len()
    }

    /// 留痕核。
    pub fn trace_ok(&self) -> Result<(), u32> {
        let mut prev = 0u64;
        for (st, _, _) in self.trace.iter() {
            if *st <= prev {
                return Err(XC_MAGIC);
            }
            prev = *st;
        }
        Ok(())
    }

    /// 重放核：留痕序列的版本链一致（回滚后版本 = 下一留痕前版本）。
    pub fn replay_ok(&self) -> Result<(), u32> {
        for i in 1..self.trace.len() {
            let (_, _, after_prev) = self.trace[i - 1];
            let (_, before_cur, _) = self.trace[i];
            if before_cur != after_prev {
                return Err(XC_REPLAY);
            }
        }
        Ok(())
    }
}

pub fn run_veb366_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0366");
    // 族一：升级与回滚循环。
    let mut r = match Rollback::new(10) {
        Ok(x) => x,
        Err(_) => {
            s.add("建机", false, "拒");
            return s;
        }
    };
    s.add("初版 10", r.cur_ver == 10, "起始");
    // 升级到 11（快照 ver=10）。
    let up1 = r.upgrade(11, Snap { ver: 10, sig: 0xAA, cfg: 1 });
    s.add("升级到 11", up1.is_ok(), "快照压栈");
    s.add("栈深 1", r.depth() == 1, "一份快照");
    let up2 = r.upgrade(12, Snap { ver: 11, sig: 0xBB, cfg: 2 });
    s.add("升级到 12", up2.is_ok(), "栈深 2");
    let back1 = r.rollback();
    s.add("回滚到 11", back1 == Ok(11), "严格回退");
    s.add("回滚后版本 11", r.cur_ver == 11, "当前版本");
    let back2 = r.rollback();
    s.add("再滚到 10", back2 == Ok(10), "栈清空");
    s.add("栈空回滚拒", r.rollback() == Err(XC_STACK_EMPTY), "无快照");
    s.add("留痕 2", r.trace_len() == 2, "两跳");
    s.add("留痕步号核", r.trace_ok().is_ok(), "严格递增");
    s.add("留痕链核", r.replay_ok().is_ok(), "12→11→10");
    // 族二：栈深挤老。
    let mut d = match Rollback::new(1) {
        Ok(x) => x,
        Err(_) => {
            s.add("建机2", false, "拒");
            return s;
        }
    };
    for i in 2..=5u32 {
        let _ = d.upgrade(i, Snap { ver: i - 1, sig: i as u64, cfg: i });
    }
    s.add("四连升级", d.depth() == 3, "栈深封顶 3");
    s.add("最老被挤", d.rollback() == Ok(4), "ver1 快照已挤出");
    // 族三：不完整快照拒。
    let mut n = match Rollback::new(10) {
        Ok(x) => x,
        Err(_) => {
            s.add("建机3", false, "拒");
            return s;
        }
    };
    s.add("零签快照拒", n.upgrade(11, Snap { ver: 10, sig: 0, cfg: 1 }) == Err(XC_SNAP_INCOMPLETE), "sig=0");
    s.add("零配置拒", n.upgrade(11, Snap { ver: 10, sig: 1, cfg: 0 }) == Err(XC_CFG_MISS), "cfg=0");
    s.add("零版本拒", n.upgrade(11, Snap { ver: 0, sig: 1, cfg: 1 }) == Err(XC_VER_ZERO), "ver=0");
    s.add("拒绝不入栈", n.depth() == 0, "无副作用");
    // 升级版本不新拒。
    s.add("升级不新拒", n.upgrade(10, Snap { ver: 9, sig: 1, cfg: 1 }) == Err(XC_NOT_OLDER), "10 ≤ 10");
    // 族四：超龄拒。
    let mut g = match Rollback::new(20) {
        Ok(x) => x,
        Err(_) => {
            s.add("建机4", false, "拒");
            return s;
        }
    };
    let _ = g.upgrade(21, Snap { ver: 20, sig: 1, cfg: 1 }); // 挤出后 ver20 在栈
    let _ = g.upgrade(22, Snap { ver: 21, sig: 2, cfg: 2 });
    let _ = g.upgrade(23, Snap { ver: 22, sig: 3, cfg: 3 }); // ver20 被挤
    let _ = g.upgrade(26, Snap { ver: 23, sig: 4, cfg: 4 });
    s.add("栈底 20", g.rollback() == Ok(23), "先弹 23");
    s.add("超龄检出", g.rollback() == Err(XC_SNAP_STALE), "26 - 20 > 5");
    // 代差 = 5 恰好过：当前 26 回滚到 21 差 5。
    let mut h = match Rollback::new(21) {
        Ok(x) => x,
        Err(_) => {
            s.add("建机5", false, "拒");
            return s;
        }
    };
    let _ = h.upgrade(26, Snap { ver: 21, sig: 5, cfg: 5 });
    s.add("代差 5 恰好过", h.rollback() == Ok(21), "26 - 21 = 5");
    // 族五：建机版本零拒。
    s.add("建机零版拒", Rollback::new(0).is_err(), "ver=0");
    s.add("魔数核", RB_SEG_BASE == 0x30100, "段基址");
    s
}
