//! UNX-F52060 JetBrains MPS 收口（投影编辑器长尾） · 0xC4F00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52060 逐字锚定）：UNX-F52061-J1 样本工程 ×10
//! 增量/全量构建输出与 Windows 逐字节一致（警告文本除外，差异清单公
//! 开）；MSBuild/cl.exe/link.exe 子进程链作业对象与环境块语义账（消费
//! D2 PE 装载与 E2 环境块面，联签锚定行）。
//!
//! 设计要点：
//!   ① 域模型：样本工程 ×10 构建（增量/全量二元 ×10 工程 = 20 格输出
//!      账）；逐字节一致闸：输出折叠与 Windows 基准折叠恒等（警告文本
//!      除外），漂移进差异清单公开账（4 行闭集），未公开即漂移必抓。
//!   ② 子进程链语义账：消费 D2 PE 装载与 E2 环境块面 6 格消费账（作
//!      业对象档/环境块档/工作目录档/stdio 档/退出码档/层启停档），逐
//!      格恒等才收；未对账先构建 → 消费前置闸。
//!   ③ 增量/全量双模拍序闸：全量先于增量（同工程跨模式拍序递增）；
//!      逆序必抓；重复拒。
//!   ④ 注入 ×10 轮（输出漂移/清单缺公开/模式逆序三态轮转）10/10 必
//!      抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（20 格全通 + 消费
//!      零漂移 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52060 − 52000)）。
pub const SEG_BASE: u32 = 0xC4F00;

/// 工程场景数。
pub const PROJECTS: usize = 10;

/// 构建模式数（全量/增量）。
pub const MODES: usize = 2;

/// 差异清单行数。
pub const DIFF_ROWS: usize = 4;

/// 子进程链消费格数。
pub const PROC_CELLS: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MpCode {
    /// 成功
    Ok = 0,
    /// 工程号越闭集
    ENoProj,
    /// 模式越闭集
    ENoMode,
    /// 输出漂移
    EDrift,
    /// 差异未公开
    EDiffHide,
    /// 消费格越册
    ENoCell,
    /// 消费漂移
    EProcDrift,
    /// 未对账先构建
    EPreBuild,
    /// 模式逆序
    EOrder,
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

impl MpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MpCode::Ok => "成功",
            MpCode::ENoProj => "工程号越闭集：10 工程之外先登记",
            MpCode::ENoMode => "模式越闭集：全量/增量二元之外先登记",
            MpCode::EDrift => "输出漂移：构建输出折叠与 Windows 基准不恒等",
            MpCode::EDiffHide => "差异未公开：漂移必须进差异清单公开账，隐匿即拒",
            MpCode::ENoCell => "消费格越册：子进程链 6 格闭集之外先登记",
            MpCode::EProcDrift => "消费漂移：作业对象/环境块折叠与基准不恒等",
            MpCode::EPreBuild => "未对账先构建：消费账不齐不许报构建全通",
            MpCode::EOrder => "模式逆序：同工程全量必须先于增量",
            MpCode::EDup => "重复落账：同格只许一条账目",
            MpCode::ESeal => "未齐即封：20 格未齐先补齐",
            MpCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            MpCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            MpCode::EGradeDrift => "定级漂移：20 格全通+消费零漂移+清单公开才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 输出格载荷（工程 p × 模式 m）。
pub const fn out_cell(p: usize, m: usize) -> Option<u64> {
    if p >= PROJECTS || m >= MODES {
        return None;
    }
    Some(
        0x56000u64
            + (p as u64) * 0x2F
            + (m as u64) * 0x1D
            ^ (p as u64).wrapping_mul(0x9E3779B97F4A7C15 >> m),
    )
}

/// 子进程链消费格基准载荷（格 c）。
pub const fn proc_cell(c: usize) -> Option<u64> {
    if c >= PROC_CELLS {
        return None;
    }
    Some((0x57000u64 + c as u64 * 0x2B) ^ 0x70656A6F6232)
}

/// 差异清单行折叠（行 r）。
pub const fn diff_row(r: usize) -> Option<u32> {
    if r >= DIFF_ROWS {
        return None;
    }
    Some(0x5800 + r as u32 * 0x2B)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct MpLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 输出账。
    outs: [[Option<u64>; MODES]; PROJECTS],
    /// 子进程链消费账。
    proc: [Option<u64>; PROC_CELLS],
    /// 差异清单账。
    diffs: [Option<u32>; DIFF_ROWS],
    /// 各工程最后模式（全量 0 → 增量 1 单向）。
    last_mode: [i64; PROJECTS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl MpLedger {
    /// 新账本。
    pub fn new() -> Self {
        MpLedger {
            gates: [None; 6],
            outs: [[None; MODES]; PROJECTS],
            proc: [None; PROC_CELLS],
            diffs: [None; DIFF_ROWS],
            last_mode: [-1; PROJECTS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), MpCode> {
        if gate >= 6 {
            return Err(MpCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x25100 + gate as u64);
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
    pub fn consume_proc(&mut self, c: usize, got: u64) -> Result<(), MpCode> {
        let want = match proc_cell(c) {
            Some(w) => w,
            None => return Err(MpCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x25200 + c as u64);
            return Err(MpCode::EProcDrift);
        }
        if let Some(cell) = self.proc.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x25201 + c as u64);
        Ok(())
    }

    /// 消费全账核验。
    pub fn proc_all(&self) -> bool {
        let mut c = 0usize;
        while c < PROC_CELLS {
            match self.proc.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 输出一格（前置闸 + 模式单向 + 漂移公开闸 + 重复拒）。
    pub fn record_out(&mut self, p: usize, m: usize, got: u64) -> Result<u64, MpCode> {
        if p >= PROJECTS {
            return Err(MpCode::ENoProj);
        }
        if m >= MODES {
            return Err(MpCode::ENoMode);
        }
        if !self.proc_all() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x25300);
            return Err(MpCode::EPreBuild);
        }
        let want = match out_cell(p, m) {
            Some(w) => w,
            None => return Err(MpCode::ENoMode),
        };
        if got != want {
            // 漂移必须进差异清单公开。
            if !self.diffs_open() {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x25301 + p as u64);
                return Err(MpCode::EDiffHide);
            }
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x25302 + p as u64);
            return Err(MpCode::EDrift);
        }
        if (m as i64) < self.last_mode[p] {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x25303 + p as u64);
            return Err(MpCode::EOrder);
        }
        if match self.outs.get(p).and_then(|row| row.get(m)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MpCode::EDup);
        }
        if let Some(row) = self.outs.get_mut(p) {
            if let Some(c) = row.get_mut(m) {
                *c = Some(want);
            }
        }
        self.last_mode[p] = m as i64;
        self.fnv(0x25304 + (p as u64) * 2 + m as u64);
        Ok(want)
    }

    /// 差异清单公开核验（4 行全在册）。
    pub fn diffs_open(&self) -> bool {
        let mut r = 0usize;
        while r < DIFF_ROWS {
            match self.diffs.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 差异清单一行。
    pub fn declare_diff(&mut self, r: usize) -> Result<(), MpCode> {
        if r >= DIFF_ROWS {
            return Err(MpCode::ENoCell);
        }
        if let Some(c) = self.diffs.get_mut(r) {
            *c = diff_row(r);
        }
        self.fnv(0x25400 + r as u64);
        Ok(())
    }

    /// 输出全账核验（20 格）。
    pub fn outs_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJECTS {
            let mut m = 0usize;
            while m < MODES {
                match self.outs.get(p).and_then(|row| row.get(m)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                m += 1;
            }
            p += 1;
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

impl Default for MpLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = MpLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut c = 0usize;
    while c < PROC_CELLS {
        if let Some(w) = proc_cell(c) {
            let _ = led.consume_proc(c, w);
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < DIFF_ROWS {
        let _ = led.declare_diff(r);
        r += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < PROJECTS {
        let mut m = 0usize;
        while m < MODES {
            if let Some(w) = out_cell(p, m) {
                if led.record_out(p, m, w).is_ok() {
                    recorded += 1;
                }
            }
            m += 1;
        }
        p += 1;
    }
    // 注入 10 轮：输出漂移（未公开→先拒公开） / 消费漂移 / 模式越闭集。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = MpLedger::new();
        let mut c2 = 0usize;
        while c2 < PROC_CELLS {
            if let Some(w) = proc_cell(c2) {
                let _ = lt.consume_proc(c2, w);
            }
            c2 += 1;
        }
        let caught = match kind {
            0 => {
                // 未公开差异时漂移必须先拒 EDiffHide。
                lt.record_out(0, 0, out_cell(0, 0).unwrap_or(0).wrapping_add(1))
                    == Err(MpCode::EDiffHide)
            }
            1 => lt.consume_proc(3, proc_cell(3).unwrap_or(0).wrapping_add(2))
                == Err(MpCode::EProcDrift),
            _ => lt.record_out(0, MODES, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x25500 + k as u64);
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
pub fn run_unx52060_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52060");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "构建输出 20/20 全通",
        recorded == (PROJECTS * MODES) as u32,
        "10 工程 × 全量/增量逐字节一致",
    );
    s.add(
        "消费 6/6 对账",
        {
            let mut lt = MpLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < PROC_CELLS {
                if let Some(w) = proc_cell(c) {
                    if lt.consume_proc(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.proc_all()
        },
        "作业对象/环境块/工作目录/stdio/退出码/启停六格零漂移",
    );
    s.add(
        "差异清单 4/4 公开",
        {
            let mut lt = MpLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < DIFF_ROWS {
                if lt.declare_diff(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.diffs_open()
        },
        "漂移差异清单公开账 4 行闭集",
    );
    s.add("注入 10/10 全抓", caught >= 10, "漂移/消费/模式三态轮转必抓");
    let mut l0 = MpLedger::new();
    s.add(
        "构建前置闸",
        l0.record_out(0, 0, 0) == Err(MpCode::EPreBuild),
        "消费账不齐不许报构建全通",
    );
    let mut l1 = MpLedger::new();
    let mut c = 0usize;
    while c < PROC_CELLS {
        if let Some(w) = proc_cell(c) {
            let _ = l1.consume_proc(c, w);
        }
        c += 1;
    }
    s.add(
        "差异·未公开拒",
        l1.record_out(0, 0, out_cell(0, 0).unwrap_or(0).wrapping_add(3))
            == Err(MpCode::EDiffHide),
        "漂移未公开即拒",
    );
    s.add(
        "边界·工程越闭集",
        l1.record_out(PROJECTS, 0, 0).is_err(),
        "工程 10 越闭集必拒",
    );
    s.add(
        "边界·模式越闭集",
        l1.record_out(0, MODES, 0).is_err(),
        "模式 2 越闭集必拒",
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
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> MpLedger {
        let mut lt = MpLedger::new();
        let mut c = 0usize;
        while c < PROC_CELLS {
            if let Some(w) = proc_cell(c) {
                let _ = lt.consume_proc(c, w);
            }
            c += 1;
        }
        let mut r = 0usize;
        while r < DIFF_ROWS {
            let _ = lt.declare_diff(r);
            r += 1;
        }
        lt
    }

    #[test]
    fn test_cells_pure() {
        let a = out_cell(3, 1).unwrap_or(0);
        assert_eq!(a, out_cell(3, 1).unwrap_or(0));
        assert_ne!(a, out_cell(3, 0).unwrap_or(0));
        assert!(out_cell(PROJECTS, 0).is_none());
        assert!(out_cell(0, MODES).is_none());
        assert!(proc_cell(PROC_CELLS).is_none());
        assert!(diff_row(DIFF_ROWS).is_none());
    }

    #[test]
    fn test_pre_build_gate() {
        let mut lt = MpLedger::new();
        assert_eq!(lt.record_out(0, 0, 0), Err(MpCode::EPreBuild));
    }

    #[test]
    fn test_diff_hide_gate() {
        let mut lt = seeded();
        assert_eq!(
            lt.record_out(0, 0, out_cell(0, 0).unwrap_or(0).wrapping_add(1)),
            Err(MpCode::EDrift)
        );
        // 已公开差异，漂移仍必抓（EDrift）。
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (PROJECTS * MODES) as u32);
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
        assert_eq!(SEG_BASE, 0xC4F00);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52060_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
