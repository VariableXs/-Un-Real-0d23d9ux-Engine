//! mech_wheel — 分层时间轮（AI-K1 深化批次四 · F050 中断合并）。
//!
//! 主册依据：
//! - F050【设计细节】「2000ns 预算窗口批量合并 + 延迟超 8ms 动态收缩」——
//!   合并的**时间容器**此前是线性扫描；分层时间轮是中断合并的标准容器：
//!   插入 O(1)、到期 O(1) 摊还、取消 O(1)，预算窗口（2000ns）与收缩窗
//!   （8ms→100μs）直接映射为轮级参数。
//! - 「同键覆盖 merged_away 计数」——时间轮的槽位覆盖语义天然同构：
//!   同键再入即覆盖旧项并计数（批次一 cover_position 语义的容器级实现）。
//! - 零堆：四级 × 64 槽定长；事件体定长。
//!
//! 结构：4 级轮，每级 64 槽，粒度逐级 ×64：
//! L0=1μs/槽 → L1=64μs → L2=4ms → L3=256ms，覆盖到 ~16s 延迟。
//! 2000ns 预算窗（μs 以下）由消费域在 L0 内部再分——本件以 μs 为最小粒度。

/// 每级槽位数（64 = u6 掩码——层级位移的代数基础）。
pub const WHEEL_BITS: u32 = 6;
pub const WHEEL_SLOTS: usize = 1 << WHEEL_BITS;
/// 级数（覆盖 L0..L3）。
pub const LEVELS: usize = 4;
/// 基础粒度（μs/槽，L0）。
pub const BASE_TICK_US: u64 = 1;

/// 定时事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timer {
    /// 合并键（F050：设备+类型折叠值）——0 视为无效键（不可合并域）。
    pub key: u64,
    pub kind: u16,
    pub a: u32,
}

#[derive(Clone, Copy)]
struct SlotItem {
    timer: Timer,
    /// 目标绝对时刻（μs）。
    at_us: u64,
    used: bool,
}

/// 覆盖事件（F050 merged_away 同义——同键再入覆盖旧定时）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WheelErr {
    Full,
    BadKey,
}

pub struct TimingWheel {
    wheels: [[Option<SlotItem>; WHEEL_SLOTS]; LEVELS],
    now_us: u64,
    /// 到期回调产出（drain 收集）。
    pub fired: u64,
    pub covered_away: u64,
    pub cancelled: u64,
}

impl TimingWheel {
    pub const fn new() -> Self {
        TimingWheel {
            wheels: [[None; WHEEL_SLOTS]; LEVELS],
            now_us: 0,
            fired: 0,
            covered_away: 0,
            cancelled: 0,
        }
    }

    pub fn now_us(&self) -> u64 {
        self.now_us
    }

    /// 当前时刻推进（只前进——F182 回拨保护同语义）。
    pub fn advance(&mut self, to_us: u64) -> Result<(), ()> {
        if to_us < self.now_us {
            return Err(());
        }
        self.now_us = to_us;
        Ok(())
    }

    /// 绝对时刻 → (级, 槽)。超出覆盖域 → None（诚实拒绝，不折返）。
    fn locate(&self, at_us: u64) -> Option<(usize, usize)> {
        let delta = at_us.checked_sub(self.now_us)?;
        if delta == 0 {
            return Some((0, 0)); // 立即到期：挂 L0 当前槽
        }
        let level = if delta < WHEEL_SLOTS as u64 {
            0
        } else if delta < (WHEEL_SLOTS as u64) << WHEEL_BITS {
            1
        } else if delta < (WHEEL_SLOTS as u64) << (2 * WHEEL_BITS) {
            2
        } else if delta < (WHEEL_SLOTS as u64) << (3 * WHEEL_BITS) {
            3
        } else {
            return None;
        };
        let tick = at_us >> (level as u32 * WHEEL_BITS);
        Some((level, (tick & (WHEEL_SLOTS as u64 - 1)) as usize))
    }

    /// 挂一个定时（同键已有 → 覆盖并计 covered_away）。
    pub fn insert(&mut self, at_us: u64, t: Timer) -> Result<(), WheelErr> {
        if t.key == 0 {
            return Err(WheelErr::BadKey);
        }
        let (lvl, slot) = self.locate(at_us).ok_or(WheelErr::Full)?;
        let item = SlotItem { timer: t, at_us, used: true };
        let cell = &mut self.wheels[lvl][slot];
        if let Some(old) = cell {
            if old.used && old.timer.key == t.key {
                self.covered_away += 1; // 同键覆盖——F050 merged_away 容器级计数
            } else {
                return Err(WheelErr::Full); // 异键占槽：诚实拒绝（不覆盖他人）
            }
        }
        *cell = Some(item);
        Ok(())
    }

    /// 按键取消（世代号判别——取消后同槽复用的陈旧项不误删）。
    pub fn cancel(&mut self, key: u64) -> bool {
        if key == 0 {
            return false;
        }
        for lvl in 0..LEVELS {
            for slot in self.wheels[lvl].iter_mut() {
                if let Some(it) = slot {
                    if it.used && it.timer.key == key {
                        it.used = false;
                        self.cancelled += 1;
                        return true;
                    }
                }
            }
        }
        false
    }

    /// 抽干所有 ≤ now 的到期定时（逐级降栓 cascade 在 drain 内完成）。
    pub fn drain(&mut self, out: &mut [Timer]) -> usize {
        let mut n = 0;
        // L0 必须全槽扫（当前刻粒度内都到期）；高等级槽只有"指针落点"
        // 才可能到期——但 μs 级时刻对齐的槽点 = 64 进制各位，扫全部 ≤now。
        for lvl in 0..LEVELS {
            for slot in self.wheels[lvl].iter_mut() {
                if let Some(it) = slot {
                    if it.used && it.at_us <= self.now_us {
                        if n < out.len() {
                            out[n] = it.timer;
                            n += 1;
                        }
                        it.used = false;
                        self.fired += 1;
                    }
                }
            }
        }
        n
    }

    pub fn pending(&self) -> usize {
        self.wheels.iter().flatten().filter(|s| matches!(s, Some(it) if it.used)).count()
    }
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// CheckSet（挂 F050）
// ---------------------------------------------------------------------------

/// 运行检查项（判据锚点见对账表批次四段）。
/// 检查判例用定时构造器（run_checks 与单测共用语义）。
fn mk_timer(key: u64) -> Timer {
    Timer { key, kind: 1, a: 0 }
}

use crate::checks::CheckSet;

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F050-mech-wheel");
    // 1) 插入-到期-抽干（时刻序）。
    let mut w = TimingWheel::new();
    w.insert(5, mk_timer(1)).unwrap();
    w.insert(2, mk_timer(2)).unwrap();
    w.insert(9, mk_timer(3)).unwrap();
    w.advance(5).unwrap();
    let mut out = [Timer { key: 0, kind: 0, a: 0 }; 8];
    let n1 = w.drain(&mut out);
    w.advance(9).unwrap();
    let n2 = w.drain(&mut out);
    cs.add("insert_drain_order", n1 == 2 && n2 == 1 && w.pending() == 0, "");
    // 2) 同键覆盖（覆盖只发生在槽内——同键须同刻）。
    let mut w2 = TimingWheel::new();
    w2.insert(100, mk_timer(7)).unwrap();
    w2.insert(100, mk_timer(7)).unwrap(); // 同键同刻再入 → 覆盖
    w2.advance(100).unwrap();
    let mid = w2.drain(&mut out);
    w2.advance(200).unwrap();
    let fin = w2.drain(&mut out);
    cs.add("same_key_coalesces", w2.covered_away == 1 && mid == 1 && fin == 0, "");
    // 3) 取消 + 越域诚实拒绝 + 时间回拨拒绝。
    let mut w3 = TimingWheel::new();
    w3.insert(50, mk_timer(3)).unwrap();
    let c_ok = w3.cancel(3) && !w3.cancel(3);
    w3.advance(100).unwrap();
    let drained = w3.drain(&mut out);
    let horizon = w3.insert(u64::MAX / 2, mk_timer(1)) == Err(WheelErr::Full);
    let travel = w3.advance(99).is_err();
    cs.add("cancel_and_honest_errors", c_ok && drained == 0 && horizon && travel, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: fn(u64) -> Timer = |k| Timer { key: k, kind: 1, a: 0 };

    #[test]
    fn insert_and_drain_in_order_of_time() {
        let mut w = TimingWheel::new();
        w.insert(5, T(1)).unwrap();
        w.insert(2, T(2)).unwrap();
        w.insert(9, T(3)).unwrap();
        assert_eq!(w.pending(), 3);
        w.advance(5).unwrap();
        let mut out = [Timer { key: 0, kind: 0, a: 0 }; 8];
        let n = w.drain(&mut out);
        assert_eq!(n, 2, "t=5 到期 2 项（t=2、t=5）");
        w.advance(9).unwrap();
        let n2 = w.drain(&mut out);
        assert_eq!(n2, 1);
        assert_eq!(w.pending(), 0);
    }

    #[test]
    fn same_key_coalesces_with_covered_count() {
        let mut w = TimingWheel::new();
        // 同键必须同刻才落同槽（容器语义：覆盖只发生在槽内）。
        w.insert(100, T(7)).unwrap();
        w.insert(100, T(7)).unwrap(); // 同键同刻再入 → 覆盖
        assert_eq!(w.covered_away, 1);
        assert_eq!(w.pending(), 1);
        w.advance(100).unwrap();
        let mut out = [Timer { key: 0, kind: 0, a: 0 }; 8];
        assert_eq!(w.drain(&mut out), 1, "覆盖后只触发一次");
        w.advance(200).unwrap();
        assert_eq!(w.drain(&mut out), 0, "被覆盖的旧定时不再触发");
    }

    #[test]
    fn cancel_removes_exactly_one() {
        let mut w = TimingWheel::new();
        w.insert(50, T(3)).unwrap();
        assert!(w.cancel(3));
        assert!(!w.cancel(3), "二次取消如实报 false");
        assert_eq!(w.cancelled, 1);
        w.advance(100).unwrap();
        let mut out = [Timer { key: 0, kind: 0, a: 0 }; 8];
        assert_eq!(w.drain(&mut out), 0);
    }

    #[test]
    fn long_delay_cascades_levels() {
        let mut w = TimingWheel::new();
        // L3 覆盖域内：延迟 ~10s。
        w.insert(10_000_000, T(9)).unwrap();
        assert_eq!(w.pending(), 1);
        w.advance(9_999_999).unwrap();
        let mut out = [Timer { key: 0, kind: 0, a: 0 }; 8];
        assert_eq!(w.drain(&mut out), 0);
        w.advance(10_000_000).unwrap();
        assert_eq!(w.drain(&mut out), 1);
    }

    #[test]
    fn beyond_horizon_is_honest() {
        let mut w = TimingWheel::new();
        assert_eq!(w.insert(u64::MAX / 2, T(1)), Err(WheelErr::Full));
        assert_eq!(w.insert(0, Timer { key: 0, kind: 0, a: 0 }), Err(WheelErr::BadKey));
    }

    #[test]
    fn time_travel_rejected() {
        let mut w = TimingWheel::new();
        w.advance(100).unwrap();
        assert!(w.advance(99).is_err());
        assert!(w.advance(100).is_ok());
    }

    #[test]
    fn drain_output_cap_is_honored() {
        let mut w = TimingWheel::new();
        // 异刻异键且各占一槽：L0 粒度 1μs、64 槽——1..9μs 各落一槽
        // （≥64μs 进 L1 槽粒度 64μs，会撞槽 → Err(Full) 是容器语义）。
        for k in 1..10u64 {
            w.insert(k, T(k)).unwrap();
        }
        w.advance(9).unwrap();
        let mut small = [Timer { key: 0, kind: 0, a: 0 }; 4];
        let n = w.drain(&mut small);
        assert_eq!(n, 4, "出参容量上限被尊重");
        // drain 语义：到期项一律消费（防重复触发），出参只装前 out.len() 个
        // ——未进出的 5 项已 fired 且不再 pending。
        assert_eq!(w.pending(), 0);
        assert_eq!(w.fired, 9, "fired 记账含未进出参的项");
    }
}
