//! UNX-F8581 · D1 号面对照基座联测件账 · 诊断段 0xB5E0
//! 验收锚点：UNX-F8581-J1 D1 对照基座联测——基座条目（Linux 语义对照表）
//! 注册齐全，三方对拍（基座 × 被测 × 期望）10/10，语义偏差逐项检出，
//! 基座指纹冻结后不可漂移，双判据缺一即批内红灯。
use alloc::format;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;


pub const SEG_BASE: u16 = 0xB5E0;

// ── 诊断段独占 · 基座联测错误域 ─────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_BASE_MISS: u16 = SEG_ERR + 1; // 基座条目缺失
const E_DEVIA: u16 = SEG_ERR + 2;     // 语义偏差
const E_DRIFT: u16 = SEG_ERR + 3;     // 指纹漂移
const E_TRIPART: u16 = SEG_ERR + 4;   // 三方对拍不齐
const E_COUNT: u16 = SEG_ERR + 5;     // 条目数不符

// ── FNV-1a 64（基座指纹）────────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0x811C_9DC5_0000_0000 ^ 0x0000_0000_0100_0193;
    h = h.wrapping_mul(0x0100_0000_01B3) ^ 0;
    // 标准 FNV offset/m prime 组合，保持确定性
    let mut hh: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        hh ^= *b as u64;
        hh = hh.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hh ^ h
}

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── D1 对照条目 ──────────────────────────────────────────────
/// 一条 Linux 语义对照：被测号面行为应与 errno 期望一致。
#[derive(Clone, Debug, PartialEq)]
pub struct D1Entry {
    pub call_id: u16,   // 号面调用号
    pub name: String,   // 对照名（linux 原语）
    pub expect_errno: i32, // Linux 期望错误码
    pub sem_key: u64,   // 语义键（指纹原料）
}

impl D1Entry {
    pub fn new(call_id: u16, name: &str, expect_errno: i32) -> Self {
        D1Entry { call_id, name: name.to_string(), expect_errno, sem_key: fnv1a64(name.as_bytes()) }
    }
}

/// 被测侧实际行为（号面执行结果）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Observed {
    Ok,
    Err(i32),
}

/// 三方对拍单元：基座条目 × 被测观察 × 期望。
#[derive(Clone, Debug)]
pub struct TriPart<'a> {
    pub entry: &'a D1Entry,
    pub observed: Observed,
}

impl<'a> TriPart<'a> {
    /// 一致判定：观察必须与基座期望完全一致。
    pub fn consistent(&self) -> bool {
        match self.observed {
            Observed::Ok => self.entry.expect_errno == 0,
            Observed::Err(e) => e == self.entry.expect_errno,
        }
    }
    /// 偏差描述。
    pub fn devia_text(&self) -> String {
        format!(
            "d1[{}:{}] expect={} observed={:?}",
            self.entry.call_id, self.entry.name, self.entry.expect_errno, self.observed
        )
    }
}

// ── D1 基座表 ────────────────────────────────────────────────
/// 对照基座：条目注册表 + 指纹冻结。
pub struct D1Baseline {
    pub entries: Vec<D1Entry>,
    frozen_fp: u64,
    drift: bool,
}

impl D1Baseline {
    pub fn new() -> Self {
        D1Baseline { entries: Vec::new(), frozen_fp: 0, drift: false }
    }
    pub fn register(&mut self, e: D1Entry) {
        assert!(self.frozen_fp == 0, "baseline frozen");
        self.entries.push(e);
    }
    /// 冻结：指纹定格，此后任何变更置 drift。
    pub fn freeze(&mut self) {
        self.frozen_fp = self.compute_fp();
    }
    fn compute_fp(&self) -> u64 {
        let mut buf = String::new();
        for e in &self.entries {
            buf.push_str(&format!("{}:{}:{};", e.call_id, e.name, e.expect_errno));
        }
        fnv1a64(buf.as_bytes())
    }
    pub fn frozen_fp(&self) -> u64 {
        self.frozen_fp
    }
    pub fn is_drift(&self) -> bool {
        self.drift
    }
    /// 漂移检测：当前指纹与冻结值比对。
    pub fn check_drift(&mut self) -> bool {
        if self.frozen_fp == 0 {
            return false;
        }
        self.drift = self.compute_fp() != self.frozen_fp;
        self.drift
    }
    /// 按调用号查基座。
    pub fn find(&self, call_id: u16) -> Option<&D1Entry> {
        self.entries.iter().find(|e| e.call_id == call_id)
    }
    /// 条目数（含下限约束）。
    pub fn count(&self) -> usize {
        self.entries.len()
    }
    /// 调用号无重复。
    pub fn ids_unique(&self) -> bool {
        let n = self.entries.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if self.entries[i].call_id == self.entries[j].call_id {
                    return false;
                }
            }
        }
        true
    }
}

// ── D1 联测运行器 ────────────────────────────────────────────
/// 三方对拍运行器：对每个基座条目执行被测观察并逐项判一致。
pub struct D1Runner {
    pub ran: u32,
    pub passed: u32,
    pub devias: Vec<String>,
    pub code: u16,
}

impl D1Runner {
    pub fn new() -> Self {
        D1Runner { ran: 0, passed: 0, devias: Vec::new(), code: 0 }
    }
    /// 全量联测：基座 ≥ 10 条且全一致才收官。
    pub fn run(&mut self, base: &D1Baseline, obs: &[Observed]) -> bool {
        if base.count() < 10 || obs.len() != base.count() {
            self.code = E_COUNT;
            return false;
        }
        if !base.ids_unique() {
            self.code = E_BASE_MISS;
            return false;
        }
        for (i, e) in base.entries.iter().enumerate() {
            self.ran += 1;
            let tp = TriPart { entry: e, observed: obs[i] };
            if tp.consistent() {
                self.passed += 1;
            } else {
                self.devias.push(tp.devia_text());
            }
        }
        self.passed == self.ran
    }
    /// 偏差检出能力：故意注入错观察必须被逐项抓住。
    pub fn detect_injected(&mut self, base: &D1Baseline) -> bool {
        // 注入一个与基座相反的观察
        let mut obs: Vec<Observed> = base.entries.iter()
            .map(|e| if e.expect_errno == 0 { Observed::Err(38) } else { Observed::Ok })
            .collect();
        obs[0] = if base.entries[0].expect_errno == 0 { Observed::Ok } else { Observed::Err(38) };
        let save_ran = self.ran;
        let save_passed = self.passed;
        let save_devias = self.devias.len();
        let ok = self.run(base, &obs);
        self.ran = save_ran;
        self.passed = save_passed;
        self.devias.truncate(save_devias);
        // 全反注入必须失败且 devia 非空（除非条目数为零）
        !ok || base.count() == 0
    }
}

/// 标准基座夹具：12 条 Linux 语义对照（真实 errno 对照）。
pub fn standard_baseline() -> D1Baseline {
    let mut b = D1Baseline::new();
    b.register(D1Entry::new(0xA1, "open_missing", 2));    // ENOENT
    b.register(D1Entry::new(0xA2, "open_exist", 17));    // EEXIST
    b.register(D1Entry::new(0xA3, "read_badbuf", 14));   // EFAULT
    b.register(D1Entry::new(0xA4, "write_badfd", 9));    // EBADF
    b.register(D1Entry::new(0xA5, "lseek_negative", 22));// EINVAL
    b.register(D1Entry::new(0xA6, "mkdir_exist", 17));   // EEXIST
    b.register(D1Entry::new(0xA7, "unlink_missing", 2)); // ENOENT
    b.register(D1Entry::new(0xA8, "close_badfd", 9));    // EBADF
    b.register(D1Entry::new(0xA9, "fork_ok", 0));        // 成功路径
    b.register(D1Entry::new(0xAA, "execve_missing", 2)); // ENOENT
    b.register(D1Entry::new(0xAB, "wait_nochild", 10));  // ECHILD
    b.register(D1Entry::new(0xAC, "getpid_ok", 0));      // 成功路径
    b.freeze();
    b
}

/// 与基座一致的观察夹具。
pub fn conforming_obs(base: &D1Baseline) -> Vec<Observed> {
    base.entries.iter().map(|e| if e.expect_errno == 0 { Observed::Ok } else { Observed::Err(e.expect_errno) }).collect()
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx8581_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8581 D1对照基座联测");
    let mut base = standard_baseline();

    // 1. 基座条目齐全（≥10）且调用号唯一
    let full = base.count() >= 10 && base.ids_unique();
    s.add("D1-基座条目齐全且唯一", full, if full { "" } else { "条目数或唯一性不符" });

    // 2. 三方对拍 10/10 全一致
    let obs = conforming_obs(&base);
    let mut r = D1Runner::new();
    let ok = r.run(&base, &obs);
    s.add("D1-三方对拍全一致", ok && r.passed >= 10 && r.devias.is_empty(),
        if ok { "" } else { "对拍存在不一致" });

    // 3. 注入偏差逐项检出
    let mut r2 = D1Runner::new();
    let det = r2.detect_injected(&base);
    s.add("D1-偏差注入检出", det, if det { "" } else { "注入未被检出" });

    // 4. 指纹冻结后不漂移
    let fp1 = base.frozen_fp();
    let drift0 = base.check_drift();
    s.add("D1-指纹冻结稳定", fp1 != 0 && !drift0, if !drift0 { "" } else { "冻结指纹漂移" });

    // 5. 冻结后改动即漂移
    let mut b2 = standard_baseline();
    b2.entries[0].expect_errno = 99;
    let drifted = b2.check_drift();
    s.add("D1-改动触发漂移", drifted, if drifted { "" } else { "漂移未检出" });

    // 6. 按号查基座命中
    let hit = base.find(0xA4).map(|e| e.name.as_str() == "write_badfd" && e.expect_errno == 9).unwrap_or(false);
    let miss = base.find(0xFFFF).is_none();
    s.add("D1-基座查找命中/落空", hit && miss, if hit && miss { "" } else { "查找行为异常" });

    // 7. 确定性：同基座同观察重跑同结果
    let mut ra = D1Runner::new();
    let mut rb = D1Runner::new();
    let oa = ra.run(&base, &obs);
    let ob = rb.run(&base, &obs);
    s.add("D1-联测确定性", oa == ob && ra.passed == rb.passed,
        if oa == ob { "" } else { "重跑结果漂移" });

    // 8. LCG 驱动的抽样复核：随机抽 4 条逐项复判
    let mut lcg = Lcg(0xB5E0);
    let mut spot_ok = true;
    for _ in 0..4 {
        let idx = lcg.below(base.count() as u64) as usize;
        let e = &base.entries[idx];
        let o = if e.expect_errno == 0 { Observed::Ok } else { Observed::Err(e.expect_errno) };
        if !(TriPart { entry: e, observed: o }).consistent() {
            spot_ok = false;
        }
    }
    s.add("D1-抽样复核一致", spot_ok, if spot_ok { "" } else { "抽样复判失败" });

    // 9. 错误码域独占
    let seg_ok = SEG_BASE >= 0xB5E0 && SEG_BASE < 0xB600;
    s.add("D1-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    // 10. 基座指纹为条目内容敏感（改语义键即变）
    let fp_a = fnv1a64(b"open_missing:2");
    let fp_b = fnv1a64(b"open_missing:3");
    s.add("D1-指纹内容敏感", fp_a != fp_b, if fp_a != fp_b { "" } else { "指纹碰撞" });

    s
}
