//! UNX-F10971 终端状态快照与恢复联测（对称性）（0x1d8e0 段）
//!
//! 卡片判据（UNX-F10971-J1）：
//! - get→set→get 往返逐位一致（10/10），千次往返零漂移（10/10）。
//!
//! 域：CoRun·UNX-C4-B29。本模块建模终端状态快照对称性：
//! termios 状态体（四标志组+速率+窗口尺寸）get 出快照、set 写回、
//! 再 get 必逐位一致；千次往返摘要零漂移；非对称面（改位后
//! 不还原）显性报错。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10971-10900)*0x20 = 0x1d8e0）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d8e0;

/// TS_EBADF：fd 无效
pub const TS_EBADF: u32 = SEG_BASE + 0x01;
/// TS_EINVAL：状态域非法（速率/尺寸越界）
pub const TS_EINVAL: u32 = SEG_BASE + 0x02;
/// TS_EDRIFT：往返逐位漂移（自检面）
pub const TS_EDRIFT: u32 = SEG_BASE + 0x03;
/// TS_ENOSYM：快照非对称（自检面）
pub const TS_ENOSYM: u32 = SEG_BASE + 0x04;
/// TS_ECLOSED：终端已关闭
pub const TS_ECLOSED: u32 = SEG_BASE + 0x05;
/// TS_EORDER：先 set 后 get 顺序外使用（自检面）
pub const TS_EORDER: u32 = SEG_BASE + 0x06;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// 支持的波特率集合（建模值）
pub const SPEEDS: [u32; 6] = [9600, 19200, 38400, 57600, 115200, 230400];
/// 行上界
const ROW_MAX: u32 = 200;
/// 列上界
const COL_MAX: u32 = 500;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 终端状态快照
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermState {
    pub iflag: u32,
    pub oflag: u32,
    pub cflag: u32,
    pub lflag: u32,
    pub ispeed: u32,
    pub ospeed: u32,
    pub rows: u32,
    pub cols: u32,
}

impl TermState {
    /// 默认态
    pub fn default_state() -> Self {
        TermState {
            iflag: 0x0500,
            oflag: 0x0005,
            cflag: 0x00bf,
            lflag: 0x8a3b,
            ispeed: 115200,
            ospeed: 115200,
            rows: 24,
            cols: 80,
        }
    }

    /// FNV 折叠（逐位对账摘要）
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.iflag as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.oflag as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.cflag as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.lflag as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.ispeed as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.ospeed as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.rows as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.cols as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 域校验
    pub fn valid(&self) -> bool {
        SPEEDS.contains(&self.ispeed)
            && SPEEDS.contains(&self.ospeed)
            && self.rows > 0
            && self.rows <= ROW_MAX
            && self.cols > 0
            && self.cols <= COL_MAX
    }
}

/// 快照对称性联测台
#[derive(Clone, Debug)]
pub struct TermSnapDesk {
    pub cur: TermState,
    /// fd 存活
    pub fd_alive: bool,
    /// get 计数
    pub n_get: u64,
    /// set 计数
    pub n_set: u64,
    /// 往返成功计数
    pub n_roundtrip: u64,
    /// 错误计数
    pub n_ebadf: u32,
    pub n_einval: u32,
}

impl TermSnapDesk {
    pub fn new() -> Self {
        TermSnapDesk {
            cur: TermState::default_state(),
            fd_alive: true,
            n_get: 0,
            n_set: 0,
            n_roundtrip: 0,
            n_ebadf: 0,
            n_einval: 0,
        }
    }

    /// get：快照当前状态
    pub fn get(&mut self, fd: i32) -> Result<TermState, u32> {
        if fd < 0 || !self.fd_alive {
            self.n_ebadf = self.n_ebadf.wrapping_add(1);
            return Err(TS_EBADF);
        }
        self.n_get = self.n_get.wrapping_add(1);
        Ok(self.cur)
    }

    /// set：写回状态
    pub fn set(&mut self, fd: i32, st: TermState) -> Result<(), u32> {
        if fd < 0 || !self.fd_alive {
            self.n_ebadf = self.n_ebadf.wrapping_add(1);
            return Err(TS_EBADF);
        }
        if !st.valid() {
            self.n_einval = self.n_einval.wrapping_add(1);
            return Err(TS_EINVAL);
        }
        self.cur = st;
        self.n_set = self.n_set.wrapping_add(1);
        Ok(())
    }

    /// get→set→get 往返：三次逐位一致
    pub fn roundtrip(&mut self, fd: i32, st: TermState) -> Result<bool, u32> {
        let before = self.get(fd)?;
        self.set(fd, st)?;
        let after = self.get(fd)?;
        // 对称性：set 后 get == set 值；且 set 前快照可完整还原
        self.set(fd, before)?;
        let back = self.get(fd)?;
        let ok = after == st && back == before;
        if ok {
            self.n_roundtrip = self.n_roundtrip.wrapping_add(1);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 对账：每次往返恰 3 get（写前快照/写后核验/还原核验）与 2 set（写回/还原）。
    pub fn ledger_ok(&self, expect_rt: u64) -> bool {
        self.n_roundtrip == expect_rt
            && self.n_get == self.n_roundtrip.wrapping_mul(3)
            && self.n_set == self.n_roundtrip.wrapping_mul(2)
    }

    /// 总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h = self.cur.fold();
        h = (h ^ self.n_roundtrip).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染
    pub fn render(&self) -> String {
        crate::alloc::format!(
            "termsnap:rt={} get={} set={} rows={}x{}",
            self.n_roundtrip, self.n_get, self.n_set, self.cur.rows, self.cur.cols
        )
    }
}

impl Default for TermSnapDesk {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    let mut d = TermSnapDesk::new();
    // 路1：坏 fd get → EBADF
    let r1 = matches!(d.get(-1), Err(x) if x == TS_EBADF);
    // 路2：非法速率 set → EINVAL
    let mut bad = TermState::default_state();
    bad.ispeed = 1234;
    let r2 = matches!(d.set(0, bad), Err(x) if x == TS_EINVAL);
    // 路3：零尺寸 → EINVAL
    let mut bad2 = TermState::default_state();
    bad2.rows = 0;
    let r3 = matches!(d.set(0, bad2), Err(x) if x == TS_EINVAL);
    // 路4：非对称面：改位不还原必被检出
    let before = d.get(0).unwrap_or(TermState::default_state());
    let mut mid = before;
    mid.cols = 100;
    let _ = d.set(0, mid);
    let after = d.get(0).unwrap_or(TermState::default_state());
    let r4 = after != before;
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：每轮构造变化状态做 get→set→get→还原→get 往返，
/// 逐位一致记 1 轮。
pub fn sample_thousand() -> u32 {
    let mut d = TermSnapDesk::new();
    let mut good: u32 = 0;
    for r in 0..1000u32 {
        let st = TermState {
            iflag: 0x0500 ^ ((r & 0xff) as u32),
            oflag: 0x0005 ^ ((r >> 2) & 0x0f) as u32,
            cflag: 0x00bf,
            lflag: 0x8a3b ^ ((r >> 4) & 0xff) as u32,
            ispeed: SPEEDS[(r % 6) as usize],
            ospeed: SPEEDS[((r + 3) % 6) as usize],
            rows: (r % ROW_MAX) + 1,
            cols: (r % COL_MAX) + 1,
        };
        match d.roundtrip(0, st) {
            Ok(true) => good = good.wrapping_add(1),
            _ => {}
        }
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10971_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10971");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "EBADF/EINVAL/非对称面坏样本全部命中");

    // J1 get→set→get 往返逐位一致 10/10
    let mut ok10 = 0u32;
    for i in 0..10u32 {
        let mut d = TermSnapDesk::new();
        let mut st = TermState::default_state();
        st.rows = 30 + i;
        st.cols = 100 + i;
        if d.roundtrip(0, st) == Ok(true) {
            ok10 = ok10.wrapping_add(1);
        }
    }
    s.add("J1-往返逐位一致10/10", ok10 == 10, "get/set/get 十轮逐位全同");

    // 千次往返零漂移 10/10（每组千轮）
    let mut drift10 = 0u32;
    for seed in 0..10u32 {
        let mut d = TermSnapDesk::new();
        let mut all_ok = true;
        for r in 0..1000u32 {
            let rr = r.wrapping_add(seed * 1000);
            let st = TermState {
                iflag: 0x0500 ^ ((rr & 0x7f) as u32),
                oflag: 0x0005,
                cflag: 0x00bf,
                lflag: 0x8a3b,
                ispeed: SPEEDS[(rr % 6) as usize],
                ospeed: 115200,
                rows: (rr % ROW_MAX) + 1,
                cols: 80 + (rr % 100),
            };
            if d.roundtrip(0, st) != Ok(true) {
                all_ok = false;
            }
        }
        // 终态与最后一次还原一致且账对平
        if all_ok && d.ledger_ok(1000) {
            drift10 = drift10.wrapping_add(1);
        }
    }
    s.add(
        "J2-千次往返零漂移10/10",
        drift10 == 10,
        "十组千轮往返摘要零漂移且账对平",
    );

    // 千轮样本
    let good = sample_thousand();
    s.add("J3-千轮样本", good == 1000, "千轮往返全对");

    // 折叠对称性：相同状态折叠一致
    let a = TermState::default_state();
    let b = TermState::default_state();
    let fold_sym = a.fold() == b.fold();
    s.add("J4-折叠对称", fold_sym, "相同状态两次折叠一致");

    // 逐位比较面：任一位变化必可检出
    let mut c = a;
    c.lflag = c.lflag ^ 0x1;
    let bit_face = c != a && c.fold() != a.fold();
    s.add("J5-逐位检出", bit_face, "单位翻转折叠必变");

    // 错误码分段
    let codes = [TS_EBADF, TS_EINVAL, TS_EDRIFT, TS_ENOSYM, TS_ECLOSED, TS_EORDER];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J6-错误码分段", seg_ok, "六码全部落在 0x1d8e0 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J7-错误码互异", distinct, "六码两两互异");

    // 账目对平
    let mut d2 = TermSnapDesk::new();
    let _ = d2.roundtrip(0, TermState::default_state());
    s.add("J8-账目对平", d2.ledger_ok(1) && d2.n_get == 3 && d2.n_set == 2, "一次往返 3 get / 2 set 严格对平");

    // 速率表
    s.add(
        "J9-速率表",
        SPEEDS.contains(&115200) && SPEEDS.len() == 6,
        "六档速率含 115200",
    );

    // 判定序：EBADF 先于 EINVAL
    let mut d3 = TermSnapDesk::new();
    d3.fd_alive = false;
    let seq_ok = matches!(d3.set(0, TermState::default_state()), Err(x) if x == TS_EBADF);
    s.add("J10-判定序", seq_ok, "失效 fd 先于域校验");

    // fold 稳定
    let h1 = d2.fold_all();
    let h2 = d2.fold_all();
    s.add("J11-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染
    let r = d2.render();
    s.add(
        "J12-渲染非空",
        r.starts_with("termsnap:") && r.contains("rt=1"),
        "摘要含往返账与尺寸",
    );

    // 动态判据
    s.add_dyn(
        "J13-往返账",
        d2.n_roundtrip == 1,
        &crate::alloc::format!("rt={} get={} set={}", d2.n_roundtrip, d2.n_get, d2.n_set),
    );
    s.add_dyn(
        "J14-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J15-终态有效",
        d2.cur.valid(),
        &crate::alloc::format!("rows={} cols={} speed={}", d2.cur.rows, d2.cur.cols, d2.cur.ispeed),
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
    fn unx10971_termstate_snap_flow() {
        let mut d = TermSnapDesk::new();
        let mut st = TermState::default_state();
        st.cols = 132;
        assert_eq!(d.roundtrip(0, st), Ok(true));
        assert_eq!(d.get(0).map(|t| t.cols), Ok(80));
        assert!(matches!(d.set(0, TermState { ispeed: 1, ..TermState::default_state() }), Err(x) if x == TS_EINVAL));
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10971_checks_green() {
        let s = run_unx10971_checks();
        assert!(s.all_passed(), "UNX-F10971 判据未全绿");
    }
}
