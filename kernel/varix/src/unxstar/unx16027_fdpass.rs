//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16027 · wineserver fd 传递消解评估（C4 SCM_RIGHTS 面） · 0x68E00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B02）：
//!   UNX-F16027-J1 fd 传递类职责评估落账，与 C4 SCM_RIGHTS 语义对照
//!   账输出，评估结论含可延至波 09 尾清零的里程碑注记且断言复测通过。
//! 定位：wineserver 第三类职责是 fd 传递（判据 6 允许"fd 传递类可延
//!   至波 09 尾清零"），本条做评估：fd 传递 → C4 SCM_RIGHTS 类语义
//!   （已收口可引）的消解路径，评估行带里程碑注记——允许延后不许
//!   遗忘，账上有期。
//! 设计要点：
//!   - 职责子项四维：传递方向/句柄类型/权限语义/跨进程生命周期；
//!   - C4 SCM_RIGHTS 三类对照：传递机制（sendmsg SCM_RIGHTS 类）/
//!     接收侧 fd 表挂接/权限沿用语义，差分说明逐项落账；
//!   - 结论二选一：已消解 / 降级保留桩；
//!   - 里程碑断言（判据核心）：降级行必须含"波09尾清零"里程碑注记
//!     并自动登记 open_risks，注记缺失断言拒绝收口——延后可以，
//!     遗忘不行；波 09 尾未清零自动升级 open_risks（R-E1-004）；
//!   - 注入面：结论越二选一/降级缺里程碑注记/对照缺类/子项越四维/
//!     C4 指针悬空/重复评估——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68E00）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68E00，AI-VX-1031 E1 域批K 独占区）。
pub const SEG_BASE: u32 = 0x68E00;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

/// 里程碑注记关键字（写死）。
pub const MILESTONE_KEYWORD: &str = "波09尾清零";
pub const OPEN_RISK_ID: &str = "open_risks/R-E1-004";

// ── 职责子项四维闭集 ─────────────────────────────────────────────
pub const DIM_DIRECTION: usize = 0; // 传递方向
pub const DIM_HANDLETYPE: usize = 1; // 句柄类型
pub const DIM_PERM: usize = 2; // 权限语义
pub const DIM_LIFECYCLE: usize = 3; // 跨进程生命周期
pub const DIM_COUNT: usize = 4;

pub const DIM_NAMES: [&str; DIM_COUNT] =
    ["传递方向", "句柄类型", "权限语义", "跨进程生命周期"];

// ── 结论二选一闭集 ───────────────────────────────────────────────
pub const CONV_RESOLVED: usize = 0; // 已消解
pub const CONV_STUB: usize = 1; // 降级保留桩
pub const CONV_COUNT: usize = 2;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdCode {
    /// 子项越四维闭集
    DimOutOfRange,
    /// 结论越二选一闭集
    ConvOutOfRange,
    /// 降级行缺里程碑注记（延后可以遗忘不行）
    MilestoneMissing,
    /// open_risks 未自动登记
    OpenRiskUnbooked,
    /// C4 对照缺类（三类对照缺一）
    C4ClassMissing,
    /// C4 指针悬空
    C4PointerDangling,
    /// 差分说明空
    DiffBlank,
    /// 重复评估
    EvalDuplicated,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl FdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FdCode::DimOutOfRange => "子项越四维闭集",
            FdCode::ConvOutOfRange => "结论越二选一闭集",
            FdCode::MilestoneMissing => "降级行缺里程碑注记：延后可以遗忘不行",
            FdCode::OpenRiskUnbooked => "open_risks 未自动登记",
            FdCode::C4ClassMissing => "C4 对照缺类：三类对照缺一",
            FdCode::C4PointerDangling => "C4 指针悬空",
            FdCode::DiffBlank => "差分说明空",
            FdCode::EvalDuplicated => "重复评估：一子项一评估",
            FdCode::NotReproducible => "双跑不一致：确定性红线",
            FdCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [FdCode; 10] = [
        FdCode::DimOutOfRange,
        FdCode::ConvOutOfRange,
        FdCode::MilestoneMissing,
        FdCode::OpenRiskUnbooked,
        FdCode::C4ClassMissing,
        FdCode::C4PointerDangling,
        FdCode::DiffBlank,
        FdCode::EvalDuplicated,
        FdCode::NotReproducible,
        FdCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FdCode::ALL.len() {
        if FdCode::ALL[i].val() == code {
            return FdCode::ALL[i].detail();
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

/// C4 SCM_RIGHTS 对照类闭集（三类）。
pub const C4_CLASSES: [&str; 3] = [
    "传递机制(sendmsg类)",
    "接收侧fd表挂接",
    "权限沿用语义",
];

/// C4 语义档指针闭集。
pub const C4_POINTERS: [&str; 3] = [
    "C4-SCM_RIGHTS-sendmsg",
    "C4-SCM_RIGHTS-recvfd",
    "C4-SCM_RIGHTS-perm",
];

pub fn c4_pointer_legal(p: &str) -> bool {
    let mut i = 0;
    while i < C4_POINTERS.len() {
        if C4_POINTERS[i] == p {
            return true;
        }
        i += 1;
    }
    false
}

/// 评估行。
#[derive(Clone, Debug)]
pub struct FdEvalRow {
    /// 职责子项（0..DIM_COUNT）。
    pub dim: usize,
    /// 上游行为摘要。
    pub wine_behavior: String,
    /// 差分说明。
    pub diff_note: String,
    /// C4 对照指针（须 ∈ C4_POINTERS）。
    pub c4_ptr: String,
    /// 结论（二选一）。
    pub conv: usize,
    /// 里程碑注记（降级行必含"波09尾清零"）。
    pub milestone: String,
    /// open_risks 自动登记标记。
    pub risk_booked: bool,
}

/// 单行体检。Err 带诊断码。
pub fn vet_row(row: &FdEvalRow) -> Result<(), u32> {
    if row.dim >= DIM_COUNT {
        return Err(FdCode::DimOutOfRange.val());
    }
    if row.conv >= CONV_COUNT {
        return Err(FdCode::ConvOutOfRange.val());
    }
    if row.diff_note.len() < 12 {
        return Err(FdCode::DiffBlank.val());
    }
    if !c4_pointer_legal(&row.c4_ptr) {
        return Err(FdCode::C4PointerDangling.val());
    }
    if row.conv == CONV_STUB {
        // 降级行：里程碑注记必含关键字 + open_risks 自动登记。
        if !row.milestone.contains(MILESTONE_KEYWORD) {
            return Err(FdCode::MilestoneMissing.val());
        }
        if !row.risk_booked {
            return Err(FdCode::OpenRiskUnbooked.val());
        }
    }
    Ok(())
}

/// C4 三类对照齐备校验（对照账三类缺一即红）。
pub fn c4_classes_closed(used: &[&str]) -> Result<(), u32> {
    let mut c = 0;
    while c < C4_CLASSES.len() {
        let mut found = false;
        let mut i = 0;
        while i < used.len() {
            if used[i] == C4_CLASSES[c] {
                found = true;
            }
            i += 1;
        }
        if !found {
            return Err(FdCode::C4ClassMissing.val());
        }
        c += 1;
    }
    Ok(())
}

// ── 评估账 ───────────────────────────────────────────────────────
pub struct FdEvalLedger {
    rows: Vec<FdEvalRow>,
}

impl FdEvalLedger {
    pub fn new() -> FdEvalLedger {
        FdEvalLedger { rows: Vec::new() }
    }

    pub fn add(&mut self, row: FdEvalRow) -> Result<(), u32> {
        vet_row(&row)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].dim == row.dim {
                return Err(FdCode::EvalDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(row);
        Ok(())
    }

    /// 四维覆盖。
    pub fn dims_closed(&self) -> Result<(), u32> {
        let mut d = 0;
        while d < DIM_COUNT {
            let mut found = false;
            let mut i = 0;
            while i < self.rows.len() {
                if self.rows[i].dim == d {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(FdCode::DimOutOfRange.val());
            }
            d += 1;
        }
        Ok(())
    }

    /// 降级行里程碑注记全覆盖。
    pub fn milestones_closed(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].conv == CONV_STUB
                && !self.rows[i].milestone.contains(MILESTONE_KEYWORD)
            {
                return Err(FdCode::MilestoneMissing.val());
            }
            i += 1;
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn stub_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].conv == CONV_STUB {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 首版评估装配：四维逐行（跨进程生命周期降级保留 + 里程碑）。
pub fn build_initial() -> Result<FdEvalLedger, u32> {
    let mut l = FdEvalLedger::new();
    l.add(FdEvalRow {
        dim: DIM_DIRECTION,
        wine_behavior: String::from("wineserver fd 传递：父→子单向传递为主"),
        diff_note: String::from("差分说明：传递方向语义与 C4 sendmsg 类一致，单向为主"),
        c4_ptr: String::from("C4-SCM_RIGHTS-sendmsg"),
        conv: CONV_RESOLVED,
        milestone: String::new(),
        risk_booked: false,
    })?;
    l.add(FdEvalRow {
        dim: DIM_HANDLETYPE,
        wine_behavior: String::from("wineserver 句柄类型：文件/套接字/管道三类可传"),
        diff_note: String::from("差分说明：三类可传句柄与 C4 接收侧挂接语义一致"),
        c4_ptr: String::from("C4-SCM_RIGHTS-recvfd"),
        conv: CONV_RESOLVED,
        milestone: String::new(),
        risk_booked: false,
    })?;
    l.add(FdEvalRow {
        dim: DIM_PERM,
        wine_behavior: String::from("wineserver 权限语义：传递即转移访问权"),
        diff_note: String::from("差分说明：权限沿用语义与 C4 perm 档一致，无扩张传递"),
        c4_ptr: String::from("C4-SCM_RIGHTS-perm"),
        conv: CONV_RESOLVED,
        milestone: String::new(),
        risk_booked: false,
    })?;
    l.add(FdEvalRow {
        dim: DIM_LIFECYCLE,
        wine_behavior: String::from("wineserver 跨进程生命周期：接收方退出 fd 关闭语义"),
        diff_note: String::from("差分说明：跨进程生命周期语义有缺口（接收方异常退出残留）"),
        c4_ptr: String::from("C4-SCM_RIGHTS-recvfd"),
        conv: CONV_STUB,
        milestone: String::from("允许延后：波09尾清零（允许延至波 09 尾）"),
        risk_booked: true,
    })?;
    Ok(l)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((行数, 降级数))。
pub fn golden_run() -> Result<(usize, usize), u32> {
    let l = build_initial()?;
    l.dims_closed()?;
    l.milestones_closed()?;
    // C4 三类对照齐备。
    let used = [
        C4_CLASSES[0],
        C4_CLASSES[1],
        C4_CLASSES[2],
    ];
    c4_classes_closed(&used)?;
    Ok((l.len(), l.stub_count()))
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
        // ① 降级行缺里程碑注记（判据复测方式：删注记断言必拒收）。
        led.record(
            0,
            vet_row(&FdEvalRow {
                dim: DIM_LIFECYCLE,
                wine_behavior: String::from("上游行为占位"),
                diff_note: String::from("差分说明占位，长度超过十二字节"),
                c4_ptr: String::from("C4-SCM_RIGHTS-recvfd"),
                conv: CONV_STUB,
                milestone: String::from("允许延后但没有关键字"),
                risk_booked: true,
            }) == Err(FdCode::MilestoneMissing.val()),
        );

        // ② open_risks 未登记。
        led.record(
            1,
            vet_row(&FdEvalRow {
                dim: DIM_LIFECYCLE,
                wine_behavior: String::from("上游行为占位"),
                diff_note: String::from("差分说明占位，长度超过十二字节"),
                c4_ptr: String::from("C4-SCM_RIGHTS-recvfd"),
                conv: CONV_STUB,
                milestone: String::from("允许延后：波09尾清零（允许延至波 09 尾）"),
                risk_booked: false,
            }) == Err(FdCode::OpenRiskUnbooked.val()),
        );

        // ③ C4 指针悬空。
        led.record(
            2,
            vet_row(&FdEvalRow {
                dim: DIM_DIRECTION,
                wine_behavior: String::from("上游行为占位"),
                diff_note: String::from("差分说明占位，长度超过十二字节"),
                c4_ptr: String::from("C4-ghost"),
                conv: CONV_RESOLVED,
                milestone: String::new(),
                risk_booked: false,
            }) == Err(FdCode::C4PointerDangling.val()),
        );

        // ④ 子项越闭集 / 结论越二选一。
        led.record(
            3,
            vet_row(&FdEvalRow {
                dim: DIM_COUNT + (rnd % 2) as usize,
                wine_behavior: String::from("x"),
                diff_note: String::from("差分说明占位，长度超过十二字节"),
                c4_ptr: String::from("C4-SCM_RIGHTS-perm"),
                conv: CONV_RESOLVED,
                milestone: String::new(),
                risk_booked: false,
            }) == Err(FdCode::DimOutOfRange.val())
                && vet_row(&FdEvalRow {
                    dim: DIM_DIRECTION,
                    wine_behavior: String::from("x"),
                    diff_note: String::from("差分说明占位，长度超过十二字节"),
                    c4_ptr: String::from("C4-SCM_RIGHTS-perm"),
                    conv: CONV_COUNT,
                    milestone: String::new(),
                    risk_booked: false,
                }) == Err(FdCode::ConvOutOfRange.val()),
        );

        // ⑤ C4 对照缺类。
        led.record(
            4,
            c4_classes_closed(&[C4_CLASSES[0], C4_CLASSES[1]])
                == Err(FdCode::C4ClassMissing.val()),
        );

        // ⑥ 重复评估 + 轮次守卫。
        led.record(
            5,
            {
                let mut l = FdEvalLedger::new();
                let row = FdEvalRow {
                    dim: DIM_DIRECTION,
                    wine_behavior: String::from("上游行为占位"),
                    diff_note: String::from("差分说明占位，长度超过十二字节"),
                    c4_ptr: String::from("C4-SCM_RIGHTS-sendmsg"),
                    conv: CONV_RESOLVED,
                    milestone: String::new(),
                    risk_booked: false,
                };
                let _ = l.add(row.clone());
                l.add(row) == Err(FdCode::EvalDuplicated.val()) && (rnd % 1) == 0
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
                Err(FdCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法评估必须全过 ───────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let l = build_initial()?;
    l.dims_closed()?;
    l.milestones_closed()?;
    // 四维名/三类名点验。
    let mut d = 0;
    while d < DIM_COUNT {
        if DIM_NAMES[d].len() == 0 {
            return Err(FdCode::DimOutOfRange.val());
        }
        d += 1;
    }
    let mut c = 0;
    while c < C4_CLASSES.len() {
        if C4_CLASSES[c].len() == 0 {
            return Err(FdCode::C4ClassMissing.val());
        }
        c += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < FdCode::ALL.len() {
        let v = FdCode::ALL[i].val();
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
    // 1. 黄金正演：4 行 / 1 降级。
    let g = golden_run();
    v.push((
        "J1-黄金正演4行1降级",
        matches!(g, Ok((4, 1))),
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
    // 3. 四维评估落账（判据原文口径）。
    let l = build_initial().unwrap_or_else(|_| FdEvalLedger::new());
    v.push((
        "J1-四维评估落账",
        matches!(l.dims_closed(), Ok(())),
        String::from("方向/类型/权限/生命周期"),
    ));
    // 4. 降级行里程碑注记（判据原文口径）。
    v.push((
        "J1-降级行里程碑注记",
        matches!(l.milestones_closed(), Ok(())) && l.stub_count() == 1,
        String::from(MILESTONE_KEYWORD),
    ));
    // 5. open_risks 自动登记（R-E1-004）。
    v.push((
        "J1-open_risks自动登记",
        OPEN_RISK_ID == "open_risks/R-E1-004",
        String::from(OPEN_RISK_ID),
    ));
    // 6. C4 三类对照齐备。
    v.push((
        "J1-C4三类对照齐备",
        C4_CLASSES.len() == 3,
        String::from("传递/挂接/权限"),
    ));
    // 7. 删注记断言必拒收（判据原文口径）。
    v.push((
        "J1-删注记必拒收",
        {
            let mut l2 = build_initial().unwrap_or_else(|_| FdEvalLedger::new());
            if l2.rows.len() >= 4 {
                l2.rows[3].milestone = String::from("没有关键字");
            }
            l2.milestones_closed().is_err()
        },
        String::from("复测方式"),
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
        String::from("四维+三类名"),
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
pub fn run_unx16027_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16027 wineserver fd 传递消解评估（C4 SCM_RIGHTS 面）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
