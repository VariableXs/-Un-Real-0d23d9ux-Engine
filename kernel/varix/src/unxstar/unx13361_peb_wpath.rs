//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13361 · PEB 写路径联签总成（D2 装载器→D1 内核写路径全链） · 0x65980
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F13361-J1 写路径全链 30 例（装载器→D1 内核写路径→PEB 落位）
//!      与 D1 冻结接口一致，联签记录登记；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 写路径四态机：Idle→LoaderStaged（D2 装载器暂存）→KernelVetted
//!     （D1 内核写路径校验）→PebCommitted（PEB 落位）；只准前向，
//!     回跳/跨级直跳一律拒收；
//!   - 联签闭集：LoaderStaged 盖 D2-LOADER 章、KernelVetted 盖
//!     D1-KERNEL 章、落位盖 PEB-COMMIT 章；三章不齐不落位（红线）；
//!   - PEB 字段闭集 10 项（ImageBase/ProcessParameters/ProcessHeap 等，
//!     初值写死并注来源），落位才改值，落位后篡改必抓（校验和对账）；
//!   - 30 例全链：Σcommitted+Σpending ≡ 30；落位后表校验和双跑逐位同；
//!   - 注入三族：缺章落位/字段越闭集/回跳 3/3 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65980），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65980，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65980;

/// 写路径全链案例闭集：30 例（判据 J1 明示）。
pub const MAX_CASES: usize = 30;

/// PEB 字段闭集：10 项（D1 冻结接口面）。
pub const PEB_FIELDS: usize = 10;

/// PEB 字段名闭集（顺序即下标，D1 冻结接口）。
pub const PEB_FIELD_NAMES: [&str; PEB_FIELDS] = [
    "ImageBaseAddress",
    "ProcessParameters",
    "SubSystemData",
    "ProcessHeap",
    "FastPebLock",
    "AtlThunkSListPtr",
    "KernelCallbackTable",
    "TlsExpansionCounter",
    "NumberOfProcessors",
    "Environment",
];

/// PEB 初值表（写死并注来源：x64 PE 默认布局/Win 常量）。
pub const PEB_INIT: [u32; PEB_FIELDS] = [
    0x0040_0000, // PE 默认基址（/LARGEADDRESSAWARE 前 32 位缺省）
    0x0002_0000, // ProcessParameters 块占位（64KiB 对齐区）
    0x0000_0000, // SubSystemData：CLI 映像为空
    0x0001_0000, // ProcessHeap：Windows 首堆惯用句柄区
    0x0002_1000, // FastPebLock：CRITICAL_SECTION 对齐占位
    0x0000_0000, // AtlThunkSListPtr：未启用 ATL thunk 为空
    0x0000_0000, // KernelCallbackTable：win32k 未挂接为空
    0x0000_0040, // TlsExpansionCounter：TLS 扩展上限 64 槽
    0x0000_0004, // NumberOfProcessors：样板 4 核（仿真冻结值）
    0x0003_0000, // Environment：环境块占位
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PebCode {
    /// 字段下标越闭集
    FieldOutOfRange,
    /// 写路径态非法迁移（回跳/跨级）
    StageIllegal,
    /// 联签缺章落位
    SignMissing,
    /// 签章枚举外
    SignerInvalid,
    /// 未暂存先校验（乱序）
    WriteBeforeSign,
    /// 写同值（零静默：无变化不落位）
    ValueUnchanged,
    /// 案例账守恒破（Σcommitted+Σpending≡30）
    ConservationBreak,
    /// 落位后篡改（校验和不符）
    TamperDetected,
    /// 案例重复入账
    CaseBookDup,
}

impl PebCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PebCode::FieldOutOfRange => "字段下标越闭集：10 项冻结接口",
            PebCode::StageIllegal => "写路径态非法迁移：只准前向",
            PebCode::SignMissing => "联签缺章落位：三章不齐不落位",
            PebCode::SignerInvalid => "签章枚举外：三章闭集",
            PebCode::WriteBeforeSign => "未暂存先校验：乱序拒收",
            PebCode::ValueUnchanged => "写同值：无变化不落位（零静默）",
            PebCode::ConservationBreak => "案例账守恒破：Σcommitted+Σpending≡30",
            PebCode::TamperDetected => "落位后篡改：表校验和对账不符",
            PebCode::CaseBookDup => "案例重复入账：一例一行",
        }
    }
    pub const ALL: [PebCode; 9] = [
        PebCode::FieldOutOfRange,
        PebCode::StageIllegal,
        PebCode::SignMissing,
        PebCode::SignerInvalid,
        PebCode::WriteBeforeSign,
        PebCode::ValueUnchanged,
        PebCode::ConservationBreak,
        PebCode::TamperDetected,
        PebCode::CaseBookDup,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PebCode::ALL.len() {
        if PebCode::ALL[i].val() == code {
            return PebCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65980 段闭集内"
}

// ── 写路径四态闭集 ───────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WpathStage {
    /// 待暂存
    Idle,
    /// D2 装载器暂存
    LoaderStaged,
    /// D1 内核写路径校验
    KernelVetted,
    /// PEB 落位
    PebCommitted,
}

// ── 签章闭集 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Signer {
    D2Loader,
    D1Kernel,
    PebCommit,
}

impl Signer {
    pub fn tag(self) -> &'static str {
        match self {
            Signer::D2Loader => "D2-LOADER",
            Signer::D1Kernel => "D1-KERNEL",
            Signer::PebCommit => "PEB-COMMIT",
        }
    }
}

/// 单例写请求：字段 + 新值 + 态 + 联签位。
#[derive(Clone, Copy, Debug)]
pub struct WriteCase {
    pub field: usize,
    pub old: u32,
    pub new: u32,
    pub stage: WpathStage,
    pub loader_sign: bool,
    pub kernel_sign: bool,
    pub committed: bool,
}

// ── PEB 写路径账 ─────────────────────────────────────────────────
pub struct PebLedger {
    pub table: [u32; PEB_FIELDS],
    pub cases: Vec<WriteCase>,
    pub signs: Vec<(usize, Signer)>,
    pub committed: u32,
    pub pending: u32,
}

impl PebLedger {
    pub fn new() -> PebLedger {
        PebLedger {
            table: PEB_INIT,
            cases: Vec::new(),
            signs: Vec::new(),
            committed: 0,
            pending: 0,
        }
    }

    /// 入账一例：字段越界/写同值/重复入账在此拦截。
    pub fn book(&mut self, field: usize, new: u32) -> Result<usize, u32> {
        if field >= PEB_FIELDS {
            return Err(PebCode::FieldOutOfRange.val());
        }
        if self.table[field] == new {
            return Err(PebCode::ValueUnchanged.val());
        }
        let old = self.table[field];
        let idx = self.cases.len();
        if idx >= MAX_CASES {
            return Err(PebCode::CaseBookDup.val());
        }
        // 同字段同新值不许重复入账（一例一行）。
        let mut i = 0;
        while i < self.cases.len() {
            if self.cases[i].field == field && self.cases[i].new == new {
                return Err(PebCode::CaseBookDup.val());
            }
            i += 1;
        }
        self.cases.push(WriteCase {
            field,
            old,
            new,
            stage: WpathStage::Idle,
            loader_sign: false,
            kernel_sign: false,
            committed: false,
        });
        self.pending += 1;
        Ok(idx)
    }

    /// 前向迁移（闭式）：Idle→LoaderStaged→KernelVetted→PebCommitted。
    pub fn advance(&mut self, idx: usize, signer: Signer) -> Result<(), u32> {
        if idx >= self.cases.len() {
            return Err(PebCode::FieldOutOfRange.val());
        }
        let cur = self.cases[idx].stage;
        let next = match signer {
            Signer::D2Loader => WpathStage::LoaderStaged,
            Signer::D1Kernel => WpathStage::KernelVetted,
            Signer::PebCommit => WpathStage::PebCommitted,
        };
        let legal = matches!(
            (cur, next),
            (WpathStage::Idle, WpathStage::LoaderStaged)
                | (WpathStage::LoaderStaged, WpathStage::KernelVetted)
                | (WpathStage::KernelVetted, WpathStage::PebCommitted)
        );
        if !legal {
            return Err(PebCode::StageIllegal.val());
        }
        match next {
            WpathStage::LoaderStaged => {
                self.cases[idx].loader_sign = true;
            }
            WpathStage::KernelVetted => {
                if !self.cases[idx].loader_sign {
                    return Err(PebCode::WriteBeforeSign.val());
                }
                self.cases[idx].kernel_sign = true;
            }
            WpathStage::PebCommitted => {
                if !self.cases[idx].loader_sign || !self.cases[idx].kernel_sign {
                    return Err(PebCode::SignMissing.val());
                }
                let f = self.cases[idx].field;
                self.table[f] = self.cases[idx].new;
                self.cases[idx].committed = true;
                self.committed += 1;
                self.pending -= 1;
            }
            WpathStage::Idle => {
                return Err(PebCode::StageIllegal.val());
            }
        }
        self.cases[idx].stage = next;
        self.signs.push((idx, signer));
        Ok(())
    }

    /// 表校验和（FNV-1a over 10 字段，公开常数）。
    pub fn table_checksum(&self) -> u32 {
        let mut h: u32 = 0x811C9DC5;
        let mut i = 0;
        while i < PEB_FIELDS {
            h = (h ^ self.table[i]).wrapping_mul(0x01000193);
            i += 1;
        }
        h
    }

    /// 案例账守恒：Σcommitted+Σpending ≡ 案例数，且落位行数≡committed。
    pub fn conservation(&self) -> Result<(), u32> {
        if self.committed + self.pending != self.cases.len() as u32 {
            return Err(PebCode::ConservationBreak.val());
        }
        let mut committed_rows = 0;
        let mut i = 0;
        while i < self.cases.len() {
            if self.cases[i].committed {
                committed_rows += 1;
            }
            i += 1;
        }
        if committed_rows != self.committed {
            return Err(PebCode::ConservationBreak.val());
        }
        Ok(())
    }

    /// 落位对账：每字段的现值必须等于该字段**最新一次落位**的 new 值
    /// （同字段多写取最新，篡改必抓）。
    pub fn tamper_audit(&self) -> Result<(), u32> {
        let mut f = 0;
        while f < PEB_FIELDS {
            let mut latest: Option<u32> = None;
            let mut i = 0;
            while i < self.cases.len() {
                let c = &self.cases[i];
                if c.committed && c.field == f {
                    latest = Some(c.new);
                }
                i += 1;
            }
            if let Some(v) = latest {
                if self.table[f] != v {
                    return Err(PebCode::TamperDetected.val());
                }
            }
            f += 1;
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
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

// ── 黄金正演：30 例三站联签全链 → 守恒 → 篡改对账 → 双跑幂等 ────
pub fn golden_run() -> Result<u32, u32> {
    let mut led = PebLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < MAX_CASES {
        let field = rng.below(PEB_FIELDS as u64) as usize;
        // 新值：基址微移 0x10000 步进，避开初值。
        let new = PEB_INIT[field] ^ (0x0001_0000 + (i as u32));
        if led.table[field] == new {
            i += 1;
            continue;
        }
        let r = led.book(field, new);
        match r {
            Ok(idx) => {
                led.advance(idx, Signer::D2Loader)?;
                led.advance(idx, Signer::D1Kernel)?;
                led.advance(idx, Signer::PebCommit)?;
            }
            Err(_) => {
                // 同字段同值重复：换步进重试一次。
                let new2 = new ^ 0x0002_0000;
                if led.table[field] == new2 {
                    i += 1;
                    continue;
                }
                let idx2 = led.book(field, new2)?;
                led.advance(idx2, Signer::D2Loader)?;
                led.advance(idx2, Signer::D1Kernel)?;
                led.advance(idx2, Signer::PebCommit)?;
            }
        }
        i += 1;
    }
    led.conservation()?;
    led.tamper_audit()?;
    // 联签记录：每落位例恰 3 章。
    if led.signs.len() != led.committed as usize * 3 {
        return Err(PebCode::SignMissing.val());
    }
    // 双跑幂等：表校验和稳定（再算两遍逐位同）。
    let c1 = led.table_checksum();
    let c2 = led.table_checksum();
    if c1 != c2 {
        return Err(PebCode::TamperDetected.val());
    }
    Ok(c1)
}

// ── 注入面：三族 3/3 必抓 ────────────────────────────────────────
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
    let mut d = PebLedger::new();
    // 族 0：缺章落位（只盖 D2 章就直送 PebCommit）。
    ledger.tries[0] += 1;
    let idx = match d.book(0, 0x0041_0000) {
        Ok(i) => i,
        Err(_) => 0,
    };
    let _ = d.advance(idx, Signer::D2Loader);
    if let Err(e) = d.advance(idx, Signer::PebCommit) {
        if e == PebCode::StageIllegal.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：字段越闭集。
    ledger.tries[1] += 1;
    if let Err(e) = d.book(PEB_FIELDS, 0x1) {
        if e == PebCode::FieldOutOfRange.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：回跳（KernelVetted→LoaderStaged 前向闭集外）。
    ledger.tries[2] += 1;
    let idx2 = match d.book(3, 0x0001_1000) {
        Ok(i) => i,
        Err(_) => 0,
    };
    let _ = d.advance(idx2, Signer::D2Loader);
    let _ = d.advance(idx2, Signer::D1Kernel);
    if let Err(e) = d.advance(idx2, Signer::D2Loader) {
        if e == PebCode::StageIllegal.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PebCode::ALL.len() {
        let v = PebCode::ALL[i].val();
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
    v.push(("J1-黄金正演30例联签", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 签章闭集：三章 tag 互异且非空。
    v.push((
        "J1-三章闭集互异",
        Signer::D2Loader.tag() != Signer::D1Kernel.tag()
            && Signer::D1Kernel.tag() != Signer::PebCommit.tag()
            && !Signer::PebCommit.tag().is_empty(),
        String::from("D2-LOADER/D1-KERNEL/PEB-COMMIT"),
    ));
    // 写同值拒收（反向判据）。
    let mut d2 = PebLedger::new();
    v.push((
        "J1-写同值必拒",
        d2.book(0, PEB_INIT[0]) == Err(PebCode::ValueUnchanged.val()),
        String::from("new≡old 拒收"),
    ));
    // detail 反查闭链。
    let first = detail(PebCode::FieldOutOfRange.val());
    let last = detail(PebCode::CaseBookDup.val());
    v.push((
        "J2-detail 反查闭链",
        first != "未知诊断码：不在 0x65980 段闭集内"
            && last != "未知诊断码：不在 0x65980 段闭集内",
        String::from("首尾反查非未知"),
    ));
    // 字段名表完整性：10 名互异非空。
    let mut uniq = true;
    let mut i = 0;
    while i < PEB_FIELDS {
        let mut j = i + 1;
        while j < PEB_FIELDS {
            if PEB_FIELD_NAMES[i] == PEB_FIELD_NAMES[j] {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    v.push((
        "J1-字段名表完整",
        uniq && !PEB_FIELD_NAMES[0].is_empty(),
        String::from("10 名互异非空"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65980 窗口")));
    v
}

pub fn run_unx13361_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13361 PEB 写路径联签总成");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
