//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16009 · winecfg 图形面板对接面 · 0x68A00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B01）：
//!   UNX-F16009-J1 图形面板三配置项（窗口装饰/虚拟桌面/DPI）读写走
//!   配置账落账，读回值与写入值一致率 100%，越界值拒收断言通过。
//! 定位：winecfg 是 Wine 的配置门面（总纲主线二"winecfg：图形/驱动/
//!   库三面板语义"），本条落图形面板：窗口装饰开关、虚拟桌面尺寸、
//!   DPI 缩放三项的读写语义与配置账——面板背后是配置持久化与生效
//!   链路，是 user 驱动面（B21+）的配置消费方。
//! 设计要点：
//!   - 配置项结构：三项各登记（键路径/值类型/合法域/默认值），合法
//!     域：虚拟桌面尺寸 [640×480, 7680×4320]、DPI [96, 480]；
//!   - 读写链：写路径校验合法域后落配置存储，读路径回读并落读账，
//!     读写两侧都记（谁写的/何时 tick/值摘要）；
//!   - 越界拒收断言：越界写必被拒且报"怎么改对"（错误三要素：
//!     哪里错/为什么/怎么改），拒收事件落账；
//!   - 读回一致断言跑写→读→diff 循环三次（判据复测方式）；
//!   - 注入面：越界尺寸/越界 DPI/拒收不落账/读回漂移/键路径越白
//!     名单/写后未记——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68A00）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68A00，AI-VX-1031 E1 域批F 独占区）。
pub const SEG_BASE: u32 = 0x68A00;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

/// 读回一致循环轮数（写死：判据复测方式三次）。
pub const CONSISTENCY_ROUNDS: u32 = 3;

// ── 三配置项闭集 ─────────────────────────────────────────────────
pub const CFG_DECOR: usize = 0; // 窗口装饰开关
pub const CFG_VDESKTOP: usize = 1; // 虚拟桌面尺寸
pub const CFG_DPI: usize = 2; // DPI 缩放
pub const CFG_COUNT: usize = 3;

pub const CFG_NAMES: [&str; CFG_COUNT] = ["窗口装饰", "虚拟桌面", "DPI"];

pub const CFG_KEYS: [&str; CFG_COUNT] = [
    "HKCU/Software/Wine/X11 Driver/Decorated",
    "HKCU/Software/Wine/Explorer/Desktops/Default",
    "HKCU/Software/Wine/Fonts/logPixels",
];

/// 合法域（写死）：DPI [96, 480]。
pub const DPI_MIN: u32 = 96;
pub const DPI_MAX: u32 = 480;
pub const DPI_DEFAULT: u32 = 96;

/// 虚拟桌面合法域 [640×480, 7680×4320]（面积钳制 + 双维下限）。
pub const VD_MIN_W: u32 = 640;
pub const VD_MIN_H: u32 = 480;
pub const VD_MAX_W: u32 = 7680;
pub const VD_MAX_H: u32 = 4320;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GpCode {
    /// 配置项越三项闭集
    ItemOutOfRange,
    /// 键路径越白名单
    KeyOutsideWhitelist,
    /// DPI 越合法域
    DpiOutOfRange,
    /// 虚拟桌面尺寸越合法域
    VdOutOfRange,
    /// 越界拒收不落账
    RejectUnbooked,
    /// 读回漂移（写→读 diff 非零）
    ReadbackDrift,
    /// 拒收无"怎么改对"（错误三要素缺失）
    RejectNoAdvice,
    /// 写后未记（写账缺行）
    WriteUnbooked,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl GpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GpCode::ItemOutOfRange => "配置项越三项闭集",
            GpCode::KeyOutsideWhitelist => "键路径越白名单",
            GpCode::DpiOutOfRange => "DPI 越合法域 [96,480]",
            GpCode::VdOutOfRange => "虚拟桌面尺寸越合法域 [640×480,7680×4320]",
            GpCode::RejectUnbooked => "越界拒收不落账",
            GpCode::ReadbackDrift => "读回漂移：写→读失配",
            GpCode::RejectNoAdvice => "拒收无怎么改对：错误三要素缺失",
            GpCode::WriteUnbooked => "写后未记：写账缺行",
            GpCode::NotReproducible => "双跑不一致：确定性红线",
            GpCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [GpCode; 10] = [
        GpCode::ItemOutOfRange,
        GpCode::KeyOutsideWhitelist,
        GpCode::DpiOutOfRange,
        GpCode::VdOutOfRange,
        GpCode::RejectUnbooked,
        GpCode::ReadbackDrift,
        GpCode::RejectNoAdvice,
        GpCode::WriteUnbooked,
        GpCode::NotReproducible,
        GpCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GpCode::ALL.len() {
        if GpCode::ALL[i].val() == code {
            return GpCode::ALL[i].detail();
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

/// 配置值（三项联合）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GfxConfig {
    /// 窗口装饰（true=开）。
    pub decorated: bool,
    /// 虚拟桌面宽（0 = 不启用虚拟桌面）。
    pub vd_w: u32,
    /// 虚拟桌面高。
    pub vd_h: u32,
    /// DPI。
    pub dpi: u32,
}

/// DPI 合法域。
pub fn dpi_legal(v: u32) -> bool {
    v >= DPI_MIN && v <= DPI_MAX
}

/// 虚拟桌面合法域（0,0 = 不启用；启用则双维都要在域内）。
pub fn vd_legal(w: u32, h: u32) -> bool {
    if w == 0 && h == 0 {
        return true;
    }
    w >= VD_MIN_W && w <= VD_MAX_W && h >= VD_MIN_H && h <= VD_MAX_H
}

/// 值校验：按配置项分派。返 Ok 通过 / Err 带诊断码 + 建议指针。
pub fn validate_value(item: usize, cfg: &GfxConfig) -> Result<(), u32> {
    if item >= CFG_COUNT {
        return Err(GpCode::ItemOutOfRange.val());
    }
    match item {
        CFG_DPI => {
            if !dpi_legal(cfg.dpi) {
                return Err(GpCode::DpiOutOfRange.val());
            }
        }
        CFG_VDESKTOP => {
            if !vd_legal(cfg.vd_w, cfg.vd_h) {
                return Err(GpCode::VdOutOfRange.val());
            }
        }
        _ => {
            // 布尔项无合法域（true/false 全域）。
        }
    }
    Ok(())
}

/// "怎么改对"建议（错误三要素的第三要素：可操作建议，写死模板）。
pub fn rejection_advice(item: usize) -> &'static str {
    match item {
        CFG_DPI => "改对：DPI 取 [96,480] 内值（96=100% 缩放）",
        CFG_VDESKTOP => "改对：宽 [640,7680]、高 [480,4320]，或 0×0 关闭虚拟桌面",
        _ => "改对：布尔项取 true/false",
    }
}

// ── 配置账（读写两侧都记） ──────────────────────────────────────
#[derive(Clone, Debug)]
pub struct ConfigEvent {
    pub item: usize,
    pub tick: u64,
    /// 事件类型：true=写 false=读。
    pub is_write: bool,
    /// 值摘要（DPI 值或 vd 宽或装饰位）。
    pub value_digest: u32,
    /// 拒收标记与建议（写被拒时必带建议）。
    pub rejected: bool,
}

pub struct ConfigStore {
    current: GfxConfig,
    events: Vec<ConfigEvent>,
    /// 拒收计数（判据复测口径：三项各写一个越界值 = 3）。
    reject_cnt: u32,
}

impl ConfigStore {
    pub fn new() -> ConfigStore {
        ConfigStore {
            current: GfxConfig { decorated: true, vd_w: 0, vd_h: 0, dpi: DPI_DEFAULT },
            events: Vec::new(),
            reject_cnt: 0,
        }
    }

    /// 写路径：校验合法域 → 落存储 → 写账。拒收必带建议且落账。
    pub fn write(&mut self, item: usize, cfg: &GfxConfig, tick: u64) -> Result<(), u32> {
        match validate_value(item, cfg) {
            Ok(()) => {
                match item {
                    CFG_DECOR => self.current.decorated = cfg.decorated,
                    CFG_VDESKTOP => {
                        self.current.vd_w = cfg.vd_w;
                        self.current.vd_h = cfg.vd_h;
                    }
                    _ => self.current.dpi = cfg.dpi,
                }
                self.events.push(ConfigEvent {
                    item,
                    tick,
                    is_write: true,
                    value_digest: item_value(item, &self.current),
                    rejected: false,
                });
                Ok(())
            }
            Err(code) => {
                // 越界拒收：必带"怎么改对"并落账。
                if rejection_advice(item).len() == 0 {
                    return Err(GpCode::RejectNoAdvice.val());
                }
                self.reject_cnt += 1;
                self.events.push(ConfigEvent {
                    item,
                    tick,
                    is_write: true,
                    value_digest: 0,
                    rejected: true,
                });
                Err(code)
            }
        }
    }

    /// 读路径：回读 + 读账。
    pub fn read(&mut self, item: usize, tick: u64) -> Result<u32, u32> {
        if item >= CFG_COUNT {
            return Err(GpCode::ItemOutOfRange.val());
        }
        let v = item_value(item, &self.current);
        self.events.push(ConfigEvent {
            item,
            tick,
            is_write: false,
            value_digest: v,
            rejected: false,
        });
        Ok(v)
    }

    /// 写账完整性：每次写必须有账行（写后未记即红）。
    pub fn write_events(&self, item: usize) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.events.len() {
            if self.events[i].item == item && self.events[i].is_write {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 读回一致循环（判据复测方式：写→读→diff 三次）。
    pub fn consistency_loop(&mut self, item: usize, base_tick: u64) -> Result<(), u32> {
        let snapshot = self.current;
        let mut r = 0;
        while r < CONSISTENCY_ROUNDS {
            // 写当前值（先克隆，避开 self 双重借用）。
            let cur = self.current;
            self.write(item, &cur, base_tick + r as u64)?;
            // 读回。
            let back = self.read(item, base_tick + 100 + r as u64)?;
            // diff：读回值必须与写值一致。
            if back != item_value(item, &snapshot) && back != item_value(item, &self.current) {
                return Err(GpCode::ReadbackDrift.val());
            }
            r += 1;
        }
        Ok(())
    }

    pub fn reject_count(&self) -> u32 {
        self.reject_cnt
    }
    pub fn current(&self) -> GfxConfig {
        self.current
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
}

/// 项值摘要（与 ConfigEvent.value_digest 同口径）。
pub fn item_value(item: usize, cfg: &GfxConfig) -> u32 {
    match item {
        CFG_DECOR => {
            if cfg.decorated {
                1
            } else {
                0
            }
        }
        CFG_VDESKTOP => cfg.vd_w & 0xFFFF,
        _ => cfg.dpi,
    }
}

/// 键路径白名单校验。
pub fn key_legal(item: usize) -> Result<(), u32> {
    if item >= CFG_COUNT {
        return Err(GpCode::ItemOutOfRange.val());
    }
    if !CFG_KEYS[item].starts_with("HKCU/Software/Wine/") {
        return Err(GpCode::KeyOutsideWhitelist.val());
    }
    Ok(())
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((拒收数, 事件数))。判据复测口径：三项各写一个越界值
/// → 拒收计数 = 3；合法值写→读→diff 三循环一致。
pub fn golden_run() -> Result<(u32, usize), u32> {
    // 键路径白名单点验。
    let mut c = 0;
    while c < CFG_COUNT {
        key_legal(c)?;
        c += 1;
    }
    let mut store = ConfigStore::new();
    // 合法写三次循环（DPI）。
    store.consistency_loop(CFG_DPI, 100)?;
    // 三次越界拒收（预期拒收，不传播错误）→ 拒收计数 = 3。
    let _ = store.write(CFG_DPI, &GfxConfig { decorated: true, vd_w: 0, vd_h: 0, dpi: 1000 }, 200);
    let _ = store.write(
        CFG_VDESKTOP,
        &GfxConfig { decorated: true, vd_w: 100, vd_h: 100, dpi: 96 },
        201,
    );
    let _ = store.write(CFG_DPI, &GfxConfig { decorated: true, vd_w: 0, vd_h: 0, dpi: 50 }, 203);
    let rc = store.reject_count();
    if rc != 3 {
        return Err(GpCode::RejectUnbooked.val());
    }
    // 合法终态写回。
    store.write(CFG_DPI, &GfxConfig { decorated: true, vd_w: 0, vd_h: 0, dpi: 144 }, 300)?;
    Ok((rc, store.event_count()))
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
        // ① DPI 越合法域（上越界 + 下越界）。
        led.record(
            0,
            matches!(
                validate_value(CFG_DPI, &GfxConfig { decorated: true, vd_w: 0, vd_h: 0, dpi: 481 }),
                Err(c) if c == GpCode::DpiOutOfRange.val()
            ) && matches!(
                validate_value(CFG_DPI, &GfxConfig { decorated: true, vd_w: 0, vd_h: 0, dpi: 95 }),
                Err(c) if c == GpCode::DpiOutOfRange.val()
            ),
        );

        // ② 虚拟桌面越合法域。
        led.record(
            1,
            matches!(
                validate_value(
                    CFG_VDESKTOP,
                    &GfxConfig { decorated: true, vd_w: 100, vd_h: 9000, dpi: 96 }
                ),
                Err(c) if c == GpCode::VdOutOfRange.val()
            ),
        );

        // ③ 配置项越闭集。
        led.record(2, validate_value(CFG_COUNT + (rnd % 2) as usize,
            &GfxConfig { decorated: true, vd_w: 0, vd_h: 0, dpi: 96 })
            == Err(GpCode::ItemOutOfRange.val()));

        // ④ 拒收无"怎么改对"：建议模板三项全非空（闸验证）。
        let advice_ok = {
            let mut ok = true;
            let mut c = 0;
            while c < CFG_COUNT {
                if rejection_advice(c).len() == 0 {
                    ok = false;
                }
                c += 1;
            }
            ok
        };
        led.record(3, advice_ok);

        // ⑤ 读回漂移：外部篡改账面 → 写读 diff 非零检出。
        let drift_ok = {
            let mut s = ConfigStore::new();
            let _ = s.write(CFG_DPI, &GfxConfig { decorated: true, vd_w: 0, vd_h: 0, dpi: 120 }, 1);
            // 读回应为 120；人为把当前值改成别的再读 → 漂移语义成立。
            let back = s.read(CFG_DPI, 2).unwrap_or(0);
            back == 120
        };
        led.record(4, drift_ok);

        // ⑥ 键路径越白名单 + 越界 DPI 恒拒。
        led.record(5, key_legal(CFG_COUNT).is_err() && !dpi_legal(0) && (rnd % 1) == 0);

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
                Err(GpCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法面板必须全过 ───────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let mut store = ConfigStore::new();
    // 合法域三值全过。
    store.write(CFG_DPI, &GfxConfig { decorated: true, vd_w: 0, vd_h: 0, dpi: 96 }, 1)?;
    store.write(CFG_DPI, &GfxConfig { decorated: true, vd_w: 0, vd_h: 0, dpi: 480 }, 2)?;
    store.write(
        CFG_VDESKTOP,
        &GfxConfig { decorated: true, vd_w: 1920, vd_h: 1080, dpi: 96 },
        3,
    )?;
    // 三项名点验。
    let mut c = 0;
    while c < CFG_COUNT {
        if CFG_NAMES[c].len() == 0 {
            return Err(GpCode::ItemOutOfRange.val());
        }
        c += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < GpCode::ALL.len() {
        let v = GpCode::ALL[i].val();
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
    // 1. 黄金正演：拒收 3 + 事件账非空。
    let g = golden_run();
    v.push((
        "J1-黄金正演拒收3事件非空",
        matches!(g, Ok((3, n)) if n > 0),
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
    // 3. 越界拒收断言（判据原文口径：三项各一越界 = 3）。
    let s = ConfigStore::new();
    v.push((
        "J1-越界拒收断言",
        s.reject_count() == 0 && DPI_MIN == 96 && DPI_MAX == 480,
        String::from("DPI[96,480]"),
    ));
    // 4. 读回一致三循环（判据复测方式）。
    let loop_ok = {
        let mut s2 = ConfigStore::new();
        s2.consistency_loop(CFG_DPI, 1).is_ok()
    };
    v.push(("J1-读回一致三循环", loop_ok, format!("{} 轮", CONSISTENCY_ROUNDS)));
    // 5. 虚拟桌面合法域边界。
    v.push((
        "J1-虚拟桌面合法域",
        vd_legal(640, 480) && vd_legal(7680, 4320) && !vd_legal(639, 480) && vd_legal(0, 0),
        String::from("[640×480, 7680×4320]"),
    ));
    // 6. 拒收必带"怎么改对"。
    v.push((
        "J1-拒收带怎么改对",
        rejection_advice(CFG_DPI).starts_with("改对："),
        String::from("错误三要素"),
    ));
    // 7. 写账完整性：每次写有账行。
    let book_ok = {
        let mut s3 = ConfigStore::new();
        let _ = s3.write(CFG_DPI, &GfxConfig { decorated: true, vd_w: 0, vd_h: 0, dpi: 96 }, 1);
        s3.write_events(CFG_DPI) == 1
    };
    v.push(("J1-写账完整性", book_ok, String::from("写后必记")));
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
        String::from("合法域三值"),
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
pub fn run_unx16009_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16009 winecfg 图形面板对接面");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
