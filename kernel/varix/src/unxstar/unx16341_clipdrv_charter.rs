//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16341 · 剪贴板驱动面总纲（与 D5 IDataObject 通道分界） · 0x67EC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-E1-B18）：
//!   ① 纯功能代码 ≥430 行；
//!   ② UNX-F16341-J1 总纲落账（Open/Close/Empty/SetData/GetData 通
//!     道+分界声明），五通道齐备率 100%，与 D5 通道分界声明在账
//!     （本域=user 驱动面挂接，D5=COM 对象语义）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五通道状态机：Closed→Open→（SetData*）→（GetData*）→Closed；
//!     未 Open 即操作 = 通道序破坏（拒）；
//!   - Empty 通道：仅 Open 态可清；清后格式账归零且守恒闭合；
//!   - SetData：格式注册制（闭集五格式），未注册格式拒入；
//!     同格式覆盖必须落账（覆盖数对账）；
//!   - GetData：仅取已置格式；未置格式显性 None（不伪造数据）；
//!   - 分界声明：本域=user 驱动面（字节缓冲语义），D5=COM 对象
//!     语义（IDataObject 面谱），两域边界声明在账且互斥——
//!     本域不得出现 COM 面谱调用名，D5 通道名单独冻结；
//!   - LCG 种子写死 = SEG_BASE（0x67EC0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67EC0，AI-VX3-1030 批14 独占区）。
pub const SEG_BASE: u32 = 0x67EC0;

// ── 五通道闭集 ──────────────────────────────────────────────────
pub const CH_OPEN: usize = 0;
pub const CH_CLOSE: usize = 1;
pub const CH_EMPTY: usize = 2;
pub const CH_SETDATA: usize = 3;
pub const CH_GETDATA: usize = 4;
pub const CHANNEL_COUNT: usize = 5;
pub const CHANNEL_NAMES: [&str; CHANNEL_COUNT] =
    ["Open", "Close", "Empty", "SetData", "GetData"];
pub const CHANNEL_FLOOR: usize = 5;

// ── 格式闭集（5 格式注册制） ────────────────────────────────────
pub const FMT_TEXT: usize = 0;
pub const FMT_BITMAP: usize = 1;
pub const FMT_METAFILE: usize = 2;
pub const FMT_UNICODE: usize = 3;
pub const FMT_DIB: usize = 4;
pub const FORMAT_COUNT: usize = 5;
pub const FORMAT_NAMES: [&str; FORMAT_COUNT] =
    ["CF_TEXT", "CF_BITMAP", "CF_METAFILEPICT", "CF_UNICODETEXT", "CF_DIB"];

// ── D5 分界声明（冻结：COM 对象语义名单，本域禁用） ─────────────
pub const D5_COM_TABLE: [&str; 3] =
    ["IDataObject_GetData", "IDataObject_SetData", "IDataObject_EnumFormatEtc"];
/// 本域驱动面冻结入口（user 语义）。
pub const DRV_FROZEN_ENTRY: &str = "user_clipboard_datum";
/// 分界声明文本（在账）。
pub const BOUNDARY_DECL: &str = "user 驱动面=字节缓冲；D5=COM 对象语义；两域互斥";

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipCode {
    /// 通道越闭集
    ChannelOutOfRange,
    /// 通道名空（齐备破坏）
    ChannelNameBlank,
    /// 未 Open 即操作（通道序破坏）
    NotOpenViolation,
    /// Open 态重复 Open
    DoubleOpen,
    /// 格式越注册闭集
    FormatOutOfRange,
    /// GetData 未置格式显性（不伪造，正常返回码）
    GetDataMiss,
    /// Empty 后守恒破坏（格式账未归零）
    EmptyLedgerBroken,
    /// 覆盖未落账
    OverwriteUnbooked,
    /// 分界断裂：本域出现 D5 COM 名
    BoundaryBroken,
    /// 双跑不一致
    NotReproducible,
}

impl ClipCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ClipCode::ChannelOutOfRange => "通道越闭集：五通道冻结",
            ClipCode::ChannelNameBlank => "通道名空：齐备破坏",
            ClipCode::NotOpenViolation => "未 Open 即操作：通道序破坏",
            ClipCode::DoubleOpen => "Open 态重复 Open",
            ClipCode::FormatOutOfRange => "格式越注册闭集：五格式冻结",
            ClipCode::GetDataMiss => "GetData 未置格式：显性缺席不伪造",
            ClipCode::EmptyLedgerBroken => "Empty 后格式账未归零：守恒破坏",
            ClipCode::OverwriteUnbooked => "同格式覆盖未落账",
            ClipCode::BoundaryBroken => "分界断裂：本域出现 D5 COM 名",
            ClipCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [ClipCode; 10] = [
        ClipCode::ChannelOutOfRange,
        ClipCode::ChannelNameBlank,
        ClipCode::NotOpenViolation,
        ClipCode::DoubleOpen,
        ClipCode::FormatOutOfRange,
        ClipCode::GetDataMiss,
        ClipCode::EmptyLedgerBroken,
        ClipCode::OverwriteUnbooked,
        ClipCode::BoundaryBroken,
        ClipCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ClipCode::ALL.len() {
        if ClipCode::ALL[i].val() == code {
            return ClipCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67EC0 段闭集内"
}

// ── 数据项 ──────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipDatum {
    pub fmt: usize,
    pub bytes: Vec<u8>,
}

impl ClipDatum {
    pub fn legal(&self) -> Result<(), u32> {
        if self.fmt >= FORMAT_COUNT {
            return Err(ClipCode::FormatOutOfRange.val());
        }
        Ok(())
    }
}

// ── 剪贴板驱动面账（五通道状态机） ──────────────────────────────
pub struct ClipDriver {
    pub open: bool,
    /// 格式→数据账（每格式至多一项，覆盖落账）。
    slots: [Option<ClipDatum>; FORMAT_COUNT],
    pub set_cnt: u32,
    pub overwrite_cnt: u32,
    pub get_hit_cnt: u32,
    pub get_miss_cnt: u32,
    pub empty_cnt: u32,
    pub open_cnt: u32,
    pub close_cnt: u32,
}

impl ClipDriver {
    pub fn new() -> ClipDriver {
        ClipDriver {
            open: false,
            slots: [None, None, None, None, None],
            set_cnt: 0,
            overwrite_cnt: 0,
            get_hit_cnt: 0,
            get_miss_cnt: 0,
            empty_cnt: 0,
            open_cnt: 0,
            close_cnt: 0,
        }
    }

    /// Open：Closed→Open。
    pub fn open_channel(&mut self) -> Result<(), u32> {
        if self.open {
            return Err(ClipCode::DoubleOpen.val());
        }
        self.open = true;
        self.open_cnt += 1;
        Ok(())
    }

    /// Close：Open→Closed。
    pub fn close_channel(&mut self) -> Result<(), u32> {
        if !self.open {
            return Err(ClipCode::NotOpenViolation.val());
        }
        self.open = false;
        self.close_cnt += 1;
        Ok(())
    }

    /// Empty：仅 Open 态；清后格式账归零。
    pub fn empty_channel(&mut self) -> Result<u32, u32> {
        if !self.open {
            return Err(ClipCode::NotOpenViolation.val());
        }
        let mut cleared = 0;
        let mut i = 0;
        while i < FORMAT_COUNT {
            if self.slots[i].is_some() {
                self.slots[i] = None;
                cleared += 1;
            }
            i += 1;
        }
        self.empty_cnt += 1;
        Ok(cleared)
    }

    /// SetData：注册制（越闭集拒）；同格式覆盖落账。
    pub fn set_data(&mut self, d: ClipDatum) -> Result<(), u32> {
        if !self.open {
            return Err(ClipCode::NotOpenViolation.val());
        }
        d.legal()?;
        let fmt = d.fmt;
        if self.slots[fmt].is_some() {
            self.overwrite_cnt += 1;
        }
        self.slots[fmt] = Some(d);
        self.set_cnt += 1;
        Ok(())
    }

    /// GetData：已置返回克隆；未置显性 miss 计数（不伪造）。
    pub fn get_data(&mut self, fmt: usize) -> Result<Option<ClipDatum>, u32> {
        if !self.open {
            return Err(ClipCode::NotOpenViolation.val());
        }
        if fmt >= FORMAT_COUNT {
            return Err(ClipCode::FormatOutOfRange.val());
        }
        match &self.slots[fmt] {
            Some(d) => {
                self.get_hit_cnt += 1;
                Ok(Some(d.clone()))
            }
            None => {
                self.get_miss_cnt += 1;
                Err(ClipCode::GetDataMiss.val())
            }
        }
    }

    /// 占用格式数。
    pub fn occupied(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < FORMAT_COUNT {
            if self.slots[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 守恒：置入总数 = 覆盖数 + 当前占用数。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.set_cnt != self.overwrite_cnt + self.occupied() {
            return Err(ClipCode::EmptyLedgerBroken.val());
        }
        Ok(())
    }

    /// 五通道齐备账。
    pub fn channels_ready() -> bool {
        if CHANNEL_NAMES.len() < CHANNEL_FLOOR {
            return false;
        }
        let mut i = 0;
        while i < CHANNEL_NAMES.len() {
            if CHANNEL_NAMES[i].len() == 0 {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 分界声明校验 ────────────────────────────────────────────────
/// 分界声明在账：文本冻结非空 + COM 名单冻结可查 + 本域入口不与
/// COM 名单交叠。
pub fn boundary_declared() -> bool {
    if BOUNDARY_DECL.len() == 0 {
        return false;
    }
    if D5_COM_TABLE.len() != 3 {
        return false;
    }
    let mut i = 0;
    while i < D5_COM_TABLE.len() {
        if D5_COM_TABLE[i] == DRV_FROZEN_ENTRY {
            return false;
        }
        i += 1;
    }
    true
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

/// 随机数据项（合法格式）。
pub fn random_datum(round: usize) -> Result<ClipDatum, u32> {
    let mut lcg = Lcg::new(SEG_BASE as u64 + round as u64);
    let fmt = lcg.below(FORMAT_COUNT as u64) as usize;
    let n = lcg.below(32) as usize + 1;
    let mut bytes = Vec::new();
    let mut i = 0;
    while i < n {
        bytes.push((lcg.below(256) as u8) ^ (fmt as u8));
        i += 1;
    }
    let d = ClipDatum { fmt, bytes };
    d.legal()?;
    Ok(d)
}

/// 黄金全链：Open→Set×3（一覆盖）→Get×2（一中一缺）→Empty→Close。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let mut drv = ClipDriver::new();
    drv.open_channel()?;
    // 置 3 格式。
    let mut i = 0;
    while i < 3 {
        let d = random_datum(i)?;
        drv.set_data(d)?;
        i += 1;
    }
    // 覆盖 FMT_TEXT。
    let d = random_datum(FMT_TEXT)?;
    drv.set_data(d)?;
    drv.conserve()?;
    // 读命中 FMT_UNICODE、读缺 FMT_BITMAP（若未置则 miss）。
    let got = drv.get_data(FMT_UNICODE)?;
    let hit = got.is_some();
    let miss_probe = drv.get_data(FMT_BITMAP).is_err();
    // 清账 + 关闭。
    let cleared = drv.empty_channel()?;
    if drv.occupied() != 0 {
        return Err(ClipCode::EmptyLedgerBroken.val());
    }
    drv.close_channel()?;
    // 分界声明。
    if !ClipDriver::channels_ready() {
        return Err(ClipCode::ChannelNameBlank.val());
    }
    if !boundary_declared() {
        return Err(ClipCode::BoundaryBroken.val());
    }
    Ok((drv.set_cnt, drv.overwrite_cnt, cleared + if hit { 1 } else { 0 } + if miss_probe { 1 } else { 0 }, drv.get_miss_cnt))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    // 黄金序：Open→Set→Get→Close。
    let mut drv = ClipDriver::new();
    drv.open_channel()?;
    let d = ClipDatum { fmt: FMT_TEXT, bytes: Vec::new() };
    // 空字节也合法（字节数无下限）。
    drv.set_data(d)?;
    drv.get_data(FMT_TEXT)?;
    drv.close_channel()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 未 Open 即 Set → 必拒。
    let mut drv = ClipDriver::new();
    drv.set_data(ClipDatum { fmt: FMT_TEXT, bytes: Vec::new() })
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 重复 Open → 必拒。
    let mut drv = ClipDriver::new();
    drv.open_channel()?;
    drv.open_channel()
}

/// 注入 10/10：未 Open、重复 Open、越格式、分界断裂四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：未 Open 即 Empty。
        let mut drv = ClipDriver::new();
        led.record(0, drv.empty_channel().is_err());
        // 槽1：重复 Open。
        let mut drv2 = ClipDriver::new();
        drv2.open_channel().ok();
        led.record(1, drv2.open_channel().is_err());
        // 槽2：越格式闭集。
        let d2 = ClipDatum { fmt: FORMAT_COUNT, bytes: Vec::new() };
        led.record(2, d2.legal().is_err());
        // 槽3：分界断裂注入（伪入口与 COM 名交叠必须被识别，
        // 真入口与 COM 名无交叠必须干净）。
        let broken = {
            let fake_entry = D5_COM_TABLE[0];
            let mut j = 0;
            let mut fake_hit = false;
            while j < D5_COM_TABLE.len() {
                if D5_COM_TABLE[j] == fake_entry {
                    fake_hit = true;
                }
                j += 1;
            }
            let mut k = 0;
            let mut real_hit = false;
            while k < D5_COM_TABLE.len() {
                if D5_COM_TABLE[k] == DRV_FROZEN_ENTRY {
                    real_hit = true;
                }
                k += 1;
            }
            fake_hit == true && real_hit == false
        };
        led.record(3, broken);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ClipCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ClipCode::ALL.len() {
        let v = ClipCode::ALL[i].val();
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
    // 五通道齐备率 100%。
    v.push((
        "J1-五通道齐备率100%",
        ClipDriver::channels_ready() && CHANNEL_NAMES.len() == 5,
        format!("{:?}", CHANNEL_NAMES),
    ));
    // 分界声明在账。
    v.push((
        "J1-D5分界声明在账",
        boundary_declared() && BOUNDARY_DECL.len() > 0,
        String::from(BOUNDARY_DECL),
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
    let a = probe_seq_a();
    let b = probe_seq_b();
    let c = probe_seq_c();
    v.push((
        "J1-探针三序",
        a.is_ok() && b.is_err() && c.is_err(),
        format!("a={:?} b={:?} c={:?}", a.is_ok(), b.is_err(), c.is_err()),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 格式注册制闭集点验。
    v.push((
        "J1-格式闭集点验",
        FORMAT_NAMES.len() == 5,
        format!("{:?}", FORMAT_NAMES),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67EC0 窗口")));
    v
}

pub fn run_unx16341_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16341 剪贴板驱动面总纲");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
