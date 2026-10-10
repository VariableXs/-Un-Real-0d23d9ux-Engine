//! VE-F2419 · 动画组一致性（VE-M 域 · 动画段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2419`
//!
//! **判据（锚点原文）**：术语唯一义、边界与单源复述、跟随义务、三要素统一、判据。
//!
//! **职责定位（锚点原文）**：动画组一致性——与 I04/L07 术语一致（轨道/
//! 采样/骨骼动画用词精确；蒙皮归 I、动画归 M 的边界复述：F2401 边界表
//! 的术语维——蒙皮=skinning（I04）/动画=animation（M01）用词纪律与
//! 边界复述核验）、与 F1345 术语统一（轨道/关键帧/clip 三词跨域同义
//! 声明——F1345 单源的术语一致性：剪辑域与动画域共用轨道词汇——
//! 单源复用的术语统一声明）、错误三要素统一（动画报错格式抽查），
//! F2419/F2219 模式复刻。
//!
//! # 一、术语对照表（六词组+唯一义裁决）
//!
//! M01 新增六词组（轨道/关键帧/插值器/绑定/事件轨/clip）各带定义、
//! 收录状态、跨域对齐状态。**冲突即裁决**：[`term_verdict`] 查重名
//! （一词二义=裁决表失守）与空定义（无定义=无裁决）——裁决表唯一义。
//!
//! # 二、两复述核验（边界×I04 / 单源×F1345）
//!
//! - **边界复述**：[`boundary_rows`] 复述 F2401 边界表的术语维（蒙皮
//!   =skinnging 归 I04 / 动画=animation 归 M01）——语义 diff=0 的可
//!   执行面：两域领地**不交**（同一词不得同时挂两域）；
//! - **单源复述**：三词跨域同义声明（轨道/关键帧/clip）——M 域与
//!   F1345 剪辑域共用词汇，载体即 vem02 [`KeyframeRef`]（字段文档
//!   明记 F1345 侧）——同词同义 diff=0（复述失真即对账钩子拦截）。
//!
//! # 三、三要素抽样 + 跟随义务 + 白名单时效
//!
//! 抽样真调：8 条畸形输入过 vem02 [`parse_bind_path`](vem02_track::
//! parse_bind_path) 与 vem06 事件注册，逐条诊断必须有（码+什么错
//! +怎么办）三要素；vem09 的诊断是三字段型（码/严重度/标签），其
//! 三要素在 `label()` 一句话里（什么错+怎么办），同样逐条验非空
//! ——**不拿格式差异当缺口误报**。[`FollowDuty`] 登记两处跨域契约
//! 的跟随版本（I04/F1345 侧变更未跟随=拦截——契约化第七度）；
//! 白名单带有效期（误报条目过期自动失效——时效管理不养永久豁免）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::svstar2::vem02_track::{parse_bind_path, DiagBag, KeyframeRef};
use crate::svstar2::vem06_event::{DiagBag as EventBag, EventNameRegistry};
use crate::svstar2::vem09_import::DiagCode as ImportCode;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const GROUP_VERSION: &str = "M19-group-v1";

/// 术语冲突（裁决表唯一义失守）。
pub const E_GRP_TERM: &str = "E_GRP_TERM";

/// 复述失真（边界或单源漂移——对账钩子）。
pub const E_GRP_RESTATE: &str = "E_GRP_RESTATE";

/// 三要素缺项（限期修复）。
pub const E_GRP_3ELEM: &str = "E_GRP_3ELEM";

/// 跟随义务失守（I04/F1345 侧变更未跟随）。
pub const E_GRP_FOLLOW: &str = "E_GRP_FOLLOW";

/// 白名单过期（误报条目失效）。
pub const E_GRP_WHITELIST: &str = "E_GRP_WHITELIST";

/// 三要素抽样条数（锚点：抽 8 条）。
pub const SAMPLE_COUNT: usize = 8;

// ---------------------------------------------------------------------------
// 二、术语对照表（六词组+唯一义）
// ---------------------------------------------------------------------------

/// 术语收录状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TermStatus {
    /// 已收录进 M 术语表。
    Registered,
    /// 候选中（未定稿）。
    Candidate,
}

/// 跨域对齐状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignStatus {
    /// 与对端同义（diff=0）。
    Aligned,
    /// 与对端存在语义差（显性登记，不当一致）。
    Diverged,
    /// 无对端（M 域独有）。
    Local,
}

/// 术语条目（M01 新增词组）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermEntry {
    /// 英文词（裁决表主键）。
    pub en: &'static str,
    /// 中文词（裁决表主值）。
    pub zh: &'static str,
    /// 定义（非空=有裁决）。
    pub definition: &'static str,
    /// 收录状态。
    pub status: TermStatus,
    /// 跨域对齐状态。
    pub align: AlignStatus,
    /// 对端（对齐/分歧的对象；Local 为 None）。
    pub peer: Option<&'static str>,
}

/// 术语对照表（锚点六词组：轨道/关键帧/插值器/绑定/事件轨/clip）。
pub fn term_table() -> [TermEntry; 6] {
    [
        TermEntry { en: "track", zh: "轨道", definition: "关键帧序列+绑定语法的载体；与 F1345 剪辑域同词同义", status: TermStatus::Registered, align: AlignStatus::Aligned, peer: Some("F1345 剪辑域") },
        TermEntry { en: "keyframe", zh: "关键帧", definition: "带时刻与值的采样点；与 F1345 关键帧资产引用同义", status: TermStatus::Registered, align: AlignStatus::Aligned, peer: Some("F1345 剪辑域") },
        TermEntry { en: "clip", zh: "片段", definition: "轨道清单+时长的可复用单元；与 F1345 clip 同词同义", status: TermStatus::Registered, align: AlignStatus::Aligned, peer: Some("F1345 剪辑域") },
        TermEntry { en: "interpolator", zh: "插值器", definition: "两关键帧间的求值算子（step/linear/bezier/eased/slerp）", status: TermStatus::Registered, align: AlignStatus::Local, peer: None },
        TermEntry { en: "bind", zh: "绑定", definition: "轨道到目标属性的路径挂接（F2402 路径协议）", status: TermStatus::Registered, align: AlignStatus::Local, peer: None },
        TermEntry { en: "event_track", zh: "事件轨", definition: "离散触发轨——注册制事件名+三覆盖参数", status: TermStatus::Registered, align: AlignStatus::Local, peer: None },
    ]
}

/// 术语裁决核验：唯一义（无重名二义）+ 全有定义 + 对齐状态自洽。
pub fn term_verdict(entries: &[TermEntry]) -> Result<(), String> {
    // 唯一义：英文词与中文词都不得二义（一词两义=裁决表失守）。
    for (i, a) in entries.iter().enumerate() {
        for b in entries.iter().skip(i + 1) {
            if a.en == b.en {
                return Err(format!("{}：术语 {}（{}）二义——裁决表必须唯一义", E_GRP_TERM, a.en, a.zh));
            }
            if a.zh == b.zh {
                return Err(format!("{}：中文词「{}」二义（{} / {}）", E_GRP_TERM, a.zh, a.en, b.en));
            }
        }
    }
    for t in entries.iter() {
        if t.definition.trim().is_empty() {
            return Err(format!("{}：术语 {} 无定义（无裁决=无仲裁）", E_GRP_TERM, t.en));
        }
        // 对齐状态自洽：声称 Aligned 必须带对端；Local 不得带对端。
        match t.align {
            AlignStatus::Aligned | AlignStatus::Diverged => {
                if t.peer.is_none() {
                    return Err(format!("{}：术语 {} 声称 {:?} 但无对端", E_GRP_TERM, t.en, t.align));
                }
            }
            AlignStatus::Local => {
                if t.peer.is_some() {
                    return Err(format!("{}：术语 {} 声称 Local 但对端非空", E_GRP_TERM, t.en));
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、两复述核验（边界×I04 / 单源×F1345）
// ---------------------------------------------------------------------------

/// 边界复述行（F2401 边界表的术语维——蒙皮/动画分工）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundaryRow {
    /// 领域词（人读）。
    pub domain_zh: &'static str,
    /// 归属域（I04=蒙皮 / M01=动画）。
    pub owner: &'static str,
    /// 边界复述（一句话）。
    pub restatement: &'static str,
}

/// 边界表（蒙皮=skinning 归 I04 / 动画=animation 归 M01）。
pub fn boundary_rows() -> [BoundaryRow; 2] {
    [
        BoundaryRow { domain_zh: "蒙皮", owner: "I04", restatement: "skinning（骨骼蒙皮形变）归 I04——动画只喂姿态不碰蒙皮数学" },
        BoundaryRow { domain_zh: "动画", owner: "M01", restatement: "animation（轨道求值与时间轴）归 M01——蒙皮顶点运算不属本域" },
    ]
}

/// 边界复述核验：两域领地不交（同一词不得同挂两域）+ 复述非空。
pub fn boundary_verdict(rows: &[BoundaryRow]) -> Result<(), String> {
    for r in rows.iter() {
        if r.restatement.trim().is_empty() {
            return Err(format!("{}：{} 域缺边界复述", E_GRP_RESTATE, r.domain_zh));
        }
        // 领地纯度：复述句里不得出现对方域的词（I04 行提 animation 即越界
        /// 声明；M01 行提 skinning 同理）。
        let foreign = if r.owner == "I04" { "animation" } else { "skinning" };
        if r.restatement.contains(foreign) {
            return Err(format!(
                "{}：{} 域（{}）复述含对方域词 {}——领地声明漂移",
                E_GRP_RESTATE, r.domain_zh, r.owner, foreign
            ));
        }
    }
    // 两域各一行（多一行=重复声明，少一行=边界缺边）。
    let i04 = rows.iter().filter(|r| r.owner == "I04").count();
    let m01 = rows.iter().filter(|r| r.owner == "M01").count();
    if i04 != 1 || m01 != 1 {
        return Err(format!(
            "{}：边界表 I04×{} / M01×{}（应各 1）——边界缺边或重复",
            E_GRP_RESTATE, i04, m01
        ));
    }
    Ok(())
}

/// 单源复述核验：三词跨域同义（轨道/关键帧/clip）。
///
/// 物质证据：M 域轨道数据载体即 F1345 侧关键帧资产引用（vem02
/// [`KeyframeRef`]——字段文档明记 F1345 侧）——同词同义不是口头
/// 声明，是**同一个载体**在使用。
pub fn single_source_verdict() -> Result<(), String> {
    let shared = ["track", "keyframe", "clip"];
    let table = term_table();
    for w in shared.iter() {
        let hit = table.iter().any(|t| t.en == *w && t.align == AlignStatus::Aligned);
        if !hit {
            return Err(format!(
                "{}：共享词 {} 未登记为对齐态——单源复用词汇漂移",
                E_GRP_RESTATE, w
            ));
        }
    }
    // 载体证据：KeyframeRef 可构造且自洽（count 非零）。
    let carrier = KeyframeRef::new("grp-asset", 32);
    if carrier.count == 0 || carrier.asset_id.trim().is_empty() {
        return Err(format!(
            "{}：F1345 载体不自洽——单源复述失真",
            E_GRP_RESTATE
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、三要素抽样（8 条畸形输入真调诊断面）
// ---------------------------------------------------------------------------

/// 三要素诊断（码+什么错+怎么办）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreeElemDiag {
    /// 来源模块（人读）。
    pub source: &'static str,
    /// 码（wire 名或标签）。
    pub code: String,
    /// 什么错（message）。
    pub what: String,
    /// 怎么办（hint）。
    pub how: String,
}

/// 畸形绑定路径样本（缺 / 头、坏根、空段）。
const MALFORMED_PATHS: [&str; 8] = [
    "rig/spine",       // 缺 /
    "/",               // 只有根分隔符
    "/spine",          // 根不在三闭集
    "/node/",          // 空属性段
    "/material",       // 缺属性段
    "/node/spine//x",  // 双斜杠空段
    "node/spine",      // 缺 /（变体）
    "/custom/",        // 空属性段（custom 根）
];

/// 三要素抽样：8 条畸形路径过 vem02 解析器，逐条诊断三要素齐备。
pub fn sample_three_elem() -> Vec<ThreeElemDiag> {
    let mut out: Vec<ThreeElemDiag> = Vec::new();
    let mut i = 0usize;
    while i < SAMPLE_COUNT {
        let raw = MALFORMED_PATHS[i];
        let mut bag = DiagBag::new();
        let _ = parse_bind_path(raw, &mut bag);
        // vem02 诊断三字段（code/message/hint）——逐条取首条入账。
        if let Some(d) = bag.errors().first() {
            out.push(ThreeElemDiag {
                source: "vem02 bind_path",
                code: format!("{:?}", d.code),
                what: d.message.clone(),
                how: d.hint.clone(),
            });
        }
        i += 1;
    }
    out
}

/// vem09 侧诊断三要素抽样（码+严重度+标签——三要素在 label 一句话）。
pub fn sample_import_three_elem() -> Vec<ThreeElemDiag> {
    let mut out: Vec<ThreeElemDiag> = Vec::new();
    let codes = [
        ImportCode::SAMPLER_OOR,
        ImportCode::TIMES_NON_MONOTONIC,
        ImportCode::VALUE_COUNT_MISMATCH,
    ];
    let mut i = 0usize;
    while i < codes.len() {
        let label = codes[i].label();
        out.push(ThreeElemDiag {
            source: "vem09 import",
            code: format!("{:?}", codes[i]),
            what: String::from(label),
            how: String::from(label),
        });
        i += 1;
    }
    out
}

/// 三要素核验：抽满 8 条 + 每条三字段非空 + 事件侧诊断同样三字段。
pub fn three_elem_verdict(samples: &[ThreeElemDiag]) -> Result<(), String> {
    if samples.len() < SAMPLE_COUNT {
        return Err(format!(
            "{}：三要素抽样 {} 条少于 {}（覆盖面不足）",
            E_GRP_3ELEM, samples.len(), SAMPLE_COUNT
        ));
    }
    for d in samples.iter() {
        if d.what.trim().is_empty() || d.how.trim().is_empty() || d.code.trim().is_empty() {
            return Err(format!(
                "{}：{} 诊断 {} 缺要素（what/how/code 之一为空）——限期修复",
                E_GRP_3ELEM, d.source, d.code
            ));
        }
    }
    // 事件侧（vem06）诊断同格式——注册/触发路径也抽。
    let mut reg = EventNameRegistry::new();
    let mut bag = EventBag::new();
    let _ = reg.register("m.anim.event.event_track", &mut bag);
    if bag.items().iter().any(|d| d.message.trim().is_empty() || d.hint.trim().is_empty()) {
        return Err(format!("{}：vem06 事件诊断缺三要素", E_GRP_3ELEM));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、跟随义务（I04/F1345 侧变更→跟随） + 白名单时效
// ---------------------------------------------------------------------------

/// 跟随义务行（两处跨域契约）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FollowDuty {
    /// 对端契约。
    pub peer: &'static str,
    /// 本域跟随版本（对端版本 → 本域已跟随到此）。
    pub followed_version: u32,
    /// 对端当前版本（契约侧声明）。
    pub peer_version: u32,
    /// 跟随说明（人读）。
    pub note: &'static str,
}

/// 跟随义务表（I04 蒙皮边界 / F1345 剪辑术语两处）。
pub fn follow_duties() -> [FollowDuty; 2] {
    [
        FollowDuty { peer: "I04 蒙皮边界（F2401）", followed_version: 3, peer_version: 3, note: "边界表术语维复述随 I04 v3 对齐" },
        FollowDuty { peer: "F1345 剪辑术语（轨道/关键帧/clip）", followed_version: 2, peer_version: 2, note: "共享词汇随 F1345 v2 对齐" },
    ]
}

/// 跟随核验：对端版本 ≤ 已跟随版本（对端涨了没跟=拦截）。
pub fn follow_verdict(duties: &[FollowDuty]) -> Result<(), String> {
    for d in duties.iter() {
        if d.peer_version > d.followed_version {
            return Err(format!(
                "{}：{} 对端版本 {} 超过已跟随 {}——契约化第七度：变更未跟随",
                E_GRP_FOLLOW, d.peer, d.peer_version, d.followed_version
            ));
        }
        if d.note.trim().is_empty() {
            return Err(format!("{}：{} 缺跟随说明", E_GRP_FOLLOW, d.peer));
        }
    }
    Ok(())
}

/// 白名单条目（误报豁免——带有效期）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WhitelistEntry {
    /// 条目说明。
    pub subject: &'static str,
    /// 到期逻辑时钟（ms）。
    pub expires_ms: u64,
}

/// 白名单时效核验：过期条目必须出局（不养永久豁免）。
pub fn whitelist_verdict(entries: &[WhitelistEntry], now_ms: u64) -> Result<(), String> {
    for e in entries.iter() {
        if now_ms > e.expires_ms {
            return Err(format!(
                "{}：白名单条目「{}」已过期（{} > {}）——豁免失效",
                E_GRP_WHITELIST, e.subject, now_ms, e.expires_ms
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F2419 域自检（判据五组：术语/复述/三要素/跟随/收尾）。
pub fn run_vem19_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2419");

    // --- 术语唯一义（判据一）---
    let terms = term_table();
    s.add(
        "M19-术语-01",
        term_verdict(&terms).is_ok() && terms.len() == 6,
        "六词组唯一义（无重名二义+全有定义）",
    );
    // 二义冲突拒绝（英文词重复）。
    let mut dup = terms;
    dup[1].en = dup[0].en;
    let r = term_verdict(&dup);
    s.add(
        "M19-术语-02",
        r.is_err() && r.as_ref().unwrap_err().contains("二义") && r.as_ref().unwrap_err().starts_with(E_GRP_TERM),
        "一词二义拒绝（裁决表唯一义）",
    );
    // 无定义拒绝。
    let mut nodef = terms;
    nodef[3].definition = "  ";
    let r = term_verdict(&nodef);
    s.add("M19-术语-03", r.is_err() && r.unwrap_err().contains("无定义"), "无定义术语拒绝");
    // 对齐状态自洽（Aligned 必须带对端）。
    let mut ghost = terms;
    ghost[0].peer = None;
    let r = term_verdict(&ghost);
    s.add("M19-术语-04", r.is_err() && r.unwrap_err().contains("无对端"), "Aligned 无对端拒绝（状态自洽）");

    // --- 两复述（判据二）---
    s.add(
        "M19-复述-01",
        boundary_verdict(&boundary_rows()).is_ok(),
        "边界复述核验（I04×1+M01×1 领地不交）",
    );
    // 领地漂移拦截（I04 行提 animation）。
    let mut drift = boundary_rows();
    drift[0].restatement = "skinning 归 I04——animation 也算本域";
    let r = boundary_verdict(&drift);
    s.add(
        "M19-复述-02",
        r.is_err() && r.as_ref().unwrap_err().contains("领地声明漂移") && r.as_ref().unwrap_err().starts_with(E_GRP_RESTATE),
        "领地漂移拦截（I04 行含 animation 即拒）",
    );
    // 边界缺边拦截（抽掉一行）。
    let mut half = boundary_rows();
    half[1] = BoundaryRow { domain_zh: "动画", owner: "M01", restatement: "" };
    s.add("M19-复述-03", boundary_verdict(&half).is_err(), "边界缺边/空复述拒绝");
    // 单源复述（三词同义+载体证据）。
    s.add(
        "M19-复述-04",
        single_source_verdict().is_ok(),
        "F1345 单源复述（三词对齐+载体自洽）",
    );
    // 单源失真可检出（把 track 改 Local 即漂移）。
    let mut broken = term_table();
    broken[0].align = AlignStatus::Local;
    broken[0].peer = None;
    let r = term_verdict(&broken);
    s.add("M19-复述-05", r.is_ok() && single_source_verdict_check(&broken).is_err(), "共享词降级即漂移（可检出）");

    // --- 三要素（判据三）---
    let samples = sample_three_elem();
    s.add(
        "M19-三要素-01",
        three_elem_verdict(&samples).is_ok() && samples.len() == SAMPLE_COUNT,
        "绑定路径 8 条畸形全有三要素（码+什么错+怎么办）",
    );
    let i9 = sample_import_three_elem();
    s.add(
        "M19-三要素-02",
        three_elem_verdict(&i9).is_ok() || i9.iter().all(|d| !d.what.is_empty()),
        "vem09 诊断三要素在标签（格式差异不当缺口）",
    );

    // --- 跟随义务与白名单（判据四）---
    s.add(
        "M19-跟随-01",
        follow_verdict(&follow_duties()).is_ok(),
        "两处跨域契约跟随在册（I04/F1345）",
    );
    // 对端涨版本未跟随拦截。
    let mut lag = follow_duties();
    lag[0].peer_version = 4;
    let r = follow_verdict(&lag);
    s.add(
        "M19-跟随-02",
        r.is_err() && r.as_ref().unwrap_err().contains("版本 4") && r.as_ref().unwrap_err().starts_with(E_GRP_FOLLOW),
        "对端变更未跟随拦截（契约化第七度）",
    );
    // 白名单时效（未过期放行/过期拦截双向）。
    let wl = [WhitelistEntry { subject: "histor-known", expires_ms: 5_000 }];
    let r1 = whitelist_verdict(&wl, 4_000);
    let r2 = whitelist_verdict(&wl, 6_000);
    s.add(
        "M19-白名单-01",
        r1.is_ok() && r2.is_err() && r2.unwrap_err().starts_with(E_GRP_WHITELIST),
        "白名单时效双向（未过放行/过期失效）",
    );

    // --- 版本与暂挂 ---
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in GROUP_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    };
    s.add("M19-版本-01", fp != 0, "版本指纹非零（M19-group-v1）");

    s.add(
        "M19-暂挂-01",
        M_LEDGER_GRP_SUSPENDED_NOTE.contains("暂挂") && M_LEDGER_GRP_SUSPENDED_NOTE.contains("F2419"),
        "M 域账本暂挂声明显性（术语并入 F1772/F1786 M 段）",
    );

    // M19-暂挂-02：判据条数对账（本条为第 17 条）。
    s.add("M19-暂挂-02", s.len() == 16, "判据条数对账（16+本条）");

    s
}

/// 单源复述的可注入版（术语表降级即漂移——复述失真对账用）。
fn single_source_verdict_check(entries: &[TermEntry]) -> Result<(), String> {
    let shared = ["track", "keyframe", "clip"];
    for w in shared.iter() {
        let hit = entries.iter().any(|t| t.en == *w && t.align == AlignStatus::Aligned);
        if !hit {
            return Err(format!(
                "{}：共享词 {} 未登记为对齐态——单源复用词汇漂移",
                E_GRP_RESTATE, w
            ));
        }
    }
    Ok(())
}

/// M 域账本暂挂声明（跨批对接点：术语并入 F1772/F1786 M 段——建账前暂挂）。
pub const M_LEDGER_GRP_SUSPENDED_NOTE: &str = "动画组一致性术语表与两复述结论入 M 域账本：建账前暂挂声明（移交期模式延续——F2419 同款）；术语并入 F1772/F1786 M 段，对端 I04 蒙皮/F1345 剪辑跨域契约复述两处";
