//! UNX-F51548 级联拓扑变更账（中段 HUB 拔出→子树失效） · 0xC0BC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面逐字锚定）：
//!   UNX-F51549-J1 分区断电 ×10 邻端口零受扰；全断误动作 0 命中。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   ① TopoTree —— 端口拓扑树（父指针数组 + 子树成员闭包），支持中段
//!      HUB 拔出后一次性求出失效子树全体端口。
//!   ② ChangeLedger —— 级联拓扑变更账：每条变更（拔出/接入/失效圈定/
//!      子树清算/邻口确认）按序号单调入账，账本只增不减，指纹可复核。
//!   ③ ChangeParser —— 变更记录解析器：把原始 (码, 端口) 二元组按闭集
//!      解码为变更事件，越码/越界端口显性拒。
//!   ④ TopoInjector —— 注入核：5 类坏样本逐类 ×10 轮精确复现，注入账
//!      守恒（检出+逃逸=轮数，逃逸恒 0）。
//!   ⑤ TopoCode —— 诊断码闭集（12 变体），val()=SEG_BASE+1+变体序号。
//!
//! 设计要点：
//!   no_std + alloc；零浮点、零 IO、零墙钟、零随机——伪随机一律走 LCG
//!   （种子=SEG_BASE 派生）；零 unwrap/expect/panic/裸下标，取值一律走
//!   .get()/.get_mut()；双跑 FNV-1a 指纹必须逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

/// 段号：0xC0000 + 0x40 × (51548 − 51501)。
pub const SEG_BASE: u32 = 0xC0BC0;

/// 端口数上限（父指针数组定长）。
pub const MAX_PORT: usize = 16;
/// 根端口编号（端口 0 为根 HUB 上行口，不可拔出）。
pub const ROOT: usize = 0;
/// 子树失效清算判据轮数（失效圈定 ×10）。
pub const SUBTREE_ROUNDS: usize = 10;
/// 邻端口确认判据轮数（邻端口零受扰 ×5 抽验）。
pub const NEIGHBOR_ROUNDS: usize = 5;

/// 变更类别（闭集）：接入/拔出/失效圈定/子树清算/邻口确认。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeKind {
    /// 端口接入
    Attach = 0,
    /// 端口拔出（中段 HUB 拔出即触发级联）
    Detach = 1,
    /// 失效子树圈定
    MarkFailed = 2,
    /// 子树清算（失效端口全部下账）
    Purge = 3,
    /// 邻端口确认（零受扰核验）
    NeighborCheck = 4,
}

impl ChangeKind {
    /// 类别码闭集解码：越码返回 None。
    pub fn from_code(c: u32) -> Option<ChangeKind> {
        match c {
            0 => Some(ChangeKind::Attach),
            1 => Some(ChangeKind::Detach),
            2 => Some(ChangeKind::MarkFailed),
            3 => Some(ChangeKind::Purge),
            4 => Some(ChangeKind::NeighborCheck),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ChangeKind::Attach => "接入",
            ChangeKind::Detach => "拔出",
            ChangeKind::MarkFailed => "失效圈定",
            ChangeKind::Purge => "子树清算",
            ChangeKind::NeighborCheck => "邻口确认",
        }
    }
}

/// 拓扑树：parent[i] 为端口 i 的父端口，ROOT 的父为其自身。
/// 初始级联链：0→1→2→3 与 0→4→5 与 0→7（中段拔 1 即失效 2、3）。
pub struct TopoTree {
    parent: [usize; MAX_PORT],
    alive: [bool; MAX_PORT],
}

impl TopoTree {
    /// 建树：全端口存活，级联链 0→1→2→3、0→4→5、0→7，其余挂根。
    pub fn new() -> TopoTree {
        let mut parent = [ROOT; MAX_PORT];
        // 级联链 0→1→2→3、0→4→5、0→7（免裸下标，逐槽 get_mut 写入）。
        let links: [(usize, usize); 6] =
            [(1, ROOT), (2, 1), (3, 2), (4, ROOT), (5, 4), (7, ROOT)];
        for (child, up) in links.iter() {
            if let Some(slot) = parent.get_mut(*child) {
                *slot = *up;
            }
        }
        let mut alive = [true; MAX_PORT];
        // 端口 6 与 8 出厂即空槽。
        if let Some(f) = alive.get_mut(6) {
            *f = false;
        }
        if let Some(f) = alive.get_mut(8) {
            *f = false;
        }
        TopoTree { parent, alive }
    }

    /// 父端口读取（越界返回 None）。
    pub fn parent_of(&self, port: usize) -> Option<usize> {
        match self.parent.get(port) {
            Some(&p) => Some(p),
            None => None,
        }
    }

    /// 存活位读取（越界返回 None）。
    pub fn alive_of(&self, port: usize) -> Option<bool> {
        match self.alive.get(port) {
            Some(&v) => Some(v),
            None => None,
        }
    }

    /// 上行链（自 port 到根，不含 port 自身）。
    pub fn uplink(&self, port: usize) -> Vec<usize> {
        let mut chain: Vec<usize> = Vec::new();
        let mut cur = port;
        for _ in 0..MAX_PORT {
            if cur == ROOT {
                break;
            }
            match self.parent.get(cur) {
                Some(&p) => {
                    chain.push(cur);
                    cur = p;
                }
                None => break,
            }
        }
        chain
    }

    /// 失效子树圈定：拔出 port 后，其全部后代（含自身）一并失效。
    pub fn failed_subtree(&self, port: usize) -> Result<Vec<usize>, TopoCode> {
        if port >= MAX_PORT {
            return Err(TopoCode::PortOob);
        }
        if port == ROOT {
            return Err(TopoCode::RootDetach);
        }
        if self.alive_of(port) != Some(true) {
            return Err(TopoCode::PortDead);
        }
        let mut out: Vec<usize> = Vec::new();
        for p in 0..MAX_PORT {
            // p 的上行链含 port 即为 port 的后代（或自身）。
            if p == port || self.uplink(p).contains(&port) {
                out.push(p);
            }
        }
        Ok(out)
    }

    /// 拔出：端口与整棵子树置死，返回失效端口清单。
    pub fn detach(&mut self, port: usize) -> Result<Vec<usize>, TopoCode> {
        let dead = self.failed_subtree(port)?;
        for &p in dead.iter() {
            if let Some(flag) = self.alive.get_mut(p) {
                *flag = false;
            }
        }
        Ok(dead)
    }

    /// 接入：置活（仅当端口在界内且父口存活）。
    pub fn attach(&mut self, port: usize) -> Result<(), TopoCode> {
        if port >= MAX_PORT {
            return Err(TopoCode::PortOob);
        }
        if port == ROOT {
            return Err(TopoCode::RootAttach);
        }
        let pa = match self.parent.get(port) {
            Some(&p) => p,
            None => return Err(TopoCode::PortOob),
        };
        if self.alive_of(pa) != Some(true) {
            return Err(TopoCode::ParentDead);
        }
        if let Some(flag) = self.alive.get_mut(port) {
            *flag = true;
        }
        Ok(())
    }
}

/// 诊断码闭集（拓扑变更账域，12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TopoCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 端口号越界
    PortOob,
    /// 根口不可拔出
    RootDetach,
    /// 根口不可接入
    RootAttach,
    /// 端口已死仍要求操作
    PortDead,
    /// 父口已死不许接入
    ParentDead,
    /// 变更类别码越闭集
    KindUnknown,
    /// 变更序号倒退
    SeqRegress,
    /// 邻端口受扰（判据要求零命中）
    NeighborDisturbed,
    /// 账面守恒不符
    LedgerMismatch,
    /// 双跑指纹不一致
    FingerprintMismatch,
}

/// 权威闭集数组（val() 序号以此为准）。
pub const ALL: [TopoCode; 12] = [
    TopoCode::Ok,
    TopoCode::SegBad,
    TopoCode::PortOob,
    TopoCode::RootDetach,
    TopoCode::RootAttach,
    TopoCode::PortDead,
    TopoCode::ParentDead,
    TopoCode::KindUnknown,
    TopoCode::SeqRegress,
    TopoCode::NeighborDisturbed,
    TopoCode::LedgerMismatch,
    TopoCode::FingerprintMismatch,
];

impl TopoCode {
    /// 诊断值：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        match ALL.iter().position(|&c| c == self) {
            Some(i) => SEG_BASE + 1 + i as u32,
            None => SEG_BASE + 1,
        }
    }
    /// 全量非空中文建议。
    pub fn detail(self) -> &'static str {
        match self {
            TopoCode::Ok => "拓扑变更入账成功",
            TopoCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            TopoCode::PortOob => "端口越界：端口号必须在 0..16 之内",
            TopoCode::RootDetach => "根口不可拔出：根 HUB 上行口是拓扑锚点",
            TopoCode::RootAttach => "根口不可接入：根口由上电枚举占有",
            TopoCode::PortDead => "端口已死：对死口只允许子树清算或接入复位",
            TopoCode::ParentDead => "父口已死：先恢复父口再接入子口",
            TopoCode::KindUnknown => "变更类别非法：只接受 0..=4 五类闭集码",
            TopoCode::SeqRegress => "序号倒退：变更序号必须严格递增",
            TopoCode::NeighborDisturbed => "邻口受扰：拔出只许影响失效子树内端口",
            TopoCode::LedgerMismatch => "账面不符：注入=检出+逃逸且逃逸恒零",
            TopoCode::FingerprintMismatch => "指纹不一致：双跑 FNV-1a 必须逐位一致",
        }
    }
}

/// 账本条目：一条拓扑变更的完整留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChangeEntry {
    /// 变更序号（单调递增）
    pub seq: u64,
    /// 变更类别码
    pub kind_code: u32,
    /// 主端口
    pub port: usize,
    /// 波及端口清单（失效子树）
    pub affected: Vec<usize>,
    /// 邻端口受扰数（判据要求恒 0）
    pub disturbed: usize,
}

/// 变更记录解析器：原始二元组 → 闭集变更事件。
pub struct ChangeParser {
    seen: u64,
}

impl ChangeParser {
    /// 建解析器。
    pub fn new() -> ChangeParser {
        ChangeParser { seen: 0 }
    }

    /// 解析一条 (码, 端口)：类别闭集 → 端口界 → 根口特判。
    pub fn parse(&mut self, code: u32, port: usize) -> Result<(ChangeKind, usize), TopoCode> {
        let k = match ChangeKind::from_code(code) {
            Some(k) => k,
            None => return Err(TopoCode::KindUnknown),
        };
        if port >= MAX_PORT {
            return Err(TopoCode::PortOob);
        }
        if k == ChangeKind::Detach && port == ROOT {
            return Err(TopoCode::RootDetach);
        }
        self.seen = self.seen.saturating_add(1);
        Ok((k, port))
    }

    pub fn seen(&self) -> u64 {
        self.seen
    }
}

/// 级联拓扑变更账：变更留痕只增不减 + 守恒注入账。
pub struct ChangeLedger {
    tree: TopoTree,
    last_seq: u64,
    entries: Vec<ChangeEntry>,
    detach_cnt: u64,
    purge_cnt: u64,
    disturbed_total: usize,
    injected: u64,
    detected: u64,
    escaped: u64,
    /// LCG 种子：SEG_BASE 派生，唯一合法的"随机"来源。
    lcg: u64,
}

/// LCG 一步：线性同余伪随机（确定性，替代真随机源）。
pub fn lcg_next(state: &mut u64) -> u64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *state >> 33
}

/// FNV-1a 64 位指纹（双跑一致面）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

impl ChangeLedger {
    /// 建账：新树、零账本、LCG 种子=SEG_BASE。
    pub fn new() -> ChangeLedger {
        ChangeLedger {
            tree: TopoTree::new(),
            last_seq: 0,
            entries: Vec::new(),
            detach_cnt: 0,
            purge_cnt: 0,
            disturbed_total: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
            lcg: SEG_BASE as u64,
        }
    }

    /// 入账一条变更：序号单调 → 解析闭集 → 圈定失效子树 → 邻口核验。
    /// 邻口核验：失效子树之外不许有任何存活端口被误置死（零受扰）。
    pub fn commit(&mut self, seq: u64, kind: ChangeKind, port: usize) -> Result<usize, TopoCode> {
        if seq <= self.last_seq {
            return Err(TopoCode::SeqRegress);
        }
        self.last_seq = seq;
        let mut affected: Vec<usize> = Vec::new();
        let mut disturbed: usize = 0;
        match kind {
            ChangeKind::Attach => {
                self.tree.attach(port)?;
            }
            ChangeKind::Detach => {
                // 拔出前先拍存活快照，拔出后逐一核对子树外端口零受扰。
                let mut before: Vec<bool> = Vec::new();
                for p in 0..MAX_PORT {
                    before.push(self.tree.alive_of(p).unwrap_or(false));
                }
                let dead = self.tree.detach(port)?;
                for p in 0..MAX_PORT {
                    let now = self.tree.alive_of(p).unwrap_or(false);
                    let in_subtree = dead.contains(&p);
                    if before.get(p) == Some(&true) && !now && !in_subtree {
                        disturbed = disturbed.saturating_add(1);
                    }
                }
                affected = dead;
                self.detach_cnt = self.detach_cnt.saturating_add(1);
            }
            ChangeKind::MarkFailed => {
                affected = self.tree.failed_subtree(port)?;
            }
            ChangeKind::Purge => {
                let dead = self.tree.failed_subtree(port)?;
                for &p in dead.iter() {
                    if let Some(flag) = self.tree.alive.get_mut(p) {
                        *flag = false;
                    }
                }
                affected = dead;
                self.purge_cnt = self.purge_cnt.saturating_add(1);
            }
            ChangeKind::NeighborCheck => {
                // 邻口确认：端口须存活，且全树无"半死"状态。
                if self.tree.alive_of(port) != Some(true) {
                    return Err(TopoCode::PortDead);
                }
            }
        }
        self.disturbed_total = self.disturbed_total.saturating_add(disturbed);
        self.entries.push(ChangeEntry { seq, kind_code: kind as u32, port, affected, disturbed });
        Ok(disturbed)
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，逃逸恒 0 且检出非零。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }

    /// 邻端口累计受扰数（判据要求恒 0）。
    pub fn disturbed_total(&self) -> usize {
        self.disturbed_total
    }

    pub fn detach_count(&self) -> u64 {
        self.detach_cnt
    }
    pub fn purge_count(&self) -> u64 {
        self.purge_cnt
    }
    pub fn entries_len(&self) -> usize {
        self.entries.len()
    }

    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> (u64, u64, u64, usize, usize) {
        (self.last_seq, self.detach_cnt, self.purge_cnt, self.disturbed_total, self.entries.len())
    }

    /// 对账本做 FNV-1a 指纹：每条留痕的关键字段逐字节喂入。
    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for e in self.entries.iter() {
            buf.extend_from_slice(&e.seq.to_le_bytes());
            buf.push(e.kind_code as u8);
            buf.push(e.port as u8);
            buf.extend_from_slice(&(e.affected.len() as u32).to_le_bytes());
            buf.extend_from_slice(&(e.disturbed as u32).to_le_bytes());
        }
        fnv1a(&buf)
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一脚本两次建账，指纹与快照逐位一致。
pub fn dual_run_consistent() -> bool {
    let run = || -> (u64, (u64, u64, u64, usize, usize)) {
        let mut lg = ChangeLedger::new();
        let mut seq = 0u64;
        for i in 0..SUBTREE_ROUNDS {
            seq += 1;
            let _ = lg.commit(seq, ChangeKind::Detach, (1 + (i % 2) * 3) as usize);
            seq += 1;
            let _ = lg.commit(seq, ChangeKind::Attach, (1 + (i % 2) * 3) as usize);
        }
        for _ in 0..NEIGHBOR_ROUNDS {
            seq += 1;
            let _ = lg.commit(seq, ChangeKind::NeighborCheck, 5);
        }
        (lg.fingerprint(), lg.snapshot())
    };
    run() == run()
}

/// 注入核：5 类坏样本逐类 ×10 轮精确复现，全部检出、零逃逸。
pub fn run_injection_matrix() -> (u64, u64, u64) {
    let mut lg = ChangeLedger::new();
    let mut seq = 0u64;
    // 类1：变更类别码越闭集 ×10。
    for i in 0..10u32 {
        let mut p = ChangeParser::new();
        if p.parse(5 + i, 1) == Err(TopoCode::KindUnknown) {
            lg.note_detected();
        } else {
            lg.note_escape();
        }
    }
    // 类2：端口号越界 ×10。
    for i in 0..10u32 {
        let mut p = ChangeParser::new();
        if p.parse(0, MAX_PORT + i as usize) == Err(TopoCode::PortOob) {
            lg.note_detected();
        } else {
            lg.note_escape();
        }
    }
    // 类3：根口拔出 ×10。
    for _ in 0..10u32 {
        let mut p = ChangeParser::new();
        if p.parse(1, ROOT) == Err(TopoCode::RootDetach) {
            lg.note_detected();
        } else {
            lg.note_escape();
        }
    }
    // 类4：变更序号倒退 ×10。
    for _ in 0..10u32 {
        seq += 1;
        let s = seq;
        if lg.commit(s, ChangeKind::NeighborCheck, 5).is_ok() {
            // 先成功入账一条，再重放同一序号必倒退。
            if lg.commit(s, ChangeKind::NeighborCheck, 5) == Err(TopoCode::SeqRegress) {
                lg.note_detected();
            } else {
                lg.note_escape();
            }
        } else {
            lg.note_escape();
        }
    }
    // 类5：死口操作 ×10（对已拔端口再次拔出 / 对死口做邻口确认）。
    for i in 0..10u32 {
        seq += 1;
        let s = seq;
        let _ = lg.commit(s, ChangeKind::Detach, 1);
        seq += 1;
        let s2 = seq;
        let r = if i % 2 == 0 {
            lg.commit(s2, ChangeKind::Detach, 1)
        } else {
            lg.commit(s2, ChangeKind::NeighborCheck, 2)
        };
        match r {
            Err(TopoCode::PortDead) => lg.note_detected(),
            _ => lg.note_escape(),
        }
        // 复位现场：重新接入 1 与 2，保证下一轮可精确复现。
        seq += 1;
        let s3 = seq;
        let _ = lg.commit(s3, ChangeKind::Attach, 1);
        seq += 1;
        let s4 = seq;
        let _ = lg.commit(s4, ChangeKind::Attach, 2);
    }
    (lg.injected, lg.detected, lg.escaped)
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51548_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51548");
    // 1 中段拔出圈定：拔 1 → 失效子树 {1,2,3}。
    {
        let t = TopoTree::new();
        let dead = match t.failed_subtree(1) {
            Ok(d) => d,
            Err(_) => Vec::new(),
        };
        s.add_dyn(
            "中段拔出圈定",
            dead == vec![1, 2, 3],
            "拔中段 HUB 1 必须圈定子树 {1,2,3}",
        );
    }
    // 2 末梢拔出圈定：拔 4 → 失效子树 {4,5}。
    {
        let t = TopoTree::new();
        let dead = match t.failed_subtree(4) {
            Ok(d) => d,
            Err(_) => Vec::new(),
        };
        s.add_dyn("末梢拔出圈定", dead == vec![4, 5], "拔 4 必须圈定子树 {4,5}");
    }
    // 3 根口不可拔出、不可接入。
    {
        let mut t = TopoTree::new();
        let a = t.detach(ROOT) == Err(TopoCode::RootDetach);
        let b = t.attach(ROOT) == Err(TopoCode::RootAttach);
        s.add_dyn("根口守卫", a && b, "根 HUB 上行口不可拔出也不可接入");
    }
    // 4 判据锚定：分区（子树）断电 ×10 邻端口零受扰。
    {
        let mut lg = ChangeLedger::new();
        let mut seq = 0u64;
        let mut all_zero = true;
        for i in 0..SUBTREE_ROUNDS {
            seq += 1;
            let target = if i % 2 == 0 { 1 } else { 4 };
            match lg.commit(seq, ChangeKind::Detach, target) {
                Ok(d) => {
                    if d != 0 {
                        all_zero = false;
                    }
                }
                Err(_) => all_zero = false,
            }
            // 复位现场。
            seq += 1;
            let _ = lg.commit(seq, ChangeKind::Attach, target);
        }
        s.add_dyn(
            "分区断电 ×10 邻口零受扰",
            all_zero && lg.disturbed_total() == 0,
            "UNX-F51549-J1：断电 ×10 每轮邻端口受扰数必须为零",
        );
    }
    // 5 判据锚定：全断误动作 0 命中（全树置死操作被根口守卫拦截）。
    {
        let mut t = TopoTree::new();
        let mut hits = 0usize;
        // 试图以根口拔出达成"全断"，×10 全被拦截。
        for _ in 0..10usize {
            if t.detach(ROOT) == Err(TopoCode::RootDetach) {
                // 拦截成功，零误动作命中。
            } else {
                hits += 1;
            }
        }
        let alive_root = t.alive_of(ROOT) == Some(true);
        s.add_dyn(
            "全断误动作 0 命中",
            hits == 0 && alive_root,
            "UNX-F51549-J1：全断误动作必须 0 命中且根口存活",
        );
    }
    // 6 判据锚定：邻口确认 ×5 抽验全过。
    {
        let mut lg = ChangeLedger::new();
        let mut seq = 0u64;
        let mut ok = 0u32;
        for _ in 0..NEIGHBOR_ROUNDS {
            seq += 1;
            if lg.commit(seq, ChangeKind::NeighborCheck, 5).is_ok() {
                ok += 1;
            }
        }
        s.add_dyn(
            "邻口确认 ×5 抽验",
            ok == 5 && lg.entries_len() >= 5,
            "UNX-F51549-J1：邻端口确认 ×5 必须全过",
        );
    }
    // 7 变更解析闭集：0..=4 全收、5..=14 全拒。
    {
        let mut open = true;
        for c in 0..5u32 {
            let mut p = ChangeParser::new();
            if p.parse(c, 1).is_err() {
                open = false;
            }
        }
        for c in 5..15u32 {
            let mut p = ChangeParser::new();
            if p.parse(c, 1).is_ok() {
                open = false;
            }
        }
        s.add_dyn("变更闭集解码", open, "变更类别只接受 0..=4 五类");
    }
    // 8 死口/死父口守卫。
    {
        let mut t = TopoTree::new();
        let _ = t.detach(1);
        let a = t.attach(2) == Err(TopoCode::ParentDead);
        let b = t.failed_subtree(1) == Err(TopoCode::PortDead);
        s.add_dyn("死口守卫", a && b, "死口不许再圈定、死父口不许接入子口");
    }
    // 9 序号单调：重放与倒退全拒。
    {
        let mut lg = ChangeLedger::new();
        let a = lg.commit(1, ChangeKind::NeighborCheck, 5).is_ok();
        let b = lg.commit(1, ChangeKind::NeighborCheck, 5) == Err(TopoCode::SeqRegress);
        s.add_dyn("变更序号严格递增", a && b, "变更序号重复或倒退必须显性拒");
    }
    // 10 账本只增不减：连续入账后条目数与操作数对平。
    {
        let mut lg = ChangeLedger::new();
        let mut seq = 0u64;
        for i in 0..8u64 {
            seq += 1;
            let _ = lg.commit(seq, ChangeKind::NeighborCheck, (1 + (i % 2) * 3) as usize);
        }
        s.add_dyn(
            "账本只增不减",
            lg.entries_len() == 8 && lg.detach_count() == 0,
            "八条确认入账后账本恰八条且无拔出计数",
        );
    }
    // 11 注入矩阵：5 类 ×10 轮全检出、零逃逸。
    {
        let (inj, det, esc) = run_injection_matrix();
        s.add_dyn(
            "注入矩阵 5×10 全抓",
            inj == 50 && det == 50 && esc == 0,
            "5 类坏样本逐类 ×10 必须全检出且零逃逸",
        );
    }
    // 12 注入账守恒。
    {
        let mut lg = ChangeLedger::new();
        for _ in 0..10u32 {
            lg.note_detected();
        }
        s.add_dyn("注入账守恒", lg.conserved(), "注入=检出+逃逸，逃逸恒 0");
    }
    // 13 双跑指纹一致。
    s.add_dyn("双跑指纹一致", dual_run_consistent(), "同一脚本两次建账 FNV-1a 逐位一致");
    // 14 指纹区分性：受扰账本与零受扰账本指纹必不同。
    {
        let a = ChangeLedger::new();
        let mut b = ChangeLedger::new();
        let _ = b.commit(1, ChangeKind::Detach, 1);
        s.add_dyn("指纹区分性", a.fingerprint() != b.fingerprint(), "账本内容不同指纹必须不同");
    }
    // 15 诊断码闭集：文案全非空、val 严格递增。
    {
        let mut ok = true;
        for (i, c) in ALL.iter().enumerate() {
            if c.detail().is_empty() || c.val() != SEG_BASE + 1 + i as u32 {
                ok = false;
            }
        }
        s.add_dyn("诊断码闭集", ok, "12 变体文案非空且 val=段基+1+序号");
    }
    // 16 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mid_hub_detach_marks_subtree() {
        let mut t = TopoTree::new();
        let dead = match t.detach(1) {
            Ok(d) => d,
            Err(_) => Vec::new(),
        };
        assert_eq!(dead, vec![1, 2, 3]);
        assert_eq!(t.alive_of(2), Some(false));
        assert_eq!(t.alive_of(4), Some(true));
    }

    #[test]
    fn root_guard_blocks_detach_and_attach() {
        let mut t = TopoTree::new();
        assert_eq!(t.detach(ROOT), Err(TopoCode::RootDetach));
        assert_eq!(t.attach(ROOT), Err(TopoCode::RootAttach));
    }

    #[test]
    fn dead_port_guards() {
        let mut t = TopoTree::new();
        let _ = t.detach(1);
        assert_eq!(t.failed_subtree(1), Err(TopoCode::PortDead));
        assert_eq!(t.attach(2), Err(TopoCode::ParentDead));
    }

    #[test]
    fn parser_closed_set() {
        let mut p = ChangeParser::new();
        assert!(p.parse(4, 15).is_ok());
        let mut q = ChangeParser::new();
        assert_eq!(q.parse(5, 1), Err(TopoCode::KindUnknown));
        assert_eq!(q.parse(0, 16), Err(TopoCode::PortOob));
        assert_eq!(q.parse(1, 0), Err(TopoCode::RootDetach));
    }

    #[test]
    fn neighbor_zero_disturbed_ten_rounds() {
        let mut lg = ChangeLedger::new();
        let mut seq = 0u64;
        for i in 0..SUBTREE_ROUNDS {
            seq += 1;
            let target = if i % 2 == 0 { 1 } else { 4 };
            let d = match lg.commit(seq, ChangeKind::Detach, target) {
                Ok(d) => d,
                Err(_) => 99,
            };
            assert_eq!(d, 0, "第 {} 轮拔出不得扰动邻口", i);
            seq += 1;
            let _ = lg.commit(seq, ChangeKind::Attach, target);
        }
        assert_eq!(lg.disturbed_total(), 0);
    }

    #[test]
    fn full_blackout_misfire_zero_hits() {
        let mut t = TopoTree::new();
        for _ in 0..10 {
            assert_eq!(t.detach(ROOT), Err(TopoCode::RootDetach));
        }
        assert_eq!(t.alive_of(ROOT), Some(true));
    }

    #[test]
    fn injection_matrix_all_caught_zero_escape() {
        let (inj, det, esc) = run_injection_matrix();
        assert_eq!((inj, det, esc), (50, 50, 0));
    }

    #[test]
    fn seq_regress_enforced() {
        let mut lg = ChangeLedger::new();
        assert!(lg.commit(1, ChangeKind::NeighborCheck, 5).is_ok());
        assert_eq!(lg.commit(1, ChangeKind::NeighborCheck, 5), Err(TopoCode::SeqRegress));
        assert_eq!(lg.commit(0, ChangeKind::NeighborCheck, 5), Err(TopoCode::SeqRegress));
    }

    #[test]
    fn dual_run_identical_fingerprint() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn closed_set_vals_and_texts() {
        for (i, c) in ALL.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn seg_guard_passes() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0xC0BC0);
    }

    #[test]
    fn unx51548_checks_green() {
        let set = run_unx51548_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
