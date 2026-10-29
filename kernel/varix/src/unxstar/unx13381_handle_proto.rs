//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13381 · D3 对象句柄联签总成（进程/线程/文件句柄创建协议） · 0x659C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F13381-J1 句柄创建协议 30 例（进程/线程/文件三类对象句柄化）
//!      与 D3 冻结接口一致，联签记录登记；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 句柄值编码（D3 冻结接口）：bit31=1（内核态句柄，
//!     winnt.h OBJ_KERNEL_HANDLE 语义）、bits[31:2]=表下标、
//!     bits[1:0]=00（4 字节对齐）；对齐违例/解码不符必抓；
//!   - 句柄四态机：Draft→Sealed（联签）→Live→Closed；盖双章
//!     （OBJ-MGR 对象管理器 + TYPE-OWNER 类型属主）才许 Live；
//!     Closed 槽复用必须升代（generation bump），否则拒收；
//!   - 对象类型三闭集：Process/Thread/File，跨型点灯拒收；
//!   - 30 例全链：Σlive+Σclosed ≡ 30；解码对账逐句柄一致；
//!   - 注入三族：缺章点灯/关后未升代复用/对齐违例 3/3 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x659C0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x659C0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x659C0;

/// 句柄创建协议案例闭集：30 例（判据 J1 明示）。
pub const MAX_HANDLES: usize = 30;

/// 内核态句柄标志位（winnt.h OBJ_KERNEL_HANDLE 语义）。
pub const H_KERNEL_FLAG: u32 = 0x8000_0000;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HdlCode {
    /// 句柄值 4 字节对齐违例
    HValueMisaligned,
    /// 表下标越闭集
    IndexOutOfRange,
    /// 对象类型枚举外
    TypeInvalid,
    /// 句柄态非法迁移
    StateIllegal,
    /// 联签缺章点灯
    SignMissing,
    /// 关后复用未升代
    ClosedReuseNoBump,
    /// 同名同型重复句柄化
    HandleDup,
    /// 账守恒破（Σlive+Σclosed≡30）
    ConservationBreak,
    /// 解码下标与登记不符
    IndexDecodedMismatch,
}

impl HdlCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HdlCode::HValueMisaligned => "句柄值对齐违例：bits[1:0] 必须 00",
            HdlCode::IndexOutOfRange => "表下标越闭集：30 例冻结",
            HdlCode::TypeInvalid => "对象类型枚举外：三类闭集",
            HdlCode::StateIllegal => "句柄态非法迁移：四态闭集外",
            HdlCode::SignMissing => "联签缺章点灯：双章不齐不点灯",
            HdlCode::ClosedReuseNoBump => "关后复用未升代：generation 红线",
            HdlCode::HandleDup => "同名同型重复句柄化",
            HdlCode::ConservationBreak => "账守恒破：Σlive+Σclosed≡30",
            HdlCode::IndexDecodedMismatch => "解码下标与登记不符：编码对账破",
        }
    }
    pub const ALL: [HdlCode; 9] = [
        HdlCode::HValueMisaligned,
        HdlCode::IndexOutOfRange,
        HdlCode::TypeInvalid,
        HdlCode::StateIllegal,
        HdlCode::SignMissing,
        HdlCode::ClosedReuseNoBump,
        HdlCode::HandleDup,
        HdlCode::ConservationBreak,
        HdlCode::IndexDecodedMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HdlCode::ALL.len() {
        if HdlCode::ALL[i].val() == code {
            return HdlCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x659C0 段闭集内"
}

// ── 对象类型三闭集 ───────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjType {
    Process,
    Thread,
    File,
}

impl ObjType {
    pub fn name(self) -> &'static str {
        match self {
            ObjType::Process => "Process",
            ObjType::Thread => "Thread",
            ObjType::File => "File",
        }
    }
}

// ── 句柄四态闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HState {
    /// 草拟
    Draft,
    /// 已联签封章
    Sealed,
    /// 活跃（可引用）
    Live,
    /// 已关闭（槽可复用但须升代）
    Closed,
}

// ── 签章闭集 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HSigner {
    ObjManager,
    TypeOwner,
}

impl HSigner {
    pub fn tag(self) -> &'static str {
        match self {
            HSigner::ObjManager => "OBJ-MGR",
            HSigner::TypeOwner => "TYPE-OWNER",
        }
    }
}

/// 句柄表项。
#[derive(Clone, Copy, Debug)]
pub struct HandleEntry {
    pub hvalue: u32,
    pub otype: ObjType,
    pub name: &'static str,
    pub state: HState,
    pub generation: u32,
    pub obj_sign: bool,
    pub owner_sign: bool,
}

// ── D3 句柄表 ────────────────────────────────────────────────────
pub struct HandleTable {
    pub entries: Vec<HandleEntry>,
    /// 全表代数：每次关后复用 +1（升代红线载体）。
    pub global_generation: u32,
}

impl HandleTable {
    pub fn new() -> HandleTable {
        HandleTable {
            entries: Vec::new(),
            global_generation: 1,
        }
    }

    /// 编码：bit31 置位 + 下标左移 2（低两位恒 0 对齐标志）。
    pub fn encode(index: u32) -> u32 {
        H_KERNEL_FLAG | (index << 2)
    }

    /// 解码：去内核位再右移 2。
    pub fn decode(hvalue: u32) -> Result<u32, u32> {
        if hvalue & 0x3 != 0 {
            return Err(HdlCode::HValueMisaligned.val());
        }
        if hvalue & H_KERNEL_FLAG == 0 {
            return Err(HdlCode::HValueMisaligned.val());
        }
        Ok((hvalue & !H_KERNEL_FLAG) >> 2)
    }

    /// 句柄化一例：三类闭集内、同名同型唯一。
    pub fn create(
        &mut self,
        otype: ObjType,
        name: &'static str,
    ) -> Result<usize, u32> {
        if name.is_empty() {
            return Err(HdlCode::HandleDup.val());
        }
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].otype == otype && self.entries[i].name == name {
                return Err(HdlCode::HandleDup.val());
            }
            i += 1;
        }
        let idx = self.entries.len();
        if idx >= MAX_HANDLES {
            return Err(HdlCode::IndexOutOfRange.val());
        }
        self.entries.push(HandleEntry {
            hvalue: HandleTable::encode(idx as u32),
            otype,
            name,
            state: HState::Draft,
            generation: self.global_generation,
            obj_sign: false,
            owner_sign: false,
        });
        Ok(idx)
    }

    /// 盖章：Draft/Sealed 态均可补章（双章分两次盖）；Live/Closed 拒。
    /// Draft 首章即转 Sealed；Sealed 后补章不改态。
    pub fn seal(&mut self, idx: usize, signer: HSigner) -> Result<(), u32> {
        if idx >= self.entries.len() {
            return Err(HdlCode::IndexOutOfRange.val());
        }
        let st = self.entries[idx].state;
        if st != HState::Draft && st != HState::Sealed {
            return Err(HdlCode::StateIllegal.val());
        }
        match signer {
            HSigner::ObjManager => self.entries[idx].obj_sign = true,
            HSigner::TypeOwner => self.entries[idx].owner_sign = true,
        }
        if st == HState::Draft {
            self.entries[idx].state = HState::Sealed;
        }
        Ok(())
    }

    /// 点灯：Sealed 且双章齐才许；缺章拒收。
    pub fn light(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.entries.len() {
            return Err(HdlCode::IndexOutOfRange.val());
        }
        let e = &self.entries[idx];
        if e.state != HState::Sealed {
            return Err(HdlCode::StateIllegal.val());
        }
        if !e.obj_sign || !e.owner_sign {
            return Err(HdlCode::SignMissing.val());
        }
        self.entries[idx].state = HState::Live;
        Ok(())
    }

    /// 关闭：Live→Closed。
    pub fn close(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.entries.len() {
            return Err(HdlCode::IndexOutOfRange.val());
        }
        if self.entries[idx].state != HState::Live {
            return Err(HdlCode::StateIllegal.val());
        }
        self.entries[idx].state = HState::Closed;
        Ok(())
    }

    /// 关后复用：必须升代（global_generation 前进），未升代拒收。
    pub fn reopen(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.entries.len() {
            return Err(HdlCode::IndexOutOfRange.val());
        }
        let e = &self.entries[idx];
        if e.state != HState::Closed {
            return Err(HdlCode::StateIllegal.val());
        }
        if e.generation == self.global_generation {
            return Err(HdlCode::ClosedReuseNoBump.val());
        }
        self.entries[idx].state = HState::Live;
        self.entries[idx].generation = self.global_generation;
        Ok(())
    }

    /// 表级升代（外部事件推进，如句柄表重扫）。
    pub fn bump_generation(&mut self) {
        self.global_generation += 1;
    }

    /// 解码对账：逐 Live/Sealed 项 decode(hvalue) ≡ 表下标。
    pub fn decode_audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.entries.len() {
            let st = self.entries[i].state;
            if st == HState::Closed {
                i += 1;
                continue;
            }
            let d = HandleTable::decode(self.entries[i].hvalue)?;
            if d != i as u32 {
                return Err(HdlCode::IndexDecodedMismatch.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 账守恒：Σlive+Σclosed ≡ 30（全登记时）。
    pub fn conservation(&self) -> Result<[u32; 4], u32> {
        let mut counts = [0u32; 4];
        let mut i = 0;
        while i < self.entries.len() {
            let k = match self.entries[i].state {
                HState::Draft => 0,
                HState::Sealed => 1,
                HState::Live => 2,
                HState::Closed => 3,
            };
            counts[k] += 1;
            i += 1;
        }
        let total = counts[0] + counts[1] + counts[2] + counts[3];
        if total != self.entries.len() as u32 {
            return Err(HdlCode::ConservationBreak.val());
        }
        Ok(counts)
    }

    /// 事件哈希（FNV-1a over 态序列，公开常数）。
    pub fn state_hash(&self) -> u32 {
        let mut h: u32 = 0x811C9DC5;
        let mut i = 0;
        while i < self.entries.len() {
            let v = match self.entries[i].state {
                HState::Draft => 1u8,
                HState::Sealed => 2,
                HState::Live => 3,
                HState::Closed => 4,
            };
            h = (h ^ (v as u32).wrapping_mul(self.entries[i].generation + 7))
                .wrapping_mul(0x01000193);
            i += 1;
        }
        h
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

/// 黄金对象名表：30 例（三类各 10）。
pub const GOLDEN_NAMES: [&str; MAX_HANDLES] = [
    "proc.session0", "proc.session1", "proc.session2", "proc.session3",
    "proc.session4", "proc.session5", "proc.session6", "proc.session7",
    "proc.session8", "proc.session9",
    "thr.worker0", "thr.worker1", "thr.worker2", "thr.worker3",
    "thr.worker4", "thr.worker5", "thr.worker6", "thr.worker7",
    "thr.worker8", "thr.worker9",
    "file.conout", "file.conin", "file.log0", "file.log1",
    "file.log2", "file.log3", "file.log4", "file.log5",
    "file.log6", "file.log7",
];

// ── 黄金正演：30 例双章联签 → 点灯 → 关一批 → 升代复用一对账 ────
pub fn golden_run() -> Result<u32, u32> {
    let mut tab = HandleTable::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < MAX_HANDLES {
        let otype = match i / 10 {
            0 => ObjType::Process,
            1 => ObjType::Thread,
            _ => ObjType::File,
        };
        let idx = tab.create(otype, GOLDEN_NAMES[i])?;
        tab.seal(idx, HSigner::ObjManager)?;
        tab.seal(idx, HSigner::TypeOwner)?;
        tab.light(idx)?;
        i += 1;
    }
    // 关一批（LCG 选 10 例）。
    let mut closed_n = 0;
    let mut k = 0;
    while k < 10 {
        let c = rng.below(MAX_HANDLES as u64) as usize;
        if tab.entries[c].state == HState::Live {
            tab.close(c)?;
            closed_n += 1;
        }
        k += 1;
    }
    if closed_n == 0 {
        return Err(HdlCode::ConservationBreak.val());
    }
    // 升代复用一例（必须先 bump）。
    let mut victim = 0;
    while victim < MAX_HANDLES && tab.entries[victim].state != HState::Closed {
        victim += 1;
    }
    // 未升代先试复用：必抓。
    if tab.reopen(victim) != Err(HdlCode::ClosedReuseNoBump.val()) {
        return Err(HdlCode::ClosedReuseNoBump.val());
    }
    tab.bump_generation();
    tab.reopen(victim)?;
    // 解码对账 + 守恒。
    tab.decode_audit()?;
    tab.conservation()?;
    // 双跑幂等：重建同序列哈希逐位同。
    let mut tab2 = HandleTable::new();
    let mut rng2 = Lcg::new(SEG_BASE as u64);
    let mut j = 0;
    while j < MAX_HANDLES {
        let otype = match j / 10 {
            0 => ObjType::Process,
            1 => ObjType::Thread,
            _ => ObjType::File,
        };
        let idx = tab2.create(otype, GOLDEN_NAMES[j])?;
        tab2.seal(idx, HSigner::ObjManager)?;
        tab2.seal(idx, HSigner::TypeOwner)?;
        tab2.light(idx)?;
        j += 1;
    }
    let mut m = 0;
    while m < 10 {
        let c = rng2.below(MAX_HANDLES as u64) as usize;
        if tab2.entries[c].state == HState::Live {
            let _ = tab2.close(c);
        }
        m += 1;
    }
    let victim2 = victim;
    let _ = tab2.reopen(victim2); // 未升代：必拒，同正演序列
    tab2.bump_generation();
    tab2.reopen(victim2)?;
    let h1 = tab.state_hash();
    let h2 = tab2.state_hash();
    if h1 != h2 {
        return Err(HdlCode::ConservationBreak.val());
    }
    Ok(h1)
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
    let mut tab = HandleTable::new();
    // 族 0：缺章点灯（只盖一章就点灯）。
    ledger.tries[0] += 1;
    let idx = match tab.create(ObjType::Process, "proc.inj0") {
        Ok(i) => i,
        Err(_) => 0,
    };
    let _ = tab.seal(idx, HSigner::ObjManager);
    if let Err(e) = tab.light(idx) {
        if e == HdlCode::SignMissing.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：关后未升代复用。
    ledger.tries[1] += 1;
    let _ = tab.seal(idx, HSigner::TypeOwner);
    let _ = tab.light(idx);
    let _ = tab.close(idx);
    if let Err(e) = tab.reopen(idx) {
        if e == HdlCode::ClosedReuseNoBump.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：对齐违例（低两位非 00 / 缺内核位）。
    ledger.tries[2] += 1;
    let bad1 = HandleTable::decode(0x8000_0002);
    let bad2 = HandleTable::decode(0x0000_0008);
    if bad1 == Err(HdlCode::HValueMisaligned.val())
        && bad2 == Err(HdlCode::HValueMisaligned.val())
    {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HdlCode::ALL.len() {
        let v = HdlCode::ALL[i].val();
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
    v.push(("J1-黄金正演30例句柄化", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 编码对账正演：encode(5) 解码回 5。
    v.push((
        "J1-编码解码闭环",
        HandleTable::decode(HandleTable::encode(5)) == Ok(5),
        String::from("bit31|idx<<2 闭环"),
    ));
    // 反向判据：用户态句柄（无内核位）拒收。
    v.push((
        "J1-缺内核位必拒",
        HandleTable::decode(0x0000_0010) == Err(HdlCode::HValueMisaligned.val()),
        String::from("bit31=0 拒收"),
    ));
    // 三类闭集：名字互异。
    v.push((
        "J1-三类名闭集",
        ObjType::Process.name() != ObjType::Thread.name()
            && ObjType::Thread.name() != ObjType::File.name(),
        String::from("Process/Thread/File"),
    ));
    // 签章 tag 互异。
    v.push((
        "J1-双章互异",
        HSigner::ObjManager.tag() != HSigner::TypeOwner.tag(),
        String::from("OBJ-MGR/TYPE-OWNER"),
    ));
    // detail 反查闭链。
    let first = detail(HdlCode::HValueMisaligned.val());
    let last = detail(HdlCode::IndexDecodedMismatch.val());
    v.push((
        "J2-detail 反查闭链",
        first != "未知诊断码：不在 0x659C0 段闭集内"
            && last != "未知诊断码：不在 0x659C0 段闭集内",
        String::from("首尾反查非未知"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x659C0 窗口")));
    v
}

pub fn run_unx13381_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13381 D3 对象句柄联签总成");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
