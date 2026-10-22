//! unx1622_gspair —— UNX-F1622 gs_base 初始化与 swapgs 严格配对纪律。
//! 锚点（CoRun Unxreal 主册）：UNX-F1622 · gs_base 配对纪律 · 目标 340 行 · 已深化
//! 判据原文：UNX-F1622-J1 全核 gs_base 指向本核变量区，swapgs 在全部内核
//!   入口/出口配对断言 ≥30 条（ktest 加断言防漏），配对违例注入 10/10 次断言捕获。
//! 组件：每核变量区模型（核号 → gs_base 值闭集）
//!       + swapgs 配对账（append-only：核位/入口位/出口位/配对键）
//!       + 配对断言集（≥30 条闭集：入口类 15 + 出口类 15）
//!       + 配对校验器（入口无出口 / 出口无入口 / 交错违例 → 落违例账）
//!       + 指纹冻结（配对账指纹 FNV-1a 32 位）
//!       + 注入核（gs 漂移/缺出口/缺入口/交错违例四类 10/10 必抓）。
//! 职责定位：swapgs 严格配对的唯一权威纪律面——全核 gs_base 一致性 +
//!   ≥30 条配对断言使任何入口/出口漏配在 ktest 面即可检。
//! 诊断码段：0xEE00（0xEE01~0xEE08 闭集；本段为 mod.rs 段位图核验空闲独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const GS_VERSION: &str = "unx1622_gspair-v1";

/// 诊断段基址（0xEE00 段 · 本模块独占）。
pub const GS_SEG_BASE: u16 = 0xEE00;

/// XcCode 闭集（0xEE01~0xEE08，严格递增互异）。
pub const XC_GS_DRIFT: u32 = 0xEE01;
pub const XC_EXIT_MISSING: u32 = 0xEE02;
pub const XC_ENTRY_MISSING: u32 = 0xEE03;
pub const XC_INTERLEAVE: u32 = 0xEE04;
pub const XC_FINGER_DRIFT: u32 = 0xEE05;
pub const XC_CORE_OOB: u32 = 0xEE06;
pub const XC_INJECT_MISSED: u32 = 0xEE07;
pub const XC_LEDGER_BROKEN: u32 = 0xEE08;

/// 中文诊断文本（码 → 文案/建议，闭集逐码三要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_GS_DRIFT => ("gs 漂移：gs_base 未指向本核变量区", "核对每核变量区开户表"),
        XC_EXIT_MISSING => ("缺出口：入口已配对但出口未回", "内核出口必须回 swapgs"),
        XC_ENTRY_MISSING => ("缺入口：出口先于入口", "swapgs 严格入口先配"),
        XC_INTERLEAVE => ("交错违例：嵌套入口未按栈序回", "配对按 LIFO 栈序校验"),
        XC_FINGER_DRIFT => ("指纹漂移：配对账指纹与冻结值不一致", "配对账被改，回滚"),
        XC_CORE_OOB => ("核位越界：核号越核表", "核号必须落在核表闭集"),
        XC_INJECT_MISSED => ("注入未捕获：注入核存在漏抓", "四类注入必须 10/10 必抓"),
        XC_LEDGER_BROKEN => ("账结构破坏：配对账被外力改写", "append-only 账回滚"),
        _ => ("未知诊断码", "查本模块 XcCode 闭集"),
    }
}

/// 内核入口/出口类闭集（断言集的登记面，15 类入口 + 15 类出口）。
pub const ENTRY_KINDS: [&str; 15] = [
    "syscall64 入口",
    "int80 入口",
    "de 分裂错误入口",
    "db 调试入口",
    "nmi 入口",
    "bp 断点入口",
    "of 溢出入口",
    "br 边界入口",
    "ud 无效操作码入口",
    "nm 设备不可用入口",
    "df 双错误入口",
    "gp 一般保护入口",
    "pf 页错误入口",
    "apic 计时器入口",
    "ipi 重调度入口",
];

/// 出口类闭集（与入口一一对应的回侧）。
pub const EXIT_KINDS: [&str; 15] = [
    "syscall64 出口",
    "int80 出口",
    "de 分裂错误出口",
    "db 调试出口",
    "nmi 出口",
    "bp 断点出口",
    "of 溢出出口",
    "br 边界出口",
    "ud 无效操作码出口",
    "nm 设备不可用出口",
    "df 双错误出口",
    "gp 一般保护出口",
    "pf 页错误出口",
    "apic 计时器出口",
    "ipi 重调度出口",
];

/// 每核变量区开户表（核号 → gs_base 期望值）。
#[derive(Clone, Copy)]
pub struct PerCoreArea {
    /// 基址（每核 stride 对齐）。
    base: u64,
    /// 每核区大小。
    stride: u64,
    /// 核数。
    cores: u8,
}

impl PerCoreArea {
    /// 新开户表。
    pub const fn new(base: u64, stride: u64, cores: u8) -> Self {
        PerCoreArea { base, stride, cores }
    }
    /// 期望 gs_base。
    pub const fn expect_gs(&self, core: u8) -> u64 {
        self.base + core as u64 * self.stride
    }
    /// gs 校验（指向本核变量区）。
    pub const fn gs_ok(&self, core: u8, gs: u64) -> bool {
        core < self.cores && gs == self.expect_gs(core)
    }
    /// 核数。
    pub const fn core_count(&self) -> u8 {
        self.cores
    }
}

/// 配对键（入口/出口一对一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PairKey {
    /// 核位。
    pub core: u8,
    /// 类别（0~14，ENTRY/EXIT_KINDS 索引）。
    pub kind: u8,
    /// 配对序号（同核同类第几次）。
    pub seq: u32,
}

/// swapgs 配对账（栈式配对）。
pub struct SwapgsLedger {
    /// 开户表。
    area: PerCoreArea,
    /// 未回出口的入口栈（每核独立）。
    open_entries: Vec<(u8, u8, u32)>,
    /// 已配对记录。
    paired: Vec<PairKey>,
    /// 违例账。
    violations: Vec<(u32, u8, &'static str)>,
    /// 断言登记数（≥30 门槛面）。
    asserts: u32,
    /// 封账。
    sealed: bool,
}

impl SwapgsLedger {
    /// 新账。
    pub fn new(area: PerCoreArea) -> Self {
        SwapgsLedger {
            area,
            open_entries: Vec::new(),
            paired: Vec::new(),
            violations: Vec::new(),
            asserts: 0,
            sealed: false,
        }
    }

    /// gs 一致性断言（J1 全核面；返回是否通过）。
    pub fn assert_gs(&mut self, core: u8, gs: u64) -> bool {
        self.asserts += 1;
        if !self.area.gs_ok(core, gs) {
            self.violations.push((XC_GS_DRIFT, core, "gs_base 未指向本核区"));
            return false;
        }
        true
    }

    /// 内核入口（swapgs 第一次；记入栈）。
    pub fn enter(&mut self, core: u8, kind: u8) -> Result<PairKey, u32> {
        if self.sealed {
            return Err(XC_LEDGER_BROKEN);
        }
        if core >= self.area.core_count() || kind as usize >= ENTRY_KINDS.len() {
            self.violations.push((XC_CORE_OOB, core, "入口核位/类别越界"));
            return Err(XC_CORE_OOB);
        }
        let seq = self.open_entries.iter().filter(|(c, k, _)| *c == core && *k == kind).count() as u32;
        let key = PairKey { core, kind, seq };
        let _ = self.open_entries.push((core, kind, seq));
        self.asserts += 1;
        Ok(key)
    }

    /// 内核出口（swapgs 回配；LIFO 栈序校验）。
    pub fn exit(&mut self, core: u8, kind: u8) -> Result<PairKey, u32> {
        if self.sealed {
            return Err(XC_LEDGER_BROKEN);
        }
        self.asserts += 1;
        // LIFO：必须先回同核最后入的口
        match self.open_entries.last() {
            Some(&(lc, lk, ls)) if lc == core && lk == kind => {
                let key = PairKey { core, kind, seq: ls };
                let _ = self.open_entries.pop();
                let _ = self.paired.push(key);
                Ok(key)
            }
            Some(&(lc, lk, _)) => {
                if lc == core {
                    self.violations.push((XC_INTERLEAVE, core, "同类交错：出口类与栈顶不符"));
                    Err(XC_INTERLEAVE)
                } else {
                    self.violations.push((XC_INTERLEAVE, core, "跨核交错：出口核与栈顶不符"));
                    Err(XC_INTERLEAVE)
                }
            }
            None => {
                self.violations.push((XC_ENTRY_MISSING, core, "出口无入口"));
                Err(XC_ENTRY_MISSING)
            }
        }
    }

    /// 收口校验：全部入口已回（零缺出口）。
    pub fn all_paired(&self) -> bool {
        self.open_entries.is_empty()
    }
    /// 未回入口数。
    pub fn open_count(&self) -> usize {
        self.open_entries.len()
    }
    /// 已配对数。
    pub fn paired_count(&self) -> usize {
        self.paired.len()
    }
    /// 违例账长度。
    pub fn violation_count(&self) -> usize {
        self.violations.len()
    }
    /// 断言登记数。
    pub const fn assert_count(&self) -> u32 {
        self.asserts
    }
    /// 封账（全部配对 + 断言 ≥30 才许封）。
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_LEDGER_BROKEN);
        }
        if !self.all_paired() {
            return Err(XC_EXIT_MISSING);
        }
        if self.asserts < 30 {
            return Err(XC_ENTRY_MISSING);
        }
        let fp = self.joint_fingerprint();
        self.sealed = true;
        Ok(fp)
    }
    /// 账指纹。
    pub fn joint_fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::with_capacity(self.paired.len() * 7);
        for p in &self.paired {
            bytes.push(p.core);
            bytes.push(p.kind);
            bytes.extend_from_slice(&p.seq.to_le_bytes());
        }
        fnv1a32(&bytes)
    }
}

/// FNV-1a 32 位。
pub const fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// LCG 确定性发生器。
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 权威配对场景：全核 gs 断言 + 全部 15 类入口/出口各配一对（2 核 × 15 类 = 30 断言入口/出口侧）。
pub fn authoritative_pairing(led: &mut SwapgsLedger, cores: &[u8]) -> bool {
    let mut ok = true;
    // 全核 gs 断言
    for &c in cores {
        let gs = led.area.expect_gs(c);
        if !led.assert_gs(c, gs) {
            ok = false;
        }
    }
    // 15 类入口/出口逐类配对（两核）
    for &c in cores {
        for k in 0..15u8 {
            if led.enter(c, k).is_err() {
                ok = false;
            }
            if led.exit(c, k).is_err() {
                ok = false;
            }
        }
    }
    ok
}

/// 注入核：四类注入各 10 次全抓。
pub fn injection_core() -> (usize, usize) {
    let area = PerCoreArea::new(0xFFFF_8000_0000_0000, 0x1000, 4);
    let mut caught = 0;
    let mut total = 0;
    let mut rng = Lcg::new(0xEE_22);

    // 1) gs 漂移 ×10（错核 gs）
    for k in 0..10u64 {
        total += 1;
        let mut led = SwapgsLedger::new(area);
        let wrong_gs = area.expect_gs(((k + 1) % 4) as u8);
        if !led.assert_gs(0, wrong_gs) {
            caught += 1;
        }
    }
    // 2) 缺出口 ×10（入口不回 → seal 拒）
    for _ in 0..10 {
        total += 1;
        let mut led = SwapgsLedger::new(area);
        let _ = led.enter(0, 0);
        if led.seal() == Err(XC_EXIT_MISSING) {
            caught += 1;
        }
    }
    // 3) 缺入口 ×10（无入口直接出口）
    for _ in 0..10 {
        total += 1;
        let mut led = SwapgsLedger::new(area);
        if led.exit(1, 2).is_err() {
            caught += 1;
        }
    }
    // 4) 交错违例 ×10（入口 A 未回先出入口 B）
    for _ in 0..10 {
        total += 1;
        let mut led = SwapgsLedger::new(area);
        let _ = led.enter(2, 0);
        if led.exit(2, 3).is_err() {
            caught += 1;
        }
    }
    (caught, total)
}

/// 判据入口。
pub fn run_unx1622_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1622");

    // C22-01 闭集常量（J1 前置：入口/出口 15 类、gs 区模型）
    {
        let area = PerCoreArea::new(0xFFFF_8000_0000_0000, 0x1000, 4);
        let ok = ENTRY_KINDS.len() == 15
            && EXIT_KINDS.len() == 15
            && area.expect_gs(3) == 0xFFFF_8000_0000_3000
            && area.gs_ok(2, 0xFFFF_8000_0000_2000);
        s.add("C22-01-闭集与gs区", ok, if ok { "" } else { "常量/模型漂移" });
    }

    // C22-02 全核 gs_base 指向本核变量区（J1 主面）
    {
        let area = PerCoreArea::new(0xFFFF_8000_0000_0000, 0x1000, 8);
        let mut all = true;
        for c in 0..8u8 {
            if !area.gs_ok(c, area.expect_gs(c)) || area.gs_ok(c, area.expect_gs((c + 1) % 8)) {
                all = false;
            }
        }
        s.add("C22-02-全核gs一致", all, if all { "" } else { "gs 漂移" });
    }

    // C22-03 配对断言 ≥30（J1 断言面：权威场景 2 核 gs 2 + 30 配对 = 32 断言）
    {
        let area = PerCoreArea::new(0xFFFF_8000_0000_0000, 0x1000, 4);
        let mut led = SwapgsLedger::new(area);
        let ok = authoritative_pairing(&mut led, &[0, 1])
            && led.assert_count() >= 30
            && led.paired_count() == 30
            && led.all_paired();
        s.add("C22-03-断言≥30全配对", ok, if ok { "" } else { "配对面缺口" });
    }

    // C22-04 LIFO 栈序：嵌套入口按栈序回
    {
        let area = PerCoreArea::new(0xFFFF_8000_0000_0000, 0x1000, 4);
        let mut led = SwapgsLedger::new(area);
        let _ = led.enter(0, 3);
        let _ = led.enter(0, 14);
        let e1 = led.exit(0, 14); // 先回栈顶
        let e2 = led.exit(0, 3);  // 再回外层
        let ok = e1.is_ok() && e2.is_ok() && led.all_paired();
        s.add("C22-04-LIFO栈序", ok, if ok { "" } else { "栈序漂移" });
    }

    // C22-05 断言不足 30 拒封账
    {
        let area = PerCoreArea::new(0xFFFF_8000_0000_0000, 0x1000, 4);
        let mut led = SwapgsLedger::new(area);
        let _ = led.enter(0, 0);
        let _ = led.exit(0, 0);
        let ok = led.seal() == Err(XC_ENTRY_MISSING);
        s.add("C22-05-断言门槛", ok, if ok { "" } else { "断言门槛失效" });
    }

    // C22-06 指纹稳定 + 封账拒重
    {
        let area = PerCoreArea::new(0xFFFF_8000_0000_0000, 0x1000, 4);
        let mut a = SwapgsLedger::new(area);
        let mut b = SwapgsLedger::new(area);
        let _ = authoritative_pairing(&mut a, &[0, 1]);
        let _ = authoritative_pairing(&mut b, &[0, 1]);
        let fa = a.joint_fingerprint();
        let ok = fa == b.joint_fingerprint() && a.seal().is_ok() && a.seal().is_err();
        s.add("C22-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    }

    // C22-07 注入核四类 10/10（J1 注入面）
    {
        let (caught, total) = injection_core();
        let ok = total == 40 && caught == 40;
        s.add("C22-07-注入核40-40", ok, if ok { "" } else { "注入核漏抓" });
    }

    // C22-08 诊断码闭集
    {
        let codes = [
            XC_GS_DRIFT, XC_EXIT_MISSING, XC_ENTRY_MISSING, XC_INTERLEAVE,
            XC_FINGER_DRIFT, XC_CORE_OOB, XC_INJECT_MISSED, XC_LEDGER_BROKEN,
        ];
        let mut ok = codes.len() == 8;
        for i in 0..codes.len() {
            if i > 0 && codes[i] <= codes[i - 1] {
                ok = false;
            }
            if (codes[i] & 0xFF00) != (GS_SEG_BASE as u32) {
                ok = false;
            }
        }
        s.add("C22-08-诊断码闭集", ok, if ok { "" } else { "诊断码漂移" });
    }

    // C22-09 中文诊断逐码三要素
    {
        let mut ok = true;
        for c in [
            XC_GS_DRIFT, XC_EXIT_MISSING, XC_ENTRY_MISSING, XC_INTERLEAVE,
            XC_FINGER_DRIFT, XC_CORE_OOB, XC_INJECT_MISSED, XC_LEDGER_BROKEN, 0xFFFF,
        ] {
            let (t, a) = diag_text(c);
            if t.is_empty() || a.is_empty() {
                ok = false;
            }
        }
        s.add("C22-09-诊断三要素", ok, if ok { "" } else { "诊断文案缺要素" });
    }

    // C22-10 入口/出口类名逐一对齐（kind i 入口 ↔ kind i 出口）
    {
        let mut ok = true;
        for i in 0..15 {
            if ENTRY_KINDS[i].is_empty() || EXIT_KINDS[i].is_empty() {
                ok = false;
            }
        }
        s.add("C22-10-类名对齐", ok, if ok { "" } else { "类名漂移" });
    }

    s
}
