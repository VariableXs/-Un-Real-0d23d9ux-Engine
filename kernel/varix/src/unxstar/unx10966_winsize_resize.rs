//! UNX-F10966 窗口尺寸变更联测（TIOCSWINSZ→SIGWINCH→重绘）（0x1d840 段）
//!
//! 卡片判据（UNX-F10966-J1）：
//! - 尺寸变更三拍全通（10/10），重绘用新尺寸（10/10）。
//!
//! 域：CoRun·UNX-C4-B29。本模块建模窗口尺寸变更三拍链：
//! TIOCSWINSZ 设置新尺寸 → SIGWINCH 置位 → 应用捕获后按新
//! 尺寸重绘。三拍逐拍记账，重绘帧尺寸必须等于最新生效尺寸。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10966-10900)*0x20 = 0x1d840）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d840;

/// WS_EBADF：fd 无效
pub const WS_EBADF: u32 = SEG_BASE + 0x01;
/// WS_EINVAL：尺寸非法（零行/零列/超界）
pub const WS_EINVAL: u32 = SEG_BASE + 0x02;
/// WS_ENOSIG：SIGWINCH 未置位即重绘（自检面）
pub const WS_ENOSIG: u32 = SEG_BASE + 0x03;
/// WS_ESEQ：三拍顺序错乱（自检面）
pub const WS_ESEQ: u32 = SEG_BASE + 0x04;
/// WS_EIO：终端失效
pub const WS_EIO: u32 = SEG_BASE + 0x05;
/// WS_EDRIFT：重绘尺寸漂移（自检面）
pub const WS_EDRIFT: u32 = SEG_BASE + 0x06;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// SIGWINCH 信号号（建模值）
pub const SIGWINCH: u32 = 28;
/// 行数上界
const ROW_MAX: u32 = 200;
/// 列数上界
const COL_MAX: u32 = 500;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 窗口尺寸
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WinSize {
    pub rows: u32,
    pub cols: u32,
}

impl WinSize {
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

    pub fn cells(&self) -> u64 {
        self.rows as u64 * self.cols as u64
    }
}

/// 重绘帧
#[derive(Clone, Debug, PartialEq)]
pub struct RedrawFrame {
    /// 绘制所用尺寸
    pub size: WinSize,
    /// 帧序号
    pub no: u64,
}

/// 尺寸变更联测台
#[derive(Clone, Debug)]
pub struct WinchDesk {
    /// 当前生效尺寸
    pub cur: WinSize,
    /// SIGWINCH 挂起位
    pub winch_pending: bool,
    /// fd 存活
    pub fd_alive: bool,
    /// 三拍账
    pub beat_set: u32,
    pub beat_sig: u32,
    pub beat_redraw: u32,
    /// 重绘帧记录
    pub frames: Vec<RedrawFrame>,
    /// 错误计数
    pub n_einval: u32,
    pub n_ebadf: u32,
}

impl WinchDesk {
    pub fn new() -> Self {
        WinchDesk {
            cur: WinSize { rows: 24, cols: 80 },
            winch_pending: false,
            fd_alive: true,
            beat_set: 0,
            beat_sig: 0,
            beat_redraw: 0,
            frames: Vec::new(),
            n_einval: 0,
            n_ebadf: 0,
        }
    }

    /// 拍1：TIOCSWINSZ 设置尺寸并置 SIGWINCH
    pub fn tiocswinsz(&mut self, fd: i32, sz: WinSize) -> Result<WinSize, u32> {
        if fd < 0 || !self.fd_alive {
            self.n_ebadf = self.n_ebadf.wrapping_add(1);
            return Err(WS_EBADF);
        }
        if !sz.valid() {
            self.n_einval = self.n_einval.wrapping_add(1);
            return Err(WS_EINVAL);
        }
        self.cur = sz;
        self.winch_pending = true;
        self.beat_set = self.beat_set.wrapping_add(1);
        Ok(sz)
    }

    /// 拍2：信号递送（应用侧捕获置位）
    pub fn deliver_sig(&mut self) -> Result<u32, u32> {
        if !self.winch_pending {
            return Err(WS_ENOSIG);
        }
        // 挂起位转递送账
        self.beat_sig = self.beat_sig.wrapping_add(1);
        Ok(SIGWINCH)
    }

    /// 拍3：按最新尺寸重绘（必须在信号递送之后）
    pub fn redraw(&mut self) -> Result<RedrawFrame, u32> {
        if !self.fd_alive {
            self.n_ebadf = self.n_ebadf.wrapping_add(1);
            return Err(WS_EBADF);
        }
        if self.beat_sig == 0 || self.beat_sig <= self.beat_redraw {
            return Err(WS_ENOSIG);
        }
        if !self.cur.valid() {
            return Err(WS_EINVAL);
        }
        let f = RedrawFrame { size: self.cur, no: self.frames.len() as u64 + 1 };
        self.frames.push(f.clone());
        self.winch_pending = false;
        self.beat_redraw = self.beat_redraw.wrapping_add(1);
        Ok(f)
    }

    /// 三拍全通：set=sig=redraw 且无挂起残留
    pub fn chain_ok(&self) -> bool {
        self.beat_set == self.beat_sig
            && self.beat_sig == self.beat_redraw
            && !self.winch_pending
    }

    /// 重绘用新尺寸：末帧尺寸等于当前生效尺寸
    pub fn latest_matches(&self) -> bool {
        match self.frames.last() {
            Some(f) => f.size == self.cur,
            None => false,
        }
    }

    /// 账目对平
    pub fn ledger_ok(&self, expect_set: u32) -> bool {
        self.beat_set == expect_set && self.chain_ok()
    }

    /// 总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h = self.cur.fold();
        for f in &self.frames {
            h = (h ^ f.size.fold()).wrapping_mul(0x100000001b3);
            h = (h ^ f.no).wrapping_mul(0x100000001b3);
        }
        h = (h ^ self.beat_set as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染
    pub fn render(&self) -> String {
        crate::alloc::format!(
            "winch:{}x{} set={} sig={} redraw={}",
            self.cur.rows, self.cur.cols, self.beat_set, self.beat_sig, self.beat_redraw
        )
    }
}

impl Default for WinchDesk {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    let mut d = WinchDesk::new();
    // 路1：零尺寸 → EINVAL
    let r1 = matches!(d.tiocswinsz(0, WinSize { rows: 0, cols: 80 }), Err(x) if x == WS_EINVAL);
    // 路2：坏 fd → EBADF
    let r2 = matches!(d.tiocswinsz(-1, WinSize { rows: 30, cols: 90 }), Err(x) if x == WS_EBADF);
    // 路3：无信号即重绘 → ENOSIG
    let r3 = matches!(d.redraw(), Err(x) if x == WS_ENOSIG);
    // 路4：设置后不递送信号直接重绘仍拒（顺序面）
    let _ = d.tiocswinsz(0, WinSize { rows: 40, cols: 100 });
    let r4 = matches!(d.redraw(), Err(x) if x == WS_ENOSIG);
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：每轮一次三拍链（新尺寸=旧尺寸+1 循环），全通记 1 轮。
pub fn sample_thousand() -> u32 {
    let mut d = WinchDesk::new();
    let mut good: u32 = 0;
    for r in 0..1000u32 {
        let sz = WinSize {
            rows: (r % ROW_MAX) + 1,
            cols: (r % 300) + 100,
        };
        let s1 = d.tiocswinsz(0, sz);
        let s2 = d.deliver_sig();
        let s3 = d.redraw();
        let chain = d.chain_ok();
        let latest = d.latest_matches();
        // 重复重绘必须被拒（无挂起信号）
        let dup_rej = matches!(d.redraw(), Err(x) if x == WS_ENOSIG);
        if s1 == Ok(sz) && s2 == Ok(SIGWINCH) && s3.is_ok() && chain && latest && dup_rej {
            good = good.wrapping_add(1);
        }
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10966_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10966");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "EINVAL/EBADF/ENOSIG 坏样本全部命中");

    // J1 三拍全通 10/10
    let mut ok10 = 0u32;
    for i in 0..10u32 {
        let mut d = WinchDesk::new();
        let sz = WinSize { rows: 30 + i, cols: 90 + i };
        let a = d.tiocswinsz(0, sz).is_ok();
        let b = d.deliver_sig().is_ok();
        let c = d.redraw().is_ok();
        if a && b && c && d.chain_ok() {
            ok10 = ok10.wrapping_add(1);
        }
    }
    s.add("J1-三拍全通10/10", ok10 == 10, "设置/信号/重绘十轮全通");

    // 重绘用新尺寸 10/10
    let mut sz10 = 0u32;
    for i in 0..10u32 {
        let mut d = WinchDesk::new();
        let sz = WinSize { rows: 50 + i, cols: 120 + i };
        let _ = d.tiocswinsz(0, sz);
        let _ = d.deliver_sig();
        if let Ok(f) = d.redraw() {
            if f.size == sz && d.latest_matches() {
                sz10 = sz10.wrapping_add(1);
            }
        }
    }
    s.add("J2-重绘用新尺寸10/10", sz10 == 10, "重绘帧尺寸逐位等于新设置");

    // 千轮样本
    let good = sample_thousand();
    s.add("J3-千轮三拍", good == 1000, "千轮三拍链全通且重复重绘被拒");

    // 错误码分段
    let codes = [WS_EBADF, WS_EINVAL, WS_ENOSIG, WS_ESEQ, WS_EIO, WS_EDRIFT];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J4-错误码分段", seg_ok, "六码全部落在 0x1d840 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J5-错误码互异", distinct, "六码两两互异");

    // 账目对平
    let mut d2 = WinchDesk::new();
    let _ = d2.tiocswinsz(0, WinSize { rows: 25, cols: 81 });
    let _ = d2.deliver_sig();
    let _ = d2.redraw();
    s.add("J6-账目对平", d2.ledger_ok(1), "三拍账一致且无挂起残留");

    // 连续两次变更链
    let _ = d2.tiocswinsz(0, WinSize { rows: 26, cols: 82 });
    let _ = d2.deliver_sig();
    let two_ok = d2.redraw().is_ok() && d2.beat_redraw == 2 && d2.frames[1].size.rows == 26;
    s.add("J7-连续变更", two_ok, "第二次变更链重绘采用第二尺寸");

    // 判定序：EBADF 先于 EINVAL
    let mut d3 = WinchDesk::new();
    d3.fd_alive = false;
    let seq_ok = matches!(d3.tiocswinsz(0, WinSize { rows: 0, cols: 0 }), Err(x) if x == WS_EBADF);
    s.add("J8-判定序", seq_ok, "失效 fd 先于尺寸校验");

    // fold 稳定
    let h1 = d2.fold_all();
    let h2 = d2.fold_all();
    s.add("J9-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染
    let r = d2.render();
    s.add(
        "J10-渲染非空",
        r.starts_with("winch:") && r.contains("26x82"),
        "摘要含当前尺寸",
    );

    // 尺寸域校验
    let bad1 = !WinSize { rows: ROW_MAX + 1, cols: 80 }.valid();
    let bad2 = !WinSize { rows: 24, cols: 0 }.valid();
    let good1 = WinSize { rows: ROW_MAX, cols: COL_MAX }.valid();
    s.add("J11-尺寸域", bad1 && bad2 && good1, "边界尺寸判定正确");

    // 动态判据
    s.add_dyn(
        "J12-三拍账",
        d2.beat_set == 2 && d2.beat_sig == 2 && d2.beat_redraw == 2,
        &crate::alloc::format!("set={} sig={} redraw={}", d2.beat_set, d2.beat_sig, d2.beat_redraw),
    );
    s.add_dyn(
        "J13-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J14-帧账",
        d2.frames.len() == 2,
        &crate::alloc::format!("帧数={}", d2.frames.len()),
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
    fn unx10966_winsize_resize_flow() {
        let mut d = WinchDesk::new();
        let sz = WinSize { rows: 40, cols: 120 };
        assert_eq!(d.tiocswinsz(0, sz), Ok(sz));
        assert_eq!(d.deliver_sig(), Ok(SIGWINCH));
        let f = d.redraw().unwrap();
        assert_eq!(f.size, sz);
        assert!(matches!(d.tiocswinsz(0, WinSize { rows: 0, cols: 1 }), Err(x) if x == WS_EINVAL));
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10966_checks_green() {
        let s = run_unx10966_checks();
        assert!(s.all_passed(), "UNX-F10966 判据未全绿");
    }
}
