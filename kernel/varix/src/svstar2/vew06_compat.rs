//! VE-F4606 · 插件版本与兼容（VE-W 域 · 插件 SDK · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4606`
//!
//! **判据（锚点原文五条）**：**语义版本、三维矩阵、双版期、迁移期、判据**。
//!
//! - **语义版本强制**：插件自身版本与它声明的 API 依赖宿主版本，**都**必须严格合
//!   语义版本（`major.minor.patch[-prerelease][+build]`）。严格体现在：段数恰为三、
//!   前导零拒绝（`01.2.3`）、预发布标识符非空且数值标识无前导零、构建元数据不参与
//!   优先级比较。**宽松解析等于没有版本**——`1.02` 被当成 `1.2` 收下，装上以后才炸。
//! - **兼容矩阵**：插件 × 宿主版本 × 平台**三维**矩阵，格数 = `插件数 × 宿主版本数
//!   × 平台数`。每格三态且**互斥完备**（实测兼容 / 实测不兼容 / 缺格）。**缺格不是
//!   兼容**——缺格必须返回"未知"并进补测排程，绝不允许默认放行；这是本条最容易
//!   被做假的地方，也是判据的主要着力点。
//! - **破坏性变更零容忍 → 双版期**：宿主 API 废弃走双版期，插件享有**完整迁移期**。
//!   废弃在宿主版本 `at` 生效时，旧 API 在 `[at, at+DUAL_PERIOD_HOSTS]` 区间内**仍然
//!   存在**；只有越过区间上界，旧 API 才真正消失，此时仍在使用它的插件被**版本拦截**。
//!   "仍然存在"的最后一格（`remaining == 0`）**不算过期**——把边界判成过期就是
//!   提前撕毁迁移期，属于破坏性变更零容忍的反面。
//! - **迁移期完整性**：迁移视图须与废弃登记的使用方**双向集合相等**（不多不少各一次）；
//!   剩余迁移期按宿主版本逐点**算术精确**（`grace_end - current`），不做符号近似。
//!   同时，声明覆盖范围与迁移期**交叉对账**：插件若把 `host_max` 声明到旧 API 已消失
//!   的宿主版本上，就是自相矛盾；此时矩阵若还声称该格兼容，即为**伪造兼容**，判红。
//!
//! **错误路径与降级矩阵**：版本不识别 → 明确报错 + **具体升级指引**（指引须点名那个
//! 非法版本串，不接受"请检查版本"这类空话）；矩阵缺格 → 补测排程（进队列，不放行
//! 也不误判为不兼容）；兼容破坏 → 版本拦截（阻断装载方向，须给出迁移出口）。
//!
//! **性能逐项分解**：语义版本校验 O(版本串长)（**与矩阵规模无关**——每次校验不得
//! 回头重扫宿主表）；矩阵查格 O(键扫描) + O(1) 格访问（**查单格不得扫全表**）；
//! 拦截 O(1)（判据常数比较 + 查一格）；矩阵交叉对账 O(废弃数 × 使用方数 × 平台数)。
//! 三类计数器分别计真实工作量（格访问 / 键扫描 / 版本扫描），不接受自证式算术。
//!
//! **跨批对接点**：上游 F4602 Manifest（身份节 `version`、兼容节 `host_min/host_max`
//! 与平台声明，本条消费其口径：`host_min`/`host_max` 为**闭区间**，越界即不兼容）；
//! 下游 F4607 API 冻结层（本条的废弃登记是其变更评审的输入，双版期承诺须对兑现）；
//! F4624 更新消费（本条拦截结论是更新通道的准入凭据）。
//!
//! **无障碍与隐私**：无独立交互面（兼容判定在装载前的编译期通道完成，不新增可视
//! 交互）；无隐私面（只处理版本号与平台标识，不触及插件作者信息或用户数据）。
//!
//! 逻辑版本序注入，零墙钟；零 IO；类型自持（本条不 import 未注册的兄弟模块——平行
//! 会话的 `ve*` 族尚在施工，编译期硬耦合会让本条因别人的进度而红）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 域标识（CheckSet 聚合用）。
pub const VEW06_DOMAIN: &str = "VE-W";

/// 本条实现版本（跨批对接的自我声明；下游据此判断口径是否同代）。
pub const COMPAT_VERSION: &str = "E06-plugin-compat-v1";

/// 平台数（三维矩阵的第三维；封闭枚举，新增平台须改本常量并补测全格）。
pub const PLATFORM_COUNT: usize = 4;

/// 宿主版本数上界（矩阵第二维的容量）。
pub const MAX_HOST_VERSIONS: usize = 32;

/// 插件数上界（矩阵第一维的容量）。
pub const MAX_PLUGINS: usize = 32;

/// 矩阵总格数上界（`插件数 × 宿主版本数 × 平台数`）。
pub const MAX_CELLS: usize = MAX_PLUGINS * MAX_HOST_VERSIONS * PLATFORM_COUNT;

/// 版本串长度上界（防御性；语义版本串现实长度远小于此）。
pub const MAX_SEMVER_LEN: usize = 64;

/// 单个 API 废弃登记的宿主版本序上界（须落在宿主表内，越界即立案）。
pub const MAX_HOST_ORD: usize = MAX_HOST_VERSIONS;

/// **双版期长度**（以宿主版本数计）：废弃自 `at` 生效起，旧 API 在
/// `at + DUAL_PERIOD_HOSTS` 这一格**仍然存在**，越过该格才消失。
///
/// 不变式：`DUAL_PERIOD_HOSTS >= 1`。取 0 等于"废弃即刻消失"，双版期承诺归零，
/// 破坏性变更零容忍被自己撕毁。
pub const DUAL_PERIOD_HOSTS: u32 = 2;

/// 单条废弃登记的最多使用方数（防御性）。
pub const MAX_API_USERS: usize = MAX_PLUGINS;

/// 语义版本文档（对外口径，写死以免实现与承诺漂移）。
pub const SEMVER_DOC: &str = "\
语义版本口径（VE-F4606 · v1）：版本串形如 major.minor.patch[-prerelease][+build]，段数恰为三\
且逐段为无前导零十进制数；预发布标识符以点分隔、逐个非空，其中的纯数值标识同样不得\
前导零；构建元数据以 '+' 起、只含字母数字与连字符点，且**不参与优先级比较**\
——1.0.0+build 与 1.0.0 优先级相同。预发布版本优先级**低于**同数字的正式版\
（1.0.0-alpha < 1.0.0）。不合此口径者不识别，明确报错并给出升级指引。";

/// 三维兼容矩阵文档。
pub const MATRIX_DOC: &str = "\
三维兼容矩阵口径（VE-F4606 · v1）：矩阵三轴为插件 × 宿主版本 × 平台，每格三态互斥完备\
——实测兼容、实测不兼容、缺格。**缺格不是兼容**：缺格返回未知并进补测排程，既不放行\
也不误判为不兼容。同一格重复实测得出不同结论即为实测冲突，显性拒绝，不得后者覆盖前者\
——能被静默覆盖的矩阵等于没有矩阵。";

/// 双版期文档（破坏性变更零容忍的技术面）。
pub const DUAL_PERIOD_DOC: &str = "\
双版期承诺（VE-F4606 · v1）：宿主 API 废弃自宿主版本 at 生效，旧 API 在闭区间\
[at, at+2] 内仍然存在，插件享有完整迁移期；只有宿主版本越过 at+2 这一格，旧 API 才\
消失，此时仍在使用它的插件被版本拦截并给出迁移出口。区间上界那一格**仍属迁移期内**\
（剩余迁移期为 0 但未过期）——把边界判成过期等于提前撕毁承诺，与破坏性变更零容忍相悖。";

// ---------------------------------------------------------------------------
// 二、诊断（三要素：现象 / 归因 / 处置）
// ---------------------------------------------------------------------------

/// 诊断码（`wire()` 与判别值解耦，前缀 `E06_`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompatCode {
    /// 版本串为空或结构不合语义版本。
    SemVerMalformed = 1,
    /// 版本串含前导零。
    SemVerLeadingZero = 2,
    /// 宿主版本区间倒置（min > max）。
    HostRangeInverted = 3,
    /// 引用了宿主表中不存在的版本。
    HostUnknown = 4,
    /// 引用了插件表中不存在的插件。
    PluginUnknown = 5,
    /// 同一格两次实测结论冲突。
    CellConflict = 6,
    /// 试图把"缺格"当作实测结论写入。
    CellUnknownForbidden = 7,
    /// 矩阵缺格（须补测排程）。
    CellMissing = 8,
    /// 宿主版本不兼容（版本拦截）。
    HostIncompatible = 9,
    /// 平台不兼容（版本拦截）。
    PlatformUnsupported = 10,
    /// 双版期已过仍使用废弃 API（版本拦截）。
    MigrationExpired = 11,
    /// 废弃登记的宿主版本序越界。
    DeprecationOutOfRange = 12,
    /// 插件声明范围覆盖到旧 API 已消失的宿主版本（自相矛盾）。
    ScopeContradiction = 13,
    /// 插件声明范围整体落在旧 API 已消失的区间（既已破损）。
    ScopeAlreadyBroken = 14,
    /// 矩阵声称兼容，但该格落在旧 API 已消失的宿主版本上（伪造兼容）。
    FabricatedCompat = 15,
    /// 使用方数越界。
    TooManyUsers = 16,
}

impl CompatCode {
    /// 稳定诊断码（对外字符串；判别值改动不影响线上码）。
    pub fn wire(self) -> &'static str {
        match self {
            CompatCode::SemVerMalformed => "E06_SEMVER_MALFORMED",
            CompatCode::SemVerLeadingZero => "E06_SEMVER_LEADING_ZERO",
            CompatCode::HostRangeInverted => "E06_HOST_RANGE_INVERTED",
            CompatCode::HostUnknown => "E06_HOST_UNKNOWN",
            CompatCode::PluginUnknown => "E06_PLUGIN_UNKNOWN",
            CompatCode::CellConflict => "E06_CELL_CONFLICT",
            CompatCode::CellUnknownForbidden => "E06_CELL_UNKNOWN_FORBIDDEN",
            CompatCode::CellMissing => "E06_CELL_MISSING",
            CompatCode::HostIncompatible => "E06_HOST_INCOMPATIBLE",
            CompatCode::PlatformUnsupported => "E06_PLATFORM_UNSUPPORTED",
            CompatCode::MigrationExpired => "E06_MIGRATION_EXPIRED",
            CompatCode::DeprecationOutOfRange => "E06_DEPRECATION_OUT_OF_RANGE",
            CompatCode::ScopeContradiction => "E06_SCOPE_CONTRADICTION",
            CompatCode::ScopeAlreadyBroken => "E06_SCOPE_ALREADY_BROKEN",
            CompatCode::FabricatedCompat => "E06_FABRICATED_COMPAT",
            CompatCode::TooManyUsers => "E06_TOO_MANY_USERS",
        }
    }
}

/// 一条诊断（三要素齐全才允许入账；缺任一要素降级为 `IncompleteDiag`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompatDiag {
    /// 诊断码。
    pub code: CompatCode,
    /// 现象：发生了什么，含具体位置与取值。
    pub what: String,
    /// 归因：为什么会这样。
    pub why: String,
    /// 处置：具体到可执行的升级/迁移动作。
    pub fix: String,
}

impl CompatDiag {
    /// 读屏播报文本（三要素顺序固定，便于逐条对拍）。
    pub fn spoken(&self) -> String {
        format!(
            "{}；{}；{}",
            self.what,
            self.why,
            self.fix
        )
    }

    /// 处置是否点名了具体版本串/插件（拒绝"请检查版本"这类空话）。
    ///
    /// 这是"明确报错 + 升级指引"的可机检口径：指引里必须出现它所处置的那个
    /// 具体标识，否则用户无从知道该改哪一个。
    pub fn fix_names(&self, needle: &str) -> bool {
        !needle.is_empty() && self.fix.contains(needle)
    }
}

/// 三要素不全的诊断降级载体（缺一即降级，不许半条诊断蒙混过关）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncompleteDiag {
    /// 降级前的诊断。
    pub diag: CompatDiag,
    /// 缺失要素名（`what` / `why` / `fix`）。
    pub missing: &'static str,
}

/// 诊断账本（三要素完整性在此统一把关）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiagBag {
    /// 合格诊断。
    pub accepted: Vec<CompatDiag>,
    /// 降级诊断。
    pub downgraded: Vec<IncompleteDiag>,
}

impl DiagBag {
    /// 新账本。
    pub fn new() -> Self {
        DiagBag { accepted: Vec::new(), downgraded: Vec::new() }
    }

    /// 入账一条诊断；三要素缺一即降级。
    pub fn push(&mut self, d: CompatDiag) -> bool {
        let missing = if d.what.is_empty() {
            Some("what")
        } else if d.why.is_empty() {
            Some("why")
        } else if d.fix.is_empty() {
            Some("fix")
        } else {
            None
        };
        match missing {
            Some(m) => {
                self.downgraded.push(IncompleteDiag { diag: d, missing: m });
                false
            }
            None => {
                self.accepted.push(d);
                true
            }
        }
    }

    /// 是否存在指定诊断码。
    pub fn has(&self, code: CompatCode) -> bool {
        self.accepted.iter().any(|d| d.code == code)
    }

    /// 去重入账：四要素完全相同即视为同一事实，不重复入账。
    ///
    /// 账本是**事实**的集合，不是事件的流水——同一个矛盾被审计两次仍是同一个
    /// 矛盾，登记两条会让"矛盾条数"随审计次数虚增，把一条问题放大成一片。
    pub fn push_once(&mut self, d: CompatDiag) -> bool {
        let dup = self.accepted.iter().any(|x| {
            x.code == d.code && x.what == d.what && x.why == d.why && x.fix == d.fix
        });
        if dup {
            return false;
        }
        self.push(d)
    }

    /// 首个诊断码（无则空串）。
    pub fn first_code(&self) -> &'static str {
        self.accepted.first().map(|d| d.code.wire()).unwrap_or("")
    }

    /// 全部诊断。
    pub fn all(&self) -> &[CompatDiag] {
        &self.accepted
    }
}

// ---------------------------------------------------------------------------
// 三、语义版本
// ---------------------------------------------------------------------------

/// 预发布标识符（数值标识优先级低于字母数字标识）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreId {
    /// 纯数值标识（不得有前导零）。
    Num(u32),
    /// 字母数字标识。
    Text(String),
}

impl PreId {
    /// 是否数值标识。
    pub fn is_numeric(&self) -> bool {
        matches!(self, PreId::Num(_))
    }

    /// 排序档位（数值在前）。
    fn rank(&self) -> u8 {
        if self.is_numeric() {
            0
        } else {
            1
        }
    }
}

impl Ord for PreId {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (PreId::Num(a), PreId::Num(b)) => a.cmp(b),
            (PreId::Text(a), PreId::Text(b)) => a.as_bytes().cmp(b.as_bytes()),
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

impl PartialOrd for PreId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// 预发布标识符序列比较（前缀相同时长者优先级更高）。
fn cmp_pre(a: &[PreId], b: &[PreId]) -> Ordering {
    let n = a.len().min(b.len());
    for i in 0..n {
        let ord = a[i].cmp(&b[i]);
        if ord != Ordering::Equal {
            return ord;
        }
    }
    a.len().cmp(&b.len())
}

/// 语义版本号。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemVer {
    /// 主版本号。
    pub major: u32,
    /// 次版本号。
    pub minor: u32,
    /// 修订号。
    pub patch: u32,
    /// 预发布标识符序列（空 = 正式版；故此处是 `Vec` 而非 `Option`——
    /// 空序列本身就表达「无预发布」，多套一层 `Option` 只会造出
    /// `Some(vec![])` 这种既非正式版也非预发布版的第三态）。
    pub pre: Vec<PreId>,
    /// 构建元数据（**不参与**优先级比较）。
    pub build: Option<String>,
}

impl SemVer {
    /// 构造正式版。
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        SemVer { major, minor, patch, pre: Vec::new(), build: None }
    }

    /// 规范串形式（构建元数据附在末尾）。
    pub fn to_canonical(&self) -> String {
        let mut s = format!("{}.{}.{}", self.major, self.minor, self.patch);
        if !self.pre.is_empty() {
            s.push('-');
            for (i, p) in self.pre.iter().enumerate() {
                if i > 0 {
                    s.push('.');
                }
                match p {
                    PreId::Num(n) => s.push_str(&n.to_string()),
                    PreId::Text(t) => s.push_str(t),
                }
            }
        }
        if let Some(b) = &self.build {
            s.push('+');
            s.push_str(b);
        }
        s
    }

    /// 是否为预发布版。
    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => cmp_pre(&self.pre, &other.pre),
            })
    }
}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// 语义版本解析。
///
/// 严格性口径（与 `SEMVER_DOC` 逐条对应）：段数恰三、逐段无前导零、预发布标识符
/// 逐个非空且数值标识无前导零、构建元数据只含字母数字与连字符点。
///
/// 复杂度 O(版本串长)：只扫给定串一次，**不回扫宿主表**（版本扫描计数器即计此处）。
pub fn parse_semver(src: &str) -> Result<SemVer, CompatDiag> {
    if src.is_empty() {
        return Err(CompatDiag {
            code: CompatCode::SemVerMalformed,
            what: "版本串为空".to_string(),
            why: "空版本无法参与任何比较：既判不出新旧，也算不出迁移剩余".to_string(),
            fix: format!("把空版本串改写为合规语义版本，例如填入 0.1.0（当前值 \"\"）"),
        });
    }
    if src.len() > MAX_SEMVER_LEN {
        return Err(CompatDiag {
            code: CompatCode::SemVerMalformed,
            what: format!("版本串长度 {} 超上界 {}", src.len(), MAX_SEMVER_LEN),
            why: "超长版本串不是语义版本，解析它只会得到被截断的假象".to_string(),
            fix: format!("把 {} 压缩到 {} 字节以内的 major.minor.patch 形式", src, MAX_SEMVER_LEN),
        });
    }

    // '+' 只允许出现一次，且必须在预发布之后。
    let plus = src.find('+');
    let main = match plus {
        Some(i) => &src[..i],
        None => src,
    };
    let build = match plus {
        Some(i) => {
            let b = &src[i + 1..];
            if b.is_empty() {
                return Err(CompatDiag {
                    code: CompatCode::SemVerMalformed,
                    what: format!("版本串 \"{}\" 的构建元数据为空", src),
                    why: "'+' 后必须跟至少一个标识符，空元数据是分隔符写残了".to_string(),
                    fix: format!("补齐构建元数据或删掉 '+'：把 {} 改成 major.minor.patch", src),
                });
            }
            if b.contains('+') {
                return Err(CompatDiag {
                    code: CompatCode::SemVerMalformed,
                    what: format!("版本串 \"{}\" 含多个 '+'", src),
                    why: "构建元数据只允许一段，第二个 '+' 使版本串无法唯一解析".to_string(),
                    fix: format!("把 {} 改为单个 '+' 分隔的构建元数据", src),
                });
            }
            if !b.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'.') {
                return Err(CompatDiag {
                    code: CompatCode::SemVerMalformed,
                    what: format!("版本串 \"{}\" 的构建元数据含非法字符", src),
                    why: "构建元数据只允许字母数字、连字符与点".to_string(),
                    fix: format!("把 {} 的 '+' 之后部分改为字母数字或连字符点", src),
                });
            }
            Some(b.to_string())
        }
        None => None,
    };

    // '-' 之前的为核心段；核心段无 '-'，故首个 '-' 即预发布分隔符。
    let dash = main.find('-');
    let (core, pre_src) = match dash {
        Some(i) => (&main[..i], Some(&main[i + 1..])),
        None => (main, None),
    };

    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return Err(CompatDiag {
            code: CompatCode::SemVerMalformed,
            what: format!("版本核心段数 {} 不等于 3（来自 \"{}\"）", parts.len(), src),
            why: "语义版本的核心段恰为 major.minor.patch 三段；两段会被当成缺省补零，\
                  四段会被静默截断，两种宽松都会让不同写法撞成同一版本"
                .to_string(),
            fix: format!("把 {} 改写为三段式 major.minor.patch", src),
        });
    }
    let mut nums: [u32; 3] = [0, 0, 0];
    // 逐段写入用 `get_mut` 而非 `nums[i]`：段数校验就在上一行，正常路径下
    // `nums[i]` 永远安全，但**校验与写入之间不该靠"上一行挡住了"来保证**——
    // 一旦有人放宽段数规则或调整校验位置，`nums[i]` 立刻变成 panic 面，
    // 而 panic 会把这一轮所有其余判据的证据一起吞掉。取不到槽位就显性报错。
    for (i, p) in parts.iter().enumerate() {
        match nums.get_mut(i) {
            Some(slot) => *slot = parse_numeric_field(p, src)?,
            None => {
                return Err(CompatDiag {
                    code: CompatCode::SemVerMalformed,
                    what: format!(
                        "版本串 \"{}\" 的核心段数 {} 超出三段上限",
                        src,
                        parts.len()
                    ),
                    why: "语义版本核心段恰为三段；段数闸与写入槽位必须同时成立，\
                          否则多出来的段会无处安放"
                        .to_string(),
                    fix: format!("把 {} 改写为三段式 major.minor.patch", src),
                });
            }
        }
    }

    let mut pre: Vec<PreId> = Vec::new();
    if let Some(ps) = pre_src {
        if ps.is_empty() {
            return Err(CompatDiag {
                code: CompatCode::SemVerMalformed,
                what: format!("版本串 \"{}\" 的预发布段为空", src),
                why: "'-' 后必须跟至少一个标识符，空预发布段是分隔符写残了".to_string(),
                fix: format!("补齐预发布标识或删掉 '-'：把 {} 改成 major.minor.patch", src),
            });
        }
        for id in ps.split('.') {
            if id.is_empty() {
                return Err(CompatDiag {
                    code: CompatCode::SemVerMalformed,
                    what: format!("版本串 \"{}\" 的预发布标识符为空", src),
                    why: "预发布标识符以点分隔，逐个必须非空；空标识符无法参与优先级比较"
                        .to_string(),
                    fix: format!("把 {} 的预发布段改为点分隔的非空标识符", src),
                });
            }
            if id.bytes().all(|c| c.is_ascii_digit()) {
                let n = parse_numeric_field(id, src)?;
                pre.push(PreId::Num(n));
            } else {
                if !id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
                {
                    return Err(CompatDiag {
                        code: CompatCode::SemVerMalformed,
                        what: format!("版本串 \"{}\" 的预发布标识符 \"{}\" 含非法字符", src, id),
                        why: "预发布标识符只允许字母数字与连字符".to_string(),
                        fix: format!("把 {} 的标识符 {} 改为字母数字或连字符", src, id),
                    });
                }
                pre.push(PreId::Text(id.to_string()));
            }
        }
    }

    Ok(SemVer { major: nums[0], minor: nums[1], patch: nums[2], pre, build })
}

/// 解析一个十进制字段（拒绝前导零、空串、非数字与溢出）。
fn parse_numeric_field(field: &str, src: &str) -> Result<u32, CompatDiag> {
    if field.is_empty() {
        return Err(CompatDiag {
            code: CompatCode::SemVerMalformed,
            what: format!("版本串 \"{}\" 含空的数值段", src),
            why: "空数值段无法确定取值，补零会把写法差异抹平成同版本".to_string(),
            fix: format!("把 {} 中的空段补成十进制数字", src),
        });
    }
    if !field.bytes().all(|c| c.is_ascii_digit()) {
        return Err(CompatDiag {
            code: CompatCode::SemVerMalformed,
            what: format!("版本串 \"{}\" 的段 \"{}\" 不是十进制数", src, field),
            why: "语义版本的每段必须是十进制整数；混入非数字字符无法唯一确定取值"
                .to_string(),
            fix: format!("把 {} 的段 {} 改为十进制整数", src, field),
        });
    }
    if field.len() > 1 && field.starts_with('0') {
        return Err(CompatDiag {
            code: CompatCode::SemVerLeadingZero,
            what: format!("版本串 \"{}\" 的段 \"{}\" 带前导零", src, field),
            why: "前导零让同一数值有两种写法，缓存键与比较结果会分叉".to_string(),
            fix: format!("去掉 {} 中 {} 的前导零", src, field),
        });
    }
    match field.parse::<u32>() {
        Ok(v) => Ok(v),
        Err(_) => Err(CompatDiag {
            code: CompatCode::SemVerMalformed,
            what: format!("版本串 \"{}\" 的段 \"{}\" 超出 u32 范围", src, field),
            why: "版本段取值须落在 u32 内，越界值无法参与比较".to_string(),
            fix: format!("把 {} 的段 {} 缩到 4294967295 以内", src, field),
        }),
    }
}

/// 宿主版本闭区间（消费 F4602 Manifest 兼容节口径：`host_min`/`host_max` 闭区间）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostRange {
    /// 下界（含）。
    pub min: SemVer,
    /// 上界（含）。
    pub max: SemVer,
}

impl HostRange {
    /// 构造区间并校验不倒置。
    pub fn new(min: SemVer, max: SemVer) -> Result<Self, CompatDiag> {
        if min > max {
            return Err(CompatDiag {
                code: CompatCode::HostRangeInverted,
                what: format!(
                    "宿主版本区间倒置：min={} 大于 max={}",
                    min.to_canonical(),
                    max.to_canonical()
                ),
                why: "倒置区间在闭区间语义下为空集，等于声明「任何宿主版本都不支持」，\
                      而作者本意通常是漏填了一端"
                    .to_string(),
                fix: format!(
                    "把区间下界改为不大于 {} 的版本（如 0.1.0），或把上界改为不小于 {} 的版本",
                    max.to_canonical(),
                    min.to_canonical()
                ),
            });
        }
        Ok(HostRange { min, max })
    }

    /// 闭区间判定（含两端）。
    pub fn contains(&self, v: &SemVer) -> bool {
        v >= &self.min && v <= &self.max
    }
}

// ---------------------------------------------------------------------------
// 四、三维兼容矩阵
// ---------------------------------------------------------------------------

/// 平台（封闭枚举；新增平台须改 `PLATFORM_COUNT` 并补测全格）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Platform {
    /// 桌面 Windows。
    Windows = 0,
    /// 桌面 Linux。
    Linux = 1,
    /// 桌面 macOS。
    Macos = 2,
    /// 浏览器 WebAssembly。
    Wasm = 3,
}

impl Platform {
    /// 平台序（0..3）。
    pub fn ordinal(self) -> usize {
        self as usize
    }

    /// 稳定短名（Manifest 与矩阵的对拍键）。
    pub fn tag(self) -> &'static str {
        match self {
            Platform::Windows => "windows",
            Platform::Linux => "linux",
            Platform::Macos => "macos",
            Platform::Wasm => "wasm",
        }
    }

    /// 平台中文名（诊断用）。
    pub fn label(self) -> &'static str {
        match self {
            Platform::Windows => "Windows",
            Platform::Linux => "Linux",
            Platform::Macos => "macOS",
            Platform::Wasm => "WASM",
        }
    }

    /// 四平台穷举（按平台序）。
    pub fn all() -> [Platform; PLATFORM_COUNT] {
        [
            Platform::Windows,
            Platform::Linux,
            Platform::Macos,
            Platform::Wasm,
        ]
    }

    /// 按短名解析（不识别即 `None`，不静默回落到首平台）。
    pub fn parse(tag: &str) -> Option<Platform> {
        Platform::all().into_iter().find(|p| p.tag() == tag)
    }
}

/// 格的实测状态（三态互斥完备）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellState {
    /// 缺格（未实测）——**不是兼容**。
    Unknown = 0,
    /// 实测兼容。
    Compatible = 1,
    /// 实测不兼容。
    Incompatible = 2,
}

impl CellState {
    /// 是否已实测（非缺格）。
    pub fn is_measured(self) -> bool {
        self != CellState::Unknown
    }
}

/// 查格结论（与三态一一对应）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 实测兼容，可放行。
    Compatible = 0,
    /// 实测不兼容，版本拦截。
    Incompatible = 1,
    /// 缺格，未知——进补测排程，既不放行也不误判为不兼容。
    Unknown = 2,
}

impl Verdict {
    /// 是否为实测结论。
    pub fn is_measured(self) -> bool {
        self != Verdict::Unknown
    }

    /// 是否放行。
    pub fn admits(self) -> bool {
        self == Verdict::Compatible
    }
}

/// 准入闸结论（拦截方向须明确）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// 准入。
    Admit = 0,
    /// 版本拦截（兼容破坏）。
    Blocked = 1,
    /// 暂缓（缺格，待补测）——不是拦截，也不是准入。
    Held = 2,
}

impl Gate {
    /// 是否放行（仅 `Admit` 放行；`Held` 是暂缓，不是放行）。
    pub fn admits(self) -> bool {
        self == Gate::Admit
    }
}

/// 一格的定位（三维坐标的可读形式）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellRef {
    /// 插件 ID。
    pub plugin: String,
    /// 宿主版本序。
    pub host_ord: u32,
    /// 宿主版本规范串（读屏播报用）。
    pub host: String,
    /// 平台。
    pub platform: Platform,
}

/// 补测排程项（缺格的下游去处）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleItem {
    /// 待补测格。
    pub cell: CellRef,
    /// 排程序号（按登记先后）。
    pub seq: u32,
}

/// 查格结果（结论 + 真实格访问次数）。
///
/// 只带 `cell_reads` 不带 `key_scans`：键扫描是矩阵的**累计**计数，一次 probe 里
/// 分两段累加，切成"本次值"反而要引入增量口径；而格访问在 probe 内是一次性的，
/// 取差值即本次数。两个计数的口径不同，所以只把口径一致的那个放进返回值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    /// 结论。
    pub verdict: Verdict,
    /// 本次查格的格访问次数（真实计数器，非推算）。
    pub cell_reads: u32,
}

/// 三类真实工作量计数器（分别计格访问 / 键扫描 / 版本扫描）。
///
/// 分离计数而非合成一个数，是因为三类工作的复杂度不同：格访问应恒为 O(1)，
/// 键扫描随表规模线性，版本扫描应与表规模**无关**。合成一个总数就看不出
/// "查单格却扫了全表"这类退化。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    /// 格访问次数。
    pub cell_reads: u64,
    /// 键扫描次数。
    pub key_scans: u64,
    /// 版本串扫描次数。
    pub version_scans: u64,
}

/// 三维兼容矩阵。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompatMatrix {
    /// 宿主版本表（升序；矩阵第二维的序来源）。
    pub hosts: Vec<SemVer>,
    /// 插件 ID 表。
    pub plugins: Vec<String>,
    /// 格状态（行主序：`插件序 * (host_len * PLATFORM_COUNT) + host_ord * PLATFORM_COUNT + plat_ord`）。
    pub cells: Vec<CellState>,
    /// 补测排程队列。
    pub schedule: Vec<ScheduleItem>,
    /// 工作量计数。
    pub work: Work,
}

impl CompatMatrix {
    /// 建矩阵：宿主版本表 + 插件表。
    pub fn new(hosts: Vec<SemVer>, plugins: Vec<String>) -> Result<Self, CompatDiag> {
        if hosts.is_empty() {
            return Err(CompatDiag {
                code: CompatCode::HostUnknown,
                what: "宿主版本表为空".to_string(),
                why: "矩阵第二维为空则三维矩阵退化成两维，兼容判定失去宿主轴".to_string(),
                fix: "至少登记一个宿主版本（如 1.0.0）再建矩阵".to_string(),
            });
        }
        if plugins.is_empty() {
            return Err(CompatDiag {
                code: CompatCode::PluginUnknown,
                what: "插件表为空".to_string(),
                why: "矩阵第一维为空则无格可测，兼容判定失去插件轴".to_string(),
                fix: "至少登记一个插件 ID 后再建矩阵".to_string(),
            });
        }
        if hosts.len() > MAX_HOST_VERSIONS {
            return Err(CompatDiag {
                code: CompatCode::HostUnknown,
                what: format!("宿主版本数 {} 超上界 {}", hosts.len(), MAX_HOST_VERSIONS),
                why: "矩阵容量有界，超界会在定长分配时溢出".to_string(),
                fix: format!("把宿主版本数压到 {} 以内", MAX_HOST_VERSIONS),
            });
        }
        if plugins.len() > MAX_PLUGINS {
            return Err(CompatDiag {
                code: CompatCode::PluginUnknown,
                what: format!("插件数 {} 超上界 {}", plugins.len(), MAX_PLUGINS),
                why: "矩阵容量有界，超界会在定长分配时溢出".to_string(),
                fix: format!("把插件数压到 {} 以内", MAX_PLUGINS),
            });
        }
        // 宿主版本表须严格升序：序即矩阵索引，乱序会让同版本对应两格。
        for i in 1..hosts.len() {
            if hosts[i - 1] >= hosts[i] {
                return Err(CompatDiag {
                    code: CompatCode::HostUnknown,
                    what: format!(
                        "宿主版本表非严格升序：第 {} 项 {} 不小于第 {} 项 {}",
                        i,
                        hosts[i - 1].to_canonical(),
                        i - 1,
                        hosts[i].to_canonical()
                    ),
                    why: "矩阵以序索引宿主版本；乱序或重复会让同一版本落到两格，判定自相矛盾"
                        .to_string(),
                    fix: format!(
                        "把宿主版本表按升序去重后重建（当前 {} 早于或等于 {}）",
                        hosts[i - 1].to_canonical(),
                        hosts[i].to_canonical()
                    ),
                });
            }
        }
        let total = plugins.len() * hosts.len() * PLATFORM_COUNT;
        Ok(CompatMatrix {
            hosts,
            plugins,
            cells: vec![CellState::Unknown; total],
            schedule: Vec::new(),
            work: Work::default(),
        })
    }

    /// 总格数。
    pub fn cell_count(&self) -> usize {
        self.plugins.len() * self.hosts.len() * PLATFORM_COUNT
    }

    /// 每行格数（`宿主版本数 × 平台数`）。
    pub fn row_width(&self) -> usize {
        self.hosts.len() * PLATFORM_COUNT
    }

    /// 插件序（线性扫描，计入 `key_scans`）。
    fn plugin_ord(&mut self, plugin: &str) -> Option<usize> {
        let mut scans = 0u64;
        for (i, p) in self.plugins.iter().enumerate() {
            scans += 1;
            if p == plugin {
                self.work.key_scans += scans;
                return Some(i);
            }
        }
        self.work.key_scans += scans;
        None
    }

    /// 宿主版本序（线性扫描，计入 `key_scans`）。
    fn host_ord(&mut self, host: &SemVer) -> Option<usize> {
        let mut scans = 0u64;
        for (i, h) in self.hosts.iter().enumerate() {
            scans += 1;
            if h == host {
                self.work.key_scans += scans;
                return Some(i);
            }
        }
        self.work.key_scans += scans;
        None
    }

    /// 格下标（行主序）。
    fn index(&self, p: usize, h: usize, plat: usize) -> usize {
        p * self.row_width() + h * PLATFORM_COUNT + plat
    }

    /// 记录一次实测结论。
    ///
    /// 三条硬规矩：① 不接受把"缺格"当实测写入（那会让缺格伪装成兼容）；
    /// ② 同格重复实测**结论相同**为幂等（重跑测试不冲突）；③ 结论不同即实测冲突，
    /// 显性拒绝，**不得后者覆盖前者**——能被静默覆盖的矩阵等于没有矩阵。
    pub fn record(
        &mut self,
        plugin: &str,
        host: &SemVer,
        platform: Platform,
        state: CellState,
    ) -> Result<(), CompatDiag> {
        if state == CellState::Unknown {
            return Err(CompatDiag {
                code: CompatCode::CellUnknownForbidden,
                what: format!(
                    "拒绝把缺格写成 {} / {} / {} 的实测结论",
                    plugin,
                    host.to_canonical(),
                    platform.tag()
                ),
                why: "缺格是「未实测」，不是实测结果；把它写进矩阵会让缺格伪装成有结论"
                    .to_string(),
                fix: format!(
                    "对 {} / {} / {} 先跑实测，再把结论（兼容或不兼容）写入",
                    plugin,
                    host.to_canonical(),
                    platform.tag()
                ),
            });
        }
        let p = match self.plugin_ord(plugin) {
            Some(i) => i,
            None => {
                return Err(CompatDiag {
                    code: CompatCode::PluginUnknown,
                    what: format!("插件 {} 不在插件表内", plugin),
                    why: "对不存在的插件写实测，等于凭空造出一格".to_string(),
                    fix: format!("先把 {} 登记进插件表，或改用已登记的插件 ID", plugin),
                })
            }
        };
        let h = match self.host_ord(host) {
            Some(i) => i,
            None => {
                return Err(CompatDiag {
                    code: CompatCode::HostUnknown,
                    what: format!("宿主版本 {} 不在宿主版本表内", host.to_canonical()),
                    why: "对不在表内的宿主版本写实测，矩阵第二维会出现表外坐标".to_string(),
                    fix: format!(
                        "先把宿主版本 {} 登记进宿主版本表（须保持严格升序）",
                        host.to_canonical()
                    ),
                })
            }
        };
        let idx = self.index(p, h, platform.ordinal());
        self.work.cell_reads += 1;
        let prev = self.cells[idx];
        if prev.is_measured() && prev != state {
            return Err(CompatDiag {
                code: CompatCode::CellConflict,
                what: format!(
                    "实测冲突：{} / {} / {} 已记为 {:?}，现又写入 {:?}",
                    plugin,
                    host.to_canonical(),
                    platform.tag(),
                    prev,
                    state
                ),
                why: "同格两次实测结论不同，必有一方失真；静默取后者会让矩阵失去证据效力"
                    .to_string(),
                fix: format!(
                    "复跑 {} / {} / {} 的实测定论；若旧结论作废须走矩阵重测流程，不得直接覆盖",
                    plugin,
                    host.to_canonical(),
                    platform.tag()
                ),
            });
        }
        self.cells[idx] = state;
        Ok(())
    }

    /// 查格结论（**缺格返回 `Unknown`，绝不默认兼容**）。
    pub fn probe(&mut self, plugin: &str, host: &SemVer, platform: Platform) -> Probe {
        let before_cell = self.work.cell_reads;
        let p = match self.plugin_ord(plugin) {
            Some(i) => i,
            None => {
                return Probe {
                    verdict: Verdict::Unknown,
                    cell_reads: (self.work.cell_reads - before_cell) as u32,
                }
            }
        };
        let h = match self.host_ord(host) {
            Some(i) => i,
            None => {
                return Probe {
                    verdict: Verdict::Unknown,
                    cell_reads: (self.work.cell_reads - before_cell) as u32,
                }
            }
        };
        let idx = self.index(p, h, platform.ordinal());
        self.work.cell_reads += 1;
        let verdict = match self.cells[idx] {
            CellState::Unknown => Verdict::Unknown,
            CellState::Compatible => Verdict::Compatible,
            CellState::Incompatible => Verdict::Incompatible,
        };
        Probe {
            verdict,
            cell_reads: (self.work.cell_reads - before_cell) as u32,
        }
    }

    /// 读格（不计数；只供内部对账与测试支撑使用）。
    fn cell(&self, p: usize, h: usize, plat: usize) -> CellState {
        self.cells[self.index(p, h, plat)]
    }

    /// 覆盖统计（`（已实测格数, 实测兼容, 实测不兼容, 缺格）`）。
    ///
    /// 四数之和恒等于总格数——这是三态互斥完备的机检口径。
    pub fn coverage(&self) -> (usize, usize, usize, usize) {
        let mut measured = 0usize;
        let mut compat = 0usize;
        let mut incompat = 0usize;
        let mut missing = 0usize;
        for c in self.cells.iter() {
            match c {
                CellState::Unknown => missing += 1,
                CellState::Compatible => {
                    measured += 1;
                    compat += 1;
                }
                CellState::Incompatible => {
                    measured += 1;
                    incompat += 1;
                }
            }
        }
        (measured, compat, incompat, missing)
    }

    /// 枚举全部缺格（补测排程的输入；顺序按矩阵行主序，稳定可复现）。
    pub fn missing_cells(&self) -> Vec<CellRef> {
        let mut out = Vec::new();
        for (p, pid) in self.plugins.iter().enumerate() {
            for (h, hv) in self.hosts.iter().enumerate() {
                for plat in Platform::all() {
                    if self.cell(p, h, plat.ordinal()) == CellState::Unknown {
                        out.push(CellRef {
                            plugin: pid.clone(),
                            host_ord: h as u32,
                            host: hv.to_canonical(),
                            platform: plat,
                        });
                    }
                }
            }
        }
        out
    }

    /// 全表清点（三态计数，供交叉对账）。
    pub fn tally(&self) -> (usize, usize, usize) {
        let (measured, compat, incompat, _) = self.coverage();
        (measured, compat, incompat)
    }
}

// ---------------------------------------------------------------------------
// 五、插件声明（消费 F4602 Manifest 口径）
// ---------------------------------------------------------------------------

/// 插件的版本与兼容声明（F4602 Manifest 身份节 + 兼容节的收敛形态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginDecl {
    /// 插件 ID。
    pub id: String,
    /// 插件自身版本（语义版本强制）。
    pub version: SemVer,
    /// 声明支持的宿主版本闭区间。
    pub host_range: HostRange,
    /// 声明支持的平台。
    pub platforms: Vec<Platform>,
}

impl PluginDecl {
    /// 构造声明（版本须先过语义版本校验，故此处只接受已解析的 `SemVer`）。
    pub fn new(id: &str, version: SemVer, host_range: HostRange, platforms: Vec<Platform>) -> Self {
        PluginDecl { id: id.to_string(), version, host_range, platforms }
    }

    /// 是否声明支持某平台。
    pub fn supports(&self, p: Platform) -> bool {
        self.platforms.contains(&p)
    }

    /// 声明是否自洽（平台列表非空且无重复）。
    pub fn coherent(&self) -> bool {
        if self.platforms.is_empty() {
            return false;
        }
        for i in 0..self.platforms.len() {
            for j in (i + 1)..self.platforms.len() {
                if self.platforms[i] == self.platforms[j] {
                    return false;
                }
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 六、双版期与迁移期
// ---------------------------------------------------------------------------

/// 迁移阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationPhase {
    /// 迁移期内（尚有整版期）。
    Grace = 0,
    /// 迁移期最后一格（剩余为 0 但**未过期**——完整迁移期含此格）。
    LastCall = 1,
    /// 双版期已过，旧 API 已消失（版本拦截）。
    Expired = 2,
}

/// 一条废弃登记。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deprecation {
    /// 被废弃的 API 名。
    pub api: String,
    /// 废弃生效的宿主版本序（闭区间下端）。
    pub at_host: u32,
    /// 仍在使用该 API 的插件 ID。
    pub users: Vec<String>,
}

impl Deprecation {
    /// 构造废弃登记。
    pub fn new(api: &str, at_host: u32, users: Vec<String>) -> Self {
        Deprecation { api: api.to_string(), at_host, users }
    }

    /// 双版期上界（**闭**；该宿主版本上旧 API 仍然存在）。
    pub fn grace_end(&self) -> u32 {
        self.at_host + DUAL_PERIOD_HOSTS
    }
}

/// 迁移视图的一行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationRow {
    /// 插件 ID。
    pub plugin: String,
    /// API 名。
    pub api: String,
    /// 剩余迁移期（宿主版本数；过期后为 0）。
    pub remaining_hosts: u32,
    /// 迁移阶段。
    pub phase: MigrationPhase,
}

/// 迁移视图（某宿主版本序上的完整迁移台账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationView {
    /// 所在宿主版本序。
    pub at_host_ord: u32,
    /// 逐条迁移行。
    pub rows: Vec<MigrationRow>,
    /// 其中已过期（须版本拦截）的条数。
    pub expired: usize,
}

/// 生成迁移视图。
///
/// 剩余迁移期按 `grace_end - current_host` **算术精确**计算（过期后归 0），
/// 阶段按 `current < grace_end` / `==` / `>` 三分。边界（`==`）判为
/// `LastCall` 而非 `Expired`——把边界判成过期等于提前撕毁迁移期。
pub fn migration_view(deps: &[Deprecation], current_host_ord: u32) -> MigrationView {
    let mut rows = Vec::new();
    let mut expired = 0usize;
    for d in deps.iter() {
        let end = d.grace_end();
        let (remaining, phase) = if current_host_ord > end {
            expired += 1;
            (0u32, MigrationPhase::Expired)
        } else if current_host_ord == end {
            (0u32, MigrationPhase::LastCall)
        } else {
            // `saturating_sub` 而非裸减：本分支的前提是 `current < end`，裸减在
            // 前提成立时等价于饱和减，而一旦阶段判定被改坏（过期条件放宽/收紧），
            // 裸减立刻变成下溢 panic，把整轮判据证据一起吞掉。饱和减在前提被破坏
            // 时也只是退化成 0，不会带走证据——判据该红就红，不该替实现兜底。
            (end.saturating_sub(current_host_ord), MigrationPhase::Grace)
        };
        for u in d.users.iter() {
            rows.push(MigrationRow {
                plugin: u.clone(),
                api: d.api.clone(),
                remaining_hosts: remaining,
                phase,
            });
        }
    }
    MigrationView { at_host_ord: current_host_ord, rows, expired }
}

// ---------------------------------------------------------------------------
// 七、兼容引擎（版本拦截 + 矩阵 + 双版期 + 交叉对账）
// ---------------------------------------------------------------------------

/// 拦截理由（拦截方向与提示必须明确，不许含糊）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InterceptKind {
    /// 宿主版本不在声明闭区间内（版本拦截）。
    Host = 0,
    /// 平台未声明支持（版本拦截）。
    Platform = 1,
    /// 双版期已过仍使用废弃 API（版本拦截）。
    Expired = 2,
}

/// 准入结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Admission {
    /// 闸结论。
    pub gate: Gate,
    /// 三轴查格结论。
    pub verdict: Verdict,
    /// 拦截理由（未拦截则为空串）。
    pub reason: &'static str,
    /// 迁移阶段（不涉及废弃 API 时为 `Grace`）。
    pub phase: MigrationPhase,
}

/// 兼容引擎：把声明、三维矩阵、双版期与拦截收在一处。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompatEngine {
    /// 三维矩阵。
    pub matrix: CompatMatrix,
    /// 插件声明表（序与矩阵第一维一致）。
    pub decls: Vec<PluginDecl>,
    /// 废弃登记。
    pub deps: Vec<Deprecation>,
    /// 诊断账本。
    pub diags: DiagBag,
}

impl CompatEngine {
    /// 建引擎（声明表须与矩阵插件表逐名对齐，序亦须一致）。
    pub fn new(matrix: CompatMatrix, decls: Vec<PluginDecl>) -> Result<Self, CompatDiag> {
        if decls.len() != matrix.plugins.len() {
            return Err(CompatDiag {
                code: CompatCode::PluginUnknown,
                what: format!(
                    "声明表条数 {} 与矩阵插件表条数 {} 不一致",
                    decls.len(),
                    matrix.plugins.len()
                ),
                why: "两表错位会让矩阵第一维与声明错配，判定结果无法解释".to_string(),
                fix: "使声明表条数与矩阵插件表一致（同序同名）后重建引擎".to_string(),
            });
        }
        for (i, d) in decls.iter().enumerate() {
            if d.id != matrix.plugins[i] {
                return Err(CompatDiag {
                    code: CompatCode::PluginUnknown,
                    what: format!(
                        "声明表第 {} 项 {} 与矩阵第 {} 项 {} 不同名",
                        i,
                        d.id,
                        i,
                        matrix.plugins[i]
                    ),
                    why: "两表同序是矩阵第一维与声明对齐的前提；错名会让查格结论挂到别的插件上"
                        .to_string(),
                    fix: format!(
                        "把声明表第 {} 项改为与矩阵同名的 {}",
                        i, matrix.plugins[i]
                    ),
                });
            }
            if !d.coherent() {
                return Err(CompatDiag {
                    code: CompatCode::PlatformUnsupported,
                    what: format!("插件 {} 的平台声明不自洽（空或重复）", d.id),
                    why: "平台列表为空则三维矩阵第三维对该插件整体失效；重复项会让矩阵格重复计数"
                        .to_string(),
                    fix: format!("为 {} 登记 1..{} 个互不重复的平台", d.id, PLATFORM_COUNT),
                });
            }
        }
        Ok(CompatEngine { matrix, decls, deps: Vec::new(), diags: DiagBag::new() })
    }

    /// 登记一条 API 废弃（越界即立案）。
    pub fn deprecate(&mut self, d: Deprecation) -> bool {
        if d.at_host as usize >= self.matrix.hosts.len() {
            self.diags.push(CompatDiag {
                code: CompatCode::DeprecationOutOfRange,
                what: format!(
                    "废弃登记 {} 的宿主版本序 {} 越界（宿主表 {} 项）",
                    d.api,
                    d.at_host,
                    self.matrix.hosts.len()
                ),
                why: "越界的废弃登记算不出双版期，迁移期计时失去参照".to_string(),
                fix: format!(
                    "把 {} 的 at_host 改到 0..{} 的合法宿主版本序",
                    d.api,
                    self.matrix.hosts.len()
                ),
            });
            return false;
        }
        if d.users.len() > MAX_API_USERS {
            self.diags.push(CompatDiag {
                code: CompatCode::TooManyUsers,
                what: format!("废弃登记 {} 的使用方 {} 项超上界 {}", d.api, d.users.len(), MAX_API_USERS),
                why: "使用方表有界，超界会让迁移视图行数失控".to_string(),
                fix: format!("把 {} 的使用方压到 {} 项以内", d.api, MAX_API_USERS),
            });
            return false;
        }
        for u in d.users.iter() {
            if !self.matrix.plugins.iter().any(|p| p == u) {
                self.diags.push(CompatDiag {
                    code: CompatCode::PluginUnknown,
                    what: format!("废弃登记 {} 的使用方 {} 不在插件表内", d.api, u),
                    why: "对表外插件登记使用方，迁移视图会多出一行无处安放的账".to_string(),
                    fix: format!("把 {} 改为已登记的插件 ID，或先把 {} 登记进插件表", u, u),
                });
                return false;
            }
        }
        self.deps.push(d);
        true
    }

    /// 三轴准入判定。
    ///
    /// 判定次序（先声明后矩阵，是有意的）：声明层已判否的（宿主越界 / 平台未声明）
    /// 直接拦截，**不必**再查矩阵——矩阵里那些格反映的是"曾经实测过"，但作者
    /// 自己都不声称支持，矩阵结论不应反过来推翻声明。反之，声明通过而矩阵缺格，
    /// 结论是 `Held`（暂缓补测），不是准入也不是拦截。
    pub fn admit(
        &mut self,
        plugin: &str,
        host: &SemVer,
        platform: Platform,
    ) -> Admission {
        let di = match self.matrix.plugins.iter().position(|p| p == plugin) {
            Some(i) => i,
            None => {
                self.diags.push(CompatDiag {
                    code: CompatCode::PluginUnknown,
                    what: format!("准入查询引用未登记的插件 {}", plugin),
                    why: "表外插件没有可比对的声明与矩阵行".to_string(),
                    fix: format!("先把 {} 登记进插件表，或改查已登记的插件 ID", plugin),
                });
                return Admission {
                    gate: Gate::Blocked,
                    verdict: Verdict::Unknown,
                    reason: "E06_PLUGIN_UNKNOWN",
                    phase: MigrationPhase::Grace,
                };
            }
        };
        let decl = self.decls[di].clone();

        if !decl.host_range.contains(host) {
            self.diags.push(CompatDiag {
                code: CompatCode::HostIncompatible,
                what: format!(
                    "插件 {} 声明支持宿主 [{}, {}]，实测宿主为 {}",
                    plugin,
                    decl.host_range.min.to_canonical(),
                    decl.host_range.max.to_canonical(),
                    host.to_canonical()
                ),
                why: "宿主版本落在声明闭区间之外：作者已明示不支持，装上只会在运行时炸"
                    .to_string(),
                fix: format!(
                    "把宿主升到 [{}, {}] 区间内，或让作者把 {} 的兼容节上界扩到 {} 以上",
                    decl.host_range.min.to_canonical(),
                    decl.host_range.max.to_canonical(),
                    plugin,
                    host.to_canonical()
                ),
            });
            return Admission {
                gate: Gate::Blocked,
                verdict: Verdict::Incompatible,
                reason: "E06_HOST_INCOMPATIBLE",
                phase: MigrationPhase::Grace,
            };
        }

        if !decl.supports(platform) {
            self.diags.push(CompatDiag {
                code: CompatCode::PlatformUnsupported,
                what: format!(
                    "插件 {} 未声明支持平台 {}（已声明 {} 项）",
                    plugin,
                    platform.label(),
                    decl.platforms.len()
                ),
                why: "平台未声明即能力面未开放，插件在该平台上的行为无实测支撑".to_string(),
                fix: format!(
                    "在 {} 的清单兼容节补上 {} 平台，或换到已声明的平台运行",
                    plugin,
                    platform.tag()
                ),
            });
            return Admission {
                gate: Gate::Blocked,
                verdict: Verdict::Incompatible,
                reason: "E06_PLATFORM_UNSUPPORTED",
                phase: MigrationPhase::Grace,
            };
        }

        // 双版期：该插件所用废弃 API 是否已过期。
        let mut phase = MigrationPhase::Grace;
        let host_ord = self
            .matrix
            .hosts
            .iter()
            .position(|h| h == host)
            .map(|i| i as u32);
        if let Some(ord) = host_ord {
            for d in self.deps.iter() {
                if !d.users.iter().any(|u| u == plugin) {
                    continue;
                }
                let end = d.grace_end();
                let p = if ord > end {
                    MigrationPhase::Expired
                } else if ord == end {
                    MigrationPhase::LastCall
                } else {
                    MigrationPhase::Grace
                };
                if p == MigrationPhase::Expired {
                    self.diags.push(CompatDiag {
                        code: CompatCode::MigrationExpired,
                        what: format!(
                            "插件 {} 在宿主 {} 上仍使用已废弃 API {}（双版期止于宿主序 {}）",
                            plugin,
                            host.to_canonical(),
                            d.api,
                            end
                        ),
                        why: "双版期已过则旧 API 已消失，插件在该宿主版本上必然失效；\
                              零容忍的口径是拦截而非放行"
                            .to_string(),
                        fix: format!(
                            "把 {} 迁移到不依赖 {} 的新接口，或在宿主 {} 之前的版本上运行 {}",
                            plugin,
                            d.api,
                            self.matrix.hosts[end as usize].to_canonical(),
                            plugin
                        ),
                    });
                    return Admission {
                        gate: Gate::Blocked,
                        verdict: Verdict::Incompatible,
                        reason: "E06_MIGRATION_EXPIRED",
                        phase: MigrationPhase::Expired,
                    };
                }
                phase = p;
            }
        }

        // 三维矩阵查格：缺格 → Held（暂缓补测），不是准入。
        let probe = self.matrix.probe(plugin, host, platform);
        match probe.verdict {
            Verdict::Compatible => Admission {
                gate: Gate::Admit,
                verdict: Verdict::Compatible,
                reason: "",
                phase,
            },
            Verdict::Incompatible => {
                self.diags.push(CompatDiag {
                    code: CompatCode::CellConflict,
                    what: format!(
                        "矩阵实测 {} / {} / {} 为不兼容，但三维声明均通过",
                        plugin,
                        host.to_canonical(),
                        platform.tag()
                    ),
                    why: "声明通过而实测不兼容：实测是更强证据，以实测为准拦截".to_string(),
                    fix: format!(
                        "让作者按 {} / {} / {} 的实测结论修正实现，或补新的实测覆盖该组合",
                        plugin,
                        host.to_canonical(),
                        platform.tag()
                    ),
                });
                Admission {
                    gate: Gate::Blocked,
                    verdict: Verdict::Incompatible,
                    reason: "E06_CELL_CONFLICT",
                    phase,
                }
            }
            Verdict::Unknown => {
                let cell = CellRef {
                    plugin: plugin.to_string(),
                    host_ord: host_ord.unwrap_or(0),
                    host: host.to_canonical(),
                    platform,
                };
                if !self.matrix.schedule.iter().any(|s| {
                    s.cell.plugin == cell.plugin
                        && s.cell.host == cell.host
                        && s.cell.platform == cell.platform
                }) {
                    let seq = self.matrix.schedule.len() as u32;
                    self.matrix.schedule.push(ScheduleItem { cell, seq });
                }
                self.diags.push(CompatDiag {
                    code: CompatCode::CellMissing,
                    what: format!(
                        "矩阵缺格：{} / {} / {} 尚无实测结论",
                        plugin,
                        host.to_canonical(),
                        platform.tag()
                    ),
                    why: "缺格不是兼容：既不能据此放行（未测），也不能据此判不兼容（未测）；\
                          唯一正确的去处是补测排程"
                        .to_string(),
                    fix: format!(
                        "为 {} / {} / {} 排一次实测并把结论写入矩阵",
                        plugin,
                        host.to_canonical(),
                        platform.tag()
                    ),
                });
                Admission {
                    gate: Gate::Held,
                    verdict: Verdict::Unknown,
                    reason: "E06_CELL_MISSING",
                    phase,
                }
            }
        }
    }

    /// 交叉对账：声明范围 × 双版期 × 矩阵实测，三者不得互相矛盾。
    ///
    /// 这是本条最关键的一类判据——形状判据（版本解析对不对、格数对不对）证明不了
    /// 数值之间的关系正确，只有把三份账摆在一起对，才能抓住"矩阵声称兼容、但那个
    /// 宿主版本上旧 API 其实已经没了"这种**伪造兼容**。对账复杂度
    /// O(废弃数 × 使用方数 × 平台数)。
    pub fn cross_check(&mut self) -> usize {
        let mut found = 0usize;
        let plugins = self.matrix.plugins.clone();
        let hosts = self.matrix.hosts.clone();
        for d in self.deps.clone().iter() {
            let end = d.grace_end();
            for u in d.users.iter() {
                let di = match plugins.iter().position(|p| p == u) {
                    Some(i) => i,
                    None => continue,
                };
                let decl = self.decls[di].clone();
                // (一) 声明下界整体落在旧 API 已消失的区间：既已破损。
                let min_ord = match hosts.iter().position(|h| *h == decl.host_range.min) {
                    Some(i) => i as u32,
                    None => continue,
                };
                if min_ord > end {
                    self.diags.push_once(CompatDiag {
                        code: CompatCode::ScopeAlreadyBroken,
                        what: format!(
                            "插件 {} 声明的宿主下界 {} 已在 API {} 的双版期之外（止于宿主序 {}）",
                            u,
                            decl.host_range.min.to_canonical(),
                            d.api,
                            end
                        ),
                        why: "声明范围整体落在旧 API 已消失的宿主版本上：插件在它自称支持的\
                              每个宿主版本上都会失效"
                            .to_string(),
                        fix: format!(
                            "把 {} 的兼容节下界 {} 提到宿主序 {} 以内，或先迁移掉对 {} 的依赖",
                            u,
                            decl.host_range.min.to_canonical(),
                            end,
                            d.api
                        ),
                    });
                    found += 1;
                }
                // (二) 声明上界越过双版期：矛盾；矩阵若称该格兼容即伪造兼容。
                let max_ord = match hosts.iter().position(|h| *h == decl.host_range.max) {
                    Some(i) => i as u32,
                    None => continue,
                };
                if max_ord > end {
                    self.diags.push_once(CompatDiag {
                        code: CompatCode::ScopeContradiction,
                        what: format!(
                            "插件 {} 声明支持到宿主 {}，但 API {} 在宿主序 {} 之后已消失",
                            u,
                            decl.host_range.max.to_canonical(),
                            d.api,
                            end
                        ),
                        why: "声明范围覆盖到旧 API 已消失的宿主版本：作者承诺的宿主里有一段\
                              根本跑不起来"
                            .to_string(),
                        fix: format!(
                            "把 {} 的兼容节上界 {} 收到宿主序 {} 以内，或迁移掉对 {} 的依赖",
                            u,
                            decl.host_range.max.to_canonical(),
                            end,
                            d.api
                        ),
                    });
                    found += 1;
                    for plat in Platform::all() {
                        if self.matrix.cell(di, max_ord as usize, plat.ordinal())
                            == CellState::Compatible
                        {
                            self.diags.push_once(CompatDiag {
                                code: CompatCode::FabricatedCompat,
                                what: format!(
                                    "矩阵声称 {} / {} / {} 兼容，但该宿主版本上 API {} 已不存在",
                                    u,
                                    decl.host_range.max.to_canonical(),
                                    plat.tag(),
                                    d.api
                                ),
                                why: "旧 API 已消失的宿主版本上不可能实测兼容；这一格是伪造的\
                                      证据，矩阵据此放行等于把插件放上必崩的宿主"
                                    .to_string(),
                                fix: format!(
                                    "撤掉 {} / {} / {} 的兼容结论，重跑实测；并把 {} 的声明上界收到宿主序 {} 以内",
                                    u,
                                    decl.host_range.max.to_canonical(),
                                    plat.tag(),
                                    u,
                                    end
                                ),
                            });
                            found += 1;
                        }
                    }
                }
            }
        }
        found
    }

    /// 补测排程：把全部缺格登记进队列（幂等；不吞已有项）。
    pub fn schedule_missing(&mut self) -> usize {
        let missing = self.matrix.missing_cells();
        let mut added = 0usize;
        for cell in missing.into_iter() {
            let dup = self.matrix.schedule.iter().any(|s| {
                s.cell.plugin == cell.plugin
                    && s.cell.host == cell.host
                    && s.cell.platform == cell.platform
            });
            if dup {
                continue;
            }
            let seq = self.matrix.schedule.len() as u32;
            self.matrix.schedule.push(ScheduleItem { cell, seq });
            added += 1;
        }
        added
    }

    /// 版本扫描计数（语义版本校验的工作量；应与矩阵规模无关）。
    pub fn scan_version(&mut self, src: &str) -> Result<SemVer, CompatDiag> {
        self.matrix.work.version_scans += 1;
        parse_semver(src)
    }
}

// ---------------------------------------------------------------------------
// 八、可视化与无障碍出口（矩阵与迁移台账的读屏替代）
// ---------------------------------------------------------------------------

/// 矩阵文本视图（等宽表格；缺口显式标 `?`，不与兼容混同）。
pub fn render_matrix(m: &CompatMatrix) -> String {
    let mut s = String::new();
    s.push_str("插件 × 宿主 × 平台 兼容矩阵\n");
    s.push_str("图例: + 实测兼容  - 实测不兼容  ? 缺格(未实测, 非兼容)\n");
    for (p, pid) in m.plugins.iter().enumerate() {
        s.push_str(pid);
        s.push('\n');
        for (h, hv) in m.hosts.iter().enumerate() {
            s.push_str("  ");
            s.push_str(&hv.to_canonical());
            s.push_str(" |");
            for plat in Platform::all() {
                let mark = match m.cell(p, h, plat.ordinal()) {
                    CellState::Compatible => '+',
                    CellState::Incompatible => '-',
                    CellState::Unknown => '?',
                };
                s.push(' ');
                s.push(mark);
            }
            s.push('\n');
        }
        s.push_str("         ");
        for plat in Platform::all() {
            s.push(' ');
            s.push(plat.tag().as_bytes()[0] as char);
        }
        s.push('\n');
    }
    let (measured, compat, incompat, missing) = m.coverage();
    s.push_str(&format!(
        "格 {} = 实测 {} (兼容 {} / 不兼容 {}) + 缺格 {}；缺格不是兼容\n",
        m.cell_count(),
        measured,
        compat,
        incompat,
        missing
    ));
    s
}

/// 矩阵读屏播报（逐格播报；缺格播报为"未实测"，不播报为兼容）。
pub fn matrix_spoken(m: &CompatMatrix) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "兼容矩阵共 {} 格，实测 {} 格，缺格 {} 格。",
        m.cell_count(),
        m.coverage().0,
        m.coverage().3
    ));
    for (p, pid) in m.plugins.iter().enumerate() {
        for (h, hv) in m.hosts.iter().enumerate() {
            for plat in Platform::all() {
                let word = match m.cell(p, h, plat.ordinal()) {
                    CellState::Compatible => "实测兼容",
                    CellState::Incompatible => "实测不兼容",
                    CellState::Unknown => "未实测",
                };
                s.push_str(&format!(
                    "{} 于宿主 {} 于 {}：{}。",
                    pid,
                    hv.to_canonical(),
                    plat.label(),
                    word
                ));
            }
        }
    }
    s
}

/// 迁移台账读屏播报（逐条播报剩余迁移期与阶段）。
pub fn migration_spoken(view: &MigrationView, hosts: &[SemVer]) -> String {
    let mut s = String::new();
    let at = hosts
        .get(view.at_host_ord as usize)
        .map(|h| h.to_canonical())
        .unwrap_or_else(|| format!("宿主序 {}", view.at_host_ord));
    s.push_str(&format!("迁移台账（宿主 {}）：", at));
    if view.rows.is_empty() {
        s.push_str("当前无使用废弃 API 的插件。");
        return s;
    }
    for r in view.rows.iter() {
        let phase = match r.phase {
            MigrationPhase::Grace => "迁移期内",
            MigrationPhase::LastCall => "迁移期最后一格，旧 API 仍存在",
            MigrationPhase::Expired => "双版期已过，旧 API 已消失",
        };
        s.push_str(&format!(
            "{} 使用 {}：{}，剩余 {} 个宿主版本。",
            r.plugin, r.api, phase, r.remaining_hosts
        ));
    }
    s
}

// ---------------------------------------------------------------------------
// 九、自检底座（合规夹具）
// ---------------------------------------------------------------------------

/// 宿主版本表（五档，序 0..4）。
///
/// 档数取 5 是被双版期算式逼出来的：`DUAL_PERIOD_HOSTS = 2` 故过期宿主序 = `at + 3`，
/// 夹具若只备三档，取过期宿主时就会越界。夹具必须覆盖到过期格，否则过期路径根本测不到。
fn hosts_fixture() -> Vec<SemVer> {
    vec![
        SemVer::new(1, 0, 0),
        SemVer::new(1, 2, 0),
        SemVer::new(2, 0, 0),
        SemVer::new(2, 1, 0),
        SemVer::new(2, 2, 0),
    ]
}

/// 插件表（两只）。
fn plugins_fixture() -> Vec<String> {
    vec!["demo.theme".to_string(), "demo.chart".to_string()]
}

/// 声明表：`demo.theme` 支持 1.0.0~2.2.0 全平台；`demo.chart` 只支持 1.0.0~1.2.0 与桌面三平台。
///
/// `demo.theme` 的上界特意取到宿主表末档（2.2.0 / 序 4），好让"过期宿主序 = at+3"
/// 落在它声明的范围**之内**——否则过期路径会先被宿主越界拦掉，双版期判据就测不到。
fn decls_fixture() -> Vec<PluginDecl> {
    let theme_range = HostRange::new(SemVer::new(1, 0, 0), SemVer::new(2, 2, 0));
    let chart_range = HostRange::new(SemVer::new(1, 0, 0), SemVer::new(1, 2, 0));
    let theme_range = theme_range.unwrap_or(HostRange {
        min: SemVer::new(1, 0, 0),
        max: SemVer::new(2, 2, 0),
    });
    let chart_range = chart_range.unwrap_or(HostRange {
        min: SemVer::new(1, 0, 0),
        max: SemVer::new(1, 2, 0),
    });
    vec![
        PluginDecl::new("demo.theme", SemVer::new(0, 3, 1), theme_range, Platform::all().to_vec()),
        PluginDecl::new(
            "demo.chart",
            SemVer::new(1, 4, 0),
            chart_range,
            vec![Platform::Windows, Platform::Linux, Platform::Macos],
        ),
    ]
}

/// 合规引擎：矩阵全格实测兼容、零废弃登记。
fn compliant() -> CompatEngine {
    let mut m = CompatMatrix::new(hosts_fixture(), plugins_fixture()).unwrap_or(
        CompatMatrix {
            hosts: Vec::new(),
            plugins: Vec::new(),
            cells: Vec::new(),
            schedule: Vec::new(),
            work: Work::default(),
        },
    );
    let hosts = m.hosts.clone();
    let plugins = m.plugins.clone();
    for p in plugins.iter() {
        for h in hosts.iter() {
            for plat in Platform::all() {
                let _ = m.record(p, h, plat, CellState::Compatible);
            }
        }
    }
    CompatEngine::new(m, decls_fixture()).unwrap_or(CompatEngine {
        matrix: CompatMatrix {
            hosts: Vec::new(),
            plugins: Vec::new(),
            cells: Vec::new(),
            schedule: Vec::new(),
            work: Work::default(),
        },
        decls: Vec::new(),
        deps: Vec::new(),
        diags: DiagBag::new(),
    })
}

/// 半实测夹具：仅 demo.theme 在 1.0.0 全平台实测兼容，其余全缺格。
fn half_filled() -> CompatEngine {
    let mut m = CompatMatrix::new(hosts_fixture(), plugins_fixture()).unwrap_or(
        CompatMatrix {
            hosts: Vec::new(),
            plugins: Vec::new(),
            cells: Vec::new(),
            schedule: Vec::new(),
            work: Work::default(),
        },
    );
    let h0 = m.hosts[0].clone();
    for plat in Platform::all() {
        let _ = m.record("demo.theme", &h0, plat, CellState::Compatible);
    }
    CompatEngine::new(m, decls_fixture()).unwrap_or(CompatEngine {
        matrix: CompatMatrix {
            hosts: Vec::new(),
            plugins: Vec::new(),
            cells: Vec::new(),
            schedule: Vec::new(),
            work: Work::default(),
        },
        decls: Vec::new(),
        deps: Vec::new(),
        diags: DiagBag::new(),
    })
}

/// 迁��夹具：API `theme.token` 自宿主序 0 废弃，使用方 demo.theme。
fn with_deprecation(e: &mut CompatEngine) {
    let _ = e.deprecate(Deprecation::new("theme.token", 0, vec!["demo.theme".to_string()]));
}

// ---------------------------------------------------------------------------
// 十、域自检（CheckSet）
// ---------------------------------------------------------------------------

/// VE-F4606 域自检。
pub fn run_vew06_checks() -> CheckSet {
    let mut set = CheckSet::new(VEW06_DOMAIN);

    // 前置哨兵：基础夹具必须建得出引擎，否则后续判据的前提不成立。
    let base_ok = match CompatMatrix::new(hosts_fixture(), plugins_fixture()) {
        Ok(m) => CompatEngine::new(m, decls_fixture()).is_ok(),
        Err(_) => false,
    };
    if !base_ok {
        set.fail(
            "W06-前置-基础夹具可建引擎",
            "宿主/插件/声明夹具或建矩阵失败，后续判据的前提不成立，已停止本轮自检",
        );
        return set;
    }
    set.ok("W06-前置-基础夹具可建引擎");

    // ---- 判据一：语义版本 ----
    {
        let v = parse_semver("1.2.3");
        set.add(
            "W06-语义版本-三段式可解析且规范串回环",
            matches!(&v, Ok(p) if p.major == 1 && p.minor == 2 && p.patch == 3
                && p.to_canonical() == "1.2.3" && !p.is_prerelease()),
            "",
        );
    }
    {
        // 段数恰三：两段须拒（四段同理）。
        let two = parse_semver("1.2").is_err();
        let four = parse_semver("1.2.3.4").is_err();
        let seg = two && four;
        set.add("W06-语义版本-段数非三显性拒绝", seg, "");
    }
    {
        // 前导零拒绝，且诊断码须是专用码而非泛化 malformed。
        let d = parse_semver("01.2.3");
        let leading = matches!(&d, Err(e) if e.code == CompatCode::SemVerLeadingZero);
        set.add("W06-语义版本-前导零拒绝并归专用码", leading, "");
    }
    {
        // 预发布优先级低于同数字正式版，且数值标识低于字母数字标识。
        let pre = parse_semver("1.0.0-alpha").unwrap_or(SemVer::new(0, 0, 0));
        let pre_num = parse_semver("1.0.0-1").unwrap_or(SemVer::new(0, 0, 0));
        let rel = SemVer::new(1, 0, 0);
        let ok = pre < rel && pre_num < pre;
        set.add("W06-语义版本-预发布低于正式版且数值标识更低", ok, "");
    }
    {
        // 构建元数据不参与**优先级**比较。
        //
        // 判据盯的是 `Ord` 而不是 `==`：两个版本的 `build` 字段字面不同（`aaa`
        // 与 `zzz`），派生 `PartialEq` 判它们不等是对的——那是**结构身份**；
        // 而"不参与优先级"说的是排序，两者是两回事。拿 `==` 当判据会把
        // "元数据被正确保留"误判成失败，也会让真正的优先级缺陷漏网。
        let a = parse_semver("1.0.0+aaa").unwrap_or(SemVer::new(0, 0, 0));
        let b = parse_semver("1.0.0+zzz").unwrap_or(SemVer::new(0, 0, 0));
        set.add(
            "W06-语义版本-构建元数据不参与优先级",
            a.cmp(&b) == Ordering::Equal && a.build != b.build,
            "",
        );
    }
    {
        // 版本不识别 → 明确报错 + 升级指引点名那个非法串（不接受空话）。
        let d = parse_semver("1.2.x").unwrap_err_or();
        set.add(
            "W06-语义版本-不识别者指引点名该串",
            d.code == CompatCode::SemVerMalformed && d.fix_names("1.2.x"),
            "",
        );
    }
    {
        // 预发布标识符为空须拒。
        let bad_empty = parse_semver("1.0.0-").is_err();
        let bad_dot = parse_semver("1.0.0-a..b").is_err();
        set.add("W06-语义版本-预发布标识符非空", bad_empty && bad_dot, "");
    }
    {
        // 多位数须按数值比较：2.10.0 > 2.9.0，且 10.0.0 > 9.0.0。
        //
        // 两条都要：只测 `10 > 9`，一个把解析结果整体取反或错位一位的实现仍可能
        // 蒙对；再测 `2.10 > 2.9`（跨段的两位数）才能锁住"逐段独立按数值比"。
        let a = parse_semver("2.10.0").unwrap_or(SemVer::new(0, 0, 0));
        let b = parse_semver("2.9.0").unwrap_or(SemVer::new(0, 0, 0));
        let c = parse_semver("10.0.0").unwrap_or(SemVer::new(0, 0, 0));
        let d = parse_semver("9.0.0").unwrap_or(SemVer::new(0, 0, 0));
        // 两段版本须被拒（对 Result 本身断，不在 unwrap_or 后的值上断）。
        let mid_rejected = parse_semver("2.10").is_err();
        set.add(
            "W06-语义版本-多位数按数值比较",
            a > b && c > d && mid_rejected,
            "",
        );
    }
    {
        // 多位数逐段保真：解析出的数值必须**逐段等于**字面数值。
        //
        // 这是上一条的独立复算面：序关系判据只证明"排得对"，一个把 10 解析成
        // 1、把 2.10 解析成 2.1 的实现仍能维持大小关系（1 < 9、2.1 < 2.9 都成立），
        // 却把版本号读错了。逐段等值才抓得住这种"序对值错"。
        let v = parse_semver("12.34.56").unwrap_or(SemVer::new(0, 0, 0));
        set.add(
            "W06-语义版本-多位数逐段解析保真",
            v.major == 12 && v.minor == 34 && v.patch == 56,
            "",
        );
    }
    {
        // 区间倒置即拒，且指引点名两端。
        let r = HostRange::new(SemVer::new(2, 0, 0), SemVer::new(1, 0, 0));
        set.add(
            "W06-语义版本-区间倒置拒绝且指引点名两端",
            matches!(&r, Err(e) if e.code == CompatCode::HostRangeInverted
                && e.fix_names("2.0.0") && e.fix_names("1.0.0")),
            "",
        );
    }
    {
        // 闭区间含两端（消费 F4602 口径）。
        let r = HostRange::new(SemVer::new(1, 0, 0), SemVer::new(2, 0, 0)).unwrap_or(
            HostRange { min: SemVer::new(1, 0, 0), max: SemVer::new(1, 0, 0) },
        );
        set.add(
            "W06-语义版本-宿主区间为闭区间含两端",
            r.contains(&SemVer::new(1, 0, 0))
                && r.contains(&SemVer::new(2, 0, 0))
                && !r.contains(&SemVer::new(2, 0, 1)),
            "",
        );
    }

    // ---- 判据二：三维矩阵 ----
    {
        let e = compliant();
        let (measured, compat, incompat, missing) = e.matrix.coverage();
        // 三态互斥完备：四数之和恒等于总格数。
        let total = e.matrix.cell_count();
        let ok = measured + missing == total && incompat == 0 && compat == total
            && e.matrix.hosts.len() == 5
            && e.matrix.plugins.len() == 2;
        set.add("W06-三维矩阵-三态互斥完备且格数等于三轴之积", ok, "");
    }
    {
        // 缺格不是兼容：仅一格实测的表上，其余格一律 Unknown。
        let e = half_filled();
        let (measured, _, _, missing) = e.matrix.coverage();
        let total = e.matrix.cell_count();
        let ok = measured == PLATFORM_COUNT && missing == total - PLATFORM_COUNT;
        set.add("W06-三维矩阵-缺格计为未实测而非兼容", ok, "");
    }
    {
        // 最强形态判据：缺格时准入闸须为 Held，**既不放行也不拦截**。
        let mut e = half_filled();
        let a = e.admit("demo.theme", &SemVer::new(2, 0, 0), Platform::Wasm);
        let held = a.gate == Gate::Held && !a.gate.admits() && a.verdict == Verdict::Unknown;
        let queued = e.matrix.schedule.iter().any(|s| {
            s.cell.plugin == "demo.theme"
                && s.cell.host == "2.0.0"
                && s.cell.platform == Platform::Wasm
        });
        set.add("W06-三维矩阵-缺格准入为暂缓且进补测排程", held && queued, "");
    }
    {
        // 缺格不得被写成实测结论（否则缺格伪装成兼容）。
        //
        // 夹具用 `half_filled()`：合规底座那格已是兼容，往里写"缺格"会先被
        // 实测冲突闸拒掉——那样即使"缺格闸"整个不存在，这条判据照样全绿，
        // 等于用另一道闸替它站岗（两闸叠着测一条，永远测不出其中一条被拆）。
        let mut e = half_filled();
        let rejected = e
            .matrix
            .record("demo.chart", &SemVer::new(2, 2, 0), Platform::Wasm, CellState::Unknown)
            .is_err();
        set.add("W06-三维矩阵-拒绝把缺格写成实测结论", rejected, "");
    }
    {
        // 上一条的对照组：拆掉缺格闸后，冲突闸**不得**接手把这条判据救绿。
        //
        // 这里显式验一次"两道闸各管各的"：往**已兼容**的格写缺格，冲突闸拒；
        // 往**缺格**写缺格（等价于把它抹掉），缺格闸拒。两个方向都得有人拦。
        let mut e = half_filled();
        let into_empty = e
            .matrix
            .record("demo.chart", &SemVer::new(2, 2, 0), Platform::Wasm, CellState::Unknown)
            .is_err();
        let into_filled = e
            .matrix
            .record("demo.theme", &SemVer::new(1, 0, 0), Platform::Wasm, CellState::Unknown)
            .is_err();
        set.add(
            "W06-三维矩阵-缺格闸与冲突闸各守一个方向",
            into_empty && into_filled,
            "",
        );
    }
    {
        // 实测冲突显性拒绝：同格两次不同结论不得后者覆盖。
        let mut e = compliant();
        let before = e.matrix.cell(0, 0, Platform::Wasm.ordinal());
        let conflict = e
            .matrix
            .record("demo.theme", &SemVer::new(1, 0, 0), Platform::Wasm, CellState::Incompatible)
            .is_err();
        let after = e.matrix.cell(0, 0, Platform::Wasm.ordinal());
        set.add(
            "W06-三维矩阵-同格实测冲突拒绝且不覆盖原值",
            conflict && before == CellState::Compatible && after == before,
            "",
        );
    }
    {
        // 同结论重跑幂等（不误报冲突）。
        let mut e = compliant();
        let idem = e
            .matrix
            .record("demo.theme", &SemVer::new(1, 0, 0), Platform::Wasm, CellState::Compatible)
            .is_ok();
        set.add("W06-三维矩阵-同结论重复实测幂等", idem, "");
    }
    {
        // 实测不兼容 → 版本拦截（拦截方向明确）。
        //
        // 底座用 `half_filled()`：合规底座那格已记为兼容，往里写"不兼容"会
        // 被实测冲突闸拒掉（那是对的），格仍是兼容，判据就变成在测别的东西。
        let mut e = half_filled();
        let _ = e.matrix.record("demo.chart", &SemVer::new(1, 2, 0), Platform::Linux, CellState::Incompatible);
        let a = e.admit("demo.chart", &SemVer::new(1, 2, 0), Platform::Linux);
        set.add(
            "W06-三维矩阵-实测不兼容即版本拦截",
            a.gate == Gate::Blocked && a.verdict == Verdict::Incompatible && a.reason == "E06_CELL_CONFLICT",
            "",
        );
    }
    {
        // 宿主越界 → 版本拦截，且诊断点名宿主版本。
        let mut e = compliant();
        let a = e.admit("demo.chart", &SemVer::new(2, 0, 0), Platform::Linux);
        let named = e.diags.has(CompatCode::HostIncompatible)
            && e.diags
                .all()
                .iter()
                .any(|d| d.code == CompatCode::HostIncompatible && d.fix_names("2.0.0"));
        set.add("W06-三维矩阵-宿主越界拦截且指引点名该版本", a.gate == Gate::Blocked && named, "");
    }
    {
        // 平台未声明 → 版本拦截。
        let mut e = compliant();
        let a = e.admit("demo.chart", &SemVer::new(1, 0, 0), Platform::Wasm);
        set.add(
            "W06-三维矩阵-平台未声明即拦截",
            a.gate == Gate::Blocked && a.reason == "E06_PLATFORM_UNSUPPORTED",
            "",
        );
    }
    {
        // 补测排程覆盖全部缺格且幂等（不重复排队）。
        let mut e = half_filled();
        let first = e.schedule_missing();
        let total_missing = e.matrix.coverage().3;
        let again = e.schedule_missing();
        set.add(
            "W06-三维矩阵-补测排程覆盖全缺格且幂等",
            first == total_missing && again == 0 && e.matrix.schedule.len() == total_missing,
            "",
        );
    }
    {
        // 排程项内容须可读屏（三轴俱全 + 序稳定）。
        let mut e = half_filled();
        let n = e.schedule_missing();
        let item = e.matrix.schedule.first().map(|s| s.cell.clone());
        let ok = n > 0
            && item.as_ref().map(|c| c.host_ord < 5 && !c.host.is_empty() && !c.plugin.is_empty())
                == Some(true)
            && e.matrix.schedule[0].seq == 0;
        set.add("W06-三维矩阵-排程项三轴俱全且序稳定", ok, "");
    }
    {
        // 宿主表非严格升序即拒（序即索引，乱序会让同版本落两格）。
        let bad = CompatMatrix::new(
            vec![SemVer::new(2, 0, 0), SemVer::new(1, 0, 0)],
            vec!["a".to_string()],
        );
        set.add("W06-三维矩阵-宿主表非严格升序拒绝", bad.is_err(), "");
    }
    {
        // 表外坐标不得写入实测。
        let mut e = compliant();
        let bad_plugin = e
            .matrix
            .record("no.such", &SemVer::new(1, 0, 0), Platform::Linux, CellState::Compatible)
            .is_err();
        let bad_host = e
            .matrix
            .record("demo.theme", &SemVer::new(9, 9, 9), Platform::Linux, CellState::Compatible)
            .is_err();
        set.add("W06-三维矩阵-表外插件与表外宿主写入拒绝", bad_plugin && bad_host, "");
    }

    // ---- 判据三：双版期 ----
    {
        // 双版期长度不为零：取 0 等于废弃即刻消失，承诺归零。
        set.add("W06-双版期-长度不为零且区间闭", DUAL_PERIOD_HOSTS >= 1, "");
    }
    {
        // 上界闭：该宿主版本上旧 API **仍然存在**（阶段 LastCall，不是 Expired）。
        let mut e = compliant();
        with_deprecation(&mut e);
        let end = e.deps[0].grace_end();
        let at_end = migration_view(&e.deps, end);
        let row = at_end.rows.first();
        set.add(
            "W06-双版期-上界那一格旧 API 仍存在",
            at_end.expired == 0
                && row.map(|r| r.phase == MigrationPhase::LastCall) == Some(true),
            "",
        );
    }
    {
        // 越界一格即消失并拦截。
        let mut e = compliant();
        with_deprecation(&mut e);
        let end = e.deps[0].grace_end();
        let past = migration_view(&e.deps, end + 1);
        let row = past.rows.first();
        set.add(
            "W06-双版期-越界一格即过期",
            past.expired == 1 && row.map(|r| r.phase == MigrationPhase::Expired) == Some(true),
            "",
        );
    }
    {
        // 过期后在宿主上准入须被拦（破坏性变更零容忍）。
        let mut e = compliant();
        with_deprecation(&mut e);
        let end = e.deps[0].grace_end();
        let host_past = e.matrix.hosts[(end + 1) as usize].clone();
        let a = e.admit("demo.theme", &host_past, Platform::Linux);
        set.add(
            "W06-双版期-过期后使用废弃 API 即版本拦截",
            a.gate == Gate::Blocked && a.reason == "E06_MIGRATION_EXPIRED",
            "",
        );
    }
    {
        // 边界宿主上**不得**拦截（提前撕毁迁移期是反面）。
        let mut e = compliant();
        with_deprecation(&mut e);
        let end = e.deps[0].grace_end();
        let host_end = e.matrix.hosts[end as usize].clone();
        let a = e.admit("demo.theme", &host_end, Platform::Linux);
        set.add(
            "W06-双版期-边界宿主不拦截（迁移期未被提前撕毁）",
            a.gate != Gate::Blocked && a.phase == MigrationPhase::LastCall,
            "",
        );
    }
    {
        // 废弃登记的宿主序越界即立案。
        let mut e = compliant();
        let ok = !e.deprecate(Deprecation::new("x", 99, vec!["demo.theme".to_string()]))
            && e.diags.has(CompatCode::DeprecationOutOfRange);
        set.add("W06-双版期-废弃登记越界立案", ok, "");
    }
    {
        // 废弃登记的使用方须在插件表内。
        let mut e = compliant();
        let ok = !e.deprecate(Deprecation::new("x", 0, vec!["no.such".to_string()]))
            && e.diags.has(CompatCode::PluginUnknown);
        set.add("W06-双版期-废弃登记表外使用方立案", ok, "");
    }

    // ---- 判据四：迁移期 ----
    {
        // 剩余迁移期**算术精确**：逐宿主版本逐点核对，不做符号近似。
        let d = vec![Deprecation::new("theme.token", 0, vec!["demo.theme".to_string()])];
        let end = d[0].grace_end();
        let mut all_exact = true;
        for cur in 0..=(end + 2) {
            let v = migration_view(&d, cur);
            let want = if cur > end { 0 } else { end - cur };
            match v.rows.first() {
                Some(r) if r.remaining_hosts == want => {}
                _ => all_exact = false,
            }
        }
        set.add("W06-迁移期-剩余量逐点算术精确", all_exact, "");
    }
    {
        // 边界剩余量为 0 但阶段是 LastCall（**不是** Expired）——两者不可混同。
        let d = vec![Deprecation::new("a", 0, vec!["p".to_string()])];
        let end = d[0].grace_end();
        let v = migration_view(&d, end);
        let r = v.rows.first();
        set.add(
            "W06-迁移期-剩余为零但阶段为最后一格",
            r.map(|x| x.remaining_hosts == 0 && x.phase == MigrationPhase::LastCall) == Some(true),
            "",
        );
    }
    {
        // 迁移视图与废弃登记的使用方**双向集合相等**（不多不少各一行）。
        let mut e = compliant();
        let _ = e.deprecate(Deprecation::new(
            "a",
            0,
            vec!["demo.theme".to_string(), "demo.chart".to_string()],
        ));
        let _ = e.deprecate(Deprecation::new("b", 1, vec!["demo.chart".to_string()]));
        let v = migration_view(&e.deps, 0);
        let expect = 3usize; // (theme,a) + (chart,a) + (chart,b)
        let mut forward = true;
        for r in v.rows.iter() {
            let found = e.deps.iter().any(|d| {
                d.api == r.api && d.users.iter().any(|u| *u == r.plugin)
            });
            if !found {
                forward = false;
            }
        }
        set.add(
            "W06-迁移期-视图行与废弃登记双向集合相等",
            v.rows.len() == expect && forward,
            "",
        );
    }
    {
        // 使用方顺序不影响视图条数（集合相等而非多重集）。
        let mut e = compliant();
        let _ = e.deprecate(Deprecation::new(
            "a",
            0,
            vec!["demo.chart".to_string(), "demo.theme".to_string()],
        ));
        let v = migration_view(&e.deps, 0);
        set.add("W06-迁移期-使用方顺序不改变视图条数", v.rows.len() == 2, "");
    }
    {
        // 无废弃登记时迁移视图为空（不是漏算）。
        let e = compliant();
        let v = migration_view(&e.deps, 0);
        set.add("W06-迁移期-无废弃登记则视图为空", v.rows.is_empty() && v.expired == 0, "");
    }
    {
        // 多条废弃各自独立计时（不得共用一个上界）。
        let d = vec![
            Deprecation::new("a", 0, vec!["p".to_string()]),
            Deprecation::new("b", 2, vec!["p".to_string()]),
        ];
        // a 的双版期止于宿主序 2，b 止于宿主序 4；取宿主序 3 看分叉：
        // a 已过期（3 > 2），b 仍在迁移期内且剩 1 版（3 < 4）。
        let v = migration_view(&d, 3);
        let a_expired = v.rows.iter().any(|r| r.api == "a" && r.phase == MigrationPhase::Expired);
        let b_grace = v.rows.iter().any(|r| {
            r.api == "b" && r.phase == MigrationPhase::Grace && r.remaining_hosts == 1
        });
        set.add("W06-迁移期-多条废弃各自独立计时", a_expired && b_grace, "");
    }
    {
        // 迁移台账读屏播报须区分三个阶段（无障碍替述）。
        let d = vec![Deprecation::new("a", 0, vec!["demo.theme".to_string()])];
        let v = migration_view(&d, 0);
        let s = migration_spoken(&v, &hosts_fixture());
        let ok = s.contains("迁移期内") && s.contains("剩余 2 个宿主版本");
        set.add("W06-迁移期-台账读屏播报含剩余量", ok, "");
    }

    // ---- 判据五：交叉对账（声明 × 双版期 × 矩阵）----
    {
        // 声明上界越过双版期 → 矛盾立案。
        let mut e = compliant();
        // deprecate chart.old 于宿主序 0，双版期止于宿主序 2；
        // demo.chart 声明上界 1.2.0（序 1）未越界 → 对账不得误报。
        let _ = e.deprecate(Deprecation::new("chart.old", 0, vec!["demo.chart".to_string()]));
        let found = e.cross_check();
        let no_false_positive = found == 0
            && !e.diags.has(CompatCode::ScopeContradiction)
            && !e.diags.has(CompatCode::FabricatedCompat);
        set.add("W06-交叉-声明未越界时不误报矛盾", no_false_positive, "");
    }
    {
        // 声明上界越过双版期且矩阵称兼容 → **伪造兼容**（本条最强判据）。
        let mut e = compliant();
        // 把 chart 上界推到 2.2.0（宿主序 4）> 双版期上界 2。
        e.decls[1].host_range.max = SemVer::new(2, 2, 0);
        let _ = e.deprecate(Deprecation::new("chart.old", 0, vec!["demo.chart".to_string()]));
        let found = e.cross_check();
        let fabricated = e.diags.has(CompatCode::FabricatedCompat);
        let contradicted = e.diags.has(CompatCode::ScopeContradiction);
        // 四平台全为兼容 → 四条伪造兼容诊断 + 一条矛盾。
        let fab_count = e
            .diags
            .all()
            .iter()
            .filter(|d| d.code == CompatCode::FabricatedCompat)
            .count();
        set.add(
            "W06-交叉-矩阵称兼容而旧 API 已消失判为伪造兼容",
            found >= PLATFORM_COUNT + 1 && fabricated && contradicted && fab_count == PLATFORM_COUNT,
            "",
        );
    }
    {
        // 伪造兼容诊断须给出具体迁移出口（点名插件与上界版本）。
        let mut e = compliant();
        e.decls[1].host_range.max = SemVer::new(2, 2, 0);
        let _ = e.deprecate(Deprecation::new("chart.old", 0, vec!["demo.chart".to_string()]));
        let _ = e.cross_check();
        let ok = e
            .diags
            .all()
            .iter()
            .filter(|d| d.code == CompatCode::FabricatedCompat)
            .all(|d| d.fix_names("demo.chart") && d.fix_names("2.2.0"));
        set.add("W06-交叉-伪造兼容诊断给出点名迁移出口", ok, "");
    }
    {
        // 声明下界整体落在已消失区间 → 既已破损。
        let mut e = compliant();
        e.decls[1].host_range.min = SemVer::new(2, 2, 0);
        e.decls[1].host_range.max = SemVer::new(2, 2, 0);
        let _ = e.deprecate(Deprecation::new("chart.old", 0, vec!["demo.chart".to_string()]));
        let _ = e.cross_check();
        set.add(
            "W06-交叉-声明整体落在已消失区间判为既已破损",
            e.diags.has(CompatCode::ScopeAlreadyBroken),
            "",
        );
    }
    {
        // 对账幂等：连跑两次，账本条数不得增生。
        //
        // 判据盯**账本条数**而不是 `cross_check()` 的返回值：`cross_check` 返回的是
        // 「检出几条矛盾」，矛盾客观存在，重跑仍应被检出（返回 5 条是对的）；
        // 而账本是**事实集合**，同一矛盾登记两次会把一条问题放大成一片。
        // 两个数各管一件事，判据必须分开盯。
        let mut e = compliant();
        e.decls[1].host_range.max = SemVer::new(2, 2, 0);
        let _ = e.deprecate(Deprecation::new("chart.old", 0, vec!["demo.chart".to_string()]));
        let first = e.cross_check();
        let before = e.diags.all().len();
        let second = e.cross_check();
        set.add(
            "W06-交叉-对账幂等不增生诊断",
            first == 5 && second == 5 && e.diags.all().len() == before && before > 0,
            "",
        );
    }
    {
        // 矩阵与迁移结论须一致：过期宿主上的格若仍为兼容，admit 必拦（不靠 cross_check 兜）。
        let mut e = compliant();
        with_deprecation(&mut e);
        let end = e.deps[0].grace_end();
        let host_past = e.matrix.hosts[(end + 1) as usize].clone();
        let a = e.admit("demo.theme", &host_past, Platform::Linux);
        set.add(
            "W06-交叉-过期宿主上的兼容格仍被准入拦截",
            a.gate == Gate::Blocked,
            "",
        );
    }
    {
        // 三要素完整性：缺任一要素即降级（半条诊断不许蒙混）。
        let mut bag = DiagBag::new();
        let ok1 = bag.push(CompatDiag {
            code: CompatCode::CellMissing,
            what: "缺格".to_string(),
            why: "未测".to_string(),
            fix: "补测".to_string(),
        });
        let bad = bag.push(CompatDiag {
            code: CompatCode::CellMissing,
            what: "缺格".to_string(),
            why: String::new(),
            fix: "补测".to_string(),
        });
        set.add(
            "W06-诊断-三要素缺一即降级",
            ok1 && !bad && bag.downgraded.len() == 1 && bag.downgraded[0].missing == "why",
            "",
        );
    }
    {
        // 三要素的**三个方向**分别设哨（上一条只盯了 why 一侧）。
        //
        // 只测一个方向，另两个方向被拆掉时判据照样全绿——"降级机制存在"不等于
        // "三种缺失都被降级"。这里逐方向各记一条，确认账本里降级项与缺失要素名
        // 一一对应（缺 what / 缺 why / 缺 fix 各一条）。
        let mut bag = DiagBag::new();
        let _ = bag.push(CompatDiag {
            code: CompatCode::CellMissing,
            what: String::new(),
            why: "未测".to_string(),
            fix: "补测 1.2.3".to_string(),
        });
        let _ = bag.push(CompatDiag {
            code: CompatCode::CellMissing,
            what: "缺格".to_string(),
            why: String::new(),
            fix: "补测 1.2.3".to_string(),
        });
        let _ = bag.push(CompatDiag {
            code: CompatCode::CellMissing,
            what: "缺格".to_string(),
            why: "未测".to_string(),
            fix: String::new(),
        });
        let names: Vec<&'static str> = bag.downgraded.iter().map(|d| d.missing).collect();
        let all = names.contains(&"what") && names.contains(&"why") && names.contains(&"fix");
        set.add(
            "W06-诊断-三要素三个缺失方向各自降级",
            bag.accepted.is_empty() && bag.downgraded.len() == 3 && all,
            "",
        );
    }
    {
        // 降级项必须**保留**原诊断（现象/归因/处置一个都不能丢）。
        //
        // 只数降级条数不够：一个把降级项内容清空的实现，条数照样对，用户看到的却是
        // 一条空壳——降级本是为了"诊断要诚实"，丢了内容的降级比不降级更坏
        // （看上去像查过了，其实什么线索都没留下）。
        let mut bag = DiagBag::new();
        let _ = bag.push(CompatDiag {
            code: CompatCode::CellMissing,
            what: "矩阵缺格 1.2.3".to_string(),
            why: "未实测".to_string(),
            fix: String::new(),
        });
        let kept = bag
            .downgraded
            .first()
            .map(|d| {
                d.diag.code == CompatCode::CellMissing
                    && d.diag.what.contains("1.2.3")
                    && d.diag.why == "未实测"
            })
            .unwrap_or(false);
        set.add(
            "W06-诊断-降级项保留原诊断内容",
            kept && bag.accepted.is_empty(),
            "",
        );
    }
    {
        // 指引点名必须**双向**成立：点名了要真，不点名也必须假。
        //
        // 只测"该点名的点上了"，把点名检查改成恒真（"只要 fix 非空就算点名"）
        // 判据就全绿——空话指引（"请检查版本"）会畅通无阻，用户仍不知道该改哪个。
        let honest = CompatDiag {
            code: CompatCode::HostIncompatible,
            what: "宿主越界".to_string(),
            why: "作者已明示不支持".to_string(),
            fix: "把宿主升到 2.0.0 或让作者扩上界".to_string(),
        };
        let vague = CompatDiag {
            code: CompatCode::HostIncompatible,
            what: "宿主越界".to_string(),
            why: "作者已明示不支持".to_string(),
            fix: "请检查版本".to_string(),
        };
        set.add(
            "W06-诊断-空话指引不得算作点名",
            honest.fix_names("2.0.0") && !vague.fix_names("2.0.0") && !vague.fix_names("1.2.3"),
            "",
        );
    }
    {
        // 读屏播报须含三要素（无障碍替述）。
        let d = CompatDiag {
            code: CompatCode::HostIncompatible,
            what: "宿主越界".to_string(),
            why: "作者已明示不支持".to_string(),
            fix: "升到 2.0.0".to_string(),
        };
        let s = d.spoken();
        set.add(
            "W06-诊断-播报含现象归因处置三要素",
            s.contains("宿主越界") && s.contains("作者已明示不支持") && s.contains("升到 2.0.0"),
            "",
        );
    }
    {
        // 诊断码与判别值解耦：wire() 稳定且互不重复。
        let codes = [
            CompatCode::SemVerMalformed,
            CompatCode::SemVerLeadingZero,
            CompatCode::HostRangeInverted,
            CompatCode::HostUnknown,
            CompatCode::PluginUnknown,
            CompatCode::CellConflict,
            CompatCode::CellUnknownForbidden,
            CompatCode::CellMissing,
            CompatCode::HostIncompatible,
            CompatCode::PlatformUnsupported,
            CompatCode::MigrationExpired,
            CompatCode::DeprecationOutOfRange,
            CompatCode::ScopeContradiction,
            CompatCode::ScopeAlreadyBroken,
            CompatCode::FabricatedCompat,
            CompatCode::TooManyUsers,
        ];
        let mut uniq = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i].wire() == codes[j].wire() {
                    uniq = false;
                }
            }
            if !codes[i].wire().starts_with("E06_") {
                uniq = false;
            }
        }
        set.add("W06-诊断-十六个码互异且前缀一致", uniq, "");
    }
    {
        // 处置方向相反的状态不得共用码（缺格 vs 不兼容；暂缓 vs 拦截）。
        set.add(
            "W06-诊断-缺格与不兼容分码、暂缓与拦截分码",
            CompatCode::CellMissing.wire() != CompatCode::CellConflict.wire()
                && CompatCode::CellMissing.wire() != CompatCode::HostIncompatible.wire(),
            "",
        );
    }

    // ---- 判据六：性能（实测真实工作量，非自证式算术）----
    {
        // 版本扫描与矩阵规模无关：空表与满表上各解析一次，工作量相同。
        let src = "3.14.159";
        let mut small = compliant();
        small.matrix.plugins.clear();
        small.matrix.hosts.clear();
        let before_small = small.matrix.work.version_scans;
        let mut big = compliant();
        let before_big = big.matrix.work.version_scans;
        let _ = small.scan_version(src);
        let _ = big.scan_version(src);
        set.add(
            "W06-性能-版本扫描工作量与矩阵规模无关",
            big.matrix.work.version_scans - before_big
                == small.matrix.work.version_scans - before_small,
            "",
        );
    }
    {
        // 查单格不得扫全表：连续查同一格 N 次，格访问恒为 N。
        let mut e = compliant();
        let host = SemVer::new(1, 2, 0);
        let before = e.matrix.work.cell_reads;
        for _ in 0..16 {
            let _ = e.matrix.probe("demo.theme", &host, Platform::Linux);
        }
        let reads = e.matrix.work.cell_reads - before;
        set.add("W06-性能-查单格不扫全表（格访问恰为次数）", reads == 16, "");
    }
    {
        // 全表清点的格访问量与格数成正比（线性而非平方）。
        let mut e = compliant();
        let before = e.matrix.work.cell_reads;
        let total = e.matrix.cell_count();
        let plugins = e.matrix.plugins.clone();
        let hosts = e.matrix.hosts.clone();
        for p in plugins.iter() {
            for h in hosts.iter() {
                for plat in Platform::all() {
                    let _ = e.matrix.probe(p, h, plat);
                }
            }
        }
        let reads = e.matrix.work.cell_reads - before;
        // 允许 0 次额外格访问（probe 不写格）；线性判据：读取量恰等于探测次数。
        set.add("W06-性能-全表探测格访问线性（不平方）", reads == total as u64, "");
    }
    {
        // 键扫描随表规模线性（不是常数假象）。
        //
        // 两个计数陷阱，都必须避开：
        // ① 计数口径：一次 `probe` 的键扫描 = 插件表扫描 + 宿主表扫描，两段都计。
        //    demo.theme 在插件表第 0 位、1.0.0 在宿主表第 0 位 → 1 + 1 = 2；
        //    demo.chart 在插件表第 1 位 → 2 + 1 = 3。只数插件表那一段，判据就会在
        //    实现悄悄少扫一半宿主表时全绿。
        // ② 基线：合规底座建表时已扫过键，绝对值带一大截建表基线；必须取**增量**，
        //    否则基线一变判据就飘。
        let mut e = compliant();
        let host = SemVer::new(1, 0, 0);
        let base = e.matrix.work.key_scans;
        let _ = e.matrix.probe("demo.theme", &host, Platform::Linux);
        let first_scan = e.matrix.work.key_scans - base;
        let _ = e.matrix.probe("demo.chart", &host, Platform::Linux);
        let second_scan = e.matrix.work.key_scans - base - first_scan;
        set.add(
            "W06-性能-键扫描随插件序线性增长",
            first_scan == 2 && second_scan == 3,
            "",
        );
    }
    {
        // 拦截只取决于**目标格**的结论，不受旁格影响。
        //
        // 两只引擎的声明完全相同，只差"demo.chart 在 1.0.0/Linux 那格有没有实测
        // 结论"：a 全格兼容故放行，b 该格实测不兼容故拦截。若实现是拿全表聚合成
        // 一个总判定，两只引擎就会给出同一个结论，这条判据立刻变红。
        //
        // 底座取 `half_filled()`：合规底座那格已记为兼容，往里写"不兼容"会被
        // 实测冲突闸拒掉（闸门行为正确），格仍是兼容，判据就变成在测别的东西。
        let mut a = half_filled();
        let mut b = half_filled();
        let _ = b.matrix.record("demo.chart", &SemVer::new(1, 0, 0), Platform::Linux, CellState::Incompatible);
        let _ = a.matrix.record("demo.chart", &SemVer::new(1, 0, 0), Platform::Linux, CellState::Compatible);
        let ga = a.admit("demo.chart", &SemVer::new(1, 0, 0), Platform::Linux);
        let gb = b.admit("demo.chart", &SemVer::new(1, 0, 0), Platform::Linux);
        set.add(
            "W06-性能-拦截判定只取决于目标格结论",
            ga.gate == Gate::Admit
                && gb.gate == Gate::Blocked
                && ga.verdict == Verdict::Compatible
                && gb.verdict == Verdict::Incompatible,
            "",
        );
    }

    // ---- 判据七：可访问性出口与跨批对接 ----
    {
        // 矩阵渲染不得把缺格显示成兼容符号。
        let e = half_filled();
        let s = render_matrix(&e.matrix);
        let ok = s.contains('?') && s.contains("缺格不是兼容") && s.contains('+');
        set.add("W06-出口-矩阵渲染区分缺格与兼容", ok, "");
    }
    {
        // 读屏播报缺格为"未实测"，不得说成兼容。
        let e = half_filled();
        let s = matrix_spoken(&e.matrix);
        let ok = s.contains("未实测") && s.contains("缺格") && !s.contains("缺格 兼容");
        set.add("W06-出口-矩阵读屏播报缺格为未实测", ok, "");
    }
    {
        // 跨批对接：消费 F4602 口径（闭区间）+ 对接 F4607 冻结语义版本声明。
        let ok = COMPAT_VERSION.starts_with("E06") && SEMVER_DOC.contains("构建元数据")
            && DUAL_PERIOD_DOC.contains("at+2");
        set.add("W06-对接-版本口径与双版期承诺显式登记", ok, "");
    }
    {
        // 兼容矩阵可复现：同一夹具两次建表覆盖统计一致（无隐式随机源）。
        let a = compliant();
        let b = compliant();
        set.add("W06-对接-同夹具两次建表结论一致", a.matrix.coverage() == b.matrix.coverage(), "");
    }

    set
}

/// 供 `Result` 兜底取值用的辅助（无 `unwrap_or` 语义歧义）。
trait UnwrapOrDiag {
    /// 取值或兜底诊断。
    fn unwrap_err_or(self) -> CompatDiag;
}

impl UnwrapOrDiag for Result<SemVer, CompatDiag> {
    fn unwrap_err_or(self) -> CompatDiag {
        match self {
            Ok(_) => CompatDiag {
                code: CompatCode::SemVerMalformed,
                what: "预期解析失败却成功".to_string(),
                why: "判据前提错误：这条输入本应被拒绝".to_string(),
                fix: "修正该判据的输入".to_string(),
            },
            Err(e) => e,
        }
    }
}


#[doc(hidden)]
pub fn test_engine_half() -> CompatEngine { half_filled() }

#[doc(hidden)]
pub fn test_engine_compliant() -> CompatEngine { compliant() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_orders_numerically() {
        let a = parse_semver("2.10.0").unwrap_or(SemVer::new(0, 0, 0));
        let b = parse_semver("2.9.0").unwrap_or(SemVer::new(0, 0, 0));
        assert!(a > b, "2.10.0 须大于 2.9.0（按数值而非字典序）");
    }

    #[test]
    fn leading_zero_is_a_dedicated_code() {
        let d = parse_semver("01.0.0");
        assert_eq!(d.unwrap_err_or().code, CompatCode::SemVerLeadingZero);
    }

    #[test]
    fn build_metadata_does_not_affect_precedence() {
        let a = parse_semver("1.0.0+a").unwrap_or(SemVer::new(0, 0, 0));
        let b = parse_semver("1.0.0+b").unwrap_or(SemVer::new(0, 0, 0));
        assert_eq!(a, b);
    }

    #[test]
    fn missing_cell_is_held_not_admitted() {
        let mut e = half_filled();
        let a = e.admit("demo.theme", &SemVer::new(2, 0, 0), Platform::Linux);
        assert_eq!(a.gate, Gate::Held);
        assert_eq!(a.verdict, Verdict::Unknown);
        assert!(!e.matrix.schedule.is_empty(), "缺格必须进补测排程");
    }

    #[test]
    fn boundary_host_is_not_intercepted() {
        let mut e = compliant();
        with_deprecation(&mut e);
        let end = e.deps[0].grace_end();
        let host = e.matrix.hosts[end as usize].clone();
        let a = e.admit("demo.theme", &host, Platform::Linux);
        assert_ne!(a.gate, Gate::Blocked, "边界格属迁移期内，不得提前拦截");
    }

    #[test]
    fn migration_remaining_is_exact() {
        let d = vec![Deprecation::new("a", 0, vec!["p".to_string()])];
        let end = d[0].grace_end();
        for cur in 0..=(end + 3) {
            let v = migration_view(&d, cur);
            let want = if cur > end { 0 } else { end - cur };
            assert_eq!(v.rows[0].remaining_hosts, want, "宿主序 {} 的剩余量须精确", cur);
        }
    }

    #[test]
    fn fabricated_compat_is_detected() {
        let mut e = compliant();
        e.decls[1].host_range.max = SemVer::new(2, 0, 0);
        let _ = e.deprecate(Deprecation::new("chart.old", 0, vec!["demo.chart".to_string()]));
        let found = e.cross_check();
        assert!(found > 0);
        assert!(e.diags.has(CompatCode::ScopeContradiction));
        assert!(e.diags.has(CompatCode::FabricatedCompat));
    }

    #[test]
    fn three_states_partition_all_cells() {
        let e = compliant();
        let (measured, compat, incompat, missing) = e.matrix.coverage();
        assert_eq!(measured + missing, e.matrix.cell_count());
        assert_eq!(measured, compat + incompat);
    }

    #[test]
    fn probe_does_not_scan_whole_table() {
        let mut e = compliant();
        let before = e.matrix.work.cell_reads;
        for _ in 0..8 {
            let _ = e.matrix.probe("demo.theme", &SemVer::new(1, 0, 0), Platform::Linux);
        }
        assert_eq!(e.matrix.work.cell_reads - before, 8);
    }
}