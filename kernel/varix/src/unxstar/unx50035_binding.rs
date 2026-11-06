//! UNX-F50035 骑马钉与胶装两种册序差异显式登记（0x43080 段）
//!
//! 锚点：UNX-F50035-J1 两序差异账逐页落对；判据锚定 §AI-63 §2 专题二。
//!
//! 数据段：`SEG_BASE = 0x43080`，诊断码闭集 `0x43080 + 1..=0x43080 + 7`。
//!
//! 结构清单：
//! - 两种册序：`BindingKind::{SaddleStitch, PerfectBound}`。骑马钉以「折帖」为单位，
//!   每帖 4 页（一面对折即两个版面）：第 k 帖正面 `[N-2k, 2k+1]`、反面
//!   `[2k+2, N-2k-1]`，N 为 4 的补白页数；胶装以「叶」为单位，页面严格顺序：
//!   第 j 叶正面 `2j+1`、反面 `2j+2`。两种册序共用 `Slot{sheet,side,index}` 坐标，
//!   sheet 对骑马钉是折帖号、对胶装是叶号（全局顺序），由此可直接逐页比对；
//! - 印版账：`Imposition` 保存两侧面表 `SheetSide` 与**逐页槽位表**
//!   `slots[page-1]`。槽位表是 J1 的落点：既可由面表反推，也可用闭式公式
//!   `saddle_slot_of / perfect_slot_of` 独立重算，两路必须逐页一致；
//! - 差异账：`PageDiff` 每页一行（含补白页），记两种册序各自的槽位与 differs
//!   标记；`audit_diff` 复算标记并要求行序严格 1..=N；
//! - 审计：`audit_slots` 依次检查缺页（RowMissing）、槽位重复（RowDup）、
//!   闭式不一致（Mismatch）。三类码各有可复现的篡改样本，不是统一兜底；
//! - 注入核：`injection_core` 六类坏输入/坏账逐类断言诊断码。
//!
//! 为什么补白必须进账：骑马钉页数不整除 4 时最后几页是空白，空白页同样占据
//! 印版槽位；若把它们从账里删掉，「逐页落对」就退化成「逐实页落对」，
//! 装订工单会在折帖上错位。故 `padded = 4×ceil(pages/4)`，空白单列计数。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段基址（0x43080 段，独占 0x20）。
pub const SEG_BASE: u32 = 0x43080;

/// 单册页数上限：4096 页 = 1024 折帖，已超出常规书册；再大应改分卷模型。
pub const MAX_PAGES: u16 = 4096;
/// 胶装默认帖容量（页）：16 页一帖是书刊胶装的常见模数。
pub const DEFAULT_SIGNATURE: u8 = 16;
/// 允许的帖容量：必须是偶数且能整除印张；4/8/16/32 四档。
pub const SIG_SIZES: [u8; 4] = [4, 8, 16, 32];

const E_EMPTY: u16 = 1;
const E_TOOMANY: u16 = 2;
const E_SIGNATURE: u16 = 3;
const E_UNKNOWN: u16 = 4;
const E_MISSING: u16 = 5;
const E_DUP: u16 = 6;
const E_MISMATCH: u16 = 7;

/// 册序域错误七型，`code()` 恒为 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindError {
    Empty,
    TooMany,
    BadSignature,
    UnknownPage,
    RowMissing,
    RowDup,
    Mismatch,
}

impl BindError {
    pub fn code(self) -> u32 {
        let n = match self {
            BindError::Empty => E_EMPTY,
            BindError::TooMany => E_TOOMANY,
            BindError::BadSignature => E_SIGNATURE,
            BindError::UnknownPage => E_UNKNOWN,
            BindError::RowMissing => E_MISSING,
            BindError::RowDup => E_DUP,
            BindError::Mismatch => E_MISMATCH,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            BindError::Empty => "page count zero",
            BindError::TooMany => "page count above 4096",
            BindError::BadSignature => "signature size not in {4,8,16,32}",
            BindError::UnknownPage => "page outside 1..=padded",
            BindError::RowMissing => "page has no imposition slot",
            BindError::RowDup => "two pages share one slot",
            BindError::Mismatch => "slot/ledger row disagrees with closed form",
        }
    }
}

/// 装订方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingKind {
    SaddleStitch,
    PerfectBound,
}

impl BindingKind {
    pub fn code(self) -> u8 {
        match self {
            BindingKind::SaddleStitch => 0,
            BindingKind::PerfectBound => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BindingKind::SaddleStitch => "saddle-stitch",
            BindingKind::PerfectBound => "perfect-bound",
        }
    }
}

/// 印张的正面 / 反面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Front,
    Back,
}

impl Side {
    pub fn code(self) -> u8 {
        match self {
            Side::Front => 0,
            Side::Back => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Side::Front => "front",
            Side::Back => "back",
        }
    }
}

/// 页在册序中的位置。骑马钉：sheet=折帖号、index∈{0,1}；
/// 胶装：sheet=叶号（全局顺序）、index 恒 0。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub sheet: u16,
    pub side: Side,
    pub index: u8,
}

impl Slot {
    pub const fn new(sheet: u16, side: Side, index: u8) -> Slot {
        Slot { sheet, side, index }
    }
}

/// 一个印版面：骑马钉每面 2 页（count=2），胶装每面 1 页（count=1，pages[1]=0）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SheetSide {
    pub sheet: u16,
    pub side: Side,
    pub pages: [u16; 2],
    pub count: u8,
}

/// 一本册子的印版账。
#[derive(Clone, Debug, PartialEq)]
pub struct Imposition {
    pub kind: BindingKind,
    pub padded: u16,
    pub blanks: u16,
    pub sides: Vec<SheetSide>,
    pub slots: Vec<Option<Slot>>,
}

/// 逐页差异行：两序各自的槽位 + 是否不同 + 是否补白。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageDiff {
    pub page: u16,
    pub blank: bool,
    pub saddle: Slot,
    pub perfect: Slot,
    pub differs: bool,
}

fn pad4(pages: u16) -> u16 {
    let bumped = pages as u32 + 3;
    ((bumped / 4) * 4) as u16
}

/// 骑马钉闭式槽位。四段推导（N = padded，k 从 0 起）：
/// 正面 `[N-2k, 2k+1]`、反面 `[2k+2, N-2k-1]`；页 p 落在哪一段由 p 与 N/2、
/// 奇偶性唯一确定——这就是「逐页落对」的独立第二路。
pub fn saddle_slot_of(page: u16, padded: u16) -> Result<Slot, BindError> {
    if padded == 0 || padded % 4 != 0 || page == 0 || page > padded {
        return Err(BindError::UnknownPage);
    }
    let n = padded;
    let half = n / 2;
    if page > half {
        if (n - page) % 2 == 0 {
            Ok(Slot::new((n - page) / 2, Side::Front, 0))
        } else {
            Ok(Slot::new((n - page - 1) / 2, Side::Back, 1))
        }
    } else if page % 2 == 1 {
        Ok(Slot::new((page - 1) / 2, Side::Front, 1))
    } else {
        Ok(Slot::new((page - 2) / 2, Side::Back, 0))
    }
}

/// 胶装闭式槽位：叶号 `(p-1)/2`，奇数页在正面、偶数页在反面。
pub fn perfect_slot_of(page: u16, padded: u16) -> Result<Slot, BindError> {
    if padded == 0 || padded % 2 != 0 || page == 0 || page > padded {
        return Err(BindError::UnknownPage);
    }
    let leaf = (page - 1) / 2;
    if page % 2 == 1 {
        Ok(Slot::new(leaf, Side::Front, 0))
    } else {
        Ok(Slot::new(leaf, Side::Back, 0))
    }
}

fn assign(slots: &mut Vec<Option<Slot>>, page: u16, slot: Slot) -> Result<(), BindError> {
    let idx = match page.checked_sub(1) {
        Some(v) => v as usize,
        None => return Err(BindError::UnknownPage),
    };
    match slots.get_mut(idx) {
        Some(cell) => {
            if cell.is_some() {
                return Err(BindError::RowDup);
            }
            *cell = Some(slot);
            Ok(())
        }
        None => Err(BindError::UnknownPage),
    }
}

/// 生成印版账：先按面表铺页，再由 `audit_slots` 复算槽位与闭式是否同构。
pub fn impose(kind: BindingKind, pages: u16) -> Result<Imposition, BindError> {
    if pages == 0 {
        return Err(BindError::Empty);
    }
    if pages > MAX_PAGES {
        return Err(BindError::TooMany);
    }
    let padded = pad4(pages);
    let blanks = padded - pages;
    let mut slots: Vec<Option<Slot>> = Vec::with_capacity(padded as usize);
    let mut i = 0u16;
    while i < padded {
        slots.push(None);
        i += 1;
    }
    let mut sides = Vec::new();

    match kind {
        BindingKind::SaddleStitch => {
            let sheets = padded / 4;
            let mut k = 0u16;
            while k < sheets {
                let left = padded - 2 * k;
                let right = 2 * k + 1;
                sides.push(SheetSide {
                    sheet: k,
                    side: Side::Front,
                    pages: [left, right],
                    count: 2,
                });
                assign(&mut slots, left, Slot::new(k, Side::Front, 0))?;
                assign(&mut slots, right, Slot::new(k, Side::Front, 1))?;
                let back_left = 2 * k + 2;
                let back_right = padded - 2 * k - 1;
                sides.push(SheetSide {
                    sheet: k,
                    side: Side::Back,
                    pages: [back_left, back_right],
                    count: 2,
                });
                assign(&mut slots, back_left, Slot::new(k, Side::Back, 0))?;
                assign(&mut slots, back_right, Slot::new(k, Side::Back, 1))?;
                k += 1;
            }
        }
        BindingKind::PerfectBound => {
            let leaves = padded / 2;
            let mut j = 0u16;
            while j < leaves {
                let front = 2 * j + 1;
                let back = 2 * j + 2;
                sides.push(SheetSide {
                    sheet: j,
                    side: Side::Front,
                    pages: [front, 0],
                    count: 1,
                });
                assign(&mut slots, front, Slot::new(j, Side::Front, 0))?;
                sides.push(SheetSide {
                    sheet: j,
                    side: Side::Back,
                    pages: [back, 0],
                    count: 1,
                });
                assign(&mut slots, back, Slot::new(j, Side::Back, 0))?;
                j += 1;
            }
        }
    }

    let mut p = 0u16;
    while p < padded {
        if let Some(None) = slots.get(p as usize) {
            return Err(BindError::RowMissing);
        }
        p += 1;
    }
    let impo = Imposition {
        kind,
        padded,
        blanks,
        sides,
        slots,
    };
    let _ = audit_slots(&impo)?;
    Ok(impo)
}

/// 逐页审计槽位表：缺页 → RowMissing；同槽两页 → RowDup；与闭式不一致 → Mismatch。
pub fn audit_slots(impo: &Imposition) -> Result<u16, BindError> {
    if impo.padded == 0 || impo.slots.len() != impo.padded as usize {
        return Err(BindError::Mismatch);
    }
    let units = match impo.kind {
        BindingKind::SaddleStitch => impo.padded / 4,
        BindingKind::PerfectBound => impo.padded / 2,
    };
    let mut used: Vec<u8> = Vec::with_capacity(units as usize * 4 + 4);
    let mut u = 0usize;
    while u < units as usize * 4 + 4 {
        used.push(0);
        u += 1;
    }
    let mut page = 1u16;
    while page <= impo.padded {
        let slot = match impo.slots.get(page as usize - 1) {
            Some(Some(s)) => *s,
            Some(None) => return Err(BindError::RowMissing),
            None => return Err(BindError::Mismatch),
        };
        if slot.sheet >= units || slot.index > 1 {
            return Err(BindError::Mismatch);
        }
        let pos = slot.sheet as usize * 4 + slot.side.code() as usize * 2 + slot.index as usize;
        if let Some(cell) = used.get_mut(pos) {
            if *cell > 0 {
                return Err(BindError::RowDup);
            }
            *cell += 1;
        } else {
            return Err(BindError::Mismatch);
        }
        page += 1;
    }
    // 闭式交叉校验放在重复检查之后：这样篡改样本能区分 RowDup 与 Mismatch。
    let mut p = 1u16;
    while p <= impo.padded {
        let expect = match impo.kind {
            BindingKind::SaddleStitch => saddle_slot_of(p, impo.padded)?,
            BindingKind::PerfectBound => perfect_slot_of(p, impo.padded)?,
        };
        match impo.slots.get(p as usize - 1) {
            Some(Some(s)) => {
                if *s != expect {
                    return Err(BindError::Mismatch);
                }
            }
            _ => return Err(BindError::RowMissing),
        }
        p += 1;
    }
    Ok(impo.padded)
}

/// 单页槽位查询。
pub fn slot_of(impo: &Imposition, page: u16) -> Result<Slot, BindError> {
    if page == 0 || page > impo.padded {
        return Err(BindError::UnknownPage);
    }
    match impo.slots.get(page as usize - 1) {
        Some(Some(s)) => Ok(*s),
        Some(None) => Err(BindError::RowMissing),
        None => Err(BindError::UnknownPage),
    }
}

/// 差异账：逐页一行，两序槽位并列 + differs 标记。补白页一并入账。
pub fn difference_ledger(pages: u16) -> Result<Vec<PageDiff>, BindError> {
    let saddle = impose(BindingKind::SaddleStitch, pages)?;
    let perfect = impose(BindingKind::PerfectBound, pages)?;
    if saddle.padded != perfect.padded {
        return Err(BindError::Mismatch);
    }
    let mut rows = Vec::with_capacity(saddle.padded as usize);
    let mut page = 1u16;
    while page <= saddle.padded {
        let s = slot_of(&saddle, page)?;
        let p = slot_of(&perfect, page)?;
        rows.push(PageDiff {
            page,
            blank: page > pages,
            saddle: s,
            perfect: p,
            differs: s != p,
        });
        page += 1;
    }
    let _ = audit_diff(&rows, saddle.padded)?;
    Ok(rows)
}

/// 差异账审计：行数、行序、differs 标记三重一致；返回差异页数。
pub fn audit_diff(rows: &[PageDiff], padded: u16) -> Result<u16, BindError> {
    if padded == 0 || rows.len() != padded as usize {
        return Err(BindError::Mismatch);
    }
    let mut differs = 0u16;
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            if r.page != (i as u16) + 1 {
                return Err(BindError::Mismatch);
            }
            if r.differs != (r.saddle != r.perfect) {
                return Err(BindError::Mismatch);
            }
            if r.differs {
                differs += 1;
            }
        }
        i += 1;
    }
    Ok(differs)
}

/// 胶装签名分组：按帖容量切页段（页码闭区间），末帖按实际页数收口。
pub fn signature_split(pages: u16, sig: u8) -> Result<Vec<(u16, u16)>, BindError> {
    if pages == 0 {
        return Err(BindError::Empty);
    }
    if pages > MAX_PAGES {
        return Err(BindError::TooMany);
    }
    let mut allowed = false;
    let mut i = 0usize;
    while i < SIG_SIZES.len() {
        if let Some(v) = SIG_SIZES.get(i) {
            if *v == sig {
                allowed = true;
            }
        }
        i += 1;
    }
    if !allowed {
        return Err(BindError::BadSignature);
    }
    let mut out = Vec::new();
    let mut start = 1u16;
    while start <= pages {
        let mut end = start.saturating_add(sig as u16 - 1);
        if end > pages {
            end = pages;
        }
        out.push((start, end));
        if end == pages {
            break;
        }
        start = end + 1;
    }
    Ok(out)
}

/// 注入核：六类坏输入/坏账分别落到 Empty / TooMany / BadSignature / UnknownPage /
/// RowMissing / RowDup。
pub fn injection_core() -> (usize, usize) {
    let total = 6usize;
    let mut caught = 0usize;

    if impose(BindingKind::SaddleStitch, 0).err() == Some(BindError::Empty) {
        caught += 1;
    }
    if impose(BindingKind::PerfectBound, MAX_PAGES + 1).err() == Some(BindError::TooMany) {
        caught += 1;
    }
    let sig_zero = signature_split(8, 0).err() == Some(BindError::BadSignature);
    let sig_odd = signature_split(8, 3).err() == Some(BindError::BadSignature);
    if sig_zero && sig_odd {
        caught += 1;
    }
    if saddle_slot_of(0, 8).err() == Some(BindError::UnknownPage)
        && perfect_slot_of(9, 8).err() == Some(BindError::UnknownPage)
    {
        caught += 1;
    }

    let mut missing = match impose(BindingKind::SaddleStitch, 12) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if let Some(cell) = missing.slots.get_mut(0) {
        *cell = None;
    }
    if audit_slots(&missing).err() == Some(BindError::RowMissing) {
        caught += 1;
    }

    let mut dup = match impose(BindingKind::SaddleStitch, 12) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    let stolen = match dup.slots.get(0) {
        Some(Some(s)) => Some(*s),
        _ => None,
    };
    if let Some(s) = stolen {
        if let Some(cell) = dup.slots.get_mut(1) {
            *cell = Some(s);
        }
        if audit_slots(&dup).err() == Some(BindError::RowDup) {
            caught += 1;
        }
    }
    (caught, total)
}

/// 册序域自检（J1：两序差异账逐页落对 + 补白守恒 + 篡改必抓）。
pub fn run_unx50035_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50035");

    // J1-1 逐页落对：37 页（补白到 40），两序槽位与闭式公式逐页一致。
    let (ok, det) = match (
        impose(BindingKind::SaddleStitch, 37),
        impose(BindingKind::PerfectBound, 37),
    ) {
        (Ok(sad), Ok(per)) => {
            let mut miss_s = 0usize;
            let mut miss_p = 0usize;
            let mut page = 1u16;
            while page <= sad.padded {
                match (saddle_slot_of(page, sad.padded), slot_of(&sad, page)) {
                    (Ok(a), Ok(b)) => {
                        if a != b {
                            miss_s += 1;
                        }
                    }
                    _ => miss_s += 1,
                }
                match (perfect_slot_of(page, per.padded), slot_of(&per, page)) {
                    (Ok(a), Ok(b)) => {
                        if a != b {
                            miss_p += 1;
                        }
                    }
                    _ => miss_p += 1,
                }
                page += 1;
            }
            (
                miss_s == 0 && miss_p == 0 && sad.padded == 40 && per.padded == 40,
                format!("saddle_miss={} perfect_miss={}", miss_s, miss_p),
            )
        }
        _ => (false, format!("impose failed")),
    };
    s.add_dyn("50035-J1-逐页落对", ok, &det);

    // J1-2 双射守恒：两种册序 40 页各占一个槽位，审计返回 padded。
    let bijective = match (
        impose(BindingKind::SaddleStitch, 37),
        impose(BindingKind::PerfectBound, 37),
    ) {
        (Ok(a), Ok(b)) => {
            let aa = audit_slots(&a).ok() == Some(40);
            let bb = audit_slots(&b).ok() == Some(40);
            let all_some = a.slots.iter().all(|s| s.is_some()) && b.slots.iter().all(|s| s.is_some());
            (aa && bb && all_some, aa, bb, all_some)
        }
        _ => (false, false, false, false),
    };
    let det = format!("saddle={} perfect={} all_some={}", bijective.1, bijective.2, bijective.3);
    s.add_dyn("50035-J1-双射守恒", bijective.0, &det);

    // J1-3 封面同面：第 1 页与第 N 页同在第 0 帖正面，index 分别为 1/0。
    let cover = match impose(BindingKind::SaddleStitch, 40) {
        Ok(v) => {
            let a = slot_of(&v, 1);
            let b = slot_of(&v, 40);
            match (a, b) {
                (Ok(sa), Ok(sb)) => {
                    let face = sa.sheet == 0
                        && sa.side == Side::Front
                        && sa.index == 1
                        && sb.sheet == 0
                        && sb.side == Side::Front
                        && sb.index == 0;
                    (face, sa.sheet, sb.sheet)
                }
                _ => (false, 99, 99),
            }
        }
        Err(_) => (false, 99, 99),
    };
    let det = format!("sheet1={} sheetN={}", cover.1, cover.2);
    s.add_dyn("50035-J1-封面同面", cover.0, &det);

    // J1-4 每面成对：骑马钉任一面两页之和 == padded+1，且两页互异、非 0。
    let pairs = match impose(BindingKind::SaddleStitch, 40) {
        Ok(v) => {
            let mut bad = 0usize;
            let mut i = 0usize;
            while i < v.sides.len() {
                if let Some(side) = v.sides.get(i) {
                    if side.count == 2 {
                        let a = side.pages[0];
                        let b = side.pages[1];
                        if a == 0 || b == 0 || a == b || a as u32 + b as u32 != v.padded as u32 + 1 {
                            bad += 1;
                        }
                    }
                }
                i += 1;
            }
            (bad == 0 && v.sides.len() == 20, bad)
        }
        Err(_) => (false, 99),
    };
    let det = format!("bad_sides={}", pairs.1);
    s.add_dyn("50035-J1-每面成对", pairs.0, &det);

    // J1-5 胶装单调：页序递增时叶号不减，奇数页正面、偶数页反面。
    let mono = match impose(BindingKind::PerfectBound, 40) {
        Ok(v) => {
            let mut bad = 0usize;
            let mut last: u16 = 0;
            let mut page = 1u16;
            while page <= v.padded {
                match slot_of(&v, page) {
                    Ok(slot) => {
                        if slot.sheet < last {
                            bad += 1;
                        }
                        last = slot.sheet;
                        let want = if page % 2 == 1 { Side::Front } else { Side::Back };
                        if slot.side != want || slot.index != 0 {
                            bad += 1;
                        }
                    }
                    Err(_) => bad += 1,
                }
                page += 1;
            }
            (bad == 0, bad)
        }
        Err(_) => (false, 99),
    };
    let det = format!("bad={}", mono.1);
    s.add_dyn("50035-J1-胶装单调", mono.0, &det);

    // J1-6 补白守恒：padded = pages + blanks，padded%4==0。
    let mut blanks_ok = 0usize;
    for pages in [1u16, 35, 37, 4096].iter() {
        if let Ok(v) = impose(BindingKind::SaddleStitch, *pages) {
            if v.padded == *pages + v.blanks && v.padded % 4 == 0 && v.padded >= *pages {
                blanks_ok += 1;
            }
        }
    }
    let ok = blanks_ok == 4;
    let det = format!("blanks_ok={}/4", blanks_ok);
    s.add_dyn("50035-J1-补白守恒", ok, &det);

    // J1-7 差异账：40 行覆盖 1..=40，补白 3 页，differs 与槽位比较一致，差异 30 页。
    let diff = match difference_ledger(37) {
        Ok(rows) => {
            let audit = audit_diff(&rows, 40);
            let mut blanks = 0usize;
            let mut differs = 0usize;
            let mut i = 0usize;
            while i < rows.len() {
                if let Some(r) = rows.get(i) {
                    if r.blank {
                        blanks += 1;
                    }
                    if r.differs {
                        differs += 1;
                    }
                }
                i += 1;
            }
            let counted = audit.ok() == Some(differs as u16);
            (
                rows.len() == 40 && blanks == 3 && differs == 30 && counted,
                format!("rows={} blanks={} differs={} audit={}", rows.len(), blanks, differs, counted),
            )
        }
        Err(e) => (false, format!("ledger={}", e.code())),
    };
    s.add_dyn("50035-J1-差异账", diff.0, &det);

    // J1-8 差异公式：无补白册子上差异页数恒为 3N/4。
    // 推导：前半册 N/2 页里，正面的 N/4 个奇数页因折帖 index 差异而不同、
    // 其余 N/4 个偶数页位置重合；后半册 N/2 页叶号/帖号必然错位，全部不同。
    let mut formula_ok = 0usize;
    for n in [4u16, 8, 12, 16].iter() {
        if let Ok(rows) = difference_ledger(*n) {
            if let Ok(d) = audit_diff(&rows, *n) {
                if d as u32 == (*n as u32) * 3 / 4 {
                    formula_ok += 1;
                }
            }
        }
    }
    let ok = formula_ok == 4;
    let det = format!("formula_ok={}/4", formula_ok);
    s.add_dyn("50035-J1-差异公式", ok, &det);

    // J1-9 差异行序：页号严格 1..=padded 且不重复。
    let ordering = match difference_ledger(37) {
        Ok(rows) => {
            let mut bad = 0usize;
            let mut i = 0usize;
            while i < rows.len() {
                if let Some(r) = rows.get(i) {
                    if r.page != (i as u16) + 1 {
                        bad += 1;
                    }
                }
                i += 1;
            }
            (bad == 0 && rows.len() == 40, bad)
        }
        Err(_) => (false, 99),
    };
    let det = format!("bad={}", ordering.1);
    s.add_dyn("50035-J1-差异行序", ordering.0, &det);

    // J1-10 确定性：同参数两次印版账逐位一致。
    let det_ok = match (
        impose(BindingKind::SaddleStitch, 37),
        impose(BindingKind::SaddleStitch, 37),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    s.add("50035-J1-印版确定", det_ok, if det_ok { "" } else { "两次印版账不一致" });

    // J1-11 非法必拒：0 页、超上限、坏帖容、页号越界。
    let mut bad = 0usize;
    if impose(BindingKind::PerfectBound, 0).err() == Some(BindError::Empty) {
        bad += 1;
    }
    if impose(BindingKind::SaddleStitch, MAX_PAGES + 1).err() == Some(BindError::TooMany) {
        bad += 1;
    }
    if signature_split(10, 0).err() == Some(BindError::BadSignature) {
        bad += 1;
    }
    if signature_split(10, 6).err() == Some(BindError::BadSignature) {
        bad += 1;
    }
    if saddle_slot_of(41, 40).err() == Some(BindError::UnknownPage) {
        bad += 1;
    }
    let ok = bad == 5;
    let det = format!("rejected={}/5", bad);
    s.add_dyn("50035-J1-非法必拒", ok, &det);

    // J1-12 篡改必抓：缺页 RowMissing、重槽 RowDup、坏标记 Mismatch、闭式不符 Mismatch。
    let tamper = match (
        impose(BindingKind::SaddleStitch, 12),
        difference_ledger(12),
    ) {
        (Ok(base), Ok(rows)) => {
            let mut missing = base.clone();
            if let Some(c) = missing.slots.get_mut(3) {
                *c = None;
            }
            let row_missing = audit_slots(&missing).err() == Some(BindError::RowMissing);

            let mut dup = base.clone();
            let stolen = match dup.slots.get(0) {
                Some(Some(s)) => Some(*s),
                _ => None,
            };
            let mut row_dup = false;
            if let Some(sv) = stolen {
                if let Some(c) = dup.slots.get_mut(1) {
                    *c = Some(sv);
                }
                row_dup = audit_slots(&dup).err() == Some(BindError::RowDup);
            }

            let mut unique = base.clone();
            if let Some(c) = unique.slots.get_mut(2) {
                *c = Some(Slot::new(999, Side::Back, 1));
            }
            let closed_mismatch = audit_slots(&unique).err() == Some(BindError::Mismatch);

            let mut flag_rows = rows.clone();
            let mut flag_bad = false;
            if let Some(r) = flag_rows.get_mut(0) {
                r.differs = !r.differs;
                flag_bad = audit_diff(&flag_rows, 12).err() == Some(BindError::Mismatch);
            }
            (
                row_missing && row_dup && closed_mismatch && flag_bad,
                row_missing,
                row_dup,
                closed_mismatch && flag_bad,
            )
        }
        _ => (false, false, false, false),
    };
    let det = format!(
        "missing={} dup={} mismatch={}",
        tamper.1, tamper.2, tamper.3
    );
    s.add_dyn("50035-J1-篡改必抓", tamper.0, &det);

    // J1-13 签名分组：37 页按 16 切 3 段、按 8 切 5 段，连续覆盖无缺口。
    let sig = match (signature_split(37, 16), signature_split(37, 8)) {
        (Ok(a), Ok(b)) => {
            let a_ok = a.len() == 3 && a.first() == Some(&(1, 16)) && a.last() == Some(&(33, 37));
            let b_ok = b.len() == 5 && b.first() == Some(&(1, 8)) && b.last() == Some(&(33, 37));
            let mut contiguous = 0usize;
            let mut cursor = 1u16;
            let mut i = 0usize;
            while i < b.len() {
                if let Some(range) = b.get(i) {
                    if range.0 == cursor {
                        contiguous += 1;
                        cursor = range.1 + 1;
                    }
                }
                i += 1;
            }
            (a_ok && b_ok && contiguous == 5 && cursor == 38, a.len(), b.len())
        }
        _ => (false, 0, 0),
    };
    let det = format!("sig16={} sig8={}", sig.1, sig.2);
    s.add_dyn("50035-J1-签名分组", sig.0, &det);

    // J1-14 六类注入全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50035-J1-注入六类", ok, &det);

    // J1-15 错误码闭集 0x43080+1..=0x43080+7。
    let all = [
        BindError::Empty,
        BindError::TooMany,
        BindError::BadSignature,
        BindError::UnknownPage,
        BindError::RowMissing,
        BindError::RowDup,
        BindError::Mismatch,
    ];
    let mut closed = 0usize;
    let mut k = 0usize;
    while k < all.len() {
        if let Some(e) = all.get(k) {
            let code = e.code();
            if code > SEG_BASE && code <= SEG_BASE + 7 && !e.message().is_empty() {
                closed += 1;
            }
        }
        k += 1;
    }
    let ok = closed == all.len();
    let det = format!("closed={}/{}", closed, all.len());
    s.add_dyn("50035-J1-码集闭包", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50035_checks_green() {
        let set = run_unx50035_checks();
        assert!(set.all_passed(), "UNX-F50035 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50035_saddle_is_bijection() {
        let impo = match impose(BindingKind::SaddleStitch, 37) {
            Ok(v) => v,
            Err(e) => panic!("impose {}", e.code()),
        };
        assert_eq!(impo.padded, 40);
        assert_eq!(audit_slots(&impo).ok(), Some(40));
        let mut page = 1u16;
        while page <= 40 {
            let closed = match saddle_slot_of(page, 40) {
                Ok(s) => s,
                Err(_) => panic!("closed form"),
            };
            assert_eq!(slot_of(&impo, page).ok(), Some(closed));
            page += 1;
        }
    }

    #[test]
    fn unx50035_diff_count_formula() {
        let rows = match difference_ledger(16) {
            Ok(v) => v,
            Err(_) => panic!("ledger"),
        };
        assert_eq!(audit_diff(&rows, 16).ok(), Some(12));
    }
}
