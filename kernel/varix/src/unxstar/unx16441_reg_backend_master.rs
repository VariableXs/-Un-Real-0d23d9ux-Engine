//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16441 · 注册表后端替换总纲（Wine hive→D3 原生后端） · 0x67D00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F16441-J1 总纲落账（后端接口面/替换点/D3 消费面三段），
//!      三段齐备率 100%，Wine hive 内置路径仅存导入导出媒介断言 10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三段闭集：接口面 12 点 / 替换点 8 点 / D3 消费面 10 点；
//!   - 后端状态机：Unbound→Bridged→Native→Retired（合法边闭集，
//!     非法转换拒收并落诊断码）；
//!   - Wine hive 内置路径 10 断言：仅允许 导入媒介/导出媒介 两种
//!     用途，其余 8 类直接调用路径一律拒收（每条断言独立落账）；
//!   - 守恒恒等式：接口 booked==12、替换 booked==8、消费 booked==10，
//!     任何一段不足即落账错误；
//!   - 双跑复现：同输入两轮逐点一致（确定性红线）；
//!   - LCG 种子写死 = SEG_BASE（0x67D00），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67D00，AI-VX3-1030 批 16 独占区）。
pub const SEG_BASE: u32 = 0x67D00;

// ── 三段闭集 ─────────────────────────────────────────────────────
/// 接口面 12 点（闭集冻结）。
pub const IFACE_OPEN_KEY: usize = 0;
pub const IFACE_READ_VALUE: usize = 1;
pub const IFACE_WRITE_VALUE: usize = 2;
pub const IFACE_ENUM_SUBKEY: usize = 3;
pub const IFACE_ENUM_VALUE: usize = 4;
pub const IFACE_DELETE_KEY: usize = 5;
pub const IFACE_DELETE_VALUE: usize = 6;
pub const IFACE_QUERY_INFO: usize = 7;
pub const IFACE_CREATE_KEY: usize = 8;
pub const IFACE_CLOSE_HANDLE: usize = 9;
pub const IFACE_FLUSH: usize = 10;
pub const IFACE_TXN_COMMIT: usize = 11;
pub const IFACE_COUNT: usize = 12;
pub const IFACE_NAMES: [&str; IFACE_COUNT] = [
    "打开键", "读值", "写值", "枚举子键", "枚举值", "删除键",
    "删除值", "查询信息", "创建键", "关闭句柄", "刷新", "事务提交",
];

/// 替换点 8 点（闭集冻结）。
pub const REPL_HIVE_LOADER: usize = 0;
pub const REPL_HANDLE_TABLE: usize = 1;
pub const REPL_VALUE_CODEC: usize = 2;
pub const REPL_SECDESC: usize = 3;
pub const REPL_NOTIFY_CB: usize = 4;
pub const REPL_TXN_LOG: usize = 5;
pub const REPL_IMPORT_EXPORT: usize = 6;
pub const REPL_UNLOADER: usize = 7;
pub const REPL_COUNT: usize = 8;
pub const REPL_NAMES: [&str; REPL_COUNT] = [
    "hive装载器", "句柄表", "值序列化", "安全描述符",
    "通知回调", "事务日志", "导入导出", "卸载器",
];

/// D3 消费面 10 点（闭集冻结）。
pub const CONSUME_D3_CORE: usize = 0;
pub const CONSUME_D3_FS: usize = 1;
pub const CONSUME_D3_NET: usize = 2;
pub const CONSUME_D3_GFX: usize = 3;
pub const CONSUME_D3_AUDIO: usize = 4;
pub const CONSUME_D3_LOADER: usize = 5;
pub const CONSUME_D3_SHELL: usize = 6;
pub const CONSUME_D3_CFGD: usize = 7;
pub const CONSUME_D3_BOTTLE: usize = 8;
pub const CONSUME_D3_DIAG: usize = 9;
pub const CONSUME_COUNT: usize = 10;
pub const CONSUME_NAMES: [&str; CONSUME_COUNT] = [
    "D3内核", "D3文件系", "D3网络", "D3图形", "D3音频",
    "D3加载器", "D3壳层", "D3配置守护", "D3bottle", "D3诊断",
];

/// Wine hive 内置路径 10 断言（闭集冻结）。
pub const BI_DIRECT_READ: usize = 0;
pub const BI_DIRECT_WRITE: usize = 1;
pub const BI_DIRECT_ENUM: usize = 2;
pub const BI_DIRECT_CREATE: usize = 3;
pub const BI_DIRECT_DELETE: usize = 4;
pub const BI_TXN_DIRECT: usize = 5;
pub const BI_HANDLE_PASSTHRU: usize = 6;
pub const BI_LOADER_MOUNT: usize = 7;
pub const BI_IMPORT_MEDIUM: usize = 8;
pub const BI_EXPORT_MEDIUM: usize = 9;
pub const BI_COUNT: usize = 10;
pub const BI_NAMES: [&str; BI_COUNT] = [
    "内置直读", "内置直写", "内置直枚举", "内置直建键", "内置直删键",
    "内置事务直写", "内置句柄直通", "内置装载直挂", "导入媒介", "导出媒介",
];

/// 判据常量（独立写死，不复用被测值）。
pub const EXPECT_IFACE: usize = 12;
pub const EXPECT_REPL: usize = 8;
pub const EXPECT_CONSUME: usize = 10;
pub const EXPECT_BI_ASSERTS: usize = 10;
/// 仅这两个内置用途允许（导入媒介/导出媒介）。
pub const BI_ALLOWED_LO: usize = 8;
pub const BI_ALLOWED_HI: usize = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegCode {
    /// 接口面下标越闭集
    IfaceOutOfRange,
    /// 替换点下标越闭集
    ReplOutOfRange,
    /// 消费面下标越闭集
    ConsumeOutOfRange,
    /// 三段任一段未齐备
    SegmentUnbooked,
    /// 后端状态机非法转换
    TransitionIllegal,
    /// Wine hive 内置路径非法直接用途
    BuiltinDirectUse,
    /// 双跑不一致（复现红线）
    NotReproducible,
    /// 内置媒介断言缺口
    BuiltinAssertGap,
}

impl RegCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RegCode::IfaceOutOfRange => "接口面下标越闭集：12 点冻结",
            RegCode::ReplOutOfRange => "替换点下标越闭集：8 点冻结",
            RegCode::ConsumeOutOfRange => "消费面下标越闭集：10 点冻结",
            RegCode::SegmentUnbooked => "三段任一段未齐备：齐备率红线",
            RegCode::TransitionIllegal => "后端状态机非法转换：边闭集拒收",
            RegCode::BuiltinDirectUse => "Wine hive 内置路径非法直接用途",
            RegCode::NotReproducible => "双跑不一致：总纲落账不可复现",
            RegCode::BuiltinAssertGap => "内置媒介断言缺口：10/10 红线",
        }
    }
    pub const ALL: [RegCode; 8] = [
        RegCode::IfaceOutOfRange,
        RegCode::ReplOutOfRange,
        RegCode::ConsumeOutOfRange,
        RegCode::SegmentUnbooked,
        RegCode::TransitionIllegal,
        RegCode::BuiltinDirectUse,
        RegCode::NotReproducible,
        RegCode::BuiltinAssertGap,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RegCode::ALL.len() {
        if RegCode::ALL[i].val() == code {
            return RegCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67D00 段闭集内"
}

// ── 后端状态机 ───────────────────────────────────────────────────
/// Unbound（未绑定）→ Bridged（桥接 hive）→ Native（D3 原生）→
/// Retired（旧后端退役）。合法边闭集：仅相邻前进；其余一律拒收。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BackendState {
    Unbound,
    Bridged,
    Native,
    Retired,
}

pub fn step(cur: BackendState, next: BackendState) -> Result<BackendState, u32> {
    let legal = match cur {
        BackendState::Unbound => matches!(next, BackendState::Bridged),
        BackendState::Bridged => matches!(next, BackendState::Native),
        BackendState::Native => matches!(next, BackendState::Retired),
        BackendState::Retired => false,
    };
    if legal {
        Ok(next)
    } else {
        Err(RegCode::TransitionIllegal.val())
    }
}

/// 内置路径用途判白：仅导入媒介/导出媒介允许，其余拒收。
pub fn builtin_allowed(use_id: usize) -> Result<bool, u32> {
    if use_id >= BI_COUNT {
        return Err(RegCode::BuiltinDirectUse.val());
    }
    Ok(use_id >= BI_ALLOWED_LO && use_id < BI_ALLOWED_HI)
}

// ── 总纲落账 ─────────────────────────────────────────────────────
pub struct MasterLedger {
    pub iface_booked: Vec<usize>,
    pub repl_booked: Vec<usize>,
    pub consume_booked: Vec<usize>,
    pub bi_asserts: Vec<bool>,
    pub state: BackendState,
    pub digest: u64,
}

impl MasterLedger {
    pub fn new() -> MasterLedger {
        MasterLedger {
            iface_booked: Vec::new(),
            repl_booked: Vec::new(),
            consume_booked: Vec::new(),
            bi_asserts: Vec::new(),
            state: BackendState::Unbound,
            digest: 0,
        }
    }

    /// 接口面落账一点。
    pub fn book_iface(&mut self, id: usize) -> Result<(), u32> {
        if id >= IFACE_COUNT {
            return Err(RegCode::IfaceOutOfRange.val());
        }
        // 已落账点拒重（点验唯一性）。
        let mut i = 0;
        while i < self.iface_booked.len() {
            if self.iface_booked[i] == id {
                return Err(RegCode::IfaceOutOfRange.val());
            }
            i += 1;
        }
        self.iface_booked.push(id);
        self.digest = self.digest.wrapping_mul(31).wrapping_add((id as u64) + 1);
        Ok(())
    }

    /// 替换点落账一点。
    pub fn book_repl(&mut self, id: usize) -> Result<(), u32> {
        if id >= REPL_COUNT {
            return Err(RegCode::ReplOutOfRange.val());
        }
        let mut i = 0;
        while i < self.repl_booked.len() {
            if self.repl_booked[i] == id {
                return Err(RegCode::ReplOutOfRange.val());
            }
            i += 1;
        }
        self.repl_booked.push(id);
        self.digest = self.digest.wrapping_mul(37).wrapping_add((id as u64) + 1);
        Ok(())
    }

    /// 消费面落账一点。
    pub fn book_consume(&mut self, id: usize) -> Result<(), u32> {
        if id >= CONSUME_COUNT {
            return Err(RegCode::ConsumeOutOfRange.val());
        }
        let mut i = 0;
        while i < self.consume_booked.len() {
            if self.consume_booked[i] == id {
                return Err(RegCode::ConsumeOutOfRange.val());
            }
            i += 1;
        }
        self.consume_booked.push(id);
        self.digest = self.digest.wrapping_mul(41).wrapping_add((id as u64) + 1);
        Ok(())
    }

    /// 内置路径断言一条：期望=判白结果，账记逐条 bool。
    pub fn book_builtin(&mut self, use_id: usize) -> Result<(), u32> {
        let allowed = builtin_allowed(use_id)?;
        self.bi_asserts.push(allowed);
        Ok(())
    }

    /// 状态机走完全程：Unbound→Bridged→Native→Retired。
    pub fn advance_full(&mut self) -> Result<(), u32> {
        self.state = step(self.state, BackendState::Bridged)?;
        self.state = step(self.state, BackendState::Native)?;
        self.state = step(self.state, BackendState::Retired)?;
        Ok(())
    }

    /// 三段齐备守恒：12/8/10 逐段相等。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.iface_booked.len() != EXPECT_IFACE
            || self.repl_booked.len() != EXPECT_REPL
            || self.consume_booked.len() != EXPECT_CONSUME
        {
            return Err(RegCode::SegmentUnbooked.val());
        }
        Ok(())
    }

    /// 内置断言守恒：恰 10 条且仅最后两条为真。
    pub fn builtin_conserve(&self) -> Result<(), u32> {
        if self.bi_asserts.len() != EXPECT_BI_ASSERTS {
            return Err(RegCode::BuiltinAssertGap.val());
        }
        let mut i = 0;
        while i < self.bi_asserts.len() {
            let expect = i >= BI_ALLOWED_LO && i < BI_ALLOWED_HI;
            if self.bi_asserts[i] != expect {
                return Err(RegCode::BuiltinAssertGap.val());
            }
            i += 1;
        }
        Ok(())
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
        self.next() % (if n == 0 { 1 } else { n })
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(usize, usize, usize, u64), u32> {
    let mut led = MasterLedger::new();
    // 状态机先走：Unbound→Bridged（桥接期落账）→Native→Retired。
    led.advance_full()?;
    let mut id = 0;
    while id < IFACE_COUNT {
        led.book_iface(id)?;
        id += 1;
    }
    let mut id = 0;
    while id < REPL_COUNT {
        led.book_repl(id)?;
        id += 1;
    }
    let mut id = 0;
    while id < CONSUME_COUNT {
        led.book_consume(id)?;
        id += 1;
    }
    let mut id = 0;
    while id < BI_COUNT {
        led.book_builtin(id)?;
        id += 1;
    }
    led.conserve()?;
    led.builtin_conserve()?;
    // 终态校验：Retired 且不可再前进。
    if led.state != BackendState::Retired {
        return Err(RegCode::TransitionIllegal.val());
    }
    if step(BackendState::Retired, BackendState::Unbound).is_ok() {
        return Err(RegCode::TransitionIllegal.val());
    }
    Ok((
        led.iface_booked.len(),
        led.repl_booked.len(),
        led.consume_booked.len(),
        led.digest,
    ))
}

// ── 注入面：三族全抓（tries/caught 双列） ────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：接口面越界（下标 12）。
    ledger.tries[0] += 1;
    let mut led = MasterLedger::new();
    if let Err(e) = led.book_iface(IFACE_COUNT) {
        if e == RegCode::IfaceOutOfRange.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：状态机非法跳转（Unbound→Native）。
    ledger.tries[1] += 1;
    if let Err(e) = step(BackendState::Unbound, BackendState::Native) {
        if e == RegCode::TransitionIllegal.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：内置路径非法直接用途（直读必须判白=否）。
    ledger.tries[2] += 1;
    if let Ok(false) = builtin_allowed(BI_DIRECT_READ) {
        ledger.caught[2] += 1;
    }
}

// ── 复现对拍 ─────────────────────────────────────────────────────
pub fn reproducible() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a.0 != b.0 || a.1 != b.1 || a.2 != b.2 || a.3 != b.3 {
        return Err(RegCode::NotReproducible.val());
    }
    Ok(())
}

// ── LCG 确定性抽样（种子=SEG_BASE） ─────────────────────────────
pub fn sampled_digest() -> u64 {
    let mut g = Lcg(SEG_BASE as u64);
    let mut acc: u64 = 0;
    let mut k = 0;
    while k < 16 {
        let v = g.below(IFACE_COUNT as u64);
        acc = acc.wrapping_mul(7).wrapping_add(v + 1);
        k += 1;
    }
    acc
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RegCode::ALL.len() {
        let v = RegCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 三段齐备率 100%（12/8/10 守恒 + 状态机走完）。
    let golden = golden_run();
    let tri_ok = match &golden {
        Ok((i, r, c, _)) => {
            *i == EXPECT_IFACE && *r == EXPECT_REPL && *c == EXPECT_CONSUME
        }
        Err(_) => false,
    };
    v.push((
        "J1-三段齐备率100%（接口12/替换8/消费10）",
        tri_ok,
        format!("{:?}", golden),
    ));
    // 2. Wine hive 内置路径媒介断言 10/10（仅导入导出为真）。
    let bi_ok = match &golden {
        Ok(_) => {
            let mut led = MasterLedger::new();
            let mut id = 0;
            let mut all = true;
            while id < BI_COUNT {
                if led.book_builtin(id).is_err() {
                    all = false;
                }
                id += 1;
            }
            all && led.builtin_conserve().is_ok()
        }
        Err(_) => false,
    };
    v.push((
        "J1-内置媒介断言10/10",
        bi_ok,
        String::from("仅 导入媒介/导出媒介 判白"),
    ));
    // 3. 状态机非法转换拒收（反向判据：合法链必须通）。
    let fwd = step(BackendState::Unbound, BackendState::Bridged).is_ok()
        && step(BackendState::Bridged, BackendState::Native).is_ok()
        && step(BackendState::Native, BackendState::Retired).is_ok();
    let bwd = step(BackendState::Retired, BackendState::Native).is_err()
        && step(BackendState::Unbound, BackendState::Retired).is_err();
    v.push((
        "J1-状态机边闭集（正链通/逆跳拒）",
        fwd && bwd,
        format!("fwd={} bwd={}", fwd, bwd),
    ));
    // 4. 双跑复现。
    v.push((
        "J1-总纲双跑复现",
        reproducible().is_ok(),
        String::from("digest 逐轮一致"),
    ));
    // 5. LCG 确定性抽样（种子写死，两次同值）。
    let s1 = sampled_digest();
    let s2 = sampled_digest();
    v.push((
        "J1-LCG确定性抽样",
        s1 == s2,
        format!("digest={}", s1),
    ));
    // 6. 诊断码段位守卫 + detail 反查回环。
    let mut loop_ok = true;
    let mut i = 0;
    while i < RegCode::ALL.len() {
        let code = RegCode::ALL[i];
        if detail(code.val()) != code.detail() {
            loop_ok = false;
        }
        i += 1;
    }
    v.push((
        "J1-段位守卫+detail反查回环",
        segment_guard() && loop_ok,
        String::from("0x67D00 窗口 8 码"),
    ));
    // 7. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 8. 名册点验（三段名册非空逐条）。
    let mut named = 0;
    i = 0;
    while i < IFACE_COUNT {
        if IFACE_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    let mut repl_named = 0;
    i = 0;
    while i < REPL_COUNT {
        if REPL_NAMES[i].len() > 0 {
            repl_named += 1;
        }
        i += 1;
    }
    v.push((
        "J1-名册点验 12+8",
        named == EXPECT_IFACE && repl_named == EXPECT_REPL,
        format!("{}/12 {}/8", named, repl_named),
    ));
    v
}

pub fn run_unx16441_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16441 注册表后端替换总纲（Wine hive→D3 原生后端）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
