//! UNX-F50058 B23 批向量账骨架（水印用例集生成器 + 校验和）（0x43360 段）
//!
//! 锚点：UNX-F50058-J1 用例带校验和落账；判据锚定 §AI-63 §5。
//!
//! 数据段：`SEG_BASE = 0x43360`；诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 9`，
//! 逐项见 [`VecError`]。
//!
//! 向量账解决的是「水印用例集合能不能被篡改而无人发现」。生成器按
//! 「落位锚点 × 透明度档 × 正放/转 90」三维笛卡尔积产出用例，每条用例的
//! 期望覆盖量由**几何闭式**算出（戳记外接框面积 × 透明度 ÷ 页面积），
//! 并带一枚 FNV-1a 校验和；落账时只接受「字段合法 + 期望值可复算 + 校验和
//! 自洽」的条目，入账后任何字段漂移都会被 [`CaseBook::verify`] 全表重算抓出。
//!
//! 为什么覆盖量用整数「十万分之一页面积」而不是百分比浮点：判据要拿它做
//! 单调与相等断言，整除口径在两次运行、两台机器上逐位一致；百分比四舍五入
//! 会把 0.4% 与 0.6% 抹成同一个 1%，「转 90 覆盖不变」这类性质就测不出来。
//!
//! 结构清单：
//! * [`Anchor`] + [`ANCHORS`]：8 个落位锚点注册表（per-mille 坐标）；
//! * [`WmCase`]：用例五件套（id / 锚点 / 透明度 / 旋转 / 期望覆盖）+ 校验和；
//! * [`LedgerEntry`] + [`CaseBook`]：定容向量账（入账 / 复算 / 指纹 / 重标定）；
//! * [`gen_cases`]：确定性用例集生成器（锚点码与透明度档先过闭集校验）；
//! * [`injection_core`]：7 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43360 独占 0x20）。
pub const SEG_BASE: u32 = 0x43360;

const E_EMPTY: u16 = 1;
const E_FULL: u16 = 2;
const E_DUPLICATE: u16 = 3;
const E_BAD_SUM: u16 = 4;
const E_BAD_ANCHOR: u16 = 5;
const E_RANGE: u16 = 6;
const E_DRIFT: u16 = 7;
const E_EXPECT: u16 = 8;
const E_GRID: u16 = 9;

/// 向量账错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VecError {
    /// 空锚点集 / 空透明度集 / 空用例集 / 空账。
    Empty,
    /// 账本容量已满。
    Full,
    /// 用例 id 重复。
    Duplicate,
    /// 声称校验和与用例自算值不符。
    BadSum,
    /// 锚点码不在闭集 0..=7。
    BadAnchor,
    /// 字段越界（id=0、旋转 >1、透明度 >95）。
    Range,
    /// 全表重算发现条目漂移。
    Drift,
    /// 期望覆盖量与本用例几何不符。
    Expect,
    /// 透明度不落在 5% 网格上。
    Grid,
}

impl VecError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            VecError::Empty => E_EMPTY,
            VecError::Full => E_FULL,
            VecError::Duplicate => E_DUPLICATE,
            VecError::BadSum => E_BAD_SUM,
            VecError::BadAnchor => E_BAD_ANCHOR,
            VecError::Range => E_RANGE,
            VecError::Drift => E_DRIFT,
            VecError::Expect => E_EXPECT,
            VecError::Grid => E_GRID,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            VecError::Empty => "empty anchor/opacity/case set",
            VecError::Full => "case book capacity reached",
            VecError::Duplicate => "case id already present",
            VecError::BadSum => "case checksum mismatch",
            VecError::BadAnchor => "anchor code outside closed set",
            VecError::Range => "case field out of range",
            VecError::Drift => "ledger entry drifted from its checksum",
            VecError::Expect => "expected coverage does not match case geometry",
            VecError::Grid => "opacity not on the 5% grid",
        }
    }
}

/// 戳记基准尺寸（mm）：60×20 是 A4 对角水印的常规外接框，转 90 后互换。
pub const STAMP_W_MM: u32 = 60;
/// 戳记基准高度（mm）。
pub const STAMP_H_MM: u32 = 20;

/// 参照页面积（mm²）：A4 210×297。覆盖量一律按此页折算，换纸型由调用方
/// 另立账本——混算会让「同一条用例」在两种纸型上给出两个期望值。
pub const PAGE_AREA_MM2: u32 = 210 * 297;

/// 覆盖量单位：十万分之一页面积（0..=100_000）。
pub const COVER_UNIT: u32 = 100_000;

/// 透明度网格步长（%）。5% 是 UI 档位与印前打样共同承认的最小可分辨步。
pub const OPACITY_STEP: u8 = 5;

/// 落位锚点。坐标用 per-mille 表示中心点（0=左/上，1000=右/下），
/// 整数坐标让「锚点表唯一」成为可判定的性质（浮点相等不可判定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// 左上。
    TopLeft,
    /// 上中。
    TopCenter,
    /// 右上。
    TopRight,
    /// 左中。
    MidLeft,
    /// 正中。
    Center,
    /// 右中。
    MidRight,
    /// 下中。
    BottomCenter,
    /// 平铺（覆盖整页）。
    Tile,
}

impl Anchor {
    /// 锚点码 0..=7。
    pub fn code(self) -> u8 {
        match self {
            Anchor::TopLeft => 0,
            Anchor::TopCenter => 1,
            Anchor::TopRight => 2,
            Anchor::MidLeft => 3,
            Anchor::Center => 4,
            Anchor::MidRight => 5,
            Anchor::BottomCenter => 6,
            Anchor::Tile => 7,
        }
    }

    /// 锚点码反解；闭集外返回 [`VecError::BadAnchor`]，不兜底到正中。
    pub fn from_code(c: u8) -> Result<Anchor, VecError> {
        match c {
            0 => Ok(Anchor::TopLeft),
            1 => Ok(Anchor::TopCenter),
            2 => Ok(Anchor::TopRight),
            3 => Ok(Anchor::MidLeft),
            4 => Ok(Anchor::Center),
            5 => Ok(Anchor::MidRight),
            6 => Ok(Anchor::BottomCenter),
            7 => Ok(Anchor::Tile),
            _ => Err(VecError::BadAnchor),
        }
    }

    /// 锚点中心坐标（per-mille）。
    pub fn xy(self) -> (u16, u16) {
        match self {
            Anchor::TopLeft => (150, 120),
            Anchor::TopCenter => (500, 120),
            Anchor::TopRight => (850, 120),
            Anchor::MidLeft => (150, 500),
            Anchor::Center => (500, 500),
            Anchor::MidRight => (850, 500),
            Anchor::BottomCenter => (500, 880),
            Anchor::Tile => (500, 500),
        }
    }

    /// 是否铺满整页（平铺锚点覆盖语义与单点戳记不同）。
    pub fn is_tile(self) -> bool {
        matches!(self, Anchor::Tile)
    }

    /// 锚点名（日志用）。
    pub fn label(self) -> &'static str {
        match self {
            Anchor::TopLeft => "top-left",
            Anchor::TopCenter => "top-center",
            Anchor::TopRight => "top-right",
            Anchor::MidLeft => "mid-left",
            Anchor::Center => "center",
            Anchor::MidRight => "mid-right",
            Anchor::BottomCenter => "bottom-center",
            Anchor::Tile => "tile",
        }
    }

    /// 全部八锚点（遍历用）。
    pub const ALL: [Anchor; 8] = [
        Anchor::TopLeft,
        Anchor::TopCenter,
        Anchor::TopRight,
        Anchor::MidLeft,
        Anchor::Center,
        Anchor::MidRight,
        Anchor::BottomCenter,
        Anchor::Tile,
    ];
}

/// 一条水印向量用例。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WmCase {
    /// 用例号（1 起，账内唯一）。
    pub id: u16,
    /// 落位锚点。
    pub anchor: Anchor,
    /// 透明度百分比（5..=95，落 5% 网格）。
    pub opacity_pct: u8,
    /// 象限旋转：0=正放，1=转 90（只允许 0/1：转 180/270 与正放在对称戳记上
    /// 覆盖量完全相同，收进用例集只会虚增条数）。
    pub rot: u8,
    /// 期望覆盖量（十万分之一页面积）。
    pub expect_cover: u32,
}

impl WmCase {
    /// 按几何闭式修好期望值后构造（避免调用方传错期望值）。
    pub fn fitted(id: u16, anchor: Anchor, opacity_pct: u8, rot: u8) -> WmCase {
        let mut c = WmCase {
            id,
            anchor,
            opacity_pct,
            rot,
            expect_cover: 0,
        };
        c.expect_cover = c.expected_cover();
        c
    }

    /// 戳记外接框（mm）：转 90 后宽高互换，面积不变。
    pub fn bbox(&self) -> (u32, u32) {
        if self.rot == 0 {
            (STAMP_W_MM, STAMP_H_MM)
        } else {
            (STAMP_H_MM, STAMP_W_MM)
        }
    }

    /// 几何闭式期望覆盖量：`面积 × 透明度 × 100000 / (页面积 × 100)`。
    /// 平铺锚点例外——它按定义覆盖整页，覆盖量只随透明度走。
    /// 中间量走 u64：1200 mm² × 95% × 100000 已达 1.14e10，u32 会饱和成
    /// 一个「看起来合理」的大数，把伪造期望值一并放过。
    pub fn expected_cover(&self) -> u32 {
        let op = self.opacity_pct as u64;
        if self.anchor.is_tile() {
            return (op * (COVER_UNIT as u64 / 100)) as u32;
        }
        let (w, h) = self.bbox();
        let num = (w as u64) * (h as u64) * op * (COVER_UNIT as u64);
        let den = (PAGE_AREA_MM2 as u64) * 100;
        if den == 0 {
            return 0;
        }
        (num / den) as u32
    }

    /// 字段闭区间：id 非 0、透明度落 5% 网格且 5..=95、旋转 0/1。
    pub fn fields_ok(&self) -> bool {
        self.id != 0
            && self.opacity_pct >= OPACITY_STEP
            && self.opacity_pct <= 95
            && self.opacity_pct % OPACITY_STEP == 0
            && self.rot <= 1
    }

    /// 期望值可复算（与几何闭式逐位相等）。
    pub fn expect_ok(&self) -> bool {
        self.expect_cover == self.expected_cover()
    }

    /// 完整性 = 字段合法 + 期望值自洽。
    pub fn valid(&self) -> bool {
        self.fields_ok() && self.expect_ok()
    }

    /// 用例校验和：id / 锚点码 / 透明度 / 旋转 / 期望覆盖，逐字节喂入不跳步。
    pub fn checksum(&self) -> u32 {
        let mut h = FNV_OFFSET;
        h = fnv_byte(h, (self.id >> 8) as u8);
        h = fnv_byte(h, self.id as u8);
        h = fnv_byte(h, self.anchor.code());
        h = fnv_byte(h, self.opacity_pct);
        h = fnv_byte(h, self.rot);
        let c = self.expect_cover;
        h = fnv_byte(h, (c >> 24) as u8);
        h = fnv_byte(h, (c >> 16) as u8);
        h = fnv_byte(h, (c >> 8) as u8);
        h = fnv_byte(h, c as u8);
        h
    }

    /// 一行描述（落账日志用）。
    pub fn describe(&self) -> String {
        let mut s = String::from("case#");
        s.push_str(&self.id.to_string());
        s.push_str(&format!(
            " {} op{}% rot{} cover{}",
            self.anchor.label(),
            self.opacity_pct,
            self.rot,
            self.expect_cover
        ));
        s
    }
}

/// FNV-1a 偏移基。
pub const FNV_OFFSET: u32 = 0x811c_9dc5;
/// FNV-1a 质数。
pub const FNV_PRIME: u32 = 0x0100_0193;
/// 单本账容量（4 锚点 × 6 透明度 × 2 旋转 = 48，留 16 槽补测）。
pub const MAX_CASES: usize = 64;

fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ b as u32).wrapping_mul(FNV_PRIME)
}

/// 逐字节续算 FNV-1a（对外暴露，判据可独立复算）。
pub fn fnv_bytes(seed: u32, bytes: &[u8]) -> u32 {
    let mut h = seed;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = fnv_byte(h, *b);
        }
        i += 1;
    }
    h
}

/// 账内一条记录：用例本体 + 入账时算出的校验和 + 入账序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerEntry {
    /// 用例本体。
    pub case: WmCase,
    /// 入账校验和（必须等于 `case.checksum()`）。
    pub sum: u32,
    /// 入账序（0 起）。
    pub seq: u16,
}

/// 定容向量账。入账即校验：字段 / 期望值 / 校验和 / 唯一性 / 容量，
/// 任一不过就不入账——账面上不存在「差一点的用例」。
pub struct CaseBook {
    entries: Vec<LedgerEntry>,
    capacity: usize,
    next_seq: u16,
    rejected: u32,
}

impl CaseBook {
    /// 建空账；容量 0 按 1 处理，上限 1024 防误用。
    pub fn new(capacity: usize) -> CaseBook {
        let cap = if capacity == 0 { 1 } else { capacity.min(1024) };
        CaseBook {
            entries: Vec::with_capacity(cap),
            capacity: cap,
            next_seq: 0,
            rejected: 0,
        }
    }

    fn reject<T>(&mut self, e: VecError) -> Result<T, VecError> {
        self.rejected = self.rejected.saturating_add(1);
        Err(e)
    }

    /// 按 id 找下标。
    pub fn find(&self, id: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            match self.entries.get(i) {
                Some(e) if e.case.id == id => return Some(i),
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// 入账：字段合法 → 期望值自洽 → 唯一 → 容量，校验和由账本自算。
    pub fn insert(&mut self, case: WmCase) -> Result<u32, VecError> {
        if !case.fields_ok() {
            return self.reject(VecError::Range);
        }
        if !case.expect_ok() {
            return self.reject(VecError::Expect);
        }
        if self.find(case.id).is_some() {
            return self.reject(VecError::Duplicate);
        }
        if self.entries.len() >= self.capacity {
            return self.reject(VecError::Full);
        }
        let sum = case.checksum();
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.entries.push(LedgerEntry { case, sum, seq });
        Ok(sum)
    }

    /// 带「声称校验和」入账：声称值与自算值不符即拒（防上游伪造）。
    pub fn insert_claimed(&mut self, case: WmCase, claimed: u32) -> Result<u32, VecError> {
        if !case.fields_ok() {
            return self.reject(VecError::Range);
        }
        if claimed != case.checksum() {
            return self.reject(VecError::BadSum);
        }
        self.insert(case)
    }

    /// 重标定：透明度档修订后改某条的透明度（**不改**已入账校验和）。
    /// 这是「档位表改版必须重生成用例」的强制留痕——改完 [`CaseBook::verify`]
    /// 必报 `Drift`，逼调用方把整本账重建，而不是留一条半新半旧的记录。
    pub fn retune(&mut self, id: u16, opacity_pct: u8) -> Result<(), VecError> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(VecError::Range),
        };
        match self.entries.get_mut(idx) {
            Some(e) => {
                e.case.opacity_pct = opacity_pct;
                e.case.expect_cover = e.case.expected_cover();
                Ok(())
            }
            None => Err(VecError::Range),
        }
    }

    /// 全表重算：逐条校验和 + 期望值 + 入账序。任一不符即 `Drift`。
    pub fn verify(&self) -> Result<usize, VecError> {
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = match self.entries.get(i) {
                Some(v) => v,
                None => return Err(VecError::Drift),
            };
            if e.case.checksum() != e.sum || !e.case.expect_ok() || e.seq as usize != i {
                return Err(VecError::Drift);
            }
            i += 1;
        }
        Ok(self.entries.len())
    }

    /// 账面指纹：入账序串 id 与校验和。顺序敏感——换序即换指纹。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = fnv_byte(h, (e.case.id >> 8) as u8);
                h = fnv_byte(h, e.case.id as u8);
                h = fnv_byte(h, (e.sum >> 24) as u8);
                h = fnv_byte(h, (e.sum >> 16) as u8);
                h = fnv_byte(h, (e.sum >> 8) as u8);
                h = fnv_byte(h, e.sum as u8);
            }
            i += 1;
        }
        h
    }

    /// 账面条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 累计被拒次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 取第 `i` 条（越界 `None`）。
    pub fn entry(&self, i: usize) -> Option<&LedgerEntry> {
        self.entries.get(i)
    }

    /// 按 id 取用例本体。
    pub fn case(&self, id: u16) -> Option<&WmCase> {
        match self.find(id) {
            Some(i) => self.entries.get(i).map(|e| &e.case),
            None => None,
        }
    }

    /// 期望覆盖量总和（账面守恒用）。
    pub fn total_cover(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                sum = sum.saturating_add(e.case.expect_cover as u64);
            }
            i += 1;
        }
        sum
    }

    /// 导出全部用例（按入账序），供下游回归器消费。
    pub fn cases(&self) -> Vec<WmCase> {
        let mut out = Vec::with_capacity(self.entries.len());
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                out.push(e.case);
            }
            i += 1;
        }
        out
    }
}

/// 用例集生成器：锚点 × 透明度 × 旋转(0,1) 的笛卡尔积。
///
/// 顺序契约：外层锚点（按入参顺序）、中层透明度（按入参顺序）、内层旋转
/// 0→1；id 从 1 连续递增。同样的入参两次生成逐项一致——判据依赖这一点。
pub fn gen_cases(anchors: &[u8], opacities: &[u8]) -> Result<Vec<WmCase>, VecError> {
    if anchors.is_empty() || opacities.is_empty() {
        return Err(VecError::Empty);
    }
    // 先把闭集校验全部做完再产出：半套用例比空集更危险，下游会把
    // 「只测了 3 个锚点」当成「4 个锚点都测了」。
    let mut i = 0usize;
    while i < anchors.len() {
        match anchors.get(i) {
            Some(c) => {
                Anchor::from_code(*c)?;
            }
            None => return Err(VecError::BadAnchor),
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < opacities.len() {
        match opacities.get(j) {
            Some(o) => {
                if *o < OPACITY_STEP || *o > 95 || *o % OPACITY_STEP != 0 {
                    return Err(VecError::Grid);
                }
            }
            None => return Err(VecError::Grid),
        }
        j += 1;
    }
    let mut out = Vec::with_capacity(anchors.len() * opacities.len() * 2);
    let mut next_id: u16 = 1;
    i = 0;
    while i < anchors.len() {
        let ac = match anchors.get(i) {
            Some(c) => *c,
            None => return Err(VecError::BadAnchor),
        };
        let anchor = Anchor::from_code(ac)?;
        j = 0;
        while j < opacities.len() {
            let op = match opacities.get(j) {
                Some(o) => *o,
                None => return Err(VecError::Grid),
            };
            let mut r = 0u8;
            while r < 2 {
                out.push(WmCase::fitted(next_id, anchor, op, r));
                next_id = next_id.saturating_add(1);
                r += 1;
            }
            j += 1;
        }
        i += 1;
    }
    Ok(out)
}

/// 把一整套用例灌入账本；任一条被拒即整体失败（错误原样上抛，不吞）。
pub fn fill_book(book: &mut CaseBook, cases: &[WmCase]) -> Result<usize, VecError> {
    if cases.is_empty() {
        return Err(VecError::Empty);
    }
    let mut i = 0usize;
    while i < cases.len() {
        match cases.get(i) {
            Some(c) => {
                book.insert(*c)?;
            }
            None => return Err(VecError::Range),
        }
        i += 1;
    }
    Ok(book.len())
}

/// 造一本标准账：4 锚点 × 3 透明度档 × 2 旋转 = 24 例。
pub fn standard_book() -> Result<CaseBook, VecError> {
    let anchors = [0u8, 1, 2, 6];
    let ops = [10u8, 30, 60];
    let cases = gen_cases(&anchors, &ops)?;
    let mut book = CaseBook::new(MAX_CASES);
    fill_book(&mut book, &cases)?;
    Ok(book)
}

/// 注入核：7 类坏样本，逐类断言被显式拒绝。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 7usize;

    // 1) 空锚点集
    if gen_cases(&[], &[10u8]).err() == Some(VecError::Empty) {
        caught += 1;
    }

    // 2) 锚点码越出闭集（8 是合法上界 7 之外）
    if gen_cases(&[0u8, 8], &[10u8]).err() == Some(VecError::BadAnchor) {
        caught += 1;
    }

    // 3) 透明度不在 5% 网格（7% 是 UI 不承认的档位）
    if gen_cases(&[0u8], &[7u8]).err() == Some(VecError::Grid) {
        caught += 1;
    }

    // 4) 重复 id：第一条入账后第二条同 id 必拒，账长不变
    let mut b = CaseBook::new(4);
    let c1 = WmCase::fitted(1, Anchor::TopLeft, 10, 0);
    let c2 = WmCase::fitted(1, Anchor::Center, 20, 1);
    let first = b.insert(c1).is_ok();
    if first && b.insert(c2).err() == Some(VecError::Duplicate) && b.len() == 1 {
        caught += 1;
    }

    // 5) 篡改：声称校验和与实际差一位
    let mut b2 = CaseBook::new(4);
    let c3 = WmCase::fitted(2, Anchor::Tile, 40, 0);
    if b2.insert_claimed(c3, c3.checksum().wrapping_add(1)).err() == Some(VecError::BadSum) {
        caught += 1;
    }

    // 6) 期望值伪造：几何闭式重算对不上（覆盖量手改成 999）
    let forged = WmCase {
        id: 3,
        anchor: Anchor::TopLeft,
        opacity_pct: 10,
        rot: 0,
        expect_cover: 999,
    };
    let mut b3 = CaseBook::new(4);
    if b3.insert(forged).err() == Some(VecError::Expect) {
        caught += 1;
    }

    // 7) 容量满
    let mut b4 = CaseBook::new(1);
    let a = WmCase::fitted(4, Anchor::Center, 10, 0);
    let c = WmCase::fitted(5, Anchor::Center, 15, 0);
    let ok_a = b4.insert(a).is_ok();
    if ok_a && b4.insert(c).err() == Some(VecError::Full) {
        caught += 1;
    }

    (caught, total)
}

/// UNX-F50058 自检：16 条判据。
pub fn run_unx50058_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50058");
    let anchors = [0u8, 1, 2, 6];
    let ops = [10u8, 30, 60];

    // J1-1 生成规模与 id 连续：4 × 3 × 2 = 24，id 1..=24 无洞。
    let cases = match gen_cases(&anchors, &ops) {
        Ok(v) => v,
        Err(_) => Vec::new(),
    };
    let mut ids_ok = cases.len() == 24;
    let mut i = 0usize;
    while i < cases.len() {
        if let Some(c) = cases.get(i) {
            if c.id as usize != i + 1 {
                ids_ok = false;
            }
        }
        i += 1;
    }
    let det = format!("n={}", cases.len());
    s.add_dyn("50058-J1-生成规模", ids_ok, &det);

    // J1-2 生成幂等：同入参两次逐项相等。
    let again = gen_cases(&anchors, &ops);
    let ok = match (&again, cases.is_empty()) {
        (Ok(v), false) => *v == cases,
        _ => false,
    };
    s.add("50058-J1-生成幂等", ok, if ok { "" } else { "两次生成不一致" });

    // J1-3 旋转几何：转 90 外接框宽高互换、覆盖量逐位相等。
    let rot_ok = {
        let a = WmCase::fitted(1, Anchor::Center, 30, 0);
        let b = WmCase::fitted(1, Anchor::Center, 30, 1);
        let (aw, ah) = a.bbox();
        let (bw, bh) = b.bbox();
        aw == bh && ah == bw && a.expect_cover == b.expect_cover && a.checksum() != b.checksum()
    };
    s.add("50058-J1-旋转几何", rot_ok, if rot_ok { "" } else { "旋转改了覆盖量" });

    // J1-4 平铺语义：平铺覆盖量 = 透明度 × 页面积 / 100，与戳记不同径。
    let tile_ok = {
        let t = WmCase::fitted(1, Anchor::Tile, 40, 0);
        let p = WmCase::fitted(1, Anchor::Center, 40, 0);
        t.expect_cover == 40 * (COVER_UNIT / 100)
            && t.expect_cover > p.expect_cover
            && p.expect_cover > 0
    };
    s.add("50058-J1-平铺语义", tile_ok, if tile_ok { "" } else { "平铺未按整页折算" });

    // J1-5 覆盖单调：透明度升 → 覆盖量不减（8 锚点 × 3 档全扫）。
    let mut mono = true;
    let mut a = 0usize;
    while a < Anchor::ALL.len() {
        if let Some(anchor) = Anchor::ALL.get(a) {
            let lo = WmCase::fitted(1, *anchor, 10, 0).expect_cover;
            let mid = WmCase::fitted(1, *anchor, 30, 0).expect_cover;
            let hi = WmCase::fitted(1, *anchor, 60, 0).expect_cover;
            if !(lo < mid && mid < hi) {
                mono = false;
            }
        }
        a += 1;
    }
    s.add("50058-J1-覆盖单调", mono, if mono { "" } else { "覆盖量非单调" });

    // J1-6 期望值自洽：24 条逐条用几何闭式重算，逐位相等。
    let mut self_ok = cases.len() == 24;
    let mut k = 0usize;
    while k < cases.len() {
        if let Some(c) = cases.get(k) {
            if c.expect_cover != c.expected_cover() || !c.fields_ok() {
                self_ok = false;
            }
        }
        k += 1;
    }
    s.add("50058-J1-期望自洽", self_ok, if self_ok { "" } else { "期望值不可复算" });

    // J1-7 锚点表唯一：8 个锚点坐标互不相同（平铺与正中重合是刻意的，
    // 但两者语义不同，故按「坐标 + 平铺标志」做唯一键）。
    let mut unique = true;
    let mut p = 0usize;
    while p < Anchor::ALL.len() {
        let mut q = p + 1;
        while q < Anchor::ALL.len() {
            if let (Some(x), Some(y)) = (Anchor::ALL.get(p), Anchor::ALL.get(q)) {
                if x.xy() == y.xy() && x.is_tile() == y.is_tile() {
                    unique = false;
                }
                let (px, py) = x.xy();
                if px > 1000 || py > 1000 {
                    unique = false;
                }
            }
            q += 1;
        }
        p += 1;
    }
    s.add("50058-J1-锚点表唯一", unique, if unique { "" } else { "锚点重复或越界" });

    // J1-8 校验和字段敏感：改 id / 锚点 / 透明度 / 旋转 / 期望各得不同指纹。
    let base = WmCase::fitted(7, Anchor::MidLeft, 25, 0);
    let m_id = WmCase::fitted(8, Anchor::MidLeft, 25, 0);
    let m_anchor = WmCase::fitted(7, Anchor::MidRight, 25, 0);
    let m_op = WmCase::fitted(7, Anchor::MidLeft, 30, 0);
    let m_rot = WmCase::fitted(7, Anchor::MidLeft, 25, 1);
    let m_expect = WmCase {
        id: 7,
        anchor: Anchor::MidLeft,
        opacity_pct: 25,
        rot: 0,
        expect_cover: base.expect_cover + 1,
    };
    let sums = [
        base.checksum(),
        m_id.checksum(),
        m_anchor.checksum(),
        m_op.checksum(),
        m_rot.checksum(),
        m_expect.checksum(),
    ];
    let mut distinct = true;
    let mut x = 0usize;
    while x < sums.len() {
        let mut y = x + 1;
        while y < sums.len() {
            if let (Some(u), Some(v)) = (sums.get(x), sums.get(y)) {
                if u == v {
                    distinct = false;
                }
            }
            y += 1;
        }
        x += 1;
    }
    s.add("50058-J1-校验和分敏感", distinct, if distinct { "" } else { "字段未进指纹" });

    // J1-9 落账自洽：返回 sum 等于自算值、seq 从 0 递增、逐条 verify 通过。
    let mut book = CaseBook::new(MAX_CASES);
    let filled = fill_book(&mut book, &cases);
    let per_entry = {
        let mut ok = true;
        let mut j = 0usize;
        while j < book.len() {
            match book.entry(j) {
                Some(e) => {
                    if e.case.checksum() != e.sum || e.seq as usize != j {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            j += 1;
        }
        ok
    };
    let ok = match filled {
        Ok(n) => n == 24,
        Err(_) => false,
    } && per_entry
        && book.verify() == Ok(24)
        && book.entry(0).map(|e| e.sum) == cases.first().map(|c| c.checksum());
    let det = format!("n={} dg={:#010x}", book.len(), book.digest());
    s.add_dyn("50058-J1-落账自洽", ok, &det);

    // J1-10 重复 id 必拒，账长与指纹都不动。
    let len_before = book.len();
    let dg_before = book.digest();
    let dup = WmCase::fitted(1, Anchor::TopLeft, 10, 0);
    let ok = book.insert(dup).err() == Some(VecError::Duplicate)
        && book.len() == len_before
        && book.digest() == dg_before;
    s.add("50058-J1-防重复入账", ok, if ok { "" } else { "重复 id 进了账" });

    // J1-11 篡改必拒：同 id 改透明度 + 冒用旧校验和。
    let forged_sum = match book.case(2) {
        Some(c) => c.checksum(),
        None => 0,
    };
    let tampered = WmCase::fitted(2, Anchor::Tile, 90, 0);
    let ok = book.insert_claimed(tampered, forged_sum).err() == Some(VecError::BadSum)
        && book.len() == len_before
        && book.rejected() >= 1;
    s.add("50058-J1-篡改必拒", ok, if ok { "" } else { "篡改条目被接受" });

    // J1-12 定容不破：容量 2 的账收满后第三条 Full。
    let mut small = CaseBook::new(2);
    let sa = WmCase::fitted(10, Anchor::TopCenter, 10, 0);
    let sb = WmCase::fitted(11, Anchor::TopCenter, 15, 0);
    let sc = WmCase::fitted(12, Anchor::TopCenter, 20, 0);
    let ok = small.insert(sa).is_ok()
        && small.insert(sb).is_ok()
        && small.insert(sc).err() == Some(VecError::Full)
        && small.len() == small.capacity()
        && small.total_cover() == sa.expect_cover as u64 + sb.expect_cover as u64;
    let det = format!("n={} cap={} cover={}", small.len(), small.capacity(), small.total_cover());
    s.add_dyn("50058-J1-定容守恒", ok, &det);

    // J1-13 指纹有序：标准账与逐条重建同指纹，倒序灌账必不同指纹。
    let book2 = match standard_book() {
        Ok(v) => v,
        Err(_) => CaseBook::new(1),
    };
    let mut rev = CaseBook::new(MAX_CASES);
    let mut r = cases.len();
    while r > 0 {
        if let Some(c) = cases.get(r - 1) {
            let _ = rev.insert(*c);
        }
        r -= 1;
    }
    let ok = book2.len() == 24
        && book2.digest() == book.digest()
        && rev.digest() != book.digest()
        && rev.len() == book.len();
    let det = format!("d1={:#010x} d2={:#010x}", book.digest(), rev.digest());
    s.add_dyn("50058-J1-指纹有序", ok, &det);

    // J1-14 重标定留痕：改透明度后校验和与期望值脱钩，verify 必报 Drift。
    let mut re = match standard_book() {
        Ok(v) => v,
        Err(_) => CaseBook::new(1),
    };
    let before = re.verify();
    let tuned = re.retune(1, 55);
    let after = re.verify();
    let ok = before == Ok(24) && tuned.is_ok() && after.err() == Some(VecError::Drift);
    let det = format!("before={:?} after_drift={}", before, after.is_err());
    s.add_dyn("50058-J1-重标定留痕", ok, &det);

    // J1-15 空/越界入参显式：空锚集、空透明度集、空用例集、空账、越界字段。
    let bad_id = WmCase {
        id: 0,
        anchor: Anchor::Center,
        opacity_pct: 10,
        rot: 0,
        expect_cover: 0,
    };
    let bad_rot = WmCase {
        id: 9,
        anchor: Anchor::Center,
        opacity_pct: 10,
        rot: 2,
        expect_cover: WmCase::fitted(9, Anchor::Center, 10, 0).expect_cover,
    };
    let mut probe = CaseBook::new(4);
    let ok = gen_cases(&[], &[10u8]).err() == Some(VecError::Empty)
        && gen_cases(&[0u8], &[]).err() == Some(VecError::Empty)
        && fill_book(&mut CaseBook::new(4), &[]).err() == Some(VecError::Empty)
        && probe.insert(bad_id).err() == Some(VecError::Range)
        && probe.insert(bad_rot).err() == Some(VecError::Range)
        && probe.insert(WmCase::fitted(9, Anchor::Center, 7, 0)).err() == Some(VecError::Range)
        && probe.is_empty()
        && CaseBook::new(4).verify() == Ok(0);
    s.add("50058-J1-非法必拒", ok, if ok { "" } else { "非法入参被接受" });

    // J1-16 七类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50058-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50058_checks_green() {
        let set = run_unx50058_checks();
        assert!(set.all_passed(), "UNX-F50058 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50058_generator_deterministic() {
        let a = gen_cases(&[0u8, 1, 2, 6], &[10u8, 30, 60]);
        let b = gen_cases(&[0u8, 1, 2, 6], &[10u8, 30, 60]);
        assert!(a.is_ok() && b.is_ok());
        if let (Ok(x), Ok(y)) = (a, b) {
            assert_eq!(x, y);
            assert_eq!(x.len(), 24);
        }
    }

    #[test]
    fn unx50058_rotation_preserves_cover() {
        let a = WmCase::fitted(1, Anchor::MidRight, 45, 0);
        let b = WmCase::fitted(1, Anchor::MidRight, 45, 1);
        assert_eq!(a.expect_cover, b.expect_cover);
        assert_ne!(a.checksum(), b.checksum());
        let (aw, ah) = a.bbox();
        let (bw, bh) = b.bbox();
        assert_eq!((aw, ah), (bh, bw));
    }

    #[test]
    fn unx50058_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 7);
    }
}
