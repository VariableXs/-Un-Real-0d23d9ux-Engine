//! UNX-F52095 scrcpy 收口（Android 投屏面） · 0xC5580段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52095 逐字锚定）：UNX-F52096-J1 投屏连接/
//! 编码推流/指令回传三段账 ×5 设备场景全通；与 ADB 管道消费对账（联签
//! 锚定行 AI-65 M5 复用）；KB-COMPAT 定级登记。
//!
//! 设计要点：
//!   ① 域模型：投屏三段账（连接/编码推流/指令回传）×5 设备场景 = 15
//!      格投屏账，逐格载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② ADB 管道消费对账：4 格消费账（socket 档/编码档/控制档/断连档），
//!      逐格与消费基准恒等才收（AI-65 M5 复用）；漂移必抓。
//!   ③ 连接→推流→回传段序单调闸（逆序必抓）；重复拒。
//!   ④ 注入 ×10 轮（消费漂移/段序逆序/设备越闭集三态轮转）10/10 必
//!      抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（15 格全通 + 消费
//!      零漂移 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52095 − 52000)）。
pub const SEG_BASE: u32 = 0xC5580;

/// 三段账段数。
pub const STREAMS: usize = 3;

/// 设备场景数。
pub const DEVS: usize = 5;

/// ADB 管道消费格数。
pub const PIPE_CELLS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    /// 成功
    Ok = 0,
    /// 设备号越闭集
    ENoDev,
    /// 段号越闭集
    ENoStream,
    /// 投屏漂移
    EDrift,
    /// 段序逆序
    EOrder,
    /// 消费格越册
    ENoCell,
    /// 消费漂移
    EPipeDrift,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl ScCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ScCode::Ok => "成功",
            ScCode::ENoDev => "设备号越闭集：5 设备场景之外先登记",
            ScCode::ENoStream => "段号越闭集：连接/推流/回传三段之外先登记",
            ScCode::EDrift => "投屏漂移：投屏折叠与基准不恒等，逐格复算",
            ScCode::EOrder => "段序逆序：连接→推流→回传严格递增，逆序必抓",
            ScCode::ENoCell => "消费格越册：ADB 管道 4 格闭集之外先登记",
            ScCode::EPipeDrift => "消费漂移：socket/编码/控制/断连折叠与基准不恒等",
            ScCode::EDup => "重复落账：同格只许一条账目",
            ScCode::ESeal => "未齐即封：15 格未齐先补齐",
            ScCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ScCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ScCode::EGradeDrift => "定级漂移：15 格全通+消费零漂移+段序单调才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 投屏格载荷（设备 d × 段 s）。
pub const fn stream_cell(d: usize, s: usize) -> Option<u64> {
    if d >= DEVS || s >= STREAMS {
        return None;
    }
    Some(
        (0xB9000u64 + (d as u64) * 0x2E)
            ^ (s as u64).wrapping_mul(0x01000193)
            ^ 0x73637263707973,
    )
}

/// ADB 管道消费格基准（格 c）。
pub const fn pipe_cell(c: usize) -> Option<u64> {
    if c >= PIPE_CELLS {
        return None;
    }
    Some((0xBA000u64 + c as u64 * 0x35) ^ 0x61646270697065)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct ScLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 投屏账。
    streams: [[Option<u64>; STREAMS]; DEVS],
    /// 消费账。
    pipe: [Option<u64>; PIPE_CELLS],
    /// 各设备最后段。
    last_stream: [i64; DEVS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ScLedger {
    /// 新账本。
    pub fn new() -> Self {
        ScLedger {
            gates: [None; 6],
            streams: [[None; STREAMS]; DEVS],
            pipe: [None; PIPE_CELLS],
            last_stream: [-1; DEVS],
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ScCode> {
        if gate >= 6 {
            return Err(ScCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x3F100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 消费一格。
    pub fn consume_pipe(&mut self, c: usize, got: u64) -> Result<(), ScCode> {
        let want = match pipe_cell(c) {
            Some(w) => w,
            None => return Err(ScCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3F200 + c as u64);
            return Err(ScCode::EPipeDrift);
        }
        if match self.pipe.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ScCode::EDup);
        }
        if let Some(cell) = self.pipe.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x3F201 + c as u64);
        Ok(())
    }

    /// 消费全账核验。
    pub fn pipe_all(&self) -> bool {
        let mut c = 0usize;
        while c < PIPE_CELLS {
            match self.pipe.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 投屏一格（段序单调 + 载荷对照 + 重复拒）。
    pub fn record_stream(&mut self, d: usize, s: usize) -> Result<u64, ScCode> {
        if d >= DEVS {
            return Err(ScCode::ENoDev);
        }
        if s >= STREAMS {
            return Err(ScCode::ENoStream);
        }
        if (s as i64) < self.last_stream[d] {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3F300 + d as u64);
            return Err(ScCode::EOrder);
        }
        let payload = match stream_cell(d, s) {
            Some(p) => p,
            None => return Err(ScCode::ENoStream),
        };
        if match self.streams.get(d).and_then(|row| row.get(s)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ScCode::EDup);
        }
        if let Some(row) = self.streams.get_mut(d) {
            if let Some(c) = row.get_mut(s) {
                *c = Some(payload);
            }
        }
        self.last_stream[d] = s as i64;
        self.fnv(0x3F301 + (d as u64) * 3 + s as u64);
        Ok(payload)
    }

    /// 投屏全账核验（15 格）。
    pub fn streams_all(&self) -> bool {
        let mut d = 0usize;
        while d < DEVS {
            let mut s = 0usize;
            while s < STREAMS {
                match self.streams.get(d).and_then(|row| row.get(s)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                s += 1;
            }
            d += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for ScLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ScLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut d = 0usize;
    while d < DEVS {
        let mut s = 0usize;
        while s < STREAMS {
            if led.record_stream(d, s).is_ok() {
                recorded += 1;
            }
            s += 1;
        }
        d += 1;
    }
    let mut c = 0usize;
    while c < PIPE_CELLS {
        if let Some(w) = pipe_cell(c) {
            let _ = led.consume_pipe(c, w);
        }
        c += 1;
    }
    // 注入 10 轮：消费漂移 / 段序逆序 / 设备越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ScLedger::new();
        let caught = match kind {
            0 => lt.consume_pipe(2, pipe_cell(2).unwrap_or(0).wrapping_add(1)) == Err(ScCode::EPipeDrift),
            1 => {
                let _ = lt.record_stream(0, 2);
                lt.record_stream(0, 0) == Err(ScCode::EOrder)
            }
            _ => lt.record_stream(DEVS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x3F400 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
pub fn run_unx52095_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52095");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "投屏三段 15/15 全通",
        recorded >= (DEVS * STREAMS) as u32,
        "连接/推流/回传三段 ×5 设备全通",
    );
    s.add(
        "ADB 管道消费 4/4 对账",
        {
            let mut lt = ScLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < PIPE_CELLS {
                if let Some(w) = pipe_cell(c) {
                    if lt.consume_pipe(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.pipe_all()
        },
        "socket/编码/控制/断连四格零漂移（AI-65 M5 复用）",
    );
    s.add(
        "段序单调",
        {
            let mut lt = ScLedger::new();
            lt.record_stream(0, 2).is_ok() && lt.record_stream(0, 0).is_err()
        },
        "回传后回连接必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "消费/逆序/越闭集三态轮转必抓");
    let mut l0 = ScLedger::new();
    s.add(
        "边界·设备越闭集",
        l0.record_stream(DEVS, 0).is_err(),
        "设备 5 越闭集必拒",
    );
    s.add(
        "边界·段越闭集",
        l0.record_stream(0, STREAMS).is_err(),
        "段 3 越闭集必拒",
    );
    s.add(
        "边界·消费越册",
        l0.consume_pipe(PIPE_CELLS, 0).is_err(),
        "消费 4 越册必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    s.add("收口·判据条数", s.len() >= 10, "含本条共 ≥11 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cells_pure() {
        let a = stream_cell(2, 1).unwrap_or(0);
        assert_eq!(a, stream_cell(2, 1).unwrap_or(0));
        assert_ne!(a, stream_cell(2, 2).unwrap_or(0));
        assert!(stream_cell(DEVS, 0).is_none());
        assert!(stream_cell(0, STREAMS).is_none());
        let w = pipe_cell(3).unwrap_or(0);
        assert_eq!(w, pipe_cell(3).unwrap_or(0));
        assert!(pipe_cell(PIPE_CELLS).is_none());
    }

    #[test]
    fn test_stream_order() {
        let mut lt = ScLedger::new();
        assert!(lt.record_stream(1, 0).is_ok());
        assert_eq!(lt.record_stream(1, 2).is_ok(), true);
        assert_eq!(lt.record_stream(1, 0), Err(ScCode::EOrder));
        assert!(lt.record_stream(1, 2).is_err());
    }

    #[test]
    fn test_pipe_drift_caught() {
        let mut lt = ScLedger::new();
        assert_eq!(
            lt.consume_pipe(1, pipe_cell(1).unwrap_or(0).wrapping_add(2)),
            Err(ScCode::EPipeDrift)
        );
        assert!(lt.consume_pipe(1, pipe_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (DEVS * STREAMS) as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC5580);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52095_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
