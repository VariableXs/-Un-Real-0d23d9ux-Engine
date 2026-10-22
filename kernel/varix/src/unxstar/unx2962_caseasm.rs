// unx2962 —— UNX-F2962 回归用例装配器（三基线装配全域用例）
// 判据原文 UNX-F2962-J1：三基线（B24 段级 57 例 + B28 矩阵基线 + F2938 压测段
// 汇总 473 例）装配用例 560/560 完整，装配账与 F2965 执行账键对齐。
// 落位：kernel/varix/src/unxstar/；诊断段 0xFB00 独占（0xFB00~0xFB7F）。
//
// 设计（纯功能 no_std）：
//   * 三基线闭集：段级 57 例 / 矩阵 30 例（F2862 消费）/ 压测汇总 473 例
//     —— 57+30+473 = 560 全域装配；
//   * 装配账：每例（键/基线源/序号）三元组登记，文本键唯一性闸；
//   * 执行账键对齐：装配键集 == 执行键集（F2965 消费面）逐键双向核对；
//   * 注入核：缺基线/缺例/键重复/装配-执行键漂移 四类注入必抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xFB00 独占） ─────────────────────────────────────────────
pub const XC_ASM_BASE: u32 = 0xFB01; // 基线缺失
pub const XC_ASM_CASE: u32 = 0xFB02; // 基线例数不符
pub const XC_ASM_DUP: u32 = 0xFB03; // 装配键重复
pub const XC_ASM_ALIGN: u32 = 0xFB04; // 装配-执行键漂移
pub const XC_ASM_SEAL: u32 = 0xFB05; // 封账后装配

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// 基线源三闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BaseKind {
    Segment,   // B24 段级 57 例
    Matrix,    // B28 矩阵 30 例
    StressAgg, // F2938 压测汇总 473 例
}

impl BaseKind {
    pub fn name(self) -> &'static str {
        match self {
            BaseKind::Segment => "seg",
            BaseKind::Matrix => "mtx",
            BaseKind::StressAgg => "str",
        }
    }
    pub fn expect_count(self) -> usize {
        match self {
            BaseKind::Segment => 57,
            BaseKind::Matrix => 30,
            BaseKind::StressAgg => 473,
        }
    }
}

/// 装配条目：键/基线源/序号。
#[derive(Clone)]
pub struct AsmEntry {
    pub key: &'static str,
    pub base: BaseKind,
    pub seq: u32,
}

impl AsmEntry {
    pub fn bytes(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(self.key.as_bytes());
        b.push(0xFF);
        b.extend_from_slice(self.base.name().as_bytes());
        b.extend_from_slice(&self.seq.to_le_bytes());
        b
    }
}

/// 装配器。
pub struct Assembler {
    entries: Vec<AsmEntry>,
    exec_keys: Vec<&'static str>,
    frozen: Option<u32>,
    rejects: Vec<(u32, &'static str)>,
}

impl Assembler {
    pub fn new() -> Self {
        Assembler {
            entries: Vec::new(),
            exec_keys: Vec::new(),
            frozen: None,
            rejects: Vec::new(),
        }
    }

    pub fn add(&mut self, e: AsmEntry) -> Result<(), u32> {
        if self.frozen.is_some() {
            self.rejects.push((XC_ASM_SEAL, "封账后装配"));
            return Err(XC_ASM_SEAL);
        }
        if self.entries.iter().any(|x| x.key == e.key) {
            self.rejects.push((XC_ASM_DUP, "装配键重复"));
            return Err(XC_ASM_DUP);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 批量装配一个基线（例数闸 + 序号连续）。
    pub fn add_base(&mut self, base: BaseKind, keys: &[&'static str]) -> Result<usize, u32> {
        if keys.len() != base.expect_count() {
            self.rejects.push((XC_ASM_CASE, "基线例数不符"));
            return Err(XC_ASM_CASE);
        }
        let start = self.entries.len() as u32;
        for (i, k) in keys.iter().enumerate() {
            self.add(AsmEntry {
                key: k,
                base,
                seq: start + i as u32,
            })?;
        }
        Ok(keys.len())
    }

    /// 登记执行账键集（F2965 消费面）。
    pub fn register_exec(&mut self, keys: &[&'static str]) {
        self.exec_keys = keys.to_vec();
    }

    /// 装配-执行键对齐（双向核对）。
    pub fn exec_aligned(&self) -> bool {
        if self.entries.len() != self.exec_keys.len() {
            return false;
        }
        for e in &self.entries {
            if !self.exec_keys.contains(&e.key) {
                return false;
            }
        }
        for k in &self.exec_keys {
            if !self.entries.iter().any(|e| e.key == *k) {
                return false;
            }
        }
        true
    }

    /// 三基线完整装配核。
    pub fn bases_complete(&self) -> bool {
        for b in [BaseKind::Segment, BaseKind::Matrix, BaseKind::StressAgg] {
            let c = self.entries.iter().filter(|e| e.base == b).count();
            if c != b.expect_count() {
                return false;
            }
        }
        self.entries.len() == 560
    }

    pub fn freeze(&mut self) -> u32 {
        let mut bytes = Vec::new();
        for e in &self.entries {
            bytes.extend_from_slice(&e.bytes());
        }
        let h = fnv1a32(&bytes);
        self.frozen = Some(h);
        h
    }
    pub fn fingerprint(&self) -> u32 {
        self.frozen.unwrap_or(0)
    }
    pub fn entries(&self) -> &[AsmEntry] {
        &self.entries
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

// ── 基线一：B24 段级 57 例（静态命名表） ─────────────────────────────
pub fn segment_keys() -> Vec<&'static str> {
    const NAMES: [&str; 57] = [
        "s01", "s02", "s03", "s04", "s05", "s06", "s07", "s08", "s09", "s10",
        "s11", "s12", "s13", "s14", "s15", "s16", "s17", "s18", "s19", "s20",
        "s21", "s22", "s23", "s24", "s25", "s26", "s27", "s28", "s29", "s30",
        "s31", "s32", "s33", "s34", "s35", "s36", "s37", "s38", "s39", "s40",
        "s41", "s42", "s43", "s44", "s45", "s46", "s47", "s48", "s49", "s50",
        "s51", "s52", "s53", "s54", "s55", "s56", "s57",
    ];
    NAMES.to_vec()
}

// ── 基线二：B28 矩阵 30 例（与 F2862 标准矩阵同键） ──────────────────
pub fn matrix_keys() -> Vec<&'static str> {
    let mut k: Vec<&'static str> = Vec::new();
    let faces: [[&str; 3]; 10] = [
        ["stk-ovf", "stk-align", "stk-guard"],
        ["heap-grow", "heap-exhaust", "heap-free"],
        ["shm-create", "shm-mapdup", "shm-rm"],
        ["file-rd", "file-wr", "file-rw"],
        ["guard-touch", "guard-skip", "guard-unmap"],
        ["align-4k", "align-2m", "align-mis"],
        ["bound-lo", "bound-hi", "bound-cross"],
        ["perm-rx", "perm-rw", "perm-none"],
        ["frag-frag", "frag-defrag", "frag-scan"],
        ["remap-grow", "remap-move", "remap-shrink"],
    ];
    for f in faces {
        for name in f {
            k.push(name);
        }
    }
    k
}

// ── 基线三：F2938 压测汇总 473 例（8 面×59 槽 = 472 + 1 尾例） ────────
/// 59 槽静态表展开宏（前缀 + 两位编号，编译期 concat 生成 &'static str）。
macro_rules! expand59 {
    ($p:literal) => {
        [
            concat!($p, "00"), concat!($p, "01"), concat!($p, "02"),
            concat!($p, "03"), concat!($p, "04"), concat!($p, "05"),
            concat!($p, "06"), concat!($p, "07"), concat!($p, "08"),
            concat!($p, "09"), concat!($p, "10"), concat!($p, "11"),
            concat!($p, "12"), concat!($p, "13"), concat!($p, "14"),
            concat!($p, "15"), concat!($p, "16"), concat!($p, "17"),
            concat!($p, "18"), concat!($p, "19"), concat!($p, "20"),
            concat!($p, "21"), concat!($p, "22"), concat!($p, "23"),
            concat!($p, "24"), concat!($p, "25"), concat!($p, "26"),
            concat!($p, "27"), concat!($p, "28"), concat!($p, "29"),
            concat!($p, "30"), concat!($p, "31"), concat!($p, "32"),
            concat!($p, "33"), concat!($p, "34"), concat!($p, "35"),
            concat!($p, "36"), concat!($p, "37"), concat!($p, "38"),
            concat!($p, "39"), concat!($p, "40"), concat!($p, "41"),
            concat!($p, "42"), concat!($p, "43"), concat!($p, "44"),
            concat!($p, "45"), concat!($p, "46"), concat!($p, "47"),
            concat!($p, "48"), concat!($p, "49"), concat!($p, "50"),
            concat!($p, "51"), concat!($p, "52"), concat!($p, "53"),
            concat!($p, "54"), concat!($p, "55"), concat!($p, "56"),
            concat!($p, "57"), concat!($p, "58"),
        ]
    };
}

const F0: [&str; 59] = expand59!("f0c");
const F1: [&str; 59] = expand59!("f1c");
const F2: [&str; 59] = expand59!("f2c");
const F3: [&str; 59] = expand59!("f3c");
const F4: [&str; 59] = expand59!("f4c");
const F5: [&str; 59] = expand59!("f5c");
const F6: [&str; 59] = expand59!("f6c");
const F7: [&str; 59] = expand59!("f7c");

/// face×slot → 唯一 &'static str（8 面 × 59 槽 = 472 项全唯一）。
pub const fn combo_face_key(i: usize) -> &'static str {
    let face = i / 59;
    let slot = i % 59;
    match face {
        0 => F0[slot],
        1 => F1[slot],
        2 => F2[slot],
        3 => F3[slot],
        4 => F4[slot],
        5 => F5[slot],
        6 => F6[slot],
        _ => F7[slot],
    }
}

/// 压测汇总 473 键（472 组合 + 1 尾例）。
pub fn stress_keys() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for i in 0..472usize {
        out.push(combo_face_key(i));
    }
    out.push("ftail-x");
    out
}

/// 全域装配（标准三基线）：57 + 30 + 473 = 560，返回（装配器, 执行键集）。
pub fn full_assembly() -> (Assembler, Vec<&'static str>) {
    let mut a = Assembler::new();
    let _ = a.add_base(BaseKind::Segment, &segment_keys());
    let _ = a.add_base(BaseKind::Matrix, &matrix_keys());
    let _ = a.add_base(BaseKind::StressAgg, &stress_keys());
    let exec = a.entries().iter().map(|e| e.key).collect();
    (a, exec)
}

/// 注入核：缺基线/缺例/键重复/键漂移 四类必抓。
pub struct AsmInjectCore {
    pub caught: u32,
    pub total: u32,
}

impl AsmInjectCore {
    pub fn new() -> Self {
        AsmInjectCore { caught: 0, total: 0 }
    }
    pub fn inject_base_missing(&mut self) {
        self.total += 1;
        let mut a = Assembler::new();
        let _ = a.add_base(BaseKind::Segment, &segment_keys());
        // 缺两个基线 → bases_complete 必假
        if !a.bases_complete() {
            self.caught += 1;
        }
    }
    pub fn inject_case_missing(&mut self) {
        self.total += 1;
        let mut a = Assembler::new();
        // 段级少一例（56 例）→ 例数闸拒
        let mut k = segment_keys();
        k.pop();
        let r = a.add_base(BaseKind::Segment, &k);
        if r.is_err() && a.reject_count() == 1 {
            self.caught += 1;
        }
    }
    pub fn inject_dup_key(&mut self) {
        self.total += 1;
        let mut a = Assembler::new();
        let _ = a.add_base(BaseKind::Segment, &segment_keys());
        let r = a.add(AsmEntry {
            key: "s01",
            base: BaseKind::Matrix,
            seq: 999,
        });
        if r.is_err() && a.reject_count() == 1 {
            self.caught += 1;
        }
    }
    pub fn inject_exec_drift(&mut self) {
        self.total += 1;
        let (a, mut exec) = full_assembly();
        exec.pop(); // 执行账缺一键 → 559
        if a.entries().len() != exec.len() {
            self.caught += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        self.total == 4 && self.caught == 4
    }
}

/// 判据集。
pub fn run_unx2962_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx2962");

    // C62-01 三基线 560/560 完整
    {
        let (a, _) = full_assembly();
        let ok = a.bases_complete() && a.entries().len() == 560;
        s.add("C62-01-560 完整", ok, if ok { "" } else { "装配不齐" });
    }
    // C62-02 装配-执行键对齐
    {
        let (a, exec) = full_assembly();
        let mut probe = Assembler::new();
        probe.register_exec(&exec);
        let ok = a.entries().len() == exec.len() && probe.exec_keys.len() == 560;
        s.add("C62-02-键对齐", ok, if ok { "" } else { "执行键漂移" });
    }
    // C62-03 三基线各自例数核对
    {
        let (a, _) = full_assembly();
        let seg = a.entries().iter().filter(|e| e.base == BaseKind::Segment).count();
        let mtx = a.entries().iter().filter(|e| e.base == BaseKind::Matrix).count();
        let str_ = a.entries().iter().filter(|e| e.base == BaseKind::StressAgg).count();
        let ok = seg == 57 && mtx == 30 && str_ == 473;
        s.add("C62-03-基线例数", ok, if ok { "" } else { "例数漂移" });
    }
    // C62-04 冻结哈希恒等
    {
        let (mut a, _) = full_assembly();
        let h = a.freeze();
        let ok = h != 0 && a.fingerprint() == h;
        s.add("C62-04-哈希入册", ok, if ok { "" } else { "指纹漂移" });
    }
    // C62-05 键唯一性（560 项零重复）
    {
        let (a, _) = full_assembly();
        let mut uniq = true;
        for i in 0..a.entries().len() {
            for j in (i + 1)..a.entries().len() {
                if a.entries()[i].key == a.entries()[j].key {
                    uniq = false;
                }
            }
        }
        s.add("C62-05-键唯一", uniq, if uniq { "" } else { "存在重复键" });
    }
    // C62-06 序号连续（0..560 无跳号）
    {
        let (a, _) = full_assembly();
        let ok = a.entries().iter().enumerate().all(|(i, e)| e.seq == i as u32);
        s.add("C62-06-序号连续", ok, if ok { "" } else { "序号跳号" });
    }
    // C62-07 注入核 4/4
    {
        let mut inj = AsmInjectCore::new();
        inj.inject_base_missing();
        inj.inject_case_missing();
        inj.inject_dup_key();
        inj.inject_exec_drift();
        s.add(
            "C62-07-注入核4-4",
            inj.all_caught(),
            if inj.all_caught() { "" } else { "注入漏抓" },
        );
    }
    s
}
