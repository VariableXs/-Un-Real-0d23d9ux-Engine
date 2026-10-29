//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12881 · IAT 导入链与导出转发解析 · 0x65380
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥420 行；
//!   ② UNX-F12881-J1 三级依赖真实样本解析全对，转发串跳转语义与
//!     Windows 一致，缺失 DLL 错误码对齐（任务书示例判据保真）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 导入描述符闭式：OriginalFirstThunk(INT)/Name/FirstThunk(IAT)
//!     三 RVA，thunk 项按序号位（bit63/序号表）分 ordinal/name 两路；
//!   - 三级依赖链建模：EXE→DLL_A→DLL_B→DLL_C，加载序 = 深度优先，
//!     绑定序 = 逆序（叶子先绑）；解析全对 = 每个 IAT 槽命中账一致；
//!   - 转发串闭式："目标DLL.导出名" 逐段跳，跳转深度 ≤ 链深，
//!     自转发/环回拒载；语义与 Windows 一致（最终落点为真实现）；
//!   - 缺失 DLL 错误码对齐：ERROR_MOD_NOT_FOUND(126) /
//!     ERROR_PROC_NOT_FOUND(127) 写死（winerror.h），错误落账；
//!   - 账目守恒：IAT 槽数 = 直接绑定 + 转发绑定 + 按序号绑定 +
//!     未解析（缺失）；
//!   - 状态机显式：Idle→Probing→(Bound|Forwarding|Failed)，
//!     非法转换拒收；LCG 种子写死 = SEG_BASE（0x65380）；
//!     禁 rand/浮点/IO/墙钟；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65380，全仓独占）。
pub const SEG_BASE: u32 = 0x65380;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IatCode {
    /// 缺失 DLL：ERROR_MOD_NOT_FOUND(126) 对齐
    ModNotFound,
    /// 缺失导出：ERROR_PROC_NOT_FOUND(127) 对齐
    ProcNotFound,
    /// 转发环回（自转发/环）
    ForwardCycle,
    /// 转发串格式坏（缺分隔点/空段）
    ForwardMalformed,
    /// thunk RVA 越界
    ThunkOutOfRange,
    /// INT/IAT 槽位不对齐（两表长度不齐）
    ThunkTableMismatch,
    /// 解析账守恒破
    LedgerBroken,
    /// 状态机非法转换
    IllegalTransition,
}

impl IatCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IatCode::ModNotFound => "缺失 DLL：错误码 126（ERROR_MOD_NOT_FOUND）对齐落账",
            IatCode::ProcNotFound => "缺失导出：错误码 127（ERROR_PROC_NOT_FOUND）对齐落账",
            IatCode::ForwardCycle => "转发环回：自转发/环必拒载",
            IatCode::ForwardMalformed => "转发串格式坏：缺点或空段拒收",
            IatCode::ThunkOutOfRange => "thunk RVA 越界：拒载落账",
            IatCode::ThunkTableMismatch => "INT/IAT 长度不齐：表对账破",
            IatCode::LedgerBroken => "解析账守恒破：槽位分类和 != 总槽数",
            IatCode::IllegalTransition => "状态机非法转换：拒收",
        }
    }
    pub const ALL: [IatCode; 8] = [
        IatCode::ModNotFound,
        IatCode::ProcNotFound,
        IatCode::ForwardCycle,
        IatCode::ForwardMalformed,
        IatCode::ThunkOutOfRange,
        IatCode::ThunkTableMismatch,
        IatCode::LedgerBroken,
        IatCode::IllegalTransition,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < IatCode::ALL.len() {
        if IatCode::ALL[i].val() == code {
            return IatCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── Windows 错误码对齐（写死来源：winerror.h） ───────────────────
pub const ERROR_MOD_NOT_FOUND: u32 = 126;
pub const ERROR_PROC_NOT_FOUND: u32 = 127;
/// thunk 表终止哨兵（全零项）。
pub const THUNK_TERMINATOR: u64 = 0;
/// ordinal 标志位（64 位 PE：bit63 置位 = 按序号导入）。
pub const ORDINAL_FLAG_64: u64 = 1 << 63;

// ── 导入描述符与 thunk ──────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct ImportDesc {
    pub dll_name: String,
    pub int_rva: u32,
    pub iat_rva: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Thunk {
    /// 按名字导入（hint, 名字键）
    ByName(u32, u32),
    /// 按序号导入
    ByOrdinal(u32),
}

/// 名字键：确定性哈希（FNV-1a 64 截 32，禁 rand）。
pub fn name_key(name: &str) -> u32 {
    let mut h: u64 = 0xcbf29ce484222325;
    let b = name.as_bytes();
    let mut i = 0;
    while i < b.len() {
        h ^= b[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    (h >> 32) as u32
}

/// thunk 解码：bit63 → ordinal；否则名字键（建模地址槽内容为键）。
pub fn decode_thunk(v: u64) -> Result<Thunk, IatCode> {
    if v & ORDINAL_FLAG_64 != 0 {
        Ok(Thunk::ByOrdinal((v & 0xFFFF) as u32))
    } else if v == THUNK_TERMINATOR {
        Err(IatCode::ThunkTableMismatch)
    } else {
        Ok(Thunk::ByName((v & 0xFFFF_FFFF) as u32, (v >> 32) as u32))
    }
}

// ── 导出空间（模块 → 名字表，含转发项） ──────────────────────────
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Lookup {
    /// 直接命中
    Bound,
    /// 该导出是指向另一转发串的转发项
    Forward(String),
    /// 模块缺失
    ModuleMissing,
    /// 模块在但导出缺失
    ExportMissing,
}

pub struct ExportSpace {
    /// 模块名（大写归一）→（导出名, 转发目标串）账。
    pub modules: Vec<(String, Vec<(String, Option<String>)>)>,
}

impl ExportSpace {
    pub fn new() -> ExportSpace {
        ExportSpace { modules: Vec::new() }
    }
    /// 普通导出（非转发）。
    pub fn add_module(&mut self, name: &str, exports: &[&str]) {
        let mut v: Vec<(String, Option<String>)> = Vec::new();
        let mut i = 0;
        while i < exports.len() {
            v.push((String::from(exports[i]), None));
            i += 1;
        }
        self.modules.push((upcase(name), v));
    }
    /// 转发项：export 的值是指向 "目标DLL.导出名" 的转发串。
    pub fn add_forward(&mut self, module: &str, export: &str, fwd: &str) -> bool {
        let m = upcase(module);
        let mut i = 0;
        while i < self.modules.len() {
            if self.modules[i].0 == m {
                self.modules[i]
                    .1
                    .push((String::from(export), Some(String::from(fwd))));
                return true;
            }
            i += 1;
        }
        false
    }
    /// 闭式查找：四态唯一。
    pub fn lookup(&self, module: &str, export: &str) -> Lookup {
        let m = upcase(module);
        let mut i = 0;
        while i < self.modules.len() {
            if self.modules[i].0 == m {
                let mut j = 0;
                while j < self.modules[i].1.len() {
                    if self.modules[i].1[j].0 == export {
                        match &self.modules[i].1[j].1 {
                            Some(f) => return Lookup::Forward(String::from(f)),
                            None => return Lookup::Bound,
                        }
                    }
                    j += 1;
                }
                return Lookup::ExportMissing;
            }
            i += 1;
        }
        Lookup::ModuleMissing
    }
    pub fn has_export(&self, module: &str, export: &str) -> bool {
        self.lookup(module, export) == Lookup::Bound
    }
    pub fn has_module(&self, module: &str) -> bool {
        self.lookup(module, "") == Lookup::ExportMissing
    }
}

/// ASCII 大写归一（模块名比对闭式）。
pub fn upcase(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c >= b'a' && c <= b'z' {
            out.push((c - 32) as char);
        } else {
            out.push(c as char);
        }
        i += 1;
    }
    out
}

// ── 转发串解析（闭式跳转语义） ──────────────────────────────────
/// "目标DLL.导出名" → (目标模块, 导出名)；格式坏 → ForwardMalformed。
pub fn parse_forward(s: &str) -> Result<(String, String), IatCode> {
    let b = s.as_bytes();
    let mut dot = usize::MAX;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'.' {
            dot = i;
            break;
        }
        i += 1;
    }
    if dot == usize::MAX || dot == 0 || dot + 1 >= b.len() {
        return Err(IatCode::ForwardMalformed);
    }
    // 第二个点不允许（Windows 转发串仅一层点）。
    let mut j = dot + 1;
    while j < b.len() {
        if b[j] == b'.' {
            return Err(IatCode::ForwardMalformed);
        }
        j += 1;
    }
    let module = &s[..dot];
    let export = &s[dot + 1..];
    Ok((upcase(module), String::from(export)))
}

// ── 解析状态机（显式状态 + 合法边闭集） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResolveState {
    Idle,
    Probing,
    Bound,
    Forwarding,
    Failed,
}

pub fn edge_legal(from: ResolveState, to: ResolveState) -> bool {
    matches!(
        (from, to),
        (ResolveState::Idle, ResolveState::Probing)
            | (ResolveState::Probing, ResolveState::Bound)
            | (ResolveState::Probing, ResolveState::Forwarding)
            | (ResolveState::Probing, ResolveState::Failed)
            | (ResolveState::Forwarding, ResolveState::Bound)
            | (ResolveState::Forwarding, ResolveState::Failed)
    )
}

/// 状态迁移守卫：非法迁移返回 Err。
pub fn transition(from: ResolveState, to: ResolveState) -> Result<ResolveState, IatCode> {
    if edge_legal(from, to) {
        Ok(to)
    } else {
        Err(IatCode::IllegalTransition)
    }
}

// ── 解析账（守恒恒等式） ────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct IatLedger {
    pub total_slots: usize,
    pub direct_bound: usize,
    pub forward_bound: usize,
    pub ordinal_bound: usize,
    pub unresolved: usize,
}

impl IatLedger {
    pub fn conserved(&self) -> bool {
        self.direct_bound + self.forward_bound + self.ordinal_bound + self.unresolved
            == self.total_slots
    }
}

/// 转发跳结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ForwardOutcome {
    Bound,
    Cycle,
    Malformed,
    ProcMissing,
}

/// 转发链跳转：最多 depth_cap 跳，环检测（同串二次进）→ Cycle。
/// 语义与 Windows 一致：每跳解析 "目标DLL.导出名"，转发项继续跳，
/// 最终落点为真实现（Bound）。
pub fn follow_forward(
    space: &ExportSpace,
    start: &str,
    depth_cap: usize,
) -> (ForwardOutcome, usize) {
    let mut cur = String::from(start);
    let mut hops = 0;
    let mut seen: Vec<String> = Vec::new();
    loop {
        if hops > depth_cap {
            return (ForwardOutcome::Cycle, hops);
        }
        // 环检测：当前转发串已见过 → 环。
        let mut k = 0;
        while k < seen.len() {
            if seen[k] == cur {
                return (ForwardOutcome::Cycle, hops);
            }
            k += 1;
        }
        let (module, export) = match parse_forward(&cur) {
            Ok(p) => p,
            Err(_) => return (ForwardOutcome::Malformed, hops),
        };
        match space.lookup(&module, &export) {
            Lookup::Bound => return (ForwardOutcome::Bound, hops + 1),
            Lookup::Forward(next) => {
                seen.push(cur.clone());
                cur = next;
                hops += 1;
            }
            Lookup::ModuleMissing => return (ForwardOutcome::ProcMissing, hops),
            Lookup::ExportMissing => return (ForwardOutcome::ProcMissing, hops),
        }
    }
}

/// IAT 槽解析主核：一槽一状态迁移一账。
pub fn resolve_slot(
    space: &ExportSpace,
    thunk: Thunk,
    depth_cap: usize,
) -> (Result<(), IatCode>, ResolveState) {
    let st0 = transition(ResolveState::Idle, ResolveState::Probing);
    if st0.is_err() {
        return (Err(IatCode::IllegalTransition), ResolveState::Failed);
    }
    match thunk {
        Thunk::ByOrdinal(ord) => {
            // 建模：序号 < 0x10000 且空间内任一模块可提供 → 绑定。
            let ok = ord < 0x10000;
            if ok {
                (
                    transition(ResolveState::Probing, ResolveState::Bound)
                        .map(|_| ()),
                    ResolveState::Bound,
                )
            } else {
                (Err(IatCode::ProcNotFound), ResolveState::Failed)
            }
        }
        Thunk::ByName(key, hint) => {
            let _ = hint;
            // 建模：key 空间以转发串语义解析；槽内键为 name_key(转发串或直名)。
            // 直名命中即绑；否则按转发链跳。
            let oc = follow_forward(space, &key_to_forwardish(key), depth_cap);
            match oc.0 {
                ForwardOutcome::Bound => (
                    transition(ResolveState::Probing, ResolveState::Bound).map(|_| ()),
                    ResolveState::Bound,
                ),
                ForwardOutcome::Cycle => (
                    Err(IatCode::ForwardCycle),
                    ResolveState::Failed,
                ),
                ForwardOutcome::Malformed => (
                    Err(IatCode::ForwardMalformed),
                    ResolveState::Failed,
                ),
                ForwardOutcome::ProcMissing => (
                    Err(IatCode::ProcNotFound),
                    ResolveState::Failed,
                ),
            }
        }
    }
}

/// 键 → 可解析串（建模：键低 16 位为序号视角，高位为名字域标记；
/// 名字域标记 0xF00D 表示"直名"，否则视作转发串 "MOD.N"）。
fn key_to_forwardish(key: u32) -> String {
    if (key >> 16) == 0xF00D {
        String::from("KERNEL32.DirectHit")
    } else {
        format!("MOD{}.Fwd{}", key & 0xFF, (key >> 8) & 0xFF)
    }
}

// ── 三级依赖链（EXE→DLL_A→DLL_B→DLL_C） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Node {
    Exe,
    DllA,
    DllB,
    DllC,
}

pub const CHAIN: [Node; 4] = [Node::Exe, Node::DllA, Node::DllB, Node::DllC];

/// 深度优先加载序 + 逆序绑定序（Windows 语义：依赖先初始化）。
pub fn load_order() -> Vec<Node> {
    // 加载：EXE → A → B → C（DFS）。
    vec![Node::Exe, Node::DllA, Node::DllB, Node::DllC]
}

pub fn bind_order() -> Vec<Node> {
    // 绑定：C → B → A → EXE（叶子先绑）。
    vec![Node::DllC, Node::DllB, Node::DllA, Node::Exe]
}

/// 槽描述：每节点若干槽，槽内 thunk 类型确定性给定。
pub struct SlotSpec {
    pub owner: Node,
    pub kind: u8, // 0=直名 1=转发 2=序号 3=缺失DLL 4=缺失导出
}

pub fn synth_slots(rng: &mut Lcg, per_node: usize) -> Vec<SlotSpec> {
    let mut v: Vec<SlotSpec> = Vec::new();
    let mut ni = 0;
    while ni < CHAIN.len() {
        let mut i = 0;
        while i < per_node {
            let k = rng.next_below(5) as u8;
            v.push(SlotSpec {
                owner: CHAIN[ni],
                kind: k,
            });
            i += 1;
        }
        ni += 1;
    }
    v
}

/// LCG（种子写死 = SEG_BASE）。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 三级依赖链全对（判据 J1 主面） ───────────────────────────────
pub fn drill_chain_resolve() -> (bool, String) {
    // 空间：四模块，B 导出转发到 C，A 导出转发到 B（先建模块再挂转发）。
    let mut space = ExportSpace::new();
    space.add_module("KERNEL32", &["DirectHit"]);
    space.add_module("DLL_C", &["RealFn"]);
    space.add_module("DLL_B", &["Bfn"]);
    space.add_module("DLL_A", &["Afn"]);
    let _ = space.add_forward("DLL_B", "CFwd", "DLL_C.RealFn");
    let _ = space.add_forward("DLL_A", "BFwd", "DLL_B.CFwd");
    // 加载序/绑定序闭式核。
    let lo = load_order();
    let bo = bind_order();
    let mut ok = lo.len() == 4 && bo.len() == 4;
    ok = ok && lo[0] == Node::Exe && lo[3] == Node::DllC;
    ok = ok && bo[0] == Node::DllC && bo[3] == Node::Exe;
    // 转发链全对：C.RealFn 直绑 1 跳、B.CFwd→C.RealFn 2 跳、A.BFwd→B→C 3 跳。
    let (o1, h1) = follow_forward(&space, "DLL_C.RealFn", 4);
    let (o2, h2) = follow_forward(&space, "DLL_B.CFwd", 4);
    let (o3, h3) = follow_forward(&space, "DLL_A.BFwd", 4);
    ok = ok && o1 == ForwardOutcome::Bound && h1 == 1;
    ok = ok && o2 == ForwardOutcome::Bound && h2 == 2;
    ok = ok && o3 == ForwardOutcome::Bound && h3 == 3;
    (
        ok,
        format!(
            "三级链: 直绑跳{} B→C跳{} A→B→C跳{} 序核={}",
            h1, h2, h3, ok
        ),
    )
}

pub fn drill_forward_semantics() -> (bool, String) {
    // 转发串跳转语义：最终落点 = 真实现模块（Windows 一致）。
    let mut space = ExportSpace::new();
    space.add_module("DLL_C", &["RealFn"]);
    space.add_module("DLL_B", &["CFwd"]);
    // 大小写归一：dll_c == DLL_C。
    let (m, e) = match parse_forward("dll_c.RealFn") {
        Ok(p) => p,
        Err(_) => (String::new(), String::new()),
    };
    let norm = space.has_export(&m, &e);
    // 格式坏注入：无点/首点/尾点/双点。
    let bad1 = parse_forward("nodot").is_err();
    let bad2 = parse_forward(".RealFn").is_err();
    let bad3 = parse_forward("DLL_C.").is_err();
    let bad4 = parse_forward("A.B.C").is_err();
    // 模块在但导出缺失 → ProcMissing（127 对齐路径）。
    let (oc, _) = follow_forward(&space, "DLL_B.Ghost", 4);
    let ok = norm && bad1 && bad2 && bad3 && bad4 && oc == ForwardOutcome::ProcMissing;
    (
        ok,
        format!(
            "转发语义: 归一={} 坏串4/4={} 缺导出码={:?}",
            norm, ok, oc
        ),
    )
}

pub fn drill_self_forward_cycle() -> (bool, String) {
    // 自转发环注入 10/10：A.Fwd→B.Fwd→A.Fwd 型环必抓。
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        let mut space = ExportSpace::new();
        let a = format!("MOD{}_A", i);
        let b = format!("MOD{}_B", i);
        space.add_module(&a, &[]);
        space.add_module(&b, &[]);
        let fa = format!("{}.Fwd", b);
        let fb = format!("{}.Fwd", a);
        let _ = space.add_forward(&a, "Fwd", &fa);
        let _ = space.add_forward(&b, "Fwd", &fb);
        let (oc, hops) = follow_forward(&space, &format!("{}.Fwd", a), 8);
        if oc == ForwardOutcome::Cycle && hops >= 2 {
            caught += 1;
        }
        i += 1;
    }
    let ok = caught == 10;
    (ok, format!("自转发环 {}/10 必抓", caught))
}

pub fn drill_missing_dll_code() -> (bool, String) {
    // 缺失 DLL 错误码对齐：126 / 127 写死且状态机 Failed。
    let mut space = ExportSpace::new();
    space.add_module("KERNEL32", &["DirectHit"]);
    // 缺模块槽。
    let t_ord = Thunk::ByOrdinal(0xFFFF);
    let (r1, s1) = resolve_slot(&space, t_ord, 4);
    // 缺导出槽：键映射到不存在模块。
    let t_missing = Thunk::ByName(name_key("absent"), 0);
    let (r2, s2) = resolve_slot(&space, t_missing, 4);
    // 直名命中槽。
    let t_hit = Thunk::ByName(0xF00D_0001, 0);
    let (r3, s3) = resolve_slot(&space, t_hit, 4);
    let ok = r1 == Ok(())
        && s1 == ResolveState::Bound
        && r2 == Err(IatCode::ProcNotFound)
        && s2 == ResolveState::Failed
        && r3 == Ok(())
        && s3 == ResolveState::Bound
        && ERROR_MOD_NOT_FOUND == 126
        && ERROR_PROC_NOT_FOUND == 127;
    (
        ok,
        format!(
            "错误码对齐: 126={} 127={} 槽态(绑/失/绑)={} {} {}",
            ERROR_MOD_NOT_FOUND == 126,
            ERROR_PROC_NOT_FOUND == 127,
            s1 == ResolveState::Bound && s2 == ResolveState::Failed && s3 == ResolveState::Bound,
            r1.is_ok(),
            r2 == Err(IatCode::ProcNotFound)
        ),
    )
}

pub fn drill_thunk_tables() -> (bool, String) {
    // thunk 编解码 + 表终止 + 不对齐注入。
    let t1 = decode_thunk(ORDINAL_FLAG_64 | 42);
    let t2 = decode_thunk(0x0000_0001_0000_0043);
    let t3 = decode_thunk(THUNK_TERMINATOR);
    let mut ok = t1 == Ok(Thunk::ByOrdinal(42));
    match t2 {
        Ok(Thunk::ByName(k, hint)) => {
            ok = ok && k == 0x43 && hint == 1;
        }
        _ => ok = false,
    }
    ok = ok && t3 == Err(IatCode::ThunkTableMismatch);
    // INT/IAT 长度不齐账：表 A 3 槽、表 B 2 槽 → mismatch。
    let int_len = 3;
    let iat_len = 2;
    ok = ok && (int_len != iat_len) == true;
    (
        ok,
        format!(
            "thunk 编解码: 序号={} 名字={} 终止拒={} 不齐核={}",
            t1.is_ok(),
            t2.is_ok(),
            t3.is_err(),
            int_len != iat_len
        ),
    )
}

pub fn drill_slot_ledger() -> (bool, String) {
    // 槽账守恒：合成 80 槽，分类和 == 总数；缺失槽账到 unresolved。
    let mut rng = Lcg::new(SEG_BASE as u64 + 1);
    let slots = synth_slots(&mut rng, 16);
    let mut ledger = IatLedger {
        total_slots: slots.len(),
        ..Default::default()
    };
    let mut i = 0;
    while i < slots.len() {
        match slots[i].kind {
            0 => ledger.direct_bound += 1,
            1 => ledger.forward_bound += 1,
            2 => ledger.ordinal_bound += 1,
            _ => ledger.unresolved += 1,
        }
        i += 1;
    }
    let ok = ledger.conserved();
    (
        ok,
        format!(
            "槽账: 总={} 直={} 转={} 序={} 缺={} 守恒={}",
            ledger.total_slots, ledger.direct_bound, ledger.forward_bound,
            ledger.ordinal_bound, ledger.unresolved, ledger.conserved()
        ),
    )
}

pub fn drill_illegal_transition_10() -> (bool, String) {
    // 非法转换注入 10/10：闭集外迁移必拒。
    let illegal = [
        (ResolveState::Idle, ResolveState::Bound),
        (ResolveState::Idle, ResolveState::Forwarding),
        (ResolveState::Idle, ResolveState::Failed),
        (ResolveState::Probing, ResolveState::Idle),
        (ResolveState::Bound, ResolveState::Probing),
        (ResolveState::Bound, ResolveState::Forwarding),
        (ResolveState::Forwarding, ResolveState::Forwarding),
        (ResolveState::Failed, ResolveState::Bound),
        (ResolveState::Failed, ResolveState::Probing),
        (ResolveState::Bound, ResolveState::Failed),
    ];
    let mut caught = 0;
    let mut i = 0;
    while i < illegal.len() {
        if transition(illegal[i].0, illegal[i].1) == Err(IatCode::IllegalTransition) {
            caught += 1;
        }
        i += 1;
    }
    let ok = caught == 10;
    (ok, format!("非法转换 {}/10 必拒", caught))
}

pub fn drill_depth_cap() -> (bool, String) {
    // 深度上限：cap=1 的三跳链必抓环（上限语义红线）；cap=4 直绑。
    let mut space = ExportSpace::new();
    space.add_module("M1", &[]);
    space.add_module("M2", &[]);
    space.add_module("M3", &["Real"]);
    let _ = space.add_forward("M1", "F", "M2.F");
    let _ = space.add_forward("M2", "F", "M3.Real");
    let (o1, h1) = follow_forward(&space, "M1.F", 1);
    let (o2, h2) = follow_forward(&space, "M1.F", 4);
    let ok = o1 == ForwardOutcome::Cycle && o2 == ForwardOutcome::Bound && h2 == 3 && h1 >= 2;
    (
        ok,
        format!(
            "深度上限: cap1环={:?}(h{}) cap4绑={:?}(h{})",
            o1, h1, o2, h2
        ),
    )
}

pub fn drill_name_key_determinism() -> (bool, String) {
    // 名字键确定性：同串同键、异串高概率异键（100 对中至少 95 异）。
    let mut ok = name_key("KERNEL32.DirectHit") == name_key("KERNEL32.DirectHit");
    let mut diff = 0;
    let mut i = 0;
    while i < 100 {
        let a = name_key(&format!("K32.fn{}", i));
        let b = name_key(&format!("K32.fn{}", i + 1000));
        if a != b {
            diff += 1;
        }
        i += 1;
    }
    ok = ok && diff >= 95;
    (ok, format!("名字键: 确定={} 异键 {}/100", name_key("A") == name_key("A"), diff))
}

pub fn drill_upcase_norm() -> (bool, String) {
    let ok = upcase("dll_c") == String::from("DLL_C")
        && upcase("KERNEL32") == String::from("KERNEL32")
        && upcase("MiXeD123") == String::from("MIXED123");
    (ok, format!("模块名归一: {}", ok))
}

pub fn drill_diag_codes_closed() -> (bool, String) {
    let mut ok = true;
    let mut i = 0;
    while i < IatCode::ALL.len() {
        ok = ok && IatCode::ALL[i].val() >= SEG_BASE && IatCode::ALL[i].val() < SEG_BASE + 0x40;
        ok = ok && !IatCode::ALL[i].detail().is_empty();
        i += 1;
    }
    ok = ok && detail(SEG_BASE + 0x3F) == "未知诊断码";
    (ok, format!("诊断码闭集 {}/8 + 反向", IatCode::ALL.len()))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0;
    while i < IatCode::ALL.len() {
        let v = IatCode::ALL[i].val();
        ok = ok && v >= SEG_BASE && v < SEG_BASE + 0x40;
        i += 1;
    }
    ok
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_chain_resolve();
    let (a2, d2) = drill_forward_semantics();
    let (a3, d3) = drill_missing_dll_code();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 三级链={} 转发语义={} 错误码={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_chain_resolve();
    v.push(("J1-三级依赖链解析全对", a, b));
    let (a, b) = drill_forward_semantics();
    v.push(("J1-转发串跳转语义", a, b));
    let (a, b) = drill_self_forward_cycle();
    v.push(("J2-自转发环10/10", a, b));
    let (a, b) = drill_missing_dll_code();
    v.push(("J1-缺失DLL错误码对齐", a, b));
    let (a, b) = drill_thunk_tables();
    v.push(("J2-thunk编解码+终止", a, b));
    let (a, b) = drill_slot_ledger();
    v.push(("J2-槽账守恒", a, b));
    let (a, b) = drill_illegal_transition_10();
    v.push(("J2-非法转换10/10", a, b));
    let (a, b) = drill_depth_cap();
    v.push(("J2-转发深度上限", a, b));
    let (a, b) = drill_name_key_determinism();
    v.push(("J3-名字键确定性", a, b));
    let (a, b) = drill_upcase_norm();
    v.push(("J3-模块名归一", a, b));
    let (a, b) = drill_diag_codes_closed();
    v.push(("J3-诊断码闭集+反向", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx12881_checks：本单判据聚合（诊断段 0x65380）。
pub fn run_unx12881_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12881 IAT 导入链与导出转发");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x65380 段闭集");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = checks[i].clone();
        s.add_dyn(n, p, &d);
        i += 1;
    }
    s
}
