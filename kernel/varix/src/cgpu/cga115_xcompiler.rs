//! cga115_xcompiler — CGPU-F0115 跨编译器一致性。
//!
//! 判据原文（施工书锚点 CGPU-F0115）：SIMD 代码的跨编译器一致性：
//! MSVC/GCC/Clang 三编译器的浮点行为差异治理（编译器标志统一：FP 收缩
//! 禁止/快速数学禁用——确定性 F0114 的编译器维度）、三编译器构建全测试、
//! 差异记录与收敛。判据：三编译器构建、标志统一、一致性断言、差异记录、
//! CI 矩阵。
//!
//! **设计定盘**：浮点确定性在编译器维度的风险源是「FP 收缩」（把
//! a*b+c 融合成 FMA 改变舍入）与「快速数学」（重排/化简违反 IEEE 序）。
//! 本模块把三编译器 × 收缩开关 × 快速数学开关的矩阵做成构建账：每格
//! 记录「构建通过与否 + 产出指纹」，三编译器在统一标志下必须产出同一
//! 指纹（一致性断言）；任何一格用了非统一标志即标「越标志构建」，其
//! 指纹不参与一致性对拍（防止拿错标志的结果污染判定）。
//!
//! 诊断码段 0x0100~0x010F 第 2 子区（0x0110~0x011F）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA115_VERSION: &str = "cga115-xcompiler-v1";

/// 诊断子区基值（0x01 段第 2 子区）。
pub const SEG_BASE: u16 = 0x0110;

/// 编译器闭集（判据"三编译器"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Compiler {
    /// MSVC。
    Msvc,
    /// GCC。
    Gcc,
    /// Clang。
    Clang,
}

impl Compiler {
    /// 全集遍历序（稳定序保证账本指纹可复现）。
    pub const ALL: [Compiler; 3] = [Compiler::Msvc, Compiler::Gcc, Compiler::Clang];

    /// 稳定序号。
    pub fn ordinal(self) -> u32 {
        match self {
            Compiler::Msvc => 0,
            Compiler::Gcc => 1,
            Compiler::Clang => 2,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 浮点标志统一档（判据"标志统一"——合法档只有全禁一档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FpFlags {
    /// FP 收缩禁止 + 快速数学禁用（唯一合法档——确定性档）。
    StrictNoContractNoFast,
    /// FP 收缩开启（违规档——构建可过但指纹不入对拍）。
    ContractOn,
    /// 快速数学开启（违规档——同上）。
    FastMathOn,
    /// 双开（违规档——同上）。
    ContractAndFast,
}

impl FpFlags {
    /// 是否统一档。
    pub fn is_unified(self) -> bool {
        matches!(self, FpFlags::StrictNoContractNoFast)
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        match self {
            FpFlags::StrictNoContractNoFast => SEG_BASE + 4,
            FpFlags::ContractOn => SEG_BASE + 5,
            FpFlags::FastMathOn => SEG_BASE + 6,
            FpFlags::ContractAndFast => SEG_BASE + 7,
        }
    }
}

/// 一条构建记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BuildCell {
    /// 编译器。
    pub compiler: Compiler,
    /// 浮点标志档。
    pub flags: FpFlags,
    /// 构建是否通过（0=失败 1=通过）。
    pub built: bool,
    /// 产出指纹（测试全集逐项输出的 FNV 聚合）。
    pub out_fp: u64,
    /// 失败诊断（built=false 时非空）。
    pub fail_code: u16,
}

/// 跨编译器一致性账。
pub struct XCompilerLedger {
    /// 构建记录。
    cells: Vec<BuildCell>,
    /// 差异记录（判据"差异记录"——不一致对的归因留痕）。
    diffs: Vec<(Compiler, Compiler, u64, u64)>,
    /// 收敛声明：收敛=统一档下三编译器指纹全同且无未处置差异。
    converged: bool,
    /// 封账指纹。
    sealed: Option<u64>,
}

impl XCompilerLedger {
    /// 空账。
    pub fn new() -> Self {
        XCompilerLedger {
            cells: Vec::new(),
            diffs: Vec::new(),
            converged: false,
            sealed: None,
        }
    }

    /// 登记一次构建（同编译器同标志重复登记即拒——构建账不可覆盖写）。
    pub fn record_build(
        &mut self,
        compiler: Compiler,
        flags: FpFlags,
        built: bool,
        out_fp: u64,
        fail_code: u16,
    ) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 8);
        }
        if self
            .cells
            .iter()
            .any(|c| c.compiler == compiler && c.flags == flags)
        {
            return Err(SEG_BASE + 9);
        }
        if built && fail_code != 0 {
            return Err(SEG_BASE + 10);
        }
        if !built && fail_code == 0 {
            return Err(SEG_BASE + 11);
        }
        self.cells.push(BuildCell {
            compiler,
            flags,
            built,
            out_fp,
            fail_code,
        });
        Ok(())
    }

    /// CI 矩阵完整性检查（判据"三编译器构建"+"CI 矩阵"）：
    /// 三编译器 × 统一档九格中统一档三格必须全通过，缺一即拒。
    pub fn matrix_complete(&self) -> Result<(), u16> {
        for c in Compiler::ALL {
            let hit = self.cells.iter().find(|b| {
                b.compiler == c && b.flags == FpFlags::StrictNoContractNoFast
            });
            let cell = match hit {
                Some(h) => h,
                None => return Err(c.code()),
            };
            if !cell.built {
                return Err(SEG_BASE + 12);
            }
        }
        Ok(())
    }

    /// 一致性断言（判据"一致性断言"）：统一档下三编译器指纹必须全同；
    /// 有差异时逐对落差异记录并返回差异对数。
    pub fn assert_consistency(&mut self) -> Result<usize, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 8);
        }
        self.matrix_complete()?;
        let fps: Vec<(Compiler, u64)> = self
            .cells
            .iter()
            .filter(|b| b.flags == FpFlags::StrictNoContractNoFast && b.built)
            .map(|b| (b.compiler, b.out_fp))
            .collect();
        let mut diff_pairs = 0usize;
        for i in 0..fps.len() {
            for j in (i + 1)..fps.len() {
                if fps[i].1 != fps[j].1 {
                    self.diffs.push((fps[i].0, fps[j].0, fps[i].1, fps[j].1));
                    diff_pairs += 1;
                }
            }
        }
        self.converged = diff_pairs == 0;
        if diff_pairs == 0 {
            Ok(0)
        } else {
            Err(SEG_BASE + 13)
        }
    }

    /// 是否收敛。
    pub fn converged(&self) -> bool {
        self.converged
    }

    /// 差异记录条数。
    pub fn diff_count(&self) -> usize {
        self.diffs.len()
    }

    /// 查差异记录。
    pub fn diff(&self, idx: usize) -> Option<(Compiler, Compiler, u64, u64)> {
        self.diffs.get(idx).copied()
    }

    /// 封账（必须已做一致性断言且收敛才能封——封账即 CI 证据）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 14);
        }
        if !self.converged {
            return Err(SEG_BASE + 15);
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for c in &self.cells {
            fp = fp.wrapping_mul(0x100_0000_01b3)
                ^ ((c.compiler.ordinal() as u64) << 32)
                ^ (c.flags as u64)
                ^ (c.built as u64)
                ^ c.out_fp;
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for XCompilerLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 差异根因分类闭集（判据"差异记录与收敛"的收敛面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffCause {
    /// 浮点舍入序差异（编译器对表达式结合顺序的合法重排不同）。
    RoundingOrder,
    /// FMA 收缩差异（一方把 mul+add 收缩成 FMA 改变中间舍入）。
    ContractShrink,
    /// 快速数学化简差异（一方开启了违反 IEEE 的代数化简）。
    FastMathSimplify,
    /// 内建函数展开差异（同一 SIMD 内建在不同后端展开序列不同）。
    BuiltinExpansion,
}

impl DiffCause {
    /// 诊断码。
    pub fn code(self) -> u16 {
        match self {
            DiffCause::RoundingOrder => SEG_BASE + 16,
            DiffCause::ContractShrink => SEG_BASE + 17,
            DiffCause::FastMathSimplify => SEG_BASE + 18,
            DiffCause::BuiltinExpansion => SEG_BASE + 19,
        }
    }

    /// 收敛建议动作（判据"差异记录与收敛"——每类差异一个可执行动作）。
    pub fn remedy(self) -> &'static str {
        match self {
            DiffCause::RoundingOrder => {
                "把跨语句的浮点表达式拆成显式中间绑定，消除编译器重排自由度"
            }
            DiffCause::ContractShrink => {
                "确认统一档标志（FP 收缩禁止）在三编译器命令行都生效；\
                 检查是否有内联汇编/pragma 绕过了统一档"
            }
            DiffCause::FastMathSimplify => {
                "扫描快速数学残余开关（-ffast-math / /fp:fast 的子开关），全部显性关闭"
            }
            DiffCause::BuiltinExpansion => {
                "对差异内建补标量参考实现并锁定期望指纹；差异内建列入 F0119 评估模板"
            }
        }
    }
}

/// 差异处置条目（差异 → 根因 → 建议 → 处置态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiffCase {
    /// 差异记录序号（对齐 diff() 的下标）。
    pub diff_idx: usize,
    /// 归因根因。
    pub cause: DiffCause,
    /// 处置态：false=待处置，true=已收敛。
    pub resolved: bool,
}

/// 收敛处置台（判据"差异记录与收敛"的处置面）。
pub struct ConvergenceDesk {
    /// 处置条目。
    cases: Vec<DiffCase>,
}

impl ConvergenceDesk {
    /// 空台。
    pub fn new() -> Self {
        ConvergenceDesk { cases: Vec::new() }
    }

    /// 立案（同一差异记录重复立案即拒）。
    pub fn open(&mut self, diff_idx: usize, cause: DiffCause) -> Result<(), u16> {
        if self.cases.iter().any(|c| c.diff_idx == diff_idx) {
            return Err(SEG_BASE + 20);
        }
        self.cases.push(DiffCase {
            diff_idx,
            cause,
            resolved: false,
        });
        Ok(())
    }

    /// 处置核销（未立案即拒；重复核销即拒）。
    pub fn resolve(&mut self, diff_idx: usize) -> Result<(), u16> {
        let c = match self.cases.iter_mut().find(|c| c.diff_idx == diff_idx) {
            Some(c) => c,
            None => return Err(SEG_BASE + 21),
        };
        if c.resolved {
            return Err(SEG_BASE + 22);
        }
        c.resolved = true;
        Ok(())
    }

    /// 待处置数。
    pub fn pending(&self) -> usize {
        self.cases.iter().filter(|c| !c.resolved).count()
    }

    /// 收敛判定（判据"收敛"：立案全覆盖且全部核销）。
    pub fn converged(&self) -> bool {
        !self.cases.is_empty() && self.pending() == 0
    }
}

impl Default for ConvergenceDesk {
    fn default() -> Self {
        Self::new()
    }
}

impl XCompilerLedger {
    /// 标志档影响审计（同编译器跨标志档的指纹差异留痕——证明收缩/
    /// 快速数学确实改变输出，反向支撑统一档的必要性）。
    /// 返回受审计的编译器个数。
    pub fn flag_impact_audit(&self) -> usize {
        let mut audited = 0usize;
        for c in Compiler::ALL {
            let strict = self
                .cells
                .iter()
                .find(|b| b.compiler == c && b.flags == FpFlags::StrictNoContractNoFast);
            let relaxed = self.cells.iter().find(|b| {
                b.compiler == c
                    && b.flags != FpFlags::StrictNoContractNoFast
                    && b.built
            });
            // 只有两档都有产出且指纹不同时才算"影响被证实"的审计完成。
            if let (Some(s), Some(r)) = (strict, relaxed) {
                if s.out_fp != r.out_fp {
                    audited += 1;
                }
            }
        }
        audited
    }

    /// 统一档指纹（判据"一致性断言"的读取面；未收敛时 None）。
    pub fn unified_fp(&self) -> Option<u64> {
        if !self.converged {
            return None;
        }
        self.cells
            .iter()
            .find(|b| b.flags == FpFlags::StrictNoContractNoFast && b.built)
            .map(|b| b.out_fp)
    }

    /// 构建失败清单（fail_code 非零的记录投影，CI 报告数据源）。
    pub fn failures(&self) -> Vec<(Compiler, FpFlags, u16)> {
        self.cells
            .iter()
            .filter(|b| !b.built)
            .map(|b| (b.compiler, b.flags, b.fail_code))
            .collect()
    }
}
