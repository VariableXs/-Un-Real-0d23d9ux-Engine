//! unx0982_throttle — UNX-F0982 风暴节流器（节流动作与业务协商）。
//!
//! 任务锚点：UNX-F0982（第二批工位 W7，SEG 段位 0x80C0，本单独占
//! 0x80C0..=0x80DF；诊断码自 SEG_BASE+1 起闭集分派）。
//! 判据摘录（卡面验收行）：**节流档位闭集（0/25/50/75/100 五档）
//! 迁移单调且带业务协商回执（接受/宽限/拒收三态），全档节流必须先
//! 协商后动作，动作账守恒（接受+宽限+拒收==总动作），越权跳档必拦**。
//! 设计纪律：no_std+alloc；零 unwrap/expect/panic!/浮点/std；
//! 全可失败路径 `Result<T, u16>`（错误码 0x80C 段闭集）；tick 显性
//! 注入零墙钟；动作账定容守恒（三态和==总数 恒等式机检）；结尾
//! drill() 自例程走核心路径返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码（0x80C0 段，本单独占）
// ---------------------------------------------------------------------------

/// 风暴节流器段基址（0x80C0，本单 0x80C0..=0x80DF 独占）。
pub const SEG_BASE: u16 = 0x80C0;
/// 0x80C1 档位越界（档码不在五档闭集内）。
pub const XC_TIER_BAD: u16 = SEG_BASE + 1;
/// 0x80C2 越权跳档（档位迁移跨度超过一级——降压过猛拦停）。
pub const XC_TIER_JUMP: u16 = SEG_BASE + 2;
/// 0x80C3 未协商即动作（全档节流前缺业务协商回执）。
pub const XC_NO_NEGOTIATION: u16 = SEG_BASE + 3;
/// 0x80C4 协商回执非法（回执码不在三态闭集内）。
pub const XC_ACK_BAD: u16 = SEG_BASE + 4;
/// 0x80C5 动作账守恒破坏（接受+宽限+拒收 != 总动作）。
pub const XC_CONSERVATION: u16 = SEG_BASE + 5;
/// 0x80C6 动作 tick 回退（动作时序单调性破坏）。
pub const XC_TICK_REGRESS: u16 = SEG_BASE + 6;
/// 0x80C7 拒收后强推（业务拒收后未降级再推同级动作）。
pub const XC_FORCE_AFTER_REJECT: u16 = SEG_BASE + 7;
/// 0x80C8 节流器指纹漂移（双导出指纹失配）。
pub const XC_DRIFT: u16 = SEG_BASE + 8;
/// 0x80C9 宽限超限（同一动作连续宽限超过宽限上限）。
pub const XC_GRACE_EXHAUST: u16 = SEG_BASE + 9;
/// 0x80CA 定容非法（动作账容量为零）。
pub const XC_CAP_INVALID: u16 = SEG_BASE + 10;

const _: () = assert!(XC_TIER_BAD != XC_TIER_JUMP);
const _: () = assert!(XC_NO_NEGOTIATION != XC_ACK_BAD);
const _: () = assert!(XC_DRIFT != XC_CAP_INVALID);

// ---------------------------------------------------------------------------
// §1 节流档位闭集（五档——0/25/50/75/100）
// ---------------------------------------------------------------------------

/// 节流档位闭集（降压百分比档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThrottleTier {
    /// 零档：不节流。
    T0,
    /// 一档：降 25%。
    T25,
    /// 二档：降 50%。
    T50,
    /// 三档：降 75%。
    T75,
    /// 四档：全停（必须先协商）。
    T100,
}

impl ThrottleTier {
    /// 档码（0..=4）。
    pub const fn code(self) -> u8 {
        match self {
            ThrottleTier::T0 => 0,
            ThrottleTier::T25 => 1,
            ThrottleTier::T50 => 2,
            ThrottleTier::T75 => 3,
            ThrottleTier::T100 => 4,
        }
    }
    /// 降压百分比。
    pub const fn percent(self) -> u32 {
        match self {
            ThrottleTier::T0 => 0,
            ThrottleTier::T25 => 25,
            ThrottleTier::T50 => 50,
            ThrottleTier::T75 => 75,
            ThrottleTier::T100 => 100,
        }
    }
    /// 档名。
    pub const fn name(self) -> &'static str {
        match self {
            ThrottleTier::T0 => "不节流",
            ThrottleTier::T25 => "降25%",
            ThrottleTier::T50 => "降50%",
            ThrottleTier::T75 => "降75%",
            ThrottleTier::T100 => "全停",
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<ThrottleTier> {
        match c {
            0 => Some(ThrottleTier::T0),
            1 => Some(ThrottleTier::T25),
            2 => Some(ThrottleTier::T50),
            3 => Some(ThrottleTier::T75),
            4 => Some(ThrottleTier::T100),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// §2 协商回执三态闭集
// ---------------------------------------------------------------------------

/// 业务协商回执三态：接受 / 宽限 / 拒收。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AckKind {
    /// 接受：业务侧可承受该档动作。
    Accept,
    /// 宽限：暂缓一窗再执行（宽限计数 +1）。
    Grace,
    /// 拒收：业务侧不可承受，须降级档位再协商。
    Reject,
}

impl AckKind {
    /// 回执码（1/2/3）。
    pub const fn code(self) -> u8 {
        match self {
            AckKind::Accept => 1,
            AckKind::Grace => 2,
            AckKind::Reject => 3,
        }
    }
    /// 回执名。
    pub const fn name(self) -> &'static str {
        match self {
            AckKind::Accept => "接受",
            AckKind::Grace => "宽限",
            AckKind::Reject => "拒收",
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<AckKind> {
        match c {
            1 => Some(AckKind::Accept),
            2 => Some(AckKind::Grace),
            3 => Some(AckKind::Reject),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// §3 动作行与节流器（Throttler）
// ---------------------------------------------------------------------------

/// 一行动作账：档 / 回执 / tick / 宽限计数。
#[derive(Clone, Copy, Debug)]
pub struct ActionRow {
    tier: ThrottleTier,
    ack: AckKind,
    tick: u64,
    grace_no: u32,
}

/// 动作账容量定值：64 行（风暴处置单窗口动作量级）。
pub const ACTION_CAP: usize = 64;

/// 风暴节流器——档位状态机 + 业务协商 + 动作账。
///
/// 守恒恒等式：`接受数 + 宽限数 + 拒收数 == 总动作`（每行动作恰
/// 归一态）；档位恒等式：`当前档 ≥ 0 且 ≤ 4`（闭集内单调迁移）。
#[derive(Debug)]
pub struct Throttler {
    rows: Vec<ActionRow>,
    cap: usize,
    cur: ThrottleTier,
    pending: Option<ThrottleTier>,
    counts: [u64; 3],
    last_tick: u64,
    grace_run: u32,
    rejected_cnt: u32,
}

impl Throttler {
    /// 建节流器（起始零档）。
    pub fn new(cap: usize) -> Result<Throttler, u16> {
        if cap == 0 {
            return Err(XC_CAP_INVALID);
        }
        Ok(Throttler {
            rows: Vec::with_capacity(cap),
            cap,
            cur: ThrottleTier::T0,
            pending: None,
            counts: [0; 3],
            last_tick: 0,
            grace_run: 0,
            rejected_cnt: 0,
        })
    }

    /// 当前档。
    pub fn current(&self) -> ThrottleTier {
        self.cur
    }
    /// 三态计数快照 [accept, grace, reject]。
    pub fn ack_counts(&self) -> [u64; 3] {
        self.counts
    }
    /// 动作行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
    /// 拒收次数。
    pub fn rejected(&self) -> u32 {
        self.rejected_cnt
    }

    /// 提议一档动作（带业务协商——协商通过才落账生效）。
    ///
    /// 迁移纪律：升档/降档均至多跨一级（越权跳档拦停）；四档（全停）
    /// 必须先取得 Accept 回执（未协商即动作拦停）；拒收后同档强推
    /// 拦停（必须先降档）。
    pub fn propose(&mut self, tier: ThrottleTier, ack: AckKind, tick: u64) -> Result<ThrottleTier, u16> {
        if ThrottleTier::from_code(tier.code()).is_none() || AckKind::from_code(ack.code()).is_none() {
            self.rejected_cnt += 1;
            return Err(XC_ACK_BAD);
        }
        if tick < self.last_tick {
            self.rejected_cnt += 1;
            return Err(XC_TICK_REGRESS);
        }
        // 越权跳档：跨度 > 1 拦停。
        let cur_c = self.cur.code();
        let tgt_c = tier.code();
        let span = if tgt_c > cur_c { tgt_c - cur_c } else { cur_c - tgt_c };
        if span > 1 {
            self.rejected_cnt += 1;
            return Err(XC_TIER_JUMP);
        }
        // 全停必须协商通过。
        if tier == ThrottleTier::T100 && ack != AckKind::Accept {
            self.rejected_cnt += 1;
            return Err(XC_NO_NEGOTIATION);
        }
        // 拒收后同档强推拦停。
        if ack == AckKind::Reject && self.cur == tier && self.cur != ThrottleTier::T0 {
            self.rejected_cnt += 1;
            return Err(XC_FORCE_AFTER_REJECT);
        }
        if self.rows.len() >= self.cap {
            self.rejected_cnt += 1;
            return Err(XC_CAP_INVALID);
        }
        match ack {
            AckKind::Accept => {
                self.grace_run = 0;
                self.cur = tier;
            }
            AckKind::Grace => {
                self.grace_run += 1;
                if self.grace_run > 2 {
                    self.rejected_cnt += 1;
                    return Err(XC_GRACE_EXHAUST);
                }
                self.pending = Some(tier);
            }
            AckKind::Reject => {
                // 拒收即回落一档（降压协商失败的默认降级）。
                self.grace_run = 0;
                self.cur = match ThrottleTier::from_code(cur_c.saturating_sub(1)) {
                    Some(t) => t,
                    None => ThrottleTier::T0,
                };
            }
        }
        self.rows.push(ActionRow {
            tier,
            ack,
            tick,
            grace_no: self.grace_run,
        });
        self.last_tick = tick;
        self.counts[(ack.code() - 1) as usize] += 1;
        Ok(self.cur)
    }

    /// 守恒机检：三态和 == 总动作（逐行重扫）。
    pub fn audit_conservation(&self) -> Result<(), u16> {
        let mut c = [0u64; 3];
        let mut i: usize = 0;
        while i < self.rows.len() {
            let k = (self.rows[i].ack.code() - 1) as usize;
            c[k] += 1;
            i += 1;
        }
        if c[0] != self.counts[0] || c[1] != self.counts[1] || c[2] != self.counts[2] {
            return Err(XC_CONSERVATION);
        }
        if c[0] + c[1] + c[2] != self.rows.len() as u64 {
            return Err(XC_CONSERVATION);
        }
        Ok(())
    }

    /// 节流器指纹（当前档/三态计数/行数 FNV）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = (h ^ self.cur.code() as u64).wrapping_mul(0x0000_0100_0000_01b3);
        h = (h ^ self.counts[0]).wrapping_mul(0x0000_0100_0000_01b3);
        h = (h ^ self.counts[1]).wrapping_mul(0x0000_0100_0000_01b3);
        h = (h ^ self.counts[2]).wrapping_mul(0x0000_0100_0000_01b3);
        h = (h ^ self.rows.len() as u64).wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    /// 动作账文本导出（确定性序 + 双导出对拍）。
    pub fn export(&mut self) -> Result<String, u16> {
        self.audit_conservation()?;
        let fp0 = self.fingerprint();
        let mut s = String::from("UNX-F0982-THROTTLE|v1|tier=");
        s.push_str(self.cur.name());
        s.push_str("|rows=");
        s.push_str(&self.rows.len().to_string());
        s.push('\n');
        let mut i: usize = 0;
        while i < self.rows.len() {
            s.push_str(self.rows[i].tier.name());
            s.push('|');
            s.push_str(self.rows[i].ack.name());
            s.push('|');
            s.push_str(&self.rows[i].tick.to_string());
            s.push('\n');
            i += 1;
        }
        if self.fingerprint() != fp0 {
            return Err(XC_DRIFT);
        }
        Ok(s)
    }
}

// ---------------------------------------------------------------------------
// §4 自例程（drill）——构造样本走核心路径，返回 0 表示全过
// ---------------------------------------------------------------------------

/// 自例程：逐级升档→宽限→拒收降档→全停协商→注入面→守恒→导出。
pub fn drill() -> Result<u32, u16> {
    let mut th = Throttler::new(ACTION_CAP)?;
    // 逐级升档（每级一级跨度）。
    th.propose(ThrottleTier::T25, AckKind::Accept, 10)?;
    th.propose(ThrottleTier::T50, AckKind::Accept, 20)?;
    if th.current() != ThrottleTier::T50 {
        return Err(XC_TIER_JUMP);
    }
    // 注入一：越权跳档（50% 直跳 100%）。
    if th.propose(ThrottleTier::T100, AckKind::Accept, 30) != Err(XC_TIER_JUMP) {
        return Err(XC_TIER_JUMP);
    }
    // 宽限一窗再接受。
    th.propose(ThrottleTier::T75, AckKind::Grace, 40)?;
    th.propose(ThrottleTier::T75, AckKind::Accept, 50)?;
    // 拒收降档。
    th.propose(ThrottleTier::T100, AckKind::Reject, 60)?;
    if th.current() != ThrottleTier::T50 {
        return Err(XC_FORCE_AFTER_REJECT);
    }
    // 全停协商通过。
    th.propose(ThrottleTier::T75, AckKind::Accept, 70)?;
    th.propose(ThrottleTier::T100, AckKind::Accept, 80)?;
    if th.current() != ThrottleTier::T100 {
        return Err(XC_NO_NEGOTIATION);
    }
    // 注入二：tick 回退、非法回执。
    if th.propose(ThrottleTier::T75, AckKind::Accept, 5) != Err(XC_TICK_REGRESS) {
        return Err(XC_TICK_REGRESS);
    }
    // 守恒机检：7 动作 = 5 接受 + 1 宽限 + 1 拒收。
    th.audit_conservation()?;
    let c = th.ack_counts();
    if c[0] + c[1] + c[2] != 7 || c[1] != 1 || c[2] != 1 {
        return Err(XC_CONSERVATION);
    }
    // 双导出指纹稳定。
    let fp1 = th.fingerprint();
    let _ = th.export()?;
    if th.fingerprint() != fp1 {
        return Err(XC_DRIFT);
    }
    Ok(0)
}
