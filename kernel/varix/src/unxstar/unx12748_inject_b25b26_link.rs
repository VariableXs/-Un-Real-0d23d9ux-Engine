//! UNX-F12748 异常注入与 B25/B26 矩阵回归联动 · 0x82500段
//!
//! 判据原文（派工卡 12748 逐字锚定）：
//!   UNX-F12749-J1 断言账本 40 拍对账绿（A5 ktest 口径回指），账实差零静默。
//!
//! 设计要点：
//! 1. 联动链登记表冻结闭集 ≥20 行：每行持链节号/来源面/期望拒止码/联动旗标
//!    四元组，链节号 1..=24 连号唯一，来源面闭集 B25Inject|B26Badtable 二元
//!    封死——表外链节、越集来源面一律显性拒；
//! 2. 联动判定器 judge_link(node_no) 四闸定序：未登记链节 → ENOENT；来源面
//!    越闭集 → EINVAL；联动旗标未置但被链引用 → EPERM；链上断点（前后节点
//!    期望码不一致）→ EBROKEN。次序即优先级，多闸同破时取最前者；
//! 3. 链上零断点验：全链逐节复算（前驱/后继双向比对期望码），冻结表必须零
//!    断点；任何单节期望码被翻动，断点必被抓——注入器三类（断点注入/旗标
//!    未置/越闭集来源）各 10 轮确定性 LCG 全抓，逃脱即判据失效；
//! 4. 账本 40 拍对账：拍号 1..=40 / 账面值 / 实算值 / 差值三元组逐拍落账，
//!    差值必须为 0；非零差不告警即"零静默"（ESilent 拒收不入账），非零差
//!    告警入账则封账口被 EBeatDiff 拦死——账实差零静默必告警；
//! 5. 对账状态机 开账→40 拍齐→封账 单向推进，倒退显性拒；封账后写入拒；
//!    账面值取冻结常数（BOOK_BASE/BOOK_STEP），实算值从段基址与链证据码
//!    独立复算，两侧口径失配即差值非零必告警；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!    定值抖动）；取值一律 match/if let/.get()/checked_*/saturating_*，
//!    无 unwrap/expect/panic/裸下标；双跑一致指纹闭环。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12748 − 12600) = 0x80000 + 0x2500）。
pub const SEG_BASE: u32 = 0x82500;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LnkDiag {
    /// 链节号未登记：登记表里查无此节。
    ENoent,
    /// 来源面越闭集：face_raw 不在 B25Inject|B26Badtable 二元闭集内。
    EInval,
    /// 联动旗标未置：链节已登记被链引用，但旗标为假，联动不生效。
    EPerm,
    /// 链上断点：前后相邻节点的期望拒止码不一致。
    EBroken,
    /// 对账拍号越界：拍号不在 1..=40 闭集内。
    EBeatRange,
    /// 拍号重复：同一拍只落一笔，重复落账拒。
    EDup,
    /// 对账差值非零：账面值与实算值失配，已告警但封账被拦。
    EBeatDiff,
    /// 非零差零静默：差值非零却未拉告警，属静默失账，直接拒收。
    ESilent,
    /// 40 拍未齐禁止封账。
    ENotFull,
    /// 账本已封账：封账后写入拒绝。
    ESealed,
    /// 状态机倒退：封账后重封或回退，只进不退。
    ERewind,
    /// 注入逃脱：注入的违约未被任何闸抓到，判定面失效。
    ECatch,
}

impl LnkDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                LnkDiag::ENoent => 0x01,
                LnkDiag::EInval => 0x02,
                LnkDiag::EPerm => 0x03,
                LnkDiag::EBroken => 0x04,
                LnkDiag::EBeatRange => 0x05,
                LnkDiag::EDup => 0x06,
                LnkDiag::EBeatDiff => 0x07,
                LnkDiag::ESilent => 0x08,
                LnkDiag::ENotFull => 0x09,
                LnkDiag::ESealed => 0x0a,
                LnkDiag::ERewind => 0x0b,
                LnkDiag::ECatch => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            LnkDiag::ENoent => "链节未登记：先对冻结登记表核对链节号 1..=24，表外链节先登记再联动",
            LnkDiag::EInval => "来源面越闭集：来源面只认 B25Inject/B26Badtable 两值，先回写合法来源面再判定",
            LnkDiag::EPerm => "联动旗标未置：链节被链引用但旗标为假，先置联动旗标再走判定口",
            LnkDiag::EBroken => "链上断点：前后节点期望拒止码不一致，沿链节号回修被翻动的那一节",
            LnkDiag::EBeatRange => "拍号越界：对账拍号只认 1..=40，越界拍先改号再落账",
            LnkDiag::EDup => "拍号重复：每拍只落一笔，重复落账先查账内已有拍号",
            LnkDiag::EBeatDiff => "对账差值非零：账面与实算失配已告警，按拍号回修实算口径后再封账",
            LnkDiag::ESilent => "非零差零静默：差值非零却不告警属静默失账，必须拉告警才许入账",
            LnkDiag::ENotFull => "40 拍未齐禁止封账：先补齐缺拍再走封账口",
            LnkDiag::ESealed => "账本已封账：封账后写入拒绝，改动走下一轮对账滚动",
            LnkDiag::ERewind => "状态机倒退：开账→40 拍齐→封账只进不退，倒退即账本可疑",
            LnkDiag::ECatch => "注入逃脱：注入违约未被任何闸抓到，判定面失效须整表复核",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const LN_ALL: &[LnkDiag] = &[
    LnkDiag::ENoent,
    LnkDiag::EInval,
    LnkDiag::EPerm,
    LnkDiag::EBroken,
    LnkDiag::EBeatRange,
    LnkDiag::EDup,
    LnkDiag::EBeatDiff,
    LnkDiag::ESilent,
    LnkDiag::ENotFull,
    LnkDiag::ESealed,
    LnkDiag::ERewind,
    LnkDiag::ECatch,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in LN_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82501..0x8250C 闭集内"
}

// ── 来源面闭集 ───────────────────────────────────────────────────
/// 联动来源面二元闭集（判据指定：B25 注入面 | B26 坏表回归面，冻结不扩）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SrcFace {
    /// B25 异常注入面（断点/旗标/越集三类注入的来向）。
    B25Inject,
    /// B26 矩阵回归坏表面（坏表回放联动节）。
    B26Badtable,
}

impl SrcFace {
    /// 来源面码（闭集内 1/2，越集码查无）。
    pub fn code(self) -> u32 {
        match self {
            SrcFace::B25Inject => 1,
            SrcFace::B26Badtable => 2,
        }
    }

    /// 来源面中文名。
    pub fn name(self) -> &'static str {
        match self {
            SrcFace::B25Inject => "B25注入",
            SrcFace::B26Badtable => "B26坏表",
        }
    }

    /// 由原始码收闭集（越集返回 None）。
    pub fn from_code(raw: u32) -> Option<SrcFace> {
        match raw {
            1 => Some(SrcFace::B25Inject),
            2 => Some(SrcFace::B26Badtable),
            _ => None,
        }
    }
}

/// 来源面闭集数组（序即闭集序）。
pub const SRC_ALL: &[SrcFace] = &[SrcFace::B25Inject, SrcFace::B26Badtable];

// ── 联动链登记表域模型 ───────────────────────────────────────────
/// B25 注入面链期望拒止码（冻结常数：B25 语义下链节应回的拒止值）。
pub const REJECT_B25: u32 = 0x11;
/// B26 坏表回归面链期望拒止码（冻结常数）。
pub const REJECT_B26: u32 = 0x12;
/// 冻结登记表行数闭集下限（判据要求 ≥20 行）。
pub const LINK_CAP_MIN: usize = 20;
/// 冻结登记表实际行数（1..=24 连号）。
pub const LINK_ROWS: usize = 24;

/// 单个链节四元组：链节号/来源面原始码/期望拒止码/联动旗标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkRow {
    /// 链节号（冻结表内 1..=24 连号唯一）。
    pub node_no: u32,
    /// 来源面原始码（必须落在 SrcFace 闭集 1|2 内，越集判 EINVAL）。
    pub face_raw: u32,
    /// 期望拒止码（同链前后节必须一致，不一致即断点）。
    pub expect_code: u32,
    /// 联动旗标（被链引用的节必须置真，未置判 EPERM）。
    pub flag: bool,
}

/// 冻结联动链登记表（判据冻结闭集：24 行，不重排不扩容）。
/// 来源面常数来源：B25=异常注入矩阵面、B26=矩阵回归坏表面（派工卡 12748
/// 联动域规定），期望拒止码取 REJECT_B25/REJECT_B26 冻结值。
pub const FROZEN_LINKS: [LinkRow; LINK_ROWS] = [
    LinkRow { node_no: 1, face_raw: 1, expect_code: REJECT_B25, flag: true },
    LinkRow { node_no: 2, face_raw: 1, expect_code: REJECT_B25, flag: true },
    LinkRow { node_no: 3, face_raw: 1, expect_code: REJECT_B25, flag: true },
    LinkRow { node_no: 4, face_raw: 1, expect_code: REJECT_B25, flag: true },
    LinkRow { node_no: 5, face_raw: 1, expect_code: REJECT_B25, flag: true },
    LinkRow { node_no: 6, face_raw: 1, expect_code: REJECT_B25, flag: true },
    LinkRow { node_no: 7, face_raw: 1, expect_code: REJECT_B25, flag: true },
    LinkRow { node_no: 8, face_raw: 1, expect_code: REJECT_B25, flag: true },
    LinkRow { node_no: 9, face_raw: 1, expect_code: REJECT_B25, flag: true },
    LinkRow { node_no: 10, face_raw: 1, expect_code: REJECT_B25, flag: true },
    LinkRow { node_no: 11, face_raw: 1, expect_code: REJECT_B25, flag: true },
    LinkRow { node_no: 12, face_raw: 1, expect_code: REJECT_B25, flag: true },
    // 13..=24：B26 坏表回归链（同链期望码一致，链节号连续）。
    LinkRow { node_no: 13, face_raw: 2, expect_code: REJECT_B26, flag: true },
    LinkRow { node_no: 14, face_raw: 2, expect_code: REJECT_B26, flag: true },
    LinkRow { node_no: 15, face_raw: 2, expect_code: REJECT_B26, flag: true },
    LinkRow { node_no: 16, face_raw: 2, expect_code: REJECT_B26, flag: true },
    LinkRow { node_no: 17, face_raw: 2, expect_code: REJECT_B26, flag: true },
    LinkRow { node_no: 18, face_raw: 2, expect_code: REJECT_B26, flag: true },
    LinkRow { node_no: 19, face_raw: 2, expect_code: REJECT_B26, flag: true },
    LinkRow { node_no: 20, face_raw: 2, expect_code: REJECT_B26, flag: true },
    LinkRow { node_no: 21, face_raw: 2, expect_code: REJECT_B26, flag: true },
    LinkRow { node_no: 22, face_raw: 2, expect_code: REJECT_B26, flag: true },
    LinkRow { node_no: 23, face_raw: 2, expect_code: REJECT_B26, flag: true },
    LinkRow { node_no: 24, face_raw: 2, expect_code: REJECT_B26, flag: true },
];

/// 可操作的登记表视图（冻结表的拷贝，注入器在其上做定点翻动）。
#[derive(Clone, Debug)]
pub struct LinkTable {
    rows: Vec<LinkRow>,
}

impl LinkTable {
    /// 从冻结闭集拷贝建表。
    pub fn from_frozen() -> LinkTable {
        LinkTable {
            rows: FROZEN_LINKS.iter().copied().collect(),
        }
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<LinkRow> {
        self.rows.get(idx).copied()
    }

    /// 按链节号查行（未登记 None）。
    pub fn row_of(&self, node_no: u32) -> Option<LinkRow> {
        for r in self.rows.iter() {
            if r.node_no == node_no {
                return Some(*r);
            }
        }
        None
    }

    /// 统计某来源面的链节数。
    pub fn count_face(&self, face: SrcFace) -> usize {
        let mut n = 0usize;
        for r in self.rows.iter() {
            if SrcFace::from_code(r.face_raw) == Some(face) {
                n += 1;
            }
        }
        n
    }

    /// 定点翻动联动旗标（未登记链节拒）。
    pub fn set_flag(&mut self, node_no: u32, v: bool) -> Result<(), u32> {
        if self.row_of(node_no).is_none() {
            return Err(LnkDiag::ENoent.val());
        }
        if let Some(r) = self.rows.iter_mut().find(|r| r.node_no == node_no) {
            r.flag = v;
            Ok(())
        } else {
            Err(LnkDiag::ENoent.val())
        }
    }

    /// 定点翻动期望拒止码（未登记链节拒）。
    pub fn set_expect(&mut self, node_no: u32, code: u32) -> Result<(), u32> {
        if self.row_of(node_no).is_none() {
            return Err(LnkDiag::ENoent.val());
        }
        if let Some(r) = self.rows.iter_mut().find(|r| r.node_no == node_no) {
            r.expect_code = code;
            Ok(())
        } else {
            Err(LnkDiag::ENoent.val())
        }
    }

    /// 定点翻动来源面原始码（未登记链节拒；翻成越集码供 EINVAL 注入）。
    pub fn set_face_raw(&mut self, node_no: u32, raw: u32) -> Result<(), u32> {
        if self.row_of(node_no).is_none() {
            return Err(LnkDiag::ENoent.val());
        }
        if let Some(r) = self.rows.iter_mut().find(|r| r.node_no == node_no) {
            r.face_raw = raw;
            Ok(())
        } else {
            Err(LnkDiag::ENoent.val())
        }
    }
}

/// 同链邻节查找：want_prev 为真取前驱（同面、链节号最大且小于本节），
/// 否则取后继（同面、链节号最小且大于本节）。来源面越集的行不参与邻接。
fn chain_neighbor(t: &LinkTable, row: LinkRow, want_prev: bool) -> Option<LinkRow> {
    let mut best: Option<LinkRow> = None;
    let mut i = 0usize;
    while i < t.len() {
        if let Some(c) = t.row_at(i) {
            let same_face = match (SrcFace::from_code(c.face_raw), SrcFace::from_code(row.face_raw)) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            };
            let in_dir = if want_prev {
                c.node_no < row.node_no
            } else {
                c.node_no > row.node_no
            };
            if same_face && in_dir {
                let better = match best {
                    Some(p) => {
                        if want_prev {
                            c.node_no > p.node_no
                        } else {
                            c.node_no < p.node_no
                        }
                    }
                    None => true,
                };
                if better {
                    best = Some(c);
                }
            }
        }
        i += 1;
    }
    best
}

/// 联动判定器（对给定表）：四闸定序 ENOENT → EINVAL → EPERM → EBROKEN。
pub fn judge_link_on(t: &LinkTable, node_no: u32) -> Result<LinkRow, u32> {
    // 闸一：未登记链节 → ENOENT。
    let row = match t.row_of(node_no) {
        Some(r) => r,
        None => return Err(LnkDiag::ENoent.val()),
    };
    // 闸二：来源面越闭集 → EINVAL。
    if SrcFace::from_code(row.face_raw).is_none() {
        return Err(LnkDiag::EInval.val());
    }
    // 闸三：联动旗标未置但被链引用 → EPERM。
    if !row.flag {
        return Err(LnkDiag::EPerm.val());
    }
    // 闸四：链上断点（前驱或后继期望码不一致）→ EBROKEN。
    if let Some(p) = chain_neighbor(t, row, true) {
        if p.expect_code != row.expect_code {
            return Err(LnkDiag::EBroken.val());
        }
    }
    if let Some(s) = chain_neighbor(t, row, false) {
        if s.expect_code != row.expect_code {
            return Err(LnkDiag::EBroken.val());
        }
    }
    Ok(row)
}

/// 联动判定器（判据指定签名：对冻结表判定）。
pub fn judge_link(node_no: u32) -> Result<LinkRow, u32> {
    let t = LinkTable::from_frozen();
    judge_link_on(&t, node_no)
}

/// 链上断点扫描：全链逐节复算，返回断点链节号序列（按扫描序）。
/// 断点定义：本节与前驱（同面上一节）期望码不一致。
pub fn chain_break_scan(t: &LinkTable) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < t.len() {
        if let Some(node) = t.row_at(i) {
            if let Some(p) = chain_neighbor(t, node, true) {
                if p.expect_code != node.expect_code {
                    out.push(node.node_no);
                }
            }
        }
        i += 1;
    }
    out
}

// ── 确定性 LCG（种子 = SEG_BASE 抖动，零随机红线合规） ───────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性）。
    pub fn next_u32(&mut self) -> u32 {
        // LCG 常数取自 Knuth MMIX 31 位推荐值（整数手册常用闭集）。
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 注入面：三类注入 × 10 轮，全抓 ───────────────────────────────
/// 注入轮数闭集（三类各 10 轮）。
pub const INJECT_ROUNDS: usize = 10;

/// 类型一：断点注入 × 10 轮。每轮翻动一节的期望码，全链复算必须扫出
/// 断点，且扫出的每个断点节判定必回 EBROKEN。返回 (抓到, 总轮, 首逃脱描述)。
pub fn inject_break_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x1A) as u64);
    let mut k = 0usize;
    while k < INJECT_ROUNDS {
        // 链节号 LCG 抖动（1..=24），翻到对链的另一支期望码制造断点。
        let pick = 1 + rng.below(LINK_ROWS as u32);
        let mut t = LinkTable::from_frozen();
        let cur = t.row_of(pick).map(|r| r.expect_code).unwrap_or(0);
        let flipped = if cur == REJECT_B25 { REJECT_B26 } else { REJECT_B25 };
        let _ = t.set_expect(pick, flipped);
        // 全链逐节复算：断点必须被扫出。
        let brks = chain_break_scan(&t);
        let mut ok = !brks.is_empty();
        for b in brks.iter() {
            if judge_link_on(&t, *b) != Err(LnkDiag::EBroken.val()) {
                ok = false;
            }
        }
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("断点注入第{}轮节{}逃脱：扫描{:?}", k, pick, brks);
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 类型二：旗标未置注入 × 10 轮。每轮清掉一节的联动旗标，判定必回 EPERM。
pub fn inject_flag_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x2B) as u64);
    let mut k = 0usize;
    while k < INJECT_ROUNDS {
        let pick = 1 + rng.below(LINK_ROWS as u32);
        let mut t = LinkTable::from_frozen();
        let _ = t.set_flag(pick, false);
        let hit = judge_link_on(&t, pick) == Err(LnkDiag::EPerm.val());
        total += 1;
        if hit {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("旗标注入第{}轮节{}逃脱：判定未回 EPERM", k, pick);
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 类型三：越闭集来源注入 × 10 轮。每轮把一节来源面翻成越集码
/// （0 / 3 / 0xFFFF 三路抖动），判定必回 EINVAL。
pub fn inject_face_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x3C) as u64);
    let mut k = 0usize;
    while k < INJECT_ROUNDS {
        let pick = 1 + rng.below(LINK_ROWS as u32);
        let bad = match rng.below(3) {
            0 => 0u32,
            1 => 3u32,
            _ => 0xFFFFu32,
        };
        let mut t = LinkTable::from_frozen();
        let _ = t.set_face_raw(pick, bad);
        let hit = judge_link_on(&t, pick) == Err(LnkDiag::EInval.val());
        total += 1;
        if hit {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("越集注入第{}轮节{}码{}逃脱：判定未回 EINVAL", k, pick, bad);
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 账本 40 拍对账域模型 ─────────────────────────────────────────
/// 对账拍数闭集（判据：40 拍）。
pub const BEAT_CAP: usize = 40;
/// 账面基值（冻结常数：对账账面口径的起点）。
pub const BOOK_BASE: u32 = 0x8250;
/// 账面步长（冻结常数：取 B25 链期望拒止码，账实两侧共用口径锚）。
pub const BOOK_STEP: u32 = REJECT_B25;

/// 账面值：拍号 → 账面口径（冻结常数推导，saturating 兜底）。
pub fn book_of(beat: u32) -> u32 {
    let off = BOOK_STEP.saturating_mul(beat.saturating_sub(1));
    BOOK_BASE.saturating_add(off)
}

/// 实算值：拍号 → 实算口径（从段基址与链证据码独立复算）。
/// 口径锚：SEG_BASE >> 4 恒等于 BOOK_BASE（0x82500 >> 4 = 0x8250），
/// 步长取冻结链证据码 REJECT_B25——两侧常数源不同，失配即差值非零。
pub fn real_of(beat: u32) -> u32 {
    let base = SEG_BASE >> 4;
    let off = REJECT_B25.saturating_mul(beat.saturating_sub(1));
    base.saturating_add(off)
}

/// 单拍三元组：拍号/账面值/实算值 + 差值与告警旗标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeatRow {
    /// 拍号（1..=40 闭集）。
    pub beat: u32,
    /// 账面值。
    pub book: u32,
    /// 实算值。
    pub real: u32,
    /// 差值（|账面 − 实算|，必须为 0）。
    pub diff: u32,
    /// 告警旗标（非零差入账必须置真）。
    pub warned: bool,
}

/// 对账状态机：开账 → 40 拍齐 → 封账，单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconState {
    /// 开账：允许逐拍落账。
    Open,
    /// 40 拍齐：等待封账。
    Reconciled,
    /// 封账：只读。
    Sealed,
}

impl ReconState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            ReconState::Open => 0,
            ReconState::Reconciled => 1,
            ReconState::Sealed => 2,
        }
    }
}

/// 40 拍对账账本。
#[derive(Clone, Debug)]
pub struct ReconLedger {
    beats: Vec<BeatRow>,
    alerts: Vec<u32>,
    state: ReconState,
}

impl ReconLedger {
    /// 开一本新账。
    pub fn new() -> ReconLedger {
        ReconLedger {
            beats: Vec::new(),
            alerts: Vec::new(),
            state: ReconState::Open,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> ReconState {
        self.state
    }

    /// 已落账拍数。
    pub fn len(&self) -> usize {
        self.beats.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.beats.is_empty()
    }

    /// 告警条数。
    pub fn alert_count(&self) -> usize {
        self.alerts.len()
    }

    /// 告警码快照（闭集码序列）。
    pub fn alerts_snapshot(&self) -> Vec<u32> {
        self.alerts.iter().copied().collect()
    }

    /// 取第 idx 拍（越界 None）。
    pub fn beat_at(&self, idx: usize) -> Option<BeatRow> {
        self.beats.get(idx).copied()
    }

    /// 按拍号查拍。
    pub fn beat_of(&self, beat: u32) -> Option<BeatRow> {
        for b in self.beats.iter() {
            if b.beat == beat {
                return Some(*b);
            }
        }
        None
    }

    /// 全账差值为零（判据：账实差零）。
    pub fn diffs_all_zero(&self) -> bool {
        for b in self.beats.iter() {
            if b.diff != 0 {
                return false;
            }
        }
        true
    }

    /// 逐拍落账（标准口径：账面/实算按冻结口径现算）。
    /// raise_warn 指定非零差时是否拉告警——不拉即零静默，拒收不入账。
    pub fn record(&mut self, beat: u32, raise_warn: bool) -> Result<(), u32> {
        self.record_raw(beat, book_of(beat), real_of(beat), raise_warn)
    }

    /// 逐拍落账（原始口径：账面/实算由调用方给定，注入器与探针用）。
    pub fn record_raw(&mut self, beat: u32, book: u32, real: u32, raise_warn: bool) -> Result<(), u32> {
        // 封账闸：封账后一切写入拒绝。
        if self.state.rank() >= ReconState::Sealed.rank() {
            return Err(LnkDiag::ESealed.val());
        }
        // 拍号闭集闸：1..=40 之外一律拒。
        if beat == 0 || beat > BEAT_CAP as u32 {
            return Err(LnkDiag::EBeatRange.val());
        }
        // 重复拍闸：同一拍只落一笔。
        if self.beat_of(beat).is_some() {
            return Err(LnkDiag::EDup.val());
        }
        // 差值三元组：|账面 − 实算|（checked 双向，极端兜底按非零差处理）。
        let diff = match book.checked_sub(real) {
            Some(d) => d,
            None => match real.checked_sub(book) {
                Some(d) => d,
                None => return Err(LnkDiag::EBeatDiff.val()),
            },
        };
        // 零静默闸：非零差不告警 → ESilent，拒收不入账。
        if diff != 0 && !raise_warn {
            return Err(LnkDiag::ESilent.val());
        }
        // 非零差告警：拉 EBeatDiff 告警并落带警行。
        if diff != 0 {
            self.alerts.push(LnkDiag::EBeatDiff.val());
            self.beats.push(BeatRow {
                beat,
                book,
                real,
                diff,
                warned: true,
            });
            if self.beats.len() == BEAT_CAP {
                self.state = ReconState::Reconciled;
            }
            return Ok(());
        }
        // 差值为零：正常落账。
        self.beats.push(BeatRow {
            beat,
            book,
            real,
            diff,
            warned: false,
        });
        if self.beats.len() == BEAT_CAP {
            self.state = ReconState::Reconciled;
        }
        Ok(())
    }

    /// 封账：40 拍齐 + 全账差零才许封。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= ReconState::Sealed.rank() {
            return Err(LnkDiag::ERewind.val());
        }
        if self.beats.len() != BEAT_CAP {
            return Err(LnkDiag::ENotFull.val());
        }
        if !self.diffs_all_zero() {
            return Err(LnkDiag::EBeatDiff.val());
        }
        self.state = ReconState::Sealed;
        Ok(())
    }
}

/// 40 拍全量对账（标准口径），返回落好账的账本（失败返回诊断码）。
pub fn reconcile_all(raise_warn: bool) -> Result<ReconLedger, u32> {
    let mut l = ReconLedger::new();
    let mut b = 1u32;
    while b <= BEAT_CAP as u32 {
        match l.record(b, raise_warn) {
            Ok(()) => b += 1,
            Err(e) => return Err(e),
        }
    }
    Ok(l)
}

/// 非零差拍注入 × 10 轮：每拍翻动实算值制造非零差——告警必须拉起、
/// 差值必须非零、封账口必须被 EBeatDiff 拦死，静默口必须被 ESilent 拒。
pub fn beat_tamper_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x4D) as u64);
    let mut k = 0usize;
    while k < INJECT_ROUNDS {
        let pick = 1 + rng.below(BEAT_CAP as u32);
        // 告警路：非零差 + 拉警 → 入账带警、告警 +1、封账被拦。
        let mut la = ReconLedger::new();
        let book = book_of(pick);
        let real_bad = real_of(pick).wrapping_add(1);
        let mut ok = la.record_raw(pick, book, real_bad, true).is_ok();
        if let Some(r) = la.beat_of(pick) {
            ok = ok && r.warned && r.diff == 1;
        } else {
            ok = false;
        }
        ok = ok && la.alert_count() == 1 && !la.diffs_all_zero();
        // 静默路：同样的非零差不拉警 → ESilent 拒收不入账。
        let mut ls = ReconLedger::new();
        ok = ok && ls.record_raw(pick, book, real_bad, false) == Err(LnkDiag::ESilent.val());
        ok = ok && ls.is_empty();
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("非零差注入第{}轮拍{}逃脱：告警或静默闸失效", k, pick);
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 双跑一致指纹 ─────────────────────────────────────────────────
/// 全流程确定性指纹：判定全节 + 三类注入 + 40 拍对账逐段折叠。
pub fn fingerprint() -> u64 {
    let mut h: u64 = SEG_BASE as u64;
    // 段一：冻结表全节判定结果折叠。
    let t = LinkTable::from_frozen();
    let mut n = 1u32;
    while n <= LINK_ROWS as u32 {
        let mix = match judge_link_on(&t, n) {
            Ok(r) => ((r.expect_code as u64) << 8) | (r.node_no as u64),
            Err(e) => 0xDEAD_0000u64 | (e as u64),
        };
        h = h.wrapping_mul(0x100000001B3).wrapping_add(mix);
        n += 1;
    }
    // 段二：三类注入战果折叠（轮数 + 抓到数 + 逃脱串长度）。
    let (c1, t1, e1) = inject_break_sweep();
    let (c2, t2, e2) = inject_flag_sweep();
    let (c3, t3, e3) = inject_face_sweep();
    let mix2 = ((t1 as u64) << 40)
        | ((c1 as u64) << 32)
        | ((t2 as u64) << 24)
        | ((c2 as u64) << 16)
        | ((t3 as u64) << 8)
        | (c3 as u64);
    h = h.wrapping_mul(0x100000001B3).wrapping_add(mix2);
    h = h
        .wrapping_add(e1.len() as u64)
        .wrapping_add(e2.len() as u64)
        .wrapping_add(e3.len() as u64);
    // 段三：40 拍账面/实算折叠（差值恒零，两侧同折）。
    let mut b = 1u32;
    while b <= BEAT_CAP as u32 {
        let mix3 = ((book_of(b) as u64) << 1) ^ (real_of(b) as u64);
        h = h.wrapping_mul(0x100000001B3).wrapping_add(mix3);
        b += 1;
    }
    h
}

/// 双跑一致：两次完整流程指纹与各路注入战果逐项相同。
fn dual_run_ok() -> bool {
    let a = fingerprint();
    let b = fingerprint();
    if a != b {
        return false;
    }
    if inject_break_sweep() != inject_break_sweep() {
        return false;
    }
    if inject_flag_sweep() != inject_flag_sweep() {
        return false;
    }
    if inject_face_sweep() != inject_face_sweep() {
        return false;
    }
    // 拍口径双跑：两本账逐拍三元组一致。
    let l1 = match reconcile_all(true) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let l2 = match reconcile_all(true) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let mut i = 0usize;
    while i < BEAT_CAP {
        match (l1.beat_at(i), l2.beat_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 批窗口 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：全部落在 [SEG_BASE, SEG_BASE+0x40) 窗口且查表可命中。
fn diag_closed_ok() -> bool {
    if LN_ALL.len() != 12 {
        return false;
    }
    for d in LN_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if detail(v) != d.detail() {
            return false;
        }
        if d.detail().is_empty() {
            return false;
        }
    }
    // 码互异。
    let mut i = 0usize;
    while i < LN_ALL.len() {
        let vi = match LN_ALL.get(i) {
            Some(d) => d.val(),
            None => return false,
        };
        let mut j = i + 1;
        while j < LN_ALL.len() {
            let vj = match LN_ALL.get(j) {
                Some(d) => d.val(),
                None => return false,
            };
            if vi == vj {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    // 未知码必须回落提示文案。
    if detail(SEG_BASE + 0x3F) != "未知诊断码：不在 0x82501..0x8250C 闭集内" {
        return false;
    }
    true
}

/// 来源面闭集守卫：二元封死、码互异、往返一致、越集查无。
fn src_face_ok() -> bool {
    if SRC_ALL.len() != 2 {
        return false;
    }
    if SrcFace::B25Inject.code() != 1 || SrcFace::B26Badtable.code() != 2 {
        return false;
    }
    if SrcFace::from_code(1) != Some(SrcFace::B25Inject) {
        return false;
    }
    if SrcFace::from_code(2) != Some(SrcFace::B26Badtable) {
        return false;
    }
    // 越集码（0/3/0xFFFF）一律查无。
    if SrcFace::from_code(0).is_some() {
        return false;
    }
    if SrcFace::from_code(3).is_some() {
        return false;
    }
    if SrcFace::from_code(0xFFFF).is_some() {
        return false;
    }
    true
}

/// 冻结登记表完整性：≥20 行、1..=24 连号唯一、来源面全在闭集、
/// 期望码取冻结值、旗标全置、两链各 12 节。
fn frozen_registry_ok() -> bool {
    if LINK_ROWS < LINK_CAP_MIN {
        return false;
    }
    let t = LinkTable::from_frozen();
    if t.len() != LINK_ROWS {
        return false;
    }
    // 连号唯一 + 行内容逐行核对。
    let mut n = 1u32;
    while n <= LINK_ROWS as u32 {
        let row = match t.row_of(n) {
            Some(r) => r,
            None => return false,
        };
        if row.node_no != n {
            return false;
        }
        if SrcFace::from_code(row.face_raw).is_none() {
            return false;
        }
        if row.expect_code != REJECT_B25 && row.expect_code != REJECT_B26 {
            return false;
        }
        if !row.flag {
            return false;
        }
        n += 1;
    }
    // 两链各 12 节。
    if t.count_face(SrcFace::B25Inject) != 12 {
        return false;
    }
    if t.count_face(SrcFace::B26Badtable) != 12 {
        return false;
    }
    true
}

/// 正常流：冻结表全节判定 Ok，且返回行与冻结口径逐项一致。
fn judge_all_ok() -> bool {
    let t = LinkTable::from_frozen();
    let mut n = 1u32;
    while n <= LINK_ROWS as u32 {
        let row = match judge_link_on(&t, n) {
            Ok(r) => r,
            Err(_) => return false,
        };
        let expect_face = if n <= 12 { 1u32 } else { 2u32 };
        let expect_code = if n <= 12 { REJECT_B25 } else { REJECT_B26 };
        if row.face_raw != expect_face || row.expect_code != expect_code || !row.flag {
            return false;
        }
        n += 1;
    }
    true
}

/// 链上零断点验：冻结表全链复算零断点，且全节判定无 EBROKEN。
fn zero_break_ok() -> bool {
    let t = LinkTable::from_frozen();
    if !chain_break_scan(&t).is_empty() {
        return false;
    }
    let mut n = 1u32;
    while n <= LINK_ROWS as u32 {
        if judge_link_on(&t, n) == Err(LnkDiag::EBroken.val()) {
            return false;
        }
        n += 1;
    }
    // 翻一节则断点必现：链验算法有效。
    let mut t2 = LinkTable::from_frozen();
    let _ = t2.set_expect(7, REJECT_B26);
    let brks = chain_break_scan(&t2);
    if brks.len() < 2 {
        return false;
    }
    true
}

/// 未登记链节拒止：越界号一律 ENOENT，且优先于其他闸。
fn enoent_ok() -> bool {
    if judge_link(0) != Err(LnkDiag::ENoent.val()) {
        return false;
    }
    if judge_link(LINK_ROWS as u32 + 1) != Err(LnkDiag::ENoent.val()) {
        return false;
    }
    if judge_link(u32::MAX) != Err(LnkDiag::ENoent.val()) {
        return false;
    }
    // 优先性：即便全表旗标清零，未登记链节仍判 ENOENT。
    let mut t = LinkTable::from_frozen();
    let mut n = 1u32;
    while n <= LINK_ROWS as u32 {
        let _ = t.set_flag(n, false);
        n += 1;
    }
    if judge_link_on(&t, LINK_ROWS as u32 + 1) != Err(LnkDiag::ENoent.val()) {
        return false;
    }
    // 定点翻动接口对未登记链节同样拒。
    if t.set_flag(LINK_ROWS as u32 + 1, true) != Err(LnkDiag::ENoent.val()) {
        return false;
    }
    if t.set_expect(0, REJECT_B25) != Err(LnkDiag::ENoent.val()) {
        return false;
    }
    if t.set_face_raw(u32::MAX, 3) != Err(LnkDiag::ENoent.val()) {
        return false;
    }
    true
}

/// 判定次序：ENOENT > EINVAL > EPERM > EBROKEN，多闸同破取最前。
fn order_ok() -> bool {
    // EINVAL > EPERM：面越集 + 旗标未置同时成立，判 EINVAL。
    let mut t1 = LinkTable::from_frozen();
    let _ = t1.set_face_raw(5, 3);
    let _ = t1.set_flag(5, false);
    if judge_link_on(&t1, 5) != Err(LnkDiag::EInval.val()) {
        return false;
    }
    // EPERM > EBROKEN：旗标未置 + 期望码断点同时成立，判 EPERM。
    let mut t2 = LinkTable::from_frozen();
    let _ = t2.set_flag(7, false);
    let _ = t2.set_expect(7, REJECT_B26);
    if judge_link_on(&t2, 7) != Err(LnkDiag::EPerm.val()) {
        return false;
    }
    // EBROKEN：旗标置好、面合规，仅断点——本节与前节都报断点。
    let mut t3 = LinkTable::from_frozen();
    let _ = t3.set_expect(9, REJECT_B26);
    if judge_link_on(&t3, 9) != Err(LnkDiag::EBroken.val()) {
        return false;
    }
    if judge_link_on(&t3, 10) != Err(LnkDiag::EBroken.val()) {
        return false;
    }
    // ENOENT 最高优先：表上另有断点也先报未登记。
    if judge_link_on(&t3, u32::MAX) != Err(LnkDiag::ENoent.val()) {
        return false;
    }
    true
}

/// 账本 40 拍对账绿：全量落账、差值全零、无告警、状态推进、封账成功。
fn reconcile_green_ok() -> bool {
    let mut l = match reconcile_all(true) {
        Ok(l) => l,
        Err(_) => return false,
    };
    if l.len() != BEAT_CAP || l.state() != ReconState::Reconciled {
        return false;
    }
    if !l.diffs_all_zero() || l.alert_count() != 0 {
        return false;
    }
    // 逐拍三元组核对：拍号 1..=40、账实相等、差零、不带警。
    let mut b = 1u32;
    while b <= BEAT_CAP as u32 {
        let row = match l.beat_of(b) {
            Some(r) => r,
            None => return false,
        };
        if row.beat != b || row.book != row.real || row.diff != 0 || row.warned {
            return false;
        }
        if row.book != book_of(b) || row.real != real_of(b) {
            return false;
        }
        b += 1;
    }
    // 封账成功；封账后写入拒；重封倒退拒。
    if l.seal().is_err() {
        return false;
    }
    if l.state() != ReconState::Sealed {
        return false;
    }
    if l.record(1, true) != Err(LnkDiag::ESealed.val()) {
        return false;
    }
    if l.seal() != Err(LnkDiag::ERewind.val()) {
        return false;
    }
    true
}

/// 拍号越界与重复拍拒止：越界闸先于重复闸，空账封账拒，账面不变。
fn beat_boundary_ok() -> bool {
    let mut l = ReconLedger::new();
    // 越界：0、41、u32::MAX 全拒。
    if l.record(0, true) != Err(LnkDiag::EBeatRange.val()) {
        return false;
    }
    if l.record(BEAT_CAP as u32 + 1, true) != Err(LnkDiag::EBeatRange.val()) {
        return false;
    }
    if l.record(u32::MAX, true) != Err(LnkDiag::EBeatRange.val()) {
        return false;
    }
    // 原始口径越界同样拒（拍号闸先于差值闸）。
    if l.record_raw(0, book_of(1), real_of(1), true) != Err(LnkDiag::EBeatRange.val()) {
        return false;
    }
    // 首拍落账后重复落账拒。
    if l.record(1, true).is_err() {
        return false;
    }
    if l.record(1, true) != Err(LnkDiag::EDup.val()) {
        return false;
    }
    // 空账（仅 1 拍）封账拒。
    if l.seal() != Err(LnkDiag::ENotFull.val()) {
        return false;
    }
    // 账面不变：仍只 1 拍。
    if l.len() != 1 {
        return false;
    }
    true
}

/// 非零差零静默必告警：静默路 ESilent 拒收不入账；告警路带警入账、
/// 告警 +1、封账被拦；非零差拍注入 10 轮全抓。
fn silent_alarm_ok() -> bool {
    // 静默路：非零差不拉警 → ESilent，行不入账。
    let mut l1 = ReconLedger::new();
    let book = book_of(1);
    let real_bad = real_of(1).wrapping_add(1);
    if l1.record_raw(1, book, real_bad, false) != Err(LnkDiag::ESilent.val()) {
        return false;
    }
    if !l1.is_empty() || l1.alert_count() != 0 {
        return false;
    }
    // 告警路：同差拉警 → 带警入账，但封账被 EBeatDiff 拦死。
    let mut l2 = ReconLedger::new();
    if l2.record_raw(1, book, real_bad, true).is_err() {
        return false;
    }
    let warned = match l2.beat_of(1) {
        Some(r) => r.warned && r.diff == 1 && r.real == real_bad,
        None => false,
    };
    if !warned || l2.alert_count() != 1 {
        return false;
    }
    let mut b = 2u32;
    while b <= BEAT_CAP as u32 {
        if l2.record(b, true).is_err() {
            return false;
        }
        b += 1;
    }
    if l2.seal() != Err(LnkDiag::EBeatDiff.val()) {
        return false;
    }
    // 非零差拍注入 10 轮全抓。
    let (caught, total, escape) = beat_tamper_sweep();
    if total != INJECT_ROUNDS || caught != INJECT_ROUNDS {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 注入三类 10 轮全抓汇总谓词。
fn inject_all_caught_ok() -> bool {
    let (c1, t1, e1) = inject_break_sweep();
    if t1 != INJECT_ROUNDS || c1 != INJECT_ROUNDS || !e1.is_empty() {
        return false;
    }
    let (c2, t2, e2) = inject_flag_sweep();
    if t2 != INJECT_ROUNDS || c2 != INJECT_ROUNDS || !e2.is_empty() {
        return false;
    }
    let (c3, t3, e3) = inject_face_sweep();
    if t3 != INJECT_ROUNDS || c3 != INJECT_ROUNDS || !e3.is_empty() {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12748 自检（判据 12748 卡：异常注入与 B25/B26 矩阵回归联动 +
/// UNX-F12749-J1 账本 40 拍对账绿，账实差零静默）。
pub fn run_unx12748_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12748");
    let ledger = match reconcile_all(true) {
        Ok(l) => l,
        Err(_) => ReconLedger::new(),
    };

    s.add_dyn(
        "F12748-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={} win80000_831C0={}", SEG_BASE, SEG_BASE % 0x40 == 0, (0x80000..=0x831C0).contains(&SEG_BASE)),
    );

    s.add_dyn(
        "F12748-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "n={} first=0x{:05x} last=0x{:05x}",
            LN_ALL.len(),
            LN_ALL.first().map(|d| d.val()).unwrap_or(0),
            LN_ALL.last().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12748-03 来源面闭集",
        src_face_ok(),
        &format!("faces={} 首={} 尾={}", SRC_ALL.len(), SRC_ALL.first().map(|f| f.name()).unwrap_or("?"), SRC_ALL.last().map(|f| f.name()).unwrap_or("?")),
    );

    s.add_dyn(
        "F12748-04 冻结登记表完整性",
        frozen_registry_ok(),
        &format!("rows={} b25={} b26={}", LINK_ROWS, LinkTable::from_frozen().count_face(SrcFace::B25Inject), LinkTable::from_frozen().count_face(SrcFace::B26Badtable)),
    );

    s.add_dyn(
        "F12748-05 正常流判定全 Ok",
        judge_all_ok(),
        &format!("nodes={} 全节 Ok 且行内容合冻结口径", LINK_ROWS),
    );

    s.add_dyn(
        "F12748-06 链上零断点验",
        zero_break_ok(),
        &format!("frozen_breaks={} 翻节必现断点", chain_break_scan(&LinkTable::from_frozen()).len()),
    );

    let (c1, t1, e1) = inject_break_sweep();
    s.add_dyn(
        "F12748-07 断点注入 10 轮全抓",
        c1 == INJECT_ROUNDS && t1 == INJECT_ROUNDS && e1.is_empty(),
        &format!("caught={}/{} {}", c1, t1, e1),
    );

    let (c2, t2, e2) = inject_flag_sweep();
    s.add_dyn(
        "F12748-08 旗标未置注入 10 轮全抓",
        c2 == INJECT_ROUNDS && t2 == INJECT_ROUNDS && e2.is_empty(),
        &format!("caught={}/{} {}", c2, t2, e2),
    );

    let (c3, t3, e3) = inject_face_sweep();
    s.add_dyn(
        "F12748-09 越闭集来源注入 10 轮全抓",
        c3 == INJECT_ROUNDS && t3 == INJECT_ROUNDS && e3.is_empty(),
        &format!("caught={}/{} {}", c3, t3, e3),
    );

    s.add_dyn(
        "F12748-10 未登记链节 ENOENT 拒止",
        enoent_ok(),
        &format!("越界 0/25/MAX 全拒，优先性最高"),
    );

    s.add_dyn(
        "F12748-11 判定次序 ENOENT>EINVAL>EPERM>EBROKEN",
        order_ok(),
        "多闸同破取最前，四闸逐级验证",
    );

    s.add_dyn(
        "F12748-12 账本 40 拍对账绿",
        reconcile_green_ok(),
        &format!("beats={} diff0={} alerts={}", ledger.len(), ledger.diffs_all_zero(), ledger.alert_count()),
    );

    s.add_dyn(
        "F12748-13 拍号越界与重复拍拒止",
        beat_boundary_ok(),
        "越界 0/41/MAX、重复拍、空账封账 四点全拒",
    );

    s.add_dyn(
        "F12748-14 非零差零静默必告警",
        silent_alarm_ok(),
        "静默路 ESilent 拒收，告警路带警入账封账拦，注入 10 轮全抓",
    );

    s.add_dyn(
        "F12748-15 双跑一致指纹",
        dual_run_ok(),
        &format!("fp=0x{:016x} 两次全流程一致", fingerprint()),
    );

    // 注入三类 10 轮全抓汇总（独立复核三路战果计数）。
    s.add_dyn(
        "F12748-16 注入战果汇总复核",
        inject_all_caught_ok(),
        &format!("{}轮×3类 全抓无逃脱", INJECT_ROUNDS),
    );

    // 收口：判据条数声明（本节共 17 条检查项，含本收口条）。
    let n_before = s.len();
    s.add_dyn(
        "F12748-17 收口·判据条数声明",
        n_before == 16,
        &format!("判据条数=17（含收口） 已录={} 全绿={}", n_before, s.all_passed()),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::MAX_CHECKS;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 批窗口 [0x80000, 0x831C0]。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x82500);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中、未知码回落。
        assert_eq!(LN_ALL.len(), 12);
        let mut i = 0usize;
        while i < LN_ALL.len() {
            let vi = LN_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < LN_ALL.len() {
                let vj = LN_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
        assert_eq!(detail(LnkDiag::EBroken.val()), LnkDiag::EBroken.detail());
    }

    #[test]
    fn happy_path_all_nodes_judge_ok() {
        // 正常流：冻结表 24 节判定全 Ok，B25 链码 0x11、B26 链码 0x12。
        let t = LinkTable::from_frozen();
        assert_eq!(t.len(), LINK_ROWS);
        let mut n = 1u32;
        while n <= LINK_ROWS as u32 {
            let r = judge_link_on(&t, n).unwrap_or(LinkRow { node_no: 0, face_raw: 0, expect_code: 0, flag: false });
            assert_eq!(r.node_no, n);
            if n <= 12 {
                assert_eq!(r.expect_code, REJECT_B25);
                assert_eq!(SrcFace::from_code(r.face_raw), Some(SrcFace::B25Inject));
            } else {
                assert_eq!(r.expect_code, REJECT_B26);
                assert_eq!(SrcFace::from_code(r.face_raw), Some(SrcFace::B26Badtable));
            }
            assert!(r.flag);
            n += 1;
        }
        assert!(judge_all_ok());
        assert!(zero_break_ok());
    }

    #[test]
    fn chain_scan_zero_on_frozen_and_catches_flip() {
        // 链上零断点验：冻结表零断点；翻一节后扫描必现断点且判定回 EBROKEN。
        let t = LinkTable::from_frozen();
        assert!(chain_break_scan(&t).is_empty());
        let mut t2 = LinkTable::from_frozen();
        let _ = t2.set_expect(13, REJECT_B25);
        let brks = chain_break_scan(&t2);
        assert!(!brks.is_empty());
        for b in brks.iter() {
            assert_eq!(judge_link_on(&t2, *b), Err(LnkDiag::EBroken.val()));
        }
    }

    #[test]
    fn three_injections_ten_rounds_all_caught() {
        // 三类注入各 10 轮全抓，无逃脱。
        let (c1, t1, e1) = inject_break_sweep();
        assert_eq!(t1, 10);
        assert_eq!(c1, 10, "断点注入逃脱：{}", e1);
        let (c2, t2, e2) = inject_flag_sweep();
        assert_eq!(t2, 10);
        assert_eq!(c2, 10, "旗标注入逃脱：{}", e2);
        let (c3, t3, e3) = inject_face_sweep();
        assert_eq!(t3, 10);
        assert_eq!(c3, 10, "越集注入逃脱：{}", e3);
        assert!(inject_all_caught_ok());
    }

    #[test]
    fn judge_precedence_order() {
        // 判定次序：ENOENT > EINVAL > EPERM > EBROKEN。
        assert_eq!(judge_link(0), Err(LnkDiag::ENoent.val()));
        assert_eq!(judge_link(LINK_ROWS as u32 + 1), Err(LnkDiag::ENoent.val()));
        let mut t1 = LinkTable::from_frozen();
        let _ = t1.set_face_raw(5, 3);
        let _ = t1.set_flag(5, false);
        assert_eq!(judge_link_on(&t1, 5), Err(LnkDiag::EInval.val()));
        let mut t2 = LinkTable::from_frozen();
        let _ = t2.set_flag(7, false);
        let _ = t2.set_expect(7, REJECT_B26);
        assert_eq!(judge_link_on(&t2, 7), Err(LnkDiag::EPerm.val()));
        let mut t3 = LinkTable::from_frozen();
        let _ = t3.set_expect(9, REJECT_B26);
        assert_eq!(judge_link_on(&t3, 9), Err(LnkDiag::EBroken.val()));
        assert_eq!(judge_link_on(&t3, 10), Err(LnkDiag::EBroken.val()));
        assert!(order_ok());
    }

    #[test]
    fn beat_reconcile_green_seal_monotonic() {
        // 40 拍对账绿 + 状态机只进不退 + 封账后写拒。
        let mut l = reconcile_all(true).unwrap_or_else(|_| ReconLedger::new());
        assert_eq!(l.len(), BEAT_CAP);
        assert_eq!(l.state(), ReconState::Reconciled);
        assert!(l.diffs_all_zero());
        assert_eq!(l.alert_count(), 0);
        assert!(l.seal().is_ok());
        assert_eq!(l.state(), ReconState::Sealed);
        assert_eq!(l.record(1, true), Err(LnkDiag::ESealed.val()));
        assert_eq!(l.seal(), Err(LnkDiag::ERewind.val()));
        // 拍口径两侧锚一致：SEG_BASE>>4 == BOOK_BASE，REJECT_B25 == BOOK_STEP。
        assert_eq!(SEG_BASE >> 4, BOOK_BASE);
        assert_eq!(REJECT_B25, BOOK_STEP);
        assert!(reconcile_green_ok());
    }

    #[test]
    fn beat_boundary_and_dup_rejected() {
        // 边界：拍号越界、重复拍、空账封账、原始口径越界。
        assert!(beat_boundary_ok());
        let mut l = ReconLedger::new();
        assert_eq!(l.record(0, true), Err(LnkDiag::EBeatRange.val()));
        assert_eq!(l.record(BEAT_CAP as u32 + 1, true), Err(LnkDiag::EBeatRange.val()));
        assert!(l.record(3, true).is_ok());
        assert_eq!(l.record(3, true), Err(LnkDiag::EDup.val()));
        assert_eq!(l.seal(), Err(LnkDiag::ENotFull.val()));
    }

    #[test]
    fn nonzero_diff_never_silent() {
        // 非零差零静默必告警：静默拒收、告警入账封账拦、注入 10 轮全抓。
        assert!(silent_alarm_ok());
        let (c, t, e) = beat_tamper_sweep();
        assert_eq!(t, 10);
        assert_eq!(c, 10, "非零差注入逃脱：{}", e);
    }

    #[test]
    fn registry_bounds_enoent() {
        // 未登记链节边界：0/25/MAX 全拒；翻动接口同样拒；优先性最高。
        assert!(enoent_ok());
        let mut t = LinkTable::from_frozen();
        assert_eq!(t.set_flag(25, false), Err(LnkDiag::ENoent.val()));
        assert_eq!(t.set_expect(0, REJECT_B25), Err(LnkDiag::ENoent.val()));
        assert_eq!(t.set_face_raw(u32::MAX, 3), Err(LnkDiag::ENoent.val()));
        assert!(t.row_of(25).is_none());
        assert!(t.row_at(LINK_ROWS).is_none());
    }

    #[test]
    fn checkset_capacity_exhaustion() {
        // 容量耗尽：超出 MAX_CHECKS 的条目显性记账（dropped/truncated）。
        let mut s = CheckSet::new("unx12748-cap-probe");
        let mut i = 0usize;
        while i < MAX_CHECKS + 3 {
            s.add_dyn("容量探针", true, "容量耗尽探针条目");
            i += 1;
        }
        assert_eq!(s.len(), MAX_CHECKS);
        assert_eq!(s.dropped(), 3);
        assert!(s.truncated());
    }

    #[test]
    fn dual_run_fingerprint_stable() {
        // 双跑一致：指纹与三路注入战果、40 拍三元组逐项相同。
        let a = fingerprint();
        let b = fingerprint();
        assert_eq!(a, b);
        assert_eq!(inject_break_sweep(), inject_break_sweep());
        assert_eq!(inject_flag_sweep(), inject_flag_sweep());
        assert_eq!(inject_face_sweep(), inject_face_sweep());
        let l1 = reconcile_all(true).unwrap_or_else(|_| ReconLedger::new());
        let l2 = reconcile_all(true).unwrap_or_else(|_| ReconLedger::new());
        assert_eq!(l1.len(), l2.len());
        let mut i = 0usize;
        while i < BEAT_CAP {
            assert_eq!(l1.beat_at(i), l2.beat_at(i));
            i += 1;
        }
        assert!(dual_run_ok());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条，红项为零。
        let s = run_unx12748_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
        assert_eq!(s.len(), 17);
    }
}
