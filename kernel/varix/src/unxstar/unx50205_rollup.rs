//! UNX-F50205 计费账持久化（日结/月结聚合档）（0x44040 段）
//!
//! 锚点：UNX-F50205-J1 聚合 ×30 与逐任务账勾稽一致；判据锚定 §AI-63 §2 专题十。
//!
//! 聚合账最容易出的错不是「加错」，而是**三处数字各自成立、彼此对不上**：
//! 日结档是 19 个桶的和，月结档是 3 个桶的和，逐任务账是 30 条记录的和；
//! 三处任何一处漏记（比如某天插入失败被静默丢弃、某月跨年键位算错），
//! 单看每一处都「内部自洽」，只有把三处摆在一起勾稽才会暴露。
//!
//! 本模块把这条勾稽关系做成可执行的：
//! * [`CivilDate`] —— 日历日期，`valid()` 按真实月长与闰年规则判定。
//!   键位不是字符串，而是 `yyyymmdd` / `yyyymm` 整数：**整数键的排序即时间序**，
//!   持久化后可以直接按字节序归并，不需要再解析日期；
//! * [`Rollup`] —— 单个聚合桶。`merge` 用饱和加法，桶摘要 [`Rollup::digest`]
//!   覆盖全部字段，因此「编码—解码」天然带校验和：任何一个数字被改动，
//!   重算摘要必然对不上（见 [`decode_reason`]）；
//! * [`RollupBook`] —— 日/月两本定长桶表。桶按 **key 升序**落位（插入即归位），
//!   因此同样一批任务无论以什么顺序汇总，落盘字节与账摘要都一致。这一条
//!   不是洁癖：重放日志、并发汇总、补账重算三条路径的到达顺序本来就不同，
//!   若账本顺序依赖到达顺序，「同一批任务两次汇总结果不同」就成了常态；
//! * [`reconcile`] —— 三向勾稽：桶内容对逐任务重算、覆盖性（每条记录都能
//!   在自己的键位桶里找到）、总量守恒（日结和 == 月结和 == 逐任务和）。
//!
//! 持久化格式是一行一桶的竖线分隔文本：
//! `D|yyyymmdd|faces|sheets|color|bw|recs|16位摘要`。
//! 选择文本而不是二进制，理由是可审计：计费争议时人可以逐行读；
//! 选择定长桶而不是哈希表，理由是内核态不允许持久化顺序依赖分配器状态。
//!
//! 30 条样本横跨 2026 年 1—3 月（19 个自然日、3 个月份），
//! 覆盖「同日多单」「同月跨日」「跨月边界」「月末」四种聚合形态。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 诊断段（0x44040 独占 0x40）。
pub const SEG_BASE: u32 = 0x44040;

/// 样本任务条数（卡面 30 条，硬编码）。
pub const SAMPLE_COUNT: usize = 30;

/// 日结桶容量：19 个自然日只用 19 格，留出 2 倍余量给跨日重跑。
pub const DAILY_SLOTS: usize = 40;

/// 月结桶容量：一年 12 个月，刚好装满；跨年时由上层按年分册。
pub const MONTHLY_SLOTS: usize = 12;

/// 摘要种子（FNV-1a 64）。
pub const DIGEST_SEED: u64 = 0xcbf2_9ce4_8422_2325;

/// 11 条诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettleCode {
    KeyRange,
    DateImpossible,
    BucketMissing,
    BucketDrift,
    CountDrift,
    TotalDrift,
    EncodeField,
    Checksum,
    Capacity,
    OrderDrift,
    Unknown,
}

impl SettleCode {
    /// 段内偏移（1..=11），唯一允许决定码值的地方。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                SettleCode::KeyRange => 1,
                SettleCode::DateImpossible => 2,
                SettleCode::BucketMissing => 3,
                SettleCode::BucketDrift => 4,
                SettleCode::CountDrift => 5,
                SettleCode::TotalDrift => 6,
                SettleCode::EncodeField => 7,
                SettleCode::Checksum => 8,
                SettleCode::Capacity => 9,
                SettleCode::OrderDrift => 10,
                SettleCode::Unknown => 11,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            SettleCode::KeyRange => "聚合键超出所属粒度允许的键域",
            SettleCode::DateImpossible => "日历日期在该月不存在（含闰年二月）",
            SettleCode::BucketMissing => "记录能在逐任务账中找到、但聚合档缺桶",
            SettleCode::BucketDrift => "桶内容与逐任务重算结果不一致",
            SettleCode::CountDrift => "桶内记录条数与逐任务账不一致",
            SettleCode::TotalDrift => "日结合计、月结合计与逐任务合计三者不等",
            SettleCode::EncodeField => "持久化行字段数或数值格式非法",
            SettleCode::Checksum => "持久化行摘要与重算摘要不一致",
            SettleCode::Capacity => "聚合档桶位耗尽，拒绝静默丢桶",
            SettleCode::OrderDrift => "不同插入顺序产出不同账本摘要",
            SettleCode::Unknown => "未归类缺陷，保留给审计兜底",
        }
    }
}

/// 闭集（顺序即码值序）。
pub const ALL_CODES: [SettleCode; 11] = [
    SettleCode::KeyRange,
    SettleCode::DateImpossible,
    SettleCode::BucketMissing,
    SettleCode::BucketDrift,
    SettleCode::CountDrift,
    SettleCode::TotalDrift,
    SettleCode::EncodeField,
    SettleCode::Checksum,
    SettleCode::Capacity,
    SettleCode::OrderDrift,
    SettleCode::Unknown,
];

fn fnv_mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x100_0000_01b3)
}

fn is_leap(year: u16) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// 该月天数；月份非法返回 0（让调用方用区间比较统一拒绝）。
pub fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// 日历日期。`new` 不做校验（const 友好），`valid` 是唯一的判定入口。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CivilDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl CivilDate {
    pub const fn new(year: u16, month: u8, day: u8) -> CivilDate {
        CivilDate { year, month, day }
    }

    pub const ZERO: CivilDate = CivilDate::new(0, 0, 0);

    /// 年份下限取 1970：Unix 纪元之前的时间戳在计费侧没有意义，
    /// 且 0 年会被 `day_key` 折成前缀 0，与「未初始化」混淆。
    pub fn valid(self) -> bool {
        self.year >= 1970
            && self.year <= 9999
            && self.month >= 1
            && self.month <= 12
            && self.day >= 1
            && self.day <= days_in_month(self.year, self.month)
    }

    /// 日结键：yyyymmdd。
    pub fn day_key(self) -> u32 {
        (self.year as u32) * 10_000 + (self.month as u32) * 100 + self.day as u32
    }

    /// 月结键：yyyymm。
    pub fn month_key(self) -> u32 {
        (self.year as u32) * 100 + self.month as u32
    }

    /// 从日结键还原（键域非法返回 None，不做钳位）。
    pub fn from_day_key(key: u32) -> Option<CivilDate> {
        let d = CivilDate::new(
            (key / 10_000) as u16,
            ((key / 100) % 100) as u8,
            (key % 100) as u8,
        );
        if d.valid() && d.day_key() == key {
            Some(d)
        } else {
            None
        }
    }

    /// 从月结键还原（必须恰好是 6 位十进制，即 year*100+month）。
    pub fn from_month_key(key: u32) -> Option<CivilDate> {
        let year = (key / 100) as u16;
        let month = (key % 100) as u8;
        if year >= 1970 && year <= 9999 && month >= 1 && month <= 12 {
            Some(CivilDate::new(year, month, 1))
        } else {
            None
        }
    }
}

/// 聚合粒度。`key_of` 是**唯一**允许决定键位的地方。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Granularity {
    Day,
    Month,
}

impl Granularity {
    pub fn tag(self) -> char {
        match self {
            Granularity::Day => 'D',
            Granularity::Month => 'M',
        }
    }

    pub fn from_tag(t: char) -> Option<Granularity> {
        match t {
            'D' => Some(Granularity::Day),
            'M' => Some(Granularity::Month),
            _ => None,
        }
    }

    pub fn key_of(self, date: CivilDate) -> u32 {
        match self {
            Granularity::Day => date.day_key(),
            Granularity::Month => date.month_key(),
        }
    }

    /// 键域自校验：日结键必须能还原成一个真实日期，月结键必须能还原成真实月份。
    pub fn valid_key(self, key: u32) -> bool {
        match self {
            Granularity::Day => CivilDate::from_day_key(key).is_some(),
            Granularity::Month => CivilDate::from_month_key(key).is_some(),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Granularity::Day => "日结",
            Granularity::Month => "月结",
        }
    }
}

/// 一条逐任务计费记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BillRecord {
    pub job: u32,
    pub date: CivilDate,
    pub color_faces: u32,
    pub bw_faces: u32,
    pub sheets: u32,
    pub copies: u8,
}

impl BillRecord {
    pub const fn new(
        job: u32,
        date: CivilDate,
        color_faces: u32,
        bw_faces: u32,
        sheets: u32,
        copies: u8,
    ) -> BillRecord {
        BillRecord {
            job,
            date,
            color_faces,
            bw_faces,
            sheets,
            copies,
        }
    }

    pub const ZERO: BillRecord =
        BillRecord::new(0, CivilDate::ZERO, 0, 0, 0, 0);

    /// 计费面 = 彩色面 + 黑白面。用 u64 承载：单任务双面 9999 份时
    /// 面数会超过 u32 的舒适区，聚合阶段更不该冒着回绕的风险。
    pub fn faces(self) -> u64 {
        self.color_faces as u64 + self.bw_faces as u64
    }

    /// 可接纳：日期真实、面数非零、纸张与份数非零。
    pub fn admissible(self) -> bool {
        self.date.valid() && self.faces() > 0 && self.sheets > 0 && self.copies >= 1
    }
}

/// 30 条样本（job 0..=29）。
pub static SAMPLE_RECORDS: [BillRecord; SAMPLE_COUNT] = [
    BillRecord::new(0, CivilDate::new(2026, 1, 2), 120, 340, 230, 1),
    BillRecord::new(1, CivilDate::new(2026, 1, 2), 0, 500, 250, 2),
    BillRecord::new(2, CivilDate::new(2026, 1, 5), 60, 180, 120, 3),
    BillRecord::new(3, CivilDate::new(2026, 1, 5), 300, 0, 150, 1),
    BillRecord::new(4, CivilDate::new(2026, 1, 9), 45, 255, 150, 2),
    BillRecord::new(5, CivilDate::new(2026, 1, 12), 900, 300, 600, 1),
    BillRecord::new(6, CivilDate::new(2026, 1, 12), 10, 90, 50, 4),
    BillRecord::new(7, CivilDate::new(2026, 1, 12), 240, 240, 240, 1),
    BillRecord::new(8, CivilDate::new(2026, 1, 15), 0, 1000, 500, 2),
    BillRecord::new(9, CivilDate::new(2026, 1, 20), 75, 425, 250, 1),
    BillRecord::new(10, CivilDate::new(2026, 2, 1), 130, 370, 250, 2),
    BillRecord::new(11, CivilDate::new(2026, 2, 1), 0, 260, 130, 1),
    BillRecord::new(12, CivilDate::new(2026, 2, 3), 500, 500, 500, 3),
    BillRecord::new(13, CivilDate::new(2026, 2, 14), 180, 20, 100, 1),
    BillRecord::new(14, CivilDate::new(2026, 2, 14), 0, 640, 320, 2),
    BillRecord::new(15, CivilDate::new(2026, 2, 14), 90, 210, 150, 5),
    BillRecord::new(16, CivilDate::new(2026, 2, 20), 420, 180, 300, 1),
    BillRecord::new(17, CivilDate::new(2026, 2, 27), 30, 270, 150, 2),
    BillRecord::new(18, CivilDate::new(2026, 2, 28), 0, 480, 240, 1),
    BillRecord::new(19, CivilDate::new(2026, 2, 28), 360, 120, 240, 2),
    BillRecord::new(20, CivilDate::new(2026, 3, 1), 80, 320, 200, 3),
    BillRecord::new(21, CivilDate::new(2026, 3, 1), 0, 720, 360, 1),
    BillRecord::new(22, CivilDate::new(2026, 3, 1), 150, 150, 150, 2),
    BillRecord::new(23, CivilDate::new(2026, 3, 5), 600, 200, 400, 1),
    BillRecord::new(24, CivilDate::new(2026, 3, 11), 25, 175, 100, 4),
    BillRecord::new(25, CivilDate::new(2026, 3, 11), 0, 900, 450, 2),
    BillRecord::new(26, CivilDate::new(2026, 3, 18), 210, 90, 150, 1),
    BillRecord::new(27, CivilDate::new(2026, 3, 24), 48, 352, 200, 3),
    BillRecord::new(28, CivilDate::new(2026, 3, 30), 0, 560, 280, 2),
    BillRecord::new(29, CivilDate::new(2026, 3, 31), 333, 167, 250, 1),
];

/// 单个聚合桶。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rollup {
    pub key: u32,
    pub faces: u64,
    pub sheets: u64,
    pub color: u64,
    pub bw: u64,
    pub recs: u32,
}

impl Rollup {
    pub const ZERO: Rollup = Rollup {
        key: 0,
        faces: 0,
        sheets: 0,
        color: 0,
        bw: 0,
        recs: 0,
    };

    pub fn new(key: u32) -> Rollup {
        Rollup { key, ..Rollup::ZERO }
    }

    /// 并入一条记录。全部加法饱和：计费账宁可停在 u64::MAX 并报警，
    /// 也不能回绕成一个「看起来正常」的小数字。
    pub fn merge(&mut self, r: BillRecord) {
        self.faces = self.faces.saturating_add(r.faces());
        self.sheets = self.sheets.saturating_add(r.sheets as u64);
        self.color = self.color.saturating_add(r.color_faces as u64);
        self.bw = self.bw.saturating_add(r.bw_faces as u64);
        self.recs = self.recs.saturating_add(1);
    }

    /// 桶内色彩分账闭合：color + bw == faces。
    pub fn closed(self) -> bool {
        self.color.saturating_add(self.bw) == self.faces
    }

    /// 桶摘要：键 + 五个数字。编码时写入、解码时重算，组成校验和。
    pub fn digest(self) -> u64 {
        let mut h = DIGEST_SEED;
        h = fnv_mix(h, self.key as u64);
        h = fnv_mix(h, self.faces);
        h = fnv_mix(h, self.sheets);
        h = fnv_mix(h, self.color);
        h = fnv_mix(h, self.bw);
        h = fnv_mix(h, self.recs as u64);
        h
    }
}

/// 桶位：`used` 与 `roll.key` 分离，避免用 key==0 兼任「空」标记
/// （0 是合法 usize 但非法日期键，依赖它会让「空桶」与「坏桶」混淆）。
#[derive(Clone, Copy, Debug)]
struct Slot {
    used: bool,
    roll: Rollup,
}

const EMPTY_SLOT: Slot = Slot {
    used: false,
    roll: Rollup::ZERO,
};

/// 日结 + 月结两本定长桶表。
#[derive(Clone, Copy, Debug)]
pub struct RollupBook {
    daily: [Slot; DAILY_SLOTS],
    monthly: [Slot; MONTHLY_SLOTS],
    pub nd: usize,
    pub nm: usize,
}

impl RollupBook {
    pub const fn new() -> RollupBook {
        RollupBook {
            daily: [EMPTY_SLOT; DAILY_SLOTS],
            monthly: [EMPTY_SLOT; MONTHLY_SLOTS],
            nd: 0,
            nm: 0,
        }
    }

    fn slots(&self, g: Granularity) -> &[Slot] {
        match g {
            Granularity::Day => &self.daily,
            Granularity::Month => &self.monthly,
        }
    }

    fn slots_mut(&mut self, g: Granularity) -> &mut [Slot] {
        match g {
            Granularity::Day => &mut self.daily,
            Granularity::Month => &mut self.monthly,
        }
    }

    pub fn capacity(&self, g: Granularity) -> usize {
        self.slots(g).len()
    }

    pub fn used_count(&self, g: Granularity) -> usize {
        let mut n = 0usize;
        let slots = self.slots(g);
        let mut i = 0usize;
        while i < slots.len() {
            if let Some(s) = slots.get(i) {
                if s.used {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn find(&self, g: Granularity, key: u32) -> Option<Rollup> {
        let slots = self.slots(g);
        let mut i = 0usize;
        while i < slots.len() {
            if let Some(s) = slots.get(i) {
                if s.used && s.roll.key == key {
                    return Some(s.roll);
                }
            }
            i += 1;
        }
        None
    }

    /// 桶位遍历（只读），越界返回 None。
    pub fn bucket_at(&self, g: Granularity, i: usize) -> Option<Rollup> {
        match self.slots(g).get(i) {
            Some(s) if s.used => Some(s.roll),
            _ => None,
        }
    }

    /// 带键域校验的插入：非法日期键直接拒绝，不落桶。
    pub fn insert(&mut self, g: Granularity, r: BillRecord) -> bool {
        let key = g.key_of(r.date);
        if !g.valid_key(key) || !r.admissible() {
            return false;
        }
        self.insert_raw(g, key, r)
    }

    /// 定位键位：已存在返回其下标；不存在则按升序插入空桶后返回新下标；
    /// 桶位耗尽返回 None。`(下标, 是否新建)` 的第二项只用于计数。
    fn slot_index_for(&mut self, g: Granularity, key: u32) -> Option<usize> {
        let cap = self.capacity(g);
        let (found, is_new) = {
            let slots = self.slots_mut(g);
            // 已用桶恒占据 0..n（插入即归位的直接推论）。
            let mut n = 0usize;
            while n < cap {
                match slots.get(n) {
                    Some(s) if s.used => n += 1,
                    _ => break,
                }
            }
            let mut p = 0usize;
            while p < n {
                let k = match slots.get(p) {
                    Some(s) if s.used => s.roll.key,
                    _ => key,
                };
                if k >= key {
                    break;
                }
                p += 1;
            }
            let at_p = match slots.get(p) {
                Some(s) if s.used => Some(s.roll.key),
                _ => None,
            };
            match at_p {
                Some(k) if k == key => (Some(p), false),
                Some(_) => {
                    // 中间插入：p..n 整体右移一格，空出的 p 放新桶。
                    slots.copy_within(p..n, p + 1);
                    slots[p] = Slot {
                        used: true,
                        roll: Rollup::new(key),
                    };
                    (Some(p), true)
                }
                None => {
                    if n < cap {
                        slots[n] = Slot {
                            used: true,
                            roll: Rollup::new(key),
                        };
                        (Some(n), true)
                    } else {
                        (None, false)
                    }
                }
            }
        };
        if is_new {
            match g {
                Granularity::Day => self.nd += 1,
                Granularity::Month => self.nm += 1,
            }
        }
        found
    }

    /// 原始插入（回放持久化行时使用；键域已由解码校验）。
    /// 桶位耗尽显式返回 false，绝不覆盖既有桶。
    pub fn insert_raw(&mut self, g: Granularity, key: u32, r: BillRecord) -> bool {
        match self.slot_index_for(g, key) {
            Some(i) => match self.slots_mut(g).get_mut(i) {
                Some(s) => {
                    s.roll.merge(r);
                    true
                }
                None => false,
            },
            None => false,
        }
    }

    /// 把**已聚合**的桶并入账本（回放用，不经过逐任务记录）。
    fn merge_roll(&mut self, g: Granularity, roll: Rollup) -> bool {
        match self.slot_index_for(g, roll.key) {
            Some(i) => match self.slots_mut(g).get_mut(i) {
                Some(s) => {
                    s.roll.faces = s.roll.faces.saturating_add(roll.faces);
                    s.roll.sheets = s.roll.sheets.saturating_add(roll.sheets);
                    s.roll.color = s.roll.color.saturating_add(roll.color);
                    s.roll.bw = s.roll.bw.saturating_add(roll.bw);
                    s.roll.recs = s.roll.recs.saturating_add(roll.recs);
                    true
                }
                None => false,
            },
            None => false,
        }
    }

    /// 账本摘要：粒度标签 + 全部已用桶（含键位）。
    pub fn digest(&self) -> u64 {
        let mut h = DIGEST_SEED;
        let grains = [Granularity::Day, Granularity::Month];
        let mut gi = 0usize;
        while gi < grains.len() {
            if let Some(g) = grains.get(gi).copied() {
                h = fnv_mix(h, g.tag() as u64);
                let slots = self.slots(g);
                let mut i = 0usize;
                while i < slots.len() {
                    if let Some(s) = slots.get(i) {
                        if s.used {
                            h = fnv_mix(h, s.roll.digest());
                        }
                    }
                    i += 1;
                }
            }
            gi += 1;
        }
        h
    }

    /// 日结合计 / 月结合计（面数）。
    pub fn total_faces(&self, g: Granularity) -> u64 {
        let mut sum = 0u64;
        let slots = self.slots(g);
        let mut i = 0usize;
        while i < slots.len() {
            if let Some(s) = slots.get(i) {
                if s.used {
                    sum = sum.saturating_add(s.roll.faces);
                }
            }
            i += 1;
        }
        sum
    }

    /// 日结合计 / 月结合计（记录条数）。
    pub fn total_recs(&self, g: Granularity) -> u64 {
        let mut sum = 0u64;
        let slots = self.slots(g);
        let mut i = 0usize;
        while i < slots.len() {
            if let Some(s) = slots.get(i) {
                if s.used {
                    sum = sum.saturating_add(s.roll.recs as u64);
                }
            }
            i += 1;
        }
        sum
    }

    /// 全量落盘：日结在前、月结在后，一行一桶。
    pub fn persist(&self) -> String {
        let mut out = String::new();
        let grains = [Granularity::Day, Granularity::Month];
        let mut gi = 0usize;
        while gi < grains.len() {
            if let Some(g) = grains.get(gi).copied() {
                let slots = self.slots(g);
                let mut i = 0usize;
                while i < slots.len() {
                    if let Some(s) = slots.get(i) {
                        if s.used {
                            out.push_str(&encode(g, s.roll));
                            out.push('\n');
                        }
                    }
                    i += 1;
                }
            }
            gi += 1;
        }
        out
    }

    /// 回放落盘文本。任何一行解码失败、键位重复、桶位耗尽都整体拒绝：
    /// 半本账比没有账更危险——人看见「恢复成功」就不会再去核对差额。
    pub fn restore(text: &str) -> Option<RollupBook> {
        let mut book = RollupBook::new();
        for line in text.split('\n') {
            if line.is_empty() {
                continue;
            }
            let (g, roll) = decode(line)?;
            if book.find(g, roll.key).is_some() {
                return None;
            }
            if !book.merge_roll(g, roll) {
                return None;
            }
        }
        Some(book)
    }
}

/// 单桶编码。字段顺序与文档一致，摘要放最后：人读时先看到数字，再看到指纹。
pub fn encode(g: Granularity, roll: Rollup) -> String {
    format!(
        "{}|{:08}|{}|{}|{}|{}|{}|{:016x}",
        g.tag(),
        roll.key,
        roll.faces,
        roll.sheets,
        roll.color,
        roll.bw,
        roll.recs,
        roll.digest()
    )
}

/// 解码 + 分类失败原因。这是持久化侧唯一的信任边界：
/// 解码器拒绝的行，上层不得再尝试「修补」后入库。
pub fn decode_reason(line: &str) -> Result<(Granularity, Rollup), SettleCode> {
    let mut parts = line.split('|');
    let tag = parts.next().ok_or(SettleCode::EncodeField)?;
    let key_s = parts.next().ok_or(SettleCode::EncodeField)?;
    let faces_s = parts.next().ok_or(SettleCode::EncodeField)?;
    let sheets_s = parts.next().ok_or(SettleCode::EncodeField)?;
    let color_s = parts.next().ok_or(SettleCode::EncodeField)?;
    let bw_s = parts.next().ok_or(SettleCode::EncodeField)?;
    let recs_s = parts.next().ok_or(SettleCode::EncodeField)?;
    let sum_s = parts.next().ok_or(SettleCode::EncodeField)?;
    if parts.next().is_some() {
        return Err(SettleCode::EncodeField);
    }
    let mut tc = tag.chars();
    let t = tc.next().ok_or(SettleCode::EncodeField)?;
    if tc.next().is_some() {
        return Err(SettleCode::EncodeField);
    }
    let g = Granularity::from_tag(t).ok_or(SettleCode::EncodeField)?;
    let key = u32::from_str_radix(key_s, 10).map_err(|_| SettleCode::EncodeField)?;
    let faces = u64::from_str_radix(faces_s, 10).map_err(|_| SettleCode::EncodeField)?;
    let sheets = u64::from_str_radix(sheets_s, 10).map_err(|_| SettleCode::EncodeField)?;
    let color = u64::from_str_radix(color_s, 10).map_err(|_| SettleCode::EncodeField)?;
    let bw = u64::from_str_radix(bw_s, 10).map_err(|_| SettleCode::EncodeField)?;
    let recs = u32::from_str_radix(recs_s, 10).map_err(|_| SettleCode::EncodeField)?;
    if !g.valid_key(key) {
        return Err(SettleCode::KeyRange);
    }
    if recs == 0 {
        return Err(SettleCode::CountDrift);
    }
    let roll = Rollup {
        key,
        faces,
        sheets,
        color,
        bw,
        recs,
    };
    if !roll.closed() {
        return Err(SettleCode::BucketDrift);
    }
    let want = format!("{:016x}", roll.digest());
    if want != sum_s {
        return Err(SettleCode::Checksum);
    }
    Ok((g, roll))
}

/// 解码（只要结果）。
pub fn decode(line: &str) -> Option<(Granularity, Rollup)> {
    decode_reason(line).ok()
}

/// 从逐任务账派生两本聚合档。
pub fn snapshot(recs: &[BillRecord]) -> RollupBook {
    let mut book = RollupBook::new();
    let mut i = 0usize;
    while i < recs.len() {
        if let Some(r) = recs.get(i).copied() {
            book.insert(Granularity::Day, r);
            book.insert(Granularity::Month, r);
        }
        i += 1;
    }
    book
}

/// 三向勾稽结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recon {
    pub buckets: usize,
    pub matched: usize,
    pub keys: bool,
    pub totals: bool,
    pub digest: bool,
    pub drift: SettleCode,
}

impl Recon {
    pub fn clean(self) -> bool {
        self.buckets > 0
            && self.matched == self.buckets
            && self.keys
            && self.totals
            && self.digest
            && self.drift == SettleCode::Unknown
    }

    /// 只要有一项不对就给出最贴近原因的那条码。
    pub fn reason(self) -> SettleCode {
        if !self.keys {
            SettleCode::BucketMissing
        } else if self.matched != self.buckets {
            SettleCode::BucketDrift
        } else if !self.totals {
            SettleCode::TotalDrift
        } else if !self.digest {
            SettleCode::OrderDrift
        } else {
            SettleCode::Unknown
        }
    }
}

/// 对某个键位重算逐任务合计。
fn expect_bucket(recs: &[BillRecord], g: Granularity, key: u32) -> Option<Rollup> {
    let mut roll = Rollup::new(key);
    let mut hit = false;
    let mut i = 0usize;
    while i < recs.len() {
        if let Some(r) = recs.get(i).copied() {
            if g.key_of(r.date) == key {
                roll.merge(r);
                hit = true;
            }
        }
        i += 1;
    }
    if hit {
        Some(roll)
    } else {
        None
    }
}

/// 三向勾稽：桶内容、覆盖性、总量、摘要。
pub fn reconcile(recs: &[BillRecord], book: &RollupBook) -> Recon {
    let mut buckets = 0usize;
    let mut matched = 0usize;
    let mut keys = true;

    let grains = [Granularity::Day, Granularity::Month];
    let mut gi = 0usize;
    while gi < grains.len() {
        if let Some(g) = grains.get(gi).copied() {
            // 逐个已用桶对逐任务账重算。
            let cap = book.capacity(g);
            let mut i = 0usize;
            while i < cap {
                if let Some(b) = book.bucket_at(g, i) {
                    buckets += 1;
                    match expect_bucket(recs, g, b.key) {
                        Some(e) => {
                            if e == b {
                                matched += 1;
                            }
                        }
                        None => keys = false,
                    }
                }
                i += 1;
            }
            // 覆盖性：每条记录都必须能在自己的键位找到桶。
            let mut j = 0usize;
            while j < recs.len() {
                if let Some(r) = recs.get(j).copied() {
                    if book.find(g, g.key_of(r.date)).is_none() {
                        keys = false;
                    }
                }
                j += 1;
            }
        }
        gi += 1;
    }

    let mut rec_faces = 0u64;
    let mut rec_count = 0u64;
    let mut i = 0usize;
    while i < recs.len() {
        if let Some(r) = recs.get(i).copied() {
            rec_faces = rec_faces.saturating_add(r.faces());
            rec_count = rec_count.saturating_add(1);
        }
        i += 1;
    }

    let totals = book.total_faces(Granularity::Day) == rec_faces
        && book.total_faces(Granularity::Month) == rec_faces
        && book.total_recs(Granularity::Day) == rec_count
        && book.total_recs(Granularity::Month) == rec_count;

    let digest = book.digest() == snapshot(recs).digest();

    Recon {
        buckets,
        matched,
        keys,
        totals,
        digest,
        drift: SettleCode::Unknown,
    }
}

/// 落盘—回放往返：内容相等、字节再编码相等。
pub fn roundtrip_probe() -> bool {
    let book = snapshot(&SAMPLE_RECORDS);
    let text = book.persist();
    match RollupBook::restore(&text) {
        Some(back) => {
            back.digest() == book.digest()
                && back.nd == book.nd
                && back.nm == book.nm
                && back.persist() == text
        }
        None => false,
    }
}

/// 摘要篡改：改掉校验和、丢字段、换粒度标签，三种都必须被拒且拒绝原因正确。
pub fn tamper_probe() -> bool {
    let book = snapshot(&SAMPLE_RECORDS);
    let mut line = String::new();
    let cap = book.capacity(Granularity::Day);
    let mut i = 0usize;
    while i < cap {
        if let Some(b) = book.bucket_at(Granularity::Day, i) {
            line = encode(Granularity::Day, b);
            break;
        }
        i += 1;
    }
    if line.is_empty() || line.len() < 18 {
        return false;
    }

    // 去掉「分隔符 + 16 位摘要」，剩下的正好是 7 个字段。
    let head = &line[..line.len() - 17];

    // 1) 校验和清零（补回分隔符与 16 位宽度，字段仍可解析）。
    let zero_sum = format!("{}|0000000000000000", head);
    let c1 = decode_reason(&zero_sum) == Err(SettleCode::Checksum);

    // 2) 少一个字段（只剩 7 段）。
    let c2 = decode_reason(head) == Err(SettleCode::EncodeField);

    // 3) 粒度标签换成月结，键仍是 8 位日键。
    let mut swapped = String::new();
    swapped.push('M');
    swapped.push_str(&line[1..]);
    let c3 = decode_reason(&swapped) == Err(SettleCode::KeyRange);

    c1 && c2 && c3
}

/// 插入顺序无关性：正序汇总与逆序汇总必须得到同一本账。
pub fn order_probe() -> bool {
    let a = snapshot(&SAMPLE_RECORDS);
    let mut b = RollupBook::new();
    let mut i = SAMPLE_RECORDS.len();
    while i > 0 {
        i -= 1;
        if let Some(r) = SAMPLE_RECORDS.get(i).copied() {
            if !b.insert(Granularity::Day, r) || !b.insert(Granularity::Month, r) {
                return false;
            }
        }
    }
    a.digest() == b.digest()
        && a.nd == b.nd
        && a.nm == b.nm
        && a.persist() == b.persist()
}

/// 桶位耗尽必须显式拒绝，而不是覆盖旧桶。
pub fn capacity_probe() -> bool {
    let g = Granularity::Day;
    let mut book = RollupBook::new();
    let rec = SAMPLE_RECORDS[0];
    let mut filled = 0usize;
    let mut i = 0usize;
    while i < DAILY_SLOTS {
        let key = 20260101 + i as u32;
        if book.insert_raw(g, key, rec) {
            filled += 1;
        }
        i += 1;
    }
    let refused = !book.insert_raw(g, 20260201, rec);
    filled == DAILY_SLOTS && refused && book.used_count(g) == DAILY_SLOTS
}

/// 闰年与月末：真实月长判定，不允许 2 月 30 日或非闰年 2 月 29 日入账。
pub fn leap_probe() -> bool {
    let leap = CivilDate::new(2024, 2, 29);
    let non = CivilDate::new(2026, 2, 29);
    let feb30 = CivilDate::new(2026, 2, 30);
    let apr31 = CivilDate::new(2026, 4, 31);
    let apr30 = CivilDate::new(2026, 4, 30);
    leap.valid()
        && !non.valid()
        && !feb30.valid()
        && !apr31.valid()
        && apr30.valid()
        && days_in_month(2000, 2) == 29
        && days_in_month(1900, 2) == 28
        && CivilDate::from_day_key(20260229).is_none()
        && CivilDate::from_month_key(202613).is_none()
        && CivilDate::from_day_key(leap.day_key()) == Some(leap)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50205_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50205");

    // 50205-J1-1 诊断码闭集
    let mut code_ok = ALL_CODES.len() == 11;
    let mut i = 0usize;
    while i < ALL_CODES.len() {
        if let Some(c) = ALL_CODES.get(i) {
            if c.val() != SEG_BASE + 1 + i as u32 || c.detail().is_empty() {
                code_ok = false;
            }
            let mut j = 0usize;
            while j < i {
                if let Some(p) = ALL_CODES.get(j) {
                    if p.val() == c.val() || p.detail() == c.detail() {
                        code_ok = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    s.add("50205-J1-诊断码闭集", code_ok, "11 条码连续、文案唯一且非空");

    // 50205-J1-2 样本基数：30 条、id 连续、逐条可接纳
    let mut sample_ok = SAMPLE_RECORDS.len() == SAMPLE_COUNT;
    let mut i = 0usize;
    while i < SAMPLE_COUNT {
        match SAMPLE_RECORDS.get(i) {
            Some(r) => {
                if r.job as usize != i || !r.admissible() {
                    sample_ok = false;
                }
            }
            None => sample_ok = false,
        }
        i += 1;
    }
    s.add("50205-J1-样本基数", sample_ok, "30 条样本 id 连续且可接纳");

    // 50205-J1-3 日期合法性：真实月长 + 闰年 + 键位还原
    s.add(
        "50205-J1-日期合法性",
        leap_probe(),
        "闰年/月末/假日期逐项命中",
    );

    // 50205-J1-4 键位一致性：键与日期双向可逆，非法键一律 None
    let mut keys_ok = true;
    let mut i = 0usize;
    while i < SAMPLE_COUNT {
        if let Some(r) = SAMPLE_RECORDS.get(i) {
            let dk = r.date.day_key();
            let mk = r.date.month_key();
            if CivilDate::from_day_key(dk) != Some(r.date)
                || CivilDate::from_month_key(mk).map(|d| (d.year, d.month)) != Some((r.date.year, r.date.month))
                || !Granularity::Day.valid_key(dk)
                || !Granularity::Month.valid_key(mk)
            {
                keys_ok = false;
            }
        }
        i += 1;
    }
    s.add("50205-J1-键位一致性", keys_ok, "yyyymmdd/yyyymm 双向可逆");

    // 50205-J1-5 日结聚合：19 个自然日桶，逐桶对重算
    let book = snapshot(&SAMPLE_RECORDS);
    let mut day_buckets = 0usize;
    let mut day_ok = 0usize;
    let mut i = 0usize;
    while i < book.capacity(Granularity::Day) {
        if let Some(b) = book.bucket_at(Granularity::Day, i) {
            day_buckets += 1;
            if expect_bucket(&SAMPLE_RECORDS, Granularity::Day, b.key) == Some(b) {
                day_ok += 1;
            }
        }
        i += 1;
    }
    s.add(
        "50205-J1-日结聚合",
        day_buckets == 19 && day_ok == day_buckets && book.nd == 19,
        "19 个自然日桶逐桶与逐任务账一致",
    );

    // 50205-J1-6 月结聚合：3 个月份桶，逐桶对重算
    let mut mon_buckets = 0usize;
    let mut mon_ok = 0usize;
    let mut i = 0usize;
    while i < book.capacity(Granularity::Month) {
        if let Some(b) = book.bucket_at(Granularity::Month, i) {
            mon_buckets += 1;
            if expect_bucket(&SAMPLE_RECORDS, Granularity::Month, b.key) == Some(b) {
                mon_ok += 1;
            }
        }
        i += 1;
    }
    s.add(
        "50205-J1-月结聚合",
        mon_buckets == 3 && mon_ok == mon_buckets && book.nm == 3,
        "3 个月份桶逐桶与逐任务账一致",
    );

    // 50205-J1-7 汇总守恒：日结和 == 月结和 == 逐任务和
    let mut rec_faces = 0u64;
    let mut i = 0usize;
    while i < SAMPLE_COUNT {
        if let Some(r) = SAMPLE_RECORDS.get(i) {
            rec_faces = rec_faces.saturating_add(r.faces());
        }
        i += 1;
    }
    let totals_ok = book.total_faces(Granularity::Day) == rec_faces
        && book.total_faces(Granularity::Month) == rec_faces
        && rec_faces > 0;
    s.add(
        "50205-J1-汇总守恒",
        totals_ok,
        "日结、月结、逐任务三处面数合计相等",
    );

    // 50205-J1-8 计数守恒：桶内条数之和 == 30（两本账）
    let counts_ok = book.total_recs(Granularity::Day) == SAMPLE_COUNT as u64
        && book.total_recs(Granularity::Month) == SAMPLE_COUNT as u64;
    s.add("50205-J1-计数守恒", counts_ok, "两本账条数之和均为 30");

    // 50205-J1-9 编码往返：全部 22 桶编码—解码—再编码字节相等
    s.add(
        "50205-J1-编码往返",
        roundtrip_probe(),
        "落盘文本回放后摘要与字节均一致",
    );

    // 50205-J1-10 篡改检测：改摘要/丢字段/换粒度三种必拒且原因正确
    s.add(
        "50205-J1-篡改检测",
        tamper_probe(),
        "Checksum/EncodeField/KeyRange 三类拒绝命中",
    );

    // 50205-J1-11 乱序稳定：逆序汇总量与字节一致
    s.add(
        "50205-J1-乱序稳定",
        order_probe(),
        "正序与逆序汇总产出同一本账",
    );

    // 50205-J1-12 容量防护：满 40 桶后拒绝而非覆盖
    s.add(
        "50205-J1-容量防护",
        capacity_probe(),
        "日结满 40 桶后显式拒绝第 41 桶",
    );

    // 50205-J1-13 三向勾稽／段位
    let recon = reconcile(&SAMPLE_RECORDS, &book);
    let seg_ok = SEG_BASE % 0x40 == 0
        && SAMPLE_RECORDS.len() <= DAILY_SLOTS
        && recon.clean()
        && recon.buckets == 22
        && recon.matched == 22;
    s.add(
        "50205-J1-三向勾稽",
        seg_ok,
        "22 桶全匹配、覆盖完整、总量守恒、摘要一致",
    );

    // 动态明细（≤3 条）
    s.add_dyn(
        "50205-J1-日结明细",
        day_ok == day_buckets && day_buckets == 19,
        &format!("日结桶={} 匹配={} 条数={}", day_buckets, day_ok, book.total_recs(Granularity::Day)),
    );
    s.add_dyn(
        "50205-J1-月结明细",
        mon_ok == mon_buckets && mon_buckets == 3,
        &format!("月结桶={} 匹配={} 面数={}", mon_buckets, mon_ok, book.total_faces(Granularity::Month)),
    );
    s.add_dyn(
        "50205-J1-摘要明细",
        book.digest() != 0 && totals_ok,
        &format!(
            "账摘要={:016x} 段基址={:#x} 面数={}",
            book.digest(),
            SEG_BASE,
            rec_faces
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50205_all_green() {
        let set = run_unx50205_checks();
        assert!(
            set.all_passed(),
            "UNX-F50205 red: {}",
            set.red_items_text()
        );
    }

    #[test]
    fn unx50205_leap_and_bounds() {
        assert!(leap_probe());
        let mut book = RollupBook::new();
        let bad = BillRecord::new(0, CivilDate::new(2026, 2, 29), 1, 1, 1, 1);
        assert!(!book.insert(Granularity::Day, bad));
        let zero = BillRecord::new(0, CivilDate::new(2026, 2, 28), 0, 0, 0, 1);
        assert!(!book.insert(Granularity::Day, zero));
        assert_eq!(book.used_count(Granularity::Day), 0);
    }

    #[test]
    fn unx50205_persistence_roundtrip() {
        let book = snapshot(&SAMPLE_RECORDS);
        let text = book.persist();
        assert_eq!(text.lines().count(), 22);
        let back = RollupBook::restore(&text).expect("restore");
        assert_eq!(back.digest(), book.digest());
        assert_eq!(back.persist(), text);
        assert!(RollupBook::restore(&format!("{}\n{}\n", text, text)).is_none());
    }

    #[test]
    fn unx50205_reconcile_flags_drift() {
        let book = snapshot(&SAMPLE_RECORDS);
        assert!(reconcile(&SAMPLE_RECORDS, &book).clean());

        // 只汇总日结、漏掉月结：覆盖性必须报警。
        let mut partial = RollupBook::new();
        let mut i = 0usize;
        while i < SAMPLE_COUNT {
            if let Some(r) = SAMPLE_RECORDS.get(i).copied() {
                assert!(partial.insert(Granularity::Day, r));
            }
            i += 1;
        }
        let bad = reconcile(&SAMPLE_RECORDS, &partial);
        assert!(!bad.clean());
        assert_eq!(bad.reason(), SettleCode::BucketMissing);

        let empty = RollupBook::new();
        assert_eq!(
            reconcile(&SAMPLE_RECORDS, &empty).reason(),
            SettleCode::BucketMissing
        );
    }
}
