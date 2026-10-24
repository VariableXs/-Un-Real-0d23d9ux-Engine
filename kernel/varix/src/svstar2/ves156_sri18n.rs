//! VE-F3856 · 读屏国际化
//!
//! 读屏协同多语言：多语言标注（lang 语义渲染协同——复用 F2948 lang 单源）
//! + 播报语言跟随（内容语言切换播报语言不切=读错——跟随红线）
//! + 多语言公告（时区精神多语言版）+ RTL 布局读屏渲染协同（RTL 序断言）。
//! 错误路径：跟随失效→P1、RTL 序乱→P1、标注漏 lang→P1、公告错语→修正。

#![allow(dead_code)]

extern crate alloc;

use alloc::{vec, vec::Vec};

// ── 诊断码段（0xCC00）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xCC00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CCCCode(pub u16);

impl CCCCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const FOLLOW_FAIL: Self = Self(SEG_BASE + 1);
    pub const RTL_ORDER: Self = Self(SEG_BASE + 2);
    pub const LANG_MISSING: Self = Self(SEG_BASE + 3);
    pub const ANNOUNCE_LANG: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::FOLLOW_FAIL => "跟随失效→P1（语言切换演练）",
            Self::RTL_ORDER => "RTL 序乱→P1（断言实测）",
            Self::LANG_MISSING => "标注漏 lang→P1",
            Self::ANNOUNCE_LANG => "公告错语→修正",
            _ => "未知",
        }
    }
}

// ── lang 标注（复用 F2948 单源）─────────────────────────────────────

/// 语言标注条目（单源复述：与 F2948 共用 lang 语义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LangTag {
    pub node: u32,
    /// BCP-47 简码（'zh'/'en'/'ar'…）。
    pub lang: u16,
}

impl LangTag {
    pub fn zh(node: u32) -> Self {
        Self { node, lang: 0x_7A_68 & 0xFFFF }
    }
    pub fn en(node: u32) -> Self {
        Self { node, lang: 0x656E }
    }
    pub fn ar(node: u32) -> Self {
        Self { node, lang: 0x6172 }
    }
}

/// lang 标注册（单源：全部节点必须带 lang）。
#[derive(Debug, Clone)]
pub struct LangLedger {
    pub tags: Vec<LangTag>,
    /// 应标注节点全集。
    pub expected: Vec<u32>,
}

impl LangLedger {
    pub fn new(expected: &[u32]) -> Self {
        Self {
            tags: Vec::new(),
            expected: expected.to_vec(),
        }
    }

    /// 标注。
    pub fn tag(&mut self, node: u32, lang: u16) {
        self.tags.push(LangTag { node, lang });
    }

    /// 漏标节点。
    pub fn missing(&self) -> Vec<u32> {
        self.expected
            .iter()
            .filter(|n| !self.tags.iter().any(|t| t.node == **n))
            .copied()
            .collect()
    }
}

// ── 播报语言跟随断言 ────────────────────────────────────────────────

/// 语言切换演练记录：内容语言 + 播报语言成对。
#[derive(Debug, Clone, Copy)]
pub struct FollowShot {
    /// 内容语言。
    pub content_lang: u16,
    /// 播报语言。
    pub speak_lang: u16,
}

impl FollowShot {
    /// 跟随成立：播报语言与内容语言一致。
    pub fn following(&self) -> bool {
        self.content_lang == self.speak_lang
    }
}

/// 跟随断言器：语言切换演练集上判定。
#[derive(Debug, Clone)]
pub struct FollowAssert {
    pub shots: Vec<FollowShot>,
}

impl FollowAssert {
    pub fn new() -> Self {
        Self { shots: Vec::new() }
    }

    pub fn shot(&mut self, content: u16, speak: u16) {
        self.shots.push(FollowShot {
            content_lang: content,
            speak_lang: speak,
        });
    }

    /// 全部跟随成立。
    pub fn all_following(&self) -> bool {
        self.shots.iter().all(|s| s.following())
    }

    /// 读错（不跟随）的演练数——读错防线。
    pub fn misreads(&self) -> usize {
        self.shots.iter().filter(|s| !s.following()).count()
    }

    /// 修正：把不跟随的播报语言对齐内容语言。
    pub fn correct(&mut self) -> usize {
        let mut n = 0;
        for s in self.shots.iter_mut() {
            if !s.following() {
                s.speak_lang = s.content_lang;
                n += 1;
            }
        }
        n
    }
}

impl Default for FollowAssert {
    fn default() -> Self {
        Self::new()
    }
}

// ── 多语言公告 ──────────────────────────────────────────────────────

/// 多语言公告条目（时区精神多语言版：公告自带语言与区域）。
#[derive(Debug, Clone, Copy)]
pub struct MlAnnounce {
    /// 公告文本指纹。
    pub text_fp: u32,
    /// 公告语言。
    pub lang: u16,
    /// 区域码（时区精神：与语言绑定）。
    pub region: u8,
}

/// 公告语种核对：公告语言必须命中受支持语种表。
pub fn announce_lang_ok(a: &MlAnnounce, supported: &[u16]) -> bool {
    supported.contains(&a.lang)
}

// ── RTL 序断言 ──────────────────────────────────────────────────────

/// RTL 导航序：焦点遍历应自右向左（列号递减）。
#[derive(Debug, Clone)]
pub struct RtlOrder {
    /// 焦点遍历的列号序列。
    pub cols: Vec<u32>,
}

impl RtlOrder {
    pub fn new(cols: &[u32]) -> Self {
        Self { cols: cols.to_vec() }
    }

    /// RTL 序合法：严格递减。
    pub fn valid(&self) -> bool {
        for w in self.cols.windows(2) {
            if w[0] <= w[1] {
                return false;
            }
        }
        true
    }

    /// LTR 对照（同结构镜像）。
    pub fn ltr_mirror(&self) -> Vec<u32> {
        let max = self.cols.iter().copied().max().unwrap_or(0);
        self.cols.iter().map(|c| max + 1 - c).collect()
    }
}

// ── 国际化会话（总控）───────────────────────────────────────────────

/// 读屏国际化会话。
#[derive(Debug, Clone)]
pub struct I18nSession {
    pub ledger: LangLedger,
    pub follow: FollowAssert,
    pub announces: Vec<MlAnnounce>,
    pub rtl: RtlOrder,
    /// 受支持语种表。
    pub supported: Vec<u16>,
}

impl I18nSession {
    pub fn new(expected: &[u32], supported: &[u16], rtl_cols: &[u32]) -> Self {
        Self {
            ledger: LangLedger::new(expected),
            follow: FollowAssert::new(),
            announces: Vec::new(),
            rtl: RtlOrder::new(rtl_cols),
            supported: supported.to_vec(),
        }
    }

    /// 会话总检。
    pub fn full_check(&self) -> Result<(), CCCCode> {
        if !self.ledger.missing().is_empty() {
            return Err(CCCCode::LANG_MISSING);
        }
        if self.follow.misreads() > 0 {
            return Err(CCCCode::FOLLOW_FAIL);
        }
        if !self.rtl.valid() {
            return Err(CCCCode::RTL_ORDER);
        }
        if self
            .announces
            .iter()
            .any(|a| !announce_lang_ok(a, &self.supported))
        {
            return Err(CCCCode::ANNOUNCE_LANG);
        }
        Ok(())
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves156_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves156_sri18n");

    fn ready() -> I18nSession {
        let mut s = I18nSession::new(&[1, 2, 3], &[0x7A68 & 0xFFFF, 0x656E], &[9, 7, 5]);
        s.ledger.tag(1, 0x7A68 & 0xFFFF);
        s.ledger.tag(2, 0x656E);
        s.ledger.tag(3, 0x6172);
        s.follow.shot(0x656E, 0x656E);
        s.follow.shot(0x7A68 & 0xFFFF, 0x7A68 & 0xFFFF);
        s
    }

    // 01 lang 单源标注齐备
    {
        let s = ready();
        chk(&mut set, "C3856-01", s.ledger.missing().is_empty(), "err");
    }
    // 02 标注漏 lang→P1
    {
        let mut s = I18nSession::new(&[1, 2, 3], &[0x656E], &[5, 3, 1]);
        s.ledger.tag(1, 0x656E);
        let miss = s.ledger.missing();
        chk(
            &mut set,
            "C3856-02",
            miss == vec![2, 3] && s.full_check() == Err(CCCCode::LANG_MISSING),
            "err",
        );
    }
    // 03 跟随成立判定
    {
        let f = FollowShot { content_lang: 0x656E, speak_lang: 0x656E };
        chk(&mut set, "C3856-03", f.following(), "err");
    }
    // 04 读错：内容切语播报不切
    {
        let f = FollowShot { content_lang: 0x656E, speak_lang: 0x7A68 & 0xFFFF };
        chk(&mut set, "C3856-04", !f.following(), "err");
    }
    // 05 跟随演练全过
    {
        let s = ready();
        chk(&mut set, "C3856-05", s.follow.all_following() && s.follow.misreads() == 0, "err");
    }
    // 06 跟随失效→P1
    {
        let mut s = ready();
        s.follow.shot(0x656E, 0x6172);
        chk(
            &mut set,
            "C3856-06",
            s.follow.misreads() == 1 && s.full_check() == Err(CCCCode::FOLLOW_FAIL),
            "err",
        );
    }
    // 07 读错修正
    {
        let mut s = ready();
        s.follow.shot(0x656E, 0x6172);
        let fixed = s.follow.correct();
        chk(
            &mut set,
            "C3856-07",
            fixed == 1 && s.follow.all_following(),
            "err",
        );
    }
    // 08 RTL 序合法（递减）
    {
        let s = ready();
        chk(&mut set, "C3856-08", s.rtl.valid(), "err");
    }
    // 09 RTL 序乱→P1
    {
        let mut s = ready();
        s.rtl = RtlOrder::new(&[5, 7, 3]);
        chk(
            &mut set,
            "C3856-09",
            !s.rtl.valid() && s.full_check() == Err(CCCCode::RTL_ORDER),
            "err",
        );
    }
    // 10 RTL 相等序也判乱（严格递减）
    {
        let r = RtlOrder::new(&[5, 5, 3]);
        chk(&mut set, "C3856-10", !r.valid(), "err");
    }
    // 11 LTR 镜像换算
    {
        let r = RtlOrder::new(&[9, 7]);
        chk(&mut set, "C3856-11", r.ltr_mirror() == vec![1, 3], "err");
    }
    // 12 多语言公告语种命中
    {
        let a = MlAnnounce { text_fp: 1, lang: 0x656E, region: 1 };
        chk(
            &mut set,
            "C3856-12",
            announce_lang_ok(&a, &[0x656E, 0x7A68 & 0xFFFF]),
            "err",
        );
    }
    // 13 公告错语→修正路径
    {
        let mut s = ready();
        s.announces.push(MlAnnounce { text_fp: 2, lang: 0x6465, region: 7 });
        chk(
            &mut set,
            "C3856-13",
            s.full_check() == Err(CCCCode::ANNOUNCE_LANG),
            "err",
        );
    }
    // 14 公告齐备→总检绿
    {
        let mut s = ready();
        s.announces.push(MlAnnounce { text_fp: 3, lang: 0x656E, region: 1 });
        chk(&mut set, "C3856-14", s.full_check() == Ok(()), "err");
    }
    // 15 语种码互异
    {
        let zh = 0x7A68 & 0xFFFF;
        chk(
            &mut set,
            "C3856-15",
            zh != 0x656E && 0x656E != 0x6172 && zh != 0x6172,
            "err",
        );
    }
    // 16 漏标修正闭环
    {
        let mut s = ready();
        s.ledger.tags.clear();
        let miss = s.ledger.missing();
        for n in miss.iter() {
            s.ledger.tag(*n, 0x656E);
        }
        chk(&mut set, "C3856-16", miss.len() == 3 && s.ledger.missing().is_empty(), "err");
    }
    // 17 跟随集空判通过（无演练无读错）
    {
        let f = FollowAssert::new();
        chk(&mut set, "C3856-17", f.all_following() && f.misreads() == 0, "err");
    }
    // 18 诊断码互异
    {
        let codes = [
            CCCCode::FOLLOW_FAIL,
            CCCCode::RTL_ORDER,
            CCCCode::LANG_MISSING,
            CCCCode::ANNOUNCE_LANG,
        ];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
            }
        }
        chk(&mut set, "C3856-18", distinct, "err");
    }
    set
}
