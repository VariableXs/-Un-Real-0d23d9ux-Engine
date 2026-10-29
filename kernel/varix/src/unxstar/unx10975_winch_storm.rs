//! UNX-F10975 SIGWINCH 风暴联测（连续 resize 不崩不丢终态）（0x1d960 段）
//!
//! 卡片判据（UNX-F10975-J1）：
//! - 千次连续 resize 终态正确（10/10），零崩溃零挂起（10/10）。
//!
//! 域：CoRun·UNX-C4-B29。本模块建模 SIGWINCH 风暴合并语义：
//! 千次连续 resize 只保留最新尺寸（信号可合并），应用每次唤醒
//! 以最新尺寸重绘一次；终态尺寸逐位等于最后一次设置；状态机
//! 有界（零挂起）、域校验全覆盖（零崩溃）。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10975-10900)*0x20 = 0x1d960）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d960;

/// WF_EINVAL：尺寸非法
pub const WF_EINVAL: u32 = SEG_BASE + 0x01;
/// WF_EBADF：fd 无效
pub const WF_EBADF: u32 = SEG_BASE + 0x02;
/// WF_ESTATE：状态机错乱（自检面）
pub const WF_ESTATE: u32 = SEG_BASE + 0x03;
/// WF_EHANG：单轮处理超界（挂起面，自检面）
pub const WF_EHANG: u32 = SEG_BASE + 0x04;
/// WF_EDRIFT：终态漂移（自检面）
pub const WF_EDRIFT: u32 = SEG_BASE + 0x05;
/// WF_ECOAL：合并账不平（自检面）
pub const WF_ECOAL: u32 = SEG_BASE + 0x06;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// 行上界
const ROW_MAX: u32 = 200;
/// 列上界
const COL_MAX: u32 = 500;
/// 单次唤醒最大重绘拍数（防挂起界）
const WAKE_BUDGET: u32 = 4;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 窗口尺寸
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StormSize {
    pub rows: u32,
    pub cols: u32,
}

impl StormSize {
    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.rows as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.cols as u64).wrapping_mul(0x100000001b3);
        h
    }

    pub fn valid(&self) -> bool {
        self.rows > 0 && self.cols > 0 && self.rows <= ROW_MAX && self.cols <= COL_MAX
    }
}

/// 风暴联测台
#[derive(Clone, Debug)]
pub struct WinchStorm {
    /// 最新生效尺寸
    pub latest: StormSize,
    /// 挂起信号数（可合并）
    pub pending: u32,
    /// fd 存活
    pub fd_alive: bool,
    /// resize 总次数
    pub n_resize: u64,
    /// 重绘次数（合并后）
    pub n_redraw: u64,
    /// 终态尺寸（最后一次重绘所用）
    pub final_size: Option<StormSize>,
    /// 错误计数
    pub n_einval: u32,
    pub n_ebadf: u32,
}

impl WinchStorm {
    pub fn new() -> Self {
        WinchStorm {
            latest: StormSize { rows: 24, cols: 80 },
            pending: 0,
            fd_alive: true,
            n_resize: 0,
            n_redraw: 0,
            final_size: None,
            n_einval: 0,
            n_ebadf: 0,
        }
    }

    /// resize：更新最新尺寸并置挂起信号（风暴合并）
    pub fn resize(&mut self, sz: StormSize) -> Result<(), u32> {
        if !self.fd_alive {
            self.n_ebadf = self.n_ebadf.wrapping_add(1);
            return Err(WF_EBADF);
        }
        if !sz.valid() {
            self.n_einval = self.n_einval.wrapping_add(1);
            return Err(WF_EINVAL);
        }
        self.latest = sz;
        self.pending = self.pending.wrapping_add(1);
        self.n_resize = self.n_resize.wrapping_add(1);
        Ok(())
    }

    /// 应用唤醒：一次重绘消化全部挂起信号（有界拍数内）
    pub fn wake(&mut self) -> Result<StormSize, u32> {
        if !self.fd_alive {
            self.n_ebadf = self.n_ebadf.wrapping_add(1);
            return Err(WF_EBADF);
        }
        if self.pending == 0 {
            return Err(WF_ESTATE);
        }
        if self.pending > WAKE_BUDGET as u32 * 1000 {
            return Err(WF_EHANG);
        }
        // 合并：一次重绘用最新尺寸，清空挂起
        let used = self.latest;
        self.pending = 0;
        self.n_redraw = self.n_redraw.wrapping_add(1);
        self.final_size = Some(used);
        Ok(used)
    }

    /// 终态正确：最后一次 resize 的尺寸 == 终态尺寸
    pub fn final_ok(&self, expect: StormSize) -> bool {
        self.final_size == Some(expect) && self.latest == expect
    }

    /// 零挂起：pending 清空且重绘次数有界
    pub fn no_hang(&self) -> bool {
        self.pending == 0 && self.n_redraw <= self.n_resize
    }

    /// 合并账：resize 次数 >= 重绘次数（风暴合并语义）
    pub fn coalesce_ok(&self) -> bool {
        self.n_resize >= self.n_redraw
    }

    /// 账目对平
    pub fn ledger_ok(&self, expect_resize: u64) -> bool {
        self.n_resize == expect_resize && self.no_hang() && self.coalesce_ok()
    }

    /// 总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h = self.latest.fold();
        h = (h ^ self.n_resize).wrapping_mul(0x100000001b3);
        h = (h ^ self.n_redraw).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染
    pub fn render(&self) -> String {
        crate::alloc::format!(
            "storm:{}x{} resize={} redraw={} pending={}",
            self.latest.rows,
            self.latest.cols,
            self.n_resize,
            self.n_redraw,
            self.pending
        )
    }
}

impl Default for WinchStorm {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    let mut d = WinchStorm::new();
    // 路1：零尺寸 → EINVAL
    let r1 = matches!(d.resize(StormSize { rows: 0, cols: 80 }), Err(x) if x == WF_EINVAL);
    // 路2：超界尺寸 → EINVAL
    let r2 = matches!(d.resize(StormSize { rows: ROW_MAX + 1, cols: 80 }), Err(x) if x == WF_EINVAL);
    // 路3：无挂起信号唤醒 → ESTATE
    let r3 = matches!(d.wake(), Err(x) if x == WF_ESTATE);
    // 路4：坏 fd → EBADF
    d.fd_alive = false;
    let r4 = matches!(d.resize(StormSize { rows: 30, cols: 90 }), Err(x) if x == WF_EBADF);
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：千次连续 resize 风暴 + 周期性唤醒；终态正确记 1。
pub fn sample_thousand() -> u32 {
    let mut d = WinchStorm::new();
    let mut last = StormSize { rows: 24, cols: 80 };
    for r in 0..1000u32 {
        let sz = StormSize {
            rows: (r % ROW_MAX) + 1,
            cols: (r % 400) + 50,
        };
        if d.resize(sz).is_err() {
            return 0;
        }
        last = sz;
        // 每 25 轮唤醒一次（风暴合并重绘）
        if r % 25 == 24 {
            if d.wake() != Ok(last) {
                return 0;
            }
        }
    }
    // 收尾唤醒消化终态（末尾恰无挂起信号时跳过，避免空唤醒 ESTATE）
    if d.pending > 0 && d.wake() != Ok(last) {
        return 0;
    }
    if d.final_ok(last) && d.no_hang() && d.coalesce_ok() && d.ledger_ok(1000) {
        1
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10975_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10975");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "EINVAL/ESTATE/EBADF 坏样本全部命中");

    // J1 千次 resize 终态正确 10/10
    let mut ok10 = 0u32;
    for seed in 0..10u32 {
        let mut d = WinchStorm::new();
        let mut last = StormSize { rows: 24, cols: 80 };
        for r in 0..1000u32 {
            let sz = StormSize {
                rows: ((r + seed) % ROW_MAX) + 1,
                cols: ((r * 7 + seed) % 400) + 50,
            };
            let _ = d.resize(sz);
            last = sz;
        }
        let _ = d.wake();
        if d.final_ok(last) && d.ledger_ok(1000) {
            ok10 = ok10.wrapping_add(1);
        }
    }
    s.add(
        "J1-千次resize终态正确10/10",
        ok10 == 10,
        "十组千次风暴终态逐位等于最后设置",
    );

    // 零崩溃零挂起 10/10
    let mut hang10 = 0u32;
    for _ in 0..10 {
        let mut d = WinchStorm::new();
        for r in 0..1000u32 {
            let _ = d.resize(StormSize { rows: (r % ROW_MAX) + 1, cols: 80 });
        }
        let _ = d.wake();
        if d.no_hang() && d.pending == 0 && d.final_size.is_some() {
            hang10 = hang10.wrapping_add(1);
        }
    }
    s.add("J2-零崩溃零挂起10/10", hang10 == 10, "千次风暴后信号清空状态有界");

    // 千轮样本
    let good = sample_thousand();
    s.add("J3-千轮风暴样本", good == 1, "千轮风暴终态校验通过");

    // 合并语义：40 次 resize 只需 1 次重绘消化
    let mut d2 = WinchStorm::new();
    for r in 0..40u32 {
        let _ = d2.resize(StormSize { rows: 30 + r, cols: 100 });
    }
    let merged = d2.wake() == Ok(StormSize { rows: 69, cols: 100 })
        && d2.pending == 0
        && d2.n_redraw == 1
        && d2.n_resize == 40;
    s.add("J4-风暴合并", merged, "40 次 resize 合并为一次重绘");

    // 错误码分段
    let codes = [WF_EINVAL, WF_EBADF, WF_ESTATE, WF_EHANG, WF_EDRIFT, WF_ECOAL];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J5-错误码分段", seg_ok, "六码全部落在 0x1d960 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J6-错误码互异", distinct, "六码两两互异");

    // 账目对平
    s.add("J7-账目对平", d2.ledger_ok(40), "resize=40 且合并账自洽");

    // 折叠随尺寸变化
    let h1 = d2.fold_all();
    let _ = d2.resize(StormSize { rows: 100, cols: 200 });
    let h2 = d2.fold_all();
    s.add("J8-折叠随尺寸", h1 != h2, "新尺寸必改折叠摘要");

    // 渲染
    let r = d2.render();
    s.add(
        "J9-渲染非空",
        r.starts_with("storm:") && r.contains("pending=1"),
        "摘要含尺寸与挂起账",
    );

    // 尺寸域边界
    let edge_ok = StormSize { rows: ROW_MAX, cols: COL_MAX }.valid()
        && !StormSize { rows: 1, cols: COL_MAX + 1 }.valid();
    s.add("J10-尺寸域边界", edge_ok, "上界内有效、越列无效");

    // 判定序：EBADF 先于 EINVAL
    let mut d3 = WinchStorm::new();
    d3.fd_alive = false;
    let seq_ok = matches!(d3.resize(StormSize { rows: 0, cols: 0 }), Err(x) if x == WF_EBADF);
    s.add("J11-判定序", seq_ok, "失效 fd 先于尺寸校验");

    // 动态判据
    s.add_dyn(
        "J12-风暴账",
        d2.n_resize == 41 && d2.pending == 1,
        &crate::alloc::format!("resize={} pending={}", d2.n_resize, d2.pending),
    );
    s.add_dyn(
        "J13-千轮通过率",
        good == 1,
        &crate::alloc::format!("pass={}", good),
    );
    s.add_dyn(
        "J14-重绘账",
        d2.n_redraw == 1,
        &crate::alloc::format!("redraw={}", d2.n_redraw),
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10975_winch_storm_flow() {
        let mut d = WinchStorm::new();
        for r in 0..100u32 {
            let _ = d.resize(StormSize { rows: (r % ROW_MAX) + 1, cols: 80 + r });
        }
        assert!(matches!(d.wake(), Ok(_)));
        assert!(matches!(d.wake(), Err(x) if x == WF_ESTATE));
        assert!(matches!(d.resize(StormSize { rows: 0, cols: 1 }), Err(x) if x == WF_EINVAL));
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1);
    }

    #[test]
    fn unx10975_checks_green() {
        let s = run_unx10975_checks();
        assert!(s.all_passed(), "UNX-F10975 判据未全绿");
    }
}
