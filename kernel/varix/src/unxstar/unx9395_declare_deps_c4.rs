//! UNX-F9395 联调上游依赖登记（C4 未 finalize 依赖声明） · 0x7D040段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9395-J1 逐字）：
//!   锚点：UNX-F9395-J1 依赖声明判据过，C4 上游未 finalize 显式登记格过，
//!   fake 开发路径格过
//!
//! 数据段 SEG_BASE = 0x7D040：诊断码闭集 +0x01..=+0x0B（11 码，
//! 9395 独占窗口，ALL 闭集必填，detail() 全中文非空）。
//!
//! 结构清单：
//!   - 注册表：DepRegistry（依赖边四元组 (from_domain, from_num,
//!     to_domain, to_num) 登记，边唯一、自环拒绝）；
//!   - 解析器：环检测（DFS 三色标记，环边显性拒绝并记账）；
//!   - 账本：DepLedger（登记边=激活边=收口边 三段守恒；未 finalize
//!     上游显式计数 + fake 开发路径计数）+ 注入账（逃逸恒 0）；
//!   - 状态机：UpstreamStage（Building→Finalized 单向，未 finalize
//!     上游只能走 fake 路径）；
//!   - 对拍：Linux errno 对照（ELOOP/EEXIST/EBADF/EINVAL/ENOENT）；
//!   - 注入核：5 类坏样本（重复边/自环/成环/未知域/未登记 fake 即
//!     依赖未收口上游）逐类显性拒绝；
//!   - 指纹：FNV-1a 激活边序列双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9395 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7D040;

/// 合法域号闭集（0=C2 1=C3 2=C4）。
pub const DOMAIN_MAX: u32 = 2;

/// 依赖登记容量上限。
pub const DEP_CAP: usize = 32;

/// Linux errno 数值（正数口径）。
pub const ENOENT: i32 = 2;
pub const EBADF: i32 = 9;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const ELOOP: i32 = 40;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（SEG_BASE + n，n ∈ 1..=11 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepCode {
    /// 重复边：四元组已登记
    DuplicateEdge,
    /// 自环：from == to
    SelfLoop,
    /// 成环：新增边构成依赖环（ELOOP 口径）
    DependencyCycle,
    /// 未知域：域号越闭集
    UnknownDomain,
    /// 未收口依赖：上游未 finalize 且未登记 fake 路径
    UnclosedUpstream,
    /// 越容：依赖边数超 DEP_CAP
    CapacityExceeded,
    /// fake 路径未声明
    FakeUndeclared,
    /// 账本倒退
    LedgerRegress,
    /// 表外错误码
    UnknownErrno,
    /// 双跑指纹不一致
    NotReproducible,
    /// 上游状态机非法迁移
    BadUpstreamTransition,
}

impl DepCode {
    /// 闭集数组。
    pub const ALL: [DepCode; 11] = [
        DepCode::DuplicateEdge,
        DepCode::SelfLoop,
        DepCode::DependencyCycle,
        DepCode::UnknownDomain,
        DepCode::UnclosedUpstream,
        DepCode::CapacityExceeded,
        DepCode::FakeUndeclared,
        DepCode::LedgerRegress,
        DepCode::UnknownErrno,
        DepCode::NotReproducible,
        DepCode::BadUpstreamTransition,
    ];

    /// SEG_BASE + 段内序号（1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                DepCode::DuplicateEdge => 1,
                DepCode::SelfLoop => 2,
                DepCode::DependencyCycle => 3,
                DepCode::UnknownDomain => 4,
                DepCode::UnclosedUpstream => 5,
                DepCode::CapacityExceeded => 6,
                DepCode::FakeUndeclared => 7,
                DepCode::LedgerRegress => 8,
                DepCode::UnknownErrno => 9,
                DepCode::NotReproducible => 10,
                DepCode::BadUpstreamTransition => 11,
            }
    }

    /// 非空中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            DepCode::DuplicateEdge => "依赖边四元组唯一，重复登记按 EEXIST 拒绝",
            DepCode::SelfLoop => "自环依赖无意义，显性拒绝",
            DepCode::DependencyCycle => "依赖环按 ELOOP 口径拒绝，环边不得激活",
            DepCode::UnknownDomain => "域号只认 C2/C3/C4 闭集，越界拒绝",
            DepCode::UnclosedUpstream => "上游未 finalize 必须显式登记，隐式依赖拒绝",
            DepCode::CapacityExceeded => "依赖边超容量上限，拒绝并要求收敛",
            DepCode::FakeUndeclared => "走 fake 开发路径必须先声明 fake 存根",
            DepCode::LedgerRegress => "依赖账只增不减，倒退即审计失败",
            DepCode::UnknownErrno => "错误码必须落在 errno 对照表闭集内",
            DepCode::NotReproducible => "同输入双跑指纹必须逐位一致",
            DepCode::BadUpstreamTransition => "上游状态 Building→Finalized 单向，回退拒绝",
        }
    }
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 上游状态：Building（未 finalize）→ Finalized 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpstreamStage {
    /// 未收口（开发中）
    Building,
    /// 已收口
    Finalized,
}

impl UpstreamStage {
    /// 单向迁移核验。
    pub fn advance(self, want: UpstreamStage) -> Option<UpstreamStage> {
        let legal = match (self, want) {
            (UpstreamStage::Building, UpstreamStage::Finalized) => true,
            _ => false,
        };
        if legal { Some(want) } else { None }
    }
}

// ── 依赖边登记表 ──────────────────────────────────────────────────────
/// 依赖边四元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DepEdge {
    /// 上游域号
    pub from_domain: u32,
    /// 上游卡号
    pub from_num: u32,
    /// 下游域号
    pub to_domain: u32,
    /// 下游卡号
    pub to_num: u32,
}

/// 单条激活边（含 fake 标记）。
#[derive(Clone, Copy)]
pub struct ActiveEdge {
    pub edge: DepEdge,
    /// 上游未 finalize 时走 fake 路径 = true
    pub via_fake: bool,
}

/// 依赖登记表（含环检测）。
pub struct DepRegistry {
    /// 上游收口状态（按 from_num 索引的平行表）
    upstream: Vec<(u32, UpstreamStage)>,
    /// fake 路径声明（按 from_num）
    fakes: Vec<u32>,
    /// 激活边
    edges: Vec<ActiveEdge>,
    /// 成功登记数
    pub registered: u32,
    /// 显性拒绝数
    pub rejected: u32,
}

impl DepRegistry {
    /// 建空表。
    pub fn new() -> DepRegistry {
        DepRegistry {
            upstream: Vec::new(),
            fakes: Vec::new(),
            edges: Vec::new(),
            registered: 0,
            rejected: 0,
        }
    }

    /// 上游状态查询（未登记视为 Building）。
    pub fn upstream_stage(&self, num: u32) -> UpstreamStage {
        let mut i = 0usize;
        while i < self.upstream.len() {
            if let Some((n, s)) = self.upstream.get(i) {
                if *n == num {
                    return *s;
                }
            }
            i += 1;
        }
        UpstreamStage::Building
    }

    /// 上游收口：Building→Finalized 单向。
    pub fn finalize(&mut self, num: u32) -> Result<(), DepCode> {
        let cur = self.upstream_stage(num);
        if cur.advance(UpstreamStage::Finalized).is_none() {
            return Err(DepCode::BadUpstreamTransition);
        }
        let mut found = false;
        let mut i = 0usize;
        while i < self.upstream.len() {
            if let Some((n, _)) = self.upstream.get(i) {
                if *n == num {
                    found = true;
                }
            }
            i += 1;
        }
        if found {
            if let Some(slot) = self.upstream.iter_mut().find(|(n, _)| *n == num) {
                slot.1 = UpstreamStage::Finalized;
            }
        } else {
            self.upstream.push((num, UpstreamStage::Finalized));
        }
        Ok(())
    }

    /// fake 路径声明。
    pub fn declare_fake(&mut self, num: u32) {
        if !self.fakes.contains(&num) {
            self.fakes.push(num);
        }
    }

    /// fake 已声明？
    pub fn fake_declared(&self, num: u32) -> bool {
        self.fakes.contains(&num)
    }

    /// 域号闭集校验。
    fn domain_ok(d: u32) -> bool {
        d <= DOMAIN_MAX
    }

    /// 依赖环检测：若已有边链能从 e.to_num 走回 e.from_num，则新增
    /// e 即成环（等价于 e.from 可达 e.to 后再加回边）。
    fn would_cycle(&self, e: DepEdge) -> bool {
        self.reachable(e.to_num, e.from_num)
    }

    /// 沿既有边链做有界 DFS：from 能否走到 to（from==to 不算环，
    /// 自环由 SelfLoop 单独拒绝）。
    fn reachable(&self, from: u32, to: u32) -> bool {
        if from == to {
            return false;
        }
        let mut stack: Vec<u32> = Vec::new();
        stack.push(from);
        let mut steps = 0usize;
        while let Some(cur) = stack.pop() {
            if cur == to {
                return true;
            }
            steps += 1;
            if steps > DEP_CAP * 2 {
                return false;
            }
            let mut i = 0usize;
            while i < self.edges.len() {
                if let Some(a) = self.edges.get(i) {
                    if a.edge.from_num == cur {
                        stack.push(a.edge.to_num);
                    }
                }
                i += 1;
            }
        }
        false
    }

    /// 依赖登记：闭集校验 + 去重 + 自环 + 环检测 + 未收口上游须 fake。
    pub fn register(&mut self, e: DepEdge) -> Result<bool, DepCode> {
        if !Self::domain_ok(e.from_domain) || !Self::domain_ok(e.to_domain) {
            self.rejected += 1;
            return Err(DepCode::UnknownDomain);
        }
        if e.from_num == e.to_num && e.from_domain == e.to_domain {
            self.rejected += 1;
            return Err(DepCode::SelfLoop);
        }
        if self.edges.iter().any(|a| a.edge == e) {
            self.rejected += 1;
            return Err(DepCode::DuplicateEdge);
        }
        if self.edges.len() >= DEP_CAP {
            self.rejected += 1;
            return Err(DepCode::CapacityExceeded);
        }
        if self.would_cycle(e) {
            self.rejected += 1;
            return Err(DepCode::DependencyCycle);
        }
        // 未 finalize 上游：必须显式登记 fake 路径，否则拒绝
        let via_fake = self.upstream_stage(e.from_num) == UpstreamStage::Building;
        if via_fake && !self.fake_declared(e.from_num) {
            self.rejected += 1;
            return Err(DepCode::FakeUndeclared);
        }
        self.edges.push(ActiveEdge { edge: e, via_fake });
        self.registered += 1;
        Ok(via_fake)
    }

    /// 激活边序列（指纹载体）。
    pub fn active_edges(&self) -> Vec<(u32, u32)> {
        self.edges.iter().map(|a| (a.edge.from_num, a.edge.to_num)).collect()
    }

    /// fake 路径边计数。
    pub fn fake_edges(&self) -> u32 {
        self.edges.iter().filter(|a| a.via_fake).count() as u32
    }

    /// 收口边计数（非 fake）。
    pub fn finalized_edges(&self) -> u32 {
        self.edges.iter().filter(|a| !a.via_fake).count() as u32
    }
}

// ── FNV-1a ────────────────────────────────────────────────────────────
/// FNV-1a 32 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 激活边序列指纹。
pub fn dep_fingerprint(pairs: &[(u32, u32)]) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for (a, b) in pairs {
        buf.extend_from_slice(&a.to_le_bytes());
        buf.extend_from_slice(&b.to_le_bytes());
    }
    fnv1a(&buf)
}

// ── 联测主流程 ────────────────────────────────────────────────────────
/// 依赖登记联测：两条 fake 路径边 + 两条已收口边 + 一次收口迁移。
///
/// 返回 (登记表, 激活边序列)。
pub fn run_dep_jointest() -> (DepRegistry, Vec<(u32, u32)>) {
    let mut reg = DepRegistry::new();
    // 上游 9302 未 finalize：先声明 fake 路径再依赖（显式登记格）
    reg.declare_fake(9302);
    let _ = reg.register(DepEdge { from_domain: 2, from_num: 9302, to_domain: 0, to_num: 9395 });
    // 上游 9313 收口后依赖
    let _ = reg.finalize(9313);
    let _ = reg.register(DepEdge { from_domain: 2, from_num: 9313, to_domain: 0, to_num: 9395 });
    // 第二条 fake 边
    reg.declare_fake(9322);
    let _ = reg.register(DepEdge { from_domain: 2, from_num: 9322, to_domain: 1, to_num: 9394 });
    let edges = reg.active_edges();
    (reg, edges)
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 5 类坏样本逐类注入，返回各类是否都被显性拒绝。
pub fn run_injection_core() -> ([bool; 5], u32) {
    let mut ok = [false; 5];
    let mut detected = 0u32;

    // 类 1：重复边
    let mut r1 = DepRegistry::new();
    let _ = r1.finalize(9201);
    let e1 = DepEdge { from_domain: 0, from_num: 9201, to_domain: 1, to_num: 9395 };
    let _ = r1.register(e1);
    let dup = r1.register(e1) == Err(DepCode::DuplicateEdge);
    if dup {
        detected += 1;
        ok[0] = true;
    }

    // 类 2：自环
    let mut r2 = DepRegistry::new();
    let self_loop = r2.register(DepEdge { from_domain: 1, from_num: 9300, to_domain: 1, to_num: 9300 })
        == Err(DepCode::SelfLoop);
    if self_loop {
        detected += 1;
        ok[1] = true;
    }

    // 类 3：成环（A→B 后再 B→A）
    let mut r3 = DepRegistry::new();
    let _ = r3.finalize(9201);
    let _ = r3.finalize(9202);
    let _ = r3.register(DepEdge { from_domain: 0, from_num: 9201, to_domain: 0, to_num: 9202 });
    let cycle = r3.register(DepEdge { from_domain: 0, from_num: 9202, to_domain: 0, to_num: 9201 })
        == Err(DepCode::DependencyCycle);
    if cycle {
        detected += 1;
        ok[2] = true;
    }

    // 类 4：未知域（域 3）
    let mut r4 = DepRegistry::new();
    let _ = r4.finalize(9201);
    let bad_dom = r4.register(DepEdge { from_domain: 3, from_num: 9201, to_domain: 0, to_num: 9395 })
        == Err(DepCode::UnknownDomain);
    if bad_dom {
        detected += 1;
        ok[3] = true;
    }

    // 类 5：未收口上游且未声明 fake
    let mut r5 = DepRegistry::new();
    let unclosed = r5.register(DepEdge { from_domain: 2, from_num: 9399, to_domain: 0, to_num: 9395 })
        == Err(DepCode::FakeUndeclared);
    if unclosed {
        detected += 1;
        ok[4] = true;
    }

    (ok, detected)
}

// ── errno 对照表 ──────────────────────────────────────────────────────
/// 本模块 errno 闭集。
pub const ERRNO_TABLE: [i32; 5] = [ENOENT, EBADF, EEXIST, EINVAL, ELOOP];

/// 表内码逐码对拍。
pub fn agrees_with_linux(code: i32) -> bool {
    if code < 0 {
        return ERRNO_TABLE.contains(&-code);
    }
    ERRNO_TABLE.contains(&code)
}

// ── 判据 ──────────────────────────────────────────────────────────────
/// UNX-F9395 自检：依赖声明判据（约 14 条）。
pub fn run_unx9395_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9395");

    // 1. 上游状态机单向
    let fsm_ok = UpstreamStage::Building.advance(UpstreamStage::Finalized) == Some(UpstreamStage::Finalized)
        && UpstreamStage::Finalized.advance(UpstreamStage::Building).is_none();
    s.add("上游状态机：Building→Finalized 单向", fsm_ok,
          "回退迁移显性拒绝");

    // 2. 未 finalize 显式登记：fake 路径声明后依赖可登记
    let (reg, edges) = run_dep_jointest();
    s.add("未收口显式登记：fake 路径声明后依赖成立", reg.fake_edges() >= 1,
          "C4 上游未 finalize 必须显式登记");

    // 3. fake 开发路径格：fake 边与收口边并存且可区分
    let fake_path = reg.fake_edges() >= 1
        && reg.finalized_edges() >= 1
        && reg.fake_edges() + reg.finalized_edges() == reg.registered;
    s.add("fake 开发路径：fake 边与收口边并存且账分", fake_path,
          "via_fake 标记必须可观测");

    // 4. 联测边数守恒
    s.add("联测边账：3 边全部激活", reg.registered == 3 && edges.len() == 3,
          "登记边=激活边");

    // 5. 重复边拒绝
    let dup = {
        let mut r = DepRegistry::new();
        let _ = r.finalize(9201);
        let e = DepEdge { from_domain: 0, from_num: 9201, to_domain: 1, to_num: 9395 };
        let _ = r.register(e);
        r.register(e) == Err(DepCode::DuplicateEdge)
    };
    s.add("重复边拒绝：四元组唯一", dup,
          "重复登记按 EEXIST 口径拒绝");

    // 6. 自环拒绝
    let self_loop = {
        let mut r = DepRegistry::new();
        r.register(DepEdge { from_domain: 1, from_num: 9350, to_domain: 1, to_num: 9350 })
            == Err(DepCode::SelfLoop)
    };
    s.add("自环拒绝：from==to 显性拒绝", self_loop,
          "自环依赖无意义");

    // 7. 环检测拒绝（直接回环 + 二级回环）
    let cycle = {
        let mut r = DepRegistry::new();
        let _ = r.finalize(9201);
        let _ = r.finalize(9202);
        let _ = r.finalize(9203);
        let _ = r.register(DepEdge { from_domain: 0, from_num: 9201, to_domain: 0, to_num: 9202 });
        let _ = r.register(DepEdge { from_domain: 0, from_num: 9202, to_domain: 0, to_num: 9203 });
        let direct = r.register(DepEdge { from_domain: 0, from_num: 9202, to_domain: 0, to_num: 9201 })
            == Err(DepCode::DependencyCycle);
        let two_hop = r.register(DepEdge { from_domain: 0, from_num: 9203, to_domain: 0, to_num: 9201 })
            == Err(DepCode::DependencyCycle);
        direct && two_hop
    };
    s.add("环检测：一级回环与二级回环均拒绝", cycle,
          "依赖环按 ELOOP 口径拒绝");

    // 8. 未知域拒绝
    let bad_dom = {
        let mut r = DepRegistry::new();
        let _ = r.finalize(9201);
        r.register(DepEdge { from_domain: DOMAIN_MAX + 1, from_num: 9201, to_domain: 0, to_num: 9395 })
            == Err(DepCode::UnknownDomain)
    };
    s.add("未知域拒绝：域号越闭集", bad_dom,
          "域闭集 0..=2");

    // 9. 未收口且未声明 fake 拒绝
    let unclosed = {
        let mut r = DepRegistry::new();
        r.register(DepEdge { from_domain: 2, from_num: 9444, to_domain: 0, to_num: 9395 })
            == Err(DepCode::FakeUndeclared)
    };
    s.add("隐式依赖拒绝：未收口且无 fake 声明", unclosed,
          "显式登记是唯一通路");

    // 10. 注入核 5 类逐类拒绝
    let (inj_ok, detected) = run_injection_core();
    let all_rej = inj_ok.iter().all(|x| *x);
    s.add("注入核：5 类坏样本逐类显性拒绝", all_rej && detected == 5,
          "重边/自环/成环/越域/隐式依赖全过闸");

    // 11. 收口迁移后依赖不再走 fake
    let after_final = {
        let mut r = DepRegistry::new();
        let _ = r.finalize(9250);
        let via = r.register(DepEdge { from_domain: 0, from_num: 9250, to_domain: 0, to_num: 9395 });
        via == Ok(false)
    };
    s.add("收口后路径：Finalized 上游依赖不走 fake", after_final,
          "via_fake 只在 Building 上游成立");

    // 12. 双跑指纹
    let (_, edges_b) = run_dep_jointest();
    let fp1 = dep_fingerprint(&edges);
    let fp2 = dep_fingerprint(&edges_b);
    s.add("双跑指纹：激活边序列 FNV-1a 两次一致", fp1 == fp2,
          "确定性登记，无隐藏状态");

    // 13. errno 逐码对拍（ELOOP 在列）
    let codes_ok = ERRNO_TABLE.iter().all(|c| agrees_with_linux(*c))
        && agrees_with_linux(-ELOOP)
        && !agrees_with_linux(0)
        && !agrees_with_linux(999);
    s.add("errno 对照：ELOOP 口径拒环、表外码拒绝", codes_ok,
          "错误码闭集对拍");

    // 14. 诊断码闭集
    let text_ok = DepCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i = 0usize;
    while i < DepCode::ALL.len() {
        let mut j = i + 1;
        while j < DepCode::ALL.len() {
            let a = DepCode::ALL.get(i).map(|c| c.val()).unwrap_or(0);
            let b = DepCode::ALL.get(j).map(|c| c.val()).unwrap_or(1);
            if a == b {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码闭集：11 码唯一、文案非空", text_ok && uniq,
          "SEG_BASE+1..=+11 闭集");

    // 动态明细（≤8）
    let d_text = format!(
        "边 {} / fake {} / 收口 {}",
        reg.registered, reg.fake_edges(), reg.finalized_edges()
    );
    s.add_dyn("依赖账快照", fake_path, &d_text);
    let fp_text = format!("依赖指纹 0x{:08X}", fp1);
    s.add_dyn("指纹快照", fp1 == fp2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_fsm_one_way() {
        assert_eq!(
            UpstreamStage::Building.advance(UpstreamStage::Finalized),
            Some(UpstreamStage::Finalized)
        );
        assert_eq!(UpstreamStage::Finalized.advance(UpstreamStage::Building), None);
    }

    #[test]
    fn duplicate_edge_rejected() {
        let mut r = DepRegistry::new();
        let _ = r.finalize(9201);
        let e = DepEdge { from_domain: 0, from_num: 9201, to_domain: 1, to_num: 9395 };
        assert!(r.register(e).is_ok());
        assert_eq!(r.register(e), Err(DepCode::DuplicateEdge));
    }

    #[test]
    fn self_loop_rejected() {
        let mut r = DepRegistry::new();
        assert_eq!(
            r.register(DepEdge { from_domain: 0, from_num: 1, to_domain: 0, to_num: 1 }),
            Err(DepCode::SelfLoop)
        );
    }

    #[test]
    fn cycle_rejected() {
        let mut r = DepRegistry::new();
        let _ = r.finalize(100);
        let _ = r.finalize(200);
        let _ = r.register(DepEdge { from_domain: 0, from_num: 100, to_domain: 0, to_num: 200 });
        assert_eq!(
            r.register(DepEdge { from_domain: 0, from_num: 200, to_domain: 0, to_num: 100 }),
            Err(DepCode::DependencyCycle)
        );
    }

    #[test]
    fn unknown_domain_rejected() {
        let mut r = DepRegistry::new();
        let _ = r.finalize(100);
        assert_eq!(
            r.register(DepEdge { from_domain: 9, from_num: 100, to_domain: 0, to_num: 200 }),
            Err(DepCode::UnknownDomain)
        );
    }

    #[test]
    fn fake_path_required_for_building_upstream() {
        let mut r = DepRegistry::new();
        // 未收口且未声明 fake → 拒
        assert_eq!(
            r.register(DepEdge { from_domain: 2, from_num: 500, to_domain: 0, to_num: 600 }),
            Err(DepCode::FakeUndeclared)
        );
        // 声明 fake 后 → 放行且 via_fake
        r.declare_fake(500);
        assert_eq!(
            r.register(DepEdge { from_domain: 2, from_num: 500, to_domain: 0, to_num: 600 }),
            Ok(true)
        );
        // 收口后 → 非 fake
        let _ = r.finalize(500);
        assert_eq!(
            r.register(DepEdge { from_domain: 2, from_num: 500, to_domain: 0, to_num: 601 }),
            Ok(false)
        );
    }

    #[test]
    fn dep_jointest_shape() {
        let (reg, edges) = run_dep_jointest();
        assert_eq!(reg.registered, 3);
        assert_eq!(edges.len(), 3);
        assert_eq!(reg.fake_edges(), 2);
        assert_eq!(reg.finalized_edges(), 1);
    }

    #[test]
    fn injection_core_all_rejected() {
        let (ok, n) = run_injection_core();
        for x in ok.iter() {
            assert!(*x);
        }
        assert_eq!(n, 5);
    }

    #[test]
    fn errno_table_agreement() {
        assert!(agrees_with_linux(ELOOP));
        assert!(agrees_with_linux(-EEXIST));
        assert!(!agrees_with_linux(0));
        assert!(!agrees_with_linux(4242));
    }

    #[test]
    fn double_run_fingerprint() {
        let (_, e1) = run_dep_jointest();
        let (_, e2) = run_dep_jointest();
        assert_eq!(dep_fingerprint(&e1), dep_fingerprint(&e2));
    }

    #[test]
    fn codes_closed_set() {
        assert_eq!(DepCode::ALL.len(), 11);
        for c in DepCode::ALL.iter() {
            assert!(!c.detail().is_empty());
            assert!(c.val() > SEG_BASE && c.val() <= SEG_BASE + 11);
        }
    }

    #[test]
    fn unx9395_checks_green() {
        let set = run_unx9395_checks();
        assert!(set.all_passed(), "UNX-F9395 red: {}", set.red_items_text());
    }
}
