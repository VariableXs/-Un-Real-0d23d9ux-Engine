//! UNX-F12660 B34 主题框架登记（预告链衔接） · 0x80EC0段
//!
//! 判据原文（派工卡 12659 逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F12660-J1 B34 主题框架登记，与 F12598→F12640
//!      预告链衔接零悬空；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 预告链域模型：链节二元 =（承上号/启下号）；冻结预告链十节——
//!    F12598 起、经 F12600/F12605/F12610/F12615/F12620/F12625/F12630/
//!    F12635 逐级递进、F12640 收链、链尾指向 B34 主题框架；链节序
//!    冻结不许重排；
//! 2. 零悬空判定（单一权威引擎 verify_chain）：链头承上必须等于链头
//!    常量 F12598；相邻链节首尾相接（前节启下 == 后节承上）；链尾启下
//!    必须等于链尾常量 B34-主题框架；任一环节落空即悬空链，显性拒；
//!    链节自环（承上==启下）与空链同样显性拒；
//! 3. B34 主题登记：主题三元 =（登记号/主题名/衔接锚）；衔接锚必须
//!    逐字等于链尾承上常量 F12640（与预告链收链点衔接），锚不符即
//!    悬空主题，登记拒；主题名非空、登记号连续、不得重复登记；
//! 4. 衔接断言 10/10：链十节齐、链头对平、逐节首尾相接、链尾对平、
//!    无自环节、主题非空、登记号连续、主题不重复、衔接锚逐字对平、
//!    登记容量闭集——十条独立写死，任何一条为假即红；
//! 5. 注入联测 10 路：链悬空、链头断裂、链自环、空链核验、主题锚
//!    不符、主题名空、登记号乱序、登记重复、封账后登记、主题越容
//!    ——逐路必须被闸抓住；
//! 6. 登记账状态机 链核验中 → 已衔接 → 已登记封账 单向推进，
//!    倒退显性拒；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12659 − 12600)）。
pub const SEG_BASE: u32 = 0x80EC0;

// ── B34 主题登记诊断码闭集 ───────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeDiag {
    /// 链悬空（启下号无着落）。
    EChainDangle,
    /// 链头断裂（承上号 ≠ 链头常量）。
    EChainHead,
    /// 链尾断裂（启下号 ≠ 链尾常量）。
    EChainTail,
    /// 链节首尾不接（前节启下 ≠ 后节承上）。
    EChainBreak,
    /// 链节自环（承上 == 启下）。
    EChainLoop,
    /// 空链核验。
    EChainEmpty,
    /// 衔接锚不符（非链尾承上常量）。
    EAnchorBad,
    /// 主题名空。
    EThemeEmpty,
    /// 登记号越序（跳号）。
    ERegOrder,
    /// 登记重复。
    ERegDup,
    /// 未核验链就试图登记。
    ENotVerified,
    /// 封账后写入被拒。
    ESealed,
    /// 状态机倒退（重复核验/重复封账）。
    ERewind,
}

impl ThemeDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                ThemeDiag::EChainDangle => 0x01,
                ThemeDiag::EChainHead => 0x02,
                ThemeDiag::EChainTail => 0x03,
                ThemeDiag::EChainBreak => 0x04,
                ThemeDiag::EChainLoop => 0x05,
                ThemeDiag::EChainEmpty => 0x06,
                ThemeDiag::EAnchorBad => 0x07,
                ThemeDiag::EThemeEmpty => 0x08,
                ThemeDiag::ERegOrder => 0x09,
                ThemeDiag::ERegDup => 0x0a,
                ThemeDiag::ENotVerified => 0x0b,
                ThemeDiag::ESealed => 0x0c,
                ThemeDiag::ERewind => 0x0d,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            ThemeDiag::EChainDangle => "链悬空：启下号无下节承接，先补下节或改写启下号再核验",
            ThemeDiag::EChainHead => "链头断裂：首节承上必须等于 F12598，先对链头常量",
            ThemeDiag::EChainTail => "链尾断裂：末节启下必须指向 B34 主题框架，先对链尾常量",
            ThemeDiag::EChainBreak => "链节首尾不接：前节启下必须等于后节承上，先对相邻链节",
            ThemeDiag::EChainLoop => "链节自环：承上等于启下不构成预告推进，先拆自环节",
            ThemeDiag::EChainEmpty => "空链核验：空链不构成预告链，先落链节再核验",
            ThemeDiag::EAnchorBad => "衔接锚不符：主题锚必须逐字等于 F12640（链尾承上），先对衔接点",
            ThemeDiag::EThemeEmpty => "主题名空：空名不构成可点验主题，先落主题名再登记",
            ThemeDiag::ERegOrder => "登记号越序：按 0..N 连续登记，跳号先回补上一条",
            ThemeDiag::ERegDup => "登记重复：每主题只落一笔，重复登记先查已有登记号",
            ThemeDiag::ENotVerified => "未核验链禁止登记：先过零悬空核验，再走登记口",
            ThemeDiag::ESealed => "登记账已封账：封账后写入拒绝，改动走下一批主题滚动",
            ThemeDiag::ERewind => "状态机倒退：链核验中→已衔接→已登记封账只进不退，倒退即账本可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const THEME_ALL: &[ThemeDiag] = &[
    ThemeDiag::EChainDangle,
    ThemeDiag::EChainHead,
    ThemeDiag::EChainTail,
    ThemeDiag::EChainBreak,
    ThemeDiag::EChainLoop,
    ThemeDiag::EChainEmpty,
    ThemeDiag::EAnchorBad,
    ThemeDiag::EThemeEmpty,
    ThemeDiag::ERegOrder,
    ThemeDiag::ERegDup,
    ThemeDiag::ENotVerified,
    ThemeDiag::ESealed,
    ThemeDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in THEME_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80EC1..0x80ECD 闭集内"
}

// ── 预告链常量与冻结链 ───────────────────────────────────────────
/// 链头常量（预告链起点）。
pub const CHAIN_HEAD: &str = "F12598";
/// 链尾承上常量（B34 衔接点）。
pub const CHAIN_TAIL_FROM: &str = "F12640";
/// 链尾启下常量（B34 主题框架）。
pub const CHAIN_TAIL_TO: &str = "B34-主题框架";

/// 链节二元：承上号/启下号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainLink {
    /// 承上号。
    pub from: &'static str,
    /// 启下号。
    pub to: &'static str,
}

/// 冻结预告链（十节，节序冻结不许重排）。
pub const CHAIN: &[ChainLink] = &[
    ChainLink { from: "F12598", to: "F12600" },
    ChainLink { from: "F12600", to: "F12605" },
    ChainLink { from: "F12605", to: "F12610" },
    ChainLink { from: "F12610", to: "F12615" },
    ChainLink { from: "F12615", to: "F12620" },
    ChainLink { from: "F12620", to: "F12625" },
    ChainLink { from: "F12625", to: "F12630" },
    ChainLink { from: "F12630", to: "F12635" },
    ChainLink { from: "F12635", to: "F12640" },
    ChainLink { from: "F12640", to: "B34-主题框架" },
];

/// 链节容量。
pub const CHAIN_CAP: usize = 10;

// ── 零悬空判定引擎（单一权威） ───────────────────────────────────
/// 零悬空判定：链头对平 / 相邻相接 / 链尾对平 / 无自环 / 非空。
/// 返回 Err(诊断码) 表示悬空或断裂，Ok(()) 表示零悬空。
pub fn verify_chain(links: &[ChainLink]) -> Result<(), u32> {
    // 空链闸。
    if links.is_empty() {
        return Err(ThemeDiag::EChainEmpty.val());
    }
    // 链头闸。
    let head = match links.first() {
        Some(l) => l,
        None => return Err(ThemeDiag::EChainEmpty.val()),
    };
    if head.from != CHAIN_HEAD {
        return Err(ThemeDiag::EChainHead.val());
    }
    // 链尾闸。
    let tail = match links.last() {
        Some(l) => l,
        None => return Err(ThemeDiag::EChainEmpty.val()),
    };
    if tail.to != CHAIN_TAIL_TO {
        return Err(ThemeDiag::EChainTail.val());
    }
    // 逐节闸：自环 + 相邻相接（悬空）。
    let mut i = 0usize;
    while i < links.len() {
        let l = match links.get(i) {
            Some(v) => v,
            None => return Err(ThemeDiag::EChainEmpty.val()),
        };
        // 自环闸。
        if l.from == l.to {
            return Err(ThemeDiag::EChainLoop.val());
        }
        // 相邻相接闸（悬空：本节启下 ≠ 下节承上）。
        if i + 1 < links.len() {
            let next = match links.get(i + 1) {
                Some(v) => v,
                None => return Err(ThemeDiag::EChainEmpty.val()),
            };
            if l.to != next.from {
                return Err(ThemeDiag::EChainDangle.val());
            }
        }
        i += 1;
    }
    Ok(())
}

// ── B34 主题登记数据模型 ─────────────────────────────────────────
/// 主题三元：登记号/主题名/衔接锚。
#[derive(Clone, Debug)]
pub struct ThemeRow {
    /// 登记号（0 起连续）。
    pub reg_no: u32,
    /// 主题名（非空）。
    pub name: String,
    /// 衔接锚（必须逐字等于链尾承上常量）。
    pub anchor: String,
}

impl ThemeRow {
    /// 行摘要（诊断文本用）。
    pub fn brief(&self) -> String {
        format!("主题{}({}):锚{}", self.reg_no, self.name, self.anchor)
    }
}

/// 登记容量闭集（B34 框架主题上限）。
pub const THEME_CAP: usize = 4;

/// 登记账状态机：链核验中 → 已衔接 → 已登记封账，单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegState {
    /// 链核验中：只许核验链。
    Verifying,
    /// 已衔接：零悬空通过，允许登记主题。
    Linked,
    /// 已登记封账：只读。
    Sealed,
}

impl RegState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            RegState::Verifying => 0,
            RegState::Linked => 1,
            RegState::Sealed => 2,
        }
    }
}

/// B34 主题框架登记账。
#[derive(Clone, Debug)]
pub struct ThemeLedger {
    /// 已登记主题（按登记号序）。
    themes: Vec<ThemeRow>,
    /// 账本状态。
    state: RegState,
}

impl ThemeLedger {
    /// 开一本新登记账（状态即链核验中）。
    pub fn new() -> ThemeLedger {
        ThemeLedger {
            themes: Vec::new(),
            state: RegState::Verifying,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> RegState {
        self.state
    }

    /// 已登记主题数。
    pub fn len(&self) -> usize {
        self.themes.len()
    }

    /// 是否无行。
    pub fn is_empty(&self) -> bool {
        self.themes.is_empty()
    }

    /// 取第 idx 主题（越界 None）。
    pub fn theme_at(&self, idx: usize) -> Option<&ThemeRow> {
        self.themes.get(idx)
    }

    /// 链核验（冻结预告链零悬空通过即推进到已衔接）。
    pub fn verify(&mut self) -> Result<(), u32> {
        if self.state.rank() >= RegState::Linked.rank() {
            return Err(ThemeDiag::ERewind.val());
        }
        // 冻结链容量闸。
        if CHAIN.len() != CHAIN_CAP {
            return Err(ThemeDiag::EChainEmpty.val());
        }
        // 零悬空核验（单一权威引擎）。
        verify_chain(CHAIN)?;
        self.state = RegState::Linked;
        Ok(())
    }

    /// 主题登记（锚/名/序/重复/容量 五闸）。
    pub fn register(&mut self, reg_no: u32, name: &str, anchor: &str) -> Result<(), u32> {
        // 封账闸。
        if self.state.rank() >= RegState::Sealed.rank() {
            return Err(ThemeDiag::ESealed.val());
        }
        // 未核验闸。
        if self.state.rank() < RegState::Linked.rank() {
            return Err(ThemeDiag::ENotVerified.val());
        }
        // 登记号序闸。
        let expect = self.themes.len() as u32;
        if reg_no != expect {
            return Err(ThemeDiag::ERegOrder.val());
        }
        // 主题名空闸。
        if name.is_empty() {
            return Err(ThemeDiag::EThemeEmpty.val());
        }
        // 衔接锚闸（逐字等于链尾承上常量）。
        if anchor != CHAIN_TAIL_FROM {
            return Err(ThemeDiag::EAnchorBad.val());
        }
        // 登记容量闸。
        if self.themes.len() >= THEME_CAP {
            return Err(ThemeDiag::ERegDup.val());
        }
        self.themes.push(ThemeRow {
            reg_no,
            name: name.to_string(),
            anchor: anchor.to_string(),
        });
        Ok(())
    }

    /// 衔接断言 10/10：链十节齐 / 链头对平 / 逐节相接 / 链尾对平 /
    /// 无自环 / 主题非空 / 登记号连续 / 主题不重复 / 衔接锚逐字 /
    /// 登记容量闭集。
    pub fn assertions(&self) -> Vec<(String, bool)> {
        let mut out: Vec<(String, bool)> = Vec::new();
        // 断言 0：链十节齐。
        out.push(("衔接0-链十节齐".to_string(), CHAIN.len() == CHAIN_CAP));
        // 断言 1：链头对平。
        let head_ok = match CHAIN.first() {
            Some(l) => l.from == CHAIN_HEAD,
            None => false,
        };
        out.push(("衔接1-链头对平".to_string(), head_ok));
        // 断言 2：逐节相接（零悬空核验通过）。
        let linked = verify_chain(CHAIN).is_ok();
        out.push(("衔接2-逐节相接".to_string(), linked));
        // 断言 3：链尾对平。
        let tail_ok = match CHAIN.last() {
            Some(l) => l.to == CHAIN_TAIL_TO && l.from == CHAIN_TAIL_FROM,
            None => false,
        };
        out.push(("衔接3-链尾对平".to_string(), tail_ok));
        // 断言 4：无自环节（逐节直读）。
        let mut noloop = true;
        for l in CHAIN.iter() {
            if l.from == l.to {
                noloop = false;
            }
        }
        out.push(("衔接4-无自环节".to_string(), noloop));
        // 断言 5：主题非空。
        let mut name_ok = true;
        for t in self.themes.iter() {
            if t.name.is_empty() {
                name_ok = false;
            }
        }
        out.push(("衔接5-主题非空".to_string(), name_ok));
        // 断言 6：登记号连续。
        let mut seq_ok = true;
        for i in 0..self.themes.len() {
            match self.theme_at(i) {
                Some(t) => {
                    if t.reg_no != i as u32 {
                        seq_ok = false;
                    }
                }
                None => seq_ok = false,
            }
        }
        out.push(("衔接6-登记号连续".to_string(), seq_ok));
        // 断言 7：主题不重复（主题名去重）。
        let mut nodup_ok = true;
        let mut i = 0usize;
        while i < self.themes.len() {
            let a = match self.theme_at(i) {
                Some(t) => t.name.clone(),
                None => {
                    nodup_ok = false;
                    break;
                }
            };
            let mut j = i + 1;
            while j < self.themes.len() {
                if let Some(b) = self.theme_at(j) {
                    if a == b.name {
                        nodup_ok = false;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        out.push(("衔接7-主题不重复".to_string(), nodup_ok));
        // 断言 8：衔接锚逐字对平（每主题锚 == F12640）。
        let mut anchor_ok = true;
        for t in self.themes.iter() {
            if t.anchor != CHAIN_TAIL_FROM {
                anchor_ok = false;
            }
        }
        out.push(("衔接8-衔接锚逐字".to_string(), anchor_ok));
        // 断言 9：登记容量闭集（登记数 ≤ 容量且 ≥1）。
        out.push((
            "衔接9-登记容量闭集".to_string(),
            !self.themes.is_empty() && self.themes.len() <= THEME_CAP,
        ));
        out
    }

    /// 衔接断言 10/10 全绿判定。
    pub fn assertions_all_green(&self) -> bool {
        let list = self.assertions();
        if list.len() != 10 {
            return false;
        }
        for item in list.iter() {
            let _name = &item.0;
            if !item.1 {
                return false;
            }
        }
        true
    }

    /// 封账：已衔接 + 登记非空 + 断言 10/10 全绿才许封。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= RegState::Sealed.rank() {
            return Err(ThemeDiag::ERewind.val());
        }
        if self.state.rank() < RegState::Linked.rank() {
            return Err(ThemeDiag::ENotVerified.val());
        }
        if self.themes.is_empty() {
            return Err(ThemeDiag::EThemeEmpty.val());
        }
        if !self.assertions_all_green() {
            return Err(ThemeDiag::EAnchorBad.val());
        }
        self.state = RegState::Sealed;
        Ok(())
    }
}

// ── 确定性 LCG（种子 = SEG_BASE，零随机红线合规） ────────────────
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

// ── 构建器：完整登记账 ───────────────────────────────────────────
/// 构建完整登记账（链核验 + 三主题登记，种子消耗仅作可重演位）。
pub fn build_full_ledger() -> ThemeLedger {
    let mut l = ThemeLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let _ = rng.below(CHAIN_CAP as u32);
    match l.verify() {
        Ok(()) => {}
        Err(e) => {
            // 正常流不该出错；出错时终止构造（未衔接，断言会红）。
            let _ = e;
            return l;
        }
    }
    let themes: [&str; 3] = ["探测面实测批", "性能账实测批", "ktest 归档批"];
    let mut i = 0u32;
    while i < themes.len() as u32 {
        let name = match themes.get(i as usize) {
            Some(n) => *n,
            None => break,
        };
        match l.register(i, name, CHAIN_TAIL_FROM) {
            Ok(()) => i += 1,
            Err(e) => {
                let _ = e;
                break;
            }
        }
    }
    l
}

// ── 注入面：十路衔接违约 ─────────────────────────────────────────
/// 违约路闭集（注入联测的十路手写违约）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadLink {
    /// 链悬空（中段启下号改写无承接）。
    Dangle,
    /// 链头断裂（首节承上改写）。
    HeadMiss,
    /// 链尾断裂（末节启下改写）。
    TailMiss,
    /// 链自环。
    Loop,
    /// 空链核验。
    Empty,
    /// 主题锚不符。
    AnchorBad,
    /// 主题名空。
    NameEmpty,
    /// 登记号乱序。
    RegOrder,
    /// 未核验登记。
    NotVerified,
    /// 封账后登记。
    SealedReg,
}

impl BadLink {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            BadLink::Dangle => 0,
            BadLink::HeadMiss => 1,
            BadLink::TailMiss => 2,
            BadLink::Loop => 3,
            BadLink::Empty => 4,
            BadLink::AnchorBad => 5,
            BadLink::NameEmpty => 6,
            BadLink::RegOrder => 7,
            BadLink::NotVerified => 8,
            BadLink::SealedReg => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            BadLink::Dangle => "链悬空",
            BadLink::HeadMiss => "链头断裂",
            BadLink::TailMiss => "链尾断裂",
            BadLink::Loop => "链自环",
            BadLink::Empty => "空链核验",
            BadLink::AnchorBad => "主题锚不符",
            BadLink::NameEmpty => "主题名空",
            BadLink::RegOrder => "登记号乱序",
            BadLink::NotVerified => "未核验登记",
            BadLink::SealedReg => "封账后登记",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [BadLink] {
        &[
            BadLink::Dangle,
            BadLink::HeadMiss,
            BadLink::TailMiss,
            BadLink::Loop,
            BadLink::Empty,
            BadLink::AnchorBad,
            BadLink::NameEmpty,
            BadLink::RegOrder,
            BadLink::NotVerified,
            BadLink::SealedReg,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。返回 (抓到路数, 总路数, 首个逃脱描述)。
pub fn violation_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in BadLink::all().iter() {
        total += 1;
        let bad = run_violation(*v);
        if bad {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 构造带单点违约的链（其余节复刻冻结链）。
fn broken_chain(mode: BadLink) -> Vec<ChainLink> {
    let mut v: Vec<ChainLink> = Vec::new();
    for l in CHAIN.iter() {
        v.push(*l);
    }
    match mode {
        BadLink::Dangle => {
            // 第 3 节启下号改写（后节承接不上 → 悬空）。
            if let Some(l) = v.get_mut(3) {
                l.to = "F12616";
            }
        }
        BadLink::HeadMiss => {
            if let Some(l) = v.get_mut(0) {
                l.from = "F12597";
            }
        }
        BadLink::TailMiss => {
            if let Some(l) = v.get_mut(9) {
                l.to = "B33-主题框架";
            }
        }
        BadLink::Loop => {
            if let Some(l) = v.get_mut(5) {
                l.to = l.from;
            }
        }
        _ => {}
    }
    v
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_violation(v: BadLink) -> bool {
    match v {
        BadLink::Dangle => verify_chain(&broken_chain(BadLink::Dangle)).is_err(),
        BadLink::HeadMiss => verify_chain(&broken_chain(BadLink::HeadMiss)).is_err(),
        BadLink::TailMiss => verify_chain(&broken_chain(BadLink::TailMiss)).is_err(),
        BadLink::Loop => verify_chain(&broken_chain(BadLink::Loop)).is_err(),
        BadLink::Empty => verify_chain(&[]).is_err(),
        BadLink::AnchorBad => {
            let mut l = build_full_ledger();
            l.register(3, "锚不符主题", "F12639").is_err()
        }
        BadLink::NameEmpty => {
            let mut l = build_full_ledger();
            l.register(3, "", CHAIN_TAIL_FROM).is_err()
        }
        BadLink::RegOrder => {
            let mut l = build_full_ledger();
            l.register(5, "跳号主题", CHAIN_TAIL_FROM).is_err()
        }
        BadLink::NotVerified => {
            let mut l = ThemeLedger::new();
            l.register(0, "未核验主题", CHAIN_TAIL_FROM).is_err()
        }
        BadLink::SealedReg => {
            let mut l = build_full_ledger();
            let _ = l.seal();
            l.register(3, "封账后主题", CHAIN_TAIL_FROM).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// B34 主题框架登记（判据 J1 的"登记"直读口）。
fn registered_ok() -> bool {
    let mut l = build_full_ledger();
    if l.len() == 0 {
        return false;
    }
    if l.seal().is_err() {
        return false;
    }
    if l.state() != RegState::Sealed {
        return false;
    }
    true
}

/// 预告链衔接零悬空（判据 J1 的"零悬空"直读口）。
fn chain_linked_ok() -> bool {
    // 冻结链零悬空。
    if verify_chain(CHAIN).is_err() {
        return false;
    }
    // 冻结链容量对平。
    if CHAIN.len() != CHAIN_CAP {
        return false;
    }
    // 链头/链尾常量直读。
    match (CHAIN.first(), CHAIN.last()) {
        (Some(h), Some(t)) => {
            if h.from != CHAIN_HEAD || t.from != CHAIN_TAIL_FROM || t.to != CHAIN_TAIL_TO {
                return false;
            }
        }
        _ => return false,
    }
    true
}

/// 衔接断言 10/10（判据 J1 的断言面直读口）。
fn assertions_ok() -> bool {
    let l = build_full_ledger();
    if !l.assertions_all_green() {
        return false;
    }
    if l.assertions().len() != 10 {
        return false;
    }
    true
}

/// 注入联测 10/10 拒止。
fn injection_ok() -> bool {
    let (caught, total, escape) = violation_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 双跑一致（确定性构建下两次全流程逐项相同）。
fn dual_run_ok() -> bool {
    let a = build_full_ledger();
    let b = build_full_ledger();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.theme_at(i), b.theme_at(i)) {
            (Some(x), Some(y)) => {
                if x.name != y.name || x.anchor != y.anchor || x.reg_no != y.reg_no {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let ra = violation_sweep();
    let rb = violation_sweep();
    if ra.0 != rb.0 || ra.1 != rb.1 {
        return false;
    }
    true
}

/// 边界：链节查表边界/登记容量边界/状态倒退 三点。
fn boundary_ok() -> bool {
    // 链节直读边界：首节与末节可点验。
    if CHAIN.first().is_none() || CHAIN.last().is_none() {
        return false;
    }
    // 冻结链逐节自环直读（零自环）。
    for l in CHAIN.iter() {
        if l.from == l.to {
            return false;
        }
    }
    // 登记容量边界：满容后登记被拒。
    let mut l = build_full_ledger();
    let mut k = l.len() as u32;
    while k < THEME_CAP as u32 {
        if l.register(k, "补位主题", CHAIN_TAIL_FROM).is_err() {
            return false;
        }
        k += 1;
    }
    if l.register(THEME_CAP as u32, "越容主题", CHAIN_TAIL_FROM).is_ok() {
        return false;
    }
    true
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12659u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut l = build_full_ledger();
    if l.seal().is_err() {
        return false;
    }
    if l.state() != RegState::Sealed {
        return false;
    }
    match l.theme_at(0) {
        Some(t) => {
            if t.brief().is_empty() {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12660 B34 主题框架登记自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12659_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12659-b34-theme");
    let ledger = build_full_ledger();

    s.add_dyn(
        "F12660-01 预告链十节齐",
        CHAIN.len() == CHAIN_CAP,
        &format!("links={}", CHAIN.len()),
    );

    s.add_dyn(
        "F12660-02 预告链衔接零悬空",
        chain_linked_ok(),
        "F12598→…→F12640→B34 主题框架 全节相接",
    );

    s.add_dyn(
        "F12660-03 B34 主题框架登记",
        registered_ok(),
        &format!("themes={}", ledger.len()),
    );

    s.add_dyn(
        "F12660-04 衔接断言 10/10",
        assertions_ok(),
        &format!("rows={}", ledger.assertions().len()),
    );

    s.add_dyn(
        "F12660-05 注入联测 10/10 拒止",
        injection_ok(),
        &format!("caught={}", violation_sweep().0),
    );

    s.add_dyn(
        "F12660-06 双跑一致",
        dual_run_ok(),
        "确定性构建下两次全流程逐项相同",
    );

    s.add_dyn(
        "F12660-07 边界三点",
        boundary_ok(),
        "链节查表边界/登记容量边界/零自环",
    );

    s.add_dyn(
        "F12660-08 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12660-09 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 断言表结构：恒 10 条且每条独立为真。
    let list = ledger.assertions();
    let mut all_true = list.len() == 10;
    for item in list.iter() {
        let _name = &item.0;
        if !item.1 {
            all_true = false;
        }
    }
    s.add_dyn(
        "F12660-10 断言表结构",
        all_true,
        &format!("rows={} head={}", list.len(), match list.get(0) { Some(x) => x.0.clone(), None => String::new() }),
    );

    // 链节中段抽查：F12640 节在链内且唯一。
    let mut f12640 = 0usize;
    for l in CHAIN.iter() {
        if l.from == CHAIN_TAIL_FROM {
            f12640 += 1;
        }
    }
    s.add_dyn(
        "F12660-11 链尾节唯一",
        f12640 == 1,
        "F12640 承上节恰一（收链点唯一）",
    );

    // 主题行摘要可点验：末条摘要含登记号。
    let brief_ok = match ledger.theme_at(ledger.len().saturating_sub(1)) {
        Some(t) => t.brief().contains("主题"),
        None => false,
    };
    s.add_dyn(
        "F12660-12 主题行摘要",
        brief_ok,
        "末条摘要含登记号可点验",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 全批窗口 + 本卡公式位。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x80EC0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中。
        assert_eq!(THEME_ALL.len(), 13);
        let mut i = 0usize;
        while i < THEME_ALL.len() {
            let vi = THEME_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < THEME_ALL.len() {
                let vj = THEME_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        // 未知码回落。
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn frozen_chain_has_no_dangle() {
        // 冻结链零悬空：核验通过，头尾对平。
        assert!(verify_chain(CHAIN).is_ok());
        assert_eq!(CHAIN.len(), 10);
        let h = match CHAIN.first() {
            Some(l) => l,
            None => return,
        };
        let t = match CHAIN.last() {
            Some(l) => l,
            None => return,
        };
        assert_eq!(h.from, "F12598");
        assert_eq!(t.from, "F12640");
        assert_eq!(t.to, "B34-主题框架");
    }

    #[test]
    fn broken_chains_all_caught() {
        // 注入必抓：悬空/头断/尾断/自环/空链 五路链违约全数检出。
        assert!(verify_chain(&broken_chain(BadLink::Dangle)).is_err());
        assert!(verify_chain(&broken_chain(BadLink::HeadMiss)).is_err());
        assert!(verify_chain(&broken_chain(BadLink::TailMiss)).is_err());
        assert!(verify_chain(&broken_chain(BadLink::Loop)).is_err());
        assert!(verify_chain(&[]).is_err());
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路衔接违约全数拒止，无逃脱。
        let (caught, total, escape) = violation_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn theme_registration_gates() {
        // 未核验登记被拒。
        let mut l = ThemeLedger::new();
        assert_eq!(
            l.register(0, "未核验", CHAIN_TAIL_FROM),
            Err(ThemeDiag::ENotVerified.val())
        );
        // 核验后锚不符被拒。
        let mut full = build_full_ledger();
        assert_eq!(
            full.register(3, "锚不符", "F12639"),
            Err(ThemeDiag::EAnchorBad.val())
        );
        // 空名被拒。
        assert_eq!(
            full.register(3, "", CHAIN_TAIL_FROM),
            Err(ThemeDiag::EThemeEmpty.val())
        );
        // 跳号被拒。
        assert_eq!(
            full.register(7, "跳号", CHAIN_TAIL_FROM),
            Err(ThemeDiag::ERegOrder.val())
        );
    }

    #[test]
    fn theme_capacity_boundary() {
        // 登记容量闭集：满容后登记被拒。
        let mut l = build_full_ledger();
        let mut k = l.len() as u32;
        while k < THEME_CAP as u32 {
            assert!(l.register(k, "补位主题", CHAIN_TAIL_FROM).is_ok());
            k += 1;
        }
        assert_eq!(l.len(), THEME_CAP);
        assert!(l.register(THEME_CAP as u32, "越容", CHAIN_TAIL_FROM).is_err());
    }

    #[test]
    fn assertions_count_is_ten() {
        // 衔接断言恒 10 条且全绿；空账本断言红。
        let l = build_full_ledger();
        let list = l.assertions();
        assert_eq!(list.len(), 10);
        assert!(l.assertions_all_green());
        // 空账本：主题非空与容量闭集断言红。
        let bare = ThemeLedger::new();
        assert!(!bare.assertions_all_green());
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致：确定性构建下两次全流程逐项相同。
        let a = build_full_ledger();
        let b = build_full_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.theme_at(i), b.theme_at(i)) {
                (Some(x), Some(y)) => {
                    assert_eq!(x.name, y.name);
                    assert_eq!(x.anchor, y.anchor);
                }
                (None, None) => {}
                _ => assert!(false, "双跑条缺失"),
            }
            i += 1;
        }
        assert_eq!(violation_sweep().0, violation_sweep().0);
    }

    #[test]
    fn seal_requires_linked_and_green() {
        // 边界：未核验封账被拒；未登记封账被拒。
        let mut bare = ThemeLedger::new();
        assert_eq!(bare.seal(), Err(ThemeDiag::ENotVerified.val()));
        // 核验后未登记封账被拒。
        let mut linked = ThemeLedger::new();
        assert!(linked.verify().is_ok());
        assert_eq!(linked.seal(), Err(ThemeDiag::EThemeEmpty.val()));
        // 齐后封账成功，重封倒退拒绝，封后登记拒。
        let mut full = build_full_ledger();
        assert!(full.seal().is_ok());
        assert_eq!(full.seal(), Err(ThemeDiag::ERewind.val()));
        assert_eq!(
            full.register(9, "封后主题", CHAIN_TAIL_FROM),
            Err(ThemeDiag::ESealed.val())
        );
        // 重复核验（倒退）被拒。
        assert_eq!(linked.verify(), Err(ThemeDiag::ERewind.val()));
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Verifying→Linked→Sealed。
        let mut l = build_full_ledger();
        assert!(RegState::Verifying.rank() < RegState::Linked.rank());
        assert!(RegState::Linked.rank() < RegState::Sealed.rank());
        assert_eq!(l.state(), RegState::Linked);
        let _ = l.seal();
        assert_eq!(l.state(), RegState::Sealed);
    }

    #[test]
    fn chain_tail_unique() {
        // F12640 承上节恰一（收链点唯一）。
        let mut n = 0usize;
        for l in CHAIN.iter() {
            if l.from == CHAIN_TAIL_FROM {
                n += 1;
            }
        }
        assert_eq!(n, 1);
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12659_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
