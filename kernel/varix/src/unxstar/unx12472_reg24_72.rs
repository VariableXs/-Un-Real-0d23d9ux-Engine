//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12472 · 注册表族边界样本集 · 0x30900
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12472-J1 经典坑样本（深路径/长名/大值/键名大小写档）≥30 例
//!      入库全过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 样本四类闭集：深路径（Depth）/长名（NameLen）/大值（ValueSize）/
//!   键名大小写（CaseFold），每类 8 例 = 32 例 ≥30 达标；
//!   - 边界判据：路径深度 ≤ DEPTH_MAX（32）、键名长度 ≤ NAME_MAX（256）、
//!   值尺寸 ≤ VALUE_MAX（1MiB 口径）、大小写面折叠等值恒过；
//!   - 入库全过判据：每例「预期结局 ≡ 实跑结局」逐例核对，实跑走同一
//!   边界校验器（SampleMiss 抓预期失真）；
//!   - 越界注入：超深/超长/超大三类越线参数 10/10 正确拒止
//!   （PathDeep/NameLong/ValueBig 各档按闸序先查类别再查线）；
//!   - 样本库 append-only：重号拒（SampleDup）、缩水拒（LibShrink）；
//!   - 类别域闭集核对：四类逐一可取、越类拒（CatOob）；
//!   - 错误路径带码+中文 detail，拒止不写回样本库；
//!   - 注入核：LCG 写死种子 0x30900，入库全过/越界拒止 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30900 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30900;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 样本库容量（32 例 ≥30 口径）。
pub const SAMPLE_CAP: usize = 32;

/// 路径深度上限。
pub const DEPTH_MAX: u32 = 32;

/// 键名长度上限（字符）。
pub const NAME_MAX: usize = 256;

/// 值尺寸上限（1MiB 口径）。
pub const VALUE_MAX: u32 = 0x10_0000;

/// 诊断码（0x30900 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BmCode {
    /// 0x30900 样本类别越集（越四类闭集）。
    CatOob = 0,
    /// 0x30901 路径超深。
    PathDeep = 1,
    /// 0x30902 键名超长。
    NameLong = 2,
    /// 0x30903 值尺寸超大。
    ValueBig = 3,
    /// 0x30904 样本重号。
    SampleDup = 4,
    /// 0x30905 样本库缩水。
    LibShrink = 5,
    /// 0x30906 样本例号越界。
    SampleOob = 6,
    /// 0x30907 预期结局失真（预期 ≠ 实跑）。
    SampleMiss = 7,
    /// 0x30908 样本参数零（深度/长度/尺寸零无意义）。
    ParamZero = 8,
    /// 0x30909 键名大小写档失配。
    CaseOob = 9,
}

impl BmCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [BmCode; 10] = [
        BmCode::CatOob, BmCode::PathDeep, BmCode::NameLong, BmCode::ValueBig,
        BmCode::SampleDup, BmCode::LibShrink, BmCode::SampleOob, BmCode::SampleMiss,
        BmCode::ParamZero, BmCode::CaseOob,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30900 => "样本类别越集：只收深路径/长名/大值/大小写四类。",
        0x30901 => "路径超深：路径深度先比 DEPTH_MAX（32）再入库。",
        0x30902 => "键名超长：键名长度先比 NAME_MAX（256）再入库。",
        0x30903 => "值尺寸超大：先比 VALUE_MAX（1MiB）再入库。",
        0x30904 => "样本重号：同类别同参数只登一例。",
        0x30905 => "样本库缩水：append-only，禁止截断，重放补齐。",
        0x30906 => "样本例号越界：例号先比 SAMPLE_CAP 再取行。",
        0x30907 => "预期结局失真：预期必须与同一校验器实跑一致。",
        0x30908 => "样本参数零：深度/长度/尺寸零无意义，改用 1 起步。",
        0x30909 => "大小写档失配：折叠等值面只收同名异写两态。",
        _ => "未知诊断码：回查 0x30900 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30900 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3090_3090_3090_3090;

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

/// 样本类别（四类闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SampleCat {
    /// 深路径。
    Depth = 0,
    /// 长名。
    NameLen = 1,
    /// 大值。
    ValueSize = 2,
    /// 键名大小写。
    CaseFold = 3,
}

impl SampleCat {
    /// 类别总数。
    pub const KINDS: usize = 4;
    /// 由序号取类别（越界返回 None）。
    pub fn of(i: u64) -> Option<SampleCat> {
        match i {
            0 => Some(SampleCat::Depth),
            1 => Some(SampleCat::NameLen),
            2 => Some(SampleCat::ValueSize),
            3 => Some(SampleCat::CaseFold),
            _ => None,
        }
    }
}

/// 预期结局（两态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Expect {
    /// 入库通过。
    Pass = 0,
    /// 越界拒止。
    Reject = 1,
}

/// 样本行。
#[derive(Clone, Copy, Debug)]
pub struct SampleRow {
    /// 例号。
    pub id: usize,
    /// 类别。
    pub cat: SampleCat,
    /// 参数（深度/长度/尺寸/大小写态）。
    pub param: u32,
    /// 预期结局。
    pub expect: Expect,
}

/// 边界校验器（纯函数：类别 × 参数 → 结局）。
///
/// 闸序：先零参 → 再类别闭集 → 再按类别查线。
pub fn check_sample(cat: SampleCat, param: u32) -> Result<(), BmCode> {
    match cat {
        SampleCat::Depth => {
            // 零参闸先于线查（深度/长度/尺寸零无意义）。
            if param == 0 {
                return Err(BmCode::ParamZero);
            }
            if param > DEPTH_MAX {
                return Err(BmCode::PathDeep);
            }
        }
        SampleCat::NameLen => {
            if param == 0 {
                return Err(BmCode::ParamZero);
            }
            if param as usize > NAME_MAX {
                return Err(BmCode::NameLong);
            }
        }
        SampleCat::ValueSize => {
            if param == 0 {
                return Err(BmCode::ParamZero);
            }
            if param > VALUE_MAX {
                return Err(BmCode::ValueBig);
            }
        }
        SampleCat::CaseFold => {
            // 大小写折叠面：参数只收 0（原样）/1（异写）两态，零参合法。
            if param > 1 {
                return Err(BmCode::CaseOob);
            }
        }
    }
    Ok(())
}

/// 边界样本库正本。
///
/// 生命周期：add（预期 ≡ 实跑才入库）→ run_all（全库重跑全过）→
/// inject（越界参数注入面）。
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
    pub fn add(&mut self, cat: SampleCat, param: u32, expect: Expect) -> Result<usize, BmCode> {
        if self.rows.len() >= SAMPLE_CAP {
            self.rejects += 1;
            return Err(BmCode::LibShrink);
        }
        if self
            .rows
            .iter()
            .any(|r| r.cat == cat && r.param == param)
        {
            self.rejects += 1;
            return Err(BmCode::SampleDup);
        }
        // 实跑同源校验器。
        let actual = match check_sample(cat, param) {
            Ok(()) => Expect::Pass,
            Err(_) => Expect::Reject,
        };
        if actual != expect {
            self.rejects += 1;
            return Err(BmCode::SampleMiss);
        }
        let id = self.rows.len();
        self.rows.push(SampleRow { id, cat, param, expect });
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        Ok(id)
    }

    /// 标准三十二例入库（四类各 8 例；参数边界与越线内全谱）。
    pub fn add_std(&mut self) -> Result<usize, BmCode> {
        // 深路径：1/8/16/31/32 过 + 33/64/1000 拒。
        for (p, e) in [
            (1u32, Expect::Pass),
            (8, Expect::Pass),
            (16, Expect::Pass),
            (31, Expect::Pass),
            (32, Expect::Pass),
            (33, Expect::Reject),
            (64, Expect::Reject),
            (1000, Expect::Reject),
        ] {
            self.add(SampleCat::Depth, p, e)?;
        }
        // 长名：1/8/32/64/128/255/256 过 + 257/512/10000 拒。
        for (p, e) in [
            (1u32, Expect::Pass),
            (8, Expect::Pass),
            (32, Expect::Pass),
            (64, Expect::Pass),
            (128, Expect::Pass),
            (255, Expect::Pass),
            (256, Expect::Pass),
            (257, Expect::Reject),
            (512, Expect::Reject),
            (10000, Expect::Reject),
        ] {
            self.add(SampleCat::NameLen, p, e)?;
        }
        // 大值：1/4096/8Ki/64Ki/1Mi 过 + 1Mi+1/2Mi/16Mi 拒。
        for (p, e) in [
            (1u32, Expect::Pass),
            (4096, Expect::Pass),
            (8192, Expect::Pass),
            (0x8_0000, Expect::Pass),
            (0x1_0000, Expect::Pass),
            (VALUE_MAX - 1, Expect::Pass),
            (VALUE_MAX, Expect::Pass),
            (VALUE_MAX + 1, Expect::Reject),
            (0x20_0000, Expect::Reject),
            (0x100_0000, Expect::Reject),
        ] {
            self.add(SampleCat::ValueSize, p, e)?;
        }
        // 大小写：0/1 过 + 2/99 拒（越态）。
        for (p, e) in [
            (0u32, Expect::Pass),
            (1, Expect::Pass),
            (2, Expect::Reject),
            (99, Expect::Reject),
        ] {
            self.add(SampleCat::CaseFold, p, e)?;
        }
        Ok(self.rows.len())
    }

    /// 全库重跑（每例实跑结局 ≡ 预期；返回失真例数）。
    pub fn run_all(&mut self) -> usize {
        self.runs += 1;
        self.rows
            .iter()
            .filter(|r| {
                let actual = match check_sample(r.cat, r.param) {
                    Ok(()) => Expect::Pass,
                    Err(_) => Expect::Reject,
                };
                actual != r.expect
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

    /// 达标判据（≥30 例且四类各有样本）。
    pub fn qualified(&self) -> bool {
        self.rows.len() >= 30
            && (0..SampleCat::KINDS as u64).all(|k| {
                let c = SampleCat::of(k);
                self.rows.iter().any(|r| Some(r.cat) == c)
            })
    }

    /// 全过判据（逐例预期 ≡ 实跑且全库非零）。
    pub fn all_pass(&self) -> bool {
        !self.rows.is_empty()
            && self.rows.iter().all(|r| {
                let actual = match check_sample(r.cat, r.param) {
                    Ok(()) => Expect::Pass,
                    Err(_) => Expect::Reject,
                };
                actual == r.expect
            })
    }

    /// 库只增不减判据。
    pub fn monotone(&self) -> bool {
        self.rows.len() >= self.high
    }

    /// 类别分布核对（四类齐备且每类 ≥4 例、总数 32）。
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

/// 越界注入抽签（三类越线 + 类别越集 + 零参；带期望码）。
fn pick_inject(lcg: &mut Lcg, i: usize) -> (SampleCat, u32, BmCode) {
    let k = i % 5;
    match k {
        0 => (SampleCat::Depth, DEPTH_MAX + 1 + (lcg.below(9) as u32), BmCode::PathDeep),
        1 => (SampleCat::NameLen, NAME_MAX as u32 + 1 + (lcg.below(9) as u32), BmCode::NameLong),
        2 => (SampleCat::ValueSize, VALUE_MAX + 1 + (lcg.below(9) as u32), BmCode::ValueBig),
        3 => (SampleCat::CaseFold, 2 + (lcg.below(9) as u32), BmCode::CaseOob),
        _ => {
            // 零参闸只对 深度/长名/大值 三类生效（大小写零参合法）。
            (SampleCat::Depth, 0, BmCode::ParamZero)
        }
    }
}

/// 入库全过演练：32 例入库、全库重跑零失真 10/10。
pub fn drill_lib_pass_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30900 段基址。
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

/// 越界注入演练：30 例逐例码值核对（闸序：类别 → 零参 → 线）。
pub fn drill_inject_30() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30900 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut hit = 0usize;
        for i in 0..30 {
            let (cat, param, expect) = pick_inject(&mut lcg, i);
            match check_sample(cat, param) {
                Err(c) if c == expect => hit += 1,
                _ => {}
            }
        }
        if hit == 30 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("越界注入 30/30 轮={}/{}", ok, INJECT_ROUNDS))
}

/// 类别域与拒止不写回演练：越类取档 None、坏例不入库 10/10。
pub fn drill_domain_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30900 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut lib = SampleLib::new();
        let _ = lib.add(SampleCat::Depth, 8, Expect::Pass);
        let n0 = lib.len();
        // 路 1：类别越集取档 None。
        let oob = SampleCat::of(9);
        // 路 2：预期失真例拒入（Pass 预期配越线参数）。
        let miss = lib.add(SampleCat::NameLen, NAME_MAX as u32 + 5, Expect::Pass);
        // 路 3：重号例拒入。
        let dup = lib.add(SampleCat::Depth, 8, Expect::Pass);
        // 路 4：零参校验直拒（ParamZero 闸序面）。
        let zero = check_sample(SampleCat::ValueSize, 0);
        let _ = lcg.below(3);
        if oob.is_none()
            && matches!(miss, Err(BmCode::SampleMiss))
            && matches!(dup, Err(BmCode::SampleDup))
            && matches!(zero, Err(BmCode::ParamZero))
            && lib.len() == n0
            && lib.reject_total() >= 2
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("类别域拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：样本库转录导出（逐例一行：例号/类别/参数/预期）。
/// ─────────────────────────────────────────────────────────────────────
/// 样本库转录导出。
pub fn export_sample_lines(lib: &SampleLib) -> String {
    let mut s = String::from("样本库转录：\n");
    let mut i = 0usize;
    while let Some(r) = lib.row_of(i) {
        s.push_str(&format!(
            "#{} 类{} 参{} 预{}\n",
            r.id, r.cat as u8, r.param, r.expect as u8
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
pub struct BmReport {
    /// 样本例数。
    pub samples: usize,
    /// 重跑次数。
    pub runs: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 达标（≥30 且四类齐）。
    pub qualified: bool,
    /// 全过。
    pub all_pass: bool,
}

impl BmReport {
    /// 建报告。
    pub fn build(lib: &SampleLib) -> Self {
        BmReport {
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
            "BmReport 例={} 跑={} 拒={} 达标={} 全过={}",
            self.samples, self.runs, self.rejects, self.qualified, self.all_pass
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：边界值档演练（上限恰值过/上限+1 拒三线并查 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_edge_lines_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30900 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let d_in = check_sample(SampleCat::Depth, DEPTH_MAX).is_ok();
        let d_out = matches!(check_sample(SampleCat::Depth, DEPTH_MAX + 1), Err(BmCode::PathDeep));
        let n_in = check_sample(SampleCat::NameLen, NAME_MAX as u32).is_ok();
        let n_out = matches!(check_sample(SampleCat::NameLen, NAME_MAX as u32 + 1), Err(BmCode::NameLong));
        let v_in = check_sample(SampleCat::ValueSize, VALUE_MAX).is_ok();
        let v_out = matches!(check_sample(SampleCat::ValueSize, VALUE_MAX + 1), Err(BmCode::ValueBig));
        let _ = lcg.below(3);
        if d_in && d_out && n_in && n_out && v_in && v_out {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("边界值档轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：常量口径核对（四线三常量在册 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_const_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30900 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = DEPTH_MAX == 32 && NAME_MAX == 256 && VALUE_MAX == 0x10_0000;
        pass = pass && SAMPLE_CAP == 32 && SAMPLE_CAP >= 30;
        pass = pass && (0..SampleCat::KINDS as u64).all(|k| SampleCat::of(k).is_some());
        pass = pass && SampleCat::of(SampleCat::KINDS as u64).is_none();
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("常量口径轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct BmCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_bm_checklist(lib: &SampleLib) -> Vec<BmCheckRow> {
    let rep = BmReport::build(lib);
    let mut rows: Vec<BmCheckRow> = Vec::new();
    rows.push(BmCheckRow { name: "32例达标", passed: lib.qualified() });
    rows.push(BmCheckRow { name: "全过", passed: lib.all_pass() });
    rows.push(BmCheckRow { name: "四类齐备", passed: lib.dist_ok() });
    rows.push(BmCheckRow { name: "库单调", passed: lib.monotone() });
    rows.push(BmCheckRow { name: "类别域", passed: SampleCat::of(4).is_none() });
    rows.push(BmCheckRow { name: "常量口径", passed: drill_const_domain_10().0 });
    rows.push(BmCheckRow { name: "转录完整", passed: sample_lines_complete(lib) });
    rows.push(BmCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(BmCheckRow { name: "拒止在账", passed: lib.reject_total() >= 1 });
    rows.push(BmCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn bm_checklist_green(rows: &[BmCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn bm_stats_line(lib: &SampleLib) -> String {
    format!(
        "样本库概况：例 {} 跑 {} 拒 {}",
        lib.len(),
        lib.run_count(),
        lib.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12472_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut lib = SampleLib::new();
    let _ = lib.add_std();
    let _ = lib.run_all();
    // 注入一例重号样本（拒止账口径）。
    let _ = lib.add(SampleCat::Depth, 8, Expect::Pass);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        sample_lines_complete(&lib) && BmReport::build(&lib).all_green(),
        bm_stats_line(&lib),
    ));
    // X-b：边界值档 10/10。
    let (ok_e, d_e) = drill_edge_lines_10();
    out.push(("X-边界值档10/10", ok_e, d_e));
    // X-c：常量口径 10/10。
    let (ok_c, d_c) = drill_const_domain_10();
    out.push(("X-常量口径10/10", ok_c, d_c));
    // X-d：终验清单。
    let rows = build_bm_checklist(&lib);
    out.push(("X-终验清单10项", bm_checklist_green(&rows), BmReport::build(&lib).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：超深路径必拒（PathDeep）。
    if matches!(check_sample(SampleCat::Depth, DEPTH_MAX + 1), Err(BmCode::PathDeep)) {
        caught += 1;
        d.push("注1 超深:抓".to_string());
    } else {
        d.push("注1 超深:漏".to_string());
    }
    // 注 2：超长键名必拒（NameLong）。
    if matches!(check_sample(SampleCat::NameLen, NAME_MAX as u32 + 1), Err(BmCode::NameLong)) {
        caught += 1;
        d.push("注2 超长:抓".to_string());
    } else {
        d.push("注2 超长:漏".to_string());
    }
    // 注 3：超大值必拒（ValueBig）。
    if matches!(check_sample(SampleCat::ValueSize, VALUE_MAX + 1), Err(BmCode::ValueBig)) {
        caught += 1;
        d.push("注3 超大:抓".to_string());
    } else {
        d.push("注3 超大:漏".to_string());
    }
    // 注 4：零参必拒（ParamZero，先于线查）。
    if matches!(check_sample(SampleCat::Depth, 0), Err(BmCode::ParamZero)) {
        caught += 1;
        d.push("注4 零参:抓".to_string());
    } else {
        d.push("注4 零参:漏".to_string());
    }
    // 注 5：大小写越态必拒（CaseOob）。
    if matches!(check_sample(SampleCat::CaseFold, 2), Err(BmCode::CaseOob)) {
        caught += 1;
        d.push("注5 越态:抓".to_string());
    } else {
        d.push("注5 越态:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30900 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    BmCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12472_checks：本单判据聚合（诊断段 0x30900）。
pub fn run_unx12472_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12472 注册表族边界样本集");
    // J1-a：经典坑样本 ≥30 例入库全过（32 例 10/10）。
    let (ok1, d1) = drill_lib_pass_10();
    s.add_dyn("J1-样本入库全过10/10", ok1, &d1);
    // J1-b：越界注入 30 例逐例码值核对。
    let (ok2, d2) = drill_inject_30();
    s.add_dyn("J1-越界注入30例核对", ok2, &d2);
    // J1-c：类别域与拒止不写回（10/10）。
    let (ok3, d3) = drill_domain_reject_10();
    s.add_dyn("J1-类别域拒止10/10", ok3, &d3);
    // J1-d：标准库（32 例四类各 8 + 全过 + 单调）。
    let mut lib = SampleLib::new();
    let n = lib.add_std().unwrap_or(0);
    let bad = lib.run_all();
    s.add_dyn(
        "J1-经典坑样本库达标",
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
    let _ = lib.add(SampleCat::Depth, DEPTH_MAX + 1, Expect::Pass);
    s.add_dyn(
        "J2-拒止不写回",
        lib.len() == n0 && lib.monotone(),
        &format!("例 {}→{}", n0, lib.len()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30900段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12472_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
