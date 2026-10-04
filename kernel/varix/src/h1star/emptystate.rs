//! F210 空状态设计规范 · 判据实装（H 基础通用域）。
//!
//! **判据锚**：主册 F210「空状态设计规范」。
//!
//! **验收标准（主册第一句）**：全系统空态清单审计（每处三件套齐备）。
//!
//! **判据要点（主册）**：第一次打开、搜索无结果、列表被清空——三种「空」
//! 都有设计；空状态 = 一幅 4K 插画 + 一句说明 + 一个行动按钮；说明文案
//! 禁责怪语气（「你没有文件」错，「这里还没有文件」对）；搜索无结果的
//! 空态带「换个关键词试试」+ 最近一次有效搜索；空态→有态过渡无布局跳变。
//!
//! **设计要点**：
//! - [`EmptyKind`] 三枚举（首次打开 / 搜索无结果 / 列表清空）；
//! - [`EmptyStateSpec`] 三件套齐备校验：4K 插画资产引用 + 说明 + 行动
//!   按钮；搜索无结果类额外强制「换个关键词试试」建议与最近一次有效
//!   搜索记录（主册专项）；
//! - 禁词表扫描 [`scan_blame`]：责怪句式（第二人称指摘）中英文匹配——
//!   「你没有文件」类命中、「这里还没有文件」类放行；「你的 xx 为空」
//!   组合句式单独判定；
//! - 过渡无跳变判定 [`transition_stable`]：空态与有态同一几何骨架
//!   （原点/宽度一致，高度差 ≤ [`SKELETON_TOL_PX`]）；
//! - 审计登记册 [`EmptyBook`]：登记制（容量上限 + 逐出最旧），逐条产出
//!   齐备/禁词/骨架三判，缺项清单非空即不可发布。
//!
//! **依赖锚点**：`crate::checks::CheckSet`（自检面）、
//! `crate::h1star::h1base::Rect`（几何骨架判定）。
//! 时间注入式纪律：本模块无时间状态。零堆热路径：扫描判定不分配；
//! Vec/String 只用于登记册。

use crate::checks::CheckSet;
use crate::h1star::h1base::Rect;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 几何骨架高度容差 px——主册 F210「空态→有态过渡无布局跳变」的判定
/// 容差（原点/宽度须全等，高度差 ≤8px 视为同一骨架；实装定值）。
pub const SKELETON_TOL_PX: i32 = 8;

/// 登记册容量（空态面登记上限，满则逐出最旧——容量与淘汰纪律）。
pub const EMPTY_CAP: usize = 128;

/// 搜索无结果空态的建议按钮字面——主册 F210「带『换个关键词试试』」。
pub const SUGGEST_REPHRASE: &str = "换个关键词试试";

/// 责怪句式禁词表——主册 F210「说明文案禁责怪语气（『你没有文件』错，
/// 『这里还没有文件』对）」；中英文各列（子串匹配）。
pub const BLAME_PHRASES: &[&str] = &[
    "你没有",
    "你还没",
    "你没有选择",
    "you have no",
    "you haven't",
    "you did not",
    "your ",
];

// ---------------------------------------------------------------------------
// 空态规格
// ---------------------------------------------------------------------------

/// 三种「空」——主册 F210：第一次打开、搜索无结果、列表被清空。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmptyKind {
    /// 第一次打开（从未有过内容）。
    FirstOpen,
    /// 搜索无结果。
    SearchNoResult,
    /// 列表被清空（曾经有过）。
    ListCleared,
}

/// 空状态规格：三件套（4K 插画 + 一句说明 + 一个行动按钮）。
#[derive(Clone, Debug)]
pub struct EmptyStateSpec {
    pub kind: EmptyKind,
    /// 插画资产引用号（0 = 未挂插画）。
    pub illustration_id: u32,
    /// 插画是否走 4K 管线——主册「插画走 4K 管线验收」。
    pub illustration_4k: bool,
    /// 一句说明（禁责怪语气，登记时过禁词表）。
    pub message: String,
    /// 一个行动按钮（出路）。
    pub action: String,
    /// 搜索无结果类：是否带「换个关键词试试」。
    pub suggest_rephrase: bool,
    /// 搜索无结果类：最近一次有效搜索（原样回显给用户）。
    pub last_valid_query: String,
}

impl EmptyStateSpec {
    /// 三件套齐备校验。搜索无结果类额外要求建议句 + 最近有效搜索。
    pub fn is_complete(&self) -> bool {
        let base =
            self.illustration_id != 0 && self.illustration_4k && !self.message.is_empty()
                && !self.action.is_empty();
        if self.kind != EmptyKind::SearchNoResult {
            return base;
        }
        base && self.suggest_rephrase && !self.last_valid_query.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 禁词扫描与骨架判定
// ---------------------------------------------------------------------------

/// 责怪语气扫描：命中返回匹配到的禁词；干净返回 None。
///
/// 判定两条通路：(1) 禁词表子串命中；(2) 组合句式「你的 … 为空」
/// （「你的」与「为空」同现视为责怪）。中性表述「这里还没有文件」
/// 「文件夹是空的」不携带第二人称指摘，放行。
pub fn scan_blame(msg: &str) -> Option<&'static str> {
    let lower_has = |p: &str| {
        // 小写化只对 ASCII 生效即可：禁词表英文段全小写。
        let mut hit = false;
        let bytes = msg.as_bytes();
        let pl = p.len();
        if pl == 0 || bytes.len() < pl {
            return false;
        }
        let mut i = 0usize;
        while i + pl <= bytes.len() {
            let win = &bytes[i..i + pl];
            if win.iter().zip(p.as_bytes()).all(|(a, b)| a.to_ascii_lowercase() == *b) {
                hit = true;
                break;
            }
            i += 1;
        }
        hit
    };
    for p in BLAME_PHRASES {
        if lower_has(p) {
            return Some(p);
        }
    }
    if msg.contains("你的") && msg.contains("为空") {
        return Some("你的…为空");
    }
    None
}

/// 过渡无跳变判定：空态与有态同一几何骨架——原点/宽度全等，高度差
/// ≤ [`SKELETON_TOL_PX`]。
pub fn transition_stable(empty: Rect, filled: Rect) -> bool {
    empty.x == filled.x
        && empty.y == filled.y
        && empty.w == filled.w
        && (empty.h - filled.h).abs() <= SKELETON_TOL_PX
}

// ---------------------------------------------------------------------------
// 审计登记册
// ---------------------------------------------------------------------------

/// 一条空态审计行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmptyRow {
    pub id: u32,
    pub kind: EmptyKind,
    /// 三件套齐备。
    pub complete: bool,
    /// 禁词命中（None = 干净）。
    pub blame: Option<&'static str>,
    /// 过渡骨架稳定。
    pub stable: bool,
}

impl EmptyRow {
    /// 该处空态是否全判通过。
    pub const fn ok(&self) -> bool {
        self.complete && self.blame.is_none() && self.stable
    }
}

/// 审计汇总。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmptyAudit {
    pub total: usize,
    pub complete: usize,
    pub blame_hits: usize,
    pub unstable: usize,
}

impl EmptyAudit {
    /// 发布门：三件套全齐 + 禁词零命中 + 骨架零跳变。
    pub const fn publishable(&self) -> bool {
        self.complete == self.total && self.blame_hits == 0 && self.unstable == 0
    }
}

/// 空态登记册：全系统空态清单审计的唯一户口。
pub struct EmptyBook {
    rows: Vec<(u32, EmptyStateSpec, Rect, Rect)>,
    next_id: u32,
    /// 因容量满被逐出的最旧条数。
    pub evicted: u32,
}

impl EmptyBook {
    pub fn new() -> EmptyBook {
        EmptyBook { rows: Vec::new(), next_id: 1, evicted: 0 }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 登记一处空态：规格 + 空态几何 + 有态几何（过渡判定素材）。
    pub fn register(&mut self, spec: EmptyStateSpec, empty_rect: Rect, filled_rect: Rect) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        if self.rows.len() >= EMPTY_CAP {
            self.rows.remove(0);
            self.evicted += 1;
        }
        self.rows.push((id, spec, empty_rect, filled_rect));
        id
    }

    /// 最近一次有效搜索回显（搜索无结果类判据素材）。
    pub fn last_query_of(&self, id: u32) -> Option<&str> {
        self.rows
            .iter()
            .find(|(i, _, _, _)| *i == id)
            .map(|(_, s, _, _)| s.last_valid_query.as_str())
    }

    /// 逐条审计。
    pub fn audit_rows(&self) -> Vec<EmptyRow> {
        let mut out = Vec::new();
        for (id, spec, empty, filled) in &self.rows {
            out.push(EmptyRow {
                id: *id,
                kind: spec.kind,
                complete: spec.is_complete(),
                blame: scan_blame(&spec.message),
                stable: transition_stable(*empty, *filled),
            });
        }
        out
    }

    /// 审计汇总。
    pub fn audit(&self) -> EmptyAudit {
        let mut a = EmptyAudit { total: self.rows.len(), complete: 0, blame_hits: 0, unstable: 0 };
        for (_, spec, empty, filled) in &self.rows {
            if spec.is_complete() {
                a.complete += 1;
            }
            if scan_blame(&spec.message).is_some() {
                a.blame_hits += 1;
            }
            if !transition_stable(*empty, *filled) {
                a.unstable += 1;
            }
        }
        a
    }

    /// 缺项登记号清单（不齐备 / 禁词 / 跳变任一命中）。
    pub fn missing_ids(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for r in self.audit_rows() {
            if !r.ok() {
                out.push(r.id);
            }
        }
        out
    }
}

impl Default for EmptyBook {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 造一个三件套齐备的规格（按需带上搜索无结果专项字段）。
fn spec(kind: EmptyKind, msg: &str, search: Option<&str>) -> EmptyStateSpec {
    EmptyStateSpec {
        kind,
        illustration_id: 42,
        illustration_4k: true,
        message: String::from(msg),
        action: String::from("浏览模板库"),
        suggest_rephrase: search.is_some(),
        last_valid_query: String::from(search.unwrap_or("")),
    }
}

/// F210 自检（9 条行为级 + 1 条 fuzz）。
pub fn run_emptystate_checks() -> CheckSet {
    let mut set = CheckSet::new("F210-emptystate");

    // 1. 三种「空」齐备样张：FirstOpen / ListCleared 三件套判齐。
    let a = spec(EmptyKind::FirstOpen, "这里还没有文件", None);
    let c = spec(EmptyKind::ListCleared, "列表已清空", None);
    set.add(
        "first-open & list-cleared complete",
        a.is_complete() && c.is_complete(),
        "",
    );

    // 2. 搜索无结果类：缺「换个关键词试试」/ 最近有效搜索 → 不齐。
    let mut s = spec(EmptyKind::SearchNoResult, "没有找到匹配结果", None);
    set.add("search-no-result requires extras", !s.is_complete(), "");
    s.suggest_rephrase = true;
    set.add("search-no-result still needs last query", !s.is_complete(), "");
    s.last_valid_query = String::from("季度报表");
    set.add("search-no-result complete with extras", s.is_complete(), "");

    // 3. 三件套缺口：插画未挂（资产号 0）→ 不齐。
    let mut n = spec(EmptyKind::FirstOpen, "这里还没有文件", None);
    n.illustration_id = 0;
    set.add("missing illustration breaks kit", !n.is_complete(), "");
    // 4. 插画未走 4K 管线 → 不齐（主册「插画走 4K 管线验收」）。
    n.illustration_id = 7;
    n.illustration_4k = false;
    set.add("non-4k illustration breaks kit", !n.is_complete(), "");

    // 5. 禁词表：责怪句命中，中性句放行（中英文）。
    set.add(
        "blame scan hits & passes",
        scan_blame("你没有文件").is_some()
            && scan_blame("这里还没有文件").is_none()
            && scan_blame("你的回收站为空").is_some()
            && scan_blame("文件夹是空的").is_none()
            && scan_blame("You have no files").is_some(),
        "",
    );

    // 6. 过渡骨架：原点/宽全等且高差在容差内 → 稳定；高差超容差 → 跳变。
    let e = Rect::new(100, 80, 400, 300);
    set.add(
        "transition skeleton stable/unstable",
        transition_stable(e, Rect::new(100, 80, 400, 296))
            && transition_stable(e, Rect::new(100, 80, 400, 308))
            && !transition_stable(e, Rect::new(100, 80, 400, 320))
            && !transition_stable(e, Rect::new(99, 80, 400, 300)),
        "",
    );

    // 7. 登记册审计：齐备/禁词/跳变三判逐条产出，缺项清单回溯。
    let mut book = EmptyBook::new();
    let bad_rect_e = Rect::new(0, 0, 400, 300);
    let bad_rect_f = Rect::new(0, 0, 400, 500);
    let id_clean = book.register(spec(EmptyKind::FirstOpen, "这里还没有文件", None), bad_rect_e, bad_rect_e);
    let id_blame = book.register(spec(EmptyKind::ListCleared, "你没有文件", None), bad_rect_e, bad_rect_e);
    let id_jump = book.register(spec(EmptyKind::FirstOpen, "这里还没有文件", None), bad_rect_e, bad_rect_f);
    let rep = book.audit();
    set.add(
        "book audit three verdicts per row",
        rep.total == 3 && rep.complete == 3 && rep.blame_hits == 1 && rep.unstable == 1,
        "",
    );
    let miss = book.missing_ids();
    set.add(
        "missing list pins offenders",
        miss.len() == 2 && miss.contains(&id_blame) && miss.contains(&id_jump) && !miss.contains(&id_clean),
        "",
    );

    // 8. 最近有效搜索回显（登记后可取回，主册专项）。
    let qid = book.register(
        spec(EmptyKind::SearchNoResult, "没有找到匹配结果", Some("季度报表")),
        bad_rect_e,
        bad_rect_e,
    );
    set.add(
        "last valid query echoed back",
        book.last_query_of(qid) == Some("季度报表"),
        "",
    );

    // 9. 容量纪律：超 EMPTY_CAP 逐出最旧。
    let mut full = EmptyBook::new();
    for _ in 0..EMPTY_CAP + 2 {
        full.register(spec(EmptyKind::FirstOpen, "这里还没有文件", None), bad_rect_e, bad_rect_e);
    }
    set.add(
        "book evicts oldest beyond cap",
        full.len() == EMPTY_CAP && full.evicted == 2 && full.audit().publishable(),
        "",
    );

    // 10. fuzz（xors32 范式，2000 轮）：随机规格（随机缺件 + 随机禁词文案）
    //     ——审计计数与 brute force 同判，登记册不 panic。
    //     缺陷账本：现象=fuzz 恒红（rep.total==128≠2000）；根因=fuzz 期望
    //     按全量 2000 轮累计，但登记册容量在册（EMPTY_CAP=128、满则逐出
    //     最旧——检查 9 正是验这条纪律），期望统计越出了登记域，属 fuzz
    //     期望构造越界而非实现错；修法=逐轮判据入档，期望只在留册窗口
    //     （最新 EMPTY_CAP 轮）内统计。
    let mut x: u32 = 0x9E37_79B9;
    let msgs = [
        "这里还没有文件",
        "你没有文件",
        "You have no files",
        "列表已清空",
    ];
    let mut book = EmptyBook::new();
    // 每轮判据档案：(齐备, 禁词命中, 骨架跳变)。
    let mut verdicts: Vec<(bool, bool, bool)> = Vec::new();
    for i in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let kind = match (x >> 4) % 3 {
            0 => EmptyKind::FirstOpen,
            1 => EmptyKind::SearchNoResult,
            _ => EmptyKind::ListCleared,
        };
        let msg = msgs[(x >> 8) as usize % msgs.len()];
        let ill = if x & 1 == 0 { 9u32 } else { 0 };
        let k4 = x & 2 != 0;
        let has_action = x & 4 != 0;
        let act = if has_action { String::from("去添加") } else { String::new() };
        let search = if kind == EmptyKind::SearchNoResult && x & 8 != 0 {
            Some("q")
        } else {
            None
        };
        let spec = EmptyStateSpec {
            kind,
            illustration_id: ill,
            illustration_4k: k4,
            message: String::from(msg),
            action: act,
            suggest_rephrase: search.is_some(),
            last_valid_query: String::from(search.unwrap_or("")),
        };
        let filled_h = if x & 16 != 0 { 300i32 } else { 400 };
        let e = Rect::new(0, 0, 400, 300);
        let f = Rect::new(0, 0, 400, filled_h);
        // brute force 同判（逐轮入档，期望统计延后到留册窗口上做）。
        verdicts.push((spec.is_complete(), scan_blame(msg).is_some(), !transition_stable(e, f)));
        book.register(spec, e, f);
        let _ = i;
    }
    // 期望只在留册窗口内统计（2000 轮 > EMPTY_CAP → 留册 = 最新 128 轮）。
    let retain = 2000usize.min(EMPTY_CAP);
    let win = &verdicts[verdicts.len() - retain..];
    let exp_complete = win.iter().filter(|(c, _, _)| *c).count();
    let exp_blame = win.iter().filter(|(_, b, _)| *b).count();
    let exp_unstable = win.iter().filter(|(_, _, u)| *u).count();
    let rep = book.audit();
    set.add(
        "fuzz 2000: audit tallies match brute force",
        rep.total == retain
            && rep.complete == exp_complete
            && rep.blame_hits == exp_blame
            && rep.unstable == exp_unstable,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_kinds_each_have_design() {
        // 三种「空」都有设计：三件套齐备才算有。
        for (kind, msg) in [
            (EmptyKind::FirstOpen, "这里还没有文件"),
            (EmptyKind::SearchNoResult, "没有找到匹配结果"),
            (EmptyKind::ListCleared, "列表已清空"),
        ] {
            let s = spec(kind, msg, if kind == EmptyKind::SearchNoResult { Some("q") } else { None });
            assert!(s.is_complete(), "{:?} 三件套应齐备", kind);
        }
    }

    #[test]
    fn blame_scan_cjk_and_latin() {
        assert!(scan_blame("你没有选择任何文件").is_some());
        assert!(scan_blame("你的文档为空").is_some());
        assert!(scan_blame("You haven't added anything").is_some());
        assert!(scan_blame("这里还没有文件").is_none());
        assert!(scan_blame("回收站是空的").is_none());
    }

    #[test]
    fn skeleton_tolerance_boundary() {
        let e = Rect::new(10, 10, 320, 200);
        // 高差恰 8px：稳定（容差含边界）。
        assert!(transition_stable(e, Rect::new(10, 10, 320, 208)));
        // 高差 9px：跳变。
        assert!(!transition_stable(e, Rect::new(10, 10, 320, 209)));
    }

    #[test]
    fn registry_publish_gate() {
        let mut book = EmptyBook::new();
        let r = Rect::new(0, 0, 200, 120);
        book.register(spec(EmptyKind::FirstOpen, "你没有文件", None), r, r);
        assert!(!book.audit().publishable());
        assert_eq!(book.missing_ids().len(), 1);
    }

    #[test]
    fn selfcheck_all_green() {
        let set = run_emptystate_checks();
        assert!(set.all_passed(), "F210 自检存在红项");
        assert!(!set.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================

const VXH1_MAGIC: [u8; 4] = *b"VXH1";
const VXH1_VER: u8 = 1;

/// 损坏输入显性拒绝：四类 + 字段越界（kind 编码非法）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2CodecErr {
    BadMagic,
    BadVersion,
    BadLen,
    BadSum,
    BadField,
}

/// FNV-1a 32 位（校验和唯一实现点）。
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ---- 持久化 I/O 面：空态登记记录（三件套位图）----

/// 记录长：magic4 + ver1 + id4 + kind1 + flags1 + sum4。
pub const EMPTYREC_LEN: usize = 4 + 1 + 4 + 1 + 1 + 4;

/// 空态三件套位图位定义：bit0=插画已挂、bit1=4K 管线、bit2=说明非空、
/// bit3=行动非空、bit4=带「换个关键词试试」、bit5=带最近有效搜索。
pub const EFLAG_ILL: u8 = 1;
pub const EFLAG_4K: u8 = 2;
pub const EFLAG_MSG: u8 = 4;
pub const EFLAG_ACT: u8 = 8;
pub const EFLAG_SUG: u8 = 16;
pub const EFLAG_QRY: u8 = 32;

/// 空态类别 ↔ 字节（0..=2，其余拒绝）。
fn kind_enc(k: EmptyKind) -> u8 {
    match k {
        EmptyKind::FirstOpen => 0,
        EmptyKind::SearchNoResult => 1,
        EmptyKind::ListCleared => 2,
    }
}

fn kind_dec(v: u8) -> Option<EmptyKind> {
    match v {
        0 => Some(EmptyKind::FirstOpen),
        1 => Some(EmptyKind::SearchNoResult),
        2 => Some(EmptyKind::ListCleared),
        _ => None,
    }
}

/// 一处空态的字节级登记记录：三件套齐/缺以位图承载（位图与
/// `EmptyStateSpec::is_complete` 同判——一处一事实）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmptyRegRec {
    pub id: u32,
    pub kind: EmptyKind,
    pub flags: u8,
}

impl EmptyRegRec {
    /// 从规格取档（id 由调用方传登记号）。
    pub fn of(spec: &EmptyStateSpec, id: u32) -> EmptyRegRec {
        let mut flags = 0u8;
        if spec.illustration_id != 0 {
            flags |= EFLAG_ILL;
        }
        if spec.illustration_4k {
            flags |= EFLAG_4K;
        }
        if !spec.message.is_empty() {
            flags |= EFLAG_MSG;
        }
        if !spec.action.is_empty() {
            flags |= EFLAG_ACT;
        }
        if spec.suggest_rephrase {
            flags |= EFLAG_SUG;
        }
        if !spec.last_valid_query.is_empty() {
            flags |= EFLAG_QRY;
        }
        EmptyRegRec { id, kind: spec.kind, flags }
    }

    pub fn to_bytes(&self) -> [u8; EMPTYREC_LEN] {
        let mut out = [0u8; EMPTYREC_LEN];
        out[..4].copy_from_slice(&VXH1_MAGIC);
        out[4] = VXH1_VER;
        out[5..9].copy_from_slice(&self.id.to_le_bytes());
        out[9] = kind_enc(self.kind);
        out[10] = self.flags;
        let sum = fnv1a(&out[..11]).to_le_bytes();
        out[11..15].copy_from_slice(&sum);
        out
    }

    pub fn from_bytes(b: &[u8]) -> Result<EmptyRegRec, V2CodecErr> {
        if b.len() != EMPTYREC_LEN {
            return Err(V2CodecErr::BadLen);
        }
        let mut mg = [0u8; 4];
        mg.copy_from_slice(&b[..4]);
        if mg != VXH1_MAGIC {
            return Err(V2CodecErr::BadMagic);
        }
        if b[4] != VXH1_VER {
            return Err(V2CodecErr::BadVersion);
        }
        let mut sum = [0u8; 4];
        sum.copy_from_slice(&b[11..15]);
        if fnv1a(&b[..11]) != u32::from_le_bytes(sum) {
            return Err(V2CodecErr::BadSum);
        }
        let kind = match kind_dec(b[9]) {
            Some(k) => k,
            None => return Err(V2CodecErr::BadField),
        };
        let mut id = [0u8; 4];
        id.copy_from_slice(&b[5..9]);
        Ok(EmptyRegRec { id: u32::from_le_bytes(id), kind, flags: b[10] })
    }
}

// ---- UI 壳接线面：空态版面清单 + 占位几何 ----

/// 绘制图元：几何 + 颜色索引（0 = 插画令牌、1 = 文字令牌、2 = 按钮强调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2Prim {
    pub rect: Rect,
    pub color_idx: u8,
}

/// 空态版面清单（定长 3 图元）：0 = 4K 插画区（上半），1 = 说明行
/// （下半上格），2 = 行动按钮（下半下格、宽减半居中）——三件套
/// 「插画 + 一句说明 + 一个行动按钮」的壳层几何落点。
pub fn empty_layout_items(area: Rect) -> [V2Prim; 3] {
    let half_h = area.h / 2;
    let quarter_h = (area.h - half_h) / 2;
    [
        V2Prim { rect: Rect::new(area.x, area.y, area.w, half_h), color_idx: 0 },
        V2Prim { rect: Rect::new(area.x, area.y + half_h, area.w, quarter_h), color_idx: 1 },
        V2Prim {
            rect: Rect::new(area.x + area.w / 4, area.y + half_h + quarter_h, area.w / 2, quarter_h),
            color_idx: 2,
        },
    ]
}

/// 占位几何 = 有态几何同构（同 x/y/w/h）——空态→有态过渡的壳层纪律：
/// 空态绘制以有态骨架为占位，`transition_stable` 恒真（无布局跳变）。
pub fn placeholder_of(filled: Rect) -> Rect {
    Rect::new(filled.x, filled.y, filled.w, filled.h)
}

/// F210 v2 自检（首条恒为持久化 round-trip）。
pub fn run_emptystate_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F210-emptystate-v2");

    // 1. 持久化 round-trip：搜索无结果空态（全位图置位）编码→解码还原。
    let spec = EmptyStateSpec {
        kind: EmptyKind::SearchNoResult,
        illustration_id: 42,
        illustration_4k: true,
        message: String::from("没有找到匹配结果"),
        action: String::from("换个关键词试试"),
        suggest_rephrase: true,
        last_valid_query: String::from("季度报表"),
    };
    let rec = EmptyRegRec::of(&spec, 7);
    let bytes = rec.to_bytes();
    set.add(
        "v2 persist roundtrip empty reg rec",
        EmptyRegRec::from_bytes(&bytes) == Ok(rec)
            && rec.flags == (EFLAG_ILL | EFLAG_4K | EFLAG_MSG | EFLAG_ACT | EFLAG_SUG | EFLAG_QRY),
        "",
    );

    // 2. 损坏拒绝四类 + 字段越界（kind=9；重算 sum 使只坏字段）。
    let mut bad1 = bytes;
    bad1[0] = b'X';
    let mut bad2 = bytes;
    bad2[4] = 9;
    let mut bad3 = bytes;
    bad3[10] ^= 0xFF;
    let mut bad4 = bytes;
    bad4[9] = 9;
    let s4 = fnv1a(&bad4[..11]);
    bad4[11..15].copy_from_slice(&s4.to_le_bytes());
    set.add(
        "v2 persist rejects corrupt empty recs",
        EmptyRegRec::from_bytes(&bad1) == Err(V2CodecErr::BadMagic)
            && EmptyRegRec::from_bytes(&bad2) == Err(V2CodecErr::BadVersion)
            && EmptyRegRec::from_bytes(&bad3) == Err(V2CodecErr::BadSum)
            && EmptyRegRec::from_bytes(&bytes[..bytes.len() - 1]) == Err(V2CodecErr::BadLen)
            && EmptyRegRec::from_bytes(&bad4) == Err(V2CodecErr::BadField),
        "",
    );

    // 3. 位图同判：残缺规格位图如实反映缺口（缺插画 → bit0 清零）。
    let mut partial = spec.clone();
    partial.illustration_id = 0;
    let rec2 = EmptyRegRec::of(&partial, 8);
    set.add(
        "v2 flags bitmap reflects missing kit parts",
        rec2.flags & EFLAG_ILL == 0
            && rec2.flags & (EFLAG_4K | EFLAG_MSG | EFLAG_ACT | EFLAG_SUG | EFLAG_QRY)
                == (EFLAG_4K | EFLAG_MSG | EFLAG_ACT | EFLAG_SUG | EFLAG_QRY)
            && !partial.is_complete(),
        "",
    );

    // 4. 版面清单：三区纵向排布且全部内含于 area（插画在上、按钮居中在下）。
    let area = Rect::new(50, 40, 300, 200);
    let items = empty_layout_items(area);
    set.add(
        "v2 empty layout three zones inside area",
        items[0].rect == Rect::new(50, 40, 300, 100)
            && items[1].rect == Rect::new(50, 140, 300, 50)
            && items[2].rect == Rect::new(125, 190, 150, 50)
            && items.iter().all(|p| {
                p.rect.x >= area.x
                    && p.rect.y >= area.y
                    && p.rect.right() <= area.right()
                    && p.rect.bottom() <= area.bottom()
            }),
        "",
    );

    // 5. 占位几何 = 有态几何同构 → transition_stable 恒真（验主册
    //    F210「空态→有态过渡无布局跳变」的壳层纪律）。
    let filled = Rect::new(10, 20, 320, 240);
    let mut stable_all = true;
    for dh in [0i32, -8, 8] {
        let f = Rect::new(filled.x, filled.y, filled.w, filled.h + dh);
        if !transition_stable(placeholder_of(f), f) {
            stable_all = false;
        }
    }
    set.add("v2 placeholder geometry always stable", stable_all, "");

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn empty_rec_minimal_roundtrip() {
        let rec = EmptyRegRec { id: u32::MAX, kind: EmptyKind::ListCleared, flags: 0 };
        assert_eq!(EmptyRegRec::from_bytes(&rec.to_bytes()).unwrap(), rec);
    }

    #[test]
    fn layout_items_zero_area_safe() {
        // 极小区域不产生负宽/负高图元。
        let items = empty_layout_items(Rect::new(0, 0, 4, 4));
        assert!(items.iter().all(|p| p.rect.w >= 1 && p.rect.h >= 0));
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_emptystate_v2_checks();
        assert!(set.all_passed(), "F210 v2 自检存在红项");
        assert!(!set.truncated());
        assert!((4..=6).contains(&set.len()));
    }
}
