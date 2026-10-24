//! UNX-F0250 早传第九步：上次引导记录携带（0x3BF0 段）
//!
//! 锚点：UNX-F0250-J1 F0130 记录/F0131 序号/失败计数三件入
//! Manifest，读回一致。
//! 域：CoRun·UNX-A1-B13（上次引导携带面）。
//!
//! 结构：上次引导记录（F0130：结果闭集 + 结果码 + 时长）+ 引导
//! 序号（F0131：本次序号 = 上次 + 1）+ 失败计数（连续失败累计）
//! + 三件入 Manifest + 读回对账核（写入态与读回态逐字段一致）+
//! 失败推进核（失败累计 / 成功清零）+ 注入核（序号倒退 / 结果
//! 越集 / 封账拒写 / 读回漂移必抓）。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;

/// 诊断段（0x3BF0 独占 0x10）。
pub const SEG_BASE: u32 = 0x3BF0;
pub const LB_SEQ_BACK: u32 = SEG_BASE + 0x01; // 序号倒退（本次 ≤ 上次）
pub const LB_RESULT_OOB: u32 = SEG_BASE + 0x02; // 结果越出闭集
pub const LB_SEALED: u32 = SEG_BASE + 0x03; // 记录已封账拒写
pub const LB_READBACK: u32 = SEG_BASE + 0x04; // 读回漂移
pub const LB_FAIL_OOB: u32 = SEG_BASE + 0x05; // 失败计数饱和后仍递增

/// 上次引导结果闭集（F0130 同源：成功/失败/中断/未知）。
pub const RES_OK: u8 = 0;
pub const RES_FAIL: u8 = 1;
pub const RES_ABORT: u8 = 2;
pub const RES_UNKNOWN: u8 = 3;
pub const RESULT_COUNT: usize = 4;

/// 结果名。
pub fn result_name(r: u8) -> &'static str {
    match r {
        RES_OK => "成功",
        RES_FAIL => "失败",
        RES_ABORT => "中断",
        RES_UNKNOWN => "未知",
        _ => "?",
    }
}

/// 上次引导记录三件。
#[derive(Clone, Copy)]
pub struct LastBootRecord {
    pub result: u8,
    pub result_code: u16,
    pub duration_ms: u64,
}

impl LastBootRecord {
    /// 结果闭集守卫。
    pub fn valid(&self) -> bool {
        (self.result as usize) < RESULT_COUNT
    }
}

/// 携带面板：三件聚合（上次记录 / 本次序号 / 连续失败计数），可封账。
pub struct CarryPanel {
    pub last: Option<LastBootRecord>,
    pub last_seq: u32,
    pub this_seq: u32,
    pub fail_streak: u32,
    pub sealed: bool,
}

impl CarryPanel {
    /// 开面板（从持久层读入上次序号，本次序号 = 上次 + 1）。
    pub fn open(last_seq: u32, fail_streak: u32) -> Self {
        CarryPanel {
            last: None,
            last_seq,
            this_seq: last_seq + 1,
            fail_streak,
            sealed: false,
        }
    }

    /// 写上次记录（结果闭集守卫）。
    pub fn set_last(&mut self, r: LastBootRecord) -> Result<(), u32> {
        if self.sealed {
            return Err(LB_SEALED);
        }
        if !r.valid() {
            return Err(LB_RESULT_OOB);
        }
        self.last = Some(r);
        Ok(())
    }

    /// 推进核：本次引导结束，按结果更新失败计数（成功清零 / 失败 +1）。
    pub fn advance(&mut self, this_result: u8) -> Result<u32, u32> {
        if self.sealed {
            return Err(LB_SEALED);
        }
        if (this_result as usize) >= RESULT_COUNT {
            return Err(LB_RESULT_OOB);
        }
        if this_result == RES_FAIL {
            // 饱和守卫：u32::MAX 时拒绝再增（显性失败优于静默回绕）
            if self.fail_streak == u32::MAX {
                return Err(LB_FAIL_OOB);
            }
            self.fail_streak += 1;
        } else if this_result == RES_OK {
            self.fail_streak = 0;
        }
        // 中断/未知不改计数（只改序号）
        self.last_seq = self.this_seq;
        self.this_seq = self.last_seq + 1;
        Ok(self.fail_streak)
    }

    /// 序号单调核：本次 > 上次恒成立。
    pub fn seq_forward(&self) -> bool {
        self.this_seq > self.last_seq
    }

    /// 三件齐备核。
    pub fn complete(&self) -> bool {
        self.last.is_some() && self.seq_forward()
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// Manifest 渲染。
    pub fn render(&self) -> String {
        let mut out = format!(
            "上次引导携带 序号上次={} 本次={} 失败计数={}\n",
            self.last_seq, self.this_seq, self.fail_streak
        );
        match self.last {
            Some(r) => {
                let line = format!(
                    "  上次记录 结果={}({}) 码=0x{:04x} 时长={}ms\n",
                    r.result,
                    result_name(r.result),
                    r.result_code,
                    r.duration_ms
                );
                out.push_str(&line);
            }
            None => out.push_str("  上次记录 无（首启）\n"),
        }
        out
    }

    /// 序列化快照（五元组，读回对账用）。
    pub fn snapshot(&self) -> (u8, u16, u64, u32, u32) {
        match self.last {
            Some(r) => (r.result, r.result_code, r.duration_ms, self.last_seq, self.fail_streak),
            None => (RES_UNKNOWN, 0, 0, self.last_seq, self.fail_streak),
        }
    }
}

/// 读回对账核：面板态与持久层快照逐字段一致（漂移返回 Err）。
pub fn readback_reconcile(panel: &CarryPanel, snap: (u8, u16, u64, u32, u32)) -> Result<(), u32> {
    if panel.snapshot() != snap {
        return Err(LB_READBACK);
    }
    Ok(())
}

/// 标准夹具：上次成功 + 序号 42 + 失败计数 0。
pub fn standard_panel() -> CarryPanel {
    let mut p = CarryPanel::open(41, 0);
    let _ = p.set_last(LastBootRecord {
        result: RES_OK,
        result_code: 0,
        duration_ms: 3_250,
    });
    p
}

/// 失败推进核（场景账）：失败累计 → 成功清零 → 中断不改 → 序号单调。
pub fn advance_scenarios() -> bool {
    // 场景一：连败三次累计 3
    let mut p = CarryPanel::open(9, 0);
    let _ = p.set_last(LastBootRecord { result: RES_FAIL, result_code: 0x11, duration_ms: 800 });
    let _ = p.advance(RES_FAIL);
    let _ = p.advance(RES_FAIL);
    let r3 = p.advance(RES_FAIL);
    if r3 != Ok(3) || !p.seq_forward() {
        return false;
    }
    // 场景二：成功清零
    if p.advance(RES_OK) != Ok(0) {
        return false;
    }
    // 场景三：中断不改计数
    if p.advance(RES_ABORT) != Ok(0) {
        return false;
    }
    // 场景四：未知不改计数
    p.advance(RES_UNKNOWN) == Ok(0)
}

/// 注入核：7 注入必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 结果越集拒
    let mut p = CarryPanel::open(1, 0);
    if p.set_last(LastBootRecord { result: 9, result_code: 0, duration_ms: 0 }) == Err(LB_RESULT_OOB)
    {
        caught += 1;
    }
    // 2) 推进结果越集拒
    if p.advance(7) == Err(LB_RESULT_OOB) {
        caught += 1;
    }
    // 3) 封账拒写
    p.seal();
    if p.set_last(LastBootRecord { result: RES_OK, result_code: 0, duration_ms: 1 }) == Err(LB_SEALED)
    {
        caught += 1;
    }
    if p.advance(RES_OK) == Err(LB_SEALED) {
        caught += 1;
    }
    // 4) 读回漂移必抓（快照被篡改一位）
    let q = standard_panel();
    let mut snap = q.snapshot();
    snap.3 += 1; // 篡改序号
    if readback_reconcile(&standard_panel(), snap) == Err(LB_READBACK) {
        caught += 1;
    }
    // 5) 干净读回绿（双向实证）
    let q = standard_panel();
    if readback_reconcile(&q, q.snapshot()).is_ok() {
        caught += 1;
    }
    // 6) 失败饱和显性拒
    let mut p2 = CarryPanel::open(0, u32::MAX);
    if p2.advance(RES_FAIL) == Err(LB_FAIL_OOB) {
        caught += 1;
    }
    // 7) 序号单调恒立（推进多轮后仍 forward）
    let mut p3 = CarryPanel::open(0, 0);
    let _ = p3.advance(RES_FAIL);
    let _ = p3.advance(RES_OK);
    let _ = p3.advance(RES_FAIL);
    if p3.seq_forward() {
        caught += 1;
    }
    (caught, 7)
}

/// 判据入口（CheckSet）。
pub fn run_unx250_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0250");
    // J1-1 三件入 Manifest
    let p = standard_panel();
    let ok = p.complete() && !p.render().is_empty();
    s.add("B13-20-三件入面板", ok, if ok { "" } else { "三件缺一" });
    // J1-2 读回一致
    let q = standard_panel();
    let ok = readback_reconcile(&q, q.snapshot()).is_ok();
    s.add("B13-21-读回一致", ok, if ok { "" } else { "读回漂移" });
    // J1-3 失败推进场景账
    let ok = advance_scenarios();
    s.add("B13-22-失败推进账", ok, if ok { "" } else { "推进语义漂移" });
    // J1-4 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B13-23-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx250_all_checks_green() {
        let set = run_unx250_checks();
        assert!(set.all_passed(), "UNX-F0250 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx250_seq_always_forward() {
        let mut p = CarryPanel::open(10, 2);
        let _ = p.advance(RES_FAIL);
        assert!(p.this_seq == 12 && p.fail_streak == 3);
    }
}
