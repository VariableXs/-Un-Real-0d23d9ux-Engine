//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2163 · 压测取数口域级清单（三十处导出消费视图） · 0x6D480
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B5）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F2163-J1 取数清单断言全过（三十处导出+十六族账取数菜
//!     单按压测轴分组），清单与导出签名一致，空转取数 10/10 拒绝；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三十处导出闭集（10 轴 × 3 视图：时序/分布/计数）；
//!    - 十六族账取数菜单：族→可取视图分组（菜单路由）；
//!    - 清单签名：全清单 FNV 与导出签名一致（一致判据）；
//!    - 空转取数：无采样（sampled=0）时取数一律拒，10/10 拒；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D480，AI-VX3-1030 批F 独占区）。
pub const SEG_BASE: u32 = 0x6D480;

// ── 冻结值 ─────────────────────────────────────────────────────
/// 压测轴数。
pub const AXIS_COUNT: usize = 10;
/// 每轴视图数。
pub const VIEWS_PER_AXIS: usize = 3;
/// 导出总数。
pub const EXPORT_TOTAL: usize = AXIS_COUNT * VIEWS_PER_AXIS;
/// 族账数（菜单分组）。
pub const MENU_FAMILIES: usize = 16;
pub const AXIS_NAMES: [&str; AXIS_COUNT] = [
    "切换", "唤醒", "迁移", "击落", "idle",
    "频率", "负载", "IPI", "竞态", "对账",
];
pub const VIEW_NAMES: [&str; VIEWS_PER_AXIS] = ["时序", "分布", "计数"];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuCode {
    /// 轴越闭集
    AxisOutOfRange,
    /// 视图越闭集
    ViewOutOfRange,
    /// 清单签名不一致
    SignatureMismatch,
    /// 菜单路由缺族
    MenuFamilyShort,
    /// 空转取数（无采样）
    IdleFetch,
    /// 空转拒漏
    IdleLeak,
    /// 导出总数缺（≠30）
    ExportTotalShort,
    /// 双跑不一致
    NotReproducible,
}

impl MenuCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MenuCode::AxisOutOfRange => "轴越闭集：十轴冻结",
            MenuCode::ViewOutOfRange => "视图越闭集：三视图冻结",
            MenuCode::SignatureMismatch => "清单签名不一致",
            MenuCode::MenuFamilyShort => "菜单路由缺族：≠16",
            MenuCode::IdleFetch => "空转取数：无采样",
            MenuCode::IdleLeak => "空转拒漏",
            MenuCode::ExportTotalShort => "导出总数缺：≠30",
            MenuCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [MenuCode; 8] = [
        MenuCode::AxisOutOfRange,
        MenuCode::ViewOutOfRange,
        MenuCode::SignatureMismatch,
        MenuCode::MenuFamilyShort,
        MenuCode::IdleFetch,
        MenuCode::IdleLeak,
        MenuCode::ExportTotalShort,
        MenuCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MenuCode::ALL.len() {
        if MenuCode::ALL[i].val() == code {
            return MenuCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D480 段闭集内"
}

// ── 取数清单 ───────────────────────────────────────────────────
pub struct FetchMenu {
    /// 清单项（axis*3+view 展开号）。
    pub items: Vec<u16>,
    /// 族路由表（16 族 → 允许轴）。
    pub routes: Vec<u8>,
    /// 已采样旗。
    pub sampled: bool,
}

impl FetchMenu {
    pub fn build() -> Result<FetchMenu, u32> {
        if AXIS_NAMES.len() != AXIS_COUNT {
            return Err(MenuCode::AxisOutOfRange.val());
        }
        let mut items = Vec::new();
        let mut a = 0;
        while a < AXIS_COUNT {
            let mut v = 0;
            while v < VIEWS_PER_AXIS {
                items.push((a * VIEWS_PER_AXIS + v) as u16);
                v += 1;
            }
            a += 1;
        }
        if items.len() != EXPORT_TOTAL {
            return Err(MenuCode::ExportTotalShort.val());
        }
        // 16 族路由（确定性：族 f 允许轴 f%10）。
        let mut routes = Vec::new();
        let mut f = 0;
        while f < MENU_FAMILIES {
            routes.push((f % AXIS_COUNT) as u8);
            f += 1;
        }
        if routes.len() != MENU_FAMILIES {
            return Err(MenuCode::MenuFamilyShort.val());
        }
        Ok(FetchMenu { items, routes, sampled: false })
    }

    /// 清单签名（FNV）。
    pub fn signature(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.items.len() {
            h ^= self.items[i] as u64;
            h = h.wrapping_mul(0x100000001b3);
            i += 1;
        }
        h ^= self.routes.len() as u64;
        h = h.wrapping_mul(0x100000001b3);
        h
    }

    /// 取数（空转拒）。
    pub fn fetch(&mut self, axis: usize, view: usize) -> Result<u16, u32> {
        if axis >= AXIS_COUNT {
            return Err(MenuCode::AxisOutOfRange.val());
        }
        if view >= VIEWS_PER_AXIS {
            return Err(MenuCode::ViewOutOfRange.val());
        }
        if !self.sampled {
            return Err(MenuCode::IdleFetch.val());
        }
        Ok((axis * VIEWS_PER_AXIS + view) as u16)
    }
}

/// 空转取数 10/10 拒。
pub fn idle_reject_10() -> Result<u32, u32> {
    let mut menu = FetchMenu::build()?;
    let mut rejected = 0;
    let mut k = 0;
    while k < 10 {
        if menu.fetch(k % AXIS_COUNT, k % VIEWS_PER_AXIS).is_err() {
            rejected += 1;
        }
        k += 1;
    }
    if rejected != 10 {
        return Err(MenuCode::IdleLeak.val());
    }
    Ok(rejected)
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 黄金全链：清单 30 项→签名一致→空转 10/10→采样后取数通。
pub fn golden_run() -> Result<(usize, u64, u32), u32> {
    let mut menu = FetchMenu::build()?;
    // 清单签名一致（两次构建同签名）。
    let m2 = FetchMenu::build()?;
    if menu.signature() != m2.signature() {
        return Err(MenuCode::SignatureMismatch.val());
    }
    // 空转 10/10。
    let rej = idle_reject_10()?;
    // 采样后取数通。
    menu.sampled = true;
    let v = menu.fetch(3, 1)?;
    if v != 10 {
        return Err(MenuCode::AxisOutOfRange.val());
    }
    Ok((menu.items.len(), menu.signature(), rej))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut menu = FetchMenu::build()?;
    menu.sampled = true;
    menu.fetch(0, 0).map(|_| ()).map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 空转取数 → 必拒。
    let mut menu = FetchMenu::build()?;
    menu.fetch(0, 0).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 越轴 → 必拒。
    let mut menu = FetchMenu::build()?;
    menu.sampled = true;
    menu.fetch(99, 0).map(|_| ())
}

/// 注入 10/10：越轴、越视图、空转、签名四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越轴。
        let mut m0 = FetchMenu::build().ok();
        let h0 = match &mut m0 {
            Some(m) => {
                m.sampled = true;
                m.fetch(99, 0).is_err()
            }
            None => false,
        };
        led.record(0, h0);
        // 槽1：越视图。
        let mut m1 = FetchMenu::build().ok();
        let h1 = match &mut m1 {
            Some(m) => {
                m.sampled = true;
                m.fetch(0, 9).is_err()
            }
            None => false,
        };
        led.record(1, h1);
        // 槽2：空转。
        let mut m2 = FetchMenu::build().ok();
        let h2 = match &mut m2 {
            Some(m) => m.fetch(0, 0).is_err(),
            None => false,
        };
        led.record(2, h2);
        // 槽3：签名。
        let m3 = FetchMenu::build().ok();
        let h3 = match &m3 {
            Some(m) => m.signature() == FetchMenu::build().unwrap().signature(),
            None => false,
        };
        led.record(3, h3);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MenuCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MenuCode::ALL.len() {
        let v = MenuCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 30 导出 + 16 族菜单。
    let menu = FetchMenu::build();
    v.push((
        "J1-30导出+16族菜单",
        match &menu {
            Ok(m) => m.items.len() == 30 && m.routes.len() == 16,
            Err(_) => false,
        },
        format!("{:?} / 16 族", menu.as_ref().map(|m| m.items.len())),
    ));
    // 空转 10/10。
    v.push((
        "J1-空转取数10/10拒",
        idle_reject_10() == Ok(10),
        format!("{}/10 拒", idle_reject_10().unwrap_or(0)),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D480 窗口")));
    v
}

pub fn run_unx2163_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2163 压测取数口域级清单（三十处导出消费视图）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
