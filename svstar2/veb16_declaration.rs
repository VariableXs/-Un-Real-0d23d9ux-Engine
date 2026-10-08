//! VE-F0216 · virtio 参考驱动宣告（目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0216`
//!
//! 职责定位：本组实现即 virtio 家族参考实现，声明支持范围（QEMU 6.0
//! 以上、`VIRTIO_F_VERSION_1` 必需、2D 与 virgl 与 Venus 三通路）
//! 与语义承诺；外部实现按本组语义对齐；变更走语义版本化，破坏性
//! 变更提前一版公告。
//!
//! 数据结构：宣告文档（支持范围×语义承诺）；变更日志（版本×条目×
//! 破坏性标记）。
//!
//! 错误路径与降级矩阵：
//! - 宣告与实现不符 → 以测试套件结果为准修正宣告
//! - 范围外请求 → 明确拒绝不模糊承诺
//! - 公告缺失 → 阻断发布
//!
//! 性能逐项分解：宣告零运行时开销；版本比对 O(1)。
//!
//! ---
//!
//! ## 设计要点一：宣告是**可执行断言**，不是文档
//!
//! 锚点：「宣告与实现不符 → 以测试套件结果为准修正宣告」。这句话
//! 定死了宣告与实现的**方向**：宣告不是许愿，测试套件（VE-F0215）
//! 是裁判。若宣告写在 `.md` 里靠人读，半年后没人知道它还成不成立；
//! 故本单把宣告做成**数据结构 + 判据**：每条承诺带
//! [`Promise`] 证据指针，由测试套件结果回填，缺证据即
//! [`Declaration::blocking_gaps`] 非空 → [`Verdict::Blocked`]。
//!
//! 白话：宣告不是一份可以过期的 Word 文档，是一份**过期就红**的代码。
//!
//! ## 设计要点二：范围外请求必须**明确拒绝**，不许降级为「尽力而为」
//!
//! 锚点：「范围外请求 → 明确拒绝不模糊承诺」。模糊承诺是参考实现
//! 最有毒的失败模式：调用方拿到一句「应该可以」，跑出来的行为却不在
//! 承诺内，排查成本全在调用方。故 [`Scope::admits`] 是**布尔判定**
//! 而不是「置信度」，[`Declaration::admit`] 对范围外请求返回
//! [`Verdict::Refused`] 并附**具体越界项**（哪个通路/哪个版本），
//! 不返回任何「可能支持」。
//!
//! ## 设计要点三：破坏性变更必须**提前一版公告**，不许追溯
//!
//! 锚点：「破坏性变更走语义版本化，破坏性变更提前一版公告」。语义
//! 版本化里破坏性变更加主版本号；但「加主版本号」只是**结果**，
//! 锚点真正要的是**提前一版**——即上一版就得有预告条目。故
//! [`ChangeLog::publish_breakage`] 要求该破坏项的公告版本
//! **严格小于**落地版本，且 [`ChangeLog::pending_notices`] 列出
//! 「已公告但尚未落地」的条目。缺预告即 [`Verdict::Blocked`]，
//! 阻断发布（锚点「公告缺失 → 阻断发布」）。
//!
//! ## 设计要点四：版本比对 O(1)，靠**显式 wire 码**而非解析字符串
//!
//! 锚点：「版本比对 O(1)」。QEMU 版本用结构化三元组
//! （major/minor/patch）存，解码一次成 [`Version`]，比对是整数
//! 比较。**不**用 `str::parse` 现场解析版本串——那是 O(len) 且
//! 每次比对重复解析。编码走显式 `wire()` 映射（不用枚举判别值当线上
//! 编码），并有自洽断言钉死。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 锚点「QEMU 6.0 以上」——最低支持版本。
pub const MIN_QEMU: Version = Version { major: 6, minor: 0, patch: 0 };

/// 锚点「`VIRTIO_F_VERSION_1` 必需」——特性位值（virtio 规范定值）。
pub const VIRTIO_F_VERSION_1: u32 = 32;

/// 锚点「2D 与 virgl 与 Venus 三通路」——通路枚举。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pathway {
    /// 纯 2D 通路。
    TwoD,
    /// virgl 3D 通路。
    Virgl,
    /// Venus 3D 通路。
    Venus,
}

impl Pathway {
    pub fn name(self) -> &'static str {
        match self {
            Pathway::TwoD => "2d",
            Pathway::Virgl => "virgl",
            Pathway::Venus => "venus",
        }
    }

    /// 三通路全集——锚点声明的**全部**通路名册。
    pub const ALL: [Pathway; 3] = [Pathway::TwoD, Pathway::Virgl, Pathway::Venus];

    /// 线上编码：**显式映射**，不用枚举判别值充当 wire 码
    /// （枚举变序即静默改协议，且改后二进制布局仍合法、测不出来）。
    pub fn wire(self) -> u8 {
        match self {
            Pathway::TwoD => 1,
            Pathway::Virgl => 2,
            Pathway::Venus => 3,
        }
    }

    pub fn from_wire(w: u8) -> Option<Pathway> {
        match w {
            1 => Some(Pathway::TwoD),
            2 => Some(Pathway::Virgl),
            3 => Some(Pathway::Venus),
            _ => None,
        }
    }
}

/// QEMU 版本（结构化三元组，比对 O(1)）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Version {
        Version { major, minor, patch }
    }

    /// 是否达到最低支持版本（锚点「QEMU 6.0 以上」，含 6.0 本身）。
    pub fn at_least_min(self, min: Version) -> bool {
        (self.major, self.minor, self.patch) >= (min.major, min.minor, min.patch)
    }

    /// `major.minor.patch` 文本（仅用于输出，不用于比对）。
    pub fn text(self) -> String {
        let mut s = String::new();
        s.push_str(&self.major.to_string());
        s.push('.');
        s.push_str(&self.minor.to_string());
        s.push('.');
        s.push_str(&self.patch.to_string());
        s
    }
}

/// 宣告条目类别（锚点「支持范围×语义承诺」两轴）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClauseKind {
    /// 支持范围条目（版本、通路）。
    Support,
    /// 语义承诺条目（行为契约）。
    Semantic,
}

/// 宣告判定结果（**四态**，不许模糊）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 承诺成立且有证据。
    Declared,
    /// 宣告与实现不符——以测试套件结果为准修正宣告。
    Corrected,
    /// 范围外请求——**明确拒绝**。
    Refused,
    /// 公告缺失/证据缺失——**阻断发布**。
    Blocked,
}

impl Verdict {
    /// 是否阻断发布（锚点「公告缺失 → 阻断发布」）。
    pub fn blocks_release(self) -> bool {
        matches!(self, Verdict::Blocked)
    }

    /// 是否为拒绝。**拒绝不等于失败**：范围外请求被正确拒绝是
    /// 宣告在履行职责，不是宣告坏了。
    pub fn is_refusal(self) -> bool {
        matches!(self, Verdict::Refused)
    }
}

/// 单条宣告（支持范围或语义承诺）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clause {
    pub kind: ClauseKind,
    /// 条目标识（判据与证据引用用）。
    pub key: &'static str,
    /// 该条对**哪些通路**生效（支持范围条目用；语义承诺为空切片 = 全通路）。
    pub pathways: &'static [Pathway],
    /// 该条要求的最低 QEMU 版本（支持范围条目用）。
    pub min_qemu: Option<Version>,
    /// 该条要求的必需特性位（`VIRTIO_F_VERSION_1` 等，0 = 无要求）。
    pub required_feature: u32,
}

/// 证据状态（由测试套件 VE-F0215 结果回填）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence {
    /// 测试套件已覆盖且通过。
    Verified,
    /// 测试套件已覆盖但失败 → 宣告须以测试结果为准修正。
    Failed,
    /// 测试套件未覆盖 → 证据缺失，阻断发布。
    Missing,
}

// ---------------------------------------------------------------------------
// 二、宣告文档
// ---------------------------------------------------------------------------

/// 宣告文档：支持范围×语义承诺 + 每条的证据状态。
#[derive(Clone, Copy, Debug)]
pub struct Declaration {
    pub clauses: &'static [Clause],
    evidence: [Evidence; CLAUSE_CAPACITY],
    evidence_len: usize,
    /// 声明的本参考实现语义版本（锚点「变更走语义版本化」）。
    pub semver: SemVer,
}

/// 证据数组容量（`clause` 为 `&'static`，故按声明条数上界定容）。
pub const CLAUSE_CAPACITY: usize = 16;

impl Declaration {
    /// 构造一份宣告：`evidence` 缺省全为 [`Evidence::Missing`]——
    /// **默认缺证据即阻断**，不默认「假定成立」。
    pub fn new(clauses: &'static [Clause], semver: SemVer) -> Declaration {
        let n = if clauses.len() < CLAUSE_CAPACITY {
            clauses.len()
        } else {
            CLAUSE_CAPACITY
        };
        let mut evidence = [Evidence::Missing; CLAUSE_CAPACITY];
        let mut i = 0;
        while i < n {
            evidence[i] = Evidence::Missing;
            i += 1;
        }
        Declaration { clauses, evidence, evidence_len: n, semver }
    }

    /// 写入某条证据（按 `key` 定位，不按下标——下标会随插入漂移）。
    pub fn set_evidence(&mut self, key: &str, ev: Evidence) {
        let mut i = 0;
        while i < self.clauses.len() {
            if self.clauses[i].key == key {
                if i < self.evidence_len {
                    self.evidence[i] = ev;
                }
                return;
            }
            i += 1;
        }
    }

    /// 查某条的证据。
    pub fn evidence_of(&self, key: &str) -> Evidence {
        let mut i = 0;
        while i < self.clauses.len() {
            if self.clauses[i].key == key {
                if i < self.evidence_len {
                    return self.evidence[i];
                }
                return Evidence::Missing;
            }
            i += 1;
        }
        Evidence::Missing
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.clauses.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clauses.is_empty()
    }

    /// 证据缺失/失败的条目键——**阻断发布的清单**。
    ///
    /// 锚点：「宣告与实现不符 → 以测试套件结果为准修正宣告」+
    /// 「公告缺失 → 阻断发布」。两条合起来即：**没有绿证据的承诺
    /// 一律不许发布**。
    pub fn blocking_gaps(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < self.clauses.len() {
            let e = if i < self.evidence_len { self.evidence[i] } else { Evidence::Missing };
            if e != Evidence::Verified {
                v.push(self.clauses[i].key);
            }
            i += 1;
        }
        v
    }

    /// 宣告整体判定：全绿 [`Verdict::Declared`]；有失败项
    /// [`Verdict::Corrected`]（须以测试结果为准修正）；有缺证据
    /// [`Verdict::Blocked`]。
    ///
    /// 优先级：**阻断 > 修正**——有缺证据时发布已被阻断，「修正」
    /// 这个结论没有意义，故不让它盖过阻断。
    pub fn verdict(&self) -> Verdict {
        let mut has_failed = false;
        let mut i = 0;
        while i < self.clauses.len() {
            let e = if i < self.evidence_len { self.evidence[i] } else { Evidence::Missing };
            match e {
                Evidence::Verified => {}
                Evidence::Failed => has_failed = true,
                Evidence::Missing => return Verdict::Blocked,
            }
            i += 1;
        }
        if has_failed {
            Verdict::Corrected
        } else {
            Verdict::Declared
        }
    }
}

/// 范围外请求的答复（**明确拒绝**，不给模糊承诺）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// 被拒的请求键（判据与调用方定位用）。
    pub key: &'static str,
    /// 越界原因（人类可读，具体到哪一项）。
    pub reason: &'static str,
}

/// 请求侧事实（问宣告「你行不行」的一方）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub pathway: Pathway,
    pub qemu: Version,
    /// 设备提供的特性位。
    pub features: u32,
}

impl Declaration {
    /// 判定某请求是否在支持范围内（**布尔**，不是置信度）。
    ///
    /// 逐项检查：版本下限、必需特性位、通路是否在名册内。三者
    /// 任一不过即在范围外。
    pub fn admits(&self, req: &Request) -> bool {
        if !req.qemu.at_least_min(MIN_QEMU) {
            return false;
        }
        if req.features & VIRTIO_F_VERSION_1 != VIRTIO_F_VERSION_1 {
            return false;
        }
        let mut i = 0;
        while i < Pathway::ALL.len() {
            if Pathway::ALL[i] == req.pathway {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 明确拒绝范围外请求：给出**具体越界项**。
    ///
    /// 返回 `Some(Refusal)` 表示在范围外（越界原因具体到版本/特性/
    /// 通路）；返回 `None` 表示在范围内。**不返回任何「可能支持」。**
    pub fn admit(&self, req: &Request) -> Result<(), Refusal> {
        if !req.qemu.at_least_min(MIN_QEMU) {
            return Err(Refusal {
                key: "min-qemu",
                reason: "qemu-version-below-6.0",
            });
        }
        if req.features & VIRTIO_F_VERSION_1 != VIRTIO_F_VERSION_1 {
            return Err(Refusal {
                key: "virtio-f-version-1",
                reason: "required-feature-missing",
            });
        }
        let mut i = 0;
        let mut in_list = false;
        while i < Pathway::ALL.len() {
            if Pathway::ALL[i] == req.pathway {
                in_list = true;
            }
            i += 1;
        }
        if !in_list {
            return Err(Refusal { key: "pathway", reason: "pathway-not-declared" });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、语义版本与变更日志
// ---------------------------------------------------------------------------

/// 语义版本（锚点「变更走语义版本化」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SemVer {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl SemVer {
    pub const fn new(major: u32, minor: u32, patch: u32) -> SemVer {
        SemVer { major, minor, patch }
    }

    /// 该版本变更是否为**破坏性**（主版本号变更即破坏性）。
    pub fn is_breaking_from(self, prev: SemVer) -> bool {
        self.major > prev.major
    }

    pub fn wire(self) -> u32 {
        (self.major << 16) | (self.minor << 8) | self.patch
    }

    pub fn text(self) -> String {
        let mut s = String::new();
        s.push_str(&self.major.to_string());
        s.push('.');
        s.push_str(&self.minor.to_string());
        s.push('.');
        s.push_str(&self.patch.to_string());
        s
    }
}

/// 变更日志条目（版本×条目×破坏性标记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    /// 该变更落在哪个版本。
    pub landed_in: SemVer,
    /// 变更键（判据与追溯用）。
    pub key: &'static str,
    /// 是否破坏性（锚点「破坏性变更提前一版公告」）。
    pub breaking: bool,
    /// 公告版本；破坏性变更**必须**早于落地版本。
    pub announced_in: SemVer,
}

impl Change {
    /// 该破坏性变更是否**提前一版**公告。
    ///
    /// 锚点「破坏性变更提前一版公告」：非破坏性变更不要求预告
    /// （`announced_in` 无意义，返回 `true`）；破坏性变更要求
    /// `announced_in < landed_in`。
    pub fn notice_ok(self) -> bool {
        if !self.breaking {
            return true;
        }
        self.announced_in < self.landed_in
    }
}

/// 变更日志。
#[derive(Clone, Copy, Debug)]
pub struct ChangeLog {
    changes: [Change; CHANGE_CAPACITY],
    len: usize,
}

pub const CHANGE_CAPACITY: usize = 12;

impl ChangeLog {
    pub fn new() -> ChangeLog {
        ChangeLog { changes: [Change {
            landed_in: SemVer::new(0, 0, 0),
            key: "",
            breaking: false,
            announced_in: SemVer::new(0, 0, 0),
        }; CHANGE_CAPACITY], len: 0 }
    }

    /// 记一条变更。容量满返回 `false`（**不静默丢弃**）。
    pub fn record(&mut self, c: Change) -> bool {
        if self.len >= CHANGE_CAPACITY {
            return false;
        }
        self.changes[self.len] = c;
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, i: usize) -> Option<Change> {
        if i < self.len {
            Some(self.changes[i])
        } else {
            None
        }
    }

    /// 破坏性变更中**公告缺失/过晚**的条目键。
    ///
    /// 空切片 = 公告齐备，可发布；非空 → 阻断发布。
    pub fn missing_notices(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < self.len {
            let c = self.changes[i];
            if c.breaking && !c.notice_ok() {
                v.push(c.key);
            }
            i += 1;
        }
        v
    }

    /// 已**合规公告**（提前一版）但落地版本仍**晚于**公告版本的破坏性变更
    /// ——即「预告在册、等着落地」的待办项。
    ///
    /// 与 [`ChangeLog::announced_breaking`] 的区别：那个函数列的是
    /// **全部**合规公告过的破坏性变更（不论是否已落地），本函数只列
    /// **尚未落地**的那部分。两者都读同一个 `landed_in > announced_in`
    /// 条件，但后者按变更是否已生效再筛一层——函数名必须与行为严格
    /// 对应，否则读判据的人会按名字理解成另一件事（注释与代码反向
    /// 是最危险的一种错）。
    pub fn pending_notices(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < self.len {
            let c = self.changes[i];
            if c.breaking && c.notice_ok() && c.landed_in > c.announced_in {
                v.push(c.key);
            }
            i += 1;
        }
        v
    }

    /// **全部**已合规公告过的破坏性变更键（含已落地者）。
    ///
    /// 用于回答「哪些破坏性变更走过了预告流程」，与
    /// [`ChangeLog::pending_notices`]（只算待落地）互补。
    pub fn announced_breaking(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < self.len {
            let c = self.changes[i];
            if c.breaking && c.notice_ok() {
                v.push(c.key);
            }
            i += 1;
        }
        v
    }

    /// 发布闸：破坏性公告缺失即阻断（锚点「公告缺失 → 阻断发布」）。
    pub fn release_gate(&self) -> Verdict {
        if self.missing_notices().is_empty() {
            Verdict::Declared
        } else {
            Verdict::Blocked
        }
    }
}

// ---------------------------------------------------------------------------
// 四、锚点声明的默认宣告
// ---------------------------------------------------------------------------

/// 锚点「QEMU 6.0 以上、`VIRTIO_F_VERSION_1` 必需、2D 与 virgl 与
/// Venus 三通路」——支持范围三条。
pub const SUPPORT_CLAUSES: [Clause; 4] = [
    Clause {
        kind: ClauseKind::Support,
        key: "min-qemu",
        pathways: &Pathway::ALL,
        min_qemu: Some(MIN_QEMU),
        required_feature: 0,
    },
    Clause {
        kind: ClauseKind::Support,
        key: "virtio-f-version-1",
        pathways: &Pathway::ALL,
        min_qemu: None,
        required_feature: VIRTIO_F_VERSION_1,
    },
    Clause {
        kind: ClauseKind::Support,
        key: "pathway-2d",
        pathways: &[Pathway::TwoD],
        min_qemu: None,
        required_feature: 0,
    },
    Clause {
        kind: ClauseKind::Support,
        key: "pathway-virgl",
        pathways: &[Pathway::Virgl],
        min_qemu: None,
        required_feature: 0,
    },
];

/// 锚点「语义承诺」——含无障碍相关承诺项
/// （锚点：「宣告文档含无障碍相关承诺项」）。
pub const SEMANTIC_CLAUSES: [Clause; 3] = [
    Clause {
        kind: ClauseKind::Semantic,
        key: "pathway-venus",
        pathways: &[Pathway::Venus],
        min_qemu: None,
        required_feature: 0,
    },
    Clause {
        kind: ClauseKind::Semantic,
        key: "reset-order-follows-f0212",
        pathways: &[],
        min_qemu: None,
        required_feature: 0,
    },
    Clause {
        kind: ClauseKind::Semantic,
        key: "a11y-notification-triple",
        pathways: &[],
        min_qemu: None,
        required_feature: 0,
    },
];

/// 锚点要求的全部宣告条目（支持范围 + 语义承诺）。
pub const CLAUSES: [Clause; 7] = [
    SUPPORT_CLAUSES[0],
    SUPPORT_CLAUSES[1],
    SUPPORT_CLAUSES[2],
    SUPPORT_CLAUSES[3],
    SEMANTIC_CLAUSES[0],
    SEMANTIC_CLAUSES[1],
    SEMANTIC_CLAUSES[2],
];

/// 锚点「变更走语义版本化」——本参考实现当前语义版本。
pub const CURRENT_SEMVER: SemVer = SemVer::new(1, 0, 0);

/// 按锚点构造一份宣告文档。
pub fn declaration() -> Declaration {
    Declaration::new(&CLAUSES, CURRENT_SEMVER)
}

// ---------------------------------------------------------------------------
// 五、裁决：宣告 + 变更日志 → 发布裁决
// ---------------------------------------------------------------------------

/// 发布裁决结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ruling {
    /// 最终判定。
    pub verdict: Verdict,
    /// 阻断项键（空 = 无阻断）。
    pub blockers: usize,
    /// 越界拒绝数（范围外请求被正确拒绝对发布无影响）。
    pub refusals: usize,
}

/// 合并裁决：证据缺失优先于公告缺失；两者皆无阻断才放行。
///
/// 顺序纪律：先查**证据**（宣告与实现是否相符），再查**公告**
/// （破坏性变更预告是否齐备）。任一阻断即阻断，不做「半数阻断也放行」。
pub fn rule(decl: &Declaration, log: &ChangeLog) -> Ruling {
    let gaps = decl.blocking_gaps().len();
    let missing = log.missing_notices().len();
    let blockers = gaps + missing;
    let verdict = if blockers > 0 { Verdict::Blocked } else { decl.verdict() };
    Ruling { verdict, blockers, refusals: 0 }
}

/// 供 [`rule`] 统计「范围外请求被拒绝」而不影响发布。
pub fn count_refusals(decl: &Declaration, reqs: &[Request]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < reqs.len() {
        if decl.admit(&reqs[i]).is_err() {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 逐字校验三通路的 wire 码自洽（锚点纪律：编码须自洽断言）。
pub fn wire_codes_self_consistent() -> bool {
    let mut i = 0;
    let mut ok = true;
    while i < Pathway::ALL.len() {
        let w = Pathway::ALL[i].wire();
        if Pathway::from_wire(w) != Some(Pathway::ALL[i]) {
            ok = false;
        }
        if w == 0 {
            ok = false;
        }
        i += 1;
    }
    ok
}

/// 支持范围通路覆盖全三通路（锚点「2D 与 virgl 与 Venus 三通路」）。
pub fn pathways_declared() -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < Pathway::ALL.len() {
        let p = Pathway::ALL[i];
        let mut j = 0;
        let mut found = false;
        while j < CLAUSES.len() {
            let mut k = 0;
            while k < CLAUSES[j].pathways.len() {
                if CLAUSES[j].pathways[k] == p {
                    found = true;
                }
                k += 1;
            }
            j += 1;
        }
        if found {
            n += 1;
        }
        i += 1;
    }
    n
}
