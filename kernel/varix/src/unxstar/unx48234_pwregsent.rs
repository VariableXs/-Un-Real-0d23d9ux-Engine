//! unx48234_pwregsent — UNX-F48234 标点半全角三类窗口回归哨兵（vxwm 原生/Wine/Xwayland 三窗行为一致判据·宪法加严）（诊断段 0xBE20）。
//!
//! 判据原文：
//!   J1 三类窗口（vxwm 原生/Wine/Xwayland）回归判据：三窗标点/半全角行为一致；判据锚定 §AI-61 §2 专题三。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 三窗同尺：标点/半全角行为在三窗（VxwmNative/Wine/Xwayland）
//!      上必须逐用例一致；「一致」的度量 = 同输入同期望输出语义位
//!      （上屏字符指纹 + 候选数 + 状态位三元组），不比较像素。
//!   2. 哨兵用例闭集：标点 12 例 + 半全角 8 例 = 20 例常量账，
//!      每例三窗各跑一遍 → 60 回执；缺窗缺例即红。
//!   3. 分歧显性：任一窗与基准（vxwm 原生为基准窗）回执不一致，
//!      分歧账记录 (用例, 窗, 三元组差)——绝不静默归一。
//!   4. 回归账可复跑：哨兵账整体封账指纹；重跑指纹不变（确定性）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;      // vec! 宏（坑 2 三件套）
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBE20;

const E_BAD_WINDOW: u16 = SEG_BASE + 1; // 窗闭集外
const E_BAD_CASE: u16 = SEG_BASE + 2;   // 用例闭集外
const E_DIVERGE: u16 = SEG_BASE + 3;    // 三窗行为分歧
const E_MISS_RUN: u16 = SEG_BASE + 4;   // 缺窗缺例
const E_SEAL_MISMATCH: u16 = SEG_BASE + 5; // 封账指纹不符
const E_GUARD_UNBAL: u16 = SEG_BASE + 6;   // 哨兵账不平

/// 三窗闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Win {
    VxwmNative, // 基准窗
    Wine,
    Xwayland,
}

impl Win {
    fn idx(&self) -> usize {
        match self {
            Win::VxwmNative => 0,
            Win::Wine => 1,
            Win::Xwayland => 2,
        }
    }
}

/// 行为三元组：上屏字符指纹 / 候选数 / 状态位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BehTriple {
    pub out_fp: u32,
    pub cand_n: u8,
    pub status_bits: u8,
}

/// 哨兵用例：标点（1..=12）/ 半全角（13..=20）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SentinelCase {
    pub id: u8,      // 1..=20
    pub kind: u8,    // 1=标点 2=半全角
    pub input: u32,  // 输入键面（键码+修饰合成）
}

/// 一格回执。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunCell {
    pub case_id: u8,
    pub win: Win,
    pub got: BehTriple,
}

/// 哨兵总账。
#[derive(Debug)]
pub struct PwSentinel {
    cases: Vec<SentinelCase>,
    cells: Vec<RunCell>,
    diverges: Vec<(u8, Win, BehTriple, BehTriple)>, // (用例, 窗, 基准, 实测)
    sealed: Option<u32>,
}

/// 语义执行器：输入 → 行为三元组（确定性，三窗共用同一语义核，
/// 分歧只可能来自窗特有化配置——本账用配置差异注入模拟）。
pub fn semantic_run(input: u32, win: Win, diverge_cfg: Option<(u8, Win)>, case_id: u8) -> BehTriple {
    // 基准语义：out_fp = FNV(input)，cand_n = (input % 7) as u8，
    // status_bits = (input % 3) as u8。
    let mut h: u32 = 0x811C_9DC5;
    h ^= input;
    h = h.wrapping_mul(0x0100_0193);
    let mut t = BehTriple {
        out_fp: h,
        cand_n: (input % 7) as u8,
        status_bits: (input % 3) as u8,
    };
    // 窗特有化分歧注入（仅命中指定用例+指定窗时制造分歧）。
    if let Some((cid, w)) = diverge_cfg {
        if cid == case_id && w == win {
            t.cand_n = t.cand_n.wrapping_add(1); // 制造分歧
        }
    }
    t
}

impl PwSentinel {
    pub fn new() -> Self {
        PwSentinel { cases: Vec::new(), cells: Vec::new(), diverges: Vec::new(), sealed: None }
    }

    /// 装载哨兵用例：标点 12 + 半全角 8，号唯一。
    pub fn load(&mut self, cs: Vec<SentinelCase>) -> Result<(), u16> {
        for c in &cs {
            if !(1..=20).contains(&c.id) {
                return Err(E_BAD_CASE);
            }
            if !(c.kind == 1 || c.kind == 2) {
                return Err(E_BAD_CASE);
            }
            if self.cases.iter().any(|x| x.id == c.id) {
                return Err(E_BAD_CASE);
            }
        }
        // 完整性：标点 12 例（1..=12）+ 半全角 8 例（13..=20）。
        let p = cs.iter().filter(|c| c.kind == 1).count();
        let f = cs.iter().filter(|c| c.kind == 2).count();
        if p != 12 || f != 8 {
            return Err(E_MISS_RUN);
        }
        self.cases = cs;
        Ok(())
    }

    /// 跑哨兵：三窗 × 全用例，与基准窗逐格比对，分歧入账。
    pub fn run(&mut self, diverge_cfg: Option<(u8, Win)>) -> Result<u32, u16> {
        self.cells.clear();
        self.diverges.clear();
        let mut pass = 0u32;
        // 基准先行。
        let mut base: Vec<(u8, BehTriple)> = Vec::new();
        for c in &self.cases {
            let t = semantic_run(c.input, Win::VxwmNative, diverge_cfg, c.id);
            base.push((c.id, t));
            self.cells.push(RunCell { case_id: c.id, win: Win::VxwmNative, got: t });
        }
        for w in [Win::Wine, Win::Xwayland] {
            for c in &self.cases {
                let t = semantic_run(c.input, w, diverge_cfg, c.id);
                self.cells.push(RunCell { case_id: c.id, win: w, got: t });
                let b = base.iter().find(|(id, _)| *id == c.id).map(|(_, t)| *t).ok_or(E_GUARD_UNBAL)?;
                if t != b {
                    self.diverges.push((c.id, w, b, t));
                } else {
                    pass += 1;
                }
            }
        }
        Ok(pass)
    }

    /// 判据：零分歧 + 回执满格（60）。
    pub fn assert_consistent(&self) -> Result<(), u16> {
        if self.cells.len() != self.cases.len() * 3 {
            return Err(E_MISS_RUN);
        }
        if !self.diverges.is_empty() {
            return Err(E_DIVERGE);
        }
        Ok(())
    }

    /// 封账：用例集 + 回执指纹。
    pub fn seal(&mut self) -> Result<u32, u16> {
        let mut h: u32 = 0x811C_9DC5;
        for c in &self.cases {
            h ^= c.id as u32;
            h = h.wrapping_mul(0x0100_0193);
            h ^= c.input;
            h = h.wrapping_mul(0x0100_0193);
        }
        for cell in &self.cells {
            h ^= cell.got.out_fp;
            h = h.wrapping_mul(0x0100_0193);
            h ^= (cell.win.idx() as u32) << 8 | cell.case_id as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        self.sealed = Some(h);
        Ok(h)
    }

    pub fn verify(&self, fp: u32) -> Result<(), u16> {
        match self.sealed {
            Some(s) if s == fp => Ok(()),
            _ => Err(E_SEAL_MISMATCH),
        }
    }

    pub fn cells(&self) -> usize {
        self.cells.len()
    }
    pub fn diverge_len(&self) -> usize {
        self.diverges.len()
    }
    pub fn cases(&self) -> usize {
        self.cases.len()
    }
}

/// 标准哨兵用例集：标点 12 + 半全角 8。
pub fn std_cases() -> Vec<SentinelCase> {
    let mk = |id: u8, kind: u8| SentinelCase { id, kind, input: 0x1000 + id as u32 * 7 };
    let mut v = Vec::new();
    for i in 1u8..=12 {
        v.push(mk(i, 1));
    }
    for i in 13u8..=20 {
        v.push(mk(i, 2));
    }
    v
}

/// 自例程判据：落实 J1「三窗标点/半全角行为一致」全链。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 场景：满编哨兵 —— 20 例 × 3 窗 = 60 回执，零分歧。
    let mut s = PwSentinel::new();
    s.load(std_cases())?;
    if s.cases() != 20 {
        return Err(E_MISS_RUN);
    }
    let pass = s.run(None)?;
    if pass != 40 || s.cells() != 60 {
        return Err(E_GUARD_UNBAL); // 基准 20 不计 pass，两对比窗 40
    }
    s.assert_consistent()?;
    hits += 1;

    // R2 场景：可复跑 —— 重跑指纹不变（确定性）。
    let fp1 = s.seal()?;
    s.verify(fp1)?;
    s.run(None)?;
    let fp2 = s.seal()?;
    if fp1 != fp2 {
        return Err(E_SEAL_MISMATCH);
    }
    hits += 1;

    // R3 场景：分歧显性 —— 注入 Wine 窗分歧配置，分歧账逐格落地且 assert 红。
    let mut s3 = PwSentinel::new();
    s3.load(std_cases())?;
    let _ = s3.run(Some((13, Win::Wine)))?; // 用例 13 在 Wine 分歧
    if s3.diverge_len() != 1 {
        return Err(E_GUARD_UNBAL);
    }
    match s3.assert_consistent() {
        Err(E_DIVERGE) => {}
        _ => return Err(E_DIVERGE),
    }
    hits += 1;

    // R4 场景：分歧定位 —— 分歧账三元组可读（用例号+窗+差值）。
    let (cid, w, b, g) = s3.diverges[0];
    if !(cid == 13 && w == Win::Wine && g.cand_n == b.cand_n + 1) {
        return Err(E_DIVERGE);
    }
    hits += 1;

    // R5 场景：Xwayland 分歧同样显性 —— 换窗注入照样抓住。
    let mut s5 = PwSentinel::new();
    s5.load(std_cases())?;
    let _ = s5.run(Some((5, Win::Xwayland)))?;
    match s5.assert_consistent() {
        Err(E_DIVERGE) => {}
        _ => return Err(E_DIVERGE),
    }
    if s5.diverge_len() != 1 {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R6 场景：缺窗缺例显性 —— 少装一条半全角例 → load 拒收。
    let mut s6 = PwSentinel::new();
    let mut cs6 = std_cases();
    cs6.retain(|c| c.id != 20);
    match s6.load(cs6) {
        Err(E_MISS_RUN) => {}
        _ => return Err(E_MISS_RUN),
    }
    hits += 1;

    // R7 场景：用例号越界拒收（0 与 21）。
    let mut s7 = PwSentinel::new();
    match s7.load(vec![SentinelCase { id: 0, kind: 1, input: 1 }, SentinelCase { id: 2, kind: 1, input: 2 }]) {
        Err(E_BAD_CASE) => {}
        _ => return Err(E_BAD_CASE),
    }
    match s7.load(vec![SentinelCase { id: 21, kind: 1, input: 1 }]) {
        Err(E_BAD_CASE) => {}
        _ => return Err(E_BAD_CASE),
    }
    hits += 1;

    // R8 场景：kind 闭集外拒收。
    let mut s8 = PwSentinel::new();
    match s8.load(vec![SentinelCase { id: 3, kind: 9, input: 1 }]) {
        Err(E_BAD_CASE) => {}
        _ => return Err(E_BAD_CASE),
    }
    hits += 1;

    // R9 场景：号重复拒收。
    let mut s9 = PwSentinel::new();
    match s9.load(vec![
        SentinelCase { id: 4, kind: 1, input: 1 },
        SentinelCase { id: 4, kind: 1, input: 2 },
    ]) {
        Err(E_BAD_CASE) => {}
        _ => return Err(E_BAD_CASE),
    }
    hits += 1;

    // R10 场景：封账篡改显性 —— 回执被改（重跑带分歧）后旧指纹失效。
    let mut s10 = PwSentinel::new();
    s10.load(std_cases())?;
    let _ = s10.run(None)?;
    let fpa = s10.seal()?;
    let _ = s10.run(Some((1, Win::Wine)))?; // 分歧重跑改变回执
    let fpb = s10.seal()?;
    if fpa == fpb {
        return Err(E_SEAL_MISMATCH); // 指纹必须敏感
    }
    match s10.verify(fpa) {
        Err(E_SEAL_MISMATCH) => {}
        _ => return Err(E_SEAL_MISMATCH),
    }
    hits += 1;

    // R11 场景：语义核确定性 —— 同输入同窗同配置，三元组逐字段相等；
    //        不同输入指纹互异（防碰撞退化）。
    let t1 = semantic_run(0x1057, Win::Wine, None, 0);
    let t2 = semantic_run(0x1057, Win::Wine, None, 0);
    let t3 = semantic_run(0x1058, Win::Wine, None, 0);
    if t1 != t2 || t1.out_fp == t3.out_fp {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    Ok(hits)
}
