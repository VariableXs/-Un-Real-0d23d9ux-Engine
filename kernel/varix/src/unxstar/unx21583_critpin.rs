//! UNX-F21583 · 判据 800 枚终钉版（J1 全集自洽）
//!
//! 诊断段 0x91011~0x91018（F2-B40 域收官段 3/100）。
//!
//! 判据原文（施工书 UNX-F21583-J1）：800 枚 J1 编号自洽，判据文本格式统一
//! （抽查 80 例）。
//!
//! 设计要点（为什么不是「逐条 contains 一遍」）：
//! 判据文本是人写的，格式故障有三种互不相同的成因——编号前缀错（贴错条目）、
//! 长度越窗（写太短没判据、写太长夹带说明）、措辞含糊（「大致」「酌情」这类词
//! 让判据无法判定）。三者的修法不同，故格式校验器返回三种不同的码，不做布尔化。
//!
//! 编号自洽不靠「相邻两条差一」：总序跨条目连续，但条目号只在其 20 枚内递增，
//! 跨条目时 J 序回落到 1。故自洽规则由 `CritTag::from_serial` 单独表达，登记时拿
//! 声明序反推标签再与文本前缀比对，两条独立线索互相印证，任一处写错都会分叉。
//!
//! 抽样必须确定性：判据要在任意机器上可重放，「随机抽 80 例」今天绿明天红毫无
//! 意义。故用「步长取模」伪散列——步长 = 总数 / 样本数，每步内按 k×7 取模偏移，
//! 既打散成非连续片段，又保证下标严格递增不重复（偏移量恒小于步长）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 条目号首（施工书 F21581）。
pub const OWNER_FIRST: u32 = 21_581;
/// 每条目判据数（J1~J20）。
pub const PER_OWNER: u8 = 20;
/// 判据总数（40 × 20）。
pub const PIN_TOTAL: usize = 800;
/// 抽样下限（判据原文「抽查 80 例」）。
pub const SAMPLE_MIN: usize = 80;
/// 文本长度窗下界（字符数，非字节：中文一字一符，按字节算会误伤多字节正文）。
pub const TEXT_MIN: usize = 24;
/// 文本长度窗上界。
pub const TEXT_MAX: usize = 120;
/// 禁用模糊词：出现即视为「无法判定」，不是文风问题而是判据失效。
const FORBIDDEN: [&str; 5] = ["待定", "酌情", "大致", "等等", "视情况"];

/// 钉版诊断码闭集（0x91011~0x91018 八码）。
///
/// 显式判别式：码值是跨模块日志对账键，隐式递增会在插入变体时整体漂移。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PinCode {
    Ok = 0x91011,
    BadSerial = 0x91012,
    PrefixMismatch = 0x91013,
    TextShape = 0x91014,
    DuplicatePin = 0x91015,
    Unpinned = 0x91016,
    CapacityOver = 0x91017,
    SampleShort = 0x91018,
}

impl PinCode {
    /// 诊断三要素之一：现象。
    pub const fn what(self) -> &'static str {
        match self {
            PinCode::Ok => "判据终钉通过",
            PinCode::BadSerial => "判据编号不在册或跳号",
            PinCode::PrefixMismatch => "文本前缀与条目号不符",
            PinCode::TextShape => "文本形态不合规",
            PinCode::DuplicatePin => "同一判据被二次钉版",
            PinCode::Unpinned => "存在未钉版判据",
            PinCode::CapacityOver => "判据数超出钉版容量",
            PinCode::SampleShort => "抽样数低于 80 例",
        }
    }
    /// 诊断三要素之二：原因。
    pub const fn why(self) -> &'static str {
        match self {
            PinCode::Ok => "八百枚编号自洽、文本合规、逐枚在位",
            PinCode::BadSerial => "声明序与已登条数不衔接，或序号为 0",
            PinCode::PrefixMismatch => "文本从别条复制而未改 UNX-F 号",
            PinCode::TextShape => "长度越窗、首尾带空白或含模糊词",
            PinCode::DuplicatePin => "同一枚被两次写入钉版标记",
            PinCode::Unpinned => "钉版步骤被跳过或中途失败",
            PinCode::CapacityOver => "条目数已满 800 枚仍继续追加入册",
            PinCode::SampleShort => "抽样参数小于判据书规定的最小例数",
        }
    }
    /// 诊断三要素之三：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            PinCode::Ok => "转入批册落盘核",
            PinCode::BadSerial => "按已登条数重排声明序，0 号直接丢弃",
            PinCode::PrefixMismatch => "把前缀改成该条目的 UNX-F 号与 J 序",
            PinCode::TextShape => "压到长度窗内、去首尾空白、删模糊词",
            PinCode::DuplicatePin => "钉版前先读状态，已钉则跳过",
            PinCode::Unpinned => "按清点结果逐枚补钉",
            PinCode::CapacityOver => "核对条目数与每条目判据数是否被改",
            PinCode::SampleShort => "抽样数取 80 或以上",
        }
    }
    /// 数值形式（u32：五位数码段，`as u16` 会静默截断）。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 判据标签（条目号 + 条内 J 序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CritTag {
    /// 归属条目号。
    pub owner: u32,
    /// 条内 J 序（1-based）。
    pub ordinal: u8,
}

impl CritTag {
    /// 直接构造（判据里用来写期望值，不经换算——换算错时两者才会分叉）。
    pub const fn new(owner: u32, ordinal: u8) -> CritTag {
        CritTag { owner, ordinal }
    }

    /// 由 1-based 总序反推标签。
    ///
    /// 0 号返回全零标签：0 号不是「第 0 枚」，而是漏填，必须能被识别成坏值而不是
    /// 被算成上一条的末枚。
    pub const fn from_serial(serial: u16) -> CritTag {
        if serial == 0 {
            return CritTag { owner: 0, ordinal: 0 };
        }
        let zero = (serial - 1) as u32;
        CritTag {
            owner: OWNER_FIRST + zero / PER_OWNER as u32,
            ordinal: ((zero % PER_OWNER as u32) + 1) as u8,
        }
    }

    /// 文本前缀（`UNX-F<条目号>-J<序>`）。
    pub fn label(&self) -> String {
        let mut s = String::with_capacity(20);
        s.push_str("UNX-F");
        s.push_str(&self.owner.to_string());
        s.push_str("-J");
        s.push_str(&self.ordinal.to_string());
        s
    }
}

/// 钉版故障（定位字段与 ID 闭合模块刻意不同：这里关心「第几枚」与「差多少」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PinFault {
    /// 诊断码（u32）。
    pub code: u32,
    /// 涉事总序（0 表示与具体枚无关）。
    pub serial: u16,
    /// 辅助量（未钉数 / 抽样请求数 / 首个坏枚）。
    pub aux: u16,
}

impl PinFault {
    /// 建故障。
    pub const fn new(code: PinCode, serial: u16, aux: u16) -> PinFault {
        PinFault { code: code.code(), serial, aux }
    }
    /// 码还原（闭集外保守落到 `TextShape`：内容是最后一道闸，宁可重判文本）。
    pub const fn as_code(self) -> PinCode {
        match self.code {
            0x91011 => PinCode::Ok,
            0x91012 => PinCode::BadSerial,
            0x91013 => PinCode::PrefixMismatch,
            0x91014 => PinCode::TextShape,
            0x91015 => PinCode::DuplicatePin,
            0x91016 => PinCode::Unpinned,
            0x91017 => PinCode::CapacityOver,
            _ => PinCode::SampleShort,
        }
    }
    /// 码是否落在闭集内。
    pub const fn is_known(self) -> bool {
        self.code >= 0x91011 && self.code <= 0x91018
    }
    /// 一行可读诊断。
    pub fn describe(self) -> String {
        let mut s = String::with_capacity(80);
        s.push_str(self.as_code().what());
        s.push_str(" serial=");
        s.push_str(&self.serial.to_string());
        s.push_str(" aux=");
        s.push_str(&self.aux.to_string());
        s
    }
}

/// 单枚判据。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PinEntry {
    /// 标签（登记时由声明序反推，不取调用方所传）。
    pub tag: CritTag,
    /// 判据正文。
    pub text: String,
    /// 钉版状态。
    pub pinned: bool,
    /// 文本校验和（FNV-1a 64，链式指纹的叶）。
    pub checksum: u64,
}

impl PinEntry {
    /// 由标签与正文造一枚（不校验正文，校验统一在 `PinSet::admit` 与 `check_text`）。
    fn new(tag: CritTag, text: &str) -> PinEntry {
        PinEntry { tag, text: text.to_string(), pinned: false, checksum: fnv_bytes(text) }
    }
}

/// 抽样回执。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SampleReport {
    /// 实抽例数。
    pub asked: u16,
    /// 通过例数。
    pub passed: u16,
    /// 首个不通过的总序（0 表示无）。
    pub first_bad: u16,
}

/// 判据钉版册。
///
/// 只追加、只翻状态位：正文允许 `revise_text` 改，但改一次校验和与指纹链必须跟着
/// 变——这正是链的意义：单看某一枚的格式查不出「合规但被换过」，只有链能。
pub struct PinSet {
    entries: Vec<PinEntry>,
    /// 钉版成功次数（幂等观测点）。
    pin_ops: u16,
    /// 被拒操作次数（诊断面）。
    rejects: u16,
}

impl PinSet {
    /// 新建空册。
    pub const fn new() -> PinSet {
        PinSet { entries: Vec::new(), pin_ops: 0, rejects: 0 }
    }

    /// 下一枚应得的总序。
    fn next_serial(&self) -> u16 {
        (self.entries.len() as u16).saturating_add(1)
    }

    /// 总序折下标（0 与越界都返回 None）。
    fn index_of(&self, serial: u16) -> Option<usize> {
        if serial == 0 {
            return None;
        }
        let idx = (serial - 1) as usize;
        if idx < self.entries.len() {
            Some(idx)
        } else {
            None
        }
    }

    /// 入册一枚（声明总序 + 正文）。
    ///
    /// 闸序：容量 → 序连续 → 正文。容量最便宜；序连续是「编号自洽」的第一义，
    /// 跳号意味着前面的枚根本没登记，此后再校文本都是在错误的册子上做校订。
    /// 标签由声明序反推而非调用方传入：传标签等于让调用方自证编号，前缀比对就
    /// 失去意义。
    pub fn admit(&mut self, serial: u16, text: &str) -> Result<u16, PinFault> {
        if self.entries.len() >= PIN_TOTAL {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PinFault::new(PinCode::CapacityOver, serial, self.entries.len() as u16));
        }
        if serial != self.next_serial() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PinFault::new(PinCode::BadSerial, serial, self.next_serial()));
        }
        let tag = CritTag::from_serial(serial);
        if let Err(c) = check_text(text, &tag) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PinFault::new(c, serial, 0));
        }
        self.entries.push(PinEntry::new(tag, text));
        Ok(serial)
    }

    /// 已入册枚数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 按总序取一枚（1-based）。
    pub fn entry(&self, serial: u16) -> Option<&PinEntry> {
        match self.index_of(serial) {
            Some(idx) => Some(&self.entries[idx]),
            None => None,
        }
    }

    /// 已钉枚数。
    pub fn pinned_count(&self) -> u16 {
        let mut n = 0u16;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].pinned {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 钉版成功次数。
    pub const fn pin_ops(&self) -> u16 {
        self.pin_ops
    }

    /// 被拒操作次数。
    pub const fn rejects(&self) -> u16 {
        self.rejects
    }

    /// 钉一枚。
    ///
    /// 已钉再钉必须报 `DuplicatePin` 而不是幂等返回 Ok：把重复操作静默算成功，
    /// `pin_ops` 就会随重试次数虚增，「钉了多少枚」这个数从此不可信。
    pub fn pin(&mut self, serial: u16) -> Result<(), PinFault> {
        let idx = match self.index_of(serial) {
            Some(i) => i,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(PinFault::new(PinCode::BadSerial, serial, self.len() as u16));
            }
        };
        if self.entries[idx].pinned {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PinFault::new(PinCode::DuplicatePin, serial, self.pin_ops));
        }
        self.entries[idx].pinned = true;
        self.pin_ops = self.pin_ops.saturating_add(1);
        Ok(())
    }

    /// 撤钉（构造未钉样本用；未钉再撤报 `Unpinned`，与 `pin` 的重复语义对称）。
    pub fn unpin(&mut self, serial: u16) -> Result<(), PinFault> {
        let idx = match self.index_of(serial) {
            Some(i) => i,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(PinFault::new(PinCode::BadSerial, serial, self.len() as u16));
            }
        };
        if !self.entries[idx].pinned {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PinFault::new(PinCode::Unpinned, serial, 0));
        }
        self.entries[idx].pinned = false;
        self.pin_ops = self.pin_ops.saturating_sub(1);
        Ok(())
    }

    /// 改写正文。
    ///
    /// 必须重算校验和：不重算的话「换正文」在链上完全隐形，而这正是本模块最想
    /// 抓的一类改动。改写前先过格式校验——改写不是绕过格式闸的后门。
    pub fn revise_text(&mut self, serial: u16, text: &str) -> Result<(), PinFault> {
        let idx = match self.index_of(serial) {
            Some(i) => i,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(PinFault::new(PinCode::BadSerial, serial, self.len() as u16));
            }
        };
        let tag = self.entries[idx].tag;
        if let Err(c) = check_text(text, &tag) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PinFault::new(c, serial, 0));
        }
        self.entries[idx].text = text.to_string();
        self.entries[idx].checksum = fnv_bytes(text);
        Ok(())
    }

    /// 确定性抽样：返回实抽到的总序（1-based，严格递增）。
    /// 只读、不记被拒数：抽样是查询，查询带副作用会让「被拒次数」失去意义。
    /// 步长取整会丢掉尾部余数——抽查本就不承诺覆盖全集，全量核验另有其路。
    pub fn sample(&self, want: usize) -> Result<Vec<u16>, PinFault> {
        if want < SAMPLE_MIN {
            return Err(PinFault::new(PinCode::SampleShort, 0, want as u16));
        }
        if want > self.entries.len() {
            return Err(PinFault::new(PinCode::CapacityOver, 0, want as u16));
        }
        let step = self.entries.len() / want;
        let mut out = Vec::with_capacity(want);
        let mut k = 0usize;
        while k < want {
            // 偏移恒小于步长，故相邻两项差落在 (0, 2×step) 内且严格为正：
            // 既散开又不重复，不需要额外查重。
            let idx = k * step + (k * 7) % step;
            out.push((idx as u16).saturating_add(1));
            k += 1;
        }
        Ok(out)
    }

    /// 对抽样结果逐枚复检正文。
    pub fn sample_verdict(&self, want: usize) -> Result<SampleReport, PinFault> {
        let serials = self.sample(want)?;
        let mut passed = 0u16;
        let mut first_bad = 0u16;
        let mut i = 0usize;
        while i < serials.len() {
            let s = serials[i];
            let ok = match self.entry(s) {
                Some(e) => check_text(&e.text, &e.tag).is_ok(),
                None => false,
            };
            if ok {
                passed = passed.saturating_add(1);
            } else if first_bad == 0 {
                first_bad = s;
            }
            i += 1;
        }
        Ok(SampleReport { asked: serials.len() as u16, passed, first_bad })
    }

    /// 终钉清点：全册 800 枚、无未钉、抽样 80 例全过。
    /// 先全量后抽样：全量清点便宜且能定位未钉枚；若先抽样，抽样命中的坏文本会被
    /// 误报成整体格式崩坏。同样只读，不改被拒计数。
    pub fn close_audit(&self) -> Result<u16, PinFault> {
        if self.entries.len() != PIN_TOTAL {
            return Err(PinFault::new(PinCode::BadSerial, self.next_serial(), 0));
        }
        let mut unpinned = 0u16;
        let mut i = 0usize;
        while i < self.entries.len() {
            if !self.entries[i].pinned {
                unpinned = unpinned.saturating_add(1);
            }
            i += 1;
        }
        if unpinned > 0 {
            return Err(PinFault::new(PinCode::Unpinned, 0, unpinned));
        }
        let rep = self.sample_verdict(SAMPLE_MIN)?;
        if rep.passed != rep.asked {
            return Err(PinFault::new(PinCode::TextShape, rep.first_bad, rep.passed));
        }
        Ok(self.entries.len() as u16)
    }

    /// 指纹链：每枚把 (条目号, J 序, 文本校验和, 钉版位) 咬进上一步的结果。
    ///
    /// 逐枚咬着走而不是把各枚和起来：链能定位「从第几枚起变了」，求和只能说明
    /// 「整体变了」。定位能力决定了修复成本。
    pub fn chain(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = &self.entries[i];
            h = fnv_u64(h, e.tag.owner as u64);
            h = fnv_u64(h, e.tag.ordinal as u64);
            h = fnv_u64(h, e.checksum);
            h = fnv_u64(h, if e.pinned { 1 } else { 0 });
            i += 1;
        }
        h
    }
}

/// 文本格式校验器（判据原文「判据文本格式统一」的唯一实现处）。
///
/// 校验次序按「判别力从高到低」排：先看前缀（一眼看出贴错条目），再看分隔与空白
/// （手写最常带进来的噪声），再看长度窗，最后查模糊词（最贵：全串扫五遍）。
pub fn check_text(text: &str, tag: &CritTag) -> Result<(), PinCode> {
    let label = tag.label();
    if !text.starts_with(&label) {
        return Err(PinCode::PrefixMismatch);
    }
    let rest = &text[label.len()..];
    if !rest.starts_with(' ') {
        return Err(PinCode::TextShape);
    }
    if text != text.trim() {
        return Err(PinCode::TextShape);
    }
    let chars = text.chars().count();
    if chars < TEXT_MIN || chars > TEXT_MAX {
        return Err(PinCode::TextShape);
    }
    if rest.trim().chars().count() < 12 {
        return Err(PinCode::TextShape);
    }
    let mut i = 0usize;
    while i < FORBIDDEN.len() {
        if text.contains(FORBIDDEN[i]) {
            return Err(PinCode::TextShape);
        }
        i += 1;
    }
    Ok(())
}

/// FNV-1a 64 单步（质数 0x100000001b3 来自 FNV 规范）。
pub const fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        let byte = ((v >> (i * 8)) & 0xFF) as u8;
        h ^= byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 整串字节摘要（逐字节咬合，与 `fnv_u64` 共用质数但入口不同：正文是字节流，
/// 按 u64 分段喂会在末尾补零，导致「a」与「a\\0」同摘要）。
pub fn fnv_bytes(s: &str) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 标准正文（夹具用）：带真标签、长度在窗内、无模糊词。
fn body_text(serial: u16) -> String {
    let tag = CritTag::from_serial(serial);
    let mut s = tag.label();
    s.push_str(" 判据第");
    s.push_str(&serial.to_string());
    s.push_str("枚终钉：编号自洽、文本合规、钉版在位，逐字可复核");
    s
}

/// 构造标准钉版册（800 枚全钉）。返回 Result：夹具若是坏的，整册判据会被一处强制取值挡在门外、从未真跑过，
/// 故构造失败要能被第一条判据看见。
pub fn standard_pinset() -> Result<PinSet, PinFault> {
    let mut set = PinSet::new();
    let mut serial: u16 = 1;
    while serial <= PIN_TOTAL as u16 {
        set.admit(serial, &body_text(serial))?;
        set.pin(serial)?;
        serial += 1;
    }
    Ok(set)
}

/// 取故障码（成功记 0）。
fn code_of<T>(r: Result<T, PinFault>) -> u32 {
    match r {
        Ok(_) => 0,
        Err(f) => f.code,
    }
}

/// 判据入口（UNX-F21583-J1 及其配套）。
///
/// 800 / 21581 / 21620 / 80 在断言里一律写字面量：复用被测常量等于让被测值
/// 自己证自己。
pub fn run_unx21583_critpin_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21583");
    let fixture = standard_pinset();

    // 1) 夹具可构造（800 枚全入册、全钉）。
    cs.add("f21583.fixture_buildable",
        match &fixture { Ok(s) => s.len() == 800 && s.pinned_count() == 800, Err(_) => false },
        "标准 800 枚钉版册构造失败或未全钉");

    // 2) 编号自洽：1 / 20 / 21 / 800 四处的条目号与 J 序必须精确命中。
    cs.add("f21583.numbering_selfconsistent",
        match &fixture {
            Ok(s) => {
                let a = s.entry(1).map(|e| e.tag) == Some(CritTag::new(21_581, 1));
                let b = s.entry(20).map(|e| e.tag) == Some(CritTag::new(21_581, 20));
                let c = s.entry(21).map(|e| e.tag) == Some(CritTag::new(21_582, 1));
                let d = s.entry(800).map(|e| e.tag) == Some(CritTag::new(21_620, 20));
                a && b && c && d
            }
            Err(_) => false,
        },
        "编号自洽失败（期望 1→F21581-J1、20→F21581-J20、21→F21582-J1、800→F21620-J20）");

    // 3) 全量文本合规：800 枚逐枚过校验器。
    {
        let mut ok = 0u16;
        if let Ok(s) = &fixture {
            let mut serial: u16 = 1;
            while serial <= 800 {
                if let Some(e) = s.entry(serial) {
                    if check_text(&e.text, &e.tag).is_ok() {
                        ok = ok.saturating_add(1);
                    }
                }
                serial += 1;
            }
        }
        cs.add("f21583.text_all_valid", ok == 800, "全量文本合规数不足 800（校验器与夹具不一致）");
    }

    // 4) 抽样 80 例：例数、严格递增、落在 1..=800、两次抽样逐位相同。
    {
        let mut good = false;
        if let Ok(s) = &fixture {
            let a = s.sample(80);
            let b = s.sample(80);
            good = match (&a, &b) {
                (Ok(x), Ok(y)) => {
                    let mut inc = true;
                    let mut i = 1usize;
                    while i < x.len() {
                        if x[i] <= x[i - 1] {
                            inc = false;
                        }
                        i += 1;
                    }
                    x.len() == 80 && x == y && inc && x[0] >= 1 && x[x.len() - 1] <= 800
                }
                _ => false,
            };
        }
        cs.add("f21583.sample_80",
            good,
            "抽样例数不为 80、非严格递增、越界，或两次抽样不一致（非确定性）");
    }

    // 5) 抽样 80 例全过。
    {
        let rep = match &fixture {
            Ok(s) => s.sample_verdict(80).ok(),
            Err(_) => None,
        };
        cs.add("f21583.sample_all_pass",
            match rep {
                Some(r) => r.asked == 80 && r.passed == 80 && r.first_bad == 0,
                None => false,
            },
            "抽样 80 例未全过（asked/passed/first_bad 不符）");
    }

    // 6) 抽样边界：79 例必报 SampleShort，801 例必报 CapacityOver。
    {
        let short = match &fixture {
            Ok(s) => code_of(s.sample(79)) == PinCode::SampleShort.code(),
            Err(_) => false,
        };
        let over = match &fixture {
            Ok(s) => code_of(s.sample(801)) == PinCode::CapacityOver.code(),
            Err(_) => false,
        };
        cs.add("f21583.sample_bounds", short && over,
            "79 例未报 SampleShort 或 801 例未报 CapacityOver");
    }

    // 7) 前缀不符必拒：格式校验器与入册路径各一例。
    {
        let tag = CritTag::new(21_581, 1);
        let wrong = "UNX-F21582-J1 判据第1枚终钉：编号自洽、文本合规、钉版在位，逐字可复核";
        let direct = check_text(wrong, &tag) == Err(PinCode::PrefixMismatch);
        let mut admit_hit = false;
        let mut set = PinSet::new();
        if let Err(f) = set.admit(1, wrong) {
            admit_hit = f.code == PinCode::PrefixMismatch.code() && f.serial == 1 && set.len() == 0;
        }
        cs.add("f21583.prefix_mismatch", direct && admit_hit,
            "错条目前缀未被判 PrefixMismatch，或入册后未回滚（len 应为 0）");
    }

    // 8) 文本形态三类违规各一例：模糊词 / 过短 / 首尾空白。
    {
        let tag = CritTag::new(21_581, 3);
        let vague = "UNX-F21581-J3 本枚判据大致要求编号自洽与文本合规，具体视情况而定";
        let short = "UNX-F21581-J3 太短";
        let spaced = "UNX-F21581-J3 判据第3枚终钉：编号自洽、文本合规、钉版在位，逐字可复核 ";
        let a = check_text(vague, &tag) == Err(PinCode::TextShape);
        let b = check_text(short, &tag) == Err(PinCode::TextShape);
        let c = check_text(spaced, &tag) == Err(PinCode::TextShape);
        cs.add("f21583.text_shape_bad", a && b && c,
            "模糊词/过短/尾随空白三类未全部判 TextShape");
    }

    // 9) 重复钉版必拒，且钉版计数不虚增。
    {
        let mut ok = false;
        if let Ok(mut s) = standard_pinset() {
            let before = s.pin_ops();
            let again = s.pin(1);
            ok = again.err().map(|f| f.code) == Some(PinCode::DuplicatePin.code())
                && s.pin_ops() == before
                && s.rejects() >= 1;
        }
        cs.add("f21583.duplicate_pin", ok, "二次钉版未被拒，或 pin_ops 被虚增");
    }

    // 10) 未钉清点：标准册通过；撤钉一枚后必报 Unpinned 且计数为 1。
    {
        let clean = match standard_pinset() { Ok(s) => s.close_audit() == Ok(800), Err(_) => false };
        let dirty = match standard_pinset() {
            Ok(mut s) => match s.unpin(77) {
                Ok(_) => s.close_audit().err().map(|f| (f.code, f.aux)),
                Err(_) => None,
            },
            Err(_) => None,
        };
        cs.add("f21583.unpinned_audit",
            clean && dirty == Some((PinCode::Unpinned.code(), 1)),
            "标准册终钉未过，或撤钉一枚后未报 Unpinned/未钉数不为 1");
    }

    // 11) 跳号必拒：0 号、越过下一位、越界钉版三种入口。
    {
        let mut set = PinSet::new();
        let zero = code_of(set.admit(0, &body_text(0))) == PinCode::BadSerial.code();
        let ok1 = set.admit(1, &body_text(1)).is_ok();
        let skip = code_of(set.admit(3, &body_text(3))) == PinCode::BadSerial.code()
            && set.len() == 1;
        let ghost = code_of(set.pin(801)) == PinCode::BadSerial.code();
        cs.add("f21583.serial_gap", zero && ok1 && skip && ghost,
            "0 号/跳号/越界钉版未全部判 BadSerial");
    }

    // 12) 容量上限：满 800 后再入册必报 CapacityOver，册长不变。
    {
        let mut ok = false;
        if let Ok(mut s) = standard_pinset() {
            let f = s.admit(801, &body_text(801));
            ok = f.err().map(|x| x.code) == Some(PinCode::CapacityOver.code()) && s.len() == 800;
        }
        cs.add("f21583.capacity_over", ok, "第 801 枚未被拒或册长被改");
    }

    // 13) 指纹链：同输入两遍一致；换一枚合规正文后链必变；换后仍合规。
    {
        let fp = |r: Result<PinSet, PinFault>| match r { Ok(x) => x.chain(), Err(_) => 0 };
        let a = fp(standard_pinset());
        let b = fp(standard_pinset());
        let mut moved = 0u64;
        let mut still_valid = false;
        if let Ok(mut s) = standard_pinset() {
            // 第 400 枚归属 F21600-J20（399/20=19 得条目，399%20+1=20 得 J 序）：
            // 前缀按规则写死，改写才可能过格式闸，否则测的是前缀码而不是链。
            let alt = "UNX-F21600-J20 改钉第400枚：编号自洽、文本合规、钉版在位，改后逐字复核";
            if s.revise_text(400, alt).is_ok() {
                moved = s.chain();
                still_valid = match s.entry(400) {
                    Some(e) => check_text(&e.text, &e.tag).is_ok(),
                    None => false,
                };
            }
        }
        cs.add("f21583.chain_tamper",
            a != 0 && a == b && moved != a && still_valid,
            "链非确定，或合规换文后链未变（校验和未重算），或换文被格式闸误拒");
    }

    // 14) 幂等：终钉清点两遍结果相同；重复钉版只加一次被拒计数。
    {
        let mut ok = false;
        if let Ok(s) = standard_pinset() {
            let first = s.close_audit();
            let second = s.close_audit();
            let rep_a = s.sample_verdict(80);
            let rep_b = s.sample_verdict(80);
            ok = first == second && first == Ok(800) && rep_a == rep_b;
        }
        cs.add("f21583.idempotent", ok, "两次终钉清点或两次抽样结果不同");
    }

    // 15) 八码闭集：落段内、两两互异、三要素齐全、未知码不 panic。
    {
        const ALL: [PinCode; 8] = [
            PinCode::Ok, PinCode::BadSerial, PinCode::PrefixMismatch, PinCode::TextShape,
            PinCode::DuplicatePin, PinCode::Unpinned, PinCode::CapacityOver, PinCode::SampleShort,
        ];
        let mut ok = true;
        let mut i = 0usize;
        while i < ALL.len() {
            let c = ALL[i];
            let n = c.code();
            if n < 0x91011 || n > 0x91018 {
                ok = false;
            }
            if c.what().is_empty() || c.why().is_empty() || c.advice().is_empty() {
                ok = false;
            }
            let mut j = i + 1;
            while j < ALL.len() {
                if ALL[j].code() == n {
                    ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        let bogus = PinFault { code: 0x910FF, serial: 0, aux: 0 };
        ok = ok && !bogus.is_known() && !bogus.describe().is_empty();
        cs.add("f21583.code_closed_set", ok, "诊断码越段、互复、三要素缺失，或未知码未被识别");
    }

    cs
}
