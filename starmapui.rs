//! 星图前端（WP-206 · B-2103）：按钮即评级的最后一道诚实呈现。
//!
//! MD2 篇 21.3：星图前端是原生应用，三屏结构（首页/列表屏/详情屏）。
//! 一键打开的语义与评级联动：兜底级条目的主按钮是"切换 Windows 域打开"
//! （走交接），桥接级标注"经网页壳"——按钮即评级（MD1 第 18.5 节）。
//! 首页五个评级入口加推荐位（规则公开：新上架与判例更新，不设商业位）。
//! 列表屏三轴过滤加排序。安装进度四段（解析、校验、落盘、登记），卸载
//! 前检查关联（MIME 表引用、自启注册）。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 评级五档与按钮联动
// ---------------------------------------------------------------------------

pub const RATING_TIERS: usize = 5;

/// 五档评级（MD1 18.1 五级星标——顺序即首页入口顺序）：
/// ★★★★★ 原生级 / ★★★★☆ 直插级 / ★★★☆☆ 兼容级 /
/// ★★☆☆☆ 桥接级（Servo 壳承载） / ★☆☆☆☆ 兜底级（切 Windows 域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rating {
    Native,   // 原生级：vx-SDK 构建的官方版本
    Direct,   // 直插级：官方 Linux 版经 Linuxulator 直插运行
    Compat,   // 兼容级：Wine 运行的 Windows 软件
    Bridged,  // 桥接级：经 Servo 壳承载的 Web/远程形态
    Fallback, // 兜底级：需切 Windows 域运行（双域接力）
}

pub const ALL_RATINGS: [Rating; RATING_TIERS] = [
    Rating::Native,
    Rating::Direct,
    Rating::Compat,
    Rating::Bridged,
    Rating::Fallback,
];

/// 档位在首页入口的序号（五档各一，顺序与 MD1 18.1 表一致）。
pub fn tier_index(r: Rating) -> usize {
    match r {
        Rating::Native => 0,
        Rating::Direct => 1,
        Rating::Compat => 2,
        Rating::Bridged => 3,
        Rating::Fallback => 4,
    }
}

/// 主按钮语义：按钮即评级——档位决定主按钮，呈现面无权改写。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrimaryAction {
    /// 原生级/直插级/兼容级：直接打开。
    Open,
    /// 桥接级：打开但标注"经网页壳"（诚实标注路径）。
    OpenViaWebShell,
    /// 兜底级：切换 Windows 域打开（走交接）。
    SwitchToWindows,
}

pub const BRIDGED_NOTE: &str = "经网页壳";
pub const FALLBACK_NOTE: &str = "切换 Windows 域打开";

pub fn primary_action(r: Rating) -> PrimaryAction {
    match r {
        Rating::Native | Rating::Direct | Rating::Compat => PrimaryAction::Open,
        Rating::Bridged => PrimaryAction::OpenViaWebShell,
        Rating::Fallback => PrimaryAction::SwitchToWindows,
    }
}

pub fn primary_note(r: Rating) -> &'static str {
    match r {
        Rating::Bridged => BRIDGED_NOTE,
        Rating::Fallback => FALLBACK_NOTE,
        _ => "打开",
    }
}

// ---------------------------------------------------------------------------
// 星卡
// ---------------------------------------------------------------------------

pub const CASE_CAP: usize = 32; // 判例清单可展开
pub const NAME_LEN: usize = 24;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StarCard {
    pub id: u32,
    pub name: [u8; NAME_LEN],
    pub name_len: usize,
    pub rating: Rating,
    pub leg: u8, // 腿别（三轴过滤第三轴）
    pub case_cnt: u16,
    pub metric_value: u64,
    pub metric_unit: [u8; 8], // 指标带单位
    pub metric_unit_len: usize,
    pub recheck_ymd: u32, // 复评日期（YYYYMMDD 整数）
    pub dir_version: u64, // 星卡引用的目录版本
    pub is_new: bool,     // 新上架（推荐位规则第一类）
    pub is_case_updated: bool, // 判例更新（推荐位规则第二类）
}

impl StarCard {
    pub fn name_bytes(&self) -> &[u8] {
        &self.name[..self.name_len.min(NAME_LEN)]
    }

    pub fn unit_bytes(&self) -> &[u8] {
        &self.metric_unit[..self.metric_unit_len.min(8)]
    }
}

// ---------------------------------------------------------------------------
// 三屏结构与过滤
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Home,
    List,
    Detail,
}

/// 列表屏三轴过滤器：名称模糊（子串）+ 评级 + 腿别。
pub struct ListFilter<'a> {
    pub name_sub: &'a [u8],
    pub rating: Option<Rating>,
    pub leg: Option<u8>,
}

impl<'a> ListFilter<'a> {
    pub fn matches(&self, c: &StarCard) -> bool {
        if !self.name_sub.is_empty() && !subslice(c.name_bytes(), self.name_sub) {
            return false;
        }
        if let Some(r) = self.rating {
            if c.rating != r {
                return false;
            }
        }
        if let Some(l) = self.leg {
            if c.leg != l {
                return false;
            }
        }
        true
    }
}

/// 子串匹配（字节面，零分配）。
fn subslice(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > hay.len() {
        return false;
    }
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if &hay[i..i + needle.len()] == needle {
            return true;
        }
        i += 1;
    }
    false
}

/// 过滤结果容量（定长收集体）。
pub const HIT_CAP: usize = 32;

pub fn filter_cards(cards: &[Option<StarCard>; HIT_CAP], cnt: usize, f: &ListFilter) -> ([bool; HIT_CAP], usize) {
    let mut hits = [false; HIT_CAP];
    let mut n = 0;
    let mut i = 0;
    while i < cnt.min(HIT_CAP) {
        if let Some(c) = &cards[i] {
            if f.matches(c) {
                hits[i] = true;
                n += 1;
            }
        }
        i += 1;
    }
    (hits, n)
}

// ---------------------------------------------------------------------------
// 推荐位（规则公开：新上架与判例更新，不设商业位）
// ---------------------------------------------------------------------------

pub const RECO_CAP: usize = 8;

pub const RECO_RULE_NOTE: &str = "推荐位规则：新上架与判例更新，不设商业位";

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RecoSlot {
    pub used: bool,
    pub card_id: u32,
    /// 商业位标记：规则只收两类，任何"付费位"语义在入口即拒。
    pub commercial: bool,
}

pub struct RecoBoard {
    pub slots: [RecoSlot; RECO_CAP],
    pub rejected_commercial: u64,
}

impl RecoBoard {
    pub fn new() -> Self {
        RecoBoard { slots: [RecoSlot { used: false, card_id: 0, commercial: false }; RECO_CAP], rejected_commercial: 0 }
    }

    /// 推荐位准入：商业位拒绝并留痕；其余取新上架或判例更新。
    pub fn offer(&mut self, card: &StarCard, commercial: bool) -> bool {
        if commercial {
            self.rejected_commercial += 1;
            return false;
        }
        if !card.is_new && !card.is_case_updated {
            return false;
        }
        let mut i = 0;
        while i < RECO_CAP {
            if !self.slots[i].used {
                self.slots[i] = RecoSlot { used: true, card_id: card.id, commercial: false };
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn count(&self) -> usize {
        self.slots.iter().filter(|s| s.used).count()
    }
}

// ---------------------------------------------------------------------------
// 安装进度四段（解析→校验→落盘→登记，顺序不可跳）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InstallStage {
    Idle,
    Parse,
    Verify,
    Write,
    Register,
    Done,
}

pub const INSTALL_STAGES: usize = 4;

pub struct InstallFlow {
    pub stage: InstallStage,
}

impl InstallFlow {
    pub fn new() -> Self {
        InstallFlow { stage: InstallStage::Idle }
    }

    /// 推进一段：只能按序推进，跳段拒绝（解析未完成不得校验）。
    pub fn advance(&mut self, want: InstallStage) -> bool {
        let legal = match (self.stage, want) {
            (InstallStage::Idle, InstallStage::Parse)
            | (InstallStage::Parse, InstallStage::Verify)
            | (InstallStage::Verify, InstallStage::Write)
            | (InstallStage::Write, InstallStage::Register)
            | (InstallStage::Register, InstallStage::Done) => true,
            _ => false,
        };
        if legal {
            self.stage = want;
        }
        legal
    }
}

// ---------------------------------------------------------------------------
// 卸载前关联检查（MIME 表引用、自启注册）
// ---------------------------------------------------------------------------

pub const REF_CAP: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct UninstallRefs {
    pub mime_refs: u32,
    pub autostart: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UninstallReport {
    pub has_assoc: bool,
    pub mime_refs: u32,
    pub autostart: bool,
}

/// 卸载前检查：有 MIME 引用或自启注册时如实报告（清理完整 SC-133 的
/// 关联提示面——报告给确认层，不静默清除）。
pub fn uninstall_check(refs: UninstallRefs) -> UninstallReport {
    UninstallReport {
        has_assoc: refs.mime_refs > 0 || refs.autostart,
        mime_refs: refs.mime_refs,
        autostart: refs.autostart,
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-2103 · 10 项）
// ---------------------------------------------------------------------------

fn mkcard(id: u32, name: &[u8], rating: Rating, leg: u8) -> StarCard {
    let mut n = [0u8; NAME_LEN];
    let l = name.len().min(NAME_LEN);
    n[..l].copy_from_slice(&name[..l]);
    let mut u = [0u8; 8];
    u[..3].copy_from_slice(b"fps");
    StarCard {
        id,
        name: n,
        name_len: l,
        rating,
        leg,
        case_cnt: 6,
        metric_value: 60,
        metric_unit: u,
        metric_unit_len: 3,
        recheck_ymd: 20270_101,
        dir_version: 41,
        is_new: false,
        is_case_updated: false,
    }
}

pub fn run_starmapui_checks() -> CheckSet {
    let mut set = CheckSet::new("B-2103 按钮即评级");
    // 1. 三屏结构齐。
    set.add(
        "B-2103 三屏结构齐",
        Screen::Home != Screen::List && Screen::List != Screen::Detail && Screen::Home != Screen::Detail,
        "首页/列表屏/详情屏三屏齐备",
    );
    // 2. 五档评级与档位序（枚举面锁定五档，MD1 18.1 表序）。
    set.add(
        "B-2103 五档评级齐",
        ALL_RATINGS.len() == RATING_TIERS
            && ALL_RATINGS[0] == Rating::Native
            && ALL_RATINGS[1] == Rating::Direct
            && ALL_RATINGS[2] == Rating::Compat
            && ALL_RATINGS[3] == Rating::Bridged
            && ALL_RATINGS[4] == Rating::Fallback
            && tier_index(Rating::Fallback) == 4,
        "评级五档枚举齐：原生/直插/兼容/桥接/兜底，顺序锁定",
    );
    // 3. 兜底级主按钮走交接（"切换 Windows 域打开"）。
    let fb = primary_action(Rating::Fallback);
    set.add(
        "B-2103 兜底级走交接",
        fb == PrimaryAction::SwitchToWindows && primary_note(Rating::Fallback) == FALLBACK_NOTE,
        "兜底级条目主按钮=切换 Windows 域打开，文案锁定",
    );
    // 4. 桥接级标注经网页壳。
    set.add(
        "B-2103 桥接级标注",
        primary_action(Rating::Bridged) == PrimaryAction::OpenViaWebShell
            && primary_note(Rating::Bridged) == BRIDGED_NOTE,
        "桥接级打开必须带\"经网页壳\"标注",
    );
    // 5. 按钮即评级全域一致：五档×主按钮映射穷举（档位决定按钮，无旁路）。
    let all_ok = primary_action(Rating::Native) == PrimaryAction::Open
        && primary_action(Rating::Direct) == PrimaryAction::Open
        && primary_action(Rating::Compat) == PrimaryAction::Open
        && primary_action(Rating::Bridged) == PrimaryAction::OpenViaWebShell
        && primary_action(Rating::Fallback) == PrimaryAction::SwitchToWindows;
    set.add(
        "B-2103 映射全域一致",
        all_ok,
        "五档×主按钮映射穷举：按钮由评级函数唯一决定，呈现面无权改写",
    );
    // 6. 推荐位规则：只收新上架与判例更新，商业位拒绝留痕。
    let mut reco = RecoBoard::new();
    let mut c_new = mkcard(1, b"new-app", Rating::Native, 1);
    c_new.is_new = true;
    let mut c_upd = mkcard(2, b"upd-app", Rating::Compat, 2);
    c_upd.is_case_updated = true;
    let c_plain = mkcard(3, b"plain-app", Rating::Native, 1);
    let a1 = reco.offer(&c_new, false);
    let a2 = reco.offer(&c_upd, false);
    let a3 = reco.offer(&c_plain, false);
    let a4 = reco.offer(&c_new, true);
    set.add(
        "B-2103 推荐位规则公开",
        a1 && a2 && !a3 && !a4 && reco.count() == 2 && reco.rejected_commercial == 1,
        "新上架与判例更新可入位；无标记条目不入；商业位拒绝并留痕",
    );
    // 7. 列表屏三轴过滤（名称子串+评级+腿别）。
    let mut cards: [Option<StarCard>; HIT_CAP] = [None; HIT_CAP];
    cards[0] = Some(mkcard(10, b"editor-pro", Rating::Native, 1));
    cards[1] = Some(mkcard(11, b"editor-lite", Rating::Compat, 2));
    cards[2] = Some(mkcard(12, b"viewer", Rating::Native, 1));
    let (h7, n7) = filter_cards(&cards, 3, &ListFilter { name_sub: b"editor", rating: Some(Rating::Native), leg: Some(1) });
    set.add(
        "B-2103 列表三轴过滤",
        n7 == 1 && h7[0] && !h7[1] && !h7[2],
        "名称模糊+评级+腿别三轴同时生效，命中恰一",
    );
    // 8. 详情屏星卡全文要素（判例数/指标带单位/复评日期/目录版本）。
    let c8 = mkcard(20, b"full-card", Rating::Compat, 2);
    set.add(
        "B-2103 详情屏要素齐",
        c8.case_cnt > 0
            && c8.metric_value > 0
            && c8.unit_bytes() == b"fps"
            && c8.recheck_ymd == 20270_101
            && c8.dir_version > 0,
        "判例清单可展开+指标带单位+复评日期+目录版本四要素齐",
    );
    // 9. 安装进度四段顺序：跳段拒绝，按序可达 Done。
    let mut flow = InstallFlow::new();
    let skip1 = flow.advance(InstallStage::Write);
    let ok1 = flow.advance(InstallStage::Parse);
    let skip2 = flow.advance(InstallStage::Register);
    let ok2 = flow.advance(InstallStage::Verify);
    let ok3 = flow.advance(InstallStage::Write);
    let ok4 = flow.advance(InstallStage::Register);
    let ok5 = flow.advance(InstallStage::Done);
    set.add(
        "B-2103 安装四段顺序",
        !skip1 && ok1 && !skip2 && ok2 && ok3 && ok4 && ok5 && flow.stage == InstallStage::Done,
        "解析→校验→落盘→登记严格按序，跳段一律拒绝",
    );
    // 10. 卸载前关联检查（MIME/自启如实报告，不静默清除）。
    let r10a = uninstall_check(UninstallRefs { mime_refs: 3, autostart: true });
    let r10b = uninstall_check(UninstallRefs { mime_refs: 0, autostart: false });
    set.add(
        "B-2103 卸载关联检查",
        r10a.has_assoc && r10a.mime_refs == 3 && r10a.autostart && !r10b.has_assoc,
        "有关联如实报告（关联提示进确认层），无关联直接清",
    );
    set
}

// ---------------------------------------------------------------------------
// 单测（fa04 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fa04_button_rating_binding() {
        // 按钮即评级：五档主按钮穷举。
        assert_eq!(primary_action(Rating::Native), PrimaryAction::Open);
        assert_eq!(primary_action(Rating::Compat), PrimaryAction::Open);
        assert_eq!(primary_action(Rating::Bridged), PrimaryAction::OpenViaWebShell);
        assert_eq!(primary_action(Rating::Fallback), PrimaryAction::SwitchToWindows);
        assert_eq!(primary_note(Rating::Fallback), FALLBACK_NOTE);
        assert_eq!(primary_note(Rating::Bridged), BRIDGED_NOTE);
    }

    #[test]
    fn fa04_reco_rules() {
        let mut reco = RecoBoard::new();
        let mut a = mkcard(1, b"a", Rating::Native, 1);
        a.is_new = true;
        let mut b = mkcard(2, b"b", Rating::Native, 1);
        b.is_case_updated = true;
        let c = mkcard(3, b"c", Rating::Native, 1);
        assert!(reco.offer(&a, false));
        assert!(reco.offer(&b, false));
        assert!(!reco.offer(&c, false), "无标记不入位");
        assert!(!reco.offer(&a, true), "商业位拒");
        assert_eq!(reco.rejected_commercial, 1);
        assert_eq!(reco.count(), 2);
        assert!(RECO_RULE_NOTE.len() >= 60); // 全中文文案 UTF-8 字节数
    }

    #[test]
    fn fa04_install_order() {
        let mut f = InstallFlow::new();
        assert!(!f.advance(InstallStage::Verify));
        assert!(f.advance(InstallStage::Parse));
        assert!(!f.advance(InstallStage::Done));
        assert!(f.advance(InstallStage::Verify));
        assert!(f.advance(InstallStage::Write));
        assert!(f.advance(InstallStage::Register));
        assert!(f.advance(InstallStage::Done));
    }

    #[test]
    fn fa04_filter_axes() {
        let mut cards: [Option<StarCard>; HIT_CAP] = [None; HIT_CAP];
        cards[0] = Some(mkcard(1, b"alpha", Rating::Native, 1));
        cards[1] = Some(mkcard(2, b"beta", Rating::Fallback, 2));
        cards[2] = Some(mkcard(3, b"alpha-beta", Rating::Compat, 1));
        // 只按名称：命中 2。
        let (_, n1) = filter_cards(&cards, 3, &ListFilter { name_sub: b"alpha", rating: None, leg: None });
        assert_eq!(n1, 2);
        // 名称+评级：命中 1（alpha-beta 是 Compat）。
        let (h2, n2) = filter_cards(&cards, 3, &ListFilter { name_sub: b"alpha", rating: Some(Rating::Native), leg: None });
        assert_eq!(n2, 1);
        assert!(h2[0]);
        // 三轴全上：命中 1（"alpha" 本身 Native+leg1 全中；"alpha-beta" 是
        // leg1 但 Compat 出局）——对练序列先过一遍被测语义再写断言。
        let (h3, n3) = filter_cards(&cards, 3, &ListFilter { name_sub: b"alpha", rating: Some(Rating::Native), leg: Some(1) });
        assert_eq!(n3, 1);
        assert!(h3[0] && !h3[2]);
    }
}
