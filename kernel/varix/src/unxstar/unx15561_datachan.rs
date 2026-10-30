//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15561 · IDataObject 面谱与数据通道总纲 · 0x67500
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-D5-B19）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15561-J1 面谱成文（GetData/GetDataHere/QueryGetData/
//!     EnumFormatEtc 四核语义 10/10 次），假句柄注入（未取数即返回
//!     成功）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 剪贴格式闭集（CF_TEXT/CF_BITMAP/CF_METAFILEPICT/CF_UNICODETEXT/
//!     CF_HDROP 五格式冻结），越闭集格式拒收；
//!   - 数据介质状态机：Empty → Fetched（已取数）→ Handed（已交付）；
//!     Handed 前必有 Fetched——未取数即交付 = 假句柄注入，如实拒；
//!   - 四核分账：GetData（取数入介质）、GetDataHere（定向取数）、
//!     QueryGetData（格式在册探测）、EnumFormatEtc（格式枚举闭环
//!     Next/Skip/Reset/Clone 语义借 F15581 总纲建模）；
//!   - 守恒恒等式：枚举产出格式数 = 在册格式数；交付数 ≤ 取数数；
//!   - LCG 种子写死 = SEG_BASE（0x67500）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67500，AI-VX3-1030 批13 独占区）。
pub const SEG_BASE: u32 = 0x67500;

// ── 剪贴格式闭集 ─────────────────────────────────────────────────
pub const CF_TEXT: usize = 0;
pub const CF_BITMAP: usize = 1;
pub const CF_METAFILEPICT: usize = 2;
pub const CF_UNICODETEXT: usize = 3;
pub const CF_HDROP: usize = 4;
pub const CF_COUNT: usize = 5;
pub const CF_NAMES: [&str; CF_COUNT] = [
    "CF_TEXT", "CF_BITMAP", "CF_METAFILEPICT", "CF_UNICODETEXT", "CF_HDROP",
];

/// 取数方向核闭集。
pub const CORE_GETDATA: usize = 0;
pub const CORE_GETDATAHERE: usize = 1;
pub const CORE_QUERYGETDATA: usize = 2;
pub const CORE_ENUMFORMATETC: usize = 3;
pub const CORE_COUNT: usize = 4;
pub const CORE_NAMES: [&str; CORE_COUNT] =
    ["GetData", "GetDataHere", "QueryGetData", "EnumFormatEtc"];

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DataCode {
    /// 格式越闭集
    FormatOutOfRange,
    /// 取数核越闭集
    CoreOutOfRange,
    /// 假句柄：未取数即交付（如实拒）
    FakeHandle,
    /// 格式不在册（GetData 未注册格式）
    FormatNotRegistered,
    /// 介质非法迁移（Handed→Fetched 回退）
    IllegalMediumState,
    /// 枚举产出与在册不守恒
    EnumConservationBroken,
    /// 枚举越界（Next 请求超余量）
    EnumOverrun,
    /// 定向取数容器未备
    HereContainerMissing,
    /// 假句柄注入漏抓
    FakeHandleLeak,
    /// 双跑不一致
    NotReproducible,
}

impl DataCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DataCode::FormatOutOfRange => "格式越闭集：5 剪贴格式冻结",
            DataCode::CoreOutOfRange => "取数核越闭集：四核冻结",
            DataCode::FakeHandle => "假句柄：未取数即交付，如实拒",
            DataCode::FormatNotRegistered => "格式不在册：GetData 拒",
            DataCode::IllegalMediumState => "介质非法迁移：交付态回退",
            DataCode::EnumConservationBroken => "枚举产出与在册不守恒",
            DataCode::EnumOverrun => "枚举越界：Next 请求超余量",
            DataCode::HereContainerMissing => "定向取数容器未备：GetDataHere 拒",
            DataCode::FakeHandleLeak => "假句柄注入漏抓：检出 10/10 红线",
            DataCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [DataCode; 10] = [
        DataCode::FormatOutOfRange,
        DataCode::CoreOutOfRange,
        DataCode::FakeHandle,
        DataCode::FormatNotRegistered,
        DataCode::IllegalMediumState,
        DataCode::EnumConservationBroken,
        DataCode::EnumOverrun,
        DataCode::HereContainerMissing,
        DataCode::FakeHandleLeak,
        DataCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DataCode::ALL.len() {
        if DataCode::ALL[i].val() == code {
            return DataCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67500 段闭集内"
}

// ── 介质状态机 ───────────────────────────────────────────────────
/// 三态闭集：Empty → Fetched → Handed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediumState {
    Empty,
    Fetched,
    Handed,
}

/// 合法边：Empty→Fetched（取数）；Fetched→Handed（交付）；
/// Fetched→Fetched（重取幂等）；其余（Empty→Handed 跳步=假句柄、
/// Handed→任何回退）非法。
pub fn medium_can_move(from: MediumState, to: MediumState) -> bool {
    match (from, to) {
        (MediumState::Empty, MediumState::Fetched) => true,
        (MediumState::Fetched, MediumState::Fetched) => true,
        (MediumState::Fetched, MediumState::Handed) => true,
        _ => false,
    }
}

/// 数据介质记录。
#[derive(Clone, Debug)]
pub struct Medium {
    pub fmt: usize,
    pub state: MediumState,
    /// 载荷字节数（取数时写入）。
    pub payload: u32,
}

// ── 数据通道账（IDataObject 建模） ───────────────────────────────
pub struct DataChannel {
    /// 在册格式表（bool 按格式下标）。
    pub registered: [bool; CF_COUNT],
    pub mediums: Vec<Medium>,
    pub fetched_cnt: u32,
    pub handed_cnt: u32,
    pub query_hits: u32,
    pub enum_calls: u32,
}

impl DataChannel {
    pub fn new() -> DataChannel {
        DataChannel {
            registered: [false; CF_COUNT],
            mediums: Vec::new(),
            fetched_cnt: 0,
            handed_cnt: 0,
            query_hits: 0,
            enum_calls: 0,
        }
    }

    fn fmt_ok(f: usize) -> Result<(), u32> {
        if f >= CF_COUNT {
            return Err(DataCode::FormatOutOfRange.val());
        }
        Ok(())
    }

    /// 注册格式（SetData 建模）。
    pub fn register(&mut self, fmt: usize) -> Result<(), u32> {
        Self::fmt_ok(fmt)?;
        self.registered[fmt] = true;
        Ok(())
    }

    /// QueryGetData：格式在册探测（不在册≠错误，返回布尔语义）。
    pub fn query_get_data(&mut self, fmt: usize) -> Result<bool, u32> {
        Self::fmt_ok(fmt)?;
        self.enum_calls += 1;
        if self.registered[fmt] {
            self.query_hits += 1;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 找介质（按格式）。
    fn find_medium(&self, fmt: usize) -> Option<usize> {
        let mut i = 0;
        while i < self.mediums.len() {
            if self.mediums[i].fmt == fmt {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// GetData：取数入介质（Empty→Fetched）。
    pub fn get_data(&mut self, fmt: usize) -> Result<u32, u32> {
        Self::fmt_ok(fmt)?;
        if !self.registered[fmt] {
            return Err(DataCode::FormatNotRegistered.val());
        }
        match self.find_medium(fmt) {
            Some(i) => {
                let st = self.mediums[i].state;
                if !medium_can_move(st, MediumState::Fetched) {
                    return Err(DataCode::IllegalMediumState.val());
                }
                self.mediums[i].state = MediumState::Fetched;
            }
            None => {
                self.mediums.push(Medium {
                    fmt,
                    state: MediumState::Fetched,
                    payload: 16 + (fmt as u32) * 8,
                });
            }
        }
        self.fetched_cnt += 1;
        Ok(16 + (fmt as u32) * 8)
    }

    /// GetDataHere：定向取数（容器必须已 Fetch 同格式）。
    pub fn get_data_here(&mut self, fmt: usize) -> Result<u32, u32> {
        Self::fmt_ok(fmt)?;
        if !self.registered[fmt] {
            return Err(DataCode::FormatNotRegistered.val());
        }
        match self.find_medium(fmt) {
            Some(i) if self.mediums[i].state != MediumState::Empty => {
                let st = self.mediums[i].state;
                if !medium_can_move(st, MediumState::Fetched) {
                    return Err(DataCode::IllegalMediumState.val());
                }
                self.mediums[i].state = MediumState::Fetched;
                self.fetched_cnt += 1;
                Ok(self.mediums[i].payload)
            }
            _ => Err(DataCode::HereContainerMissing.val()),
        }
    }

    /// 交付（半通道建模：SetData/出去通道）——Fetched 才可 Handed。
    pub fn hand_over(&mut self, fmt: usize) -> Result<(), u32> {
        Self::fmt_ok(fmt)?;
        match self.find_medium(fmt) {
            Some(i) => {
                let st = self.mediums[i].state;
                if !medium_can_move(st, MediumState::Handed) {
                    // Empty 直接交付 = 假句柄注入。
                    return Err(DataCode::FakeHandle.val());
                }
                self.mediums[i].state = MediumState::Handed;
                self.handed_cnt += 1;
                Ok(())
            }
            None => Err(DataCode::FakeHandle.val()),
        }
    }

    /// EnumFormatEtc：产出在册格式下标闭集。
    pub fn enum_formats(&mut self) -> Result<Vec<usize>, u32> {
        self.enum_calls += 1;
        let mut v = Vec::new();
        let mut f = 0;
        while f < CF_COUNT {
            if self.registered[f] {
                v.push(f);
            }
            f += 1;
        }
        Ok(v)
    }

    /// 守恒：枚举产出数 = 在册数；交付数 ≤ 取数数。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut reg = 0;
        let mut f = 0;
        while f < CF_COUNT {
            if self.registered[f] {
                reg += 1;
            }
            f += 1;
        }
        let ev = self.enum_formats_probe();
        if ev.len() as u32 != reg {
            return Err(DataCode::EnumConservationBroken.val());
        }
        if self.handed_cnt > self.fetched_cnt {
            return Err(DataCode::FakeHandle.val());
        }
        Ok(())
    }

    /// 无副作用枚举探测（守恒对账用）。
    fn enum_formats_probe(&self) -> Vec<usize> {
        let mut v = Vec::new();
        let mut f = 0;
        while f < CF_COUNT {
            if self.registered[f] {
                v.push(f);
            }
            f += 1;
        }
        v
    }
}

// ── 枚举器（EnumFormatEtc 四法建模） ─────────────────────────────
pub struct FmtEnum {
    pub items: Vec<usize>,
    pub pos: usize,
}

impl FmtEnum {
    pub fn new(items: Vec<usize>) -> FmtEnum {
        FmtEnum { items, pos: 0 }
    }
    /// Next：越界如实返回实取数（0..=余量）。
    pub fn next_n(&mut self, want: usize) -> Result<Vec<usize>, u32> {
        let remain = self.items.len() - self.pos;
        let take = if want > remain { remain } else { want };
        let mut v = Vec::new();
        let mut k = 0;
        while k < take {
            v.push(self.items[self.pos + k]);
            k += 1;
        }
        self.pos += take;
        Ok(v)
    }
    pub fn skip(&mut self, n: usize) -> Result<usize, u32> {
        let remain = self.items.len() - self.pos;
        let s = if n > remain { remain } else { n };
        self.pos += s;
        Ok(s)
    }
    pub fn reset(&mut self) {
        self.pos = 0;
    }
    /// Clone：位置快照复制。
    pub fn clone_at(&self) -> FmtEnum {
        FmtEnum { items: self.items.clone(), pos: self.pos }
    }
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ──────────────────────────
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

// ── 四核语义 10/10 轮 ────────────────────────────────────────────
pub const GOLDEN_ROUNDS: u32 = 10;

pub fn judge_core_rounds() -> Result<(u32, u32, u32, u32), u32> {
    let mut get_ok = 0;
    let mut here_ok = 0;
    let mut query_ok = 0;
    let mut enum_ok = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut ch = DataChannel::new();
        let f = r as usize % CF_COUNT;
        ch.register(f)?;
        // QueryGetData 命中。
        if ch.query_get_data(f)? {
            query_ok += 1;
        }
        // GetData 取数。
        let p = ch.get_data(f)?;
        if p > 0 {
            get_ok += 1;
        }
        // GetDataHere 定向取数（容器已备）。
        if ch.get_data_here(f).is_ok() {
            here_ok += 1;
        }
        // EnumFormatEtc 产出=1。
        let ev = ch.enum_formats()?;
        if ev.len() == 1 && ev[0] == f {
            enum_ok += 1;
        }
        // 交付闭环。
        ch.hand_over(f)?;
        ch.conserve()?;
        r += 1;
    }
    Ok((get_ok, here_ok, query_ok, enum_ok))
}

// ── 黄金正演（判据常量独立写死） ─────────────────────────────────
pub const GOLDEN_CORE_OK: u32 = 10;
pub const GOLDEN_FAKE_CAUGHT: u32 = 10;

pub fn golden_run() -> Result<(u32, u32, u32, u32, u32), u32> {
    let (g, h, q, e) = judge_core_rounds()?;
    if g != GOLDEN_CORE_OK || h != GOLDEN_CORE_OK || q != GOLDEN_CORE_OK || e != GOLDEN_CORE_OK {
        return Err(DataCode::EnumConservationBroken.val());
    }
    // 假句柄注入 10/10：未取数直接交付必拒。
    let mut caught = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut ch = DataChannel::new();
        ch.register(CF_TEXT).ok();
        if let Err(e) = ch.hand_over(CF_TEXT) {
            if e == DataCode::FakeHandle.val() {
                caught += 1;
            }
        }
        r += 1;
    }
    if caught != GOLDEN_FAKE_CAUGHT {
        return Err(DataCode::FakeHandleLeak.val());
    }
    Ok((g, h, q, e, caught))
}

// ── 注入面：三族 10/10 全抓 ──────────────────────────────────────
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
    // 族 0：假句柄注入 ×10 必拒。
    ledger.tries[0] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut ch = DataChannel::new();
        ch.register(CF_HDROP).ok();
        if let Err(e) = ch.hand_over(CF_HDROP) {
            if e == DataCode::FakeHandle.val() {
                ledger.caught[0] += 1;
            }
        }
        r += 1;
    }
    // 族 1：格式越闭集注入 ×10 必拒。
    ledger.tries[1] += JUDGE_ROUNDS;
    let mut ch1 = DataChannel::new();
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        if let Err(e) = ch1.get_data(CF_COUNT + 1) {
            if e == DataCode::FormatOutOfRange.val() {
                ledger.caught[1] += 1;
            }
        }
        r += 1;
    }
    // 族 2：定向取数容器未备注入 ×10 必拒。
    ledger.tries[2] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut ch2 = DataChannel::new();
        ch2.register(CF_BITMAP).ok();
        if let Err(e) = ch2.get_data_here(CF_BITMAP) {
            if e == DataCode::HereContainerMissing.val() {
                ledger.caught[2] += 1;
            }
        }
        r += 1;
    }
}

// ── 枚举器越界面：Next 超余量如实返回实取数 ─────────────────────
pub fn enum_overrun_ok() -> Result<bool, u32> {
    let mut items = Vec::new();
    items.push(CF_TEXT);
    items.push(CF_BITMAP);
    let mut en = FmtEnum::new(items);
    // 请求 5，实取 2（如实返回实取数=2）。
    let got = en.next_n(5)?;
    let ok_a = got.len() == 2;
    // 再请求 5，实取 0。
    let got2 = en.next_n(5)?;
    let ok_b = got2.len() == 0;
    // Reset 后 Skip 1，Clone 位置保持。
    en.reset();
    let _ = en.skip(1)?;
    let c = en.clone_at();
    let ok_c = c.pos == 1 && en.pos == 1;
    Ok(ok_a && ok_b && ok_c)
}

// ── 反向判据：合法四核通道必须全过 ───────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let mut ch = DataChannel::new();
    let mut f = 0;
    while f < CF_COUNT {
        ch.register(f)?;
        f += 1;
    }
    ch.get_data(CF_TEXT)?;
    ch.hand_over(CF_TEXT)?;
    let ev = ch.enum_formats()?;
    if ev.len() != CF_COUNT {
        return Err(DataCode::EnumConservationBroken.val());
    }
    ch.conserve()?;
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DataCode::ALL.len() {
        let v = DataCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥6 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：四核 10/10 + 假句柄 10/10 检出。
    let g = golden_run();
    v.push((
        "J1-四核10/10+假句柄10/10检出",
        matches!(g, Ok((10, 10, 10, 10, 10))),
        format!("{:?}", g),
    ));
    // 2. 注入三族全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 3. 介质边闭集：Empty→Handed 跳步恒非法（假句柄的边层证明）。
    let closed = !medium_can_move(MediumState::Empty, MediumState::Handed)
        && !medium_can_move(MediumState::Handed, MediumState::Fetched);
    v.push(("J1-介质边闭集", closed, String::from("E→F→H 闭集")));
    // 4. 枚举器越界面：Next 超余量如实返回实取数。
    v.push((
        "J1-枚举越界如实实取数",
        matches!(enum_overrun_ok(), Ok(true)),
        String::from("Next/Skip/Reset/Clone"),
    ));
    // 5. 反向判据：合法通道必须全过。
    v.push(("J1-反向探针合法通道", matches!(reverse_probe(), Ok(())), String::from("全格式在册全过")));
    // 6. 格式名点验：5 格式非空名。
    let names_ok = {
        let mut ok = true;
        let mut f = 0;
        while f < CF_COUNT {
            if CF_NAMES[f].len() == 0 {
                ok = false;
            }
            f += 1;
        }
        ok
    };
    v.push(("J1-五格式名点验", names_ok, format!("{} 格式", CF_COUNT)));
    // 7. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0x11);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 8. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67500 窗口")));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx15561_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15561 IDataObject 面谱与数据通道总纲");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
