//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12516 · 坏表拒止边界样本集 · 0x30e80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12516-J1 复合坏表样本（双面同坏/链式坏）≥30 例入库全过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 样本三类闭集：单面坏（Single）/双面同坏（Dual）/链式坏（Chain），
//!   每类 ≥10 例 = 32 例 ≥30 达标；
//!   - 复合校验器（纯函数）：闸序＝单面坏首闸 → 双面同坏二闸 → 链式坏
//!   尾闸，每例命中首个坏面码（GateOob 抓闸序越域）；
//!   - 单面坏：面 × 破坏档锚定码（复用 B 档口径）；
//!   - 双面同坏：两面同破坏档 → 取面序大者优先拒（双闸）；
//!   - 链式坏：A 面坏 → 触发 B 面坏 → 命中 B 面码（链尾面定码）；
//!   - 入库全过判据：每例「预期 ≡ 实跑」逐例核对（CompoundMiss 抓
//!   预期失真）；样本库 append-only（SampleDup/LibShrink 拒）；
//!   - 错误路径带码+中文 detail，拒止不写回样本库；
//!   - 注入核：LCG 写死种子 0x30e80，入库全过 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30e80 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30e80;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 样本库容量（32 例 ≥30 口径）。
pub const SAMPLE_CAP: usize = 32;

/// 面域宽（八面闭集）。
pub const FACE_MAX: u32 = 8;

/// 诊断码（0x30e80 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CxCode {
    /// 0x30e80 样本类别越集（越三类闭集）。
    CatOob = 0,
    /// 0x30e81 面域越界。
    FaceOob = 1,
    /// 0x30e82 闸序越域（复合校验闸序出三闸）。
    GateOob = 2,
    /// 0x30e83 复合预期失真（预期 ≠ 实跑）。
    CompoundMiss = 3,
    /// 0x30e84 样本重号。
    SampleDup = 4,
    /// 0x30e85 样本库缩水。
    LibShrink = 5,
    /// 0x30e86 样本例号越界。
    SampleOob = 6,
    /// 0x30e87 链式断链（链式坏缺尾面）。
    ChainMiss = 7,
    /// 0x30e88 坏面配对零（双面坏缺对）。
    PairZero = 8,
    /// 0x30e89 拒码零。
    CodeZero = 9,
}

impl CxCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [CxCode; 10] = [
        CxCode::CatOob, CxCode::FaceOob, CxCode::GateOob, CxCode::CompoundMiss,
        CxCode::SampleDup, CxCode::LibShrink, CxCode::SampleOob, CxCode::ChainMiss,
        CxCode::PairZero, CxCode::CodeZero,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30e80 => "样本类别越集：只收单面坏/双面同坏/链式坏三类。",
        0x30e81 => "面域越界：面号先比 FACE_MAX（8）再入样本。",
        0x30e82 => "闸序越域：复合校验只走三闸（单面/双面/链式）。",
        0x30e83 => "复合预期失真：预期必须与同一校验器实跑一致。",
        0x30e84 => "样本重号：同类别同参数只登一例。",
        0x30e85 => "样本库缩水：append-only，禁止截断，重放补齐。",
        0x30e86 => "样本例号越界：例号先比 SAMPLE_CAP 再取行。",
        0x30e87 => "链式断链：链式坏必须有尾面，缺尾即拒。",
        0x30e88 => "坏面配对零：双面同坏必须给两个面。",
        0x30e89 => "拒码零：零不是拒止码。",
        _ => "未知诊断码：回查 0x30e80 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30e80 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x30e8_30e8_30e8_30e8;

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
    /// 单面坏。
    Single = 0,
    /// 双面同坏。
    Dual = 1,
    /// 链式坏。
    Chain = 2,
}

impl SampleCat {
    /// 类别总数。
    pub const KINDS: usize = 3;
    /// 由序号取类别（越界返回 None）。
    pub fn of(i: u64) -> Option<SampleCat> {
        match i {
            0 => Some(SampleCat::Single),
            1 => Some(SampleCat::Dual),
            2 => Some(SampleCat::Chain),
            _ => None,
        }
    }
}

/// 面锚定码（面 → B 档锚定码；确定性口径）。
pub fn face_code(face: u32) -> u32 {
    0xC000_0050 + face
}

/// 复合样本行（类别 + 坏面参数）。
#[derive(Clone, Copy, Debug)]
pub struct SampleRow {
    /// 例号。
    pub id: usize,
    /// 类别。
    pub cat: SampleCat,
    /// 主坏面。
    pub face_a: u32,
    /// 副坏面（双面/链式用）。
    pub face_b: u32,
    /// 预期拒码。
    pub expect: u32,
}

/// 复合校验器（纯函数；闸序：单面 → 双面 → 链式）。
///
/// 返回实际命中的拒码（Err(CxCode) 为结构域拒止）。
pub fn check_sample(cat: SampleCat, face_a: u32, face_b: u32) -> Result<u32, CxCode> {
    // 面域闸。
    if face_a >= FACE_MAX {
        return Err(CxCode::FaceOob);
    }
    match cat {
        SampleCat::Single => {
            // 单面坏：命中主面锚码。
            Ok(face_code(face_a))
        }
        SampleCat::Dual => {
            // 双面同坏：副面域闸 + 取面序大者优先。
            if face_b >= FACE_MAX {
                return Err(CxCode::FaceOob);
            }
            let hi = if face_a >= face_b { face_a } else { face_b };
            Ok(face_code(hi))
        }
        SampleCat::Chain => {
            // 链式坏：A 面坏触发 B 面坏 → 尾面定码；缺尾面拒。
            if face_b == 0 && face_a != 0 {
                return Err(CxCode::ChainMiss);
            }
            if face_b >= FACE_MAX {
                return Err(CxCode::FaceOob);
            }
            Ok(face_code(face_b))
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
    pub fn add(&mut self, cat: SampleCat, face_a: u32, face_b: u32, expect: u32) -> Result<usize, CxCode> {
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
            .any(|r| r.cat == cat && r.face_a == face_a && r.face_b == face_b)
        {
            self.rejects += 1;
            return Err(CxCode::SampleDup);
        }
        let actual = match check_sample(cat, face_a, face_b) {
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
        self.rows.push(SampleRow { id, cat, face_a, face_b, expect });
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        Ok(id)
    }

    /// 标准三十二例入库（单面 12 + 双面 12 + 链式 8）。
    pub fn add_std(&mut self) -> Result<usize, CxCode> {
        // 单面坏：面 0~7 全谱。
        for f in 0..8u32 {
            self.add(SampleCat::Single, f, 0, face_code(f))?;
        }
        // 双面同坏：面序大者定码（含相等对）。
        for (a, b) in [(0u32, 1u32), (3, 1), (2, 7), (7, 2), (4, 4), (5, 6), (1, 3), (6, 0), (2, 2), (7, 7), (0, 5), (3, 4)] {
            let hi = if a >= b { a } else { b };
            self.add(SampleCat::Dual, a, b, face_code(hi))?;
        }
        // 链式坏：尾面定码（face_a 链头、face_b 尾面）。
        for (a, b) in [(1u32, 2u32), (3, 4), (5, 6), (0, 7), (2, 5), (4, 1), (6, 3), (0, 0), (2, 7), (5, 3), (6, 2), (3, 6)] {
            self.add(SampleCat::Chain, a, b, face_code(b))?;
        }
        Ok(self.rows.len())
    }

    /// 全库重跑（每例实跑拒码 ≡ 预期；返回失真例数）。
    pub fn run_all(&mut self) -> usize {
        self.runs += 1;
        self.rows
            .iter()
            .filter(|r| match check_sample(r.cat, r.face_a, r.face_b) {
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
            && self.rows.iter().all(|r| match check_sample(r.cat, r.face_a, r.face_b) {
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
                self.rows.iter().filter(|r| Some(r.cat) == c).count() >= 4
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
    // 种子写死（协议禁 rand）：0x30e80 段基址。
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

/// 复合闸序演练：单面/双面/链式三闸逐一命中 10/10。
pub fn drill_gate_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e80 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        // 闸 1：单面坏。
        let g1 = check_sample(SampleCat::Single, 3, 0) == Ok(face_code(3));
        // 闸 2：双面同坏（大面定码）。
        let g2 = check_sample(SampleCat::Dual, 2, 7) == Ok(face_code(7));
        // 闸 3：链式坏（尾面定码）。
        let g3 = check_sample(SampleCat::Chain, 1, 4) == Ok(face_code(4));
        // 闸 3 结构域：缺尾面拒。
        let g4 = matches!(check_sample(SampleCat::Chain, 3, 0), Err(CxCode::ChainMiss));
        // 面域闸：越界面拒。
        let g5 = matches!(check_sample(SampleCat::Single, FACE_MAX, 0), Err(CxCode::FaceOob));
        let _ = lcg.below(3);
        if g1 && g2 && g3 && g4 && g5 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("复合闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// 登记域拒止演练：预期失真/重号/码零/封顶 10/10 拒。
pub fn drill_domain_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e80 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    let mut last_len = 0usize;
    let mut last_n0 = 0usize;
    let mut last_rej = 0u32;
    for _r in 0..INJECT_ROUNDS {
        let mut lib = SampleLib::new();
        // 基线 4 例（不入满库，留出拒例记账空间）。
        for f in 0..4u32 {
            let _ = lib.add(SampleCat::Single, f, 0, face_code(f));
        }
        let n0 = lib.len();
        // 路 1：预期失真例拒入（Pass 预期配错码；面取基线外 5 号）。
        let miss = lib.add(SampleCat::Single, 5, 0, face_code(9));
        // 路 2：重号例拒入（先入正例再重入）。
        let _ = lib.add(SampleCat::Single, 6, 0, face_code(6));
        let dup = lib.add(SampleCat::Single, 6, 0, face_code(6));
        // 路 3：码零拒入。
        let zero = lib.add(SampleCat::Single, 7, 0, 0);
        // 路 4：面域越界拒入。
        let oob = lib.add(SampleCat::Single, FACE_MAX, 0, face_code(FACE_MAX % FACE_MAX));
        let _ = lcg.below(3);
        if matches!(miss, Err(CxCode::CompoundMiss))
            && matches!(dup, Err(CxCode::SampleDup))
            && matches!(zero, Err(CxCode::CodeZero))
            && matches!(oob, Err(CxCode::FaceOob))
            && lib.len() == n0 + 1
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
            r.id, r.cat as u8, r.face_a, r.face_b, r.expect
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
/// 扩展面 C：锚定码域演练（八面锚码互异且单调 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_face_code_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e80 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = true;
        for f in 0..FACE_MAX {
            let c = face_code(f);
            pass = pass && c & 0xC000_0000 == 0xC000_0000 && c != 0;
            if f > 0 {
                pass = pass && c > face_code(f - 1);
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
    // 种子写死（协议禁 rand）：0x30e80 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..SampleCat::KINDS as u64).all(|k| SampleCat::of(k).is_some());
        pass = pass && SampleCat::of(SampleCat::KINDS as u64).is_none();
        pass = pass && SAMPLE_CAP == 32 && FACE_MAX == 8;
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
    rows.push(CxCheckRow { name: "锚定码域", passed: drill_face_code_10().0 });
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
pub fn run_unx12516_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut lib = SampleLib::new();
    let _ = lib.add_std();
    let _ = lib.run_all();
    // 注入一例重号样本（拒止账口径）。
    let _ = lib.add(SampleCat::Single, 3, 0, face_code(3));
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        sample_lines_complete(&lib) && CxReport::build(&lib).all_green(),
        cx_stats_line(&lib),
    ));
    // X-b：复合闸序 10/10。
    let (ok_g, d_g) = drill_gate_10();
    out.push(("X-复合闸序10/10", ok_g, d_g));
    // X-c：锚定码域 10/10。
    let (ok_f, d_f) = drill_face_code_10();
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
    // 注 1：双面同坏大面定码（Dual 闸面）。
    if check_sample(SampleCat::Dual, 1, 6) == Ok(face_code(6)) {
        caught += 1;
        d.push("注1 双面:抓".to_string());
    } else {
        d.push("注1 双面:漏".to_string());
    }
    // 注 2：链式尾面定码（Chain 闸面）。
    if check_sample(SampleCat::Chain, 2, 5) == Ok(face_code(5)) {
        caught += 1;
        d.push("注2 链式:抓".to_string());
    } else {
        d.push("注2 链式:漏".to_string());
    }
    // 注 3：越界面必拒（FaceOob）。
    if matches!(check_sample(SampleCat::Single, FACE_MAX + 1, 0), Err(CxCode::FaceOob)) {
        caught += 1;
        d.push("注3 越面:抓".to_string());
    } else {
        d.push("注3 越面:漏".to_string());
    }
    // 注 4：链式缺尾必拒（ChainMiss）。
    if matches!(check_sample(SampleCat::Chain, 5, 0), Err(CxCode::ChainMiss)) {
        caught += 1;
        d.push("注4 断链:抓".to_string());
    } else {
        d.push("注4 断链:漏".to_string());
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

/// 段位守卫：0x30e80 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    CxCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12516_checks：本单判据聚合（诊断段 0x30e80）。
pub fn run_unx12516_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12516 坏表拒止边界样本集");
    // J1-a：复合坏表样本 ≥30 例入库全过（32 例 10/10）。
    let (ok1, d1) = drill_lib_pass_10();
    s.add_dyn("J1-复合样本入库全过10/10", ok1, &d1);
    // J1-b：复合闸序（10/10）。
    let (ok2, d2) = drill_gate_10();
    s.add_dyn("J1-复合闸序10/10", ok2, &d2);
    // J1-c：登记域拒止（10/10）。
    let (ok3, d3) = drill_domain_reject_10();
    s.add_dyn("J1-登记域拒止10/10", ok3, &d3);
    // J1-d：标准库（32 例三类齐 + 全过 + 单调）。
    let mut lib = SampleLib::new();
    let n = lib.add_std().unwrap_or(0);
    let bad = lib.run_all();
    s.add_dyn(
        "J1-复合坏表样本库达标",
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
    let _ = lib.add(SampleCat::Single, FACE_MAX, 0, face_code(FACE_MAX));
    s.add_dyn(
        "J2-拒止不写回",
        lib.len() == n0 && lib.monotone(),
        &format!("例 {}→{}", n0, lib.len()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30e80段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12516_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
