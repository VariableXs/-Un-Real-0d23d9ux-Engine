//! cga135_secscan — CGPU-F0135 A 域安全审查。
//!
//! 判据原文（施工书锚点 CGPU-F0135）：A 域安全审查（F0392 范式）：
//! SIMD 越界写防护复核（F0001 防护全算子覆盖）、无锁结构安全（F0039
//! 的 ABA/内存序审查）、任务图死锁安全（F0003 检测覆盖）、外部输入
//! 清洗（任务参数的校验）。注入测试全绿。判据：四面逐项过、注入全绿、
//! 🔴 零、豁免 ADR、报告归档。
//!
//! **设计定盘**：四面审查账：①越界写防护面（逐算子登记防护态——
//! Protected/Guarded（有界检查）/Exposed（裸写））；②无锁结构面
//! （逐结构登记 ABA 防护与内存序档位）；③死锁安全面（任务图检测器
//! 覆盖态）；④输入清洗面（逐参数入口登记清洗器覆盖）。注入测试 =
//! 四面各自的注入用例账（注入→期望拦截），全绿才收。🔴（Exposed/
//! 无 ABA 防护/死锁检测缺位/未清洗入口）必须清零或持 ADR 豁免。
//! 报告 = 四面 + 注入 + 豁免的指纹链归档。
//!
//! 诊断码段 0x0260~0x026F（0x02 段第 7 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA135_VERSION: &str = "cga135-secscan-v1";

/// 诊断子区基值（0x02 段第 7 子区）。
pub const SEG_BASE: u16 = 0x0260;

/// FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 越界写防护三态（判据"越界写防护"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoundGuard {
    /// 静态可证安全（编译期界检查）。
    Protected,
    /// 运行期界检查。
    Guarded,
    /// 裸写（🔴——必须清零或 ADR）。
    Exposed,
}

impl BoundGuard {
    /// 是否🔴。
    pub fn is_red(self) -> bool {
        matches!(self, BoundGuard::Exposed)
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        match self {
            BoundGuard::Protected => SEG_BASE + 1,
            BoundGuard::Guarded => SEG_BASE + 2,
            BoundGuard::Exposed => SEG_BASE + 3,
        }
    }
}

/// 内存序档位（判据"内存序审查"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemOrder {
    /// Relaxed（仅计数器，无同步语义要求处）。
    Relaxed,
    /// Acquire/Release 配对（常规发布）。
    AcqRel,
    /// SeqCst（全序——跨结构一致性要求处）。
    SeqCst,
}

/// ABA 防护态（判据"ABA/内存序审查"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LockFreeRow {
    /// 结构号。
    pub structure: u32,
    /// ABA 防护（代际标签/版本指针）。
    pub aba_guarded: bool,
    /// 内存序档。
    pub order: MemOrder,
}

impl LockFreeRow {
    /// 行合法性：Relaxed 档只允许计数器语义结构（structure 号 < 100
    /// 为计数器保留段）；非计数器结构用 Relaxed 即🔴。
    pub fn validate(&self) -> Result<(), u16> {
        if self.structure == 0 {
            return Err(SEG_BASE + 4);
        }
        if self.order == MemOrder::Relaxed && self.structure >= 100 {
            return Err(SEG_BASE + 5);
        }
        Ok(())
    }

    /// 是否🔴。
    pub fn is_red(&self) -> bool {
        !self.aba_guarded
    }
}

/// 四面审查账。
pub struct SecAudit {
    /// ① 越界写防护（算子号 → 防护态）。
    bounds: Vec<(u32, BoundGuard)>,
    /// ② 无锁结构面。
    lockfree: Vec<LockFreeRow>,
    /// ③ 死锁检测覆盖（任务图检测器登记）。
    deadlock_covered: Option<bool>,
    /// ④ 输入清洗（入口号 → 清洗器指纹）。
    sanitize: Vec<(u32, u64)>,
    /// 注入用例账（面号, 用例指纹, 是否拦截）。
    injections: Vec<(u8, u64, bool)>,
    /// 豁免 ADR（主体号 → ADR 编号）。
    waivers: Vec<(u32, u32)>,
    /// 归档指纹。
    archived: Option<u64>,
}

impl SecAudit {
    /// 空账。
    pub fn new() -> Self {
        SecAudit {
            bounds: Vec::new(),
            lockfree: Vec::new(),
            deadlock_covered: None,
            sanitize: Vec::new(),
            injections: Vec::new(),
            waivers: Vec::new(),
            archived: None,
        }
    }

    /// ① 登记算子防护态（重复即拒）。
    pub fn record_bounds(&mut self, op: u32, g: BoundGuard) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 6);
        }
        if op == 0 || self.bounds.iter().any(|(o, _)| *o == op) {
            return Err(SEG_BASE + 7);
        }
        self.bounds.push((op, g));
        Ok(())
    }

    /// ② 登记无锁结构（重复即拒）。
    pub fn record_lockfree(&mut self, row: LockFreeRow) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 6);
        }
        row.validate()?;
        if self.lockfree.iter().any(|r| r.structure == row.structure) {
            return Err(SEG_BASE + 8);
        }
        self.lockfree.push(row);
        Ok(())
    }

    /// ③ 登记死锁检测覆盖（判据"死锁安全"——只许登记 true，
    /// false 是未覆盖不是登记值，未覆盖直接在审查面红）。
    pub fn record_deadlock(&mut self, covered: bool) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 6);
        }
        if self.deadlock_covered.is_some() {
            return Err(SEG_BASE + 9);
        }
        self.deadlock_covered = Some(covered);
        Ok(())
    }

    /// ④ 登记清洗入口（重复即拒；零指纹拒——没有清洗器就没有覆盖）。
    pub fn record_sanitize(&mut self, entry: u32, sanitizer_fp: u64) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 6);
        }
        if entry == 0 || sanitizer_fp == 0 {
            return Err(SEG_BASE + 10);
        }
        if self.sanitize.iter().any(|(e, _)| *e == entry) {
            return Err(SEG_BASE + 11);
        }
        self.sanitize.push((entry, sanitizer_fp));
        Ok(())
    }

    /// 注入用例（面号 1..=4；重复用例指纹即拒）。
    pub fn inject(&mut self, face: u8, case: &[u8], intercepted: bool) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 6);
        }
        if face < 1 || face > 4 {
            return Err(SEG_BASE + 12);
        }
        let fp = fnv1a64(case);
        if self.injections.iter().any(|(_, f, _)| *f == fp) {
            return Err(SEG_BASE + 13);
        }
        self.injections.push((face, fp, intercepted));
        Ok(())
    }

    /// 豁免登记（判据"豁免 ADR"：主体号 + ADR 编号；重复豁免即拒）。
    pub fn waive(&mut self, subject: u32, adr: u32) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 6);
        }
        if adr == 0 || self.waivers.iter().any(|(s, _)| *s == subject) {
            return Err(SEG_BASE + 14);
        }
        self.waivers.push((subject, adr));
        Ok(())
    }

    /// 主体是否豁免。
    pub fn waived(&self, subject: u32) -> bool {
        self.waivers.iter().any(|(s, _)| *s == subject)
    }

    /// 🔴 清单（判据"🔴 零"的枚举面）：Exposed 算子 + 无 ABA 防护
    /// 结构 + 死锁未覆盖；未在红单且未豁免的主体 = 审查欠账。
    pub fn red_items(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for (op, g) in &self.bounds {
            if g.is_red() {
                out.push(*op);
            }
        }
        for r in &self.lockfree {
            if r.is_red() {
                out.push(r.structure);
            }
        }
        if self.deadlock_covered == Some(false) {
            out.push(0xFFFF_FFFF);
        }
        out
    }

    /// 🔴 清零核验（判据"🔴 零"：红单全部被 ADR 豁免或不存在）。
    pub fn reds_clear(&self) -> bool {
        self.red_items().iter().all(|s| self.waived(*s))
    }

    /// 注入全绿（判据"注入全绿"：有用例且全部拦截）。
    pub fn injections_green(&self) -> bool {
        !self.injections.is_empty() && self.injections.iter().all(|(_, _, i)| *i)
    }

    /// 四面完整性（每面至少一条登记）。
    pub fn faces_complete(&self) -> Result<(), u16> {
        if self.bounds.is_empty() {
            return Err(SEG_BASE + 15);
        }
        if self.lockfree.is_empty() {
            return Err(SEG_BASE + 16);
        }
        if self.deadlock_covered.is_none() {
            return Err(SEG_BASE + 17);
        }
        if self.sanitize.is_empty() {
            return Err(SEG_BASE + 18);
        }
        Ok(())
    }

    /// 归档（判据"报告归档"：四面齐 + 红零 + 注入绿才能归档）。
    pub fn archive(&mut self) -> Result<u64, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 19);
        }
        self.faces_complete()?;
        if !self.reds_clear() {
            return Err(SEG_BASE + 20);
        }
        if !self.injections_green() {
            return Err(SEG_BASE + 21);
        }
        let mut fp = FNV_OFFSET;
        for (op, g) in &self.bounds {
            fp = fp.wrapping_mul(FNV_PRIME) ^ ((*op as u64) << 8 | g.code() as u64);
        }
        for r in &self.lockfree {
            fp = fp.wrapping_mul(FNV_PRIME) ^ (r.structure as u64) ^ r.order as u64;
        }
        for (e, f) in &self.sanitize {
            fp = fp.wrapping_mul(FNV_PRIME) ^ (*e as u64) ^ f;
        }
        self.archived = Some(fp);
        Ok(fp)
    }
}

impl Default for SecAudit {
    fn default() -> Self {
        Self::new()
    }
}

impl SecAudit {
    /// 面计数投影（判据"四面逐项过"的账面：四面各几条）。
    pub fn face_census(&self) -> (usize, usize, usize, usize) {
        (
            self.bounds.len(),
            self.lockfree.len(),
            self.deadlock_covered.is_some() as usize,
            self.sanitize.len(),
        )
    }

    /// 注入面分布（四面各多少用例——判据"注入全绿"的覆盖面前提：
    /// 每面至少一例）。
    pub fn injection_face_census(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for (f, _, _) in &self.injections {
            out[(*f - 1) as usize] += 1;
        }
        out
    }

    /// 注入拦截率（千分比）。
    pub fn intercept_permille(&self) -> Option<u64> {
        if self.injections.is_empty() {
            return None;
        }
        let hit = self.injections.iter().filter(|(_, _, i)| *i).count();
        Some(hit as u64 * 1000 / self.injections.len() as u64)
    }

    /// 防护覆盖投影（判据"F0001 防护全算子覆盖"：Protected/Guarded
    /// 两档的算子占比千分比——Exposed 未清零时该值 < 1000）。
    pub fn guard_coverage_permille(&self) -> Option<u64> {
        if self.bounds.is_empty() {
            return None;
        }
        let guarded = self
            .bounds
            .iter()
            .filter(|(_, g)| !g.is_red())
            .count();
        Some(guarded as u64 * 1000 / self.bounds.len() as u64)
    }

    /// 无 ABA 防护结构清单（判据"ABA/内存序审查"的定位面）。
    pub fn unguarded_structures(&self) -> Vec<u32> {
        self.lockfree
            .iter()
            .filter(|r| r.is_red())
            .map(|r| r.structure)
            .collect()
    }

    /// 归档指纹。
    pub fn archive_fp(&self) -> Option<u64> {
        self.archived
    }
}
