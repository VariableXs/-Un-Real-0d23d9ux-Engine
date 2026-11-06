//! UNX-F49007 多用户配置隔离预告 + UNX-F49008 quirk 表版本化 · 0x71980
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，VX6 批 B11-D 首槽；任务板 J1 引用按原文保留，
//! 不把 F49007 卡面引用的 F49008-J1 改写成 F49007-J1，也不把 F49008 卡面
//! 引用的 F49009-J1 改写成 F49008-J1，以便任务板可逐字回查）：
//!   UNX-F49007 多用户配置隔离预告（与 L2 域联签预告登记）:
//!           ① 纯功能代码 ≥320 行；
//!           ② 判据原文：UNX-F49008-J1 校验和 ×10 防篡改检出 10/10 次；判据锚定 AI-85 向量判据审计口径；
//!           ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49008 quirk 表版本化（表内容校验和——被改 = 判据即红）:
//!           ① 纯功能代码 ≥300 行；
//!           ② 判据原文：UNX-F49009-J1 fuzz ×10^4 零崩；判据锚定 体验十 健壮性；
//!           ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（隔离预告 + 表版本化）：
//!   命名空间自审 → 请求者身份 → 归属判定 → 独立写复核 → 预告登记成文 →
//!   预演复核 → 表行自审 → 校验和封印 → 版本裁决 → 账本渲染。
//! 命名空间先于身份：四份用户配置必须在 (uid 唯一、增益非零、阈值非零) 上
//! 自洽，否则「请求者 1002 读 1002」可能命中两条记录。身份尚未参与，命名
//! 空间先坏，后面所有拒绝都只是碰巧拒绝。
//! 请求者身份先于归属判定：隔离的第一判据是「谁在问」，第二判据才是「问
//! 的是谁」。顺序反过来，实现会先取出目标配置再比对 uid——取出的那一刻数据
//! 已经在返回值里，拒绝变成事后撤回，撤回失败即泄漏。
//! 归属判定先于独立写复核：只有「自己读自己」这一条通路成立，独立写才有
//! 意义。若归属判定允许越权读，写隔离只是在保护一份已经能被读到的副本。
//! 独立写复核先于预告登记成文：预演要拿真实数据证明「改 A 不动 B」。先写
//! 预告文本再预演，预告词可以写得比实现更漂亮——这正是预告必须被预演钉住
//! 的原因。
//! 预告登记成文先于预演复核：没有登记文本，预演没有对照物，10/10 只是十个
//! 布尔值。登记文本（契约路径、主签、联签、Planned 状态、未接线）先钉死，
//! 预演才能被判为「按预告跑的那十次」。
//! 预演复核先于表行自审：两组判据共用一本账，先跑预演把「隔离侧」的绿拿到，
//! 再去判表；反序会让表的红污染预演的计数，10/10 的分母变得不可解释。
//! 表行自审先于校验和封印：校验和证明的是「这一行序列没被改」，不是「这一
//! 行序列是对的」。表行越界（厂商 0、参数 0、flags 高位）先拒，校验和才不
//! 会把一条坏行封印成合法版本。
//! 校验和封印先于版本裁决：版本号是人对表的承诺，校验和是表对内容的承诺。
//! 先比版本号后比校验和，「同版本但内容被改」会被判成「无需升级」而放行——
//! 这恰是 AI-85 审计口径里最危险的一格。
//! 版本裁决先于渲染：渲染行是账本对外的投影；先渲染后裁决，屏幕上会出现
//! 一行「版本 3 校验通过」而实际表已被改，纠错只能靠人眼。
//! 段位守卫最后：14 个诊断码必须落在 [0x71981, 0x7198E]。窗口漂移会把前九项
//! 闸门的失败记成「未知诊断码」，红变灰。
//! 关键差异：本模块不做 quirk 匹配与降级记账（B11-B 的 F49003/F49004 承担），
//! 不做触控板三档配置与热生效（B11-C 的 F49005/F49006 承担），不做 quirk 表
//! fuzz 与损坏配置回退（B11-E 的 F49009/F49010 承担）。F49009-J1「fuzz ×10^4
//! 零崩」的随机注入载体在 B11-E，本模块只逐字保留该引用，不自造 fuzz。
//! 「多用户隔离」在本模块只是**预告**：登记文本 + 十次 fake 预演 + 显式的
//! 「未接线」位。任何调用 `enable_live_isolation()` 都会得到 PreviewBad——
//! 预告不是实现，本模块不替 L2 域承诺真实隔离已上线。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71980，VX6 批 B11-D 首槽）。
pub const SEG_BASE: u32 = 0x71980;

/// 用户命名空间数。
pub const USER_COUNT: usize = 4;
/// 配置表行数。
pub const ROW_COUNT: usize = 12;
/// 表格式版本号。
pub const TABLE_VERSION: u16 = 3;
/// 表版本序号（同版本号下的修订序）。
pub const TABLE_REVISION: u16 = 0;
/// 预告条数。
pub const PREVIEW_COUNT: usize = 3;
/// 预告预演轮数（判据 10/10 的分母）。
pub const PREVIEW_FAKE_ROUNDS: u32 = 10;
/// 防篡改注入路数。
pub const TAMPER_ROUTES: usize = 10;
/// 跨读账轮数。
pub const READ_ROUNDS: u32 = 100;
/// 隔离独立写轮数。
pub const INDEPENDENCE_ROUNDS: u32 = 100;
/// 版本裁决轮数。
pub const VERSION_ROUNDS: u32 = 100;
/// 轮数上界闸门。
pub const SCENARIO_MAX: u32 = 400;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// 账本字段数。
pub const STATS_FIELDS: usize = 8;
/// 注入轮数上限。
pub const FUZZ_ROUNDS: u32 = 10_000;

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IsoCode {
    /// 用户命名空间表非法
    NamespaceBad,
    /// 越权读被拒绝（正常路径的记账码，不是缺陷）
    AccessBad,
    /// 越权读泄漏了数据
    LeakBad,
    /// 预告登记文本不完整
    PreviewBad,
    /// 预演与预告漂移
    PreviewDriftBad,
    /// 表行字段越界或行数不符
    RowBad,
    /// 表内容校验和与封印不符
    ChecksumBad,
    /// 版本号非法或版本倒退
    VersionBad,
    /// 账本不守恒
    LedgerBad,
    /// 复跑摘要不一致
    ReplayBad,
    /// 注入未全捕获
    InjectBad,
    /// 渲染不可解析
    RenderBad,
    /// 覆盖轮数越界
    CoverBad,
    /// 段位越窗
    SegmentBad,
}

impl IsoCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            IsoCode::NamespaceBad => "用户命名空间 uid 重复或配置越界",
            IsoCode::AccessBad => "越权读取被拒（隔离生效）",
            IsoCode::LeakBad => "越权读取返回了数据，隔离失效",
            IsoCode::PreviewBad => "预告登记缺契约/联签/Planned 位或已偷偷接线",
            IsoCode::PreviewDriftBad => "fake 预演结果与预告文本不一致",
            IsoCode::RowBad => "表行数为零/越界，或厂商/参数/flags 非法",
            IsoCode::ChecksumBad => "表内容校验和与封印不符（表被改）",
            IsoCode::VersionBad => "版本号非法、为零或版本倒退",
            IsoCode::LedgerBad => "账本不守恒（读/写/拒计数断裂）",
            IsoCode::ReplayBad => "两次确定性复跑摘要不一致",
            IsoCode::InjectBad => "注入轮次未被全部捕获",
            IsoCode::RenderBad => "渲染行字段数或数值不可解析",
            IsoCode::CoverBad => "覆盖轮数为零或越过上限",
            IsoCode::SegmentBad => "诊断码越出本模块段窗",
        }
    }

    pub const ALL: [IsoCode; 14] = [
        IsoCode::NamespaceBad,
        IsoCode::AccessBad,
        IsoCode::LeakBad,
        IsoCode::PreviewBad,
        IsoCode::PreviewDriftBad,
        IsoCode::RowBad,
        IsoCode::ChecksumBad,
        IsoCode::VersionBad,
        IsoCode::LedgerBad,
        IsoCode::ReplayBad,
        IsoCode::InjectBad,
        IsoCode::RenderBad,
        IsoCode::CoverBad,
        IsoCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < IsoCode::ALL.len() {
        if let Some(c) = IsoCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 用户命名空间 ────────────────────────────────────────────────
/// 一份用户配置（隔离单元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Profile {
    pub uid: u32,
    pub label: &'static str,
    pub gain: u16,
    pub invert: bool,
    pub threshold: u16,
}

/// 四个用户命名空间：uid 唯一、增益与阈值非零。
pub const PROFILES: [Profile; USER_COUNT] = [
    Profile {
        uid: 1000,
        label: "owner",
        gain: 100,
        invert: false,
        threshold: 45,
    },
    Profile {
        uid: 1001,
        label: "guest",
        gain: 60,
        invert: true,
        threshold: 30,
    },
    Profile {
        uid: 1002,
        label: "kiosk",
        gain: 140,
        invert: false,
        threshold: 70,
    },
    Profile {
        uid: 1003,
        label: "service",
        gain: 80,
        invert: true,
        threshold: 50,
    },
];

/// 命名空间自审：条数 → uid 唯一 → 增益/阈值非零。
pub fn namespace_integrity(table: &[Profile]) -> Result<u64, u32> {
    if table.len() != USER_COUNT {
        return Err(IsoCode::NamespaceBad.val());
    }
    let mut fp = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < table.len() {
        let p = match table.get(i) {
            Some(v) => *v,
            None => return Err(IsoCode::NamespaceBad.val()),
        };
        if p.uid == 0 || p.label.is_empty() || p.gain == 0 || p.threshold == 0 {
            return Err(IsoCode::NamespaceBad.val());
        }
        let mut j = i + 1;
        while j < table.len() {
            if let Some(q) = table.get(j) {
                if q.uid == p.uid {
                    return Err(IsoCode::NamespaceBad.val());
                }
            }
            j += 1;
        }
        fp = p_core::fold_u32(fp, p.uid);
        fp = p_core::fold_u32(fp, u32::from(p.gain));
        fp = p_core::fold_u32(fp, u32::from(p.invert));
        fp = p_core::fold_u32(fp, u32::from(p.threshold));
        let bs = p.label.as_bytes();
        let mut k = 0usize;
        while k < bs.len() {
            if let Some(b) = bs.get(k) {
                fp = p_core::fold_byte(fp, *b);
            }
            k += 1;
        }
        i += 1;
    }
    Ok(fp)
}

/// 读结果：拒绝时**不带任何数据**（枚举里没有数据分支）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadOutcome {
    /// 自己读自己：返回本命名空间的配置
    Owned(Profile),
    /// 越权或未知身份：无数据
    Denied,
}

/// 配置仓：每个命名空间一格，读写都按 uid 定位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Store {
    pub slots: [Profile; USER_COUNT],
}

impl Store {
    pub const fn new() -> Store {
        Store { slots: PROFILES }
    }

    pub fn find(&self, uid: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(p) = self.slots.get(i) {
                if p.uid == uid {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 读取：先判身份（requester），再判归属（target），命中才取数据。
    pub fn read(&self, requester: u32, target: u32) -> ReadOutcome {
        if requester != target || requester == 0 {
            return ReadOutcome::Denied;
        }
        match self.find(target) {
            Some(i) => match self.slots.get(i) {
                Some(p) => ReadOutcome::Owned(*p),
                None => ReadOutcome::Denied,
            },
            None => ReadOutcome::Denied,
        }
    }

    /// 写：只允许改写请求者自己的那一格。
    pub fn update_gain(&mut self, requester: u32, gain: u16) -> Result<(), u32> {
        if gain == 0 {
            return Err(IsoCode::NamespaceBad.val());
        }
        let i = match self.find(requester) {
            Some(v) => v,
            None => return Err(IsoCode::NamespaceBad.val()),
        };
        match self.slots.get_mut(i) {
            Some(p) => {
                p.gain = gain;
                Ok(())
            }
            None => Err(IsoCode::NamespaceBad.val()),
        }
    }
}

/// 跨读账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReadLedger {
    pub rounds: u32,
    pub own: u32,
    pub denied: u32,
    pub leaks: u32,
    pub unknown: u32,
    /// 清单外身份探测（uid 0xFFFF）被拒的次数：每次读取都伴随一次。
    pub ghost: u32,
    pub fingerprint: u64,
}

impl ReadLedger {
    pub const fn new() -> ReadLedger {
        ReadLedger {
            rounds: 0,
            own: 0,
            denied: 0,
            leaks: 0,
            unknown: 0,
            ghost: 0,
            fingerprint: 0,
        }
    }

    /// 每次读取恰好落进「自己读自己 / 越权被拒 / 未知身份被拒 / 泄漏」之一。
    pub fn conserved(&self) -> bool {
        self.rounds
            == self
                .own
                .saturating_add(self.denied)
                .saturating_add(self.leaks)
                .saturating_add(self.unknown)
            && self.ghost == self.rounds
            && self.leaks == 0
    }
}

/// 跨读轮：随机请求者 × 随机目标，越权必须被拒且零泄漏。
pub fn read_rounds(rounds: u32) -> Result<ReadLedger, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(IsoCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x4900_7A11);
    let store = Store::new();
    let mut lg = ReadLedger::new();
    let mut fp = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < rounds {
        // 取高位比特：LCG 的低位周期很短，直接用 below(4) 会让两次抽样
        // 永远取到不同用户，隔离账里「自己读自己」的分子恒为 0。
        let x = rng.next();
        let ai = ((x >> 8) & 0xFF) as usize % USER_COUNT;
        let bi = ((x >> 32) & 0xFF) as usize % USER_COUNT;
        let a = match store.slots.get(ai) {
            Some(v) => v.uid,
            None => return Err(IsoCode::CoverBad.val()),
        };
        let b = match store.slots.get(bi) {
            Some(v) => v.uid,
            None => return Err(IsoCode::CoverBad.val()),
        };
        match store.read(a, b) {
            ReadOutcome::Owned(p) => {
                if p.uid != a {
                    // 自己读自己却拿到别人的配置：泄漏。
                    lg.leaks = lg.leaks.saturating_add(1);
                } else {
                    lg.own = lg.own.saturating_add(1);
                }
            }
            ReadOutcome::Denied => {
                if a == b {
                    // 该给自己却被拒：命名空间/身份闸门出了问题。
                    lg.unknown = lg.unknown.saturating_add(1);
                } else {
                    lg.denied = lg.denied.saturating_add(1);
                }
            }
        }
        // 未知身份探测：uid 0 与清单外 uid 一律拒。
        let ghost = store.read(0xFFFF, a);
        if ghost != ReadOutcome::Denied {
            lg.leaks = lg.leaks.saturating_add(1);
        } else {
            lg.ghost = lg.ghost.saturating_add(1);
        }
        fp = p_core::fold_u32(fp, a);
        fp = p_core::fold_u32(fp, b);
        lg.rounds = lg.rounds.saturating_add(1);
        r += 1;
    }
    lg.fingerprint = fp;
    if !lg.conserved() {
        return Err(IsoCode::LeakBad.val());
    }
    Ok(lg)
}

/// 跨读复跑：两轮摘要一致。
pub fn read_replay(rounds: u32) -> bool {
    match (read_rounds(rounds), read_rounds(rounds)) {
        (Ok(a), Ok(b)) => a.fingerprint == b.fingerprint && a.fingerprint != 0,
        _ => false,
    }
}

/// 独立写账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IsoLedger {
    pub rounds: u32,
    pub writes: u32,
    pub touched_self: u32,
    pub touched_other: u32,
    pub fingerprint: u64,
}

impl IsoLedger {
    pub const fn new() -> IsoLedger {
        IsoLedger {
            rounds: 0,
            writes: 0,
            touched_self: 0,
            touched_other: 0,
            fingerprint: 0,
        }
    }

    pub fn conserved(&self) -> bool {
        self.rounds == self.writes && self.touched_self == self.writes && self.touched_other == 0
    }
}

/// 独立写轮：改 A 的增益，必须只动 A 那一格。
pub fn independence_rounds(rounds: u32) -> Result<IsoLedger, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(IsoCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x4900_7B31);
    let mut store = Store::new();
    let mut lg = IsoLedger::new();
    let mut fp = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < rounds {
        let idx = rng.below(USER_COUNT as u32) as usize;
        let uid = match store.slots.get(idx) {
            Some(v) => v.uid,
            None => return Err(IsoCode::CoverBad.val()),
        };
        let new_gain = 40u16.saturating_add(rng.below(120) as u16);
        let before = store.slots;
        // 改自己。
        store.update_gain(uid, new_gain)?;
        lg.writes = lg.writes.saturating_add(1);
        match store.slots.get(idx) {
            Some(p) => {
                if p.gain == new_gain {
                    lg.touched_self = lg.touched_self.saturating_add(1);
                }
            }
            None => return Err(IsoCode::NamespaceBad.val()),
        }
        // 其余三格必须逐字段不变。
        let mut j = 0usize;
        while j < USER_COUNT {
            if j != idx {
                let same = match (before.get(j), store.slots.get(j)) {
                    (Some(a), Some(b)) => a == b,
                    _ => false,
                };
                if !same {
                    lg.touched_other = lg.touched_other.saturating_add(1);
                }
            }
            j += 1;
        }
        // 把本轮的写入回滚，保持后续轮次的基线固定（便于复跑）。
        if let Some(slot) = store.slots.get_mut(idx) {
            if let Some(old) = before.get(idx) {
                *slot = *old;
            }
        }
        fp = p_core::fold_u32(fp, uid);
        fp = p_core::fold_u32(fp, u32::from(new_gain));
        lg.rounds = lg.rounds.saturating_add(1);
        r += 1;
    }
    lg.fingerprint = fp;
    if !lg.conserved() {
        return Err(IsoCode::LeakBad.val());
    }
    Ok(lg)
}

// ── 隔离预告（L2 联签，仅登记） ──────────────────────────────────
/// 预告状态：本模块只允许 Planned。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PreviewStatus {
    /// 已登记、未实现
    Planned,
    /// 已联签（仍不接线）
    Signed,
    /// 已上线（本模块禁止出现）
    Live,
}

impl PreviewStatus {
    pub fn name(self) -> &'static str {
        match self {
            PreviewStatus::Planned => "planned",
            PreviewStatus::Signed => "signed",
            PreviewStatus::Live => "live",
        }
    }
}

/// 一条预告登记：契约路径 + 主签 + L2 联签 + 预演计数 + 是否接线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Preview {
    pub id: &'static str,
    pub contract: &'static str,
    pub owner: &'static str,
    pub cosign: &'static str,
    pub status: PreviewStatus,
    pub fake_runs: u32,
    pub fake_pass: u32,
    pub wired: bool,
}

/// 三条与 L2 域联签的预告。
pub const PREVIEWS: [Preview; PREVIEW_COUNT] = [
    Preview {
        id: "cfg-isolation-v1",
        contract: "L2/config-isolation/v1",
        owner: "B11-D",
        cosign: "L2",
        status: PreviewStatus::Planned,
        fake_runs: PREVIEW_FAKE_ROUNDS,
        fake_pass: PREVIEW_FAKE_ROUNDS,
        wired: false,
    },
    Preview {
        id: "cfg-isolation-read-gate",
        contract: "L2/config-isolation/read-gate",
        owner: "B11-D",
        cosign: "L2",
        status: PreviewStatus::Planned,
        fake_runs: PREVIEW_FAKE_ROUNDS,
        fake_pass: PREVIEW_FAKE_ROUNDS,
        wired: false,
    },
    Preview {
        id: "cfg-isolation-write-guard",
        contract: "L2/config-isolation/write-guard",
        owner: "B11-D",
        cosign: "L2",
        status: PreviewStatus::Planned,
        fake_runs: PREVIEW_FAKE_ROUNDS,
        fake_pass: PREVIEW_FAKE_ROUNDS,
        wired: false,
    },
];

/// 预告登记成文判定。
pub fn preview_wellformed(p: &Preview) -> bool {
    !p.id.is_empty()
        && !p.contract.is_empty()
        && p.owner == "B11-D"
        && p.cosign == "L2"
        && p.status == PreviewStatus::Planned
        && !p.wired
        && p.fake_runs == PREVIEW_FAKE_ROUNDS
        && p.fake_pass == p.fake_runs
}

/// 全部预告成文。
pub fn previews_ok() -> bool {
    let mut i = 0usize;
    while i < PREVIEWS.len() {
        match PREVIEWS.get(i) {
            Some(p) => {
                if !preview_wellformed(p) {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 预演账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FakeLedger {
    pub rounds: u32,
    pub own_reads: u32,
    pub cross_denied: u32,
    pub leaks: u32,
    pub drift: u32,
    pub fingerprint: u64,
}

impl FakeLedger {
    pub const fn new() -> FakeLedger {
        FakeLedger {
            rounds: 0,
            own_reads: 0,
            cross_denied: 0,
            leaks: 0,
            drift: 0,
            fingerprint: 0,
        }
    }

    pub fn conserved(&self) -> bool {
        self.own_reads == self.rounds.saturating_mul(USER_COUNT as u32)
            && self
                .cross_denied
                .saturating_add(self.leaks)
                == self.rounds.saturating_mul((USER_COUNT as u32) * (USER_COUNT as u32 - 1))
            && self.leaks == 0
            && self.drift == 0
    }
}

/// fake 预演：每轮 4 次自己读自己 + 12 次越权读（必须全拒）+ 4 次独立写。
pub fn fake_preview(rounds: u32) -> Result<FakeLedger, u32> {
    if rounds == 0 || rounds > SCENARIO_MAX {
        return Err(IsoCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x4900_7C51);
    let mut store = Store::new();
    let mut lg = FakeLedger::new();
    let mut fp = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < rounds {
        // 每轮先把四份配置设成互不相同的随机增益。
        let mut u = 0usize;
        while u < USER_COUNT {
            let uid = match store.slots.get(u) {
                Some(v) => v.uid,
                None => return Err(IsoCode::CoverBad.val()),
            };
            let g = 40u16.saturating_add(rng.below(120) as u16);
            store.update_gain(uid, g)?;
            u += 1;
        }
        // 自己读自己：四份都要成功且 uid 自洽。
        let mut i2 = 0usize;
        while i2 < USER_COUNT {
            let uid = match store.slots.get(i2) {
                Some(v) => v.uid,
                None => return Err(IsoCode::CoverBad.val()),
            };
            match store.read(uid, uid) {
                ReadOutcome::Owned(p) => {
                    if p.uid == uid {
                        lg.own_reads = lg.own_reads.saturating_add(1);
                    } else {
                        lg.drift = lg.drift.saturating_add(1);
                    }
                }
                ReadOutcome::Denied => lg.drift = lg.drift.saturating_add(1),
            }
            i2 += 1;
        }
        // 越权读：12 组 (请求者, 目标) 必须全拒。
        let mut a = 0usize;
        while a < USER_COUNT {
            let ua = match store.slots.get(a) {
                Some(v) => v.uid,
                None => return Err(IsoCode::CoverBad.val()),
            };
            let mut b = 0usize;
            while b < USER_COUNT {
                if a != b {
                    let ub = match store.slots.get(b) {
                        Some(v) => v.uid,
                        None => return Err(IsoCode::CoverBad.val()),
                    };
                    match store.read(ua, ub) {
                        ReadOutcome::Denied => {
                            lg.cross_denied = lg.cross_denied.saturating_add(1)
                        }
                        ReadOutcome::Owned(_) => lg.leaks = lg.leaks.saturating_add(1),
                    }
                }
                b += 1;
            }
            a += 1;
        }
        // 独立写：改一个，另外三个不变。
        let idx = rng.below(USER_COUNT as u32) as usize;
        let before = store.slots;
        if let Some(p) = store.slots.get(idx) {
            let uid = p.uid;
            store.update_gain(uid, 0x00FF)?;
        }
        let mut j = 0usize;
        while j < USER_COUNT {
            if j != idx {
                let same = match (before.get(j), store.slots.get(j)) {
                    (Some(x), Some(y)) => x == y,
                    _ => false,
                };
                if !same {
                    lg.drift = lg.drift.saturating_add(1);
                }
            }
            j += 1;
        }
        fp = p_core::fold_u32(fp, lg.own_reads);
        fp = p_core::fold_u32(fp, lg.cross_denied);
        lg.rounds = lg.rounds.saturating_add(1);
        r += 1;
    }
    lg.fingerprint = fp;
    Ok(lg)
}

/// 接线闸门：本模块只有预告，没有实现——任何「上线」请求一律 PreviewBad。
pub fn enable_live_isolation() -> Result<(), u32> {
    Err(IsoCode::PreviewBad.val())
}

/// 预演复跑：两次摘要一致。
pub fn preview_replay(rounds: u32) -> bool {
    match (fake_preview(rounds), fake_preview(rounds)) {
        (Ok(a), Ok(b)) => a.fingerprint == b.fingerprint && a.fingerprint != 0,
        _ => false,
    }
}

// ── quirk 表版本化（校验和封印） ─────────────────────────────────
/// 一行 quirk 表：机型 / 固件下界 / 参数覆盖 / 标志位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuirkRow {
    pub vendor: u16,
    pub model: u16,
    pub fw_min: u32,
    pub param: u16,
    pub flags: u8,
}

/// 十二行表，按 (厂商, 机型, 固件下界) 升序固化。
pub const QUIRK_ROWS: [QuirkRow; ROW_COUNT] = [
    QuirkRow { vendor: 0x045E, model: 0x0C01, fw_min: 0x0001_0100, param: 0x0012, flags: 0x01 },
    QuirkRow { vendor: 0x0F11, model: 0x0001, fw_min: 0x0001_0000, param: 0x0007, flags: 0x00 },
    QuirkRow { vendor: 0x1025, model: 0x1601, fw_min: 0x0002_0000, param: 0x0015, flags: 0x02 },
    QuirkRow { vendor: 0x1028, model: 0x0BE1, fw_min: 0x0003_0000, param: 0x0009, flags: 0x04 },
    QuirkRow { vendor: 0x1028, model: 0x0BE2, fw_min: 0x0003_0600, param: 0x000B, flags: 0x05 },
    QuirkRow { vendor: 0x103C, model: 0x8B21, fw_min: 0x0004_0100, param: 0x001C, flags: 0x01 },
    QuirkRow { vendor: 0x1043, model: 0x1C42, fw_min: 0x0005_0000, param: 0x000E, flags: 0x08 },
    QuirkRow { vendor: 0x1532, model: 0x028F, fw_min: 0x0006_0000, param: 0x0002, flags: 0x00 },
    QuirkRow { vendor: 0x1558, model: 0x1401, fw_min: 0x0007_0000, param: 0x0011, flags: 0x03 },
    QuirkRow { vendor: 0x17AA, model: 0x22F1, fw_min: 0x0008_0000, param: 0x0020, flags: 0x07 },
    QuirkRow { vendor: 0x17AA, model: 0x22F2, fw_min: 0x0008_0800, param: 0x0021, flags: 0x06 },
    QuirkRow { vendor: 0x19E5, model: 0x3E01, fw_min: 0x0009_0000, param: 0x0004, flags: 0x09 },
];

/// 表行合法区间。
pub const PARAM_MAX: u16 = 0x00FF;
/// flags 合法掩码（高四位的出现即表病）。
pub const FLAGS_MASK: u8 = 0x0F;

// 与 p_core 同参的 FNV-1a，写成 const fn 以便在编译期封印表摘要。
const fn fnv_b(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01b3)
}

const fn fnv_w(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x = fnv_b(x, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    x
}

/// 表内容校验和：行号先入摘要，故「换序」与「换值」同等被检出。
pub const fn table_checksum(rows: &[QuirkRow]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut i = 0usize;
    while i < rows.len() {
        let r = rows[i];
        h = fnv_w(h, i as u32);
        h = fnv_w(h, r.vendor as u32);
        h = fnv_w(h, r.model as u32);
        h = fnv_w(h, r.fw_min);
        h = fnv_w(h, r.param as u32);
        h = fnv_b(h, r.flags);
        i += 1;
    }
    h
}

/// 编译期封印的校验和：任何一行被改，本值与重算值就不再相等。
pub const TABLE_CHECKSUM: u64 = table_checksum(&QUIRK_ROWS);

/// 表行自审：行数 → 厂商/参数/flags → 严格升序。
pub fn row_integrity(rows: &[QuirkRow]) -> Result<(), u32> {
    if rows.len() != ROW_COUNT {
        return Err(IsoCode::RowBad.val());
    }
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(v) => *v,
            None => return Err(IsoCode::RowBad.val()),
        };
        if r.vendor == 0
            || r.fw_min == 0
            || r.param == 0
            || r.param > PARAM_MAX
            || r.flags & !FLAGS_MASK != 0
        {
            return Err(IsoCode::RowBad.val());
        }
        if i > 0 {
            if let Some(p) = rows.get(i - 1) {
                if r.vendor < p.vendor
                    || (r.vendor == p.vendor && r.model < p.model)
                    || (r.vendor == p.vendor && r.model == p.model && r.fw_min <= p.fw_min)
                {
                    return Err(IsoCode::RowBad.val());
                }
            }
        }
        i += 1;
    }
    Ok(())
}

/// 表封印校验：行自审 → 校验和。
pub fn verify_table(rows: &[QuirkRow], expected: u64) -> Result<u64, u32> {
    row_integrity(rows)?;
    if expected == 0 {
        return Err(IsoCode::ChecksumBad.val());
    }
    let got = table_checksum(rows);
    if got != expected {
        return Err(IsoCode::ChecksumBad.val());
    }
    Ok(got)
}

/// 表版本记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TableVersion {
    pub version: u16,
    pub revision: u16,
    pub rows: u16,
    pub checksum: u64,
}

/// 当前封印版本。
pub const CURRENT_VERSION: TableVersion = TableVersion {
    version: TABLE_VERSION,
    revision: TABLE_REVISION,
    rows: ROW_COUNT as u16,
    checksum: TABLE_CHECKSUM,
};

/// 版本封印校验：版本号 → 修订序 → 行数 → 校验和。
pub fn verify_version(ver: &TableVersion) -> Result<(), u32> {
    if ver.version != TABLE_VERSION || ver.revision != TABLE_REVISION {
        return Err(IsoCode::VersionBad.val());
    }
    if ver.rows as usize != ROW_COUNT {
        return Err(IsoCode::RowBad.val());
    }
    if ver.checksum != TABLE_CHECKSUM || ver.checksum == 0 {
        return Err(IsoCode::ChecksumBad.val());
    }
    Ok(())
}

/// 版本裁决：新版本对旧版本的三种结局（升级 / 拒 / 同版本改表）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 接受升级
    Upgrade,
    /// 拒绝（倒退或非法）
    Downgrade,
    /// 同版本但内容变了：判据即红
    SameVersionTampered,
}

pub fn verdict_of(new: &TableVersion, cur: &TableVersion) -> Verdict {
    if new.version == 0 || new.checksum == 0 || new.rows == 0 {
        return Verdict::Downgrade;
    }
    let newer = new.version > cur.version
        || (new.version == cur.version && new.revision > cur.revision);
    let older = new.version < cur.version
        || (new.version == cur.version && new.revision < cur.revision);
    if older {
        return Verdict::Downgrade;
    }
    if !newer && new.checksum != cur.checksum {
        return Verdict::SameVersionTampered;
    }
    if newer {
        return Verdict::Upgrade;
    }
    Verdict::Upgrade
}

/// 裁决 → 闸门结果。
pub fn accept_version(new: &TableVersion, cur: &TableVersion) -> Result<Verdict, u32> {
    match verdict_of(new, cur) {
        Verdict::Upgrade => Ok(Verdict::Upgrade),
        Verdict::Downgrade => Err(IsoCode::VersionBad.val()),
        Verdict::SameVersionTampered => Err(IsoCode::ChecksumBad.val()),
    }
}

/// 版本裁决账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VersionLedger {
    pub rounds: u32,
    pub upgrades: u32,
    pub downgrades: u32,
    pub tampered: u32,
    pub fingerprint: u64,
}

impl VersionLedger {
    pub const fn new() -> VersionLedger {
        VersionLedger {
            rounds: 0,
            upgrades: 0,
            downgrades: 0,
            tampered: 0,
            fingerprint: 0,
        }
    }

    pub fn conserved(&self) -> bool {
        self.rounds
            == self
                .upgrades
                .saturating_add(self.downgrades)
                .saturating_add(self.tampered)
    }
}

/// 版本裁决轮：六条固定路径轮转，按预期落进三格。
pub fn version_rounds(rounds: u32) -> Result<VersionLedger, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(IsoCode::CoverBad.val());
    }
    let cur = CURRENT_VERSION;
    let mut lg = VersionLedger::new();
    let mut fp = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < rounds {
        let route = r % 6;
        let cand = match route {
            // 次版本升级（表内容也变了）。
            0 => TableVersion {
                version: TABLE_VERSION + 1,
                revision: 0,
                rows: ROW_COUNT as u16,
                checksum: TABLE_CHECKSUM ^ 0x55,
            },
            // 修订升级（同版本号，内容变）。
            1 => TableVersion {
                version: TABLE_VERSION,
                revision: TABLE_REVISION + 1,
                rows: ROW_COUNT as u16,
                checksum: TABLE_CHECKSUM ^ 0x11,
            },
            // 完全相同的版本：无需升级，按接受处理。
            2 => cur,
            // 版本倒退。
            3 => TableVersion {
                version: TABLE_VERSION - 1,
                revision: 0,
                rows: ROW_COUNT as u16,
                checksum: TABLE_CHECKSUM,
            },
            // 同版本同修订但内容被改。
            4 => TableVersion {
                version: TABLE_VERSION,
                revision: TABLE_REVISION,
                rows: ROW_COUNT as u16,
                checksum: TABLE_CHECKSUM ^ 0x01,
            },
            // 非法版本：版本号为 0。
            _ => TableVersion {
                version: 0,
                revision: 0,
                rows: ROW_COUNT as u16,
                checksum: 0,
            },
        };
        match accept_version(&cand, &cur) {
            Ok(_) => lg.upgrades = lg.upgrades.saturating_add(1),
            Err(e) => {
                if e == IsoCode::VersionBad.val() {
                    lg.downgrades = lg.downgrades.saturating_add(1);
                } else {
                    lg.tampered = lg.tampered.saturating_add(1);
                }
            }
        }
        fp = p_core::fold_u32(fp, route);
        fp = p_core::fold_u32(fp, cand.checksum as u32);
        lg.rounds = lg.rounds.saturating_add(1);
        r += 1;
    }
    lg.fingerprint = fp;
    if !lg.conserved() {
        return Err(IsoCode::LedgerBad.val());
    }
    Ok(lg)
}

// ── 防篡改注入 ──────────────────────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub attempts: u32,
    pub caught: u32,
    pub last_route: u32,
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        self.attempts > 0 && self.caught == self.attempts
    }
}

/// 单路篡改（十条互不相同的改法，全部必须被封印检出）。
pub fn tamper_once(route: u32) -> Result<(), u32> {
    let mut t = QUIRK_ROWS.to_vec();
    match route {
        0 => {
            if let Some(slot) = t.get_mut(0) {
                slot.vendor = slot.vendor.wrapping_add(1);
            }
        }
        1 => {
            if let Some(slot) = t.get_mut(1) {
                slot.model = slot.model.wrapping_add(1);
            }
        }
        2 => {
            if let Some(slot) = t.get_mut(2) {
                slot.fw_min = slot.fw_min.wrapping_add(1);
            }
        }
        3 => {
            if let Some(slot) = t.get_mut(3) {
                slot.param = slot.param.wrapping_add(1);
            }
        }
        4 => {
            if let Some(slot) = t.get_mut(4) {
                slot.flags ^= 0x08;
            }
        }
        5 => {
            // 交换两行：行号入摘要，换序必被检出。
            if let (Some(a), Some(b)) = (t.get(3).copied(), t.get(4).copied()) {
                if let Some(slot) = t.get_mut(3) {
                    *slot = b;
                }
                if let Some(slot) = t.get_mut(4) {
                    *slot = a;
                }
            }
        }
        6 => {
            // 复制一行到相邻位（行内容重复但行号不同）。
            if let (Some(src), Some(slot)) = (t.get(2).copied(), t.get_mut(3)) {
                *slot = src;
            }
        }
        7 => {
            // 截断一行：行数闸门。
            t.pop();
        }
        8 => {
            // 整表旋转一位。
            if let Some(first) = t.first().copied() {
                t.remove(0);
                t.push(first);
            }
        }
        _ => {
            // 全部参数清零。
            let mut i = 0usize;
            while i < t.len() {
                if let Some(slot) = t.get_mut(i) {
                    slot.param = 0;
                }
                i += 1;
            }
        }
    }
    verify_table(&t, TABLE_CHECKSUM).map(|_| ())
}

/// 防篡改注入账（判据 ×10：十条改法逐条检出）。
pub fn tamper_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(IsoCode::CoverBad.val());
    }
    let mut caught = 0u32;
    let mut last = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let route = r % TAMPER_ROUTES as u32;
        if tamper_once(route).is_err() {
            caught = caught.saturating_add(1);
        }
        last = route;
        r += 1;
    }
    Ok(InjectLog {
        attempts: rounds,
        caught,
        last_route: last,
    })
}

/// 反例必拒：六条独立反例。
pub fn tamper_rejected() -> bool {
    // 1) 校验和封印对不上。
    let c1 = verify_table(&QUIRK_ROWS, TABLE_CHECKSUM ^ 0x01).err() == Some(IsoCode::ChecksumBad.val());
    // 2) 行数少一条。
    let short = &QUIRK_ROWS[..ROW_COUNT - 1];
    let c2 = verify_table(short, TABLE_CHECKSUM).err() == Some(IsoCode::RowBad.val());
    // 3) 厂商为零。
    let mut t = QUIRK_ROWS.to_vec();
    if let Some(slot) = t.get_mut(2) {
        slot.vendor = 0;
    }
    let c3 = row_integrity(&t).err() == Some(IsoCode::RowBad.val());
    // 4) flags 高位。
    let mut t2 = QUIRK_ROWS.to_vec();
    if let Some(slot) = t2.get_mut(2) {
        slot.flags = 0x10;
    }
    let c4 = row_integrity(&t2).err() == Some(IsoCode::RowBad.val());
    // 5) 同版本改表：判据即红。
    let cur = CURRENT_VERSION;
    let tampered = TableVersion {
        checksum: TABLE_CHECKSUM ^ 0x02,
        ..cur
    };
    let c5 = accept_version(&tampered, &cur).err() == Some(IsoCode::ChecksumBad.val());
    // 6) 越权读返回拒绝而非数据。
    let store = Store::new();
    let c6 = store.read(1000, 1001) == ReadOutcome::Denied
        && store.read(0, 0) == ReadOutcome::Denied
        && enable_live_isolation().err() == Some(IsoCode::PreviewBad.val());
    c1 && c2 && c3 && c4 && c5 && c6
}

// ── 段位守卫 + 渲染 ─────────────────────────────────────────────
/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < IsoCode::ALL.len() {
        if let Some(c) = IsoCode::ALL.get(i) {
            if c.val() != SEG_BASE + 0x01 + i as u32 {
                return false;
            }
            if c.val() < SEG_BASE + 0x01 || c.val() > SEG_BASE + 0x0E {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

/// 六字段渲染：tag | params（表行数）| schedule（预演轮数）|
/// fails（泄漏数）| keys（越权拒绝数）| digest。
pub fn render_row(tag: &str, params: u32, schedule: u32, fails: u32, keys: u32, digest: u64) -> String {
    format!(
        "{}|params={}|schedule={}|fails={}|keys={}|digest={:016X}",
        tag, params, schedule, fails, keys, digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < 16 {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d)?;
        i += 1;
    }
    Some(v)
}

pub fn row_fields(row: &str) -> Option<[&str; RENDER_FIELDS]> {
    let mut parts: [Option<&str>; RENDER_FIELDS] = [None; RENDER_FIELDS];
    let mut n = 0usize;
    let mut it = row.split('|');
    while let Some(p) = it.next() {
        if n >= RENDER_FIELDS {
            return None;
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != RENDER_FIELDS {
        return None;
    }
    let mut out: [&str; RENDER_FIELDS] = [""; RENDER_FIELDS];
    let mut i = 0usize;
    while i < RENDER_FIELDS {
        match parts.get(i).and_then(|x| *x) {
            Some(v) => {
                if let Some(slot) = out.get_mut(i) {
                    *slot = v;
                }
            }
            None => return None,
        }
        i += 1;
    }
    Some(out)
}

pub fn audit_render(row: &str) -> bool {
    let parts = match row_fields(row) {
        Some(v) => v,
        None => return false,
    };
    let tag = match parts.get(0) {
        Some(v) if !v.is_empty() => *v,
        _ => return false,
    };
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut sp = p.split('=');
        let _key = sp.next();
        let num = match sp.next().and_then(parse_u32) {
            Some(v) => v,
            None => return false,
        };
        if sp.next().is_some() {
            return false;
        }
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5) {
        Some(v) => *v,
        None => return false,
    };
    let mut sp = dpart.split('=');
    let _k = sp.next();
    let digest = match sp.next().and_then(parse_hex16) {
        Some(v) => v,
        None => return false,
    };
    if sp.next().is_some() {
        return false;
    }
    render_row(tag, vals[0], vals[1], vals[2], vals[3], digest) == row
}

/// 账本渲染：params=行数, schedule=预演轮数, fails=泄漏数, keys=越权拒绝数。
pub fn ledger_row(lg: &FakeLedger) -> String {
    render_row(
        "I49007",
        ROW_COUNT as u32,
        lg.rounds,
        lg.leaks.saturating_add(lg.drift),
        lg.cross_denied,
        lg.fingerprint,
    )
}

/// 渲染自检。
pub fn render_selfcheck() -> bool {
    let lg = match fake_preview(PREVIEW_FAKE_ROUNDS) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let row = ledger_row(&lg);
    if !audit_render(&row) {
        return false;
    }
    let parts = match row_fields(&row) {
        Some(v) => v,
        None => return false,
    };
    if parts.get(1).copied() != Some("params=12") {
        return false;
    }
    if parts.get(2).copied() != Some("schedule=10") {
        return false;
    }
    if parts.get(3).copied() != Some("fails=0") {
        return false;
    }
    true
}

// ── 判据集 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49007 ①：命名空间自洽（四份 uid 唯一、配置非零）。
    let ns = namespace_integrity(&PROFILES);
    let ns_ok = matches!(&ns, Ok(fp) if *fp != 0);
    out.push((
        "F49007/命名空间自洽",
        ns_ok,
        format!("users={} fp={:016X}", USER_COUNT, ns.as_ref().copied().unwrap_or(0)),
    ));

    // F49007 ②：跨读全拒 + 零泄漏。
    let rd = read_rounds(READ_ROUNDS);
    let rd_ok = match &rd {
        Ok(l) => l.rounds == READ_ROUNDS && l.conserved() && l.leaks == 0 && l.denied > 0 && l.own > 0,
        Err(_) => false,
    };
    out.push((
        "F49007/跨读全拒",
        rd_ok,
        format!(
            "own={:?} denied={:?} leaks={:?}",
            rd.as_ref().map(|l| l.own),
            rd.as_ref().map(|l| l.denied),
            rd.as_ref().map(|l| l.leaks)
        ),
    ));

    // F49007 ③：独立写（改 A 不动 B）。
    let iso = independence_rounds(INDEPENDENCE_ROUNDS);
    let iso_ok = match &iso {
        Ok(l) => l.rounds == INDEPENDENCE_ROUNDS && l.conserved() && l.touched_other == 0,
        Err(_) => false,
    };
    out.push((
        "F49007/独立写隔离",
        iso_ok,
        format!(
            "writes={:?} self={:?} other={:?}",
            iso.as_ref().map(|l| l.writes),
            iso.as_ref().map(|l| l.touched_self),
            iso.as_ref().map(|l| l.touched_other)
        ),
    ));

    // F49007 ④：预告登记成文 + fake 预演 10/10 + 未接线。
    let fake = fake_preview(PREVIEW_FAKE_ROUNDS);
    let previews = previews_ok();
    let fake_ok = match &fake {
        Ok(l) => {
            l.rounds == PREVIEW_FAKE_ROUNDS
                && l.conserved()
                && l.leaks == 0
                && l.drift == 0
                && l.own_reads == PREVIEW_FAKE_ROUNDS * USER_COUNT as u32
                && l.cross_denied == PREVIEW_FAKE_ROUNDS * 12
        }
        Err(_) => false,
    };
    let live_denied = enable_live_isolation().is_err();
    out.push((
        "F49007/预告成文预演10/10",
        previews && fake_ok && live_denied,
        format!(
            "previews={} fake={:?}/{:?} live_denied={}",
            PREVIEW_COUNT,
            fake.as_ref().map(|l| l.rounds),
            fake.as_ref().map(|l| l.leaks),
            live_denied
        ),
    ));

    // F49008 ①：表行自审 + 编译期封印非零。
    let rows = row_integrity(&QUIRK_ROWS);
    let pin_ok = TABLE_CHECKSUM != 0 && verify_table(&QUIRK_ROWS, TABLE_CHECKSUM).is_ok();
    out.push((
        "F49008/表行自洽封印",
        rows.is_ok() && pin_ok,
        format!("rows={} checksum={:016X} ok={}", ROW_COUNT, TABLE_CHECKSUM, pin_ok),
    ));

    // F49008 ②：校验和 ×10 防篡改检出 10/10。
    let tamper = tamper_rounds(TAMPER_ROUTES as u32);
    let tamper_ok = match &tamper {
        Ok(l) => l.attempts == TAMPER_ROUTES as u32 && l.all_caught(),
        Err(_) => false,
    };
    out.push((
        "F49008/校验和×10检出",
        tamper_ok && tamper_rejected(),
        format!(
            "attempts={:?} caught={:?} rejected={}",
            tamper.as_ref().map(|l| l.attempts),
            tamper.as_ref().map(|l| l.caught),
            tamper_rejected()
        ),
    ));

    // F49008 ③：版本裁决（升级 / 倒退 / 同版本改表）。
    let ver = version_rounds(VERSION_ROUNDS);
    let ver_ok = match &ver {
        Ok(l) => {
            l.rounds == VERSION_ROUNDS
                && l.conserved()
                && l.upgrades > 0
                && l.downgrades > 0
                && l.tampered > 0
                && verify_version(&CURRENT_VERSION).is_ok()
        }
        Err(_) => false,
    };
    out.push((
        "F49008/版本裁决",
        ver_ok,
        format!(
            "up={:?} down={:?} tampered={:?}",
            ver.as_ref().map(|l| l.upgrades),
            ver.as_ref().map(|l| l.downgrades),
            ver.as_ref().map(|l| l.tampered)
        ),
    ));

    // F49008 ④：复跑零漂移 + 段位 + 渲染。
    let replay = preview_replay(PREVIEW_FAKE_ROUNDS) && read_replay(READ_ROUNDS);
    let seg = segment_guard();
    let render = render_selfcheck();
    let dg = fake.as_ref().map(|l| l.fingerprint).unwrap_or(0);
    out.push((
        "F49008/复跑与段位",
        replay && seg && render && dg != 0,
        format!(
            "replay={} seg={} render={} digest={:016X}",
            replay, seg, render, dg
        ),
    ));

    out
}

pub fn run_unx49007_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49008 配置隔离预告与表版本校验和");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_values_are_windowed_and_unique() {
        assert!(segment_guard());
        assert_eq!(IsoCode::NamespaceBad.val(), SEG_BASE + 0x01);
        assert_eq!(IsoCode::SegmentBad.val(), SEG_BASE + 14);
        assert_eq!(IsoCode::ALL.len(), 14);
    }

    #[test]
    fn every_variant_has_detail_and_free_lookup() {
        let mut i = 0usize;
        while i < IsoCode::ALL.len() {
            if let Some(c) = IsoCode::ALL.get(i) {
                assert!(!c.detail().is_empty());
                assert_eq!(detail(c.val()), c.detail());
            }
            i += 1;
        }
        assert_eq!(detail(0xFFFF), "未知诊断码");
    }

    #[test]
    fn namespaces_are_unique_and_nonzero() {
        assert!(namespace_integrity(&PROFILES).is_ok());
        let mut i = 0usize;
        while i < PROFILES.len() {
            assert_ne!(PROFILES[i].uid, 0);
            assert_ne!(PROFILES[i].gain, 0);
            assert_ne!(PROFILES[i].threshold, 0);
            let mut j = i + 1;
            while j < PROFILES.len() {
                assert_ne!(PROFILES[i].uid, PROFILES[j].uid);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn self_read_succeeds_and_cross_read_denied() {
        let st = Store::new();
        let own = st.read(1000, 1000);
        assert_eq!(own, ReadOutcome::Owned(PROFILES[0]));
        assert_eq!(st.read(1000, 1001), ReadOutcome::Denied);
        assert_eq!(st.read(1001, 1000), ReadOutcome::Denied);
        assert_eq!(st.read(0, 0), ReadOutcome::Denied);
        assert_eq!(st.read(4242, 4242), ReadOutcome::Denied);
        assert_eq!(st.read(1000, 4242), ReadOutcome::Denied);
    }

    #[test]
    fn read_rounds_never_leak() {
        let l = read_rounds(READ_ROUNDS).unwrap();
        assert_eq!(l.rounds, READ_ROUNDS);
        assert_eq!(l.leaks, 0);
        assert!(l.denied > 0);
        assert!(l.own > 0);
        assert!(l.conserved());
        assert_ne!(l.fingerprint, 0);
    }

    #[test]
    fn writes_touch_only_their_own_slot() {
        let mut st = Store::new();
        assert!(st.update_gain(1002, 99).is_ok());
        assert_eq!(st.slots[2].gain, 99);
        assert_eq!(st.slots[0], PROFILES[0]);
        assert_eq!(st.slots[1], PROFILES[1]);
        assert_eq!(st.slots[3], PROFILES[3]);
        assert_eq!(st.update_gain(4242, 50).err(), Some(IsoCode::NamespaceBad.val()));
        assert_eq!(st.update_gain(1000, 0).err(), Some(IsoCode::NamespaceBad.val()));
    }

    #[test]
    fn independence_rounds_conserve() {
        let l = independence_rounds(INDEPENDENCE_ROUNDS).unwrap();
        assert_eq!(l.rounds, INDEPENDENCE_ROUNDS);
        assert_eq!(l.touched_other, 0);
        assert_eq!(l.writes, INDEPENDENCE_ROUNDS);
        assert!(l.conserved());
    }

    #[test]
    fn preview_registration_is_written_but_not_wired() {
        assert!(previews_ok());
        let mut i = 0usize;
        while i < PREVIEWS.len() {
            let p = PREVIEWS[i];
            assert_eq!(p.status, PreviewStatus::Planned);
            assert!(!p.wired);
            assert_eq!(p.cosign, "L2");
            assert_eq!(p.fake_pass, p.fake_runs);
            assert!(!p.contract.is_empty());
            i += 1;
        }
        assert_eq!(enable_live_isolation().err(), Some(IsoCode::PreviewBad.val()));
    }

    #[test]
    fn fake_preview_is_ten_out_of_ten() {
        let l = fake_preview(PREVIEW_FAKE_ROUNDS).unwrap();
        assert_eq!(l.rounds, 10);
        assert_eq!(l.own_reads, 40);
        assert_eq!(l.cross_denied, 120);
        assert_eq!(l.leaks, 0);
        assert_eq!(l.drift, 0);
        assert!(l.conserved());
    }

    #[test]
    fn preview_replay_is_stable() {
        assert!(preview_replay(PREVIEW_FAKE_ROUNDS));
    }

    #[test]
    fn quirk_rows_are_self_consistent_and_sealed() {
        assert!(row_integrity(&QUIRK_ROWS).is_ok());
        assert!(verify_table(&QUIRK_ROWS, TABLE_CHECKSUM).is_ok());
        assert!(verify_version(&CURRENT_VERSION).is_ok());
        assert_ne!(TABLE_CHECKSUM, 0);
        assert_eq!(table_checksum(&QUIRK_ROWS), TABLE_CHECKSUM);
    }

    #[test]
    fn checksum_is_index_sensitive() {
        let mut t = QUIRK_ROWS.to_vec();
        t.swap(0, 1);
        assert_ne!(table_checksum(&t), TABLE_CHECKSUM);
        // 行内容集合相同、顺序不同 → 摘要必须不同。
        let mut u = QUIRK_ROWS.to_vec();
        u.swap(5, 6);
        assert_ne!(table_checksum(&u), TABLE_CHECKSUM);
    }

    #[test]
    fn all_ten_tamper_routes_are_caught() {
        let l = tamper_rounds(TAMPER_ROUTES as u32).unwrap();
        assert_eq!(l.attempts, 10);
        assert_eq!(l.caught, 10);
        assert!(l.all_caught());
        let mut route = 0u32;
        while route < TAMPER_ROUTES as u32 {
            assert!(tamper_once(route).is_err());
            route += 1;
        }
    }

    #[test]
    fn tamper_cases_are_all_rejected() {
        assert!(tamper_rejected());
    }

    #[test]
    fn versions_upgrade_and_never_downgrade() {
        let cur = CURRENT_VERSION;
        let up = TableVersion {
            version: TABLE_VERSION + 1,
            revision: 0,
            rows: ROW_COUNT as u16,
            checksum: TABLE_CHECKSUM ^ 0x33,
        };
        assert_eq!(accept_version(&up, &cur), Ok(Verdict::Upgrade));
        let down = TableVersion {
            version: TABLE_VERSION - 1,
            revision: 0,
            rows: ROW_COUNT as u16,
            checksum: TABLE_CHECKSUM,
        };
        assert_eq!(accept_version(&down, &cur).err(), Some(IsoCode::VersionBad.val()));
        let same_bad = TableVersion {
            checksum: TABLE_CHECKSUM ^ 0x05,
            ..cur
        };
        assert_eq!(
            accept_version(&same_bad, &cur).err(),
            Some(IsoCode::ChecksumBad.val())
        );
        assert_eq!(accept_version(&cur, &cur), Ok(Verdict::Upgrade));
    }

    #[test]
    fn version_rounds_cover_three_verdicts() {
        let l = version_rounds(VERSION_ROUNDS).unwrap();
        assert_eq!(l.rounds, VERSION_ROUNDS);
        assert!(l.upgrades > 0);
        assert!(l.downgrades > 0);
        assert!(l.tampered > 0);
        assert!(l.conserved());
    }

    #[test]
    fn rounds_bounds_are_enforced() {
        assert!(read_rounds(0).is_err());
        assert!(read_rounds(FUZZ_ROUNDS + 1).is_err());
        assert!(fake_preview(0).is_err());
        assert!(fake_preview(SCENARIO_MAX + 1).is_err());
        assert!(tamper_rounds(0).is_err());
        assert!(version_rounds(0).is_err());
    }

    #[test]
    fn render_round_trips_and_rejects_bad_rows() {
        let r = render_row("I49007", 12, 10, 0, 120, 0xFEED_FACE_0123_4567);
        assert!(audit_render(&r));
        assert!(!audit_render(""));
        assert!(!audit_render("I49007|params=12|schedule=10|fails=0|keys=120"));
        assert!(!audit_render("I49007|params=q|schedule=10|fails=0|keys=120|digest=0000000000000001"));
        assert!(!audit_render("I49007|params=12|schedule=10|fails=0|keys=120|digest=12"));
        assert_eq!(parse_u32("120"), Some(120));
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_hex16("FEEDFACE01234567"), Some(0xFEED_FACE_0123_4567));
        assert_eq!(parse_hex16("FEEDFACE0123456"), None);
    }

    #[test]
    fn ledger_row_is_auditable() {
        let l = fake_preview(PREVIEW_FAKE_ROUNDS).unwrap();
        let row = ledger_row(&l);
        assert!(audit_render(&row));
        assert!(row.starts_with("I49007|params=12|schedule=10|fails=0|keys=120|"));
        assert!(render_selfcheck());
    }

    #[test]
    fn checkset_is_complete_and_untruncated() {
        let s = run_unx49007_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(extra_checks().len(), 8);
        assert_eq!(STATS_FIELDS, 8);
        assert_eq!(RENDER_FIELDS, 6);
    }

    #[test]
    fn preview_status_names_are_stable() {
        assert_eq!(PreviewStatus::Planned.name(), "planned");
        assert_eq!(PreviewStatus::Signed.name(), "signed");
        assert_eq!(PreviewStatus::Live.name(), "live");
    }

    #[test]
    fn verdict_table_is_total() {
        let cur = CURRENT_VERSION;
        let cases = [
            TableVersion {
                version: 4,
                revision: 0,
                rows: 12,
                checksum: 1,
            },
            TableVersion {
                version: 3,
                revision: 1,
                rows: 12,
                checksum: 1,
            },
            TableVersion {
                version: 3,
                revision: 0,
                rows: 12,
                checksum: cur.checksum,
            },
            TableVersion {
                version: 2,
                revision: 9,
                rows: 12,
                checksum: 1,
            },
        ];
        assert_eq!(verdict_of(&cases[0], &cur), Verdict::Upgrade);
        assert_eq!(verdict_of(&cases[1], &cur), Verdict::Upgrade);
        assert_eq!(verdict_of(&cases[2], &cur), Verdict::Upgrade);
        assert_eq!(verdict_of(&cases[3], &cur), Verdict::Downgrade);
    }
}
