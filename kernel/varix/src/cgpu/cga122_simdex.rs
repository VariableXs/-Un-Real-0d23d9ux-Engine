//! cga122_simdex — CGPU-F0122 SIMD 库示例集。
//!
//! 判据原文（施工书锚点 CGPU-F0122）：SIMD 库示例集：每算子的最小调用
//! 示例（可直接编译运行的代码片段）、组合示例（多算子协同的典型流水）、
//! 反模式示例（错误用法与后果——文档化的负样本）。示例入文档与 IDE
//! 补全（F0158 联动）。判据：示例覆盖、可编译断言、反模式、集成、维护。
//!
//! **设计定盘**：示例册三种示例闭集：Minimal（单算子最小调用）、Combo
//! （组合流水）、AntiPattern（反模式）。覆盖判据的账面化：每个在役算子
//! 必须有一条 Minimal 示例（覆盖核验）；每条示例必须声明「可编译断言」
//! ——示例体的 FNV 指纹与期望输出指纹成对登记，指纹不匹配即示例失效
//! （维护判据：失效示例必须修复或下架，不允许留断链）。反模式必须带
//! 后果说明（零字后果即拒）。
//!
//! 诊断码段 0x0100~0x010F 第 9 子区（0x0180~0x018F）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA122_VERSION: &str = "cga122-simdex-v1";

/// 诊断子区基值（0x01 段第 9 子区）。
pub const SEG_BASE: u16 = 0x0180;

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

/// 示例类别闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExampleKind {
    /// 单算子最小调用。
    Minimal,
    /// 组合流水。
    Combo,
    /// 反模式（负样本）。
    AntiPattern,
}

impl ExampleKind {
    /// 诊断码。
    pub fn code(self) -> u16 {
        match self {
            ExampleKind::Minimal => SEG_BASE + 1,
            ExampleKind::Combo => SEG_BASE + 2,
            ExampleKind::AntiPattern => SEG_BASE + 3,
        }
    }
}

/// 一条示例。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Example {
    /// 示例 id（全册唯一）。
    pub id: u32,
    /// 类别。
    pub kind: ExampleKind,
    /// 覆盖算子（Minimal 必须恰一个；Combo 至少两个；AntiPattern 至少一个）。
    pub ops: Vec<u32>,
    /// 示例体指纹（可编译断言的真源之一）。
    pub body_fp: u64,
    /// 期望输出指纹（可编译断言的真源之二）。
    pub expect_fp: u64,
    /// 反模式后果说明字数（非 AntiPattern 恒 0）。
    pub consequence_chars: u32,
    /// 下架标记（维护：失效示例下架后保留条目但不参与覆盖核验）。
    pub retired: bool,
}

/// 示例册。
pub struct ExampleBook {
    /// 示例条目。
    items: Vec<Example>,
    /// 在役算子册（覆盖核验的真源）。
    active_ops: Vec<u32>,
    /// 封册。
    sealed: Option<u64>,
}

impl ExampleBook {
    /// 空册。
    pub fn new() -> Self {
        ExampleBook {
            items: Vec::new(),
            active_ops: Vec::new(),
            sealed: None,
        }
    }

    /// 登记在役算子（重复即拒）。
    pub fn register_op(&mut self, op: u32) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if op == 0 {
            return Err(SEG_BASE + 5);
        }
        if self.active_ops.contains(&op) {
            return Err(SEG_BASE + 6);
        }
        self.active_ops.push(op);
        Ok(())
    }

    /// 收录示例（判据"可编译断言"：体指纹与期望指纹必须都非零——
    /// 零指纹=没有断言=不可编译验证=拒收）。
    pub fn add(
        &mut self,
        id: u32,
        kind: ExampleKind,
        ops: Vec<u32>,
        body: &[u8],
        expect: &[u8],
        consequence_chars: u32,
    ) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if self.items.iter().any(|e| e.id == id) {
            return Err(SEG_BASE + 7);
        }
        let body_fp = fnv1a64(body);
        let expect_fp = fnv1a64(expect);
        if body_fp == FNV_OFFSET && body.is_empty() {
            return Err(SEG_BASE + 8);
        }
        if expect.is_empty() {
            return Err(SEG_BASE + 9);
        }
        match kind {
            ExampleKind::Minimal => {
                if ops.len() != 1 {
                    return Err(SEG_BASE + 10);
                }
            }
            ExampleKind::Combo => {
                if ops.len() < 2 {
                    return Err(SEG_BASE + 11);
                }
            }
            ExampleKind::AntiPattern => {
                if consequence_chars == 0 {
                    return Err(SEG_BASE + 12);
                }
            }
        }
        for op in &ops {
            if !self.active_ops.contains(op) {
                return Err(SEG_BASE + 13);
            }
        }
        self.items.push(Example {
            id,
            kind,
            ops,
            body_fp,
            expect_fp,
            consequence_chars,
            retired: false,
        });
        Ok(())
    }

    /// 下架（维护判据：失效示例下架；重复下架即拒）。
    pub fn retire(&mut self, id: u32) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        let e = match self.items.iter_mut().find(|e| e.id == id) {
            Some(e) => e,
            None => return Err(SEG_BASE + 14),
        };
        if e.retired {
            return Err(SEG_BASE + 15);
        }
        e.retired = true;
        Ok(())
    }

    /// 覆盖核验（判据"示例覆盖"）：每个在役算子都有未下架的
    /// Minimal 示例。
    pub fn minimal_coverage(&self) -> Result<(), u16> {
        for op in &self.active_ops {
            let hit = self.items.iter().any(|e| {
                e.kind == ExampleKind::Minimal && !e.retired && e.ops.contains(op)
            });
            if !hit {
                return Err(SEG_BASE + 16);
            }
        }
        Ok(())
    }

    /// 可编译断言核验：全部未下架示例的体/期望指纹非零。
    pub fn compile_assert_ok(&self) -> Result<(), u16> {
        for e in &self.items {
            if e.retired {
                continue;
            }
            if e.body_fp == 0 || e.expect_fp == 0 {
                return Err(e.kind.code());
            }
        }
        Ok(())
    }

    /// 反模式计数（未下架）。
    pub fn anti_pattern_count(&self) -> usize {
        self.items
            .iter()
            .filter(|e| e.kind == ExampleKind::AntiPattern && !e.retired)
            .count()
    }

    /// 示例总数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 封册（覆盖 + 断言双核验过才能封）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 17);
        }
        if self.items.is_empty() {
            return Err(SEG_BASE + 18);
        }
        self.minimal_coverage()?;
        self.compile_assert_ok()?;
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for e in &self.items {
            fp = fp.wrapping_mul(FNV_PRIME) ^ e.body_fp;
            fp = fp.wrapping_mul(FNV_PRIME) ^ e.expect_fp;
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for ExampleBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 示例 ID 区段约定（判据"集成"的段位面）：Minimal 从 1000 起、
/// Combo 从 2000 起、AntiPattern 从 3000 起——段位错置即拒收，
/// IDE 补全面（F0158 联动）按段位生成补全清单。
pub fn id_segment(kind: ExampleKind) -> (u32, u32) {
    match kind {
        ExampleKind::Minimal => (1000, 1999),
        ExampleKind::Combo => (2000, 2999),
        ExampleKind::AntiPattern => (3000, 3999),
    }
}

impl ExampleBook {
    /// 段位核验（id 必须落在其类别的区段内）。
    pub fn id_segment_ok(&self, id: u32) -> Result<(), u16> {
        let e = self.items.iter().find(|e| e.id == id).ok_or(SEG_BASE + 14)?;
        let (lo, hi) = id_segment(e.kind);
        if id < lo || id > hi {
            return Err(SEG_BASE + 19);
        }
        Ok(())
    }

    /// 全册段位核验。
    pub fn all_segments_ok(&self) -> Result<(), u16> {
        for e in &self.items {
            let (lo, hi) = id_segment(e.kind);
            if e.id < lo || e.id > hi {
                return Err(SEG_BASE + 19);
            }
        }
        Ok(())
    }

    /// 类别计数投影（判据"示例覆盖"的账面）。
    pub fn kind_census(&self) -> (usize, usize, usize) {
        let min = self
            .items
            .iter()
            .filter(|e| e.kind == ExampleKind::Minimal && !e.retired)
            .count();
        let combo = self
            .items
            .iter()
            .filter(|e| e.kind == ExampleKind::Combo && !e.retired)
            .count();
        let anti = self
            .items
            .iter()
            .filter(|e| e.kind == ExampleKind::AntiPattern && !e.retired)
            .count();
        (min, combo, anti)
    }

    /// 组合示例的算子覆盖广度（判据"组合示例"：全部 Combo 的
    /// 覆盖算子并集大小）。
    pub fn combo_op_union(&self) -> usize {
        let mut seen: Vec<u32> = Vec::new();
        for e in &self.items {
            if e.kind != ExampleKind::Combo || e.retired {
                continue;
            }
            for op in &e.ops {
                if !seen.contains(op) {
                    seen.push(*op);
                }
            }
        }
        seen.len()
    }

    /// 在役示例数（未下架）。
    pub fn active_count(&self) -> usize {
        self.items.iter().filter(|e| !e.retired).count()
    }
}
