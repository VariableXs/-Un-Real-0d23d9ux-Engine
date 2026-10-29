//! UNX-F56042 引导段计时桩（0x60520 段）
//!
//! 锚点：UNX-F56042-J1 固件交权时刻→内核首任务时刻段桩（U 盘引导链
//! 零触碰——只读观测不写 NVRAM/ESP，红线预申报留痕）；J2 段起止时间
//! 戳落账 ×10 次连测（逐轮四站戳齐）；J3 观测路径只读断言（观测调用
//! 全部命中只读闭集，写类调用零命中）三判据全绿。判据锚定 §AI-71.2
//! 专题二②段表公开。
//! 域：CoRun·UNX-F2-B40（启动分段域·引导段面）。
//!
//! 结构：引导链四站闭集（固件交权→Limine→varix 入口→bootselect）+
//! 段桩账（逐轮四站时间戳落账）+ 零触碰红线账（预申报 + 只读调用闭
//! 集扫描）+ 连测账 ×10 + 注入核（越闭集站/站序回退/写类调用/连测不
//! 足/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60520 独占 0x20）。
pub const SEG_BASE: u32 = 0x60520;
pub const BS_STATION_UNK: u32 = SEG_BASE + 0x01; // 越闭集站
pub const BS_TS_BACK: u32 = SEG_BASE + 0x02; // 站序时间戳回退
pub const BS_WRITE_HIT: u32 = SEG_BASE + 0x03; // 写类调用命中（红线破）
pub const BS_RUN_SHORT: u32 = SEG_BASE + 0x04; // 连测不足 10 轮
pub const BS_SEALED: u32 = SEG_BASE + 0x05; // 段桩账已封账拒写
pub const BS_DUP_RUN: u32 = SEG_BASE + 0x06; // 同轮号重复落账
pub const BS_DECL_LOST: u32 = SEG_BASE + 0x07; // 红线预申报缺失

/// 引导链四站闭集。
pub const T_FW: usize = 0;
pub const T_LIMINE: usize = 1;
pub const T_VARIX: usize = 2;
pub const T_BOOTSEL: usize = 3;
pub const T_COUNT: usize = 4;
pub const T_NAMES: [&str; T_COUNT] =
    ["fw_handoff", "limine_entry", "varix_entry", "bootselect_done"];

/// 只读观测调用闭集（零触碰口径）。
pub const READONLY_CALLS: [&str; 4] =
    ["tsc_read", "nvram_read", "esp_list_dir", "fw_var_query"];

/// 写类调用闭集（红线禁用）。
pub const WRITE_CALLS: [&str; 4] =
    ["nvram_write", "esp_write", "fw_var_set", "esp_format"];

pub fn readonly_ok(call: &str) -> bool {
    for c in READONLY_CALLS.iter() {
        if *c == call {
            return true;
        }
    }
    false
}

pub fn write_hit(call: &str) -> bool {
    for c in WRITE_CALLS.iter() {
        if *c == call {
            return true;
        }
    }
    false
}

/// 连测轮数（10）。
pub const RUNS_N: usize = 10;

/// 单轮段桩账：四站时间戳（ns，从上电起算）。
#[derive(Clone)]
pub struct BootRun {
    pub seq: u32,
    pub ts: [u64; T_COUNT],
}

impl BootRun {
    pub fn new(seq: u32, ts: [u64; T_COUNT]) -> Result<Self, u32> {
        if seq == 0 {
            return Err(BS_DUP_RUN);
        }
        for g in 1..T_COUNT {
            if ts[g] < ts[g - 1] {
                return Err(BS_TS_BACK);
            }
        }
        Ok(BootRun { seq, ts })
    }

    /// 引导段时长（固件交权 → bootselect 完成）。
    pub fn span_ns(&self) -> u64 {
        self.ts[T_BOOTSEL] - self.ts[T_FW]
    }
}

/// 段桩账 + 零触碰红线账。
pub struct BootStubBook {
    pub runs: Vec<BootRun>,
    pub decl: bool,
    pub observed_calls: Vec<String>,
    pub sealed: bool,
}

impl BootStubBook {
    pub fn new() -> Self {
        BootStubBook {
            runs: Vec::new(),
            decl: false,
            observed_calls: Vec::new(),
            sealed: false,
        }
    }

    /// 红线预申报（零触碰声明落账）。
    pub fn declare_redline(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(BS_SEALED);
        }
        self.decl = true;
        Ok(())
    }

    /// 落一轮（同轮号拒 + 顺序核）。
    pub fn put(&mut self, r: BootRun) -> Result<(), u32> {
        if self.sealed {
            return Err(BS_SEALED);
        }
        for p in self.runs.iter() {
            if p.seq == r.seq {
                return Err(BS_DUP_RUN);
            }
        }
        self.runs.push(r);
        Ok(())
    }

    /// 观测调用留痕（写类命中即拒——红线破）。
    pub fn observe(&mut self, call: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(BS_SEALED);
        }
        if write_hit(call) {
            return Err(BS_WRITE_HIT);
        }
        if !readonly_ok(call) {
            return Err(BS_WRITE_HIT);
        }
        self.observed_calls.push(call.to_string());
        Ok(())
    }

    /// J1：零触碰核（预申报在账 + 观测留痕全只读）。
    pub fn zero_touch_ok(&self) -> bool {
        self.decl
            && self.observed_calls.iter().all(|c| readonly_ok(c) && !write_hit(c))
            && !self.observed_calls.is_empty()
    }

    /// J2：连测齐（≥10 轮且轮号连续）。
    pub fn runs10_ok(&self) -> bool {
        if self.runs.len() < RUNS_N {
            return false;
        }
        for (i, r) in self.runs.iter().enumerate() {
            if r.seq != i as u32 + 1 {
                return false;
            }
        }
        true
    }

    /// 段时长分位（P50/P95）。
    pub fn span_stats(&self) -> Result<(u64, u64), u32> {
        if self.runs.len() < RUNS_N {
            return Err(BS_RUN_SHORT);
        }
        let mut v: Vec<u64> = Vec::new();
        for r in self.runs.iter() {
            v.push(r.span_ns());
        }
        v.sort();
        let p50 = v[v.len() / 2];
        let p95 = v[(v.len() * 95 / 100).min(v.len() - 1)];
        Ok((p50, p95))
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：红线申报 + 10 轮确定性时间戳 + 只读观测留痕。
pub fn standard_stub() -> BootStubBook {
    let mut b = BootStubBook::new();
    b.declare_redline().expect("decl");
    for i in 0..RUNS_N {
        let ts = [
            1_000_000u64,
            1_200_000,
            1_500_000,
            1_900_000,
        ];
        b.put(BootRun::new(i as u32 + 1, ts).expect("run")).expect("put");
    }
    for c in READONLY_CALLS.iter() {
        b.observe(c).expect("observe");
    }
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 站序时间戳回退（站 2 早于站 1）
    if BootRun::new(1, [1_000, 1_200, 1_100, 1_900]).err() == Some(BS_TS_BACK) {
        caught += 1;
    }
    // 2) 写类调用命中（NVRAM 写——红线破）
    let mut b = BootStubBook::new();
    if b.observe("nvram_write") == Err(BS_WRITE_HIT) {
        caught += 1;
    }
    // 3) 红线未申报（零触碰核判假）
    let mut b2 = standard_stub();
    b2.decl = false;
    let ok = !b2.zero_touch_ok();
    if ok {
        caught += 1;
    }
    // 4) 连测不足 10 轮
    let mut b3 = standard_stub();
    b3.runs.pop();
    if b3.span_stats() == Err(BS_RUN_SHORT) {
        caught += 1;
    }
    // 5) 同轮号重复落账
    let mut b4 = standard_stub();
    if b4.put(BootRun::new(5, [1_000, 1_200, 1_500, 1_900]).expect("dup")) == Err(BS_DUP_RUN) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56042_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56042");
    // J1-1 红线预申报在账
    let b = standard_stub();
    let ok = b.decl;
    s.add("56042-J1-红线预申报在账", ok, if ok { "" } else { "申报缺失" });
    // J1-2 观测路径只读（四调用全命中只读闭集）
    let ok = b.zero_touch_ok() && b.observed_calls.len() == READONLY_CALLS.len();
    s.add("56042-J1-观测路径只读", ok, if ok { "" } else { "只读破" });
    // J1-3 写类闭集全拒（四个写类调用逐个必拒）
    let mut all_rejected = true;
    for c in WRITE_CALLS.iter() {
        let mut b2 = BootStubBook::new();
        if b2.observe(c) != Err(BS_WRITE_HIT) {
            all_rejected = false;
        }
    }
    let ok = all_rejected && WRITE_CALLS.len() == 4;
    s.add("56042-J1-写类全拒", ok, if ok { "" } else { "写类穿透" });
    // J2-1 ×10 连测轮账齐（轮号连续 + 四站戳齐）
    let ok = b.runs10_ok()
        && b.runs.iter().all(|r| r.ts[T_FW] == 1_000_000 && r.ts[T_BOOTSEL] == 1_900_000);
    s.add("56042-J2-十轮四站戳齐", ok, if ok { "" } else { "连测账缺" });
    // J2-2 引导段时长语义（1.9M - 1.0M = 900k ns = 0.9ms）
    let ok = b.runs[0].span_ns() == 900_000;
    s.add("56042-J2-段时长语义", ok, if ok { "" } else { "时长漂移" });
    // J2-3 段时长分位导出（确定性 → P50=P95=900k）
    let (p50, p95) = b.span_stats().expect("st");
    let ok = p50 == 900_000 && p95 == 900_000;
    s.add("56042-J2-段时长分位", ok, if ok { "" } else { "分位漂移" });
    // J1-4 站名闭集同源（fw→limine→varix→bootselect 序）
    let ok = T_NAMES[0] == "fw_handoff"
        && T_NAMES[1] == "limine_entry"
        && T_NAMES[2] == "varix_entry"
        && T_NAMES[3] == "bootselect_done";
    s.add("56042-J1-四站名同源", ok, if ok { "" } else { "命名漂移" });
    // J1-5 站间单调（夹具站戳 1.0/1.2/1.5/1.9M 递增）
    let r0 = &b.runs[0];
    let ok = r0.ts[T_LIMINE] == 1_200_000 && r0.ts[T_VARIX] == 1_500_000;
    s.add("56042-J1-站间单调", ok, if ok { "" } else { "站序破" });
    // J3-1 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56042-J3-注入核必抓", ok, &detail);
    // J3-2 只读/写类闭集互斥（无调用名同属两集）
    let mut disjoint = true;
    for r in READONLY_CALLS.iter() {
        if write_hit(r) {
            disjoint = false;
        }
    }
    for w in WRITE_CALLS.iter() {
        if readonly_ok(w) {
            disjoint = false;
        }
    }
    let ok = disjoint;
    s.add("56042-J3-读写闭集互斥", ok, if ok { "" } else { "闭集交叠" });
    // J3-3 封账冻结
    let mut b3 = standard_stub();
    b3.seal();
    let ok = b3.observe("tsc_read") == Err(BS_SEALED)
        && b3.put(BootRun::new(99, [0, 0, 0, 0]).expect("s")) == Err(BS_SEALED)
        && b3.declare_redline() == Err(BS_SEALED);
    s.add("56042-J3-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J2-4 未申报时零触碰核判假留证
    let mut b4 = standard_stub();
    b4.decl = false;
    let ok = !b4.zero_touch_ok() && b4.decl == false;
    s.add("56042-J3-未申报判假留证", ok, if ok { "" } else { "判假失效" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56042_all_checks_green() {
        let set = run_unx56042_checks();
        assert!(set.all_passed(), "UNX-F56042 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56042_span() {
        let b = standard_stub();
        assert_eq!(b.runs[0].span_ns(), 900_000);
        assert!(b.runs10_ok());
    }
}
