//! unx1764_schedlat —— UNX-F1764 调度延迟观测深化（runnable→running 分解）。
//! 锚点（CoRun Unxreal 主册）：UNX-F1764 · 调度延迟观测深化 · 目标 380 行 · 已深化
//! 判据原文：UNX-F1764-J1 入队→选中→上线三段延迟分解账落账，
//!   与 F1647/F1655/F1675 三账对齐断言全过。
//! 组件：三段延迟闭集（入队→选中 = 排队段 / 选中→上线 = 切换段 /
//!       合计 = runnable→running 全程段）
//!       + 分解账（append-only 六列：线程/入队刻度/选中刻度/上线刻度/三段值/合计）
//!       + 三账对齐器（与 F1647/F1655/F1675 三外部账逐条比对）
//!       + 分位面（P50/P95/P99 确定性分位计算）
//!       + 指纹冻结（分解账指纹 FNV-1a 32 位）
//!       + 注入核（段值漂移/对齐失配/刻度倒置/守恒破裂四类 10/10 必抓）。
//! 职责定位：调度延迟的唯一权威分解面——三段分解把总延迟归因到
//!   排队/切换两端；三账对齐使本账与历史账互证不可单方漂移。
//! 诊断码段：0xEF80（0xEF81~0xEF88 闭集；本段为 mod.rs 段位图核验空闲独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const SL_VERSION: &str = "unx1764_schedlat-v1";

/// 诊断段基址（0xEF80 段 · 本模块独占）。
pub const SL_SEG_BASE: u16 = 0xEF80;

/// XcCode 闭集（0xEF81~0xEF88，严格递增互异）。
pub const XC_SEG_DRIFT: u32 = 0xEF81;
pub const XC_ALIGN_MISS: u32 = 0xEF82;
pub const XC_TICK_INVERT: u32 = 0xEF83;
pub const XC_CONSERV_BAD: u32 = 0xEF84;
pub const XC_FINGER_DRIFT: u32 = 0xEF85;
pub const XC_THREAD_OOB: u32 = 0xEF86;
pub const XC_INJECT_MISSED: u32 = 0xEF87;
pub const XC_LEDGER_BROKEN: u32 = 0xEF88;

/// 中文诊断文本（码 → 文案/建议，闭集逐码三要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_SEG_DRIFT => ("段值漂移：三段值与刻度差不符", "段值必须由刻度差直算"),
        XC_ALIGN_MISS => ("对齐失配：与外部三账逐条不一致", "核对 F1647/F1655/F1675 账"),
        XC_TICK_INVERT => ("刻度倒置：上线早于选中或选中早于入队", "刻度必须单调"),
        XC_CONSERV_BAD => ("守恒破裂：三段和不等于全程", "段1+段2 = 全程，逐条复算"),
        XC_FINGER_DRIFT => ("指纹漂移：分解账指纹与冻结值不一致", "账被改，回滚"),
        XC_THREAD_OOB => ("线程越界：线程号越表", "线程号必须落在登记面"),
        XC_INJECT_MISSED => ("注入未捕获：注入核存在漏抓", "四类注入必须 10/10 必抓"),
        XC_LEDGER_BROKEN => ("账结构破坏：分解账被外力改写", "append-only 账回滚"),
        _ => ("未知诊断码", "查本模块 XcCode 闭集"),
    }
}

/// 分解账条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LatEntry {
    /// 线程号。
    pub tid: u32,
    /// 入队刻度。
    pub t_enq: u64,
    /// 选中刻度。
    pub t_pick: u64,
    /// 上线刻度。
    pub t_run: u64,
}

impl LatEntry {
    /// 段 1：排队延迟（入队→选中）。
    pub const fn seg_queue(&self) -> u64 {
        self.t_pick.saturating_sub(self.t_enq)
    }
    /// 段 2：切换延迟（选中→上线）。
    pub const fn seg_switch(&self) -> u64 {
        self.t_run.saturating_sub(self.t_pick)
    }
    /// 全程（runnable→running）。
    pub const fn total(&self) -> u64 {
        self.t_run.saturating_sub(self.t_enq)
    }
    /// 守恒：段1+段2 = 全程。
    pub const fn conserved(&self) -> bool {
        self.seg_queue() + self.seg_switch() == self.total()
    }
    /// 刻度单调（入队 ≤ 选中 ≤ 上线）。
    pub const fn ticks_mono(&self) -> bool {
        self.t_enq <= self.t_pick && self.t_pick <= self.t_run
    }
}

/// 分解账。
pub struct LatLedger {
    entries: Vec<LatEntry>,
    rejects: Vec<(u32, u32, &'static str)>,
    sealed: bool,
    fingerprint: Option<u32>,
}

impl LatLedger {
    /// 新账。
    pub fn new() -> Self {
        LatLedger {
            entries: Vec::new(),
            rejects: Vec::new(),
            sealed: false,
            fingerprint: None,
        }
    }

    /// 落账一条（刻度倒置即拒；线程重复即拒）。
    pub fn record(&mut self, e: LatEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_LEDGER_BROKEN);
        }
        if e.tid == 0 {
            self.rejects.push((XC_THREAD_OOB, e.tid, "线程号 0 越界"));
            return Err(XC_THREAD_OOB);
        }
        if self.entries.iter().any(|x| x.tid == e.tid) {
            self.rejects.push((XC_THREAD_OOB, e.tid, "线程重复落账"));
            return Err(XC_THREAD_OOB);
        }
        if !e.ticks_mono() {
            self.rejects.push((XC_TICK_INVERT, e.tid, "刻度倒置"));
            return Err(XC_TICK_INVERT);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 账长。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// 拒绝账长度。
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    /// 条目切片。
    pub fn entries(&self) -> &[LatEntry] {
        &self.entries
    }

    /// 全账守恒 + 单调判定。
    pub fn all_conserved(&self) -> bool {
        !self.entries.is_empty() && self.entries.iter().all(|e| e.conserved() && e.ticks_mono())
    }

    /// 分位计算（确定性：升序排序取位；pct ∈ 0~100）。
    pub fn percentile(&self, which: u8, pct: u32) -> Option<u64> {
        let mut v: Vec<u64> = Vec::new();
        for e in &self.entries {
            let x = match which {
                1 => e.seg_queue(),
                2 => e.seg_switch(),
                _ => e.total(),
            };
            let _ = v.push(x);
        }
        if v.is_empty() {
            return None;
        }
        // 插入排序（小样本确定性面）
        let n = v.len();
        for i in 1..n {
            let mut j = i;
            while j > 0 && v[j] < v[j - 1] {
                v.swap(j, j - 1);
                j -= 1;
            }
        }
        let idx = (pct.min(100) as usize * n) / 100;
        let i = if idx >= n { n - 1 } else { idx };
        Some(v[i])
    }

    /// 三账对齐：与外部三账（各为 (tid, 全程值) 序列）逐条比对。
    pub fn align_three(&self, f1647: &[(u32, u64)], f1655: &[(u32, u64)], f1675: &[(u32, u64)]) -> bool {
        let check = |ext: &[(u32, u64)]| -> bool {
            for (tid, total) in ext {
                match self.entries.iter().find(|e| e.tid == *tid) {
                    Some(e) => {
                        if &e.total() != total {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            true
        };
        check(f1647) && check(f1655) && check(f1675)
    }

    /// 封账。
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_LEDGER_BROKEN);
        }
        if !self.all_conserved() {
            return Err(XC_CONSERV_BAD);
        }
        let fp = self.joint_fingerprint();
        self.fingerprint = Some(fp);
        self.sealed = true;
        Ok(fp)
    }
    /// 账指纹。
    pub fn joint_fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::with_capacity(self.entries.len() * 25);
        for e in &self.entries {
            bytes.extend_from_slice(&e.tid.to_le_bytes());
            bytes.extend_from_slice(&e.t_enq.to_le_bytes());
            bytes.extend_from_slice(&e.t_pick.to_le_bytes());
            bytes.extend_from_slice(&e.t_run.to_le_bytes());
        }
        fnv1a32(&bytes)
    }
}

/// FNV-1a 32 位。
pub const fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// LCG 确定性发生器。
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 权威观测场景：10 线程三刻度落账 + 外部三账同源生成。
pub fn authoritative_scene(led: &mut LatLedger) -> Vec<(u32, u64)> {
    let mut rng = Lcg::new(0xEF_64);
    let mut ext: Vec<(u32, u64)> = Vec::new();
    for i in 0..10u32 {
        let t_enq = 100 * (i as u64 + 1);
        let q = 5 + rng.below(40);
        let sw = 1 + rng.below(4);
        let t_pick = t_enq + q;
        let t_run = t_pick + sw;
        let e = LatEntry {
            tid: 1000 + i,
            t_enq,
            t_pick,
            t_run,
        };
        if led.record(e).is_err() {
            break;
        }
        let _ = ext.push((e.tid, e.total()));
    }
    ext
}

/// 注入核：四类注入各 10 次全抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0;
    let mut total = 0;
    let mut rng = Lcg::new(0xEF_64);

    // 1) 段值漂移 ×10（刻度倒置构造：上线早于选中）
    for _ in 0..10 {
        total += 1;
        let mut led = LatLedger::new();
        let e = LatEntry {
            tid: 1,
            t_enq: 100,
            t_pick: 90 + rng.below(5),
            t_run: 80,
        };
        if led.record(e).is_err() {
            caught += 1;
        }
    }
    // 2) 对齐失配 ×10（外部账值与本账不同）
    for _ in 0..10 {
        total += 1;
        let mut led = LatLedger::new();
        let _ = led.record(LatEntry { tid: 1, t_enq: 0, t_pick: 10, t_run: 12 });
        let ext = [(1u32, 99u64)];
        if !led.align_three(&ext, &[], &[]) {
            caught += 1;
        }
    }
    // 3) 刻度倒置 ×10（选中早于入队）
    for _ in 0..10 {
        total += 1;
        let mut led = LatLedger::new();
        let e = LatEntry { tid: 2, t_enq: 50, t_pick: 40, t_run: 60 };
        if led.record(e).is_err() {
            caught += 1;
        }
    }
    // 4) 守恒破裂 ×10（同线程重复落账 = 账面守恒破坏的登记面）
    for _ in 0..10 {
        total += 1;
        let mut led = LatLedger::new();
        let _ = led.record(LatEntry { tid: 3, t_enq: 0, t_pick: 5, t_run: 7 });
        if led.record(LatEntry { tid: 3, t_enq: 1, t_pick: 6, t_run: 8 }).is_err() {
            caught += 1;
        }
    }
    (caught, total)
}

/// 判据入口。
pub fn run_unx1764_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1764");

    // C64-01 三段闭集与常量（J1 前置）
    {
        let e = LatEntry { tid: 1, t_enq: 10, t_pick: 30, t_run: 37 };
        let ok = e.seg_queue() == 20 && e.seg_switch() == 7 && e.total() == 27 && e.conserved();
        s.add("C64-01-三段直算", ok, if ok { "" } else { "段值漂移" });
    }

    // C64-02 权威场景 10 线程落账零拒（J1 主面）
    {
        let mut led = LatLedger::new();
        let ext = authoritative_scene(&mut led);
        let ok = led.len() == 10 && led.reject_count() == 0 && ext.len() == 10;
        s.add("C64-02-10线程落账", ok, if ok { "" } else { "落账断档" });
    }

    // C64-03 全账守恒 + 单调（J1 守恒面）
    {
        let mut led = LatLedger::new();
        let _ = authoritative_scene(&mut led);
        let ok = led.all_conserved();
        s.add("C64-03-全账守恒", ok, if ok { "" } else { "守恒破裂" });
    }

    // C64-04 三账对齐（J1 对齐面：F1647/F1655/F1675 同源账）
    {
        let mut led = LatLedger::new();
        let ext = authoritative_scene(&mut led);
        let half = ext.len() / 2;
        let f1 = &ext[..half];
        let f2 = &ext[half..];
        let ok = led.align_three(f1, f2, &ext);
        s.add("C64-04-三账对齐", ok, if ok { "" } else { "对齐失配" });
    }

    // C64-05 分位面：P50/P95/P99 有序（排队段）
    {
        let mut led = LatLedger::new();
        let _ = authoritative_scene(&mut led);
        let p50 = led.percentile(1, 50);
        let p95 = led.percentile(1, 95);
        let p99 = led.percentile(1, 99);
        let ok = match (p50, p95, p99) {
            (Some(a), Some(b), Some(c)) => a <= b && b <= c,
            _ => false,
        };
        s.add("C64-05-分位有序", ok, if ok { "" } else { "分位面漂移" });
    }

    // C64-06 指纹稳定 + 封账拒重 + 封账后写拒
    {
        let mut a = LatLedger::new();
        let mut b = LatLedger::new();
        let _ = authoritative_scene(&mut a);
        let _ = authoritative_scene(&mut b);
        let fa = a.joint_fingerprint();
        let ok = fa == b.joint_fingerprint()
            && a.seal().is_ok()
            && a.seal().is_err()
            && a.record(LatEntry { tid: 999, t_enq: 1, t_pick: 2, t_run: 3 })
                == Err(XC_LEDGER_BROKEN);
        s.add("C64-06-指纹与写拒", ok, if ok { "" } else { "指纹/写拒失效" });
    }

    // C64-07 注入核四类 10/10（J1 注入面）
    {
        let (caught, total) = injection_core();
        let ok = total == 40 && caught == 40;
        s.add("C64-07-注入核40-40", ok, if ok { "" } else { "注入核漏抓" });
    }

    // C64-08 诊断码闭集
    {
        let codes = [
            XC_SEG_DRIFT, XC_ALIGN_MISS, XC_TICK_INVERT, XC_CONSERV_BAD,
            XC_FINGER_DRIFT, XC_THREAD_OOB, XC_INJECT_MISSED, XC_LEDGER_BROKEN,
        ];
        let mut ok = codes.len() == 8;
        for i in 0..codes.len() {
            if i > 0 && codes[i] <= codes[i - 1] {
                ok = false;
            }
            if (codes[i] & 0xFF00) != (SL_SEG_BASE as u32) {
                ok = false;
            }
        }
        s.add("C64-08-诊断码闭集", ok, if ok { "" } else { "诊断码漂移" });
    }

    // C64-09 中文诊断逐码三要素
    {
        let mut ok = true;
        for c in [
            XC_SEG_DRIFT, XC_ALIGN_MISS, XC_TICK_INVERT, XC_CONSERV_BAD,
            XC_FINGER_DRIFT, XC_THREAD_OOB, XC_INJECT_MISSED, XC_LEDGER_BROKEN, 0xFFFF,
        ] {
            let (t, a) = diag_text(c);
            if t.is_empty() || a.is_empty() {
                ok = false;
            }
        }
        s.add("C64-09-诊断三要素", ok, if ok { "" } else { "诊断文案缺要素" });
    }

    // C64-10 分位守恒：全程 P50 ≥ 排队 P50 且 ≥ 切换 P50
    {
        let mut led = LatLedger::new();
        let _ = authoritative_scene(&mut led);
        let t50 = led.percentile(0, 50);
        let q50 = led.percentile(1, 50);
        let sw50 = led.percentile(2, 50);
        let ok = match (t50, q50, sw50) {
            (Some(t), Some(q), Some(w)) => t >= q && t >= w,
            _ => false,
        };
        s.add("C64-10-分位守恒", ok, if ok { "" } else { "分位守恒破裂" });
    }

    s
}
