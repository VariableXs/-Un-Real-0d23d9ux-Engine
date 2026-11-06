//! UNX-F12769 术语表一致性核验（全册术语统一） · 0x82A40段
//!
//! 判据原文（派工卡 12769 逐字锚定）：
//!   UNX-F12770-J1 域经十条齐（经验/适用面/证据三列），逐条可点验。
//!
//! 设计要点：
//! 1. 术语一致性域模型：核心术语登记表为冻结闭集（44 条 ≥ 40 条下限），
//!    每条含 术语号/规范名/别名闭集/归属册 四元组，闭集一次冻结不热改，
//!    术语号 1..=N 连续无洞，别名跨术语零冲突；
//! 2. 术语一致性判定器 judge_term(term_no, used_name)：三段判定次序——
//!    未登记术语号 → ENOENT；用名越出规范名+别名闭集 → EINVAL；
//!    跨册使用且无准入 → EPERM（EPERM 在登记器 use_term 里第三段把关）；
//! 3. 漂移注入器：三类漂移（名称漂移=used_name 不在规范名/别名闭集、
//!    跨册越权、未登记术语）各 10 轮全抓，任何一轮逃脱即判据失效；
//! 4. 域经十条齐：十条经（冻结/别名/归属/跨册/次序/漂移/容量/指纹/
//!    单调/点验）每条 经验/适用面/证据 三列齐，逐条可点验——判据 J1 直读口；
//! 5. 登记器状态机 编译→冻结→颁行 单向推进，倒退显性拒；颁行后跨册
//!    准入门封死；审计账定容（AUDIT_CAP），溢出显性拒（ECap）不静默；
//! 6. 双跑一致指纹：FNV-1a 逐字节混合 登记表+判定语料+漂移战报+颁行审计，
//!    两次完整会话指纹必须相等；no_std + alloc；零浮点、零 IO、零墙钟、
//!    零随机（splitmix64 种子写死）；取值一律 match/if let/.get()。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12769 − 12600)）。
pub const SEG_BASE: u32 = 0x82A40;

// ── 术语一致性诊断码闭集 ─────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0D 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GxDiag {
    /// 未登记术语（术语号越出登记表闭集）。
    ENoent,
    /// 别名越集（用名既非规范名也不在别名闭集）。
    Einval,
    /// 跨册越权（在非归属册使用且无跨册准入）。
    Eperm,
    /// 登记表已颁行，写入门拒绝（准入/登记后改表）。
    EFrozen,
    /// 未冻结先颁行（状态机越级）。
    ENotFrozen,
    /// 状态机倒退（冻结后再冻结、颁行后再颁行）。
    ERewind,
    /// 术语号重复或断号（闭集完整性闸）。
    EDupNo,
    /// 别名跨术语冲突（一别名挂两术语、或别名撞他术语规范名）。
    EDupAlias,
    /// 别名闭集为空（登记条目残缺）。
    ENoAlias,
    /// 审计账容量耗尽（定容显性拒，不静默丢弃）。
    ECap,
    /// 漂移注入逃脱（注入器没被判定器抓住，判据失效）。
    EDrift,
    /// 双跑指纹不一致（确定性被破坏，全流程复核）。
    EFinger,
    /// 判定次序失守（三段判定次序被颠倒）。
    EOrder,
}

impl GxDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0D 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                GxDiag::ENoent => 0x01,
                GxDiag::Einval => 0x02,
                GxDiag::Eperm => 0x03,
                GxDiag::EFrozen => 0x04,
                GxDiag::ENotFrozen => 0x05,
                GxDiag::ERewind => 0x06,
                GxDiag::EDupNo => 0x07,
                GxDiag::EDupAlias => 0x08,
                GxDiag::ENoAlias => 0x09,
                GxDiag::ECap => 0x0a,
                GxDiag::EDrift => 0x0b,
                GxDiag::EFinger => 0x0c,
                GxDiag::EOrder => 0x0d,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            GxDiag::ENoent => "未登记术语：术语号不在冻结闭集内，先查登记表补录再使用，禁止现用现造名",
            GxDiag::Einval => "别名越集：用名既非规范名也不在别名闭集，改用规范名或先走扩集评审",
            GxDiag::Eperm => "跨册越权：术语未在当前册准入，先走跨册准入门登记再引用",
            GxDiag::EFrozen => "登记表已颁行：颁行后写入门拒绝，改动走下一轮术语增补流程",
            GxDiag::ENotFrozen => "未冻结先颁行：先过完整性三闸冻结登记表，再走颁行口",
            GxDiag::ERewind => "状态机倒退：编译→冻结→颁行只进不退，倒退即登记器可疑",
            GxDiag::EDupNo => "术语号重复或断号：登记闭集必须 1..=N 连续唯一，先修号再冻结",
            GxDiag::EDupAlias => "别名跨术语冲突：一个别名只许挂一个术语，撞规范名同样拒，先改别名",
            GxDiag::ENoAlias => "别名闭集为空：登记条目残缺，至少登记一个别名再冻结",
            GxDiag::ECap => "审计账容量耗尽：定容显性拒不静默，先结转当前审计账再续记",
            GxDiag::EDrift => "漂移注入逃脱：判定器没抓住注入轮，三段判定面须整体复核",
            GxDiag::EFinger => "双跑指纹不一致：确定性被破坏，按 FNV 混合段逐段比对两跑差异",
            GxDiag::EOrder => "判定次序失守：未登记→越别名→越权次序被颠倒，先修判定器段序",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const GX_ALL: &[GxDiag] = &[
    GxDiag::ENoent,
    GxDiag::Einval,
    GxDiag::Eperm,
    GxDiag::EFrozen,
    GxDiag::ENotFrozen,
    GxDiag::ERewind,
    GxDiag::EDupNo,
    GxDiag::EDupAlias,
    GxDiag::ENoAlias,
    GxDiag::ECap,
    GxDiag::EDrift,
    GxDiag::EFinger,
    GxDiag::EOrder,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in GX_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82A41..0x82A4D 闭集内"
}

// ── 归属册闭集 ───────────────────────────────────────────────────
/// 归属册闭集：全册术语统一按四册划分归属。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Volume {
    /// 核心册：内核机制与内存/中断/同步原语。
    CoreVol,
    /// 驱动册：设备/总线/中断服务与电源。
    DriverVol,
    /// 网络册：协议栈/拥塞/路由。
    NetVol,
    /// 外壳册：命令行/管道/补全与作业。
    ShellVol,
}

impl Volume {
    /// 册序号（闭集序）。
    pub fn index(self) -> u8 {
        match self {
            Volume::CoreVol => 0,
            Volume::DriverVol => 1,
            Volume::NetVol => 2,
            Volume::ShellVol => 3,
        }
    }

    /// 册中文名。
    pub fn name(self) -> &'static str {
        match self {
            Volume::CoreVol => "核心册",
            Volume::DriverVol => "驱动册",
            Volume::NetVol => "网络册",
            Volume::ShellVol => "外壳册",
        }
    }

    /// 由册序号反查闭集册（越界返回 None）。
    pub fn from_index(idx: u8) -> Option<Volume> {
        match idx {
            0 => Some(Volume::CoreVol),
            1 => Some(Volume::DriverVol),
            2 => Some(Volume::NetVol),
            3 => Some(Volume::ShellVol),
            _ => None,
        }
    }
}

/// 四册闭集数组（册序即登记序）。
pub const VOLUME_ALL: &[Volume] = &[
    Volume::CoreVol,
    Volume::DriverVol,
    Volume::NetVol,
    Volume::ShellVol,
];

// ── 核心术语登记表（冻结闭集，≥40 条） ───────────────────────────
/// 登记表条目数下限（判据沉度抓手：核心术语登记表 ≥40 条冻结闭集）。
pub const TERM_MIN: usize = 40;

/// 单条术语登记：术语号/规范名/别名闭集/归属册 四元组。
/// 来源：全册术语统一协议（核心/驱动/网络/外壳四册术语分册，一次冻结）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermEntry {
    /// 术语号（1..=N 连续无洞）。
    pub term_no: u32,
    /// 规范名（唯一，别名不得撞规范名）。
    pub canonical: &'static str,
    /// 别名闭集（非空；用名只认规范名+本闭集）。
    pub aliases: &'static [&'static str],
    /// 归属册（唯一；跨册使用须准入）。
    pub volume: Volume,
}

impl TermEntry {
    /// 按术语号查登记表（越闭集返回 None）。
    pub fn by_no(term_no: u32) -> Option<&'static TermEntry> {
        for t in TERM_TABLE.iter() {
            if t.term_no == term_no {
                return Some(t);
            }
        }
        None
    }

    /// 用名是否落在本条规范名/别名闭集内。
    pub fn covers(&self, used_name: &str) -> bool {
        if used_name == self.canonical {
            return true;
        }
        for a in self.aliases.iter() {
            if *a == used_name {
                return true;
            }
        }
        false
    }
}

/// 核心术语登记表：44 条冻结闭集（术语号 1..=44 连续，别名零冲突）。
pub const TERM_TABLE: &[TermEntry] = &[
    TermEntry { term_no: 1, canonical: "页表", aliases: &["页表项", "PT"], volume: Volume::CoreVol },
    TermEntry { term_no: 2, canonical: "中断向量", aliases: &["向量号", "IVT"], volume: Volume::CoreVol },
    TermEntry { term_no: 3, canonical: "系统调用", aliases: &["syscall", "系统入口"], volume: Volume::CoreVol },
    TermEntry { term_no: 4, canonical: "上下文切换", aliases: &["ctxsw", "任务切换"], volume: Volume::CoreVol },
    TermEntry { term_no: 5, canonical: "物理页框", aliases: &["页框", "PFN"], volume: Volume::CoreVol },
    TermEntry { term_no: 6, canonical: "虚拟地址", aliases: &["VA", "虚址"], volume: Volume::CoreVol },
    TermEntry { term_no: 7, canonical: "内核栈", aliases: &["核心栈"], volume: Volume::CoreVol },
    TermEntry { term_no: 8, canonical: "自旋锁", aliases: &["spinlock", "旋锁"], volume: Volume::CoreVol },
    TermEntry { term_no: 9, canonical: "信号量", aliases: &["计数信号量", "sem"], volume: Volume::CoreVol },
    TermEntry { term_no: 10, canonical: "内存屏障", aliases: &["栅障", "barrier"], volume: Volume::CoreVol },
    TermEntry { term_no: 11, canonical: "高端内存", aliases: &["高端映像"], volume: Volume::CoreVol },
    TermEntry { term_no: 12, canonical: "页面置换", aliases: &["换页", "置换"], volume: Volume::CoreVol },
    TermEntry { term_no: 13, canonical: "中断下半部", aliases: &["软中断", "bottom-half"], volume: Volume::CoreVol },
    TermEntry { term_no: 14, canonical: "缺页异常", aliases: &["缺页", "page-fault"], volume: Volume::CoreVol },
    TermEntry { term_no: 15, canonical: "内存描述符", aliases: &["memdesc", "内存描述"], volume: Volume::CoreVol },
    TermEntry { term_no: 16, canonical: "引导协议", aliases: &["bootproto", "启动协议"], volume: Volume::CoreVol },
    TermEntry { term_no: 17, canonical: "设备树", aliases: &["DT", "devicetree"], volume: Volume::DriverVol },
    TermEntry { term_no: 18, canonical: "中断请求级", aliases: &["IRQL", "请求级"], volume: Volume::DriverVol },
    TermEntry { term_no: 19, canonical: "直接内存访问", aliases: &["DMA", "直访"], volume: Volume::DriverVol },
    TermEntry { term_no: 20, canonical: "中断服务例程", aliases: &["ISR", "中断例程"], volume: Volume::DriverVol },
    TermEntry { term_no: 21, canonical: "队列对", aliases: &["QP", "队偶"], volume: Volume::DriverVol },
    TermEntry { term_no: 22, canonical: "描述符环", aliases: &["描述环", "desc-ring"], volume: Volume::DriverVol },
    TermEntry { term_no: 23, canonical: "电源状态", aliases: &["P态", "电源级"], volume: Volume::DriverVol },
    TermEntry { term_no: 24, canonical: "热插拔", aliases: &["hotplug", "热插"], volume: Volume::DriverVol },
    TermEntry { term_no: 25, canonical: "总线枚举", aliases: &["枚举扫描", "bus-enum"], volume: Volume::DriverVol },
    TermEntry { term_no: 26, canonical: "驱动绑定", aliases: &["bind", "挂接驱动"], volume: Volume::DriverVol },
    TermEntry { term_no: 27, canonical: "套接字", aliases: &["socket", "套接口"], volume: Volume::NetVol },
    TermEntry { term_no: 28, canonical: "三次握手", aliases: &["握手三步", "3whs"], volume: Volume::NetVol },
    TermEntry { term_no: 29, canonical: "慢启动", aliases: &["slow-start", "慢起"], volume: Volume::NetVol },
    TermEntry { term_no: 30, canonical: "拥塞窗口", aliases: &["cwnd", "拥塞窗"], volume: Volume::NetVol },
    TermEntry { term_no: 31, canonical: "报文段", aliases: &["segment", "报段"], volume: Volume::NetVol },
    TermEntry { term_no: 32, canonical: "校验和", aliases: &["checksum", "和验"], volume: Volume::NetVol },
    TermEntry { term_no: 33, canonical: "路由表", aliases: &["转发表", "route-table"], volume: Volume::NetVol },
    TermEntry { term_no: 34, canonical: "网络前缀", aliases: &["prefix", "前缀"], volume: Volume::NetVol },
    TermEntry { term_no: 35, canonical: "帧定界", aliases: &["分帧", "framing"], volume: Volume::NetVol },
    TermEntry { term_no: 36, canonical: "回环接口", aliases: &["loopback", "环回口"], volume: Volume::NetVol },
    TermEntry { term_no: 37, canonical: "命令行", aliases: &["cmdline", "命令串"], volume: Volume::ShellVol },
    TermEntry { term_no: 38, canonical: "管道", aliases: &["pipe", "管线"], volume: Volume::ShellVol },
    TermEntry { term_no: 39, canonical: "通配符", aliases: &["glob", "万用符"], volume: Volume::ShellVol },
    TermEntry { term_no: 40, canonical: "环境变量", aliases: &["envvar", "环变量"], volume: Volume::ShellVol },
    TermEntry { term_no: 41, canonical: "作业控制", aliases: &["jobctl", "作业管制"], volume: Volume::ShellVol },
    TermEntry { term_no: 42, canonical: "命令补全", aliases: &["completion", "补全"], volume: Volume::ShellVol },
    TermEntry { term_no: 43, canonical: "别名替换", aliases: &["alias-展开", "替名"], volume: Volume::ShellVol },
    TermEntry { term_no: 44, canonical: "提示符", aliases: &["prompt", "命令提示"], volume: Volume::ShellVol },
];

// ── 术语一致性判定器（三段判定次序） ─────────────────────────────
/// 术语一致性判定器：未登记 → ENOENT；越别名闭集 → EINVAL。
/// 第三段（跨册越权 → EPERM）依赖使用现场册位，在登记器 use_term 里把关。
/// 判定次序冻结：先存在性、再别名闭集、后跨册——次序颠倒即 EOrder 失守。
pub fn judge_term(term_no: u32, used_name: &str) -> Result<(), u32> {
    // 第一段：登记存在性。
    let t = match TermEntry::by_no(term_no) {
        Some(t) => t,
        None => return Err(GxDiag::ENoent.val()),
    };
    // 第二段：规范名/别名闭集。
    if t.covers(used_name) {
        return Ok(());
    }
    Err(GxDiag::Einval.val())
}

// ── 登记器状态机与审计账 ─────────────────────────────────────────
/// 登记器状态机：编译 → 冻结 → 颁行，单向推进，倒退显性拒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlossaryState {
    /// 编译中：允许跨册准入登记，登记表内容以冻结闭集为准。
    Compiling,
    /// 已冻结：完整性三闸（号闭集/别名非空/别名零冲突）全过。
    Frozen,
    /// 已颁行：漂移演练 10/10 全抓后颁行，写入门封死。
    Enacted,
}

impl GlossaryState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            GlossaryState::Compiling => 0,
            GlossaryState::Frozen => 1,
            GlossaryState::Enacted => 2,
        }
    }
}

/// 审计用名种类：规范名直用 / 别名闭集内用名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsedKind {
    /// 规范名直用。
    Canonical,
    /// 别名闭集内用名。
    Alias,
}

impl UsedKind {
    /// 种类序号（指纹混合用）。
    pub fn index(self) -> u8 {
        match self {
            UsedKind::Canonical => 0,
            UsedKind::Alias => 1,
        }
    }

    /// 种类中文名。
    pub fn name(self) -> &'static str {
        match self {
            UsedKind::Canonical => "规范名",
            UsedKind::Alias => "别名",
        }
    }
}

/// 单条使用审计：谁（术语号）以何种用名（种类）在哪个册被判定通过。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditRecord {
    /// 术语号。
    pub term_no: u32,
    /// 用名种类。
    pub kind: UsedKind,
    /// 使用现场册位。
    pub at: Volume,
}

/// 审计账定容（溢出显性拒 ECap，不静默丢弃）。
pub const AUDIT_CAP: usize = 64;

/// 术语登记器：状态机 + 跨册准入门 + 使用审计账。
#[derive(Clone, Debug)]
pub struct GlossaryRegistry {
    /// 跨册准入门：(术语号, 准入册) 对。
    admissions: Vec<(u32, Volume)>,
    /// 使用审计账（定容）。
    audit: Vec<AuditRecord>,
    /// 状态机。
    state: GlossaryState,
}

impl GlossaryRegistry {
    /// 开一台新登记器（编译态、无准入、空审计）。
    pub fn new() -> GlossaryRegistry {
        GlossaryRegistry {
            admissions: Vec::new(),
            audit: Vec::new(),
            state: GlossaryState::Compiling,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> GlossaryState {
        self.state
    }

    /// 审计账条数。
    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }

    /// 取第 idx 条审计（越界 None）。
    pub fn audit_at(&self, idx: usize) -> Option<AuditRecord> {
        self.audit.get(idx).copied()
    }

    /// 跨册准入条数。
    pub fn admissions_len(&self) -> usize {
        self.admissions.len()
    }

    /// 是否已准入 (term_no, at)。
    fn admitted(&self, term_no: u32, at: Volume) -> bool {
        for pair in self.admissions.iter() {
            let (no, v) = *pair;
            if no == term_no && v == at {
                return true;
            }
        }
        false
    }

    /// 跨册准入门：给 term_no 开 at 册准入。幂等；颁行后封死（EFrozen）；
    /// 未登记术语号拒（ENOENT）。
    pub fn admit(&mut self, term_no: u32, at: Volume) -> Result<(), u32> {
        if self.state.rank() >= GlossaryState::Enacted.rank() {
            return Err(GxDiag::EFrozen.val());
        }
        match TermEntry::by_no(term_no) {
            Some(_) => {}
            None => return Err(GxDiag::ENoent.val()),
        }
        if self.admitted(term_no, at) {
            // 幂等准入：重复开同一扇门不重复记账。
            return Ok(());
        }
        self.admissions.push((term_no, at));
        Ok(())
    }

    /// 审计入账（定容闸）。
    fn record(&mut self, term_no: u32, kind: UsedKind, at: Volume) -> Result<(), u32> {
        if self.audit.len() >= AUDIT_CAP {
            return Err(GxDiag::ECap.val());
        }
        self.audit.push(AuditRecord { term_no, kind, at });
        Ok(())
    }

    /// 现场判定：三段次序 未登记(ENOENT) → 越别名(EINVAL) → 跨册越权(EPERM)，
    /// 全过后入审计账（账满 ECap）。
    pub fn use_term(&mut self, term_no: u32, used_name: &str, at: Volume) -> Result<(), u32> {
        // 第一段：登记存在性。
        let t = match TermEntry::by_no(term_no) {
            Some(t) => t,
            None => return Err(GxDiag::ENoent.val()),
        };
        // 第二段：规范名/别名闭集。
        let kind = if used_name == t.canonical {
            UsedKind::Canonical
        } else if {
            let mut hit = false;
            for a in t.aliases.iter() {
                if *a == used_name {
                    hit = true;
                    break;
                }
            }
            hit
        } {
            UsedKind::Alias
        } else {
            return Err(GxDiag::Einval.val());
        };
        // 第三段：跨册准入门。
        if at != t.volume && !self.admitted(term_no, at) {
            return Err(GxDiag::Eperm.val());
        }
        // 入审计账。
        self.record(term_no, kind, at)
    }

    /// 冻结：完整性三闸（号闭集连续唯一 / 别名非空 / 别名零冲突）全过才许冻结。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.state.rank() >= GlossaryState::Frozen.rank() {
            return Err(GxDiag::ERewind.val());
        }
        // 闸一：条数下限 + 号闭集 1..=N 连续唯一。
        if TERM_TABLE.len() < TERM_MIN {
            return Err(GxDiag::EDupNo.val());
        }
        let mut expect: u32 = 1;
        for t in TERM_TABLE.iter() {
            if t.term_no != expect {
                return Err(GxDiag::EDupNo.val());
            }
            // 闸二：别名闭集非空。
            if t.aliases.is_empty() {
                return Err(GxDiag::ENoAlias.val());
            }
            expect = expect.wrapping_add(1);
        }
        // 闸三：别名零冲突（别名互异 + 不撞他条规范名）。
        if !alias_conflict_ok() {
            return Err(GxDiag::EDupAlias.val());
        }
        self.state = GlossaryState::Frozen;
        Ok(())
    }

    /// 颁行：先冻结、再漂移演练 3×10 轮全抓，才许颁行。
    /// 成功返回 (抓到轮数, 总轮数)。
    pub fn enact(&mut self) -> Result<(usize, usize), u32> {
        if self.state.rank() >= GlossaryState::Enacted.rank() {
            return Err(GxDiag::ERewind.val());
        }
        if self.state.rank() < GlossaryState::Frozen.rank() {
            return Err(GxDiag::ENotFrozen.val());
        }
        let rep = drift_sweep();
        if !rep.all_caught() {
            return Err(GxDiag::EDrift.val());
        }
        self.state = GlossaryState::Enacted;
        Ok((rep.total_caught(), rep.total()))
    }
}

// ── 确定性发生器（种子 = SEG_BASE，零随机红线合规） ──────────────
/// splitmix64：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性）。
    pub fn next_u64(&mut self) -> u64 {
        // splitmix64 常数（Steele-Lea 推荐，整数发生器常用闭集）。
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        (self.next_u64() % bound as u64) as u32
    }
}

// ── 漂移注入器：三类漂移各 10 轮全抓 ─────────────────────────────
/// 漂移种类闭集：名称漂移 / 跨册越权 / 未登记术语。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriftKind {
    /// 名称漂移：used_name 与规范名/别名闭集不符。
    NameDrift,
    /// 跨册越权：在非归属册使用且无准入。
    CrossVolume,
    /// 未登记：术语号越出登记闭集。
    Unregistered,
}

impl DriftKind {
    /// 种类中文名。
    pub fn name(self) -> &'static str {
        match self {
            DriftKind::NameDrift => "名称漂移",
            DriftKind::CrossVolume => "跨册越权",
            DriftKind::Unregistered => "未登记",
        }
    }
}

/// 漂移演练战报：三类各 10 轮，抓到数与逃脱描述。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriftReport {
    /// 名称漂移抓到轮数。
    pub name_caught: usize,
    /// 名称漂移总轮数。
    pub name_total: usize,
    /// 跨册越权抓到轮数。
    pub cross_caught: usize,
    /// 跨册越权总轮数。
    pub cross_total: usize,
    /// 未登记抓到轮数。
    pub unreg_caught: usize,
    /// 未登记总轮数。
    pub unreg_total: usize,
    /// 首个逃脱轮描述（空串即无逃脱）。
    pub escape: String,
}

impl DriftReport {
    /// 空战报。
    fn new() -> DriftReport {
        DriftReport {
            name_caught: 0,
            name_total: 0,
            cross_caught: 0,
            cross_total: 0,
            unreg_caught: 0,
            unreg_total: 0,
            escape: String::new(),
        }
    }

    /// 三类是否全部抓满（无逃脱）。
    pub fn all_caught(&self) -> bool {
        self.name_caught == self.name_total
            && self.cross_caught == self.cross_total
            && self.unreg_caught == self.unreg_total
            && self.name_total > 0
            && self.cross_total > 0
            && self.unreg_total > 0
            && self.escape.is_empty()
    }

    /// 抓到总轮数。
    pub fn total_caught(&self) -> usize {
        self.name_caught + self.cross_caught + self.unreg_caught
    }

    /// 总轮数。
    pub fn total(&self) -> usize {
        self.name_total + self.cross_total + self.unreg_total
    }
}

/// 记一笔逃脱（只记首笔）。
fn note_escape(rep: &mut DriftReport, k: usize, kind: DriftKind, msg: &str) {
    if rep.escape.is_empty() {
        rep.escape = format!("路{} {} 逃脱：{}", k, kind.name(), msg);
    }
}

/// 漂移注入器：三类漂移各 10 轮，注入必须被判定器按预期码抓住。
/// 名称漂移/未登记走 judge_term 直判；跨册走无准入登记器 use_term。
pub fn drift_sweep() -> DriftReport {
    let mut rep = DriftReport::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let table_n = TERM_TABLE.len() as u32;

    // 名称漂移 10 轮：规范名后缀漂移，必越别名闭集（EINVAL）。
    let mut k = 0usize;
    while k < 10 {
        let pick = rng.below(table_n) + 1;
        let t = match TermEntry::by_no(pick) {
            Some(t) => t,
            None => {
                note_escape(&mut rep, k, DriftKind::NameDrift, "注入寻址落空");
                rep.name_total += 1;
                k += 1;
                continue;
            }
        };
        let drift_name = format!("{}·漂移{}", t.canonical, k);
        let expect = GxDiag::Einval.val();
        rep.name_total += 1;
        match judge_term(t.term_no, &drift_name) {
            Err(e) if e == expect => rep.name_caught += 1,
            _ => note_escape(&mut rep, k, DriftKind::NameDrift, "漂移名未被 EINVAL 拒"),
        }
        k += 1;
    }

    // 跨册越权 10 轮：规范名 + 非归属册 + 无准入（EPERM）。
    k = 0;
    while k < 10 {
        let pick = rng.below(table_n) + 1;
        let t = match TermEntry::by_no(pick) {
            Some(t) => t,
            None => {
                note_escape(&mut rep, k, DriftKind::CrossVolume, "注入寻址落空");
                rep.cross_total += 1;
                k += 1;
                continue;
            }
        };
        // 目标册 = 归属册之外（确定性偏移，保证异册）。
        let home = t.volume.index();
        let mut ti = (home + 1 + (rng.below(3) as u8)) % 4;
        if ti == home {
            ti = (home + 1) % 4;
        }
        let target = match Volume::from_index(ti) {
            Some(v) => v,
            None => Volume::CoreVol,
        };
        let mut reg = GlossaryRegistry::new();
        let expect = GxDiag::Eperm.val();
        rep.cross_total += 1;
        match reg.use_term(t.term_no, t.canonical, target) {
            Err(e) if e == expect => rep.cross_caught += 1,
            _ => note_escape(&mut rep, k, DriftKind::CrossVolume, "跨册越权未被 EPERM 拒"),
        }
        k += 1;
    }

    // 未登记 10 轮：术语号落在 8000..=8099（登记闭集 1..=44 之外，ENOENT）。
    k = 0;
    while k < 10 {
        let bad_no = 8000u32 + rng.below(100);
        let expect = GxDiag::ENoent.val();
        rep.unreg_total += 1;
        match judge_term(bad_no, "任意名") {
            Err(e) if e == expect => rep.unreg_caught += 1,
            _ => note_escape(&mut rep, k, DriftKind::Unregistered, "未登记号未被 ENOENT 拒"),
        }
        k += 1;
    }
    rep
}

// ── 双跑一致指纹（FNV-1a 逐字节混合） ────────────────────────────
/// FNV-1a 单步混合。
fn fnv1a(st: &mut u64, bytes: &[u8]) {
    for b in bytes.iter() {
        *st ^= *b as u64;
        *st = st.wrapping_mul(0x0000_0100_0000_01B3);
    }
}

/// 字符串混合（以 0xFF 分隔防拼接歧义）。
fn mix_str(st: &mut u64, s: &str) {
    fnv1a(st, s.as_bytes());
    fnv1a(st, &[0xFF]);
}

/// 一次完整会话指纹：登记表闭集 + 判定语料 + 漂移战报 + 颁行审计。
/// 种子与序列全部写死，双跑必须逐字节一致。
pub fn session_fingerprint() -> u64 {
    let mut st: u64 = 0xC0FF_EE12_7690;
    // 1) 冻结登记表闭集：号/规范名/别名/归属册 逐条混合。
    for t in TERM_TABLE.iter() {
        fnv1a(&mut st, &t.term_no.to_le_bytes());
        mix_str(&mut st, t.canonical);
        for a in t.aliases.iter() {
            mix_str(&mut st, a);
        }
        fnv1a(&mut st, &[t.volume.index()]);
    }
    // 2) 判定语料：每条术语规范名 + 全别名逐条判定，判定码入指纹。
    for t in TERM_TABLE.iter() {
        let v = match judge_term(t.term_no, t.canonical) {
            Ok(()) => 0x5A5Au32,
            Err(e) => e,
        };
        fnv1a(&mut st, &v.to_le_bytes());
        for a in t.aliases.iter() {
            let v2 = match judge_term(t.term_no, a) {
                Ok(()) => 0xA5A5u32,
                Err(e) => e,
            };
            fnv1a(&mut st, &v2.to_le_bytes());
        }
    }
    // 3) 漂移战报：三类抓到/总轮数 + 逃脱串。
    let rep = drift_sweep();
    fnv1a(&mut st, &(rep.name_caught as u64).to_le_bytes());
    fnv1a(&mut st, &(rep.name_total as u64).to_le_bytes());
    fnv1a(&mut st, &(rep.cross_caught as u64).to_le_bytes());
    fnv1a(&mut st, &(rep.cross_total as u64).to_le_bytes());
    fnv1a(&mut st, &(rep.unreg_caught as u64).to_le_bytes());
    fnv1a(&mut st, &(rep.unreg_total as u64).to_le_bytes());
    mix_str(&mut st, &rep.escape);
    // 4) 颁行会话审计：冻结→颁行→全册规范名各用一次，逐条审计混合。
    let mut reg = GlossaryRegistry::new();
    let _ = reg.freeze();
    let _ = reg.enact();
    for t in TERM_TABLE.iter() {
        let _ = reg.use_term(t.term_no, t.canonical, t.volume);
    }
    let mut i = 0usize;
    while i < reg.audit_len() {
        match reg.audit_at(i) {
            Some(r) => {
                fnv1a(&mut st, &r.term_no.to_le_bytes());
                fnv1a(&mut st, &[r.kind.index(), r.at.index()]);
            }
            None => {}
        }
        i += 1;
    }
    fnv1a(&mut st, &(reg.audit_len() as u64).to_le_bytes());
    st
}

// ── 域经十条（判据 J1 直读口：经验/适用面/证据三列齐） ────────────
/// 单条经：经名/经验列/适用面列/证据列（证据码非零）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonTen {
    /// 经名。
    pub jing: &'static str,
    /// 经验列。
    pub exp: &'static str,
    /// 适用面列。
    pub face: &'static str,
    /// 证据列（非零证据码）。
    pub evid: u32,
}

/// 域经十条：术语一致性域的十条经，三列齐逐条可点验（判据 J1 锚定）。
pub const JING_TEN: &[CanonTen] = &[
    CanonTen { jing: "冻结", exp: "登记表一次成账冻结、冻结后不热改", face: "全册 44 条术语", evid: 0xA1 },
    CanonTen { jing: "别名", exp: "用名只认规范名与别名闭集", face: "全部别名闭集", evid: 0xA2 },
    CanonTen { jing: "归属", exp: "每条术语唯一归属册", face: "核心/驱动/网络/外壳四册", evid: 0xA3 },
    CanonTen { jing: "跨册", exp: "跨册使用须先过准入门", face: "四册两两组合", evid: 0xA4 },
    CanonTen { jing: "次序", exp: "未登记→越别名→越权三段判定次序冻结", face: "全部判定路径", evid: 0xA5 },
    CanonTen { jing: "漂移", exp: "注入必抓：名称/跨册/未登记三类", face: "3 类 × 10 轮", evid: 0xA6 },
    CanonTen { jing: "容量", exp: "审计账定容、溢出显性拒不静默", face: "全部审计入账路径", evid: 0xA7 },
    CanonTen { jing: "指纹", exp: "双跑指纹逐字节一致", face: "全会话四段混合", evid: 0xA8 },
    CanonTen { jing: "单调", exp: "编译→冻结→颁行只进不退", face: "状态机全域", evid: 0xA9 },
    CanonTen { jing: "点验", exp: "判据逐条可点验、红项显性", face: "检查项全域", evid: 0xAA },
];

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 段窗 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：全部落窗、互异、查表命中、未知回落。
fn diag_closed_ok() -> bool {
    for d in GX_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if d.detail().is_empty() {
            return false;
        }
    }
    // 两两互异。
    let mut i = 0usize;
    while i < GX_ALL.len() {
        let vi = match GX_ALL.get(i) {
            Some(d) => d.val(),
            None => return false,
        };
        let mut j = i + 1;
        while j < GX_ALL.len() {
            let vj = match GX_ALL.get(j) {
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
    // 查表命中 + 未知回落。
    if detail(GxDiag::ENoent.val()) != GxDiag::ENoent.detail() {
        return false;
    }
    if detail(SEG_BASE + 0x3F) == GxDiag::ENoent.detail() {
        return false;
    }
    true
}

/// 登记表闭集守卫：≥40 条、号连续无洞、别名非空、册位闭集有效、四册闭集自洽。
fn table_closure_ok() -> bool {
    if TERM_TABLE.len() < TERM_MIN {
        return false;
    }
    let mut expect: u32 = 1;
    for t in TERM_TABLE.iter() {
        if t.term_no != expect {
            return false;
        }
        if t.aliases.is_empty() {
            return false;
        }
        if Volume::from_index(t.volume.index()) != Some(t.volume) {
            return false;
        }
        expect = expect.wrapping_add(1);
    }
    // 四册闭集自洽：序号回查命中、中文名非空。
    for v in VOLUME_ALL.iter() {
        if Volume::from_index(v.index()) != Some(*v) || v.name().is_empty() {
            return false;
        }
    }
    if VOLUME_ALL.len() != 4 {
        return false;
    }
    true
}

/// 别名零冲突：别名互异（条内+跨条）、别名不撞他条规范名、规范名互异。
fn alias_conflict_ok() -> bool {
    let n = TERM_TABLE.len();
    let mut i = 0usize;
    while i < n {
        let ti = match TERM_TABLE.get(i) {
            Some(t) => t,
            None => return false,
        };
        // 条内别名互异。
        let al = ti.aliases.len();
        let mut p = 0usize;
        while p < al {
            let mut q = p + 1;
            while q < al {
                match (ti.aliases.get(p), ti.aliases.get(q)) {
                    (Some(x), Some(y)) => {
                        if x == y {
                            return false;
                        }
                    }
                    _ => return false,
                }
                q += 1;
            }
            p += 1;
        }
        // 跨条：规范名互异、别名不撞他条规范名、别名不撞他条别名。
        let mut j = 0usize;
        while j < n {
            if j == i {
                j += 1;
                continue;
            }
            let tj = match TERM_TABLE.get(j) {
                Some(t) => t,
                None => return false,
            };
            if ti.canonical == tj.canonical {
                return false;
            }
            for a in tj.aliases.iter() {
                if *a == ti.canonical {
                    return false;
                }
                for b in ti.aliases.iter() {
                    if a == b {
                        return false;
                    }
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 域经十条齐：10 条、每条 经验/适用面 列非空、证据列非零。
fn ten_rows_ok() -> bool {
    if JING_TEN.len() != 10 {
        return false;
    }
    for c in JING_TEN.iter() {
        if c.jing.is_empty() || c.exp.is_empty() || c.face.is_empty() {
            return false;
        }
        if c.evid == 0 {
            return false;
        }
    }
    true
}

/// 正常流：冻结→颁行→全册规范名+全别名判定全过、审计入账。
fn happy_ok() -> bool {
    let mut reg = GlossaryRegistry::new();
    if reg.freeze().is_err() {
        return false;
    }
    if reg.enact().is_err() {
        return false;
    }
    if reg.state() != GlossaryState::Enacted {
        return false;
    }
    // 规范名直判 + 登记器现场判（归属册）。
    for t in TERM_TABLE.iter() {
        if judge_term(t.term_no, t.canonical).is_err() {
            return false;
        }
        if reg.use_term(t.term_no, t.canonical, t.volume).is_err() {
            return false;
        }
        // 别名闭集逐个别名直判。
        for a in t.aliases.iter() {
            if judge_term(t.term_no, a).is_err() {
                return false;
            }
        }
    }
    // 审计账应记满全册规范名（44 条 < AUDIT_CAP）。
    reg.audit_len() == TERM_TABLE.len()
}

/// 判定次序守卫：未登记 → 越别名 → 越权，前置段命中即短路。
fn order_ok() -> bool {
    // 未登记号 + 越集名 → ENOENT（存在性先短路）。
    if judge_term(9999, "根本不存在的名") != Err(GxDiag::ENoent.val()) {
        return false;
    }
    // 登记号 + 越集名 + 跨册现场 → EINVAL（别名段先于跨册段）。
    let mut reg = GlossaryRegistry::new();
    if reg.use_term(1, "越出闭集的名", Volume::NetVol) != Err(GxDiag::Einval.val()) {
        return false;
    }
    // 登记号 + 规范名 + 跨册无准入 → EPERM（第三段把关）。
    if reg.use_term(1, "页表", Volume::NetVol) != Err(GxDiag::Eperm.val()) {
        return false;
    }
    true
}

/// 跨册准入门：无准入拒（EPERM）、准入后放行、准入入册可查。
fn cross_admission_ok() -> bool {
    let mut reg = GlossaryRegistry::new();
    // 术语 17「设备树」归属驱动册，在核心册直用被拒。
    if reg.use_term(17, "设备树", Volume::CoreVol) != Err(GxDiag::Eperm.val()) {
        return false;
    }
    // 准入后放行，审计册位正确。
    if reg.admit(17, Volume::CoreVol).is_err() {
        return false;
    }
    if reg.use_term(17, "设备树", Volume::CoreVol).is_err() {
        return false;
    }
    match reg.audit_at(0) {
        Some(r) => {
            if r.term_no != 17 || r.at != Volume::CoreVol || r.kind != UsedKind::Canonical {
                return false;
            }
        }
        None => return false,
    }
    // 幂等准入：重复开同一扇门仍 Ok、准入条数不变。
    if reg.admit(17, Volume::CoreVol).is_err() {
        return false;
    }
    if reg.admissions_len() != 1 {
        return false;
    }
    // 别名跨册同样受准入管：别名「DT」准入后在核心册放行。
    if reg.use_term(17, "DT", Volume::CoreVol).is_err() {
        return false;
    }
    match reg.audit_at(1) {
        Some(r) => r.kind == UsedKind::Alias,
        None => false,
    }
}

/// 容量耗尽：审计账满后继续入账显性拒（ECap），账面不变。
fn capacity_ok() -> bool {
    let mut reg = GlossaryRegistry::new();
    let mut n = 0usize;
    while n < AUDIT_CAP {
        if reg.use_term(1, "页表", Volume::CoreVol).is_err() {
            return false;
        }
        n += 1;
    }
    if reg.audit_len() != AUDIT_CAP {
        return false;
    }
    let before = reg.audit_len();
    if reg.use_term(1, "页表", Volume::CoreVol) != Err(GxDiag::ECap.val()) {
        return false;
    }
    reg.audit_len() == before
}

/// 状态机单调：未冻结先颁行拒；冻结/颁行只进不退；颁行后准入封死。
fn state_ok() -> bool {
    let mut reg = GlossaryRegistry::new();
    // 越级颁行：未冻结先颁行。
    if reg.enact() != Err(GxDiag::ENotFrozen.val()) {
        return false;
    }
    // 冻结成功；重冻结倒退。
    if reg.freeze().is_err() {
        return false;
    }
    if reg.freeze() != Err(GxDiag::ERewind.val()) {
        return false;
    }
    // 颁行成功（漂移 3×10 全抓）；重颁行倒退。
    match reg.enact() {
        Ok((caught, total)) => {
            if caught != 30 || total != 30 {
                return false;
            }
        }
        Err(_) => return false,
    }
    if reg.enact() != Err(GxDiag::ERewind.val()) {
        return false;
    }
    // 颁行后准入门封死。
    if reg.admit(1, Volume::NetVol) != Err(GxDiag::EFrozen.val()) {
        return false;
    }
    // 颁行后术语使用照常放行（使用不受写入门影响）。
    reg.use_term(1, "页表", Volume::CoreVol).is_ok()
}

/// 双跑一致：两次完整会话指纹逐字节一致，两次漂移战报全等。
fn dual_ok() -> bool {
    let f1 = session_fingerprint();
    let f2 = session_fingerprint();
    if f1 != f2 {
        return false;
    }
    if drift_sweep() != drift_sweep() {
        return false;
    }
    // 两次颁行会话审计逐条全等。
    let mut r1 = GlossaryRegistry::new();
    let mut r2 = GlossaryRegistry::new();
    if r1.freeze().is_err() || r1.enact().is_err() {
        return false;
    }
    if r2.freeze().is_err() || r2.enact().is_err() {
        return false;
    }
    for t in TERM_TABLE.iter() {
        let _ = r1.use_term(t.term_no, t.canonical, t.volume);
        let _ = r2.use_term(t.term_no, t.canonical, t.volume);
    }
    if r1.audit_len() != r2.audit_len() {
        return false;
    }
    let mut i = 0usize;
    while i < r1.audit_len() {
        match (r1.audit_at(i), r2.audit_at(i)) {
            (Some(a), Some(b)) => {
                if a != b {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12769 自检（判据 12769 卡：域经十条齐，逐条可点验）。
pub fn run_unx12769_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12769");
    let mut all_ok = true;

    // 01 段位守卫。
    let b = seg_guard_ok();
    all_ok = all_ok && b;
    s.add_dyn(
        "F12770-01 段位守卫",
        b,
        &format!(
            "seg=0x{:05x} align40={} in_window={}",
            SEG_BASE,
            SEG_BASE % 0x40 == 0,
            (0x80000..=0x831C0).contains(&SEG_BASE)
        ),
    );

    // 02 诊断码闭集。
    let b = diag_closed_ok();
    all_ok = all_ok && b;
    s.add_dyn(
        "F12770-02 诊断码闭集",
        b,
        &format!(
            "n={} first=0x{:05x} last=0x{:05x}",
            GX_ALL.len(),
            GX_ALL.first().map(|d| d.val()).unwrap_or(0),
            GX_ALL.last().map(|d| d.val()).unwrap_or(0)
        ),
    );

    // 03 登记表闭集（≥40 条冻结闭集）。
    let b = table_closure_ok();
    all_ok = all_ok && b;
    s.add_dyn(
        "F12770-03 登记表闭集",
        b,
        &format!(
            "terms={} min={} 别名总数={}",
            TERM_TABLE.len(),
            TERM_MIN,
            TERM_TABLE.iter().map(|t| t.aliases.len()).sum::<usize>()
        ),
    );

    // 04 别名零冲突。
    let b = alias_conflict_ok();
    all_ok = all_ok && b;
    s.add_dyn("F12770-04 别名零冲突", b, "条内互异/跨条互异/不撞他条规范名 三查全过");

    // 05 域经十条齐（判据 J1 直读口）。
    let b = ten_rows_ok();
    all_ok = all_ok && b;
    s.add_dyn(
        "F12770-05 域经十条齐",
        b,
        &format!("经={} 三列齐={}", JING_TEN.len(), ten_rows_ok()),
    );

    // 06 正常流：规范名+全别名判定全过、审计入账。
    let b = happy_ok();
    all_ok = all_ok && b;
    s.add_dyn(
        "F12770-06 正常流判定",
        b,
        &format!(
            "规范名={} 别名判定=全过",
            TERM_TABLE.len()
        ),
    );

    // 07 判定次序：ENOENT → EINVAL → EPERM 前置短路。
    let b = order_ok();
    all_ok = all_ok && b;
    s.add_dyn("F12770-07 判定次序", b, "未登记先短路/越别名先于越权/越权末段 三点全守");

    // 08 名称漂移注入 10/10。
    let rep = drift_sweep();
    let b = rep.name_total == 10 && rep.name_caught == 10;
    all_ok = all_ok && b;
    s.add_dyn(
        "F12770-08 名称漂移注入",
        b,
        &format!("caught={}/{}", rep.name_caught, rep.name_total),
    );

    // 09 跨册越权注入 10/10。
    let b = rep.cross_total == 10 && rep.cross_caught == 10;
    all_ok = all_ok && b;
    s.add_dyn(
        "F12770-09 跨册越权注入",
        b,
        &format!("caught={}/{}", rep.cross_caught, rep.cross_total),
    );

    // 10 未登记注入 10/10。
    let b = rep.unreg_total == 10 && rep.unreg_caught == 10;
    all_ok = all_ok && b;
    s.add_dyn(
        "F12770-10 未登记注入",
        b,
        &format!("caught={}/{} 逃脱={}", rep.unreg_caught, rep.unreg_total, rep.escape),
    );

    // 11 状态机单调。
    let b = state_ok();
    all_ok = all_ok && b;
    s.add_dyn("F12770-11 状态机单调", b, "越级颁行/重冻结/重颁行/颁行后准入 四点全拒");

    // 12 跨册准入门。
    let b = cross_admission_ok();
    all_ok = all_ok && b;
    s.add_dyn("F12770-12 跨册准入门", b, "无准入拒/准入放行/幂等准入/别名同管 四点全过");

    // 13 容量耗尽。
    let b = capacity_ok();
    all_ok = all_ok && b;
    s.add_dyn(
        "F12770-13 容量耗尽显性拒",
        b,
        &format!("cap={} 溢出=ECap 账面不变", AUDIT_CAP),
    );

    // 14 双跑一致指纹。
    let b = dual_ok();
    all_ok = all_ok && b;
    s.add_dyn(
        "F12770-14 双跑一致指纹",
        b,
        &format!("fp=0x{:016x} 两跑全等", session_fingerprint()),
    );

    // 15 收口：判据条数声明（UNX-F12770-J1 共 1 条，前置检查项全绿）。
    s.add_dyn(
        "F12770-15 收口：判据条数声明",
        all_ok,
        "判据 UNX-F12770-J1 共 1 条（域经十条齐·三列·逐条可点验），前置检查项 14 条全绿",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 段窗。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：13 个码互异、全部落窗、查表命中、未知回落。
        assert_eq!(GX_ALL.len(), 13);
        let mut i = 0usize;
        while i < GX_ALL.len() {
            let vi = GX_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            assert!(!detail(vi).is_empty());
            let mut j = i + 1;
            while j < GX_ALL.len() {
                let vj = GX_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }

    #[test]
    fn term_table_frozen_closure() {
        // 登记表：≥40 条、号连续无洞、别名非空、别名零冲突。
        assert!(TERM_TABLE.len() >= 40);
        assert!(table_closure_ok());
        assert!(alias_conflict_ok());
        // 逐条点验：规范名与每个别名判定全过。
        for t in TERM_TABLE.iter() {
            assert_eq!(judge_term(t.term_no, t.canonical), Ok(()));
            for a in t.aliases.iter() {
                assert_eq!(judge_term(t.term_no, a), Ok(()));
            }
        }
    }

    #[test]
    fn judge_order_precedence() {
        // 次序守卫：未登记 → 越别名 → 越权，前置段短路。
        assert!(order_ok());
        assert_eq!(judge_term(9999, "任意名"), Err(GxDiag::ENoent.val()));
        let mut reg = GlossaryRegistry::new();
        assert_eq!(
            reg.use_term(1, "越出闭集的名", Volume::NetVol),
            Err(GxDiag::Einval.val())
        );
        assert_eq!(reg.use_term(1, "页表", Volume::NetVol), Err(GxDiag::Eperm.val()));
    }

    #[test]
    fn drift_injections_all_caught() {
        // 注入必抓：三类各 10 轮全数命中，无逃脱。
        let rep = drift_sweep();
        assert_eq!(rep.name_total, 10);
        assert_eq!(rep.name_caught, 10);
        assert_eq!(rep.cross_total, 10);
        assert_eq!(rep.cross_caught, 10);
        assert_eq!(rep.unreg_total, 10);
        assert_eq!(rep.unreg_caught, 10);
        assert!(rep.escape.is_empty());
        assert!(rep.all_caught());
    }

    #[test]
    fn dual_run_fingerprint_identical() {
        // 双跑一致：确定性种子下指纹、战报、审计逐项全等。
        assert!(dual_ok());
        assert_eq!(session_fingerprint(), session_fingerprint());
        assert_eq!(drift_sweep(), drift_sweep());
    }

    #[test]
    fn capacity_exhaustion_rejected() {
        // 容量耗尽：账满后溢出显性拒（ECap），账面不变。
        assert!(capacity_ok());
        let mut reg = GlossaryRegistry::new();
        let mut n = 0usize;
        while n < AUDIT_CAP {
            assert!(reg.use_term(1, "页表", Volume::CoreVol).is_ok());
            n += 1;
        }
        assert_eq!(reg.audit_len(), AUDIT_CAP);
        assert_eq!(
            reg.use_term(1, "页表", Volume::CoreVol),
            Err(GxDiag::ECap.val())
        );
        assert_eq!(reg.audit_len(), AUDIT_CAP);
    }

    #[test]
    fn state_machine_monotonic() {
        // 状态机只进不退：编译→冻结→颁行；越级/倒退/封门各点。
        assert!(state_ok());
        let mut reg = GlossaryRegistry::new();
        assert_eq!(reg.enact(), Err(GxDiag::ENotFrozen.val()));
        assert!(reg.freeze().is_ok());
        assert_eq!(reg.state(), GlossaryState::Frozen);
        assert_eq!(reg.freeze(), Err(GxDiag::ERewind.val()));
        assert!(reg.enact().is_ok());
        assert_eq!(reg.state(), GlossaryState::Enacted);
        assert_eq!(reg.enact(), Err(GxDiag::ERewind.val()));
        assert_eq!(reg.admit(1, Volume::NetVol), Err(GxDiag::EFrozen.val()));
    }

    #[test]
    fn cross_volume_admission_gate() {
        // 跨册准入门：无准入拒、准入放行、幂等准入、别名同管。
        assert!(cross_admission_ok());
        let mut reg = GlossaryRegistry::new();
        assert_eq!(
            reg.use_term(17, "设备树", Volume::CoreVol),
            Err(GxDiag::Eperm.val())
        );
        assert!(reg.admit(17, Volume::CoreVol).is_ok());
        assert!(reg.use_term(17, "设备树", Volume::CoreVol).is_ok());
        assert_eq!(reg.audit_at(0).map(|r| r.at), Some(Volume::CoreVol));
        assert!(reg.admit(17, Volume::CoreVol).is_ok());
        assert_eq!(reg.admissions_len(), 1);
    }

    #[test]
    fn boundary_unregistered_and_alias_edges() {
        // 四类边界：零号/越号一位/空用名/最大号。
        assert_eq!(judge_term(0, "页表"), Err(GxDiag::ENoent.val()));
        assert_eq!(judge_term(45, "页表"), Err(GxDiag::ENoent.val()));
        assert_eq!(judge_term(1, ""), Err(GxDiag::Einval.val()));
        assert_eq!(judge_term(44, "提示符"), Ok(()));
        assert_eq!(judge_term(44, "prompt"), Ok(()));
        // 未登记号 + 准入门：准入也救不了未登记术语。
        let mut reg = GlossaryRegistry::new();
        assert_eq!(reg.admit(45, Volume::NetVol), Err(GxDiag::ENoent.val()));
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12769_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
