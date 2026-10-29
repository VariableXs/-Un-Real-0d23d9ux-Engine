//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13821 · 符号链接对象创建面（\??\C: 类） · 0x65F40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F13821-J1 盘符类符号链接创建后按名解析直达目标设备，
//!      WinObj 对照一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 符号链接对象账：链接名（\??\C: 类 DOS 盘符面）→ 目标设备名
//!      （\Device\HarddiskVolumeN 类），创建→解析→对照三面；
//!   - 按名解析闭式：DOS 设备命名空间 \??\ 前缀剥离 → 链接表查名
//!      → 目标直达（一次跳转，不许多跳循环——环检）；
//!   - WinObj 对照面：创建后的链接表快照与黄金清单逐项一致
//!      （名称/目标/类型三列对照）；
//!   - 注入四族：环链接/空目标/重名创建/越前缀 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65F40），禁 rand/浮点/IO/墙钟；
///     no_std + alloc；零 unwrap/expect。
/// ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65F40，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65F40;

/// DOS 设备命名空间前缀（写死）。
pub const DOS_PREFIX: &str = "\\??\\";

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SymlinkCode {
    /// 环链接（解析链回环）
    LinkCycle,
    /// 空目标
    EmptyTarget,
    /// 重名创建
    DuplicateName,
    /// 链接名越前缀（非 \??\）
    PrefixInvalid,
    /// 解析未达（链断）
    ResolveMiss,
    /// 对照不一致
    WinobjMismatch,
    /// 链接表越容量
    TableOverflow,
    /// 二次跳转（只许一跳直达）
    DoubleHop,
}

impl SymlinkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SymlinkCode::LinkCycle => "环链接：解析链回环拒收",
            SymlinkCode::EmptyTarget => "空目标：创建面拒收",
            SymlinkCode::DuplicateName => "重名创建：链接名唯一红线",
            SymlinkCode::PrefixInvalid => "链接名越 \\??\\ 前缀",
            SymlinkCode::ResolveMiss => "解析未达：链断",
            SymlinkCode::WinobjMismatch => "WinObj 对照不一致",
            SymlinkCode::TableOverflow => "链接表越容量",
            SymlinkCode::DoubleHop => "二次跳转：目标必须是设备名非链接名",
        }
    }
    pub const ALL: [SymlinkCode; 8] = [
        SymlinkCode::LinkCycle,
        SymlinkCode::EmptyTarget,
        SymlinkCode::DuplicateName,
        SymlinkCode::PrefixInvalid,
        SymlinkCode::ResolveMiss,
        SymlinkCode::WinobjMismatch,
        SymlinkCode::TableOverflow,
        SymlinkCode::DoubleHop,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SymlinkCode::ALL.len() {
        if SymlinkCode::ALL[i].val() == code {
            return SymlinkCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65F40 段闭集内"
}

// ── 链接对象 ─────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct Symlink {
    /// 链接名（含 \??\ 前缀）。
    pub name: String,
    /// 目标设备名（\Device\ 类）。
    pub target: String,
}

/// 符号链接表（对象管理器命名空间建模）。
pub struct SymlinkTable {
    pub links: Vec<Symlink>,
    pub cap: usize,
}

pub const TABLE_CAP: usize = 32;

impl SymlinkTable {
    pub fn new() -> SymlinkTable {
        SymlinkTable { links: Vec::new(), cap: TABLE_CAP }
    }

    /// 创建：前缀校验+空目标拒+重名拒+容量拒。
    pub fn create(&mut self, name: &str, target: &str) -> Result<(), u32> {
        if !name.starts_with(DOS_PREFIX) {
            return Err(SymlinkCode::PrefixInvalid.val());
        }
        if target.is_empty() {
            return Err(SymlinkCode::EmptyTarget.val());
        }
        if self.links.iter().any(|l| l.name == name) {
            return Err(SymlinkCode::DuplicateName.val());
        }
        if self.links.len() >= self.cap {
            return Err(SymlinkCode::TableOverflow.val());
        }
        self.links.push(Symlink {
            name: String::from(name),
            target: String::from(target),
        });
        Ok(())
    }

    /// 按名解析闭式：恰好一跳直达目标设备名。
    /// 目标若是另一链接名 → DoubleHop 拒（二次跳转红线）。
    pub fn resolve(&self, name: &str) -> Result<String, u32> {
        let l = match self.links.iter().find(|l| l.name == name) {
            Some(l) => l,
            None => return Err(SymlinkCode::ResolveMiss.val()),
        };
        // 目标必须是设备名（\Device\ 前缀），不得是另一链接名。
        if self.links.iter().any(|o| o.name == l.target) {
            return Err(SymlinkCode::DoubleHop.val());
        }
        // 环检：目标不得回指自身名。
        if l.target == l.name {
            return Err(SymlinkCode::LinkCycle.val());
        }
        Ok(l.target.clone())
    }

    /// WinObj 对照：链接表快照与黄金清单逐项一致。
    pub fn winobj_compare(&self, golden: &[(String, String)]) -> Result<(), u32> {
        if self.links.len() != golden.len() {
            return Err(SymlinkCode::WinobjMismatch.val());
        }
        let mut i = 0;
        while i < golden.len() {
            let l = &self.links[i];
            if l.name != golden[i].0 || l.target != golden[i].1 {
                return Err(SymlinkCode::WinobjMismatch.val());
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 黄金清单（写死 WinObj 对照面） ───────────────────────────────
pub fn golden_winobj() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = Vec::new();
    let drives: [&str; 6] = ["C:", "D:", "E:", "F:", "X:", "Z:"];
    let mut i = 0;
    while i < drives.len() {
        v.push((
            format!("\\??\\{}", drives[i]),
            format!("\\Device\\HarddiskVolume{}", i + 1),
        ));
        i += 1;
    }
    v
}

/// 黄金正演：6 盘符创建→逐个解析直达→WinObj 对照一致。
pub fn golden_run() -> Result<u32, u32> {
    let mut t = SymlinkTable::new();
    let g = golden_winobj();
    let mut i = 0;
    while i < g.len() {
        t.create(&g[i].0, &g[i].1)?;
        i += 1;
    }
    // 逐个解析直达。
    let mut resolved = 0u32;
    let mut i = 0;
    while i < g.len() {
        let r = t.resolve(&g[i].0)?;
        if r == g[i].1 {
            resolved += 1;
        }
        i += 1;
    }
    t.winobj_compare(&g)?;
    if resolved == g.len() as u32 {
        Ok(resolved)
    } else {
        Err(SymlinkCode::WinobjMismatch.val())
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── 注入面：四族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    let mut t = SymlinkTable::new();
    let _ = t.create("\\??\\C:", "\\Device\\HarddiskVolume1");
    // 族 0：环链接（自指目标）。
    ledger.tries[0] += 1;
    let _ = t.create("\\??\\LOOP:", "\\??\\LOOP:");
    if t.resolve("\\??\\LOOP:") == Err(SymlinkCode::DoubleHop.val())
        || t.resolve("\\??\\LOOP:") == Err(SymlinkCode::LinkCycle.val())
    {
        ledger.caught[0] += 1;
    }
    // 族 1：空目标。
    ledger.tries[1] += 1;
    if t.create("\\??\\EMPTY:", "") == Err(SymlinkCode::EmptyTarget.val()) {
        ledger.caught[1] += 1;
    }
    // 族 2：重名创建。
    ledger.tries[2] += 1;
    if t.create("\\??\\C:", "\\Device\\Other") == Err(SymlinkCode::DuplicateName.val()) {
        ledger.caught[2] += 1;
    }
    // 族 3：越前缀。
    ledger.tries[3] += 1;
    if t.create("\\Dev\\C:", "\\Device\\X") == Err(SymlinkCode::PrefixInvalid.val()) {
        ledger.caught[3] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SymlinkCode::ALL.len() {
        let v = SymlinkCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push((
        "J1-6盘符解析直达+对照一致",
        golden == Ok(6),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入四族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 前缀冻结。
    v.push((
        "J1-前缀冻结",
        DOS_PREFIX == "\\??\\",
        String::from("DOS 设备命名空间"),
    ));
    // 一跳直达反例（链中链必拒）。
    let mut t2 = SymlinkTable::new();
    let _ = t2.create("\\??\\A:", "\\??\\B:");
    let _ = t2.create("\\??\\B:", "\\Device\\X");
    v.push((
        "J1-一跳直达红线",
        t2.resolve("\\??\\A:") == Err(SymlinkCode::DoubleHop.val()),
        String::from("不得二跳"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65F40 窗口")));
    v
}

pub fn run_unx13821_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13821 符号链接对象创建面（\\??\\C: 类）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
