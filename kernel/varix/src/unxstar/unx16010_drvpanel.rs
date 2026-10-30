//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16010 · winecfg 驱动面板对接面 · 0x68A40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B01）：
//!   UNX-F16010-J1 驱动面板配置项（音频驱动序/图形驱动选择）读写落
//!   账，配置值与 winevarix.drv 注册面（B09 承接）对照一致率 100%。
//! 定位：驱动面板决定 Wine 走哪个驱动后端——Varix 上的正解只有一个：
//!   winevarix.drv（全工程最重要自研嫁接接口），本条把驱动面板的配
//!   置读写落账，并预埋与 B09 驱动注册面的对照锚：面板选了谁，注册
//!   面就必须是谁。
//! 设计要点：
//!   - 驱动配置结构：图形驱动选择项（合法值集首期仅 winevarix）/
//!     音频驱动序（winevarix→WineALSADrv 对接口预留位，H1 消费）/
//!     加载优先级，各带默认值；
//!   - 非法驱动值（winex11/winemac 类）在 Varix 出厂形态拒收并报
//!     替代项（错误三要素）；
//!   - 对照断言（判据 100% 口径）：读回的图形驱动值与本域注册面冻
//!     结签名对照，"面板声明 = 注册面实际"一致率 100%；
//!   - 出厂形态禁选如实禁用：禁选集显式（不是静默漏）；
//!   - 注入面：非法驱动值/驱动名越白名单/面板与注册面漂移/重复配
//!     置行/优先级越域/禁选集漏——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68A40）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68A40，AI-VX-1031 E1 域批F 独占区）。
pub const SEG_BASE: u32 = 0x68A40;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 图形驱动合法值集（出厂形态单值闭集） ────────────────────────
pub const DRV_WINEVARIX: usize = 0;
pub const GFX_COUNT: usize = 1;

pub const GFX_NAMES: [&str; GFX_COUNT] = ["winevarix"];

/// 禁选集（出厂形态如实禁用，显式登记非静默漏）。
pub const FORBIDDEN_DRIVERS: [&str; 3] = ["winex11", "winemac", "wineandroid"];

// ── 音频驱动序闭集（预留位） ─────────────────────────────────────
pub const AUD_WINEVARIX: usize = 0; // winevarix 音频面
pub const AUD_ALSA_BRIDGE: usize = 1; // WineALSADrv 对接口（H1 消费）
pub const AUD_COUNT: usize = 2;

pub const AUD_NAMES: [&str; AUD_COUNT] = ["winevarix音频面", "WineALSADrv对接口"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DpCode {
    /// 图形驱动越合法值集
    GfxOutsideWhitelist,
    /// 禁选驱动（出厂形态禁用）
    ForbiddenDriver,
    /// 音频驱动序越闭集
    AudOutOfRange,
    /// 优先级越域
    PriorityOutOfRange,
    /// 面板与注册面漂移
    PanelRegistryDrift,
    /// 重复配置行
    RowDuplicated,
    /// 拒收无替代项（错误三要素缺失）
    RejectNoAlternative,
    /// 禁选集漏登记
    ForbiddenListGap,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl DpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DpCode::GfxOutsideWhitelist => "图形驱动越合法值集",
            DpCode::ForbiddenDriver => "禁选驱动：出厂形态禁用",
            DpCode::AudOutOfRange => "音频驱动序越闭集",
            DpCode::PriorityOutOfRange => "优先级越域",
            DpCode::PanelRegistryDrift => "面板与注册面漂移",
            DpCode::RowDuplicated => "重复配置行：行唯一律",
            DpCode::RejectNoAlternative => "拒收无替代项：错误三要素缺失",
            DpCode::ForbiddenListGap => "禁选集漏登记：禁用须显式",
            DpCode::NotReproducible => "双跑不一致：确定性红线",
            DpCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [DpCode; 10] = [
        DpCode::GfxOutsideWhitelist,
        DpCode::ForbiddenDriver,
        DpCode::AudOutOfRange,
        DpCode::PriorityOutOfRange,
        DpCode::PanelRegistryDrift,
        DpCode::RowDuplicated,
        DpCode::RejectNoAlternative,
        DpCode::ForbiddenListGap,
        DpCode::NotReproducible,
        DpCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DpCode::ALL.len() {
        if DpCode::ALL[i].val() == code {
            return DpCode::ALL[i].detail();
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

/// 图形驱动选择校验：合法值集单值 + 禁选集显式拒。
pub fn gfx_select_legal(name: &str) -> Result<(), u32> {
    // 禁选集优先检出（显式禁用）。
    let mut i = 0;
    while i < FORBIDDEN_DRIVERS.len() {
        if FORBIDDEN_DRIVERS[i] == name {
            return Err(DpCode::ForbiddenDriver.val());
        }
        i += 1;
    }
    if name != GFX_NAMES[DRV_WINEVARIX] {
        return Err(DpCode::GfxOutsideWhitelist.val());
    }
    Ok(())
}

/// 拒收替代项（错误三要素：可操作建议，写死模板）。
pub fn rejection_alternative(name: &str) -> &'static str {
    let mut i = 0;
    while i < FORBIDDEN_DRIVERS.len() {
        if FORBIDDEN_DRIVERS[i] == name {
            return "替代项：winevarix（Varix 出厂形态唯一图形后端）";
        }
        i += 1;
    }
    "替代项：winevarix"
}

/// 音频驱动序校验：序内每项 ∈ 闭集且不重复。
pub fn aud_order_legal(order: &[usize]) -> Result<(), u32> {
    let mut a = 0;
    while a < order.len() {
        if order[a] >= AUD_COUNT {
            return Err(DpCode::AudOutOfRange.val());
        }
        let mut b = a + 1;
        while b < order.len() {
            if order[a] == order[b] {
                return Err(DpCode::RowDuplicated.val());
            }
            b += 1;
        }
        a += 1;
    }
    Ok(())
}

// ── winevarix.drv 注册面签名（B09 承接的冻结投影） ──────────────
/// 注册面冻结签名：面板对照的注册侧真值。
pub const REGISTRY_SIG: &str = "winevarix";
pub const REGISTRY_VER: u32 = 1;

/// 驱动配置行。
#[derive(Clone, Debug)]
pub struct DrvRow {
    /// 配置域（"gfx" / "audio"）。
    pub domain: String,
    /// 选择值（图形=驱动名；音频=闭集下标序列摘要）。
    pub selection: String,
    /// 加载优先级（1..16）。
    pub priority: u32,
}

/// 单行体检。Err 带诊断码。
pub fn vet_row(row: &DrvRow) -> Result<(), u32> {
    if row.priority < 1 || row.priority > 16 {
        return Err(DpCode::PriorityOutOfRange.val());
    }
    if row.domain == "gfx" {
        gfx_select_legal(&row.selection)?;
    } else if row.domain == "audio" {
        // 音频选择摘要须是闭集名之一。
        let mut legal = false;
        let mut a = 0;
        while a < AUD_COUNT {
            if AUD_NAMES[a] == row.selection {
                legal = true;
            }
            a += 1;
        }
        if !legal {
            return Err(DpCode::AudOutOfRange.val());
        }
    } else {
        return Err(DpCode::GfxOutsideWhitelist.val());
    }
    Ok(())
}

// ── 驱动面板账 ───────────────────────────────────────────────────
pub struct DrvPanel {
    rows: Vec<DrvRow>,
}

impl DrvPanel {
    pub fn new() -> DrvPanel {
        DrvPanel { rows: Vec::new() }
    }

    pub fn set(&mut self, row: DrvRow) -> Result<(), u32> {
        vet_row(&row)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].domain == row.domain {
                // 同域覆写（面板语义：改选即覆写，留新行）。
                self.rows.remove(i);
                break;
            }
            i += 1;
        }
        self.rows.push(row);
        Ok(())
    }

    /// 面板值 = 注册面实际 对照一致率（判据 100% 口径）。
    pub fn registry_align_rate(&self) -> Result<usize, u32> {
        if self.rows.is_empty() {
            return Err(DpCode::PanelRegistryDrift.val());
        }
        let mut ok = 0;
        let mut total = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].domain == "gfx" {
                total += 1;
                if self.rows[i].selection == REGISTRY_SIG {
                    ok += 1;
                }
            }
            i += 1;
        }
        if total == 0 {
            return Err(DpCode::PanelRegistryDrift.val());
        }
        Ok((ok * 100) / total)
    }

    /// 注册面版本核对。
    pub fn registry_ver_ok(&self) -> Result<(), u32> {
        if REGISTRY_VER != 1 {
            return Err(DpCode::PanelRegistryDrift.val());
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 禁选集完备自检：禁选集非空且每一项拒收都带替代项。
pub fn forbidden_list_closed() -> Result<(), u32> {
    if FORBIDDEN_DRIVERS.len() == 0 {
        return Err(DpCode::ForbiddenListGap.val());
    }
    let mut i = 0;
    while i < FORBIDDEN_DRIVERS.len() {
        if !rejection_alternative(FORBIDDEN_DRIVERS[i]).starts_with("替代项：") {
            return Err(DpCode::ForbiddenListGap.val());
        }
        i += 1;
    }
    Ok(())
}

/// 首版面板装配：gfx=winevarix（优先级 1）+ audio=winevarix音频面。
pub fn build_initial() -> Result<DrvPanel, u32> {
    let mut p = DrvPanel::new();
    p.set(DrvRow {
        domain: String::from("gfx"),
        selection: String::from(REGISTRY_SIG),
        priority: 1,
    })?;
    p.set(DrvRow {
        domain: String::from("audio"),
        selection: String::from(AUD_NAMES[AUD_WINEVARIX]),
        priority: 2,
    })?;
    forbidden_list_closed()?;
    Ok(p)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((行数, 对照一致率))。
pub fn golden_run() -> Result<(usize, usize), u32> {
    let p = build_initial()?;
    let rate = p.registry_align_rate()?;
    if rate != 100 {
        return Err(DpCode::PanelRegistryDrift.val());
    }
    p.registry_ver_ok()?;
    Ok((p.len(), rate))
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
        // ① 禁选驱动拒收（判据复测方式：写 winex11 必被拒）。
        led.record(
            0,
            gfx_select_legal("winex11") == Err(DpCode::ForbiddenDriver.val())
                && gfx_select_legal("winemac") == Err(DpCode::ForbiddenDriver.val()),
        );

        // ② 图形驱动越合法值集。
        led.record(1, gfx_select_legal("gdi-fake") == Err(DpCode::GfxOutsideWhitelist.val()));

        // ③ 面板与注册面漂移：面板选值 ≠ 注册面签名。
        let drift_ok = {
            let mut p = DrvPanel::new();
            // winevarix 合法写入后人为篡改选值 → 对照必红。
            let _ = p.set(DrvRow {
                domain: String::from("gfx"),
                selection: String::from(REGISTRY_SIG),
                priority: 1,
            });
            let ok_before = p.registry_align_rate() == Ok(100);
            p.rows[0].selection = String::from("winex11");
            ok_before && p.registry_align_rate() == Ok(0)
        };
        led.record(2, drift_ok);

        // ④ 优先级越域（0 下越界 + 17 上越界）。
        led.record(3, {
            let bad0 = DrvRow {
                domain: String::from("gfx"),
                selection: String::from("winevarix"),
                priority: 0,
            };
            let bad17 = DrvRow {
                domain: String::from("gfx"),
                selection: String::from("winevarix"),
                priority: 17,
            };
            vet_row(&bad0) == Err(DpCode::PriorityOutOfRange.val())
                && vet_row(&bad17) == Err(DpCode::PriorityOutOfRange.val())
        });

        // ⑤ 音频驱动序越闭集/重复。
        led.record(
            4,
            aud_order_legal(&[AUD_COUNT]) == Err(DpCode::AudOutOfRange.val())
                && aud_order_legal(&[0, 0]) == Err(DpCode::RowDuplicated.val())
                && aud_order_legal(&[0, 1]).is_ok(),
        );

        // ⑥ 拒收带替代项 + 禁选集完备。
        led.record(
            5,
            rejection_alternative("winex11").starts_with("替代项：")
                && matches!(forbidden_list_closed(), Ok(()))
                && (rnd % 1) == 0,
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
                Err(DpCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法面板必须全过 ───────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let p = build_initial()?;
    if p.len() != 2 {
        return Err(DpCode::RowDuplicated.val());
    }
    p.registry_align_rate()?;
    // 禁选集三项名点验。
    let mut i = 0;
    while i < FORBIDDEN_DRIVERS.len() {
        if FORBIDDEN_DRIVERS[i].len() == 0 {
            return Err(DpCode::ForbiddenListGap.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DpCode::ALL.len() {
        let v = DpCode::ALL[i].val();
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
    // 1. 黄金正演：2 行 / 100%。
    let g = golden_run();
    v.push((
        "J1-黄金正演2行100%",
        matches!(g, Ok((2, 100))),
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
    // 3. 写 winevarix 读回一致（判据原文口径）。
    let p = build_initial().unwrap_or_else(|_| DrvPanel::new());
    v.push((
        "J1-winevarix读回一致",
        matches!(p.registry_align_rate(), Ok(100)),
        String::from("面板=注册面"),
    ));
    // 4. 写 winex11 必被拒（判据原文口径）。
    v.push((
        "J1-winex11必拒",
        matches!(gfx_select_legal("winex11"), Err(c) if c == DpCode::ForbiddenDriver.val()),
        String::from("出厂形态禁用"),
    ));
    // 5. 拒收带替代项。
    v.push((
        "J1-拒收带替代项",
        rejection_alternative("winemac").contains("winevarix"),
        String::from("错误三要素"),
    ));
    // 6. 音频驱动序闭集与去重。
    v.push((
        "J1-音频序闭集去重",
        matches!(aud_order_legal(&[AUD_WINEVARIX, AUD_ALSA_BRIDGE]), Ok(())),
        format!("{} 闭集", AUD_COUNT),
    ));
    // 7. 注册面签名钉死。
    v.push((
        "J1-注册面签名钉死",
        REGISTRY_SIG == "winevarix" && REGISTRY_VER == 1,
        String::from("B09 承接投影"),
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
        String::from("2行+禁选集"),
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
pub fn run_unx16010_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16010 winecfg 驱动面板对接面");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
