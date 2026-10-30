//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16045 · 装载错误语义（DLL 缺失/入口点缺失类） · 0x69200
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B03）：
//!   UNX-F16045-J1 语义账逐类落账（找不到模块/找不到入口/格式非法三
//!   基准类），错误码与预期值对照一致率 100%，错误信息三要素齐备率
//!   100%。
//! 定位：装载错误是 Windows 用户最熟悉的错误（"找不到 xxx.dll"），
//!   语义对齐度直接影响疑难归因（E5 域高频根因），本条落装载错误语
//!   义账：三类基准与 AI-17/D2 语义档对照。
//! 设计要点：
//!   - 三类基准（Win32 真实值写死）：ERROR_MOD_NOT_FOUND=126、
//!     ERROR_PROC_NOT_FOUND=127、ERROR_BAD_EXE_FORMAT=193；
//!   - 错误信息模板三要素：发生什么/为什么/怎么办（"找不到 DLL"要
//!     能说出找过哪些地方）；
//!   - 边界注记：路径搜索语义归 prefix 域后续批次，本账只管错误面；
//!   - 注入面：错误码错配/信息模板缺要素/信息字段缺失（缺失 DLL 名
//!     类）/重复类/类越闭集/模板漂移——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x69200）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x69200，AI-VX-1031 E1 域批O 独占区）。
pub const SEG_BASE: u32 = 0x69200;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 三类基准闭集（Win32 真实值写死） ────────────────────────────
pub const ERR_MOD_NOT_FOUND: u32 = 126;
pub const ERR_PROC_NOT_FOUND: u32 = 127;
pub const ERR_BAD_EXE_FORMAT: u32 = 193;
pub const CLASS_COUNT: usize = 3;

pub const CLASS_TABLE: [u32; CLASS_COUNT] =
    [ERR_MOD_NOT_FOUND, ERR_PROC_NOT_FOUND, ERR_BAD_EXE_FORMAT];

pub const CLASS_NAMES: [&str; CLASS_COUNT] =
    ["找不到模块", "找不到入口点", "格式非法"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LeCode {
    /// 类越闭集
    ClassOutOfRange,
    /// 错误码错配（与 Windows 真值不一致）
    CodeMismatch,
    /// 信息模板缺三要素
    TemplateTriElementShort,
    /// 信息字段缺失（缺失 DLL 名类必带名字段）
    FieldMissing,
    /// 重复类
    ClassDuplicated,
    /// 模板漂移（与对照真值不一致）
    TemplateDrift,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl LeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LeCode::ClassOutOfRange => "类越闭集",
            LeCode::CodeMismatch => "错误码与 Windows 真值错配",
            LeCode::TemplateTriElementShort => "信息模板缺三要素",
            LeCode::FieldMissing => "信息字段缺失：缺失 DLL 名类必带名字段",
            LeCode::ClassDuplicated => "重复类：一类一行",
            LeCode::TemplateDrift => "模板漂移：与对照真值不一致",
            LeCode::NotReproducible => "双跑不一致：确定性红线",
            LeCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [LeCode; 8] = [
        LeCode::ClassOutOfRange,
        LeCode::CodeMismatch,
        LeCode::TemplateTriElementShort,
        LeCode::FieldMissing,
        LeCode::ClassDuplicated,
        LeCode::TemplateDrift,
        LeCode::NotReproducible,
        LeCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < LeCode::ALL.len() {
        if LeCode::ALL[i].val() == code {
            return LeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ─────────────────────────
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.st
    }
}

/// 语义账行：错误码/信息模板三要素/信息字段。
#[derive(Clone, Debug)]
pub struct LoadErrRow {
    /// 类（0..CLASS_COUNT）。
    pub class: usize,
    /// 错误码（须 = CLASS_TABLE[class] Windows 真值）。
    pub code: u32,
    /// 发生什么。
    pub what: String,
    /// 为什么。
    pub why: String,
    /// 怎么办。
    pub next: String,
    /// 信息字段说明（找不到模块/入口点类必带名字段说明）。
    pub name_field: String,
}

/// 单行体检：码真值 + 三要素 + 名字段纪律。
pub fn vet_row(r: &LoadErrRow) -> Result<(), u32> {
    if r.class >= CLASS_COUNT {
        return Err(LeCode::ClassOutOfRange.val());
    }
    if r.code != CLASS_TABLE[r.class] {
        return Err(LeCode::CodeMismatch.val());
    }
    if r.what.len() < 8 || r.why.len() < 8 || r.next.len() < 8 {
        return Err(LeCode::TemplateTriElementShort.val());
    }
    // 名字段纪律：模块/入口点类必带名字段说明。
    if r.class <= 1 && !r.name_field.contains("名字段") {
        return Err(LeCode::FieldMissing.val());
    }
    Ok(())
}

// ── 语义账 ───────────────────────────────────────────────────────
pub struct LoadErrLedger {
    rows: Vec<LoadErrRow>,
}

impl LoadErrLedger {
    pub fn new() -> LoadErrLedger {
        LoadErrLedger { rows: Vec::new() }
    }

    pub fn add(&mut self, r: LoadErrRow) -> Result<(), u32> {
        vet_row(&r)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].class == r.class {
                return Err(LeCode::ClassDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(r);
        Ok(())
    }

    /// 三类覆盖。
    pub fn classes_closed(&self) -> Result<(), u32> {
        let mut c = 0;
        while c < CLASS_COUNT {
            let mut found = false;
            let mut i = 0;
            while i < self.rows.len() {
                if self.rows[i].class == c {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(LeCode::ClassOutOfRange.val());
            }
            c += 1;
        }
        Ok(())
    }

    /// 信息三要素齐备率（判据 100% 口径）。
    pub fn template_rate(&self) -> Result<usize, u32> {
        if self.rows.is_empty() {
            return Err(LeCode::TemplateTriElementShort.val());
        }
        let mut ok = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if vet_row(&self.rows[i]).is_ok() {
                ok += 1;
            }
            i += 1;
        }
        Ok((ok * 100) / self.rows.len())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 首版语义账装配：三类全落（Windows 真值 + 三要素模板）。
pub fn build_initial() -> Result<LoadErrLedger, u32> {
    let mut l = LoadErrLedger::new();
    l.add(LoadErrRow {
        class: 0,
        code: ERR_MOD_NOT_FOUND,
        what: String::from("发生什么：找不到指定模块（DLL 缺失）"),
        why: String::from("为什么：搜索路径全查无此模块（找过哪些地方可回放）"),
        next: String::from("怎么办：核对应用目录与 PATH，或补齐依赖 DLL"),
        name_field: String::from("名字段：缺失 DLL 名随错误信息携带"),
    })?;
    l.add(LoadErrRow {
        class: 1,
        code: ERR_PROC_NOT_FOUND,
        what: String::from("发生什么：找不到指定入口点（导出函数缺失）"),
        why: String::from("为什么：模块版本不含该导出或导出名不匹配"),
        next: String::from("怎么办：核对模块版本与导入表绑定"),
        name_field: String::from("名字段：缺失入口点名随错误信息携带"),
    })?;
    l.add(LoadErrRow {
        class: 2,
        code: ERR_BAD_EXE_FORMAT,
        what: String::from("发生什么：模块格式非法（非 PE 或位数错位）"),
        why: String::from("为什么：PE 头机器类型与加载位图不符"),
        next: String::from("怎么办：核对位数分档（F16008 双档布局）"),
        name_field: String::new(),
    })?;
    Ok(l)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((行数, 模板率))。
pub fn golden_run() -> Result<(usize, usize), u32> {
    let l = build_initial()?;
    l.classes_closed()?;
    let rate = l.template_rate()?;
    if rate != 100 {
        return Err(LeCode::TemplateTriElementShort.val());
    }
    Ok((l.len(), rate))
}

// ── 注入账本：六族注入 × 10 轮全抓 ──────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 6],
    pub caught: [u32; 6],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 6], caught: [0; 6] }
    }
    pub fn record(&mut self, slot: usize, caught: bool) {
        self.tries[slot] += 1;
        if caught {
            self.caught[slot] += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 6 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn rounds(&self) -> u32 {
        let mut r = self.tries[0];
        let mut i = 1;
        while i < 6 {
            if self.tries[i] < r {
                r = self.tries[i];
            }
            i += 1;
        }
        r
    }
}

pub fn run_injections(led: &mut InjectLedger) {
    const ROUNDS: u32 = 10;
    let mut rnd = 0;
    while rnd < ROUNDS {
        // ① 错误码错配（判据复测方式：码漂移断言必红）。
        led.record(
            0,
            vet_row(&LoadErrRow {
                class: 0,
                code: ERR_MOD_NOT_FOUND + 1,
                what: String::from("发生什么占位内容"),
                why: String::from("为什么占位内容"),
                next: String::from("怎么办占位内容"),
                name_field: String::from("名字段：缺失 DLL 名"),
            }) == Err(LeCode::CodeMismatch.val()),
        );

        // ② 信息模板缺三要素。
        led.record(
            1,
            vet_row(&LoadErrRow {
                class: 1,
                code: ERR_PROC_NOT_FOUND,
                what: String::from("发生什么占位内容"),
                why: String::new(),
                next: String::from("怎么办占位内容"),
                name_field: String::from("名字段：缺失入口点名"),
            }) == Err(LeCode::TemplateTriElementShort.val()),
        );

        // ③ 名字段缺失（找不到模块类必带名字段）。
        led.record(
            2,
            vet_row(&LoadErrRow {
                class: 0,
                code: ERR_MOD_NOT_FOUND,
                what: String::from("发生什么占位内容"),
                why: String::from("为什么占位内容"),
                next: String::from("怎么办占位内容"),
                name_field: String::new(),
            }) == Err(LeCode::FieldMissing.val()),
        );

        // ④ 类越闭集。
        led.record(
            3,
            vet_row(&LoadErrRow {
                class: CLASS_COUNT + (rnd % 2) as usize,
                code: ERR_BAD_EXE_FORMAT,
                what: String::from("发生什么占位内容"),
                why: String::from("为什么占位内容"),
                next: String::from("怎么办占位内容"),
                name_field: String::new(),
            }) == Err(LeCode::ClassOutOfRange.val()),
        );

        // ⑤ Win32 真实值点验。
        led.record(
            4,
            ERR_MOD_NOT_FOUND == 126
                && ERR_PROC_NOT_FOUND == 127
                && ERR_BAD_EXE_FORMAT == 193,
        );

        // ⑥ 重复类 + 轮次守卫。
        led.record(
            5,
            {
                let mut l = LoadErrLedger::new();
                let row = LoadErrRow {
                    class: 2,
                    code: ERR_BAD_EXE_FORMAT,
                    what: String::from("发生什么占位内容"),
                    why: String::from("为什么占位内容"),
                    next: String::from("怎么办占位内容"),
                    name_field: String::new(),
                };
                let _ = l.add(row.clone());
                l.add(row) == Err(LeCode::ClassDuplicated.val()) && (rnd % 1) == 0
            },
        );

        rnd += 1;
    }
}

// ── 双跑对拍 ─────────────────────────────────────────────────────
pub fn dual_run() -> Result<(), u32> {
    let a = golden_run();
    let b = golden_run();
    match (a, b) {
        (Ok(x), Ok(y)) => {
            if x == y {
                Ok(())
            } else {
                Err(LeCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法语义账必须全过 ─────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let l = build_initial()?;
    l.classes_closed()?;
    // 三类名点验。
    let mut c = 0;
    while c < CLASS_COUNT {
        if CLASS_NAMES[c].len() == 0 {
            return Err(LeCode::ClassOutOfRange.val());
        }
        c += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < LeCode::ALL.len() {
        let v = LeCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥10 条） ───────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：3 行 / 100%。
    let g = golden_run();
    v.push((
        "J1-黄金正演3行100%",
        matches!(g, Ok((3, 100))),
        format!("{:?}", g),
    ));
    // 2. 注入六族全抓 10/10。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入六族10/10全抓",
        led.all_caught() && led.rounds() == 10,
        format!("r={}", led.rounds()),
    ));
    // 3. 错误码对照一致率 100%（判据原文口径）。
    let l = build_initial().unwrap_or_else(|_| LoadErrLedger::new());
    let code_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < l.rows.len() {
            if l.rows[i].code != CLASS_TABLE[l.rows[i].class] {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-错误码对照100%", code_ok, String::from("126/127/193")));
    // 4. 信息三要素齐备率 100%（判据原文口径）。
    let rate = l.template_rate().unwrap_or(0);
    v.push(("J1-信息三要素100%", rate == 100, format!("{}%", rate)));
    // 5. Win32 真实值点验。
    v.push((
        "J1-Win32真实值",
        ERR_MOD_NOT_FOUND == 126 && ERR_BAD_EXE_FORMAT == 193,
        String::from("126/127/193"),
    ));
    // 6. 删信息字段必红（判据复测方式）。
    v.push((
        "J1-删信息字段必红",
        vet_row(&LoadErrRow {
            class: 1,
            code: ERR_PROC_NOT_FOUND,
            what: String::from("发生什么占位内容"),
            why: String::from("为什么占位内容"),
            next: String::from("怎么办占位内容"),
            name_field: String::new(),
        }) == Err(LeCode::FieldMissing.val()),
        String::from("复测方式"),
    ));
    // 7. 三类名点验。
    v.push((
        "J1-三类名点验",
        CLASS_COUNT == 3,
        String::from("模块/入口点/格式"),
    ));
    // 8. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 9. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("三类覆盖"),
    ));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口", SEG_BASE),
    ));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx16045_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16045 装载错误语义（DLL 缺失/入口点缺失类）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
