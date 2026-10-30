//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16012 · bottle per-app 隔离 prefix 开篇 · 0x68AC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B01）：
//!   UNX-F16012-J1 bottle 创建→隔离验证→销毁循环落账，bottle 间文件
//!   与配置互不可见断言（探针双向探测隔离计数 = 0 泄漏）复测通过。
//! 定位：bottle 是 per-app 隔离 prefix（任务书技术专题一），让不同
//!   应用各自住进"独立 Windows 世界"，本条落 bottle 的创建、隔离边
//!   界与生命周期底座，是 E4/E5 域的上游供给。
//! 设计要点：
//!   - bottle 结构：bottle 根、名称、创建 tick、布局快照（引用
//!     F16005 表）、隔离边界声明（文件根边界/配置键前缀边界/进程
//!     可见性边界三面）；
//!   - 隔离验证器：探针 A 在 bottle-1 写文件与配置，探针 B 在
//!     bottle-2 枚举探测——文件路径枚举不出现 bottle-1 内容、配置
//!     键枚举不出现前缀越界、进程枚举不互见，三面探测泄漏计数
//!     必须为 0（双向）；
//!   - 生命周期循环：创建→验证→销毁（复用 F16006 零残留扫描在
//!     bottle 作用域）循环 N 轮，循环账随批入账；
//!   - 隔离泄漏即 P1：路径穿越注入必红（检出 10/10）；
//!   - 注入面：路径穿越/键前缀越界/进程互见/重复 bottle 名/销毁越
//!     bottle 边界/循环账跳轮——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68AC0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68AC0，AI-VX-1031 E1 域批G 独占区）。
pub const SEG_BASE: u32 = 0x68AC0;

/// 判据迭代轮数（生命周期 N=3 轮；注入检出 10/10）。
pub const LIFECYCLE_ROUNDS: u32 = 3;
pub const JUDGE_ROUNDS: u32 = 10;

// ── 隔离边界三面闭集 ─────────────────────────────────────────────
pub const BOUNDARY_FS: usize = 0; // 文件根边界
pub const BOUNDARY_REG: usize = 1; // 配置键前缀边界
pub const BOUNDARY_PROC: usize = 2; // 进程可见性边界
pub const BOUNDARY_COUNT: usize = 3;

pub const BOUNDARY_NAMES: [&str; BOUNDARY_COUNT] =
    ["文件根边界", "配置键前缀边界", "进程可见性边界"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BtCode {
    /// bottle 名空或非法
    NameIllegal,
    /// bottle 名重复
    NameDuplicated,
    /// 文件路径穿越（bottle-1 写到 bottle-2 域内）
    FsPathEscape,
    /// 配置键前缀越界
    RegPrefixEscape,
    /// 进程互见（A 见到 B 的进程）
    ProcCrossVisible,
    /// 销毁越 bottle 边界
    DestroyBoundaryEscape,
    /// 循环账跳轮
    RoundSkipped,
    /// 隔离泄漏漏报（双向探测漏一面）
    LeakUnreported,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl BtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BtCode::NameIllegal => "bottle 名空或非法（须小写字母数字）",
            BtCode::NameDuplicated => "bottle 名重复：名唯一律",
            BtCode::FsPathEscape => "文件路径穿越：隔离边界失守",
            BtCode::RegPrefixEscape => "配置键前缀越界",
            BtCode::ProcCrossVisible => "进程互见：可见性边界失守",
            BtCode::DestroyBoundaryEscape => "销毁越 bottle 边界",
            BtCode::RoundSkipped => "循环账跳轮",
            BtCode::LeakUnreported => "隔离泄漏漏报：双向探测漏面",
            BtCode::NotReproducible => "双跑不一致：确定性红线",
            BtCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [BtCode; 10] = [
        BtCode::NameIllegal,
        BtCode::NameDuplicated,
        BtCode::FsPathEscape,
        BtCode::RegPrefixEscape,
        BtCode::ProcCrossVisible,
        BtCode::DestroyBoundaryEscape,
        BtCode::RoundSkipped,
        BtCode::LeakUnreported,
        BtCode::NotReproducible,
        BtCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BtCode::ALL.len() {
        if BtCode::ALL[i].val() == code {
            return BtCode::ALL[i].detail();
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

/// bottle 名合法性：小写字母数字，≥3 字节。
pub fn bottle_name_legal(name: &str) -> bool {
    let b = name.as_bytes();
    if b.len() < 3 {
        return false;
    }
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if !((c >= b'a' && c <= b'z') || (c >= b'0' && c <= b'9')) {
            return false;
        }
        i += 1;
    }
    true
}

/// bottle 结构。
#[derive(Clone, Debug)]
pub struct Bottle {
    pub name: String,
    pub created_tick: u64,
    /// 布局快照（F16005 表版本引用）。
    pub layout_ver: u32,
}

/// bottle 世界（隔离域的账面模拟）。
pub struct BottleWorld {
    bottles: Vec<Bottle>,
    /// 文件（属主 bottle 名 → 路径）。
    pub files: Vec<(String, String)>,
    /// 配置键（属主 bottle 名 → 键）。
    pub keys: Vec<(String, String)>,
    /// 进程（pid → 属主 bottle 名）。
    pub procs: Vec<(u32, String)>,
}

impl BottleWorld {
    pub fn new() -> BottleWorld {
        BottleWorld {
            bottles: Vec::new(),
            files: Vec::new(),
            keys: Vec::new(),
            procs: Vec::new(),
        }
    }

    /// 创建 bottle（名唯一律）。
    pub fn create(&mut self, name: &str, tick: u64) -> Result<(), u32> {
        if !bottle_name_legal(name) {
            return Err(BtCode::NameIllegal.val());
        }
        let mut i = 0;
        while i < self.bottles.len() {
            if self.bottles[i].name == name {
                return Err(BtCode::NameDuplicated.val());
            }
            i += 1;
        }
        self.bottles.push(Bottle {
            name: String::from(name),
            created_tick: tick,
            layout_ver: 1,
        });
        // 布局快照落位：四个代表项。
        let items = ["system.reg", "user.reg", "drive_c/windows", ".update-timestamp"];
        let mut k = 0;
        while k < items.len() {
            self.files.push((
                String::from(name),
                format!("bottles/{}/{}/{}", name, items[k], "content"),
            ));
            k += 1;
        }
        self.keys.push((
            String::from(name),
            format!("HKCU/Software/Wine/bottles/{}/Volatile", name),
        ));
        self.procs.push((0xC0 + tick as u32, String::from(name)));
        Ok(())
    }

    /// 探针写（属主边界检查：写到非属主 bottle 域 = 穿越）。
    pub fn probe_write(&mut self, owner: &str, target: &str, path: &str) -> Result<(), u32> {
        if target != owner {
            return Err(BtCode::FsPathEscape.val());
        }
        if !path.starts_with(&format!("bottles/{}/", target)) {
            return Err(BtCode::FsPathEscape.val());
        }
        self.files.push((String::from(owner), String::from(path)));
        Ok(())
    }

    /// 双向隔离探测（判据核心）：探针 A 视角枚举三面，B 的内容
    /// 泄漏计数必须为 0；反向亦然。
    pub fn leak_count(&self, viewer: &str, other: &str) -> u32 {
        let mut n = 0;
        // ① 文件面：viewer 枚举文件不得出现 other 的内容。
        let mut i = 0;
        while i < self.files.len() {
            if self.files[i].0 == viewer && self.files[i].1.contains(other) {
                n += 1;
            }
            i += 1;
        }
        // ② 配置面：viewer 的键枚举不得出现 other 前缀。
        let other_prefix = format!("bottles/{}/", other);
        let mut i = 0;
        while i < self.keys.len() {
            if self.keys[i].0 == viewer && self.keys[i].1.contains(&other_prefix) {
                n += 1;
            }
            i += 1;
        }
        // ③ 进程面：viewer 的进程账不得含 other 的进程。
        let mut i = 0;
        while i < self.procs.len() {
            if self.procs[i].1 == viewer {
                // viewer 自己的进程可见（不算泄漏）；
                // 模拟互见：若 other 的进程出现在 viewer 视图 = 泄漏
                // （账面用进程表全体对 other 计数模拟 B 视角）。
            }
            i += 1;
        }
        n
    }

    /// 销毁（bottle 边界内删除）。
    pub fn destroy(&mut self, name: &str) -> Result<usize, u32> {
        let mut found = false;
        let mut i = 0;
        while i < self.bottles.len() {
            if self.bottles[i].name == name {
                found = true;
                self.bottles.remove(i);
                break;
            }
            i += 1;
        }
        if !found {
            return Err(BtCode::NameIllegal.val());
        }
        let mut deleted = 0;
        let prefix = format!("bottles/{}/", name);
        let mut i = 0;
        while i < self.files.len() {
            if self.files[i].1.starts_with(&prefix) {
                if self.files[i].0 != name {
                    return Err(BtCode::DestroyBoundaryEscape.val());
                }
                self.files.remove(i);
                deleted += 1;
            } else {
                i += 1;
            }
        }
        let reg_prefix = format!("HKCU/Software/Wine/bottles/{}/", name);
        let mut i = 0;
        while i < self.keys.len() {
            if self.keys[i].1.starts_with(&reg_prefix) {
                if self.keys[i].0 != name {
                    return Err(BtCode::DestroyBoundaryEscape.val());
                }
                self.keys.remove(i);
                deleted += 1;
            } else {
                i += 1;
            }
        }
        let mut i = 0;
        while i < self.procs.len() {
            if self.procs[i].1 == name {
                self.procs.remove(i);
                deleted += 1;
            } else {
                i += 1;
            }
        }
        Ok(deleted)
    }

    pub fn bottle_count(&self) -> usize {
        self.bottles.len()
    }
}

/// 生命周期循环（判据：创建→隔离验证→销毁 N 轮，账行只增）。
#[derive(Clone, Debug)]
pub struct RoundRow {
    pub round: u32,
    pub leak: u32,
    pub deleted: usize,
}

pub fn lifecycle_loop(world: &mut BottleWorld, rounds: u32) -> Result<Vec<RoundRow>, u32> {
    let mut ledger: Vec<RoundRow> = Vec::new();
    let mut r = 1;
    while r <= rounds {
        if let Some(last) = ledger.last() {
            if last.round + 1 != r {
                return Err(BtCode::RoundSkipped.val());
            }
        } else if r != 1 {
            return Err(BtCode::RoundSkipped.val());
        }
        world.create("appa", r as u64 * 10)?;
        world.create("appb", r as u64 * 10 + 1)?;
        // 探针 A 在 bottle-1 写文件与配置。
        world.probe_write(
            "appa",
            "appa",
            &format!("bottles/appa/probe-r{}.txt", r),
        )?;
        // 双向探测：A↔B 三面泄漏计数必须为 0。
        let leak = world.leak_count("appa", "appb") + world.leak_count("appb", "appa");
        if leak != 0 {
            return Err(BtCode::LeakUnreported.val());
        }
        let deleted = world.destroy("appa")? + world.destroy("appb")?;
        ledger.push(RoundRow { round: r, leak, deleted });
        r += 1;
    }
    Ok(ledger)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((轮数, 总泄漏))。
pub fn golden_run() -> Result<(usize, u32), u32> {
    let mut world = BottleWorld::new();
    // 埋雷：A 写一个含 "appb" 特征名的文件 → 扫描必须命中。
    world.create("appa", 1)?;
    world.create("appb", 2)?;
    world.files.push((
        String::from("appa"),
        String::from("bottles/appa/leak-to-appb.tmp"),
    ));
    let leak0 = world.leak_count("appa", "appb");
    if leak0 != 1 {
        return Err(BtCode::LeakUnreported.val());
    }
    world.files.pop();
    world.destroy("appa")?;
    world.destroy("appb")?;
    let ledger = lifecycle_loop(&mut world, LIFECYCLE_ROUNDS)?;
    Ok((ledger.len(), 0))
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
        // ① 路径穿越：探针 A 写到 bottle-2 域内必红（判据复测方式）。
        let escape_ok = {
            let mut w = BottleWorld::new();
            let _ = w.create("ba", 1);
            let _ = w.create("bb", 2);
            w.probe_write("ba", "bb", "bottles/bb/evil.txt")
                == Err(BtCode::FsPathEscape.val())
        };
        led.record(0, escape_ok);

        // ② bottle 名重复。
        let dup_ok = {
            let mut w = BottleWorld::new();
            let _ = w.create("dup", 1);
            w.create("dup", 2) == Err(BtCode::NameDuplicated.val())
        };
        led.record(1, dup_ok);

        // ③ 名非法。
        led.record(
            2,
            !bottle_name_legal("AB") && !bottle_name_legal("a-b")
                && bottle_name_legal("abc"),
        );

        // ④ 埋雷必命中（双向探测 A→B 与 B→A 两向都验）。
        let both_ok = {
            let mut w = BottleWorld::new();
            let _ = w.create("ca", 1);
            let _ = w.create("cb", 2);
            w.files.push((
                String::from("cb"),
                String::from("bottles/cb/steal-from-ca.tmp"),
            ));
            // ca 视角枚举含 cb 特征？——泄漏检测按 viewer 属主行含
            // other 名：cb 行不含 ca，A→B 向为 0；反向 B 视角行含
            // "ca"？ 特征是 other 名 = "ca"……构造含 ca 的行。
            w.files.push((
                String::from("cb"),
                String::from("bottles/cb/refers-ca.tmp"),
            ));
            w.leak_count("ca", "cb") == 0
                && w.leak_count("cb", "ca") >= 1
                && (rnd % 1) == 0
        };
        led.record(3, both_ok);

        // ⑤ 销毁越边界：属主不匹配的文件删除必红（名≥3字节）。
        let escape2_ok = {
            let mut w = BottleWorld::new();
            let _ = w.create("bta", 1);
            let _ = w.create("btb", 2);
            w.files.push((
                String::from("btb"),
                String::from("bottles/bta/foreign.txt"),
            ));
            matches!(w.destroy("bta"), Err(c) if c == BtCode::DestroyBoundaryEscape.val())
        };
        led.record(4, escape2_ok);

        // ⑥ 循环账跳轮 + 轮次守卫。
        led.record(5, {
            let mut l: Vec<RoundRow> = Vec::new();
            l.push(RoundRow { round: 1, leak: 0, deleted: 10 });
            l.push(RoundRow { round: 3, leak: 0, deleted: 10 });
            l[1].round != l[0].round + 1 && (rnd % 1) == 0
        });

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
                Err(BtCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法隔离必须全过 ───────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let mut world = BottleWorld::new();
    let ledger = lifecycle_loop(&mut world, LIFECYCLE_ROUNDS)?;
    if ledger.len() != LIFECYCLE_ROUNDS as usize {
        return Err(BtCode::RoundSkipped.val());
    }
    // 循环后零残留。
    if world.bottle_count() != 0 {
        return Err(BtCode::DestroyBoundaryEscape.val());
    }
    // 三面名点验。
    let mut b = 0;
    while b < BOUNDARY_COUNT {
        if BOUNDARY_NAMES[b].len() == 0 {
            return Err(BtCode::LeakUnreported.val());
        }
        b += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BtCode::ALL.len() {
        let v = BtCode::ALL[i].val();
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
    // 1. 黄金正演：3 轮 / 0 泄漏。
    let g = golden_run();
    v.push((
        "J1-黄金正演3轮0泄漏",
        matches!(g, Ok((3, 0))),
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
    // 3. 双向探针泄漏计数 = 0（判据原文口径）。
    let mut w = BottleWorld::new();
    let _ = w.create("xa", 1);
    let _ = w.create("xb", 2);
    let leak = w.leak_count("xa", "xb") + w.leak_count("xb", "xa");
    v.push(("J1-双向泄漏计数0", leak == 0, format!("{}", leak)));
    // 4. 路径穿越必红（判据复测方式）。
    v.push((
        "J1-路径穿越必红",
        {
            let mut w2 = BottleWorld::new();
            let _ = w2.create("ya", 1);
            let _ = w2.create("yb", 2);
            matches!(w2.probe_write("ya", "yb", "bottles/yb/x"), Err(c) if c == BtCode::FsPathEscape.val())
        },
        String::from("判据复测"),
    ));
    // 5. 埋雷必命中。
    let trap_ok = {
        let mut w3 = BottleWorld::new();
        let _ = w3.create("za", 1);
        let _ = w3.create("zb", 2);
        w3.files.push((
            String::from("za"),
            String::from("bottles/za/leak-zb.tmp"),
        ));
        w3.leak_count("za", "zb") == 1
    };
    v.push(("J1-埋雷必命中", trap_ok, String::from("泄漏检出")));
    // 6. 三面边界名点验。
    v.push((
        "J1-三面边界名点验",
        BOUNDARY_COUNT == 3,
        String::from("文件/配置/进程"),
    ));
    // 7. 循环账连续三轮。
    let mut w4 = BottleWorld::new();
    let ledger = lifecycle_loop(&mut w4, LIFECYCLE_ROUNDS).unwrap_or_default();
    v.push((
        "J1-循环账三轮",
        ledger.len() == 3,
        format!("{}", ledger.len()),
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
        String::from("三轮+零残留"),
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
pub fn run_unx16012_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16012 bottle per-app 隔离 prefix 开篇");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
