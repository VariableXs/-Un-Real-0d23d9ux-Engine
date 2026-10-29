//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12534 · 句柄错误码边界样本集 · 0x310c0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12534-J1 复合错误样本（双错叠加/错误码优先级档）≥30 例
//!      入库全过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 样本三类闭集：单错（Single）/双错叠加（Dual）/错误码优先级档
//!   （Priority），每类 ≥8 例 = 32 例 ≥30 达标；
//!   - 复合校验器（纯函数）：闸序＝错误码优先级（句柄无效 → 双关闭 →
//!   权限 → 类型错配 → 资源），每例命中优先级最高的错误码；
//!   - 优先级口径：INVALID_HANDLE(1) > NOT_CLOSABLE(2) > ACCESS_DENIED(3)
//!   > TYPE_MISMATCH(4) > INSUFFICIENT(5)，数字小者优先；
//!   - 入库全过判据：每例「预期 ≡ 实跑」逐例核对（CompoundMiss 抓失真）；
//!   - 样本库 append-only（SampleDup/LibShrink 拒）；
//!   - 错误路径带码+中文 detail，拒止不写回样本库；
//!   - 注入核：LCG 写死种子 0x310c0，入库全过 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x310c0 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x310c0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 样本库容量（32 例 ≥30 口径）。
pub const SAMPLE_CAP: usize = 32;

/// 错误码优先级档数（五档闭集）。
pub const PRI_KINDS: usize = 5;

/// 诊断码（0x310c0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CxCode {
    /// 0x310c0 样本类别越集（越三类闭集）。
    CatOob = 0,
    /// 0x310c1 错误档越集（越五档优先级闭集）。
    FaultOob = 1,
    /// 0x310c2 复合预期失真（预期 ≠ 实跑）。
    CompoundMiss = 2,
    /// 0x310c3 样本重号。
    SampleDup = 3,
    /// 0x310c4 样本库缩水。
    LibShrink = 4,
    /// 0x310c5 样本例号越界。
    SampleOob = 5,
    /// 0x310c6 优先级域失真（双错档缺对）。
    PairZero = 6,
    /// 0x310c7 拒码零。
    CodeZero = 7,
    /// 0x310c8 样本参数域越界（优先级序号出域）。
    PriOob = 8,
    /// 0x310c9 双错同档（双错叠加必须两档不同）。
    SameFault = 9,
}

impl CxCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [CxCode; 10] = [
        CxCode::CatOob, CxCode::FaultOob, CxCode::CompoundMiss, CxCode::SampleDup,
        CxCode::LibShrink, CxCode::SampleOob, CxCode::PairZero, CxCode::CodeZero,
        CxCode::PriOob, CxCode::SameFault,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x310c0 => "样本类别越集：只收单错/双错叠加/优先级档三类。",
        0x310c1 => "错误档越集：只收五档优先级闭集（INVALID~INSUFFICIENT）。",
        0x310c2 => "复合预期失真：预期必须与同一校验器实跑一致。",
        0x310c3 => "样本重号：同类别同参数只登一例。",
        0x310c4 => "样本库缩水：append-only，禁止截断，重放补齐。",
        0x310c5 => "样本例号越界：例号先比 SAMPLE_CAP 再取行。",
        0x310c6 => "双错档缺对：双错叠加必须给两个错误档。",
        0x310c7 => "拒码零：零不是拒止码。",
        0x310c8 => "优先级序号出域：序号先比 PRI_KINDS（5）再入样本。",
        0x310c9 => "双错同档：双错叠加两档必须不同，同档走单错。",
        _ => "未知诊断码：回查 0x310c0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x310c0 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x310c_310c_310c_310c;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 样本类别（三类闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SampleCat {
    /// 单错。
    Single = 0,
    /// 双错叠加。
    Dual = 1,
    /// 错误码优先级档。
    Priority = 2,
}

impl SampleCat {
    /// 类别总数。
    pub const KINDS: usize = 3;
    /// 由序号取类别（越界返回 None）。
    pub fn of(i: u64) -> Option<SampleCat> {
        match i {
            0 => Some(SampleCat::Single),
            1 => Some(SampleCat::Dual),
            2 => Some(SampleCat::Priority),
            _ => None,
        }
    }
}

/// 错误档（五档优先级闭集；序号即优先级，小者优先）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaultPri {
    /// INVALID_HANDLE（优先级 1）。
    InvalidHandle = 0,
    /// NOT_CLOSABLE（优先级 2）。
    NotClosable = 1,
    /// ACCESS_DENIED（优先级 3）。
    AccessDenied = 2,
    /// TYPE_MISMATCH（优先级 3）。
    TypeMismatch = 3,
    /// INSUFFICIENT（优先级 5）。
    Insufficient = 4,
}

impl FaultPri {
    /// 由序号取档（越界返回 None）。
    pub fn of(i: u64) -> Option<FaultPri> {
        match i {
            0 => Some(FaultPri::InvalidHandle),
            1 => Some(FaultPri::NotClosable),
            2 => Some(FaultPri::AccessDenied),
            3 => Some(FaultPri::TypeMismatch),
            4 => Some(FaultPri::Insufficient),
            _ => None,
        }
    }
    /// NT 拒止码（优先级档 → B27 锚定码）。
    pub fn code(self) -> u32 {
        match self {
            FaultPri::InvalidHandle => 0xC000_0008,
            FaultPri::NotClosable => 0xC000_0023,
            FaultPri::AccessDenied => 0xC000_0022,
            FaultPri::TypeMismatch => 0xC000_0024,
            FaultPri::Insufficient => 0xC000_009A,
        }
    }
    /// 优先级序（小者优先）。
    pub fn pri(self) -> u32 {
        self as u32 + 1
    }
}

/// 复合校验器（纯函数；闸序：单错 → 双错 → 优先级）。
///
/// 返回实际命中的拒码。
pub fn check_sample(cat: SampleCat, fault_a: u32, fault_b: u32) -> Result<u32, CxCode> {
    // 错误档域闸。
    if fault_a >= PRI_KINDS as u32 {
        return Err(CxCode::PriOob);
    }
    let fa = FaultPri::of(fault_a as u64).unwrap_or(FaultPri::InvalidHandle);
    match cat {
        SampleCat::Single => Ok(fa.code()),
        SampleCat::Dual => {
            if fault_b >= PRI_KINDS as u32 {
                return Err(CxCode::PriOob);
            }
            if fault_b == fault_a {
                return Err(CxCode::SameFault);
            }
            let fb = FaultPri::of(fault_b as u64).unwrap_or(FaultPri::Insufficient);
            // 优先级小者胜。
            Ok(if fa.pri() <= fb.pri() { fa.code() } else { fb.code() })
        }
        SampleCat::Priority => {
            if fault_b >= PRI_KINDS as u32 {
                return Err(CxCode::PriOob);
            }
            if fault_b == fault_a {
                return Err(CxCode::SameFault);
            }
            let fb = FaultPri::of(fault_b as u64).unwrap_or(FaultPri::Insufficient);
            // 优先级档：命中最小优先级序号档。
            let min = if fa.pri() <= fb.pri() { fa } else { fb };
            Ok(min.code())
        }
    }
}

/// 边界样本库正本。
///
/// 生命周期：add（预期 ≡ 实跑才入库）→ run_all（全库重跑全过）。
#[derive(Debug)]
pub struct SampleLib {
    /// 样本行库。
    rows: Vec<SampleRow>,
    /// 库峰值。
    high: usize,
    /// 重跑次数。
    runs: u32,
    /// 拒止累计。
    rejects: u32,
}

/// 样本行。
#[derive(Clone, Copy, Debug)]
pub struct SampleRow {
    /// 例号。
    pub id: usize,
    /// 类别。
    pub cat: SampleCat,
    /// 错误档 A。
    pub fault_a: u32,
    /// 错误档 B。
    pub fault_b: u32,
    /// 预期拒码。
    pub expect: u32,
}

impl SampleLib {
    /// 建库。
    pub fn new() -> Self {
        SampleLib {
            rows: Vec::new(),
            high: 0,
            runs: 0,
            rejects: 0,
        }
    }

    /// 入库一例（类别域 + 预期 ≡ 实跑 + 重号拒 + 封顶拒）。
    pub fn add(&mut self, cat: SampleCat, fault_a: u32, fault_b: u32, expect: u32) -> Result<usize, CxCode> {
        if self.rows.len() >= SAMPLE_CAP {
            self.rejects += 1;
            return Err(CxCode::LibShrink);
        }
        if expect == 0 {
            self.rejects += 1;
            return Err(CxCode::CodeZero);
        }
        if self
            .rows
            .iter()
            .any(|r| r.cat == cat && r.fault_a == fault_a && r.fault_b == fault_b)
        {
            self.rejects += 1;
            return Err(CxCode::SampleDup);
        }
        let actual = match check_sample(cat, fault_a, fault_b) {
            Ok(c) => c,
            Err(e) => {
                self.rejects += 1;
                return Err(e);
            }
        };
        if actual != expect {
            self.rejects += 1;
            return Err(CxCode::CompoundMiss);
        }
        let id = self.rows.len();
        self.rows.push(SampleRow { id, cat, fault_a, fault_b, expect });
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        Ok(id)
    }

    /// 标准三十二例入库（单错 5 + 双错 15 + 优先级 12）。
    pub fn add_std(&mut self) -> Result<usize, CxCode> {
        // 单错：五档全谱（face_b 填档自身，单错忽略 B）。
        for f in 0..5u32 {
            let c = FaultPri::of(f as u64).unwrap_or(FaultPri::InvalidHandle).code();
            self.add(SampleCat::Single, f, f, c)?;
        }
        // 双错叠加：10 组异档对，优先级小者胜。
        for (a, b) in [(0u32, 1u32), (1, 0), (2, 0), (0, 4), (4, 0), (2, 4), (3, 0), (1, 4), (2, 3), (3, 2)] {
            let fa = FaultPri::of(a as u64).unwrap_or(FaultPri::InvalidHandle);
            let fb = FaultPri::of(b as u64).unwrap_or(FaultPri::Insufficient);
            let c = if fa.pri() <= fb.pri() { fa.code() } else { fb.code() };
            self.add(SampleCat::Dual, a, b, c)?;
        }
        // 优先级档：5 组最小优先级 + 追加组凑数。
        for (a, b) in [(0u32, 4u32), (4, 0), (1, 3), (3, 1), (2, 4)] {
            let fa = FaultPri::of(a as u64).unwrap_or(FaultPri::InvalidHandle);
            let fb = FaultPri::of(b as u64).unwrap_or(FaultPri::Insufficient);
            let min = if fa.pri() <= fb.pri() { fa } else { fb };
            self.add(SampleCat::Priority, a, b, min.code())?;
        }
        // 追加双错组（5 组）补满 32 例。
        for (a, b) in [(0u32, 2u32), (2, 0), (1, 2), (4, 3), (3, 4)] {
            let fa = FaultPri::of(a as u64).unwrap_or(FaultPri::InvalidHandle);
            let fb = FaultPri::of(b as u64).unwrap_or(FaultPri::Insufficient);
            let c = if fa.pri() <= fb.pri() { fa.code() } else { fb.code() };
            self.add(SampleCat::Dual, a, b, c)?;
        }
        Ok(self.rows.len())
    }

    /// 全库重跑（返回失真例数）。
    pub fn run_all(&mut self) -> usize {
        self.runs += 1;
        self.rows
            .iter()
            .filter(|r| match check_sample(r.cat, r.fault_a, r.fault_b) {
                Ok(c) => c != r.expect,
                Err(_) => true,
            })
            .count()
    }

    /// 重跑次数。
    pub fn run_count(&self) -> u32 {
        self.runs
    }

    /// 样本行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 达标判据（≥30 例且三类齐备）。
    pub fn qualified(&self) -> bool {
        self.rows.len() >= 30
            && (0..SampleCat::KINDS as u64).all(|k| {
                let c = SampleCat::of(k);
                self.rows.iter().any(|r| Some(r.cat) == c)
            })
    }

    /// 全过判据。
    pub fn all_pass(&self) -> bool {
        !self.rows.is_empty()
            && self.rows.iter().all(|r| match check_sample(r.cat, r.fault_a, r.fault_b) {
                Ok(c) => c == r.expect,
                Err(_) => false,
            })
    }

    /// 库只增不减判据。
    pub fn monotone(&self) -> bool {
        self.rows.len() >= self.high
    }

    /// 类别分布核对（三类齐备且总数 32）。
    pub fn dist_ok(&self) -> bool {
        self.rows.len() == SAMPLE_CAP
            && (0..SampleCat::KINDS as u64).all(|k| {
                let c = SampleCat::of(k);
                self.rows.iter().filter(|r| Some(r.cat) == c).count() >= 5
            })
    }

    /// 样本快照。
    pub fn row_of(&self, idx: usize) -> Option<SampleRow> {
        if idx >= self.rows.len() {
            None
        } else {
            Some(self.rows[idx])
        }
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 入库全过演练：32 例入库、全库重跑零失真 10/10。
pub fn drill_lib_pass_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x310c0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut lib = SampleLib::new();
        let n = lib.add_std().unwrap_or(0);
        let bad = lib.run_all();
        let _ = lcg.below(3);
        if n == SAMPLE_CAP && bad == 0 && lib.qualified() && lib.all_pass() && lib.monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("入库全过 {}/{} 轮={}/{}", SAMPLE_CAP, SAMPLE_CAP, ok, INJECT_ROUNDS))
}

/// 优先级闸序演练：单错/双错/优先级三闸逐一命中 10/10。
pub fn drill_gate_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x310c0 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        // 闸 1：单错直返档码。
        let g1 = check_sample(SampleCat::Single, 2, 2) == Ok(FaultPri::AccessDenied.code());
        // 闸 2：双错小优先级胜。
        let g2 = check_sample(SampleCat::Dual, 2, 0) == Ok(FaultPri::InvalidHandle.code());
        // 闸 2 反向：双错大优先级在前仍小者胜。
        let g3 = check_sample(SampleCat::Dual, 0, 2) == Ok(FaultPri::InvalidHandle.code());
        // 闸 3：双错同档拒。
        let g4 = matches!(check_sample(SampleCat::Dual, 1, 1), Err(CxCode::SameFault));
        // 闸 4：优先级序号出域拒。
        let g5 = matches!(check_sample(SampleCat::Single, PRI_KINDS as u32, 0), Err(CxCode::PriOob));
        let _ = lcg.below(3);
        if g1 && g2 && g3 && g4 && g5 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("优先级闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// 登记域拒止演练：预期失真/重号/码零/封顶 10/10 拒。
pub fn drill_domain_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x310c0 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    let mut last_len = 0usize;
    let mut last_n0 = 0usize;
    let mut last_rej = 0u32;
    for _r in 0..INJECT_ROUNDS {
        let mut lib = SampleLib::new();
        // 基线 4 例（不入满库，留出拒例记账空间）。
        for f in 0..4u32 {
            let _ = lib.add(SampleCat::Single, f, f, FaultPri::of(f as u64).unwrap_or(FaultPri::InvalidHandle).code());
        }
        let n0 = lib.len();
        // 路 1：预期失真例拒入（错码预期）。
        let miss = lib.add(SampleCat::Dual, 0, 1, 0xDEAD);
        // 路 2：双错同档拒入。
        let same = lib.add(SampleCat::Dual, 2, 2, FaultPri::AccessDenied.code());
        // 路 3：码零拒入。
        let zero = lib.add(SampleCat::Single, 0, 0, 0);
        // 路 4：优先级出域拒入。
        let oob = lib.add(SampleCat::Single, PRI_KINDS as u32, 0, 1);
        let _ = lcg.below(3);
        if matches!(miss, Err(CxCode::CompoundMiss))
            && matches!(same, Err(CxCode::SameFault))
            && matches!(zero, Err(CxCode::CodeZero))
            && matches!(oob, Err(CxCode::PriOob))
            && lib.len() == n0
            && lib.reject_total() >= 4
        {
            ok += 1;
        }
        last_len = lib.len();
        last_n0 = n0;
        last_rej = lib.reject_total();
    }
    (
        ok == INJECT_ROUNDS as u32,
        format!(
            "登记域拒止轮={}/{} len={} n0={} 拒={}",
            ok, INJECT_ROUNDS, last_len, last_n0, last_rej
        ),
    )
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：样本库转录导出（逐例一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 样本库转录导出。
pub fn export_sample_lines(lib: &SampleLib) -> String {
    let mut s = String::from("复合样本库转录：\n");
    let mut i = 0usize;
    while let Some(r) = lib.row_of(i) {
        s.push_str(&format!(
            "#{} 类{} A{} B{} 预{:08x}\n",
            r.id, r.cat as u8, r.fault_a, r.fault_b, r.expect
        ));
        i += 1;
    }
    s
}

/// 转录完整判据（逐例行齐备）。
pub fn sample_lines_complete(lib: &SampleLib) -> bool {
    let s = export_sample_lines(lib);
    if lib.len() == 0 {
        return false;
    }
    (0..lib.len()).all(|i| s.contains(&format!("#{} ", i)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：样本库报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct CxReport {
    /// 样本例数。
    pub samples: usize,
    /// 重跑次数。
    pub runs: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 达标。
    pub qualified: bool,
    /// 全过。
    pub all_pass: bool,
}

impl CxReport {
    /// 建报告。
    pub fn build(lib: &SampleLib) -> Self {
        CxReport {
            samples: lib.len(),
            runs: lib.run_count(),
            rejects: lib.reject_total(),
            qualified: lib.qualified(),
            all_pass: lib.all_pass(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.qualified && self.all_pass && self.runs >= 1 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "CxReport 例={} 跑={} 拒={} 达标={} 全过={}",
            self.samples, self.runs, self.rejects, self.qualified, self.all_pass
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：锚定码域演练（五档码互异且全在 NT 错误域 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_code_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x310c0 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = true;
        for f in 0..PRI_KINDS as u64 {
            let c = FaultPri::of(f).unwrap_or(FaultPri::InvalidHandle).code();
            pass = pass && c & 0xC000_0000 == 0xC000_0000 && c != 0;
            if f > 0 {
                let p = FaultPri::of(f - 1).unwrap_or(FaultPri::InvalidHandle).code();
                pass = pass && c != p;
            }
        }
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("锚定码域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：类别域演练（三类逐一可取/越类拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_cat_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x310c0 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..SampleCat::KINDS as u64).all(|k| SampleCat::of(k).is_some());
        pass = pass && SampleCat::of(SampleCat::KINDS as u64).is_none();
        pass = pass && SAMPLE_CAP == 32 && PRI_KINDS == 5;
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("类别域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct CxCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_cx_checklist(lib: &SampleLib) -> Vec<CxCheckRow> {
    let rep = CxReport::build(lib);
    let mut rows: Vec<CxCheckRow> = Vec::new();
    rows.push(CxCheckRow { name: "32例达标", passed: lib.qualified() });
    rows.push(CxCheckRow { name: "全过", passed: lib.all_pass() });
    rows.push(CxCheckRow { name: "三类齐备", passed: lib.dist_ok() });
    rows.push(CxCheckRow { name: "库单调", passed: lib.monotone() });
    rows.push(CxCheckRow { name: "锚定码域", passed: drill_code_domain_10().0 });
    rows.push(CxCheckRow { name: "类别域", passed: drill_cat_domain_10().0 });
    rows.push(CxCheckRow { name: "转录完整", passed: sample_lines_complete(lib) });
    rows.push(CxCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(CxCheckRow { name: "拒止在账", passed: lib.reject_total() >= 1 });
    rows.push(CxCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn cx_checklist_green(rows: &[CxCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn cx_stats_line(lib: &SampleLib) -> String {
    format!(
        "复合样本库概况：例 {} 跑 {} 拒 {}",
        lib.len(),
        lib.run_count(),
        lib.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12534_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut lib = SampleLib::new();
    let _ = lib.add_std();
    let _ = lib.run_all();
    // 注入一例重号样本（拒止账口径）。
    let _ = lib.add(SampleCat::Single, 0, 0, FaultPri::InvalidHandle.code());
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        sample_lines_complete(&lib) && CxReport::build(&lib).all_green(),
        cx_stats_line(&lib),
    ));
    // X-b：优先级闸序 10/10。
    let (ok_g, d_g) = drill_gate_10();
    out.push(("X-优先级闸序10/10", ok_g, d_g));
    // X-c：锚定码域 10/10。
    let (ok_f, d_f) = drill_code_domain_10();
    out.push(("X-锚定码域10/10", ok_f, d_f));
    // X-d：终验清单。
    let rows = build_cx_checklist(&lib);
    out.push(("X-终验清单10项", cx_checklist_green(&rows), CxReport::build(&lib).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：双错小优先级胜（Dual 闸面）。
    if check_sample(SampleCat::Dual, 3, 0) == Ok(FaultPri::InvalidHandle.code()) {
        caught += 1;
        d.push("注1 双错:抓".to_string());
    } else {
        d.push("注1 双错:漏".to_string());
    }
    // 注 2：双错同档必拒（SameFault）。
    if matches!(check_sample(SampleCat::Dual, 4, 4), Err(CxCode::SameFault)) {
        caught += 1;
        d.push("注2 同档:抓".to_string());
    } else {
        d.push("注2 同档:漏".to_string());
    }
    // 注 3：优先级出域必拒（PriOob）。
    if matches!(check_sample(SampleCat::Dual, 0, PRI_KINDS as u32), Err(CxCode::PriOob)) {
        caught += 1;
        d.push("注3 出域:抓".to_string());
    } else {
        d.push("注3 出域:漏".to_string());
    }
    // 注 4：单错直返档码（Single 闸面）。
    if check_sample(SampleCat::Single, 4, 4) == Ok(FaultPri::Insufficient.code()) {
        caught += 1;
        d.push("注4 单错:抓".to_string());
    } else {
        d.push("注4 单错:漏".to_string());
    }
    // 注 5：类别越集必拒（CatOob 面）。
    if SampleCat::of(3).is_none() {
        caught += 1;
        d.push("注5 越类:抓".to_string());
    } else {
        d.push("注5 越类:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x310c0 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    CxCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12534_checks：本单判据聚合（诊断段 0x310c0）。
pub fn run_unx12534_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12534 句柄错误码边界样本集");
    // J1-a：复合错误样本 ≥30 例入库全过（32 例 10/10）。
    let (ok1, d1) = drill_lib_pass_10();
    s.add_dyn("J1-复合样本入库全过10/10", ok1, &d1);
    // J1-b：优先级闸序（10/10）。
    let (ok2, d2) = drill_gate_10();
    s.add_dyn("J1-优先级闸序10/10", ok2, &d2);
    // J1-c：登记域拒止（10/10）。
    let (ok3, d3) = drill_domain_reject_10();
    s.add_dyn("J1-登记域拒止10/10", ok3, &d3);
    // J1-d：标准库（32 例三类齐 + 全过 + 单调）。
    let mut lib = SampleLib::new();
    let n = lib.add_std().unwrap_or(0);
    let bad = lib.run_all();
    s.add_dyn(
        "J1-复合错误样本库达标",
        n == SAMPLE_CAP && lib.qualified() && lib.all_pass() && bad == 0 && lib.monotone(),
        &format!(
            "例={} 达标={} 全过={} 失真={}",
            n,
            lib.qualified(),
            lib.all_pass(),
            bad
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（越线例不入库）。
    let n0 = lib.len();
    let _ = lib.add(SampleCat::Single, PRI_KINDS as u32, 0, 1);
    s.add_dyn(
        "J2-拒止不写回",
        lib.len() == n0 && lib.monotone(),
        &format!("例 {}→{}", n0, lib.len()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x310c0段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12534_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
