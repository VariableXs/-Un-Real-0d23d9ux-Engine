//! VE-F3609 · 创作版本与协作预留（VE-S 域 · 批次 S01 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3609`
//!
//! **判据（锚点原文六条）**：单源版本、三签名预留、显性复述、语义版本、
//! 回溯复用、判据。逐条落位：
//! - **单源版本**：资产版本链复用 F3211 哈希版本模式（跨域单源声明
//!   [`F3211_RESTATED_NOTE`]）——每个版本绑定内容指纹，指纹即身份，
//!   不造第二套版本号体系。[`VersionChain::append`] 只接受指纹变化的
//!   新版本；指纹未变的提交 → [`V09Code::NO_OP_COMMIT`] 拒收（空提交
//!   污染历史）。
//! - **三签名预留**：协作三签名（冲突解决/权限/同步）**冻结 + 实现等级
//!   预留显性**（[`CollabReserve`]）——预留被调用 → 显式未就绪
//!   ([`V09Code::RESERVE_CALLED`]，复述红线实测)，**不是 panic 也不是
//!   静默降级**。三签名各自独立冻结声明（[`Signature::Conflict`]/
//!   [`Signature::Permission`] / [`Signature::Sync`]）。
//! - **显性复述**：预留等级（[`ReserveLevel`]）四级显性——
//!   Frozen（签名冻结）/Stub（接口存在返回未就绪）/Partial（部分可试）
//!   /Ready。当前三签名全部 Frozen，等级本身也是被审计对象。
//! - **语义版本**：资产语义化版本（Major/Minor/Patch）三级
//!   （[`SemVer`]），版本混乱 → 语义修正（[`SemVer::bump`] 只允许
//!   单级递进或对齐，跳跃递进需显式 [`SemVer::force`]）。
//! - **回溯复用**：版本历史回滚复用回滚通道模式——[`VersionChain::rollback_to`]
//!   不删除历史，只移动指针（历史保护：回滚 ≠ 篡改）。
//! - **判据**：`run_ves09_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 预留被调用→显式未就绪（复述红线实测） | [`V09Code::RESERVE_CALLED`]：Stub 接口返回显式错误 |
//! | 版本混乱→语义修正（复用） | [`V09Code::SEMVER_JUMP`]：跳跃递进拦截 |
//! | 回滚失败→P1（复用红线） | [`V09Code::ROLLBACK_MISS`]：目标版本不存在即 P1 |
//! | 协作接口分歧→对齐（复述） | [`V09Code::SIG_DIVERGE`]：三签名版本对齐审计 |
//!
//! **码段**：独占 **0x39xx**。
//!
//! **性能逐项分解（锚点口径）**：版本 O(1)（链尾追加）；预留 O(0)
//! （空转验证不分配）；回溯 O(1)（指针移动）；签名 O(1)（查表）。
//!
//! **跨批对接点（锚点原文三名）**：F3211 单源复述声明；F3347 SDK 对端
//! （[`sdk_contract`]）；F3604 工作流联动（[`workflow_hook`]）——
//! 三点钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟；无全局可变状态。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3609 诊断码段（独占 0x39xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V09Code(pub u16);

impl V09Code {
    /// 空提交（指纹未变）。
    pub const NO_OP_COMMIT: V09Code = V09Code(0x3901);
    /// 预留接口被调用（显式未就绪）。
    pub const RESERVE_CALLED: V09Code = V09Code(0x3902);
    /// 语义版本跳跃递进。
    pub const SEMVER_JUMP: V09Code = V09Code(0x3903);
    /// 回滚目标不存在。
    pub const ROLLBACK_MISS: V09Code = V09Code(0x3904);
    /// 三签名版本分歧。
    pub const SIG_DIVERGE: V09Code = V09Code(0x3905);
    /// 链为空时的非法操作。
    pub const CHAIN_EMPTY: V09Code = V09Code(0x3906);
    /// 版本号重复登记。
    pub const VER_DUPLICATE: V09Code = V09Code(0x3907);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明（带可操作建议）。
    pub fn explain(self) -> &'static str {
        match self {
            V09Code::NO_OP_COMMIT => {
                "空提交：内容指纹未变化。建议：修改后再提交，或使用 amend 显式更新元数据。"
            }
            V09Code::RESERVE_CALLED => {
                "协作预留接口尚未就绪（显式未就绪而非静默降级）。建议：等待协作子系统落地后升级实现等级。"
            }
            V09Code::SEMVER_JUMP => {
                "语义版本跳跃递进（如 1.0.0 直接到 1.5.0）。建议：逐级递进，或用 force 显式声明跳跃理由。"
            }
            V09Code::ROLLBACK_MISS => {
                "回滚目标版本不存在。建议：核对版本号，或先查询链上全部版本。"
            }
            V09Code::SIG_DIVERGE => {
                "协作三签名版本不一致。建议：对齐三签名至同一版本号后重新冻结。"
            }
            V09Code::CHAIN_EMPTY => {
                "版本链为空。建议：先 append 首个版本再执行此操作。"
            }
            V09Code::VER_DUPLICATE => {
                "版本号重复登记。建议：版本号由语义递进自动分配，勿手工重复指定。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 语义版本 ────────────────────────────────────────────────────────

/// 三级语义版本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SemVer {
    /// 重大（不兼容变更）。
    pub major: u32,
    /// 次要（向后兼容新增）。
    pub minor: u32,
    /// 补丁（向后兼容修复）。
    pub patch: u32,
}

impl SemVer {
    /// 1.0.0。
    pub const fn initial() -> SemVer {
        SemVer {
            major: 1,
            minor: 0,
            patch: 0,
        }
    }

    /// 从三元组构造。
    pub const fn new(major: u32, minor: u32, patch: u32) -> SemVer {
        SemVer {
            major,
            minor,
            patch,
        }
    }

    /// 单级递进。返回新版本；跳跃需 [`SemVer::force`]。
    pub fn bump(self, level: BumpLevel) -> SemVer {
        match level {
            BumpLevel::Patch => SemVer {
                patch: self.patch.wrapping_add(1),
                ..self
            },
            BumpLevel::Minor => SemVer {
                minor: self.minor.wrapping_add(1),
                patch: 0,
                ..self
            },
            BumpLevel::Major => SemVer {
                major: self.major.wrapping_add(1),
                minor: 0,
                patch: 0,
                ..self
            },
        }
    }

    /// 是否单级递进（合法）。
    pub fn is_single_step(self, next: SemVer) -> bool {
        let p = self.bump(BumpLevel::Patch);
        let m = self.bump(BumpLevel::Minor);
        let j = self.bump(BumpLevel::Major);
        next == p || next == m || next == j
    }

    /// 显式强制（跳跃递进，调用方须持理由）。
    pub fn force(self, next: SemVer) -> bool {
        // force 只要求 next 严格大于 self
        next.major > self.major
            || (next.major == self.major && next.minor > self.minor)
            || (next.major == self.major
                && next.minor == self.minor
                && next.patch > self.patch)
    }

    /// 渲染 "a.b.c"。
    pub fn render(self) -> String {
        let mut s = String::new();
        s.push_str(&alloc::format!("{}.{}.{}", self.major, self.minor, self.patch));
        s
    }
}

/// 递进级别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BumpLevel {
    /// 补丁。
    Patch,
    /// 次要。
    Minor,
    /// 重大。
    Major,
}

// ── 版本链（F3211 哈希版本模式复用）─────────────────────────────────

/// 一个版本节点。
#[derive(Clone, Debug)]
pub struct VersionNode {
    /// 版本号。
    pub ver: SemVer,
    /// 内容指纹。
    pub fingerprint: u64,
    /// 提交说明。
    pub message: String,
    /// 是否被回滚标记（历史保留）。
    pub rolled_back: bool,
}

/// 版本链（单源复用 F3211 哈希版本模式）。
#[derive(Clone, Debug, Default)]
pub struct VersionChain {
    nodes: Vec<VersionNode>,
    head: usize,
}

impl VersionChain {
    /// 空链。
    pub fn new() -> VersionChain {
        VersionChain {
            nodes: Vec::new(),
            head: 0,
        }
    }

    /// 链长。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 当前版本。
    pub fn current(&self) -> Option<&VersionNode> {
        self.nodes.get(self.head)
    }

    /// 追加版本。指纹未变拒收。
    pub fn append(&mut self, ver: SemVer, fingerprint: u64, message: &str) -> Result<(), V09Code> {
        if let Some(last) = self.nodes.last() {
            if last.fingerprint == fingerprint {
                return Err(V09Code::NO_OP_COMMIT);
            }
            if self.nodes.iter().any(|n| n.ver == ver) {
                return Err(V09Code::VER_DUPLICATE);
            }
            // 语义版本必须单级递进或显式 force（此处链内只收单级）
            if !last.ver.is_single_step(ver) {
                return Err(V09Code::SEMVER_JUMP);
            }
        }
        self.nodes.push(VersionNode {
            ver,
            fingerprint,
            message: String::from(message),
            rolled_back: false,
        });
        self.head = self.nodes.len() - 1;
        Ok(())
    }

    /// 显式追加（允许跳跃递进，调用方持理由）。
    pub fn append_forced(
        &mut self,
        ver: SemVer,
        fingerprint: u64,
        message: &str,
    ) -> Result<(), V09Code> {
        if let Some(last) = self.nodes.last() {
            if last.fingerprint == fingerprint {
                return Err(V09Code::NO_OP_COMMIT);
            }
            if !last.ver.force(ver) {
                return Err(V09Code::SEMVER_JUMP);
            }
        }
        self.nodes.push(VersionNode {
            ver,
            fingerprint,
            message: String::from(message),
            rolled_back: false,
        });
        self.head = self.nodes.len() - 1;
        Ok(())
    }

    /// 回滚到指定版本。只移指针，不删历史。
    pub fn rollback_to(&mut self, ver: SemVer) -> Result<(), V09Code> {
        match self.nodes.iter().position(|n| n.ver == ver) {
            Some(idx) => {
                // 当前及之后的节点标记 rolled_back（历史保留）
                for n in self.nodes.iter_mut().skip(self.head) {
                    n.rolled_back = true;
                }
                self.head = idx;
                Ok(())
            }
            None => Err(V09Code::ROLLBACK_MISS),
        }
    }

    /// 全部版本（含已回滚）。
    pub fn history(&self) -> &[VersionNode] {
        &self.nodes
    }

    /// 活跃版本数（未被回滚标记）。
    pub fn active_count(&self) -> usize {
        self.nodes.iter().filter(|n| !n.rolled_back).count()
    }
}

// ── 协作预留（三签名冻结）───────────────────────────────────────────

/// 协作三签名。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Signature {
    /// 冲突解决策略。
    Conflict,
    /// 权限模型。
    Permission,
    /// 实时同步协议。
    Sync,
}

impl Signature {
    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Signature::Conflict => "冲突解决",
            Signature::Permission => "权限模型",
            Signature::Sync => "实时同步",
        }
    }

    /// 三签名全集。
    pub fn all() -> [Signature; 3] {
        [Signature::Conflict, Signature::Permission, Signature::Sync]
    }
}

/// 预留实现等级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReserveLevel {
    /// 签名冻结（当前态）。
    Frozen,
    /// 接口存在返回未就绪。
    Stub,
    /// 部分可试。
    Partial,
    /// 就绪。
    Ready,
}

impl ReserveLevel {
    /// 是否允许被调用（不报未就绪）。
    pub fn callable(self) -> bool {
        matches!(self, ReserveLevel::Partial | ReserveLevel::Ready)
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            ReserveLevel::Frozen => "冻结",
            ReserveLevel::Stub => "桩接口",
            ReserveLevel::Partial => "部分可用",
            ReserveLevel::Ready => "就绪",
        }
    }
}

/// 一条签名预留记录。
#[derive(Clone, Debug)]
pub struct SignatureRecord {
    /// 签名种类。
    pub kind: Signature,
    /// 实现等级。
    pub level: ReserveLevel,
    /// 签名版本号（对齐审计用）。
    pub sig_version: u32,
    /// 预研结论（留白红线：必须非空）。
    pub pre_research: String,
}

impl SignatureRecord {
    /// 构造（预研结论为空则 panic——但本模块零 panic，改由调用方校验）。
    pub fn new(kind: Signature, sig_version: u32, pre_research: &str) -> Option<SignatureRecord> {
        if pre_research.is_empty() {
            return None;
        }
        Some(SignatureRecord {
            kind,
            level: ReserveLevel::Frozen,
            sig_version,
            pre_research: String::from(pre_research),
        })
    }
}

/// 协作预留（三签名 + 空转验证）。
#[derive(Clone, Debug)]
pub struct CollabReserve {
    records: Vec<SignatureRecord>,
    call_count: u32,
}

impl CollabReserve {
    /// 新建空预留。
    pub fn new() -> CollabReserve {
        CollabReserve {
            records: Vec::new(),
            call_count: 0,
        }
    }

    /// 内置三签名（全部 Frozen，带预研结论）。
    pub fn builtin() -> CollabReserve {
        let mut c = CollabReserve::new();
        SignatureRecord::new(
            Signature::Conflict,
            1,
            "同令牌双编辑冲突预研：采用操作序转换（OT）+ 最后写胜出兜底； \
             冲突类型三类（值冲突/删除冲突/重命名冲突）各有裁决路径。",
        )
        .map(|r| c.records.push(r));
        SignatureRecord::new(
            Signature::Permission,
            1,
            "协作权限预研：owner/editor/viewer 三级；编辑权可细粒度到令牌域。",
        )
        .map(|r| c.records.push(r));
        SignatureRecord::new(
            Signature::Sync,
            1,
            "实时同步预研：变更事件推 + 定期全量对账；断线重连以版本向量对齐。",
        )
        .map(|r| c.records.push(r));
        c
    }

    /// 查询签名等级。
    pub fn level_of(&self, kind: Signature) -> Option<ReserveLevel> {
        self.records.iter().find(|r| r.kind == kind).map(|r| r.level)
    }

    /// 调用预留接口（显式未就绪，绝不静默）。
    pub fn call(&mut self, kind: Signature) -> Result<String, V09Code> {
        self.call_count = self.call_count.wrapping_add(1);
        match self.level_of(kind) {
            Some(level) if level.callable() => Ok(String::from("协作接口调用成功")),
            Some(_) => Err(V09Code::RESERVE_CALLED),
            None => Err(V09Code::RESERVE_CALLED),
        }
    }

    /// 三签名版本对齐审计。
    pub fn audit_alignment(&self) -> Result<(), V09Code> {
        let mut versions = self.records.iter().map(|r| r.sig_version);
        if let Some(first) = versions.next() {
            if versions.any(|v| v != first) {
                return Err(V09Code::SIG_DIVERGE);
            }
        }
        Ok(())
    }

    /// 空转验证：预留调用计数可观测（不隐藏调用行为）。
    pub fn call_count(&self) -> u32 {
        self.call_count
    }

    /// 签名数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 预研结论留白审计：任何签名结论为空即违规。
    pub fn audit_pre_research(&self) -> bool {
        self.records.iter().all(|r| !r.pre_research.is_empty())
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// F3347 SDK 对端契约。
pub fn sdk_contract(chain: &VersionChain) -> (u32, bool) {
    (
        chain.len() as u32,
        chain.current().is_some(),
    )
}

/// F3604 工作流联动钩子：版本提交事件（供工作流订阅）。
pub fn workflow_hook(committed_ver: Option<SemVer>) -> &'static str {
    match committed_ver {
        Some(_) => "版本已提交，工作流可继续",
        None => "无版本变更，工作流保持",
    }
}

/// F3211 单源复述件。
pub const F3211_RESTATED_NOTE: &str =
    "F3211（哈希版本模式）单源复述：内容指纹即版本身份语义按 F3211 锚点复述、本地实现；\
     F3211 落库后由对账钩子接管。";

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3211", "VE-F3347", "VE-F3604"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves09_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3609");

    // 0x3901 空提交拒收
    let mut ch = VersionChain::new();
    ch.append(SemVer::initial(), 100, "first").ok();
    let noop = ch.append(SemVer::new(1, 0, 1), 100, "same fp");
    out.add(
        "0x3901-指纹未变拒收",
        noop.is_err() && noop.unwrap_err() == V09Code::NO_OP_COMMIT,
        "",
    );

    // 0x3902 语义单级递进通过
    let mut ch2 = VersionChain::new();
    ch2.append(SemVer::initial(), 1, "v1").ok();
    let step = ch2.append(SemVer::new(1, 0, 1), 2, "v1.0.1");
    out.add("0x3902-补丁递进合法", step.is_ok(), "");

    // 0x3903 跳跃递进拦截
    let mut ch3 = VersionChain::new();
    ch3.append(SemVer::initial(), 1, "v1").ok();
    let jump = ch3.append(SemVer::new(1, 5, 0), 2, "jump");
    out.add(
        "0x3903-跳跃递进拦截",
        jump.is_err() && jump.unwrap_err() == V09Code::SEMVER_JUMP,
        "",
    );

    // 0x3904 force 显式跳跃
    let mut ch4 = VersionChain::new();
    ch4.append(SemVer::initial(), 1, "v1").ok();
    let forced = ch4.append_forced(SemVer::new(2, 0, 0), 2, "major force");
    out.add("0x3904-force 显式跳跃放行", forced.is_ok(), "");

    // 0x3905 回滚到存在版本
    let mut ch5 = VersionChain::new();
    ch5.append(SemVer::initial(), 1, "v1").ok();
    ch5.append(SemVer::new(1, 0, 1), 2, "v2").ok();
    let rb = ch5.rollback_to(SemVer::initial());
    out.add("0x3905-回滚移指针保留历史", rb.is_ok() && ch5.current().unwrap().ver == SemVer::initial(), "");

    // 0x3906 回滚目标不存在
    let rbmiss = ch5.rollback_to(SemVer::new(9, 9, 9));
    out.add(
        "0x3906-回滚目标缺失报 P1",
        rbmiss.is_err() && rbmiss.unwrap_err() == V09Code::ROLLBACK_MISS,
        "",
    );

    // 0x3907 预留调用显式未就绪
    let mut cr = CollabReserve::builtin();
    let call = cr.call(Signature::Conflict);
    out.add(
        "0x3907-预留调用显式未就绪",
        call.is_err() && call.unwrap_err() == V09Code::RESERVE_CALLED,
        "",
    );

    // 0x3908 三签名对齐
    let align = cr.audit_alignment();
    out.add("0x3908-三签名版本对齐", align.is_ok(), "");

    // 0x3909 预研结论留白审计
    out.add("0x3909-预研结论非留白", cr.audit_pre_research(), "");

    // 0x390a 历史保留（回滚不删）
    out.add("0x390a-回滚后历史仍全量保留", ch5.len() == 2 && ch5.active_count() == 1, "");

    out
}
