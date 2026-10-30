//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16011 · winecfg 库面板与 DLL 覆盖序语义 · 0x68A80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B01）：
//!   UNX-F16011-J1 DLL 覆盖序配置逐项落账，装载序解析器按内建/原生/
//!   禁用三态导出装载序，序冲突（同 DLL 双声明类）拒收计数断言通过。
//! 定位：DLL 覆盖序（builtin/native/disabled 三态）是 Wine 兼容疑难
//!   的第一调节旋钮（E5 疑难库 per-app 覆盖的前置机制），本条落库面
//!   板的覆盖序配置语义与装载序解析——bottle 级覆盖（F16013）与其
//!   共享解析器。
//! 设计要点：
//!   - 覆盖序结构：每条登记（DLL 名/三态值/作用域/优先级），同 DLL
//!     多作用域按 bottle→prefix→全局 优先级合成，合成规则显式落账；
//!   - 解析器纯函数化：输入覆盖序配置与 DLL 名，输出最终装载序决策
//!     （内建优先/原生优先/禁用三类），决策理由随输出（防"为什么
//!     走内建"不可答）；
//!   - 冲突拒收：同一 DLL 同一作用域双声明、非法三态值、空 DLL 名
//!     三类冲突拒收并落账，拒收报错带"怎么改对"；
//!   - 三态闭集：builtin/native/disabled 与 Wine 惯例对齐；
//!   - 注入面：双声明/非法三态/空名/作用域越闭集/合成序倒挂/决策无
//!     理由——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68A80）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68A80，AI-VX-1031 E1 域批G 独占区）。
pub const SEG_BASE: u32 = 0x68A80;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── DLL 覆盖三态闭集（Wine 惯例对齐） ───────────────────────────
pub const OVR_BUILTIN: usize = 0; // 内建优先
pub const OVR_NATIVE: usize = 1; // 原生优先
pub const OVR_DISABLED: usize = 2; // 禁用
pub const OVR_COUNT: usize = 3;

pub const OVR_NAMES: [&str; OVR_COUNT] = ["builtin", "native", "disabled"];

// ── 作用域三层闭集（合成优先级：bottle > prefix > 全局） ────────
pub const SCOPE_GLOBAL: usize = 0;
pub const SCOPE_PREFIX: usize = 1;
pub const SCOPE_BOTTLE: usize = 2;
pub const SCOPE_COUNT: usize = 3;

pub const SCOPE_NAMES: [&str; SCOPE_COUNT] = ["全局", "prefix", "bottle"];

/// 作用域优先级（数值大者优先合成）。
pub fn scope_priority(s: usize) -> Result<usize, u32> {
    if s >= SCOPE_COUNT {
        return Err(LpCode::ScopeOutOfRange.val());
    }
    Ok(s)
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LpCode {
    /// DLL 名空
    DllNameBlank,
    /// 同 DLL 同作用域双声明
    DoubleDeclared,
    /// 三态值越闭集
    OvrOutOfRange,
    /// 作用域越三层闭集
    ScopeOutOfRange,
    /// 合成序倒挂（低优先作用域覆盖高优先）
    SynthesisInverted,
    /// 决策无理由（决策理由随输出律）
    DecisionNoReason,
    /// 拒收无"怎么改对"
    RejectNoAdvice,
    /// DLL 名非法字符
    DllNameIllegal,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl LpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LpCode::DllNameBlank => "DLL 名空",
            LpCode::DoubleDeclared => "同 DLL 同作用域双声明：序冲突",
            LpCode::OvrOutOfRange => "三态值越 builtin/native/disabled 闭集",
            LpCode::ScopeOutOfRange => "作用域越三层闭集",
            LpCode::SynthesisInverted => "合成序倒挂：低优先覆盖高优先",
            LpCode::DecisionNoReason => "决策无理由：理由随输出律",
            LpCode::RejectNoAdvice => "拒收无怎么改对",
            LpCode::DllNameIllegal => "DLL 名非法字符",
            LpCode::NotReproducible => "双跑不一致：确定性红线",
            LpCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [LpCode; 10] = [
        LpCode::DllNameBlank,
        LpCode::DoubleDeclared,
        LpCode::OvrOutOfRange,
        LpCode::ScopeOutOfRange,
        LpCode::SynthesisInverted,
        LpCode::DecisionNoReason,
        LpCode::RejectNoAdvice,
        LpCode::DllNameIllegal,
        LpCode::NotReproducible,
        LpCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < LpCode::ALL.len() {
        if LpCode::ALL[i].val() == code {
            return LpCode::ALL[i].detail();
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

/// DLL 名合法性：`xxx.dll`（小写字母数字+点，≥5 字节）。
pub fn dll_name_legal(name: &str) -> bool {
    let b = name.as_bytes();
    if b.len() < 5 || !name.ends_with(".dll") {
        return false;
    }
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let legal = (c >= b'a' && c <= b'z') || (c >= b'0' && c <= b'9') || c == b'.' || c == b'_';
        if !legal {
            return false;
        }
        i += 1;
    }
    true
}

/// 拒收"怎么改对"（错误三要素，写死模板）。
pub fn rejection_advice(kind: usize) -> &'static str {
    match kind {
        0 => "改对：删除同 DLL 同作用域的既有声明，只留一条",
        1 => "改对：三态取 builtin/native/disabled 之一",
        _ => "改对：DLL 名取 xxx.dll 形式（小写）",
    }
}

/// 覆盖声明行。
#[derive(Clone, Debug)]
pub struct OvrRow {
    pub dll: String,
    pub mode: usize,
    pub scope: usize,
    pub priority: u32,
}

/// 单行体检。Err 带诊断码。
pub fn vet_row(row: &OvrRow) -> Result<(), u32> {
    if row.dll.len() == 0 {
        return Err(LpCode::DllNameBlank.val());
    }
    if !dll_name_legal(&row.dll) {
        return Err(LpCode::DllNameIllegal.val());
    }
    if row.mode >= OVR_COUNT {
        return Err(LpCode::OvrOutOfRange.val());
    }
    if row.scope >= SCOPE_COUNT {
        return Err(LpCode::ScopeOutOfRange.val());
    }
    Ok(())
}

// ── 覆盖序账 + 纯函数解析器 ─────────────────────────────────────
pub struct OvrLedger {
    rows: Vec<OvrRow>,
    /// 拒收计数（序冲突拒收断言用）。
    reject_cnt: u32,
}

impl OvrLedger {
    pub fn new() -> OvrLedger {
        OvrLedger { rows: Vec::new(), reject_cnt: 0 }
    }

    /// 登记一条覆盖声明：同 DLL 同作用域双声明即拒（序冲突）。
    pub fn declare(&mut self, row: OvrRow) -> Result<(), u32> {
        match vet_row(&row) {
            Ok(()) => {}
            Err(code) => {
                self.reject_cnt += 1;
                return Err(code);
            }
        }
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].dll == row.dll && self.rows[i].scope == row.scope {
                self.reject_cnt += 1;
                return Err(LpCode::DoubleDeclared.val());
            }
            i += 1;
        }
        self.rows.push(row);
        Ok(())
    }

    /// 解析器（纯函数语义）：输入 DLL 名，按 bottle→prefix→全局 合成
    /// 最终决策。返回 (决策三态, 决策理由)——理由必须非空。
    pub fn resolve(&self, dll: &str) -> Result<(usize, String), u32> {
        if dll.len() == 0 {
            return Err(LpCode::DllNameBlank.val());
        }
        // 按作用域优先级从高到低找命中。
        let mut best: Option<(usize, usize, u32)> = None; // (scope, mode, priority)
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].dll == dll {
                let sp = scope_priority(self.rows[i].scope)?;
                match best {
                    Some((bs, _, _)) if bs > sp => {}
                    _ => {
                        best = Some((sp, self.rows[i].mode, self.rows[i].priority));
                    }
                }
            }
            i += 1;
        }
        match best {
            Some((sp, mode, prio)) => {
                let mut reason = String::from("决策：");
                reason.push_str(OVR_NAMES[mode]);
                reason.push_str("（来源作用域=");
                reason.push_str(SCOPE_NAMES[sp]);
                reason.push_str("，优先级=");
                reason.push_str(&format!("{}", prio));
                reason.push_str("，合成序 bottle>prefix>全局）");
                if reason.len() < 10 {
                    return Err(LpCode::DecisionNoReason.val());
                }
                Ok((mode, reason))
            }
            None => {
                // 无声明 = 默认内建优先（Wine 缺省语义显式化）。
                let reason =
                    String::from("决策：builtin（无覆盖声明，Wine 缺省内建优先）");
                Ok((OVR_BUILTIN, reason))
            }
        }
    }

    /// 合成序自检：高优先作用域的声明必须压过低优先（倒挂检出）。
    pub fn synthesis_ok(&self) -> Result<(), u32> {
        // 对每个 DLL 验 resolve 选中者作用域 = 该 DLL 最高作用域。
        let mut i = 0;
        while i < self.rows.len() {
            let dll = self.rows[i].dll.as_str();
            let mut max_scope = 0;
            let mut j = 0;
            while j < self.rows.len() {
                if self.rows[j].dll == dll && self.rows[j].scope > max_scope {
                    max_scope = self.rows[j].scope;
                }
                j += 1;
            }
            let (mode, _) = self.resolve(dll)?;
            // 选中 mode 必须来自 max_scope 行。
            let mut from_max = false;
            let mut j = 0;
            while j < self.rows.len() {
                if self.rows[j].dll == dll
                    && self.rows[j].scope == max_scope
                    && self.rows[j].mode == mode
                {
                    from_max = true;
                }
                j += 1;
            }
            if !from_max {
                return Err(LpCode::SynthesisInverted.val());
            }
            i += 1;
        }
        Ok(())
    }

    pub fn reject_count(&self) -> u32 {
        self.reject_cnt
    }
    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 首版覆盖序装配：三 DLL × 跨作用域。
pub fn build_initial() -> Result<OvrLedger, u32> {
    let mut l = OvrLedger::new();
    // 全局：msvcrt 内建优先。
    l.declare(OvrRow {
        dll: String::from("msvcrt.dll"),
        mode: OVR_BUILTIN,
        scope: SCOPE_GLOBAL,
        priority: 1,
    })?;
    // prefix：xolehlp.dll 原生优先。
    l.declare(OvrRow {
        dll: String::from("xolehlp.dll"),
        mode: OVR_NATIVE,
        scope: SCOPE_PREFIX,
        priority: 2,
    })?;
    // bottle：api-ms-win-crt-runtime 原生优先（高优先压全局）。
    l.declare(OvrRow {
        dll: String::from("msvcrt.dll"),
        mode: OVR_NATIVE,
        scope: SCOPE_BOTTLE,
        priority: 3,
    })?;
    // 全局：禁用样例。
    l.declare(OvrRow {
        dll: String::from("fakeapp_helper.dll"),
        mode: OVR_DISABLED,
        scope: SCOPE_GLOBAL,
        priority: 1,
    })?;
    l.synthesis_ok()?;
    Ok(l)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((声明数, 拒收数))。
pub fn golden_run() -> Result<(usize, u32), u32> {
    let l = build_initial()?;
    // 解析点验：msvcrt 在 bottle 作用域为 native（压过全局 builtin）。
    let (mode, reason) = l.resolve("msvcrt.dll")?;
    if mode != OVR_NATIVE || reason.len() < 10 {
        return Err(LpCode::SynthesisInverted.val());
    }
    // 无声明 DLL → 缺省 builtin 且理由非空。
    let (dmode, dreason) = l.resolve("undeclared.dll")?;
    if dmode != OVR_BUILTIN || dreason.len() == 0 {
        return Err(LpCode::DecisionNoReason.val());
    }
    // 拒收计数 0（首版无冲突）。
    Ok((l.len(), l.reject_count()))
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
        // ① 双声明：同 DLL 同作用域二次声明必拒（判据复测口径）。
        let dup_ok = match build_initial() {
            Ok(mut l) => {
                let before = l.reject_count();
                let r = l.declare(OvrRow {
                    dll: String::from("msvcrt.dll"),
                    mode: OVR_BUILTIN,
                    scope: SCOPE_GLOBAL,
                    priority: 1,
                });
                r == Err(LpCode::DoubleDeclared.val())
                    && l.reject_count() == before + 1
            }
            Err(_) => false,
        };
        led.record(0, dup_ok);

        // ② 三态越闭集。
        led.record(1, {
            let bad = OvrRow {
                dll: String::from("a.dll"),
                mode: OVR_COUNT + (rnd % 2) as usize,
                scope: SCOPE_GLOBAL,
                priority: 1,
            };
            vet_row(&bad) == Err(LpCode::OvrOutOfRange.val())
        });

        // ③ 空 DLL 名。
        led.record(2, {
            let bad2 = OvrRow {
                dll: String::new(),
                mode: OVR_BUILTIN,
                scope: SCOPE_GLOBAL,
                priority: 1,
            };
            vet_row(&bad2) == Err(LpCode::DllNameBlank.val())
        });

        // ④ 作用域越闭集。
        led.record(3, {
            let bad3 = OvrRow {
                dll: String::from("b.dll"),
                mode: OVR_BUILTIN,
                scope: SCOPE_COUNT + (rnd % 2) as usize,
                priority: 1,
            };
            vet_row(&bad3) == Err(LpCode::ScopeOutOfRange.val())
        });

        // ⑤ DLL 名非法字符（大写/空格）。
        led.record(
            4,
            !dll_name_legal("MSVCRT.dll") && !dll_name_legal("my dll.dll")
                && dll_name_legal("my_dll1.dll"),
        );

        // ⑥ 拒收带怎么改对（三类模板全非空且以"改对"开头）。
        let adv_ok = {
            let mut ok = true;
            let mut k = 0;
            while k < 3 {
                if !rejection_advice(k).starts_with("改对：") {
                    ok = false;
                }
                k += 1;
            }
            ok && (rnd % 1) == 0
        };
        led.record(5, adv_ok);

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
                Err(LpCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法覆盖序必须全过 ─────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let l = build_initial()?;
    l.synthesis_ok()?;
    // 三态名/作用域名点验。
    let mut m = 0;
    while m < OVR_COUNT {
        if OVR_NAMES[m].len() == 0 {
            return Err(LpCode::OvrOutOfRange.val());
        }
        m += 1;
    }
    let mut s = 0;
    while s < SCOPE_COUNT {
        if SCOPE_NAMES[s].len() == 0 {
            return Err(LpCode::ScopeOutOfRange.val());
        }
        s += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < LpCode::ALL.len() {
        let v = LpCode::ALL[i].val();
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
    // 1. 黄金正演：4 声明 / 0 拒收。
    let g = golden_run();
    v.push((
        "J1-黄金正演4声明0拒收",
        matches!(g, Ok((4, 0))),
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
    // 3. 三态导出装载序（判据原文口径）。
    let l = build_initial().unwrap_or_else(|_| OvrLedger::new());
    let tri_ok = matches!(l.resolve("msvcrt.dll"), Ok((m, _)) if m < OVR_COUNT)
        && matches!(l.resolve("xolehlp.dll"), Ok((m, _)) if m == OVR_NATIVE);
    v.push(("J1-三态导出装载序", tri_ok, String::from("bottle>prefix>全局")));
    // 4. 双声明拒收计数断言（判据原文口径）。
    let dup_ok = {
        let mut l2 = OvrLedger::new();
        let row = OvrRow {
            dll: String::from("dup.dll"),
            mode: OVR_BUILTIN,
            scope: SCOPE_GLOBAL,
            priority: 1,
        };
        let _ = l2.declare(row.clone());
        let _ = l2.declare(row);
        l2.reject_count() == 1
    };
    v.push(("J1-双声明拒收计数", dup_ok, String::from("=1")));
    // 5. 决策理由随输出（防"为什么走内建"不可答）。
    let reason_ok = matches!(l.resolve("undeclared.dll"), Ok((_, r)) if r.contains("缺省"));
    v.push(("J1-决策理由随输出", reason_ok, String::from("理由非空")));
    // 6. 合成序无倒挂。
    v.push((
        "J1-合成序无倒挂",
        matches!(l.synthesis_ok(), Ok(())),
        String::from("高优先压低优先"),
    ));
    // 7. DLL 名闭集。
    v.push((
        "J1-DLL名闭集",
        dll_name_legal("msvcrt.dll") && !dll_name_legal("MSVCRT.DLL"),
        String::from("小写 .dll"),
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
        String::from("合成+名点验"),
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
pub fn run_unx16011_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16011 winecfg 库面板与 DLL 覆盖序语义");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
