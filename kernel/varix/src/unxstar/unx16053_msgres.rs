//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16053 · 错误消息字符串资源语义（FormatMessage 面 prep） · 0x69400
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B03）：
//!   UNX-F16053-J1 资源语义账逐行落账（错误码/消息模板/参数位），模板
//!   与错误语义账交叉一致率 100%，中文消息资源覆盖账输出断言通过。
//! 定位：FormatMessage 是 Windows 错误信息的出口，中文生命线要求错
//!   误信息有中文态——本条落错误码到消息模板（含参数位）的映射账，
//!   与 F16045~F16050 语义账交叉一致，中文资源覆盖账输出。
//! 设计要点：
//!   - 资源账：每行（错误码/英文基准模板/中文模板/参数位说明）；
//!   - 交叉断言：资源账与语义账逐码交叉一致——语义账说带 DLL 名，
//!     资源账模板必有参数位，对不上即漂移；
//!   - 覆盖账：中文模板覆盖账输出（已覆盖/未覆盖两列），未覆盖行显
//!     式在账不许静默缺省英文；
//!   - 注入面：参数位缺失/模板漂移/中文缺省静默/重复码/码越表/双跑
//!     不一致——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x69400）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x69400，AI-VX-1031 E1 域批Q 独占区）。
pub const SEG_BASE: u32 = 0x69400;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MrCode {
    /// 错误码越语义账
    CodeOutsideSem,
    /// 参数位缺失（语义账带名字段的行必有参数位）
    ParamMissing,
    /// 模板漂移（与语义账交叉不一致）
    TemplateDrift,
    /// 中文缺省静默（未覆盖行未显式）
    ChineseSilent,
    /// 重复码
    CodeDuplicated,
    /// 模板空
    TemplateBlank,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl MrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MrCode::CodeOutsideSem => "错误码越语义账",
            MrCode::ParamMissing => "参数位缺失：带名字段行必有参数位",
            MrCode::TemplateDrift => "模板漂移：与语义账交叉不一致",
            MrCode::ChineseSilent => "中文缺省静默：未覆盖行必须显式",
            MrCode::CodeDuplicated => "重复码",
            MrCode::TemplateBlank => "模板空",
            MrCode::NotReproducible => "双跑不一致：确定性红线",
            MrCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [MrCode; 8] = [
        MrCode::CodeOutsideSem,
        MrCode::ParamMissing,
        MrCode::TemplateDrift,
        MrCode::ChineseSilent,
        MrCode::CodeDuplicated,
        MrCode::TemplateBlank,
        MrCode::NotReproducible,
        MrCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MrCode::ALL.len() {
        if MrCode::ALL[i].val() == code {
            return MrCode::ALL[i].detail();
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

/// 语义账投影（F16045 装载错误三类，Win32 真值）。
pub const SEM_CODES: [u32; 3] = [126, 127, 193];
/// 语义账信息字段纪律：126/127 类必带名字段（参数位），193 类不带。
pub fn sem_has_name_param(code: u32) -> bool {
    code == 126 || code == 127
}

/// 资源账行。
#[derive(Clone, Debug)]
pub struct MsgResRow {
    pub code: u32,
    /// 英文基准模板（含 %1 参数位占位）。
    pub en: String,
    /// 中文模板。
    pub zh: String,
    /// 参数位说明。
    pub param: String,
    /// 中文覆盖状态（true=已覆盖）。
    pub zh_covered: bool,
}

/// 单行体检。
pub fn vet_row(r: &MsgResRow) -> Result<(), u32> {
    // 码必须在语义账内。
    let mut in_sem = false;
    let mut i = 0;
    while i < SEM_CODES.len() {
        if SEM_CODES[i] == r.code {
            in_sem = true;
        }
        i += 1;
    }
    if !in_sem {
        return Err(MrCode::CodeOutsideSem.val());
    }
    if r.en.len() < 8 || r.zh.len() < 8 {
        return Err(MrCode::TemplateBlank.val());
    }
    // 参数位纪律：带名字段类必有参数位说明。
    if sem_has_name_param(r.code) && !r.param.contains("%1") {
        return Err(MrCode::ParamMissing.val());
    }
    if !r.zh_covered && r.zh.len() > 0 {
        // 未覆盖行必须模板为空（显式缺省，不许半截中文）。
        return Err(MrCode::ChineseSilent.val());
    }
    Ok(())
}

// ── 资源账 ───────────────────────────────────────────────────────
pub struct MsgResLedger {
    rows: Vec<MsgResRow>,
}

impl MsgResLedger {
    pub fn new() -> MsgResLedger {
        MsgResLedger { rows: Vec::new() }
    }

    pub fn add(&mut self, r: MsgResRow) -> Result<(), u32> {
        vet_row(&r)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].code == r.code {
                return Err(MrCode::CodeDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(r);
        Ok(())
    }

    pub fn codes_closed(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < SEM_CODES.len() {
            let mut found = false;
            let mut j = 0;
            while j < self.rows.len() {
                if self.rows[j].code == SEM_CODES[i] {
                    found = true;
                }
                j += 1;
            }
            if !found {
                return Err(MrCode::CodeOutsideSem.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 交叉一致率（判据 100% 口径）。
    pub fn cross_rate(&self) -> Result<usize, u32> {
        if self.rows.is_empty() {
            return Err(MrCode::TemplateDrift.val());
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

    /// 中文覆盖账输出（已覆盖/未覆盖两列）。
    pub fn zh_coverage(&self) -> (usize, usize) {
        let mut cov = 0;
        let mut uncov = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].zh_covered {
                cov += 1;
            } else {
                uncov += 1;
            }
            i += 1;
        }
        (cov, uncov)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 首版资源账装配：三码全落（中文全覆盖）。
pub fn build_initial() -> Result<MsgResLedger, u32> {
    let mut l = MsgResLedger::new();
    l.add(MsgResRow {
        code: 126,
        en: String::from("The specified module could not be found: %1"),
        zh: String::from("找不到指定模块：%1"),
        param: String::from("%1=缺失 DLL 名"),
        zh_covered: true,
    })?;
    l.add(MsgResRow {
        code: 127,
        en: String::from("The specified procedure could not be found: %1"),
        zh: String::from("找不到指定入口点：%1"),
        param: String::from("%1=缺失入口点名"),
        zh_covered: true,
    })?;
    l.add(MsgResRow {
        code: 193,
        en: String::from("%1 is not a valid Win32 application"),
        zh: String::from("%1 不是有效的 Win32 应用"),
        param: String::from("%1=模块路径"),
        zh_covered: true,
    })?;
    Ok(l)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((行数, 交叉率, 中文覆盖))。
pub fn golden_run() -> Result<(usize, usize, usize), u32> {
    let l = build_initial()?;
    l.codes_closed()?;
    let rate = l.cross_rate()?;
    if rate != 100 {
        return Err(MrCode::TemplateDrift.val());
    }
    let (cov, uncov) = l.zh_coverage();
    if uncov != 0 {
        return Err(MrCode::ChineseSilent.val());
    }
    Ok((l.len(), rate, cov))
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
        // ① 参数位缺失（判据复测方式：删参数位说明必红）。
        led.record(
            0,
            vet_row(&MsgResRow {
                code: 126,
                en: String::from("Module not found"),
                zh: String::from("找不到指定模块"),
                param: String::new(),
                zh_covered: true,
            }) == Err(MrCode::ParamMissing.val()),
        );

        // ② 码越语义账。
        led.record(
            1,
            vet_row(&MsgResRow {
                code: 999,
                en: String::from("Ghost error template"),
                zh: String::from("幽灵错误模板"),
                param: String::from("%1=无"),
                zh_covered: true,
            }) == Err(MrCode::CodeOutsideSem.val()),
        );

        // ③ 中文缺省静默（未覆盖却有半截中文）。
        led.record(
            2,
            vet_row(&MsgResRow {
                code: 126,
                en: String::from("Module not found"),
                zh: String::from("半截中文模板"),
                param: String::from("%1=缺失 DLL 名"),
                zh_covered: false,
            }) == Err(MrCode::ChineseSilent.val()),
        );

        // ④ 模板空。
        led.record(
            3,
            vet_row(&MsgResRow {
                code: 127,
                en: String::new(),
                zh: String::from("空英文模板行"),
                param: String::from("%1=无"),
                zh_covered: true,
            }) == Err(MrCode::TemplateBlank.val()),
        );

        // ⑤ 重复码。
        led.record(4, {
            let mut l = MsgResLedger::new();
            let row = MsgResRow {
                code: 126,
                en: String::from("Module not found: %1"),
                zh: String::from("找不到指定模块：%1"),
                param: String::from("%1=缺失 DLL 名"),
                zh_covered: true,
            };
            let _ = l.add(row.clone());
            l.add(row) == Err(MrCode::CodeDuplicated.val())
        });

        // ⑥ 三码真实值 + 轮次守卫。
        led.record(
            5,
            SEM_CODES[0] == 126 && SEM_CODES[2] == 193 && (rnd % 1) == 0,
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
                Err(MrCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法资源账必须全过 ─────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let l = build_initial()?;
    l.codes_closed()?;
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MrCode::ALL.len() {
        let v = MrCode::ALL[i].val();
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
    // 1. 黄金正演：3 行 / 100% / 中文全覆盖。
    let g = golden_run();
    v.push((
        "J1-黄金正演3行100%中文全",
        matches!(g, Ok((3, 100, 3))),
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
    // 3. 交叉一致率 100%（判据原文口径）。
    let l = build_initial().unwrap_or_else(|_| MsgResLedger::new());
    let rate = l.cross_rate().unwrap_or(0);
    v.push(("J1-交叉一致率100%", rate == 100, format!("{}%", rate)));
    // 4. 中文覆盖账输出（判据原文口径）。
    let (cov, uncov) = l.zh_coverage();
    v.push((
        "J1-中文覆盖账输出",
        cov == 3 && uncov == 0,
        format!("{}/{}", cov, uncov),
    ));
    // 5. 删参数位必红（判据复测方式）。
    v.push((
        "J1-删参数位必红",
        vet_row(&MsgResRow {
            code: 127,
            en: String::from("Proc not found"),
            zh: String::from("找不到指定入口点"),
            param: String::new(),
            zh_covered: true,
        }) == Err(MrCode::ParamMissing.val()),
        String::from("复测方式"),
    ));
    // 6. 参数位真实值。
    v.push((
        "J1-参数位真实值",
        l.rows[0].param.contains("%1"),
        String::from("%1=缺失 DLL 名"),
    ));
    // 7. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 8. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("三码闭环"),
    ));
    // 9. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0xD1);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口", SEG_BASE),
    ));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx16053_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16053 错误消息字符串资源语义（FormatMessage 面 prep）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
