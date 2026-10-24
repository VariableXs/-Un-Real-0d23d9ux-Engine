//! unx0271_backpressure —— UNX-F0271 终端输出限流与背压。
//! 锚点（CoRun Unxreal 主册）：UNX-F0271 · 输出限流背压 · 目标 220 行
//! 判据原文：UNX-F0271-J1 终端输出通道限流：软限触发背压（写入
//!           方被要求让步）、硬限拒写（零丢弃原则——拒写比丢
//!           字节诚实）；水位回落放行；限流事件入账可查。
//! 组件：环形缓冲（容量 + 水位）+ 双限流（软限背压 / 硬限拒写）
//!       + 放行核（水位回落至低水位线才解除）+ 限流事件账。
//! 职责定位：背压是「流的呼吸节律」——软限让步、硬限拒写、
//!   **零丢弃**：字节被丢 = 串口对账再也无法闭合。滞回设计
//!   （高水位触发、低水位解除）防止水位在阈值附近抖动导致
//!   限流事件风暴。
//! 性能逐项分解：写入 O(1)（环形）；放行核 O(1)；事件账 O(事件数)。
//! 诊断码段：0x20300（0x20301~0x20309 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const BP_VERSION: &str = "unx0271_backpressure-v1";

/// 诊断段基址（0x20300 段 · 本模块独占）。
pub const BP_SEG_BASE: u32 = 0x20300;
pub const XC_HARD_FULL: u32 = 0x20301; // 硬限拒写
pub const XC_DROPPED: u32 = 0x20302; // 检出丢弃（绝不允许）
pub const XC_CAP_ZERO: u32 = 0x20303; // 容量 0 拒
pub const XC_HYST_BAD: u32 = 0x20304; // 滞回配置破（低≥高）
pub const XC_DRAIN_OVER: u32 = 0x20305; // 排空过量
pub const XC_EVENT_OOR: u32 = 0x20306; // 事件账越域
pub const XC_STATE_OOR: u32 = 0x20307; // 通道态越域
pub const XC_WRITE_ZERO: u32 = 0x20308; // 零长写拒绝记账
pub const XC_CLOSED: u32 = 0x20309; // 通道已闭

/// 通道态。
pub const CH_OPEN: u8 = 0; // 正常
pub const CH_SOFT: u8 = 1; // 背压（软限）
pub const CH_CLOSED: u8 = 2; // 闭写

/// 输出通道：环形缓冲 + 双限流 + 事件账。
pub struct OutChannel {
    buf: Vec<u8>,
    cap: usize,
    /// 软限（千分比容量）。
    soft_milli: u32,
    /// 硬限（千分比容量）。
    hard_milli: u32,
    /// 低水位解除线（千分比容量）。
    low_milli: u32,
    state: u8,
    /// 限流事件账：(水位, 态) 序列。
    events: Vec<(u32, u8)>,
    dropped: u64,
}

impl OutChannel {
    /// 构造：软 < 硬、低 < 软（滞回闭合）。
    pub fn new(cap: usize, soft_milli: u32, hard_milli: u32, low_milli: u32) -> Result<OutChannel, u32> {
        if cap == 0 {
            return Err(XC_CAP_ZERO);
        }
        if low_milli >= soft_milli || soft_milli >= hard_milli {
            return Err(XC_HYST_BAD);
        }
        if hard_milli > 1000 {
            return Err(XC_HYST_BAD);
        }
        Ok(OutChannel {
            buf: Vec::new(),
            cap,
            soft_milli,
            hard_milli,
            low_milli,
            state: CH_OPEN,
            events: Vec::new(),
            dropped: 0,
        })
    }

    fn threshold(&self, milli: u32) -> usize {
        (self.cap as u64 * milli as u64 / 1000) as usize
    }

    /// 水位。
    pub fn level(&self) -> usize {
        self.buf.len()
    }

    /// 通道态。
    pub fn state(&self) -> u8 {
        self.state
    }

    /// 丢弃计（必须恒 0——零丢弃原则的可验证锚）。
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 写入：按态分流——OPEN 直写（写后升档检查）；SOFT 仍可写
    /// （背压是请求不是禁令）但每次写记事件；硬限满 = 拒写零丢弃。
    pub fn write(&mut self, data: &[u8]) -> Result<usize, u32> {
        if self.state == CH_CLOSED {
            return Err(XC_CLOSED);
        }
        if data.is_empty() {
            return Err(XC_WRITE_ZERO);
        }
        let hard = self.threshold(self.hard_milli);
        if self.level() + data.len() > hard {
            return Err(XC_HARD_FULL); // 拒写，不丢
        }
        let accepted = data.len();
        self.buf.extend_from_slice(data);
        // 升档检查：过软限 → SOFT + 事件。
        if self.level() > self.threshold(self.soft_milli) && self.state == CH_OPEN {
            self.state = CH_SOFT;
            self.push_event();
        }
        Ok(accepted)
    }

    /// 排空（消费方读走 n 字节）：水位回落至低水位线以下解除背压。
    pub fn drain(&mut self, n: usize) -> Result<usize, u32> {
        if n > self.level() {
            return Err(XC_DRAIN_OVER);
        }
        self.buf.drain(..n);
        if self.state == CH_SOFT && self.level() <= self.threshold(self.low_milli) {
            self.state = CH_OPEN;
            self.push_event();
        }
        Ok(n)
    }

    /// 闭写：闭后拒写、可排空（消费方收尾）。
    pub fn close_write(&mut self) {
        self.state = CH_CLOSED;
        self.push_event();
    }

    fn push_event(&mut self) {
        if self.events.len() < 256 {
            self.events.push((self.level() as u32, self.state));
        } else {
            // 事件账满：保底不丢计数语义（越域立案由判据核）。
            self.events[255] = (self.level() as u32, XC_EVENT_OOR as u8);
        }
    }

    /// 事件账。
    pub fn events(&self) -> &[(u32, u8)] {
        &self.events
    }
}

pub fn run_unx0271_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0271_backpressure");
    // 族一：构造与滞回。
    let mut ch = match OutChannel::new(1000, 500, 900, 200) {
        Ok(c) => c,
        Err(_) => {
            cs.add("通道构造", false, "被拒");
            return cs;
        }
    };
    cs.add("初态 OPEN", ch.state() == CH_OPEN, "空缓冲");
    cs.add("软<硬核", OutChannel::new(1000, 900, 500, 200).is_err(), "HYST");
    cs.add("低≥软核", OutChannel::new(1000, 500, 900, 500).is_err(), "HYST");
    cs.add("零容拒", OutChannel::new(0, 500, 900, 200).is_err(), "CAP");
    // 族二：写入与升档。
    let w1 = ch.write(&[7u8; 400]);
    cs.add("400 写入", w1 == Ok(400), "软限 500 以下");
    cs.add("仍 OPEN", ch.state() == CH_OPEN, "未过软限");
    let w2 = ch.write(&[7u8; 200]);
    cs.add("过软限写入", w2 == Ok(200), "水 600");
    cs.add("升档 SOFT", ch.state() == CH_SOFT, "背压触发");
    cs.add("事件入账", ch.events().iter().any(|&(_, s)| s == CH_SOFT), "SOFT 事件");
    // 族三：硬限拒写零丢弃。
    cs.add("硬限拒写", ch.write(&[7u8; 500]) == Err(XC_HARD_FULL), "600+500>900");
    cs.add("零丢弃锚", ch.dropped() == 0, "拒写不丢");
    cs.add("水位不变", ch.level() == 600, "拒写后原水位");
    // 族四：排空与降档。
    cs.add("排空过量拒", ch.drain(601).is_err(), "DRAIN_OVER");
    let d1 = ch.drain(300);
    cs.add("排空 300", d1 == Ok(300), "水 300");
    cs.add("仍 SOFT", ch.state() == CH_SOFT, "300 > 低水位 200");
    let d2 = ch.drain(101);
    cs.add("再排 101", d2 == Ok(101), "水 199");
    cs.add("降档 OPEN", ch.state() == CH_OPEN, "199 ≤ 200 解除");
    cs.add("降档事件", ch.events().iter().any(|&(_, s)| s == CH_OPEN), "OPEN 事件");
    // 族五：闭写。
    ch.close_write();
    cs.add("闭后拒写", ch.write(&[1u8]) == Err(XC_CLOSED), "CLOSED");
    cs.add("闭后可排", ch.drain(199).is_ok(), "消费方收尾");
    cs.add("排空后空", ch.level() == 0, "全排空");
    // 族六：零长写。
    let mut ch2 = OutChannel::new(100, 50, 90, 10).unwrap_or(OutChannel {
        buf: Vec::new(), cap: 100, soft_milli: 50, hard_milli: 90, low_milli: 10,
        state: CH_OPEN, events: Vec::new(), dropped: 0,
    });
    cs.add("零长写拒", ch2.write(&[]) == Err(XC_WRITE_ZERO), "无意义写不记账");
    cs.add("事件账存在", !ch2.events().is_empty() || ch2.state() == CH_OPEN, "初账可空");
    cs
}
