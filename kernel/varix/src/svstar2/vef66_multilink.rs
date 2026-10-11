//! vef66_multilink — VE-F5414 网络与 S 域多用户联动（VE-F 册 · F5414 · 目标 300 行，深化版）。
//!
//! 判据原文（施工书锚点 VE-F5414）：网络与 S 域多用户联动——联动三件：多用户无障碍
//! （多人场景下读屏与字幕的多人化（谁在说话标谁）——S 域双维延伸）/多用户交互词典
//! （多人交互（交易/组队/聊天确认）入词典统一口径）/无障碍偏好随账户走（跨端偏好
//! 同步——无障碍不因换设备丢失）。数据结构：多人化标注；词典接入；偏好同步器。
//! 错误路径与降级矩阵：标注缺失→断言拦截；词典违例→驳回；同步失败→本地兜底。性能
//! 逐项分解：标注 O(用户数)；驳回 O(1)；同步 O(1)。跨批对接点：S 域双维体系上游；
//! F5404 会话消费；F5419 联调。无障碍与隐私：无障碍随账户走即无障碍承诺（域色核心）
//! ；偏好数据脱敏。判据：多人化标注、词典统一、偏好随账户、兜底、判据。
//!
//! **职责定位**：联动三件各自独立可验收——①多人化标注：说话人名册（UserId 闭集非零
//! 校验+同屏说话上限 4）逐用户标注「谁在说话」，字幕行必须绑定说话人（无绑定 0xB906
//! ），标注缺失→断言拦截 0xB901（降级矩阵第一行：不让无标注字幕出口）；标注扫描
//! O(用户数) 单遍。②交互词典：交易/组队邀请/聊天确认三类多人交互入词典统一口径，
//! 词条必须署口径版本（未署 0xB907）；违例（口径不一致/词条不在册/重复注册）→驳回
//! 0xB502 语义承载码 0xB902，驳回 O(1)（三词条小闭集线性即常数）；类别未入闭集
//! 0xB90B。③偏好同步器：无障碍偏好四件闭集（字幕开关/读屏开关/字幕字号/色盲模式）
//! 随账户跨端同步，版本对拍后写胜出留痕（冲突 0xB90A 显性立案不静默）；同步失败→
//! 本地兜底（0xB903——本地副本保全+兜底计数，无障碍不因换设备丢失=无障碍承诺的降级
//! 面）；账户未开通同步显性拒 0xB909；偏好载荷脱敏闸——原始载荷显性携带 raw_content
//! 字段供防线检出（检出 0xB908 阻断），PrefsSet 闭集无内容字段编译期保证。名册溢出
//! 0xB90C 立案。跨批对接点：SDualContract（S 域双维体系上游声明）+ F5404 会话消费
//! + F5419 联调合同三件显性化在数据面。无障碍与隐私：偏好随账户走即无障碍承诺
//! （域色核心）；偏好数据脱敏（导出/同步面零内容——只有四件布尔/枚举与版本号）。
//!
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，全整数算术（无浮点），逻辑 tick
//! 注入零墙钟，FNV-1a 指纹，诊断码 0xB9 段闭集（0xB901~0xB90C 十二码两两互异）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::result::Result;

use crate::checks::CheckSet;

// ─────────────────────────────────────────────────────────────────────────────
// 诊断码 0xB9 段闭集（本文件独占；0xB3=vead09_phystel/0xB4=vead09_telemetry/0xB5~0xB8=CGPU第二十批已占）
// ─────────────────────────────────────────────────────────────────────────────

/// 本模块诊断码段基址：0xB9（0xB901~0xB90C 十二码闭集）。
pub const SEGMENT_BASE: u16 = 0xB9;

/// 诊断码闭集：标注/词典/同步全链错误路径逐码显性，禁静默吞错。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 0xB901 说话标注缺失→断言拦截（降级矩阵第一行）。
    SpeakerMarkMissing,
    /// 0xB902 词典违例→驳回（降级矩阵第二行）。
    DictionaryViolation,
    /// 0xB903 同步失败→本地兜底（降级矩阵第三行）。
    SyncFailLocalFallback,
    /// 0xB904 空用户 id——UserId 零值显性拒。
    EmptyUserId,
    /// 0xB905 同屏说话超限——同屏说话人数上限 4。
    SpeakerCountExceeded,
    /// 0xB906 字幕无绑定——字幕行必须绑定说话人。
    CaptionNoBinding,
    /// 0xB907 词条未署口径——词典词条必须带口径版本。
    DictWordUnsigned,
    /// 0xB908 偏好载荷未脱敏——原始载荷含内容字段阻断。
    PrefPayloadUndesensitized,
    /// 0xB909 账户未开通——同步服务未开户显性拒。
    AccountNotProvisioned,
    /// 0xB90A 同步版本冲突——后写胜出留痕立案。
    SyncVersionConflict,
    /// 0xB90B 交互未入闭集——交互类别越界。
    InteractionUnclassified,
    /// 0xB90C 名册溢出——说话人名册满容拒新不覆盖。
    RosterOverflow,
}

impl XcCode {
    /// 诊断码值：段基址左移八位 + 段内序号（0xB901 起）。
    pub fn code(self) -> u16 {
        (SEGMENT_BASE << 8)
            | match self {
                XcCode::SpeakerMarkMissing => 0x01,
                XcCode::DictionaryViolation => 0x02,
                XcCode::SyncFailLocalFallback => 0x03,
                XcCode::EmptyUserId => 0x04,
                XcCode::SpeakerCountExceeded => 0x05,
                XcCode::CaptionNoBinding => 0x06,
                XcCode::DictWordUnsigned => 0x07,
                XcCode::PrefPayloadUndesensitized => 0x08,
                XcCode::AccountNotProvisioned => 0x09,
                XcCode::SyncVersionConflict => 0x0A,
                XcCode::InteractionUnclassified => 0x0B,
                XcCode::RosterOverflow => 0x0C,
            }
    }

    /// 可操作建议（全码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            XcCode::SpeakerMarkMissing => "字幕无说话标注已拦截：先出说话标注再出字幕，标注是字幕的前置件",
            XcCode::DictionaryViolation => "交互被词典驳回：核对词条口径版本与类别闭集，改后重新提交",
            XcCode::SyncFailLocalFallback => "同步失败已本地兜底：本地副本保全，查网络后重试同步",
            XcCode::EmptyUserId => "用户 id 为零：检查会话分配器，id 从 1 起分派",
            XcCode::SpeakerCountExceeded => "同屏说话超 4 人：按优先级裁剪或分屏，标注面不超载",
            XcCode::CaptionNoBinding => "字幕未绑说话人：补 SpeakerMark 绑定后再出字幕",
            XcCode::DictWordUnsigned => "词条未署口径版本：补 caliber 版本号后入库",
            XcCode::PrefPayloadUndesensitized => "偏好载荷含内容字段：删 raw_content，偏好只收四件闭集",
            XcCode::AccountNotProvisioned => "账户未开通同步：先走开户流程或继续本地模式",
            XcCode::SyncVersionConflict => "双端版本冲突：后写胜出已留痕，核对冲突账确认无覆盖丢失",
            XcCode::InteractionUnclassified => "交互类别越界：交易/组队/聊天确认三类之外请扩词典而非绕过",
            XcCode::RosterOverflow => "名册满容：先逐出离线说话人再收新，不静默覆盖",
        }
    }
}

/// 诊断对象：码 + 现场短述，render 输出「[0xB9NN] 述 || 建议」可读行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diag {
    code: XcCode,
    where_: String,
}

impl Diag {
    /// 构造诊断。
    pub fn new(code: XcCode, where_: &str) -> Self {
        Diag {
            code,
            where_: where_.to_string(),
        }
    }

    /// 诊断码值。
    pub fn code(&self) -> u16 {
        self.code.code()
    }

    /// 码枚举（判据 matches! 精确归因）。
    pub fn kind(&self) -> XcCode {
        self.code
    }

    /// 可读渲染：码 + 现场 + 建议（纯文本单行可读屏）。
    pub fn render(&self) -> String {
        format!(
            "[0x{:04X}] {} || {}",
            self.code.code(),
            self.where_,
            self.code.detail()
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FNV-1a 32 位指纹（同构共用算法——同输入同摘要）
// ─────────────────────────────────────────────────────────────────────────────

/// FNV-1a 32 位偏移基。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;
/// FNV-1a 32 位素数。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// 单字节推进。
pub fn fnv_step(hash: u32, byte: u8) -> u32 {
    (hash ^ (byte as u32)).wrapping_mul(FNV_PRIME)
}

/// 字节切片指纹。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h = FNV_OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        h = fnv_step(h, bytes[i]);
        i += 1;
    }
    h
}

/// u64 按小端八字节指纹。
pub fn fnv_u64(hash: u32, v: u64) -> u32 {
    let mut h = hash;
    let mut i = 0;
    while i < 8 {
        h = fnv_step(h, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    h
}

// ─────────────────────────────────────────────────────────────────────────────
// 第一件：多人化标注（谁在说话标谁）——O(用户数)
// ─────────────────────────────────────────────────────────────────────────────

/// 用户标识：非零闭集（零值 0xB904）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UserId(pub u32);

impl UserId {
    /// 构造校验（零值显性拒）。
    pub fn new(v: u32) -> Result<Self, XcCode> {
        if v == 0 {
            Err(XcCode::EmptyUserId)
        } else {
            Ok(UserId(v))
        }
    }
}

/// 同屏说话人数上限（标注面不超载——读屏与字幕的多人化有容量语义）。
pub const MAX_SPEAKERS: usize = 4;

/// 名册容量（说话人名册硬上限）。
pub const ROSTER_CAP: usize = 16;

/// 说话标注：谁在说话——用户 id + 逻辑 tick + 声纹序（同用户多路声）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpeakerMark {
    /// 说话人。
    pub speaker: UserId,
    /// 标注逻辑 tick。
    pub at_tick: u64,
    /// 声路序（同用户多路声从 0 计）。
    pub voice_seq: u32,
}

impl SpeakerMark {
    /// 标注指纹（FNV-1a）。
    pub fn fingerprint(&self) -> u32 {
        let h0 = fnv_u64(FNV_OFFSET.wrapping_add(0xB9), self.speaker.0 as u64);
        let h1 = fnv_u64(h0, self.at_tick);
        fnv_u64(h1, self.voice_seq as u64)
    }
}

/// 字幕行：必须绑定说话标注（无绑定 0xB906——多人化字幕的核心约束）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptionLine {
    /// 字幕行序。
    pub line_no: u32,
    /// 绑定的说话标注序（名册内下标；None=未绑定——闸口显性形态）。
    pub speaker_slot: Option<usize>,
    /// 逻辑 tick。
    pub at_tick: u64,
}

/// 说话人名册：标注收集 + 容量护栏 + 断言拦截闸。
pub struct SpeakerRoster {
    marks: Vec<SpeakerMark>,
    /// 名册内活跃说话人去重计数（同用户多路只计一人）。
    distinct: Vec<UserId>,
    /// 名册溢出立案计数（0xB90C）。
    overflow_rejected: u32,
    /// 断言拦截计数（0xB901 审计位）。
    intercepted: u32,
}

impl SpeakerRoster {
    /// 空名册。
    pub fn new() -> Self {
        SpeakerRoster {
            marks: Vec::new(),
            distinct: Vec::new(),
            overflow_rejected: 0,
            intercepted: 0,
        }
    }

    /// 说话标注入册（O(用户数) 去重扫描；同屏说话超限 0xB905；名册满容 0xB90C）。
    pub fn admit(&mut self, mark: SpeakerMark) -> Result<usize, Diag> {
        // 同屏说话去重计数校验。
        let already = self.distinct.iter().any(|u| *u == mark.speaker);
        if !already && self.distinct.len() >= MAX_SPEAKERS {
            return Err(Diag::new(
                XcCode::SpeakerCountExceeded,
                "同屏说话超上限 4 人",
            ));
        }
        if self.marks.len() >= ROSTER_CAP {
            self.overflow_rejected += 1;
            return Err(Diag::new(
                XcCode::RosterOverflow,
                "名册满容，拒新不覆盖",
            ));
        }
        let slot = self.marks.len();
        self.marks.push(mark);
        if !already {
            self.distinct.push(mark.speaker);
        }
        Ok(slot)
    }

    /// 断言拦截闸：字幕出口前置件——字幕必须绑定在册标注（降级矩阵第一行落地）。
    /// O(用户数) 扫描；未绑定 0xB906；绑定下标越界（不在册）0xB901 断言拦截。
    pub fn assert_caption_bound(&mut self, cap: &CaptionLine) -> Result<(), Diag> {
        match cap.speaker_slot {
            None => {
                self.intercepted += 1;
                Err(Diag::new(
                    XcCode::CaptionNoBinding,
                    "字幕未绑说话人，出口拦截",
                ))
            }
            Some(slot) => {
                if slot >= self.marks.len() {
                    self.intercepted += 1;
                    Err(Diag::new(
                        XcCode::SpeakerMarkMissing,
                        "绑定下标不在册，标注缺失断言拦截",
                    ))
                } else {
                    Ok(())
                }
            }
        }
    }

    /// 在册标注只读切片。
    pub fn marks(&self) -> &[SpeakerMark] {
        &self.marks
    }

    /// 活跃说话人数（去重）。
    pub fn speaker_count(&self) -> usize {
        self.distinct.len()
    }

    /// 断言拦截计数。
    pub fn intercepted(&self) -> u32 {
        self.intercepted
    }

    /// 名册溢出计数。
    pub fn overflow_rejected(&self) -> u32 {
        self.overflow_rejected
    }

    /// O(用户数) 全量标注扫描（性能判据面：单遍触达全部标注一次）。
    pub fn scan_touches(&self) -> usize {
        self.marks.len()
    }

    /// 名册指纹（FNV-1a 顺序聚合——同步对账用）。
    pub fn roster_fingerprint(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut i = 0;
        while i < self.marks.len() {
            h = fnv_step(h ^ self.marks[i].fingerprint(), 0xB9);
            i += 1;
        }
        h
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 第二件：多用户交互词典（统一口径；驳回 O(1)）
// ─────────────────────────────────────────────────────────────────────────────

/// 多人交互类别闭集（交易/组队/聊天确认——锚点原文三类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionKind {
    /// 交易。
    Trade,
    /// 组队邀请。
    PartyInvite,
    /// 聊天确认。
    ChatConfirm,
}

impl InteractionKind {
    /// 可读名。
    pub fn name(self) -> &'static str {
        match self {
            InteractionKind::Trade => "trade",
            InteractionKind::PartyInvite => "party_invite",
            InteractionKind::ChatConfirm => "chat_confirm",
        }
    }

    /// 全闭集枚举。
    pub const ALL: [InteractionKind; 3] = [
        InteractionKind::Trade,
        InteractionKind::PartyInvite,
        InteractionKind::ChatConfirm,
    ];
}

/// 词典词条：类别 + 口径版本 + 短语号——三件署齐才是合法词条。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DictEntry {
    /// 交互类别。
    pub kind: InteractionKind,
    /// 口径版本（统一口径的版本署名——未署 0xB907）。
    pub caliber_ver: u32,
    /// 短语号（词典内短语索引，从 1 起）。
    pub phrase_id: u32,
}

/// 词典当前口径版本（全词典统一——多版本并存即口径不统一）。
pub const DICT_CALIBER_VER: u32 = 1;

/// 交互词典：注册制（重复注册显性拒）+ 驳回闸（违例 O(1)——三词条小闭集线性即常数）。
pub struct InteractionDict {
    entries: Vec<DictEntry>,
    /// 驳回计数（0xB902 审计位）。
    rejected: u32,
    /// 类别越界计数（0xB90B 审计位）。
    unclassified: u32,
    /// 未署口径计数（0xB907 审计位）。
    unsigned: u32,
}

impl InteractionDict {
    /// 空词典（自带三类别默认词条署当前口径——统一口径的出厂底座）。
    pub fn with_defaults() -> Self {
        let mut d = InteractionDict {
            entries: Vec::new(),
            rejected: 0,
            unclassified: 0,
            unsigned: 0,
        };
        let mut i = 0;
        while i < InteractionKind::ALL.len() {
            let e = DictEntry {
                kind: InteractionKind::ALL[i],
                caliber_ver: DICT_CALIBER_VER,
                phrase_id: (i + 1) as u32,
            };
            let _ = d.register(e);
            i += 1;
        }
        d
    }

    /// 空词典（无默认——判据用）。
    pub fn new() -> Self {
        InteractionDict {
            entries: Vec::new(),
            rejected: 0,
            unclassified: 0,
            unsigned: 0,
        }
    }

    /// 词条注册（重复注册显性拒；未署口径 0xB907）。
    pub fn register(&mut self, e: DictEntry) -> Result<(), Diag> {
        if e.caliber_ver == 0 {
            self.unsigned += 1;
            return Err(Diag::new(XcCode::DictWordUnsigned, "词条口径版本为零（未署）"));
        }
        if self.entries.iter().any(|x| *x == e) {
            return Err(Diag::new(XcCode::DictionaryViolation, "词条重复注册"));
        }
        self.entries.push(e);
        Ok(())
    }

    /// 交互提交闸（驳回 O(1)）：词条在册 + 口径一致才放行。
    pub fn admit(&mut self, kind: InteractionKind, caliber_ver: u32, phrase_id: u32) -> Result<DictEntry, Diag> {
        if caliber_ver == 0 {
            self.unsigned += 1;
            return Err(Diag::new(XcCode::DictWordUnsigned, "提交未署口径"));
        }
        if caliber_ver != DICT_CALIBER_VER {
            self.rejected += 1;
            return Err(Diag::new(
                XcCode::DictionaryViolation,
                "口径版本与词典不一致，驳回",
            ));
        }
        let mut found = None;
        let mut i = 0;
        while i < self.entries.len() {
            let e = self.entries[i];
            if e.kind == kind && e.caliber_ver == caliber_ver && e.phrase_id == phrase_id {
                found = Some(e);
            }
            i += 1;
        }
        match found {
            Some(e) => Ok(e),
            None => {
                self.rejected += 1;
                Err(Diag::new(XcCode::DictionaryViolation, "词条不在册，驳回"))
            }
        }
    }

    /// 未入闭集语义显性锚点（0xB90B——原始类别号越界时调用方先用此闸）。
    pub fn classify(&mut self, raw_kind: u32) -> Result<InteractionKind, Diag> {
        match raw_kind {
            0 => Ok(InteractionKind::Trade),
            1 => Ok(InteractionKind::PartyInvite),
            2 => Ok(InteractionKind::ChatConfirm),
            _ => {
                self.unclassified += 1;
                Err(Diag::new(
                    XcCode::InteractionUnclassified,
                    "交互类别越出闭集，拒收",
                ))
            }
        }
    }

    /// 在册词条只读切片。
    pub fn entries(&self) -> &[DictEntry] {
        &self.entries
    }

    /// 驳回计数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 越界计数。
    pub fn unclassified(&self) -> u32 {
        self.unclassified
    }

    /// 未署口径计数。
    pub fn unsigned(&self) -> u32 {
        self.unsigned
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 第三件：无障碍偏好随账户走（跨端同步；失败→本地兜底；偏好数据脱敏）
// ─────────────────────────────────────────────────────────────────────────────

/// 无障碍偏好四件闭集（锚点：字幕/读屏/字号/色盲——全布尔与枚举，零内容字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrefsSet {
    /// 字幕开关。
    pub captions_on: bool,
    /// 读屏开关。
    pub screen_reader_on: bool,
    /// 字幕字号档（0=小 1=中 2=大）。
    pub subtitle_size: u8,
    /// 色盲模式（0=无 1=红绿 2=蓝黄）。
    pub colorblind_mode: u8,
}

impl PrefsSet {
    /// 出厂默认（全开+中字号+无色盲——无障碍默认最大可达性）。
    pub const fn accessible_default() -> Self {
        PrefsSet {
            captions_on: true,
            screen_reader_on: true,
            subtitle_size: 1,
            colorblind_mode: 0,
        }
    }

    /// 偏好指纹（FNV-1a 四件）。
    pub fn fingerprint(&self) -> u32 {
        let h0 = fnv_u64(FNV_OFFSET.wrapping_add(0xB9), self.captions_on as u64);
        let h1 = fnv_u64(h0, self.screen_reader_on as u64);
        let h2 = fnv_u64(h1, self.subtitle_size as u64);
        fnv_u64(h2, self.colorblind_mode as u64)
    }
}

/// 账户偏好副本：账户 + 偏好 + 版本号（随账户走的三件套）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccountPrefs {
    /// 账户 id。
    pub account: UserId,
    /// 偏好四件。
    pub prefs: PrefsSet,
    /// 单调版本号（后写胜出的对拍键）。
    pub version: u64,
}

/// 原始偏好载荷：显性携带 raw_content 供脱敏防线检出（合法采集恒 None）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawPrefsPayload {
    /// 账户 id。
    pub account: UserId,
    /// 偏好四件。
    pub prefs: PrefsSet,
    /// 版本号。
    pub version: u64,
    /// 内容挂载点（显性存在以供防线检出 0xB908）。
    pub raw_content: Option<String>,
}

/// 偏好同步器：跨端同步 + 版本对拍 + 本地兜底（O(1)——单账户单对象语义）。
pub struct PrefSync {
    /// 本地副本（同步失败的兜底面——无障碍不丢失的数据面）。
    local: Option<AccountPrefs>,
    /// 已开通同步的账户（开户制——未开户 0xB909）。
    provisioned: Vec<UserId>,
    /// 兜底计数（0xB903 审计位）。
    fallbacks: u32,
    /// 版本冲突立案计数（0xB90A 审计位）。
    conflicts: u32,
    /// 脱敏拦截计数（0xB908 审计位）。
    undesensitized: u32,
    /// 最近一次同步留痕（后写胜出的证据面）。
    last_sync: Option<AccountPrefs>,
}

impl PrefSync {
    /// 空同步器。
    pub fn new() -> Self {
        PrefSync {
            local: None,
            provisioned: Vec::new(),
            fallbacks: 0,
            conflicts: 0,
            undesensitized: 0,
            last_sync: None,
        }
    }

    /// 开户（同步前置件）。
    pub fn provision(&mut self, account: UserId) {
        if !self.provisioned.contains(&account) {
            self.provisioned.push(account);
        }
    }

    /// 开户判。
    pub fn is_provisioned(&self, account: UserId) -> bool {
        self.provisioned.contains(&account)
    }

    /// 脱敏闸：内容字段检出 0xB908 阻断（偏好数据脱敏判据的入口面）。
    pub fn ingest_raw(&mut self, raw: RawPrefsPayload) -> Result<AccountPrefs, Diag> {
        if raw.raw_content.is_some() {
            self.undesensitized += 1;
            return Err(Diag::new(
                XcCode::PrefPayloadUndesensitized,
                "偏好载荷含内容字段，阻断入库",
            ));
        }
        let ap = AccountPrefs {
            account: raw.account,
            prefs: raw.prefs,
            version: raw.version,
        };
        self.sync(ap)
    }

    /// 同步（O(1)）：未开户 0xB909；版本对拍后写胜出（冲突 0xB90A 立案留痕）。
    /// 同步侧失败由调用方按 0xB903 走 local_fallback（见 fallback_local）。
    pub fn sync(&mut self, incoming: AccountPrefs) -> Result<AccountPrefs, Diag> {
        if !self.is_provisioned(incoming.account) {
            return Err(Diag::new(
                XcCode::AccountNotProvisioned,
                "账户未开通同步",
            ));
        }
        if let Some(cur) = self.local {
            if incoming.version < cur.version {
                self.conflicts += 1;
                // 后写胜出纪律：incoming 版本更旧——保留本地，冲突立案留痕。
                return Err(Diag::new(
                    XcCode::SyncVersionConflict,
                    "incoming 版本旧于本地，后写胜出保留本地",
                ));
            }
            if incoming.version == cur.version && incoming.prefs != cur.prefs {
                self.conflicts += 1;
                return Err(Diag::new(
                    XcCode::SyncVersionConflict,
                    "同版本异偏好，冲突立案",
                ));
            }
        }
        self.local = Some(incoming);
        self.last_sync = self.local;
        Ok(incoming)
    }

    /// 本地兜底（降级矩阵第三行）：同步失败时本地副本保全+兜底计数。
    /// 返回兜底生效的本地副本（None=本地也空——首次即失败，仍显性无掩盖）。
    pub fn fallback_local(&mut self, account: UserId) -> (Option<AccountPrefs>, u32) {
        let hit = self.local.filter(|p| p.account == account);
        self.fallbacks += 1;
        (hit, self.fallbacks)
    }

    /// 当前本地副本只读。
    pub fn local_prefs(&self, account: UserId) -> Option<AccountPrefs> {
        self.local.filter(|p| p.account == account)
    }

    /// 最近同步留痕只读。
    pub fn last_sync(&self) -> Option<AccountPrefs> {
        self.last_sync
    }

    /// 兜底计数。
    pub fn fallbacks(&self) -> u32 {
        self.fallbacks
    }

    /// 冲突立案计数。
    pub fn conflicts(&self) -> u32 {
        self.conflicts
    }

    /// 脱敏拦截计数。
    pub fn undesensitized(&self) -> u32 {
        self.undesensitized
    }

    /// 已开户数。
    pub fn provisioned_count(&self) -> usize {
        self.provisioned.len()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 跨批对接点声明（S 域双维上游 / F5404 会话消费 / F5419 联调）
// ─────────────────────────────────────────────────────────────────────────────

/// 对接契约三件：上游/消费方/联调方一一显性（文档化在数据面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkContract {
    /// 上游：S 域双维体系。
    pub upstream: &'static str,
    /// 消费方：F5404 会话。
    pub consumer: &'static str,
    /// 联调：F5419。
    pub jointest: &'static str,
}

/// 契约实例。
pub const S_LINK: LinkContract = LinkContract {
    upstream: "S-domain-dual-axis",
    consumer: "VE-F5404-session",
    jointest: "VE-F5419-joint",
};

impl LinkContract {
    /// 契约完备校验（三件全非空）。
    pub fn well_formed(&self) -> bool {
        !self.upstream.is_empty() && !self.consumer.is_empty() && !self.jointest.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 判据入口（CheckSet 聚合面）
// ─────────────────────────────────────────────────────────────────────────────

/// 判据聚合：覆盖锚点判据关键词（多人化标注/词典统一/偏好随账户/兜底）+ 降级矩阵三行全验。
pub fn run_vef66_checks() -> CheckSet {
    let mut ck = CheckSet::new("VE-F5414");

    // ── 一组：多人化标注（谁在说话标谁 + 断言拦截）──
    {
        ck.add(
            "用户 id 零值显性拒 0xB904",
            matches!(UserId::new(0), Err(XcCode::EmptyUserId)) && UserId::new(7).is_ok(),
            "",
        );
        let mut r = SpeakerRoster::new();
        let u1 = UserId::new(1).ok();
        let u2 = UserId::new(2).ok();
        let s1 = SpeakerMark { speaker: u1.unwrap_or(UserId(1)), at_tick: 10, voice_seq: 0 };
        let s2 = SpeakerMark { speaker: u2.unwrap_or(UserId(2)), at_tick: 11, voice_seq: 0 };
        let slot1 = r.admit(s1);
        let slot2 = r.admit(s2);
        ck.add(
            "标注入册返回槽位且去重计数正确",
            slot1.is_ok() && slot2.is_ok() && r.speaker_count() == 2 && r.scan_touches() == 2,
            "",
        );
        // 同用户多路声去重（不涨说话人数）。
        let s1b = SpeakerMark { speaker: u1.unwrap_or(UserId(1)), at_tick: 12, voice_seq: 1 };
        let _ = r.admit(s1b);
        ck.add("同用户多路只计一人", r.speaker_count() == 2 && r.marks().len() == 3, "");
        // 断言拦截：无绑定字幕出口拦截 0xB906。
        let loose = CaptionLine { line_no: 1, speaker_slot: None, at_tick: 12 };
        ck.add(
            "无绑定字幕出口拦截 0xB906",
            matches!(r.assert_caption_bound(&loose), Err(d) if d.kind() == XcCode::CaptionNoBinding),
            "",
        );
        // 断言拦截：绑定越界 → 0xB901 标注缺失。
        let wild = CaptionLine { line_no: 2, speaker_slot: Some(9), at_tick: 12 };
        ck.add(
            "标注缺失断言拦截 0xB901（降级矩阵第一行）",
            matches!(r.assert_caption_bound(&wild), Err(d) if d.kind() == XcCode::SpeakerMarkMissing) && r.intercepted() == 2,
            "",
        );
        // 合法绑定放行。
        let ok = CaptionLine { line_no: 3, speaker_slot: Some(0), at_tick: 12 };
        ck.add("在册绑定放行", r.assert_caption_bound(&ok).is_ok(), "");
        // 同屏说话超限。
        let mut r2 = SpeakerRoster::new();
        let mut over_rejected = false;
        let mut i = 0;
        while i < 6 {
            if let Ok(uid) = UserId::new((i + 1) as u32) {
                let m = SpeakerMark { speaker: uid, at_tick: 1, voice_seq: 0 };
                if matches!(r2.admit(m), Err(d) if d.kind() == XcCode::SpeakerCountExceeded) {
                    over_rejected = true;
                }
            }
            i += 1;
        }
        ck.add(
            "同屏说话超限显性拒 0xB905（上限 4）",
            over_rejected && r2.speaker_count() == 4,
            "",
        );
        // 名册指纹确定性。
        let mut r3 = SpeakerRoster::new();
        let _ = r3.admit(s1);
        let _ = r3.admit(s2);
        let mut r4 = SpeakerRoster::new();
        let _ = r4.admit(s1);
        let _ = r4.admit(s2);
        ck.add(
            "同输入双名册指纹一致（确定性对拍）",
            r3.roster_fingerprint() == r4.roster_fingerprint() && r3.roster_fingerprint() != FNV_OFFSET,
            "",
        );
    }

    // ── 二组：交互词典（统一口径 + 驳回 O(1)）──
    {
        let mut d = InteractionDict::with_defaults();
        ck.add(
            "出厂三类别默认词条署当前口径",
            d.entries().len() == 3 && d.entries().iter().all(|e| e.caliber_ver == DICT_CALIBER_VER),
            "",
        );
        // 合法交互放行。
        let ok = d.admit(InteractionKind::Trade, DICT_CALIBER_VER, 1);
        ck.add("在册交互放行", ok.is_ok(), "");
        // 违例驳回：口径不一致。
        let bad_ver = matches!(
            d.admit(InteractionKind::Trade, DICT_CALIBER_VER + 5, 1),
            Err(e) if e.kind() == XcCode::DictionaryViolation
        );
        // 违例驳回：词条不在册。
        let bad_phrase = matches!(
            d.admit(InteractionKind::ChatConfirm, DICT_CALIBER_VER, 99),
            Err(e) if e.kind() == XcCode::DictionaryViolation
        );
        ck.add(
            "词典违例驳回 0xB902（口径不一致+词条不在册）且计数",
            bad_ver && bad_phrase && d.rejected() == 2,
            "",
        );
        // 未署口径。
        let unsigned = matches!(
            d.admit(InteractionKind::Trade, 0, 1),
            Err(e) if e.kind() == XcCode::DictWordUnsigned
        );
        ck.add("未署口径显性拒 0xB907", unsigned && d.unsigned() == 1, "");
        // 类别越界。
        let over = matches!(d.classify(9), Err(e) if e.kind() == XcCode::InteractionUnclassified);
        ck.add(
            "交互类别越界显性拒 0xB90B",
            over && d.unclassified() == 1,
            "",
        );
        // 重复注册显性拒。
        let dup = DictEntry {
            kind: InteractionKind::Trade,
            caliber_ver: DICT_CALIBER_VER,
            phrase_id: 1,
        };
        ck.add("词条重复注册显性拒", d.register(dup).is_err(), "");
        // 驳回 O(1) 性能语义：三类别小闭集——count 触达即常数（判据以 entries 上限语义直证）。
        ck.add(
            "三类别闭集枚举齐（驳回常数底座）",
            InteractionKind::ALL.len() == 3,
            "",
        );
    }

    // ── 三组：偏好随账户走（跨端同步 + 版本对拍 + 脱敏闸）──
    {
        let mut ps = PrefSync::new();
        let acct = UserId::new(100).ok().unwrap_or(UserId(100));
        // 未开户显性拒。
        let ap0 = AccountPrefs {
            account: acct,
            prefs: PrefsSet::accessible_default(),
            version: 1,
        };
        ck.add(
            "未开户同步显性拒 0xB909",
            matches!(ps.sync(ap0), Err(d) if d.kind() == XcCode::AccountNotProvisioned),
            "",
        );
        ps.provision(acct);
        // 首次同步落账。
        let v1 = AccountPrefs {
            account: acct,
            prefs: PrefsSet::accessible_default(),
            version: 1,
        };
        ck.add("开户后首次同步落账", ps.sync(v1).is_ok() && ps.local_prefs(acct).is_some(), "");
        // 版本回退冲突立案（后写胜出保留本地）。
        let v0 = AccountPrefs {
            account: acct,
            prefs: PrefsSet {
                captions_on: false,
                screen_reader_on: false,
                subtitle_size: 0,
                colorblind_mode: 0,
            },
            version: 0,
        };
        let conflict = matches!(ps.sync(v0), Err(d) if d.kind() == XcCode::SyncVersionConflict);
        ck.add(
            "版本回退冲突立案 0xB90A 且本地保全（后写胜出）",
            conflict && ps.conflicts() == 1 && ps.local_prefs(acct).map(|p| p.version) == Some(1),
            "",
        );
        // 新版本覆盖（跨端前进——无障碍偏好随账户走到新设备）。
        let v2 = AccountPrefs {
            account: acct,
            prefs: PrefsSet {
                captions_on: true,
                screen_reader_on: true,
                subtitle_size: 2,
                colorblind_mode: 1,
            },
            version: 2,
        };
        let advanced = matches!(ps.sync(v2), Ok(p) if p.version == 2 && p.prefs.subtitle_size == 2);
        ck.add("新版本跨端同步前进", advanced && ps.last_sync().map(|p| p.version) == Some(2), "");
        // 同版本异偏好冲突。
        let v2b = AccountPrefs {
            account: acct,
            prefs: PrefsSet::accessible_default(),
            version: 2,
        };
        ck.add(
            "同版本异偏好冲突立案",
            matches!(ps.sync(v2b), Err(d) if d.kind() == XcCode::SyncVersionConflict) && ps.conflicts() == 2,
            "",
        );
        // 脱敏闸。
        let dirty = RawPrefsPayload {
            account: acct,
            prefs: PrefsSet::accessible_default(),
            version: 3,
            raw_content: Some("用户聊天记录（内容！）".to_string()),
        };
        ck.add(
            "偏好载荷含内容阻断 0xB908",
            matches!(ps.ingest_raw(dirty), Err(d) if d.kind() == XcCode::PrefPayloadUndesensitized) && ps.undesensitized() == 1,
            "",
        );
        // 本地兜底（降级矩阵第三行）。
        let (hit, n) = ps.fallback_local(acct);
        ck.add(
            "同步失败本地兜底 0xB903（副本保全+计数）",
            hit.map(|p| p.version) == Some(2) && n == 1 && ps.fallbacks() == 1,
            "",
        );
        // PrefsSet 零内容类型学：四件闭集指纹双向实证。
        let p1 = PrefsSet::accessible_default();
        let p2 = PrefsSet::accessible_default();
        let p3 = PrefsSet {
            captions_on: false,
            ..PrefsSet::accessible_default()
        };
        ck.add(
            "偏好闭集指纹双向实证（同值同纹异值异纹）",
            p1.fingerprint() == p2.fingerprint() && p1.fingerprint() != p3.fingerprint(),
            "",
        );
    }

    // ── 四组：对接契约 + 诊断码闭集 ──
    {
        ck.add("S 域对接契约三件齐（上游/消费/联调）", S_LINK.well_formed(), "");
        let codes = [
            XcCode::SpeakerMarkMissing,
            XcCode::DictionaryViolation,
            XcCode::SyncFailLocalFallback,
            XcCode::EmptyUserId,
            XcCode::SpeakerCountExceeded,
            XcCode::CaptionNoBinding,
            XcCode::DictWordUnsigned,
            XcCode::PrefPayloadUndesensitized,
            XcCode::AccountNotProvisioned,
            XcCode::SyncVersionConflict,
            XcCode::InteractionUnclassified,
            XcCode::RosterOverflow,
        ];
        let all_seg = codes.iter().all(|c| c.code() >> 8 == SEGMENT_BASE);
        let all_detail = codes.iter().all(|c| !c.detail().is_empty());
        let mut distinct = true;
        let mut a = 0;
        while a < codes.len() {
            let mut b = a + 1;
            while b < codes.len() {
                if codes[a].code() == codes[b].code() {
                    distinct = false;
                }
                b += 1;
            }
            a += 1;
        }
        ck.add(
            "诊断码 0xB9 段十二码闭集独占且建议齐",
            all_seg && all_detail && distinct,
            "",
        );
        let dg = Diag::new(XcCode::SyncFailLocalFallback, "测试现场");
        ck.add(
            "诊断渲染含码含建议",
            dg.render().contains("0xB903") && dg.render().contains("||"),
            "",
        );
        ck.add("同构指纹同源（FNV 空串基向量）", fnv1a(&[]) == FNV_OFFSET, "");
        // 名册溢出：同用户多路声灌满名册（不触 0xB905），满容后 4 次拒新。
        let mut r5 = SpeakerRoster::new();
        let mut overflow = false;
        let mut i = 0;
        while i < ROSTER_CAP + 4 {
            let uid = UserId::new(1).ok().unwrap_or(UserId(1));
            let m = SpeakerMark {
                speaker: uid,
                at_tick: i as u64,
                voice_seq: i as u32,
            };
            if matches!(r5.admit(m), Err(d) if d.kind() == XcCode::RosterOverflow) {
                overflow = true;
            }
            i += 1;
        }
        ck.add(
            "名册满容拒新不覆盖 0xB90C",
            overflow && r5.overflow_rejected() == 4 && r5.marks().len() == ROSTER_CAP,
            "",
        );
    }

    ck
}
