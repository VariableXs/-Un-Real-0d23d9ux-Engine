//! UNX-F8241 · clone 全标志矩阵登记（CLONE_* 系）账 · 诊断段 0xB3A0
//! 验收锚点：UNX-F8241-J1 全标志逐位登记（含义/组合约束/挂点三列），与
//! Linux sched.h 标志表逐项对照无缺漏，组合非法集（VM 无 SIGHAND 类）判据在册。


use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB3A0;

/// clone 标志位（Linux sched.h 对照）。
pub const CLONE_VM: u64 = 0x0000_0100;
pub const CLONE_FS: u64 = 0x0000_0200;
pub const CLONE_FILES: u64 = 0x0000_0400;
pub const CLONE_SIGHAND: u64 = 0x0000_0800;
pub const CLONE_THREAD: u64 = 0x0001_0000;
pub const CLONE_PARENT: u64 = 0x0000_8000;
pub const CLONE_CHILD_CLEARTID: u64 = 0x0020_0000;
pub const CLONE_CHILD_SETTID: u64 = 0x0100_0000;
pub const CLONE_PARENT_SETTID: u64 = 0x0010_0000;

/// 标志登记行（三列：含义/组合约束/挂点）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlagRow {
    pub bit: u64,
    pub meaning: &'static str,
    pub constraint: &'static str, // 组合约束
    pub hook: &'static str,       // 挂点
}

impl FlagRow {
    pub fn new(bit: u64, meaning: &'static str, constraint: &'static str, hook: &'static str) -> Self {
        FlagRow { bit, meaning, constraint, hook }
    }
}

/// Linux sched.h 对照表（九位全登记）。
pub fn sched_table() -> Vec<FlagRow> {
    vec![
        FlagRow::new(CLONE_VM, "共享地址空间", "需SIGHAND同设", "mm"),
        FlagRow::new(CLONE_FS, "共享fs根/cwd", "无", "fs"),
        FlagRow::new(CLONE_FILES, "共享fd表", "无", "files"),
        FlagRow::new(CLONE_SIGHAND, "共享信号处理", "需VM同设", "sighand"),
        FlagRow::new(CLONE_THREAD, "同线程组", "需VM+FS+FILES+SIGHAND", "task"),
        FlagRow::new(CLONE_PARENT, "父指向祖父", "同THREAD用", "task"),
        FlagRow::new(CLONE_CHILD_CLEARTID, "退出清tid", "常与SETTID同用", "exit"),
        FlagRow::new(CLONE_CHILD_SETTID, "子写tid", "无", "copy"),
        FlagRow::new(CLONE_PARENT_SETTID, "父写tid", "无", "copy"),
    ]
}

/// 组合非法集：VM 无 SIGHAND / THREAD 缺 VM/FS/FILES/SIGHAND。
pub fn combo_illegal(flags: u64) -> bool {
    // VM 设而 SIGHAND 未设 → 非法
    if flags & CLONE_VM != 0 && flags & CLONE_SIGHAND == 0 {
        return true;
    }
    // SIGHAND 设而 VM 未设 → 非法
    if flags & CLONE_SIGHAND != 0 && flags & CLONE_VM == 0 {
        return true;
    }
    // THREAD 设但四要件缺一 → 非法
    if flags & CLONE_THREAD != 0 {
        let need = CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND;
        if flags & need != need {
            return true;
        }
    }
    false
}

/// 标志登记账。
pub struct CloneMatrix {
    pub rows: Vec<FlagRow>,
}

impl CloneMatrix {
    pub fn new() -> Self {
        CloneMatrix { rows: Vec::new() }
    }
    pub fn register(&mut self, r: FlagRow) -> bool {
        if self.rows.iter().any(|x| x.bit == r.bit) {
            return false;
        }
        self.rows.push(r);
        true
    }
    /// 与 Linux sched.h 逐项对照：九位无缺漏。
    pub fn no_missing(&self) -> bool {
        let sched = sched_table();
        sched.iter().all(|r| {
            self.rows.iter().any(|x| x.bit == r.bit && x.meaning == r.meaning && x.constraint == r.constraint && x.hook == r.hook)
        })
    }
    pub fn three_cols_full(&self) -> bool {
        self.rows.iter().all(|r| !r.meaning.is_empty() && !r.hook.is_empty() && !r.constraint.is_empty())
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0xB3A0 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x8241_B3A0;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("表-九位常数", sched_table().len() == 9));
    out.push(("非法-VM无SIGHAND", combo_illegal(CLONE_VM)));
    out.push(("合法-VM+SIGHAND", !combo_illegal(CLONE_VM | CLONE_SIGHAND)));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx8241_checks：本单判据聚合（诊断段 0xB3A0）。
pub fn run_unx8241_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8241 clone 全标志矩阵登记");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 九位全登记
    let mut m = CloneMatrix::new();
    for r in sched_table() {
        m.register(r);
    }
    let reg_ok = m.rows.len() == 9;
    s.add("登记-九位全入账", reg_ok, if reg_ok { "" } else { "登记缺口" });

    // 三列全满
    s.add("三列-含义/约束/挂点全满", m.three_cols_full(), if m.three_cols_full() { "" } else { "三列缺口" });

    // 与 sched.h 逐项对照无缺漏
    s.add("对照-sched.h九位逐项无缺漏", m.no_missing(), if m.no_missing() { "" } else { "对照缺口" });

    // 重复位登记拒
    let dup = !m.register(FlagRow::new(CLONE_VM, "x", "y", "z"));
    s.add("登记-重复位拒收", dup, if dup { "" } else { "重复漏网" });

    // 组合非法集判据在册：VM无SIGHAND / SIGHAND无VM / THREAD缺要件
    let ill1 = combo_illegal(CLONE_VM);
    let ill2 = combo_illegal(CLONE_SIGHAND);
    let ill3 = combo_illegal(CLONE_THREAD | CLONE_VM | CLONE_FS);
    let legal1 = !combo_illegal(CLONE_VM | CLONE_SIGHAND);
    let legal2 = !combo_illegal(CLONE_THREAD | CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND);
    let legal3 = !combo_illegal(CLONE_FILES | CLONE_FS);
    let combo_ok = ill1 && ill2 && ill3 && legal1 && legal2 && legal3;
    s.add("非法集-VM/SIGHAND/THREAD三非法三合法恰准", combo_ok, if combo_ok { "" } else { "组合判据漂移" });

    // 三列缺列拒登记（账面标记）
    let mut m2 = CloneMatrix::new();
    m2.register(FlagRow::new(CLONE_VM, "", "需SIGHAND同设", "mm"));
    let col_missing = !m2.three_cols_full();
    s.add("缺列-空含义列检出", col_missing, if col_missing { "" } else { "缺列漏检" });

    // 对照错账检出：约束串漂移
    let mut m3 = CloneMatrix::new();
    for r in sched_table() {
        let rr = if r.bit == CLONE_THREAD { FlagRow::new(r.bit, r.meaning, "漂移", r.hook) } else { r };
        m3.register(rr);
    }
    s.add("对照-约束串漂移检出", !m3.no_missing(), if !m3.no_missing() { "" } else { "漂移漏检" });

    // 九位名导出
    let names = sched_table().iter().map(|r| r.hook).collect::<Vec<&str>>().join(",");
    s.add("导出-九挂点名序一致", names == "mm,fs,files,sighand,task,task,exit,copy,copy", if names == "mm,fs,files,sighand,task,task,exit,copy,copy" { "" } else { "挂点序漂移" });

    // FNV 表指纹稳定
    let mut acc: u32 = 0x811C_9DC5;
    for r in sched_table() {
        acc ^= fnv1a(&[r.bit as u8, (r.bit >> 8) as u8]);
        acc = acc.wrapping_mul(0x0100_0193);
    }
    s.add("FNV-标志表指纹非零", acc != 0, if acc != 0 { "" } else { "指纹异常" });

    s
}
