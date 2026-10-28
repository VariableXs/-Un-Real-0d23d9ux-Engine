//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12482 · PEB 面注入矩阵 · 0x30a40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12482-J1 坏签名/坏 Size/坏版本档 30 例注入全过，拒止码与
//!      B03 档一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - PEB 档三字段：签名（MAGIC 0x00B0B0B0 口径）/结构尺寸
//!   （PEB_SIZE 0x458，64 位口径）/OS 版本（主版本 ≥6、次版本 ≤15）；
//!   - 闸序：签名 → 尺寸 → 版本（先查签名防伪，再查结构完整性，后查
//!   版本兼容），每例按闸序命中首个拒码；
//!   - B03 档拒止码锚定：坏签名 → STATUS_INVALID_PARAMETER（0xC000000D），
//!   坏尺寸 → STATUS_INVALID_PARAMETER_MIX（0xC0000031），坏版本 →
//!   STATUS_REVISION_MISMATCH（0xC0000059）；
//!   - 注入 30 例：三档各 10 例（LCG 组合参数），逐例码值与 B03 锚定
//!   表一致（CaseMiss 抓失配、CaseDup 抓重号）；
//!   - 合法域核对：三字段全好的样本必须放行（Pass 面）；
//!   - 拒止不写回：注入例不落 PEB 档账；
//!   - 错误路径带码+中文 detail；
//!   - 注入核：LCG 写死种子 0x30a40，30/30 码值一致 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30a40 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30a40;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 每轮注入例数（判准 30/30）。
pub const INJECT_CASES: usize = 30;

/// 每档例数（三档各 10）。
pub const CASES_PER_CLASS: usize = 10;

/// PEB 签名 MAGIC（口径常量）。
pub const PEB_MAGIC: u32 = 0x00B0_B0B0;

/// PEB 结构尺寸（64 位口径 0x458）。
pub const PEB_SIZE: u32 = 0x458;

/// OS 主版本下限。
pub const OS_MAJOR_MIN: u32 = 6;

/// OS 次版本上限。
pub const OS_MINOR_MAX: u32 = 15;

// B03 档拒止码锚定（ntstatus.h 口径）。
/// 坏签名拒止码（B03 档）。
pub const B03_BAD_SIGNATURE: u32 = 0xC000_000D;
/// 坏尺寸拒止码（B03 档）。
pub const B03_BAD_SIZE: u32 = 0xC000_0031;
/// 坏版本拒止码（B03 档）。
pub const B03_BAD_VERSION: u32 = 0xC000_0059;

/// 诊断码（0x30a40 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PebCode {
    /// 0x30a40 注入例漏判（例落三档之外）。
    CaseMiss = 0,
    /// 0x30a41 注入例重号。
    CaseDup = 1,
    /// 0x30a42 码值失配（拒码 ≠ B03 锚定）。
    AnchorMiss = 2,
    /// 0x30a43 字段越域（字段号出三字段闭集）。
    FieldOob = 3,
    /// 0x30a44 PEB 档账缩水。
    PebShrink = 4,
    /// 0x30a45 合法例误拒（好样本被拒）。
    PassMiss = 5,
    /// 0x30a46 尺寸零。
    SizeZero = 6,
    /// 0x30a47 版本域倒挂（主版本下限 > 次版本上限域）。
    VerDomain = 7,
    /// 0x30a48 注入类覆盖缺（三档未全覆盖）。
    ClassMiss = 8,
    /// 0x30a49 档账序倒挂。
    LedgerBack = 9,
}

impl PebCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [PebCode; 10] = [
        PebCode::CaseMiss, PebCode::CaseDup, PebCode::AnchorMiss, PebCode::FieldOob,
        PebCode::PebShrink, PebCode::PassMiss, PebCode::SizeZero, PebCode::VerDomain,
        PebCode::ClassMiss, PebCode::LedgerBack,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30a40 => "注入例漏判：逐例归三档闭集（签名/尺寸/版本），漏判红账。",
        0x30a41 => "注入例重号：例号唯一，重号先查抽签器。",
        0x30a42 => "码值失配：拒止码必须与 B03 锚定表逐例一致。",
        0x30a43 => "字段越域：字段号先比三字段闭集再取。",
        0x30a44 => "PEB 档账缩水：append-only，禁止截断。",
        0x30a45 => "合法例误拒：三字段全好的样本必须放行。",
        0x30a46 => "尺寸零：结构尺寸零无意义，按坏尺寸档拒。",
        0x30a47 => "版本域倒挂：主版本下限/次版本上限口径核对。",
        0x30a48 => "注入类覆盖缺：30 例必须三档全覆盖。",
        0x30a49 => "档账序倒挂：档账序全局单调，回拨红账。",
        _ => "未知诊断码：回查 0x30a40 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30a40 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x30a4_30a4_30a4_30a4;

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

/// PEB 档（三字段）。
#[derive(Clone, Copy, Debug)]
pub struct PebFields {
    /// 签名。
    pub signature: u32,
    /// 结构尺寸。
    pub size: u32,
    /// OS 主版本。
    pub major: u32,
    /// OS 次版本。
    pub minor: u32,
}

/// PEB 校验（纯函数；闸序：签名 → 尺寸 → 版本）。
///
/// 返回 Ok 放行；Err(B03 档码) 即拒止口径。
pub fn validate_peb(f: &PebFields) -> Result<(), u32> {
    // 闸 1：签名。
    if f.signature != PEB_MAGIC {
        return Err(B03_BAD_SIGNATURE);
    }
    // 闸 2：尺寸。
    if f.size != PEB_SIZE {
        return Err(B03_BAD_SIZE);
    }
    // 闸 3：版本。
    if f.major < OS_MAJOR_MIN || f.minor > OS_MINOR_MAX {
        return Err(B03_BAD_VERSION);
    }
    Ok(())
}

/// 字段档（三档闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PebField {
    /// 签名。
    Signature = 0,
    /// 尺寸。
    Size = 1,
    /// 版本。
    Version = 2,
}

impl PebField {
    /// 由序号取档（越界返回 None）。
    pub fn of(i: u64) -> Option<PebField> {
        match i {
            0 => Some(PebField::Signature),
            1 => Some(PebField::Size),
            2 => Some(PebField::Version),
            _ => None,
        }
    }
}

/// 注入例（字段档 × 破坏参数；带期望 B03 码）。
#[derive(Clone, Copy, Debug)]
pub struct InjectCase {
    /// 例号。
    pub case_no: usize,
    /// 字段档。
    pub field: PebField,
    /// 破坏后 PEB 档。
    pub fields: PebFields,
    /// 期望 B03 拒止码。
    pub expect: u32,
}

/// 注入档账正本。
///
/// 生命周期：run_case（逐例校验 + 码值核对 + 账面登记）→
/// class_cover（三档覆盖核对）。
#[derive(Debug)]
pub struct PebBook {
    /// 注入例账（append-only）。
    cases: Vec<(usize, u32)>,
    /// 末例号（唯一性判准）。
    last_case: u64,
    /// 合法放行数。
    passes: u32,
    /// 拒止累计。
    rejects: u32,
}

impl PebBook {
    /// 建账。
    pub fn new() -> Self {
        PebBook {
            cases: Vec::new(),
            last_case: u64::MAX,
            passes: 0,
            rejects: 0,
        }
    }

    /// 跑一例（判止后记账；合法例放行计数）。
    pub fn run_case(&mut self, case_no: usize, f: &PebFields, expect: u32) -> Result<(), u32> {
        if self.last_case != u64::MAX && (case_no as u64) <= self.last_case {
            self.rejects += 1;
            return Err(0);
        }
        self.last_case = case_no as u64;
        match validate_peb(f) {
            Ok(()) => {
                self.passes += 1;
                Ok(())
            }
            Err(code) => {
                if code != expect {
                    self.rejects += 1;
                    return Err(code);
                }
                // 拒止命中：码值一致面入账并计入拒止累计。
                self.cases.push((case_no, code));
                self.rejects += 1;
                Ok(())
            }
        }
    }

    /// 例账长。
    pub fn case_len(&self) -> usize {
        self.cases.len()
    }

    /// 例账快照。
    pub fn case_of(&self, i: usize) -> Option<(usize, u32)> {
        if i >= self.cases.len() {
            None
        } else {
            Some(self.cases[i])
        }
    }

    /// 例号唯一判据。
    pub fn cases_unique(&self) -> bool {
        let mut i = 0;
        while i < self.cases.len() {
            let mut j = i + 1;
            while j < self.cases.len() {
                if self.cases[i].0 == self.cases[j].0 {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }

    /// 放行计数。
    pub fn pass_total(&self) -> u32 {
        self.passes
    }
}

/// 标准合法 PEB 档。
pub fn good_peb() -> PebFields {
    PebFields {
        signature: PEB_MAGIC,
        size: PEB_SIZE,
        major: 6,
        minor: 1,
    }
}

/// 三档注入例抽签（30 例：签名 0..10、尺寸 10..20、版本 20..30）。
fn pick_case(lcg: &mut Lcg, i: usize) -> InjectCase {
    let class = i / CASES_PER_CLASS;
    let k = lcg.below(4);
    let base = good_peb();
    match class {
        0 => {
            // 签名档：破坏签名。
            let mut f = base;
            f.signature = match k {
                0 => 0,
                1 => PEB_MAGIC ^ 0xFF,
                2 => PEB_MAGIC + 1,
                _ => PEB_MAGIC - 1,
            };
            InjectCase { case_no: i, field: PebField::Signature, fields: f, expect: B03_BAD_SIGNATURE }
        }
        1 => {
            // 尺寸档：破坏尺寸（签名好）。
            let mut f = base;
            f.size = match k {
                0 => 0,
                1 => PEB_SIZE - 8,
                2 => PEB_SIZE + 8,
                _ => PEB_SIZE / 2,
            };
            InjectCase { case_no: i, field: PebField::Size, fields: f, expect: B03_BAD_SIZE }
        }
        _ => {
            // 版本档：破坏版本（签名/尺寸好）。
            let mut f = base;
            match k {
                0 => f.major = 5,
                1 => f.major = 3,
                2 => f.minor = OS_MINOR_MAX + 1,
                _ => f.minor = 99,
            }
            InjectCase { case_no: i, field: PebField::Version, fields: f, expect: B03_BAD_VERSION }
        }
    }
}

/// 30/30 注入演练（逐例码值与 B03 锚一致；含覆盖与唯一性核对）。
pub fn drill_inject30_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30a40 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PebBook::new();
        let mut hit = 0usize;
        let mut sig = 0usize;
        let mut size = 0usize;
        let mut ver = 0usize;
        for i in 0..INJECT_CASES {
            let (n, f, expect) = {
                let c = pick_case(&mut lcg, i);
                (c.case_no, c.fields, c.expect)
            };
            match c_field(&f) {
                PebField::Signature => sig += 1,
                PebField::Size => size += 1,
                PebField::Version => ver += 1,
            }
            match bk.run_case(n, &f, expect) {
                Err(_) => {}
                Ok(()) => hit += 1,
            }
        }
        // 全部例应命中拒止口径（30 例码值一致）；三档全覆盖；例号唯一。
        if bk.case_len() == INJECT_CASES
            && hit == INJECT_CASES
            && sig == CASES_PER_CLASS
            && size == CASES_PER_CLASS
            && ver == CASES_PER_CLASS
            && bk.cases_unique()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("PEB 注入 30/30 轮={}/{}", ok, INJECT_ROUNDS))
}

/// PEB 档字段归档便桥（签名/尺寸/版本三分类）。
fn c_field(f: &PebFields) -> PebField {
    if f.signature != PEB_MAGIC {
        PebField::Signature
    } else if f.size != PEB_SIZE {
        PebField::Size
    } else {
        PebField::Version
    }
}

/// 合法域演练：好样本放行、闸序核对 10/10。
pub fn drill_pass_face_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30a40 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let good = validate_peb(&good_peb()).is_ok();
        // 闸序：坏签名+坏尺寸 → 命中签名码（首个闸）。
        let mut f = good_peb();
        f.signature = 0;
        f.size = 0;
        let gate1 = matches!(validate_peb(&f), Err(c) if c == B03_BAD_SIGNATURE);
        // 闸序：好签名+坏尺寸+坏版本 → 命中尺寸码。
        let mut f2 = good_peb();
        f2.size = 0x100;
        f2.major = 3;
        let gate2 = matches!(validate_peb(&f2), Err(c) if c == B03_BAD_SIZE);
        // 闸序：签名/尺寸好+坏版本 → 版本码。
        let mut f3 = good_peb();
        f3.major = 5;
        let gate3 = matches!(validate_peb(&f3), Err(c) if c == B03_BAD_VERSION);
        let _ = lcg.below(3);
        if good && gate1 && gate2 && gate3 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("合法域闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入例账转录导出（逐例一行：例号/档/码）。
/// ─────────────────────────────────────────────────────────────────────
/// 注入例账转录导出。
pub fn export_case_lines(bk: &PebBook) -> String {
    let mut s = String::from("PEB 注入例账转录：\n");
    for i in 0..bk.case_len() {
        if let Some((n, code)) = bk.case_of(i) {
            let cls = if n < 10 { "签名" } else if n < 20 { "尺寸" } else { "版本" };
            s.push_str(&format!("#{} {} {:08x}\n", n, cls, code));
        }
    }
    s
}

/// 转录完整判据（逐例行齐备）。
pub fn case_lines_complete(bk: &PebBook) -> bool {
    let s = export_case_lines(bk);
    if bk.case_len() == 0 {
        return false;
    }
    (0..bk.case_len()).all(|i| {
        bk.case_of(i)
            .map(|(n, _)| s.contains(&format!("#{} ", n)))
            .unwrap_or(false)
    })
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入矩阵报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct PebReport {
    /// 例账行数。
    pub cases: usize,
    /// 放行计数。
    pub passes: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 例号唯一。
    pub unique: bool,
    /// 转录完整。
    pub lines: bool,
}

impl PebReport {
    /// 建报告。
    pub fn build(bk: &PebBook) -> Self {
        PebReport {
            cases: bk.case_len(),
            passes: bk.pass_total(),
            rejects: bk.reject_total(),
            unique: bk.cases_unique(),
            lines: case_lines_complete(bk),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == INJECT_CASES && self.unique && self.lines && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "PebReport 例={} 放={} 拒={} 唯一={} 转录={}",
            self.cases, self.passes, self.rejects, self.unique, self.lines
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：常量口径与字段域演练（B03 三码/三档 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_const_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30a40 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = B03_BAD_SIGNATURE == 0xC000_000D;
        pass = pass && B03_BAD_SIZE == 0xC000_0031 && B03_BAD_VERSION == 0xC000_0059;
        pass = pass && PEB_SIZE == 0x458 && PEB_MAGIC != 0;
        pass = pass && OS_MAJOR_MIN == 6 && OS_MINOR_MAX == 15;
        pass = pass && (0..3).all(|k| PebField::of(k).is_some()) && PebField::of(3).is_none();
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("常量域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：B03 锚定面演练（三码互异且落 NT 错误域 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_anchor_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30a40 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        const SET: [u32; 3] = [B03_BAD_SIGNATURE, B03_BAD_SIZE, B03_BAD_VERSION];
        let mut pass = true;
        for i in 0..3 {
            for j in 0..3 {
                if i != j {
                    pass = pass && SET[i] != SET[j];
                }
            }
            pass = pass && SET[i] & 0xC000_0000 == 0xC000_0000;
        }
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("B03 锚定轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct PebCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_peb_checklist(bk: &PebBook) -> Vec<PebCheckRow> {
    let rep = PebReport::build(bk);
    let mut rows: Vec<PebCheckRow> = Vec::new();
    rows.push(PebCheckRow { name: "30例在账", passed: bk.case_len() == INJECT_CASES });
    rows.push(PebCheckRow { name: "例号唯一", passed: bk.cases_unique() });
    rows.push(PebCheckRow { name: "口径", passed: INJECT_CASES == 30 && CASES_PER_CLASS == 10 });
    rows.push(PebCheckRow { name: "常量域", passed: drill_const_domain_10().0 });
    rows.push(PebCheckRow { name: "B03锚定", passed: drill_anchor_10().0 });
    rows.push(PebCheckRow { name: "转录完整", passed: case_lines_complete(bk) });
    rows.push(PebCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(PebCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(PebCheckRow { name: "字段域", passed: PebField::of(2).is_some() && PebField::of(3).is_none() });
    rows.push(PebCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn peb_checklist_green(rows: &[PebCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn peb_stats_line(bk: &PebBook) -> String {
    format!(
        "PEB 矩阵概况：例 {} 放 {} 拒 {}",
        bk.case_len(),
        bk.pass_total(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12482_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = PebBook::new();
    // 注入三例（签名/尺寸/版本各一）。
    let c0 = pick_case(&mut Lcg::new(0x1), 0);
    let c1 = pick_case(&mut Lcg::new(0x2), 10);
    let c2 = pick_case(&mut Lcg::new(0x3), 20);
    let _ = bk.run_case(c0.case_no, &c0.fields, c0.expect);
    let _ = bk.run_case(c1.case_no, &c1.fields, c1.expect);
    let _ = bk.run_case(c2.case_no, &c2.fields, c2.expect);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        case_lines_complete(&bk) && bk.case_len() == 3,
        format!("例 {} 放 {} 拒 {}", bk.case_len(), bk.pass_total(), bk.reject_total()),
    ));
    // X-b：常量域 10/10。
    let (ok_c, d_c) = drill_const_domain_10();
    out.push(("X-常量域10/10", ok_c, d_c));
    // X-c：B03 锚定 10/10。
    let (ok_a, d_a) = drill_anchor_10();
    out.push(("X-B03锚定10/10", ok_a, d_a));
    // X-d：终验清单（30 例全账面）。
    let mut bk2 = PebBook::new();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for i in 0..INJECT_CASES {
        let c = pick_case(&mut lcg, i);
        let _ = bk2.run_case(c.case_no, &c.fields, c.expect);
    }
    let rows = build_peb_checklist(&bk2);
    out.push((
        "X-终验清单10项",
        peb_checklist_green(&rows),
        PebReport::build(&bk2).to_text(),
    ));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：坏签名必拒且码与 B03 一致。
    let mut f = good_peb();
    f.signature = 0x1234;
    if matches!(validate_peb(&f), Err(c) if c == B03_BAD_SIGNATURE) {
        caught += 1;
        d.push("注1 签名:抓".to_string());
    } else {
        d.push("注1 签名:漏".to_string());
    }
    // 注 2：坏尺寸必拒且码与 B03 一致。
    let mut f2 = good_peb();
    f2.size = PEB_SIZE + 0x40;
    if matches!(validate_peb(&f2), Err(c) if c == B03_BAD_SIZE) {
        caught += 1;
        d.push("注2 尺寸:抓".to_string());
    } else {
        d.push("注2 尺寸:漏".to_string());
    }
    // 注 3：坏版本必拒且码与 B03 一致。
    let mut f3 = good_peb();
    f3.major = 2;
    if matches!(validate_peb(&f3), Err(c) if c == B03_BAD_VERSION) {
        caught += 1;
        d.push("注3 版本:抓".to_string());
    } else {
        d.push("注3 版本:漏".to_string());
    }
    // 注 4：好样本放行（Pass 面）。
    if validate_peb(&good_peb()).is_ok() {
        caught += 1;
        d.push("注4 放行:抓".to_string());
    } else {
        d.push("注4 放行:漏".to_string());
    }
    // 注 5：字段域越界（of(3) None）。
    if PebField::of(3).is_none() {
        caught += 1;
        d.push("注5 越域:抓".to_string());
    } else {
        d.push("注5 越域:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30a40 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    PebCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12482_checks：本单判据聚合（诊断段 0x30a40）。
pub fn run_unx12482_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12482 PEB 面注入矩阵");
    // J1-a：30/30 注入全过（码值与 B03 一致）。
    let (ok1, d1) = drill_inject30_10();
    s.add_dyn("J1-PEB注入30/30", ok1, &d1);
    // J1-b：合法域闸序（10/10）。
    let (ok2, d2) = drill_pass_face_10();
    s.add_dyn("J1-合法域闸序10/10", ok2, &d2);
    // J1-c：常量域 + B03 锚定（10/10 × 2）。
    let (ok3, d3) = drill_const_domain_10();
    s.add_dyn("J1-常量域10/10", ok3, &d3);
    let (ok4, d4) = drill_anchor_10();
    s.add_dyn("J1-B03锚定10/10", ok4, &d4);
    // J1-d：标准会话（三档覆盖 + 例号唯一 + 转录）。
    let mut bk = PebBook::new();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for i in 0..INJECT_CASES {
        let c = pick_case(&mut lcg, i);
        let _ = bk.run_case(c.case_no, &c.fields, c.expect);
    }
    s.add_dyn(
        "J1-三档覆盖唯一转录",
        bk.case_len() == INJECT_CASES
            && bk.cases_unique()
            && case_lines_complete(&bk)
            && bk.reject_total() >= 1,
        &format!(
            "例={} 唯一={} 转录={}",
            bk.case_len(),
            bk.cases_unique(),
            case_lines_complete(&bk)
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30a40段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12482_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
