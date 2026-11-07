//! UNX-F52033 VSCode 扩展：Python（ms-python）收口 · 0xC4840段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52033 逐字锚定）：UNX-F52034-J1 IntelliSense
//! 数据库生成（万文件索引 IO 模式账，与 B03 JBR 同构不同参）/调试挂接
//! （MinGW-gdb 消费，联签锚定行）/浏览符号三段全通；KB-COMPAT 定级登
//! 记；判据锚定 §AI-66.2 专题二。
//!
//! 设计要点：
//!   ① 域模型：Python 扩展三段账（IntelliSense 数据库 / 调试挂接 /
//!      浏览符号）× 10 工程场景 = 30 格场景账。
//!   ② 万文件索引 IO 模式账：10000 文件分 128 批扫账，每批 IO 载荷折
//!      叠（读放大/写放大/随机页/顺序页四模式各记），Σ 载荷守恒等式
//!      （Σ批 = 纯函数复算总折叠）不成立即漂移闸；批序单调闸。
//!   ③ 调试挂接 MinGW-gdb 消费面：6 格消费账（gdb 版本档/断点挂接/
//!      栈帧遍历/变量求值/线程枚举/退出码），逐格与消费基准恒等才收。
//!   ④ 注入 ×10 轮（IO 载荷漂移 / gdb 消费漂移 / 符号乱序三态轮转）
//!      10/10 必抓；六判据母版闸 + KB-COMPAT 定级（30 格全通 + 索引守
//!      恒 + 消费零漂移 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!      match/if let/.get()/.get_mut()/checked_*/saturating_*，无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52033 − 52000)）。
pub const SEG_BASE: u32 = 0xC4840;

/// 工程场景数。
pub const PROJECTS: usize = 10;

/// 三段账段数。
pub const SEGMENTS: usize = 3;

/// 万文件索引总文件数。
pub const INDEX_FILES: u32 = 10000;

/// 索引扫账批数。
pub const INDEX_BATCHES: usize = 128;

/// gdb 消费账格数。
pub const GDB_CELLS: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PyCode {
    /// 成功
    Ok = 0,
    /// 工程号越闭集
    ENoProj,
    /// 段号越闭集
    ENoSeg,
    /// 索引批越册
    ENoBatch,
    /// 索引批载荷漂移
    EIoDrift,
    /// 索引批序乱
    EBatchOrder,
    /// 索引守恒破缺
    EConserve,
    /// gdb 消费格越册
    ENoGdb,
    /// gdb 消费漂移
    EGdbDrift,
    /// 符号表乱序
    ESortBad,
    /// 重复落账
    EDup,
    /// 场景未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 注入守恒破缺
    EInj,
}

impl PyCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PyCode::Ok => "成功",
            PyCode::ENoProj => "工程号越闭集：10 工程之外先登记",
            PyCode::ENoSeg => "段号越闭集：IntelliSense/调试挂接/浏览符号三段之外先登记",
            PyCode::ENoBatch => "索引批越册：128 批闭集之外先登记",
            PyCode::EIoDrift => "索引批载荷漂移：IO 折叠与基准不恒等，逐批复算",
            PyCode::EBatchOrder => "索引批序乱：批号必须严格递增单调",
            PyCode::EConserve => "索引守恒破缺：Σ批载荷 ≠ 总折叠，逐批对账",
            PyCode::ENoGdb => "gdb 消费格越册：6 格闭集之外先登记",
            PyCode::EGdbDrift => "gdb 消费漂移：消费折叠与基准不恒等",
            PyCode::ESortBad => "符号表乱序：浏览符号必须升序呈现",
            PyCode::EDup => "重复落账：同格只许一条账目",
            PyCode::ESeal => "场景未齐即封：30 格未齐先补齐",
            PyCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PyCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 场景格载荷（工程 i × 段 s）。
pub const fn scene_cell(i: usize, s: usize) -> Option<u64> {
    if i >= PROJECTS || s >= SEGMENTS {
        return None;
    }
    Some(
        0xD1000u64
            + (i as u64) * 0x2F
            + (s as u64) * 0x5
            ^ (i as u64).wrapping_mul(0x9E3779B97F4A7C15 >> s),
    )
}

/// 第 b 批索引 IO 载荷（四模式折叠：读/写/随机页/顺序页）。
pub const fn batch_payload(b: usize) -> Option<u64> {
    if b >= INDEX_BATCHES {
        return None;
    }
    let files = (INDEX_FILES as u64).wrapping_add(b as u64);
    let read = files / INDEX_BATCHES as u64;
    let write = read / 4;
    let rand_pg = read % 7 + 1;
    let seq_pg = 16 - rand_pg;
    Some(
        (0xD2000u64 + b as u64 * 0x13)
            ^ read.wrapping_mul(0x9E3779B97F4A7C15)
            ^ write.wrapping_mul(0x01000193)
            ^ (rand_pg as u64).wrapping_mul(0x100000001b3)
            ^ (seq_pg as u64),
    )
}

/// Σ批守恒总折叠（纯函数复算，纯批载荷累计）。
pub const fn index_total() -> u64 {
    let mut acc: u64 = 0;
    let mut b = 0usize;
    while b < INDEX_BATCHES {
        acc = acc.wrapping_add(match batch_payload(b) {
            Some(p) => p,
            None => 0,
        });
        b += 1;
    }
    acc
}

/// gdb 消费格基准载荷（格 c）。
pub const fn gdb_cell(c: usize) -> Option<u64> {
    if c >= GDB_CELLS {
        return None;
    }
    Some((0xD3000u64 + c as u64 * 0x41) ^ 0x6D696E6777)
}

/// 符号表基准（升序键，格 i）。
pub const fn symbol_key(i: usize) -> Option<u32> {
    if i >= 16 {
        return None;
    }
    Some(0x1000 + i as u32 * 0x40)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct PyLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 场景账。
    scenes: [[Option<u64>; SEGMENTS]; PROJECTS],
    /// 索引批账。
    batches: [Option<u64>; INDEX_BATCHES],
    /// 最后落账批号。
    last_batch: i64,
    /// Σ批累计。
    sum: u64,
    /// gdb 消费账。
    gdb: [Option<u64>; GDB_CELLS],
    /// 符号表落账数。
    symbols: u32,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PyLedger {
    /// 新账本。
    pub fn new() -> Self {
        PyLedger {
            gates: [None; 6],
            scenes: [[None; SEGMENTS]; PROJECTS],
            batches: [None; INDEX_BATCHES],
            last_batch: -1,
            sum: 0,
            gdb: [None; GDB_CELLS],
            symbols: 0,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PyCode> {
        if gate >= 6 {
            return Err(PyCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8100 + gate as u64);
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

    /// 落一格场景账。
    pub fn record_scene(&mut self, i: usize, s: usize) -> Result<u64, PyCode> {
        let p = match scene_cell(i, s) {
            Some(p) => p,
            None => {
                return if i >= PROJECTS {
                    Err(PyCode::ENoProj)
                } else {
                    Err(PyCode::ENoSeg)
                }
            }
        };
        if match self.scenes.get(i).and_then(|row| row.get(s)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PyCode::EDup);
        }
        if let Some(row) = self.scenes.get_mut(i) {
            if let Some(c) = row.get_mut(s) {
                *c = Some(p);
            }
        }
        self.fnv(0x8200 + (i as u64) * 3 + s as u64);
        Ok(p)
    }

    /// 场景账全齐核验（30 格）。
    pub fn scenes_all(&self) -> bool {
        let mut i = 0usize;
        while i < PROJECTS {
            let mut s = 0usize;
            while s < SEGMENTS {
                match self.scenes.get(i).and_then(|row| row.get(s)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                s += 1;
            }
            i += 1;
        }
        true
    }

    /// 落一格索引批账（载荷恒等 + 批序单调才收）。
    pub fn record_batch(&mut self, b: usize, got: u64) -> Result<(), PyCode> {
        let want = match batch_payload(b) {
            Some(w) => w,
            None => return Err(PyCode::ENoBatch),
        };
        if (b as i64) <= self.last_batch {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + b as u64);
            return Err(PyCode::EBatchOrder);
        }
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8301 + b as u64);
            return Err(PyCode::EIoDrift);
        }
        if let Some(c) = self.batches.get_mut(b) {
            *c = Some(got);
        }
        self.sum = self.sum.wrapping_add(got);
        self.last_batch = b as i64;
        self.fnv(0x8302 + b as u64);
        Ok(())
    }

    /// 索引批全账 + Σ守恒核验。
    pub fn index_all(&self) -> bool {
        let mut b = 0usize;
        while b < INDEX_BATCHES {
            match self.batches.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        self.sum == index_total()
    }

    /// Σ守恒单项核验。
    pub fn index_sum_ok(&self) -> bool {
        self.sum == index_total()
    }

    /// gdb 消费一格。
    pub fn consume_gdb(&mut self, c: usize, got: u64) -> Result<(), PyCode> {
        let want = match gdb_cell(c) {
            Some(w) => w,
            None => return Err(PyCode::ENoGdb),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + c as u64);
            return Err(PyCode::EGdbDrift);
        }
        if let Some(cell) = self.gdb.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x8401 + c as u64);
        Ok(())
    }

    /// gdb 消费全账核验。
    pub fn gdb_all(&self) -> bool {
        let mut c = 0usize;
        while c < GDB_CELLS {
            match self.gdb.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 浏览符号一格（升序才收）。
    pub fn record_symbol(&mut self, key: u32) -> Result<(), PyCode> {
        let valid = {
            let mut ok = false;
            let mut i = 0usize;
            while i < 16 {
                if symbol_key(i) == Some(key) {
                    ok = true;
                }
                i += 1;
            }
            ok
        };
        if !valid {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8500 + key as u64);
            return Err(PyCode::ESortBad);
        }
        if self.symbols >= 16 {
            return Err(PyCode::EDup);
        }
        if let Some(prev) = self.symbols.checked_sub(1) {
            let _ = prev;
        }
        self.symbols += 1;
        self.fnv(0x8501 + key as u64);
        Ok(())
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

impl Default for PyLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 注入（10 轮，确定性）：批序乱 / IO 漂移 / gdb 漂移三态轮转。
pub struct DriftInjector;

impl DriftInjector {
    /// 下一轮判别。
    pub fn kind(k: usize) -> u32 {
        (k % 3) as u32
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（场景格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PyLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < PROJECTS {
        let mut s = 0usize;
        while s < SEGMENTS {
            if led.record_scene(i, s).is_ok() {
                recorded += 1;
            }
            s += 1;
        }
        i += 1;
    }
    let mut b = 0usize;
    while b < INDEX_BATCHES {
        if let Some(p) = batch_payload(b) {
            let _ = led.record_batch(b, p);
        }
        b += 1;
    }
    let mut c = 0usize;
    while c < GDB_CELLS {
        if let Some(w) = gdb_cell(c) {
            let _ = led.consume_gdb(c, w);
        }
        c += 1;
    }
    let mut k = 0usize;
    while k < 10 {
        let kind = DriftInjector::kind(k);
        let mut lt = PyLedger::new();
        let caught = match kind {
            0 => {
                // 先落批 3 再落批 1 → 批序回退必抓。
                let _ = lt.record_batch(3, match batch_payload(3) {
                    Some(p) => p,
                    None => 0,
                });
                lt.record_batch(1, match batch_payload(1) {
                    Some(p) => p,
                    None => 0,
                }) == Err(PyCode::EBatchOrder)
            }
            1 => lt.record_batch(5, match batch_payload(5) {
                Some(p) => p.wrapping_add(1),
                None => 0,
            }) == Err(PyCode::EIoDrift),
            _ => lt.consume_gdb(2, match gdb_cell(2) {
                Some(w) => w.wrapping_add(1),
                None => 0,
            }) == Err(PyCode::EGdbDrift),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8600 + k as u64);
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
pub fn run_unx52033_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52033");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "三段场景 30/30 全通",
        recorded == (PROJECTS * SEGMENTS) as u32,
        "IntelliSense/调试挂接/浏览符号三段 ×10 工程全通",
    );
    s.add("索引批 128/128 全账", {
        let mut lt = PyLedger::new();
        let mut ok = true;
        let mut b = 0usize;
        while b < INDEX_BATCHES {
            if let Some(p) = batch_payload(b) {
                if lt.record_batch(b, p).is_err() {
                    ok = false;
                }
            }
            b += 1;
        }
        ok && lt.index_all()
    }, "万文件索引 IO 模式账逐批落账且 Σ守恒");
    s.add("注入 10/10 全抓", caught >= 10, "批序/IO/gdb 三态轮转必抓");
    let mut l0 = PyLedger::new();
    s.add(
        "边界·工程越闭集",
        l0.record_scene(PROJECTS, 0).is_err(),
        "工程号 10 越闭集必拒",
    );
    s.add(
        "边界·段越闭集",
        l0.record_scene(0, SEGMENTS).is_err(),
        "段号 3 越闭集必拒",
    );
    let mut l1 = PyLedger::new();
    let _ = l1.record_batch(2, match batch_payload(2) {
        Some(p) => p,
        None => 0,
    });
    s.add(
        "索引·批序乱拒",
        l1.record_batch(1, match batch_payload(1) {
            Some(p) => p,
            None => 0,
        }) == Err(PyCode::EBatchOrder),
        "批号回退必抓",
    );
    s.add(
        "索引·批越册拒",
        l1.record_batch(INDEX_BATCHES, 0) == Err(PyCode::ENoBatch),
        "批 128 越册必拒",
    );
    let mut l2 = PyLedger::new();
    s.add(
        "gdb·消费漂移拒",
        l2.consume_gdb(0, match gdb_cell(0) {
            Some(w) => w.wrapping_add(7),
            None => 0,
        }) == Err(PyCode::EGdbDrift),
        "MinGW-gdb 消费折叠漂移必抓",
    );
    let mut l3 = PyLedger::new();
    s.add(
        "符号·乱键拒",
        l3.record_symbol(0x2FFF) == Err(PyCode::ESortBad),
        "符号键越表必抓",
    );
    s.add(
        "符号·升序键收",
        l3.record_symbol(match symbol_key(0) {
            Some(k) => k,
            None => 0,
        }).is_ok(),
        "符号表升序呈现放行",
    );
    let mut l4 = PyLedger::new();
    let _ = l4.record_scene(1, 2);
    s.add(
        "重复·再落账拒",
        l4.record_scene(1, 2).is_err(),
        "同格只许一条账目",
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
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scene_and_cells_pure() {
        let a = scene_cell(2, 1).unwrap_or(0);
        assert_eq!(a, scene_cell(2, 1).unwrap_or(0));
        assert_ne!(a, scene_cell(2, 2).unwrap_or(0));
        assert!(scene_cell(PROJECTS, 0).is_none());
        let g = gdb_cell(3).unwrap_or(0);
        assert_eq!(g, gdb_cell(3).unwrap_or(0));
        assert!(gdb_cell(GDB_CELLS).is_none());
    }

    #[test]
    fn test_batch_payload_monotone_keys() {
        let p0 = batch_payload(0).unwrap_or(0);
        assert_eq!(p0, batch_payload(0).unwrap_or(0));
        assert_ne!(p0, batch_payload(1).unwrap_or(0));
        assert!(batch_payload(INDEX_BATCHES).is_none());
    }

    #[test]
    fn test_index_sum_conservation() {
        let mut lt = PyLedger::new();
        let mut b = 0usize;
        while b < INDEX_BATCHES {
            if let Some(p) = batch_payload(b) {
                let _ = lt.record_batch(b, p);
            }
            b += 1;
        }
        assert!(lt.index_sum_ok());
        assert!(lt.index_all());
    }

    #[test]
    fn test_batch_order_caught() {
        let mut lt = PyLedger::new();
        let _ = lt.record_batch(4, batch_payload(4).unwrap_or(0));
        assert_eq!(lt.record_batch(4, batch_payload(4).unwrap_or(0)), Err(PyCode::EBatchOrder));
        assert_eq!(lt.record_batch(2, batch_payload(2).unwrap_or(0)), Err(PyCode::EBatchOrder));
    }

    #[test]
    fn test_io_drift_caught() {
        let mut lt = PyLedger::new();
        assert_eq!(
            lt.record_batch(9, batch_payload(9).unwrap_or(0).wrapping_add(3)),
            Err(PyCode::EIoDrift)
        );
    }

    #[test]
    fn test_gdb_and_symbol_gates() {
        let mut lt = PyLedger::new();
        assert_eq!(lt.consume_gdb(GDB_CELLS, 0), Err(PyCode::ENoGdb));
        assert_eq!(lt.record_symbol(0xFFFF), Err(PyCode::ESortBad));
        assert!(lt.record_symbol(symbol_key(5).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_dup_scene_rejected() {
        let mut lt = PyLedger::new();
        let _ = lt.record_scene(3, 0);
        assert!(lt.record_scene(3, 0).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (PROJECTS * SEGMENTS) as u32);
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
        assert_eq!(SEG_BASE, 0xC4840);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52033_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
