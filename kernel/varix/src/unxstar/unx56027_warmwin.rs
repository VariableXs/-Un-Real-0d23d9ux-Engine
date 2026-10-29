//! UNX-F56027 预热与采样窗控制器（0x60340 段）
//!
//! 锚点：UNX-F56027-J1 预热 10⁴ 次后开窗采 10⁵ 的时序控制（开窗/关窗
//! 时间戳落账）；J2 预热不足（阈值探测）自动延窗 ×3 验证（缺多少补多
//! 少，补满才开）；J3 窗时序纪律（未预热禁开窗、未开窗禁采、未开窗禁
//! 关窗）三判据全绿。判据锚定 §AI-71.2 专题二①。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·采样窗面）。
//!
//! 结构：预热计数账（10⁴ 阈值探测）+ 采样窗状态机（closed→open→full
//! 三态）+ 自动延窗核（预热缺口一次补齐 ×3 场景）+ 开关窗时间戳账 +
//! 采样量账（10⁵ 目标）+ 注入核（预热不足开窗/未开窗采/未开窗关/窗满
//! 再采/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60340 独占 0x20）。
pub const SEG_BASE: u32 = 0x60340;
pub const WC_WARMUP_SHORT: u32 = SEG_BASE + 0x01; // 预热不足开窗
pub const WC_NOT_OPEN: u32 = SEG_BASE + 0x02; // 未开窗即采/关
pub const WC_WINDOW_FULL: u32 = SEG_BASE + 0x03; // 窗满再采
pub const WC_DUP_OPEN: u32 = SEG_BASE + 0x04; // 重复开窗
pub const WC_TS_LOST: u32 = SEG_BASE + 0x05; // 开关窗时间戳未落账
pub const WC_SEALED: u32 = SEG_BASE + 0x06; // 窗账已封账拒写
pub const WC_TS_BACK: u32 = SEG_BASE + 0x07; // 时间戳回退

/// 预热目标（10⁴）与采样目标（10⁵）。
pub const WARMUP_TARGET: u64 = 10_000;
pub const SAMPLE_TARGET: u64 = 100_000;

/// 窗三态。
pub const WS_CLOSED: usize = 0;
pub const WS_OPEN: usize = 1;
pub const WS_FULL: usize = 2;
pub const WS_NAMES: [&str; 3] = ["closed", "open", "full"];

/// 开/关窗事件：时间戳 × 动作。
#[derive(Clone)]
pub struct WinEvent {
    pub ts: u64,
    pub open: bool,
}

impl WinEvent {

    pub fn render(&self) -> String {
        format!(
            "WIN|{}|ts={}",
            if self.open { "OPEN" } else { "CLOSE" },
            self.ts
        )
    }
}

/// 采样窗控制器。
pub struct WindowCtl {
    pub state: usize,
    pub warmup_count: u64,
    pub sample_count: u64,
    pub events: Vec<WinEvent>,
    pub extends: Vec<u64>,
    pub sealed: bool,
}

impl WindowCtl {

    /// 封账印（补：判据与注入核调用点所需）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
    pub fn new() -> Self {
        WindowCtl {
            state: WS_CLOSED,
            warmup_count: 0,
            sample_count: 0,
            events: Vec::new(),
            extends: Vec::new(),
            sealed: false,
        }
    }

    /// 预热计数（只累计不落样本）。
    pub fn warmup(&mut self, times: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(WC_SEALED);
        }
        self.warmup_count += times;
        Ok(())
    }

    /// 预热缺口（不足返回缺多少；已足返回 0）。
    pub fn warmup_gap(&self) -> u64 {
        if self.warmup_count >= WARMUP_TARGET {
            0
        } else {
            WARMUP_TARGET - self.warmup_count
        }
    }

    /// J2：自动延窗——缺口一次补齐，补齐留痕。
    pub fn auto_extend(&mut self) -> u64 {
        let gap = self.warmup_gap();
        if gap > 0 {
            self.warmup_count += gap;
            self.extends.push(gap);
        }
        gap
    }

    /// 开窗：预热足才开；重复开窗拒；时间戳落账。
    pub fn open_window(&mut self, ts: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(WC_SEALED);
        }
        if self.state == WS_OPEN || self.state == WS_FULL {
            return Err(WC_DUP_OPEN);
        }
        if self.warmup_gap() > 0 {
            return Err(WC_WARMUP_SHORT);
        }
        if let Some(prev) = self.events.last() {
            if ts < prev.ts {
                return Err(WC_TS_BACK);
            }
        }
        self.state = WS_OPEN;
        self.events.push(WinEvent { ts, open: true });
        Ok(())
    }

    /// 采样（仅开窗态；满 10⁵ 转窗满）。
    pub fn sample(&mut self, n: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(WC_SEALED);
        }
        if self.state == WS_CLOSED {
            return Err(WC_NOT_OPEN);
        }
        if self.state == WS_FULL {
            return Err(WC_WINDOW_FULL);
        }
        self.sample_count += n;
        if self.sample_count >= SAMPLE_TARGET {
            self.sample_count = SAMPLE_TARGET;
            self.state = WS_FULL;
        }
        Ok(self.sample_count)
    }

    /// 关窗（仅开窗态；时间戳落账且不回退）。
    pub fn close_window(&mut self, ts: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(WC_SEALED);
        }
        if self.state != WS_OPEN {
            return Err(WC_NOT_OPEN);
        }
        if let Some(prev) = self.events.last() {
            if ts < prev.ts {
                return Err(WC_TS_BACK);
            }
        }
        self.state = WS_CLOSED;
        self.events.push(WinEvent { ts, open: false });
        Ok(())
    }

    /// J1：开关窗时间戳账齐（OPEN 行 + CLOSE 行逐条可读）。
    pub fn events_trail_ok(&self) -> bool {
        if self.events.len() < 2 {
            return false;
        }
        self.events.iter().all(|e| e.render().starts_with("WIN|"))
    }
}

/// ×3 自动延窗场景核：三次预热不足均一次补齐。
pub fn extend_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 场景 1：0 次预热 → 缺 10⁴
    let mut w1 = WindowCtl::new();
    let g1 = w1.auto_extend();
    if g1 == WARMUP_TARGET && w1.warmup_gap() == 0 && w1.extends.len() == 1 {
        caught += 1;
    }
    // 场景 2：半程预热（5000）→ 缺 5000
    let mut w2 = WindowCtl::new();
    w2.warmup(5_000).expect("wu");
    let g2 = w2.auto_extend();
    if g2 == 5_000 && w2.warmup_count == WARMUP_TARGET {
        caught += 1;
    }
    // 场景 3：差一次（9999）→ 缺 1
    let mut w3 = WindowCtl::new();
    w3.warmup(WARMUP_TARGET - 1).expect("wu3");
    let g3 = w3.auto_extend();
    if g3 == 1 && w3.warmup_count == WARMUP_TARGET {
        caught += 1;
    }
    (caught, 3)
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 预热不足开窗
    let mut w = WindowCtl::new();
    if w.open_window(100) == Err(WC_WARMUP_SHORT) {
        caught += 1;
    }
    // 2) 未开窗即采
    let mut w2 = WindowCtl::new();
    w2.warmup(WARMUP_TARGET).expect("wu");
    if w2.sample(1) == Err(WC_NOT_OPEN) {
        caught += 1;
    }
    // 3) 未开窗即关
    if w2.close_window(200) == Err(WC_NOT_OPEN) {
        caught += 1;
    }
    // 4) 窗满再采
    let mut w3 = WindowCtl::new();
    w3.warmup(WARMUP_TARGET).expect("wu3");
    w3.open_window(100).expect("open");
    let _ = w3.sample(SAMPLE_TARGET);
    if w3.state == WS_FULL && w3.sample(1) == Err(WC_WINDOW_FULL) {
        caught += 1;
    }
    // 5) 时间戳回退
    let mut w4 = WindowCtl::new();
    w4.warmup(WARMUP_TARGET).expect("wu4");
    w4.open_window(500).expect("open4");
    if w4.close_window(400) == Err(WC_TS_BACK) {
        caught += 1;
    }
    (caught, 5)
}

/// 标准夹具：预热满 → 开窗 → 采满 → 关窗（时间戳 100/900）。
pub fn standard_window() -> WindowCtl {
    let mut w = WindowCtl::new();
    w.warmup(WARMUP_TARGET).expect("wu");
    w.open_window(100).expect("open");
    w.sample(SAMPLE_TARGET).expect("sample");
    w.close_window(900).expect("close");
    w
}

/// 判据入口（CheckSet）。
pub fn run_unx56027_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56027");
    // J1-1 时序控制全链（closed→open→full→closed）
    let w = standard_window();
    let ok = w.state == WS_CLOSED
        && w.sample_count == SAMPLE_TARGET
        && w.events.len() == 2;
    s.add("56027-J1-时序全链", ok, if ok { "" } else { "时序链断" });
    // J1-2 开关窗时间戳账齐（OPEN ts=100 / CLOSE ts=900）
    let ok = w.events_trail_ok()
        && w.events[0].render() == "WIN|OPEN|ts=100"
        && w.events[1].render() == "WIN|CLOSE|ts=900";
    s.add("56027-J1-开关窗时间戳账", ok, if ok { "" } else { "时间戳账缺" });
    // J1-3 口径常量（10⁴ / 10⁵）
    let ok = WARMUP_TARGET == 10_000 && SAMPLE_TARGET == 100_000;
    s.add("56027-J1-口径常量", ok, if ok { "" } else { "口径漂移" });
    // J2-1 ×3 自动延窗场景全过
    let (c, n) = extend_core();
    let ok = c == n && n == 3;
    let detail = if ok { String::new() } else { format!("延窗 {c}/{n}") };
    s.add_dyn("56027-J2-三次自动延窗", ok, &detail);
    // J2-2 缺口探测语义（0 / 半程 / 差一次）
    let mut w2 = WindowCtl::new();
    let g0 = w2.warmup_gap();
    w2.warmup(5_000).expect("wu");
    let g5 = w2.warmup_gap();
    w2.warmup(4_999).expect("wu2");
    let ok = g0 == WARMUP_TARGET && g5 == 5_000 && w2.warmup_gap() == 1;
    s.add("56027-J2-缺口探测", ok, if ok { "" } else { "探测漂移" });
    // J2-3 延窗留痕（extends 账逐条数值）
    let mut w3 = WindowCtl::new();
    w3.warmup(7_000).expect("wu3");
    let g = w3.auto_extend();
    let ok = g == 3_000 && w3.extends == [3_000u64] && w3.auto_extend() == 0
        && w3.extends.len() == 1;
    s.add("56027-J2-延窗留痕", ok, if ok { "" } else { "延窗未留痕" });
    // J3-1 注入核 5 类必抓
    let (ic, in_) = injection_core();
    let ok = ic == in_;
    let detail = if ok { String::new() } else { format!("注入 {ic}/{in_}") };
    s.add_dyn("56027-J3-注入核必抓", ok, &detail);
    // J3-2 窗满转态语义（恰 10⁵ 即满）
    let mut w4 = WindowCtl::new();
    w4.warmup(WARMUP_TARGET).expect("wu4");
    w4.open_window(10).expect("open4");
    let mid = w4.sample(SAMPLE_TARGET - 1).expect("s4");
    let last = w4.sample(1).expect("s5");
    let ok = mid == SAMPLE_TARGET - 1 && last == SAMPLE_TARGET && w4.state == WS_FULL;
    s.add("56027-J3-窗满转态", ok, if ok { "" } else { "窗满语义漂移" });
    // J3-3 重复开窗拒
    let mut w5 = WindowCtl::new();
    w5.warmup(WARMUP_TARGET).expect("wu5");
    w5.open_window(1).expect("open5");
    let ok = w5.open_window(2) == Err(WC_DUP_OPEN);
    s.add("56027-J3-重复开窗拒", ok, if ok { "" } else { "重复开窗穿透" });
    // J1-4 窗三态名闭集
    let ok = WS_NAMES[WS_CLOSED] == "closed" && WS_NAMES[WS_FULL] == "full";
    s.add("56027-J1-三态名同源", ok, if ok { "" } else { "命名漂移" });
    // J2-4 封账冻结
    let mut w6 = standard_window();
    w6.seal();
    let ok = w6.warmup(1) == Err(WC_SEALED)
        && w6.sample(1) == Err(WC_SEALED)
        && w6.open_window(999) == Err(WC_SEALED);
    s.add("56027-J2-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J1-5 预热只累计不落样本（预热后 sample_count 恒零）
    let mut w7 = WindowCtl::new();
    w7.warmup(WARMUP_TARGET).expect("wu7");
    let ok = w7.sample_count == 0 && w7.warmup_count == WARMUP_TARGET;
    s.add("56027-J1-预热不落样本", ok, if ok { "" } else { "预热混样" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56027_all_checks_green() {
        let set = run_unx56027_checks();
        assert!(set.all_passed(), "UNX-F56027 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56027_extend_gap() {
        let mut w = WindowCtl::new();
        w.warmup(2_500).expect("wu");
        assert_eq!(w.auto_extend(), 7_500);
        assert_eq!(w.extends, [7_500u64]);
    }
}
