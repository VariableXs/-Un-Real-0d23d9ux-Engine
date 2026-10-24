//! UNX-F0139 · 早年看门狗与 A2 中断交接 · 诊断段 0x2520
//!
//! 判据原文（UNX-F0139-J1）：中断面就绪后看门狗由轮询态迁中断态，
//! 迁移零窗口漏检（接口冻结随闸门补测）。
//!
//! 零窗口漏检机制：迁移瞬间「轮询账 + 中断账」合并视图必须无缝——
//! 交接帧（HandoffFrame）在迁移原子点同时冻结轮询侧末次喂入时刻与
//! 中断侧首次武装时刻，两时刻之间不允许存在「两边都不管」的空窗，
//! 即中断侧武装时刻 ≤ 轮询侧末次喂入 + 窗口宽（覆盖衔接）。
//! 零浮点、零 unwrap/expect。

use crate::alloc::{format, string::String};

pub const SEG_BASE: u16 = 0x2520;

const E_STATE: u16 = SEG_BASE + 1; // 状态机非法迁移
const E_GAP: u16 = SEG_BASE + 2; // 交接空窗（零窗口漏检违约）
const E_BAL: u16 = SEG_BASE + 3; // 喂入账守恒被破坏
const E_SEAL: u16 = SEG_BASE + 4; // 册已冻结拒写
const E_TWICE: u16 = SEG_BASE + 5; // 重复交接

/// 看门狗运行态（闭集三态）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WdState {
    /// 轮询态：主循环每次迭代主动喂。
    Polling,
    /// 交接中：中断面已就绪，冻结帧待原子切换。
    HandingOff,
    /// 中断态：NMI/定时器中断喂。
    Interrupt,
}

/// 交接帧：原子点的两侧时刻锚。
#[derive(Clone, Copy)]
pub struct HandoffFrame {
    pub poll_last_feed_ms: u64, // 轮询侧末次喂入
    pub intr_arm_ms: u64, // 中断侧首次武装
    pub window_ms: u64, // 窗口宽
}

impl HandoffFrame {
    /// 零窗口校验：中断侧武装必须落在「末次喂入 + 窗口」之内，
    /// 否则存在喂入责任真空。
    pub fn gap_free(&self) -> bool {
        self.intr_arm_ms <= self.poll_last_feed_ms + self.window_ms
    }
}

/// 早年看门狗。
pub struct EarlyWatchdog {
    pub state: WdState,
    pub window_ms: u64,
    pub poll_last_feed_ms: u64,
    pub poll_feeds: u64,
    pub intr_feeds: u64,
    pub total_feeds: u64,
    pub frame: Option<HandoffFrame>,
    pub frozen: bool,
}

impl EarlyWatchdog {
    pub fn new(window_ms: u64) -> Result<Self, u16> {
        if window_ms == 0 {
            return Err(E_BAL); // 零窗宽无意义
        }
        Ok(EarlyWatchdog {
            state: WdState::Polling,
            window_ms,
            poll_last_feed_ms: 0,
            poll_feeds: 0,
            intr_feeds: 0,
            total_feeds: 0,
            frame: None,
            frozen: false,
        })
    }

    /// 轮询喂入。
    pub fn poll_feed(&mut self, now_ms: u64) -> Result<(), u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        if self.state != WdState::Polling {
            return Err(E_STATE);
        }
        self.poll_feeds += 1;
        self.total_feeds += 1;
        self.poll_last_feed_ms = now_ms;
        Ok(())
    }

    /// 中断喂入。
    pub fn intr_feed(&mut self, _now_ms: u64) -> Result<(), u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        if self.state != WdState::Interrupt {
            return Err(E_STATE);
        }
        self.intr_feeds += 1;
        self.total_feeds += 1;
        Ok(())
    }

    /// 交接：中断面就绪后由轮询态迁中断态（原子点）。
    ///
    /// 零窗口判据：帧构造即校验 gap_free，违约拒迁（E_GAP）。
    pub fn handoff(&mut self, intr_arm_ms: u64) -> Result<(), u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        if self.state != WdState::Polling {
            return Err(E_TWICE); // 二次交接拒
        }
        let fr = HandoffFrame {
            poll_last_feed_ms: self.poll_last_feed_ms,
            intr_arm_ms,
            window_ms: self.window_ms,
        };
        if !fr.gap_free() {
            return Err(E_GAP);
        }
        self.frame = Some(fr);
        self.state = WdState::Interrupt;
        Ok(())
    }

    /// 喂入账守恒：轮询 + 中断 == 总喂。
    pub fn audit(&self) -> Result<(), u16> {
        if self.poll_feeds + self.intr_feeds != self.total_feeds {
            return Err(E_BAL);
        }
        match self.state {
            WdState::Interrupt => {
                if self.frame.is_none() {
                    return Err(E_BAL); // 中断态无交接帧——账面矛盾
                }
            }
            WdState::Polling => {
                if self.intr_feeds > 0 {
                    return Err(E_BAL); // 未交接却有中断喂——矛盾
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 渲染看门狗速览。
    pub fn render(&self) -> String {
        let st = match self.state {
            WdState::Polling => "轮询",
            WdState::HandingOff => "交接中",
            WdState::Interrupt => "中断",
        };
        format!(
            "早年看门狗 态:{} 窗{}ms 轮喂{} 断喂{} 总喂{}",
            st, self.window_ms, self.poll_feeds, self.intr_feeds, self.total_feeds
        )
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：轮询喂三次 → 中断面就绪 → 零窗交接 → 中断喂接管。
    let mut w = EarlyWatchdog::new(100)?;
    w.poll_feed(0)?;
    w.poll_feed(60)?;
    w.poll_feed(120)?;
    w.handoff(180)?; // 180 ≤ 120+100 ✓ 零空窗
    if w.state != WdState::Interrupt {
        return Err(E_STATE);
    }
    w.intr_feed(250)?;
    w.audit()?;
    hits += 1;

    // L2：贴窗交接（恰在窗口边界）仍无缝。
    let mut w = EarlyWatchdog::new(100)?;
    w.poll_feed(1000)?;
    w.handoff(1100)?; // == 1000+100 边界衔接
    w.audit()?;
    hits += 1;

    // L3：渲染含态与喂账。
    let w = EarlyWatchdog::new(50)?;
    let _ = w.render();
    hits += 1;

    // L4：零窗宽构造即拒（E_BAL）。
    if !matches!(EarlyWatchdog::new(0), Err(E_BAL)) {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：空窗交接拦截——中断侧武装太晚（末喂 + 窗 + 1）。
    let mut i1 = EarlyWatchdog::new(100)?;
    i1.poll_feed(0)?;
    if i1.handoff(101) != Err(E_GAP) {
        return Err(E_GAP);
    }
    // 拒迁后仍在轮询态可重试（帧未写入）。
    i1.poll_feed(90)?;
    i1.handoff(190)?;
    i1.audit()?;
    hits += 1;

    // I2：二次交接拦截。
    let mut i2 = EarlyWatchdog::new(100)?;
    i2.poll_feed(0)?;
    i2.handoff(50)?;
    if i2.handoff(80) != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    hits += 1;

    // I3：轮询态喂中断 / 中断态喂轮询 —— 双向 E_STATE。
    let mut i3 = EarlyWatchdog::new(100)?;
    if i3.intr_feed(1) != Err(E_STATE) {
        return Err(E_STATE);
    }
    i3.poll_feed(10)?;
    i3.handoff(50)?;
    if i3.poll_feed(60) != Err(E_STATE) {
        return Err(E_STATE);
    }
    hits += 1;

    // I4：冻结拒写。
    let mut i4 = EarlyWatchdog::new(100)?;
    i4.freeze();
    if i4.poll_feed(0) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    if i4.handoff(10) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    hits += 1;

    // I5：账守恒注入——中断态却轮询账被清（喂入合计破）。
    let mut i5 = EarlyWatchdog::new(100)?;
    i5.poll_feed(0)?;
    i5.handoff(50)?;
    i5.intr_feed(60)?;
    i5.poll_feeds = 0; // 破守恒：0 + 1 ≠ 总喂 2
    if i5.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
