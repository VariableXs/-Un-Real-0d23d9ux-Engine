//! UNX-F11243 DT_NEEDED 依赖递归解析（0x1f2b0 段）
//!
//! 模型：每个被装载对象带一张 DT_NEEDED 引用表，本模型压成 4 位邻接位图，
//! 位 j 置位表示「我依赖第 j 个对象」。从根节点 0 出发用显式栈做深度优先，
//! 后序 emit 的序列就是装载序：依赖永远先于依赖者。同一依赖被两条路径引用
//! 只装载一次（菱形共享去重），不可达节点不装载但也不报错。
//!
//! 规则面：名号非零且样本内唯一、引用下标必须 < 节点数、不得自指、不得成环
//! （灰色回边即环）、链深不得超过 MAX_DEPTH=3。判定顺序固定为「名号 → 引用
//! 越界与自指 → 深度 → 环」，同一坏样本永远报同一个码，便于账本比对。
//!
//! 账本：每样本一条 slot（邻接位图 + 后序序列 + 深度 + 形状 + 判定序号），
//! 另按深度分桶（0 号桶收不装载的坏样本）、按形状分桶，并记环/自指计数。
//! fold 把每条 slot 与全部分桶卷进 FNV-1a 指纹；replay 重算规则并要求观测
//! 覆盖齐全、判定与重算一致、各桶合计等于账本条数；freeze 先 replay 再落根。
//!
//! 拒止面：登记序（id 必须等于当前条数，跳号与回填都拒）、重号观测、越界样本
//! 号、冻结后登记/观测、未观测即重放、空账重放、指纹漂移、分桶不平衡。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f2b0;

/// 依赖名号为零
pub const ND_EZERO: u32 = SEG_BASE + 0x01;
/// 名号越界 / 同一样本内名号重复
pub const ND_EID: u32 = SEG_BASE + 0x02;
/// 自指（自己依赖自己）
pub const ND_ESELF: u32 = SEG_BASE + 0x03;
/// 循环依赖（灰色回边）
pub const ND_ECYCLE: u32 = SEG_BASE + 0x04;
/// 链深超过 MAX_DEPTH
pub const ND_EDEPTH: u32 = SEG_BASE + 0x05;
/// 引用下标越过节点数
pub const ND_ENODE: u32 = SEG_BASE + 0x06;
/// 登记重号
pub const ND_EDUP: u32 = SEG_BASE + 0x07;
/// 登记序违例 / 观测目标未登记
pub const ND_EORDER: u32 = SEG_BASE + 0x08;
/// 未观测即重放
pub const ND_ENOBS: u32 = SEG_BASE + 0x09;
/// 冻结后登记或观测
pub const ND_EFROZEN: u32 = SEG_BASE + 0x0a;
/// 重放指纹不一致 / 规则重算不一致
pub const ND_EMISMATCH: u32 = SEG_BASE + 0x0b;
/// 分桶不平衡
pub const ND_EBAL: u32 = SEG_BASE + 0x0c;
/// 空账重放
pub const ND_EEMPTY: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------- 域常量
/// 单样本最大节点数（根 + 三层）
pub const MAX_NODES: usize = 4;
/// 账本容量
pub const MAX_CASES: usize = 12;
/// 允许的最大链深（根算第 1 层）
pub const MAX_DEPTH: u8 = 3;
/// 根节点下标
pub const ROOT: usize = 0;
/// 深度分桶数：0..=3 再多一个坏样本桶
pub const DEPTH_BUCKETS: usize = 5;
/// 形状分桶数
pub const SHAPE_BUCKETS: usize = 5;
/// 形状：其它
pub const SHAPE_OTHER: u8 = 0;
/// 形状：单链
pub const SHAPE_CHAIN: u8 = 1;
/// 形状：分叉/菱形
pub const SHAPE_BRANCH: u8 = 2;
/// 形状：环
pub const SHAPE_CYCLE: u8 = 3;
/// 形状：自指
pub const SHAPE_SELF: u8 = 4;
/// 未判定期望值
pub const UNRULED: u8 = 0xff;
/// ENOENT（依赖不存在）
pub const ENOENT: u32 = 2;
/// EINVAL
pub const EINVAL: u32 = 22;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

// ---------------------------------------------------------------- 样本图
/// 一个依赖子图样本：名号表 + 邻接位图 + 节点数。
#[derive(Clone, Copy, Debug)]
pub struct Case {
    ids: [u16; MAX_NODES],
    adj: [u8; MAX_NODES],
    n: u8,
}

impl Case {
    /// 空样本（零节点）。
    pub const fn blank() -> Case {
        Case { ids: [0; MAX_NODES], adj: [0; MAX_NODES], n: 0 }
    }

    /// 按表构造。
    pub const fn make(ids: [u16; MAX_NODES], adj: [u8; MAX_NODES], n: u8) -> Case {
        Case { ids, adj, n }
    }

    /// 取第 i 个名号。
    pub fn node(&self, i: usize) -> u16 {
        match self.ids.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 取第 i 个节点的依赖位图。
    pub fn edges(&self, i: usize) -> u8 {
        match self.adj.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 节点数。
    pub fn len(&self) -> usize {
        self.n as usize
    }

    /// 改一条邻接边（构造边沿样本用）。
    pub fn set_edges(&mut self, i: usize, bits: u8) -> bool {
        match self.adj.get_mut(i) {
            Some(c) => {
                *c = bits;
                true
            }
            None => false,
        }
    }
}

/// 一次解析的结果：后序序列 + 深度 + 扇出。
#[derive(Clone, Copy, Debug)]
pub struct Plan {
    pub seq: [u8; MAX_NODES],
    pub seq_n: u8,
    pub depth: u8,
    pub loaded: u8,
    pub ignored: u8,
    pub fanout: u8,
}

impl Plan {
    /// 空计划。
    pub const fn empty() -> Plan {
        Plan { seq: [0; MAX_NODES], seq_n: 0, depth: 0, loaded: 0, ignored: 0, fanout: 0 }
    }

    /// 后序序列翻成名号序列。
    pub fn order_ids(&self, c: &Case) -> [u16; MAX_NODES] {
        let mut out = [0u16; MAX_NODES];
        let mut i = 0usize;
        while i < self.seq_n as usize {
            if let (Some(p), Some(dst)) = (self.seq.get(i), out.get_mut(i)) {
                *dst = c.node(*p as usize);
            }
            i = i.wrapping_add(1);
        }
        out
    }

    /// 第 i 步装的是哪个下标。
    pub fn at(&self, i: usize) -> Option<u8> {
        if i >= self.seq_n as usize {
            return None;
        }
        self.seq.get(i).copied()
    }
}

// ---------------------------------------------------------------- 解析规则
/// 从根 0 出发做显式栈深度优先，后序给出装载序。
///
/// 判定顺序：名号 → 引用越界/自指 → 深度 → 环。灰色回边判环，黑色回边是
/// 共享依赖（跳过，不重复装载），不可达节点只记 ignored。
pub fn plan(c: &Case) -> Result<Plan, u32> {
    let n = c.len();
    if n == 0 || n > MAX_NODES {
        return Err(ND_EID);
    }
    let mut i = 0usize;
    while i < n {
        if c.node(i) == 0 {
            return Err(ND_EZERO);
        }
        let mut j = 0usize;
        while j < i {
            if c.node(j) == c.node(i) {
                return Err(ND_EID);
            }
            j = j.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    let mut color = [0u8; MAX_NODES];
    let mut stack = [0u8; MAX_NODES];
    let mut cursor = [0u8; MAX_NODES];
    let mut seq = [0u8; MAX_NODES];
    let mut sn = 0usize;
    let mut sp = 1usize;
    let mut depth = 1u8;
    if let Some(v) = color.get_mut(ROOT) {
        *v = 1;
    }
    if let Some(v) = stack.get_mut(ROOT) {
        *v = 0;
    }
    while sp > 0 {
        let top = sp - 1;
        let u = match stack.get(top) {
            Some(v) => *v as usize,
            None => return Err(ND_EID),
        };
        let bits = c.edges(u);
        let from = match cursor.get(top) {
            Some(v) => *v as usize,
            None => return Err(ND_EID),
        };
        let mut bit = from;
        let mut child: Option<usize> = None;
        while bit < MAX_NODES {
            if bits & (1u8 << bit) != 0 {
                if bit >= n {
                    return Err(ND_ENODE);
                }
                child = Some(bit);
                break;
            }
            bit = bit.wrapping_add(1);
        }
        match child {
            Some(v) => {
                if let Some(cu) = cursor.get_mut(top) {
                    *cu = (v as u8).wrapping_add(1);
                }
                if v == u {
                    return Err(ND_ESELF);
                }
                let seen = match color.get(v) {
                    Some(x) => *x,
                    None => return Err(ND_EID),
                };
                if seen == 2 {
                    continue;
                }
                if seen == 1 {
                    return Err(ND_ECYCLE);
                }
                let d = (sp as u8).wrapping_add(1);
                if d > MAX_DEPTH {
                    return Err(ND_EDEPTH);
                }
                if d > depth {
                    depth = d;
                }
                if let Some(sv) = stack.get_mut(sp) {
                    *sv = v as u8;
                }
                if let Some(cv) = cursor.get_mut(sp) {
                    *cv = 0;
                }
                if let Some(col) = color.get_mut(v) {
                    *col = 1;
                }
                sp = sp.wrapping_add(1);
            }
            None => {
                if let Some(sv) = seq.get_mut(sn) {
                    *sv = u as u8;
                }
                sn = sn.wrapping_add(1);
                if let Some(col) = color.get_mut(u) {
                    *col = 2;
                }
                sp = sp.saturating_sub(1);
            }
        }
    }
    let mut fan = 0u8;
    let mut k = 0usize;
    while k < sn {
        let u = match seq.get(k) {
            Some(v) => *v as usize,
            None => 0,
        };
        let bits = c.edges(u);
        let mut cnt = 0u8;
        let mut b = 0usize;
        while b < MAX_NODES {
            if bits & (1u8 << b) != 0 && b < n {
                cnt = cnt.wrapping_add(1);
            }
            b = b.wrapping_add(1);
        }
        if cnt > fan {
            fan = cnt;
        }
        k = k.wrapping_add(1);
    }
    let ignored = (n as u8).wrapping_sub(sn as u8);
    Ok(Plan { seq, seq_n: sn as u8, depth, loaded: sn as u8, ignored, fanout: fan })
}

/// 形状分类：只看边数与扇出，环/自指由判定侧覆盖。
pub fn classify(c: &Case) -> u8 {
    let n = c.len();
    let mut edges = 0u32;
    let mut fan = 0u8;
    let mut i = 0usize;
    while i < n {
        let bits = c.edges(i);
        let mut cnt = 0u8;
        let mut b = 0usize;
        while b < MAX_NODES {
            if bits & (1u8 << b) != 0 {
                if b >= n {
                    return SHAPE_OTHER;
                }
                cnt = cnt.wrapping_add(1);
            }
            b = b.wrapping_add(1);
        }
        if cnt > fan {
            fan = cnt;
        }
        edges = edges.wrapping_add(cnt as u32);
        i = i.wrapping_add(1);
    }
    if fan >= 2 {
        SHAPE_BRANCH
    } else if edges + 1 == n as u32 {
        SHAPE_CHAIN
    } else {
        SHAPE_OTHER
    }
}

/// 每条边（u 依赖 v）都必须满足 v 在装载序里先出现。
pub fn deps_first(c: &Case, p: &Plan) -> bool {
    let mut pu = 0usize;
    while pu < p.seq_n as usize {
        let u = match p.seq.get(pu) {
            Some(v) => *v as usize,
            None => return false,
        };
        let bits = c.edges(u);
        let mut b = 0usize;
        while b < MAX_NODES {
            if bits & (1u8 << b) != 0 {
                let mut pb = 0usize;
                let mut found = false;
                while pb < p.seq_n as usize {
                    if p.seq.get(pb).copied() == Some(b as u8) {
                        found = true;
                        break;
                    }
                    pb = pb.wrapping_add(1);
                }
                if !found || pb >= pu {
                    return false;
                }
            }
            b = b.wrapping_add(1);
        }
        pu = pu.wrapping_add(1);
    }
    true
}

// ---------------------------------------------------------------- 账本
/// 一条样本账：样本图 + 判定结果 + 观测位。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    pub case: Case,
    pub seq: [u8; MAX_NODES],
    pub seq_n: u8,
    pub depth: u8,
    pub shape: u8,
    pub verdict: u8,
    pub expect: u8,
    pub observed: bool,
}

impl Slot {
    /// 空账位。
    pub const fn blank() -> Slot {
        Slot {
            case: Case::blank(),
            seq: [0; MAX_NODES],
            seq_n: 0,
            depth: 0,
            shape: SHAPE_OTHER,
            verdict: 0,
            expect: UNRULED,
            observed: false,
        }
    }

    /// 本样本是否装载成功。
    pub fn ok(&self) -> bool {
        self.verdict == 0
    }
}

/// DT_NEEDED 递归解析账本。
#[derive(Clone, Debug)]
pub struct DepLedger {
    slots: [Option<Slot>; MAX_CASES],
    n: usize,
    depth_hist: [u32; DEPTH_BUCKETS],
    shapes: [u32; SHAPE_BUCKETS],
    ok_cases: u32,
    cycles: u32,
    observed: u32,
    mismatch: u32,
    accepts: u64,
    rejects: u64,
    root: Option<u64>,
    frozen: bool,
}

impl DepLedger {
    /// 空账。
    pub const fn new() -> DepLedger {
        DepLedger {
            slots: [None; MAX_CASES],
            n: 0,
            depth_hist: [0; DEPTH_BUCKETS],
            shapes: [0; SHAPE_BUCKETS],
            ok_cases: 0,
            cycles: 0,
            observed: 0,
            mismatch: 0,
            accepts: 0,
            rejects: 0,
            root: None,
            frozen: false,
        }
    }

    /// 记一次拒止并回传错误码。
    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 已登样本数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 装载成功样本数。
    pub fn ok_cases(&self) -> u32 {
        self.ok_cases
    }

    /// 判环次数。
    pub fn cycles(&self) -> u32 {
        self.cycles
    }

    /// 已观测条数。
    pub fn observed(&self) -> u32 {
        self.observed
    }

    /// 某深度桶计数。
    pub fn depth_bucket(&self, d: usize) -> u32 {
        match self.depth_hist.get(d) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 某形状桶计数。
    pub fn shape_count(&self, s: usize) -> u32 {
        match self.shapes.get(s) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 取一条样本账。
    pub fn slot(&self, i: usize) -> Option<Slot> {
        match self.slots.get(i) {
            Some(v) => *v,
            None => None,
        }
    }

    /// 定锚指纹。
    pub fn root_value(&self) -> Option<u64> {
        self.root
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 登记核心：门禁（冻结 → 越界 → 回填 → 跳号）后跑规则并落账。
    fn put(&mut self, id: usize, expect: u8) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(ND_EFROZEN));
        }
        if id >= MAX_CASES {
            return Err(self.fail(ND_EID));
        }
        if id < self.n {
            return Err(self.fail(ND_EDUP));
        }
        if id > self.n {
            return Err(self.fail(ND_EORDER));
        }
        let (ids, adj, cn) = spec_of(id);
        let case = Case::make(ids, adj, cn);
        let mut slot = Slot::blank();
        slot.case = case;
        slot.expect = expect;
        let base_shape = classify(&case);
        let mut depth = 0u8;
        match plan(&case) {
            Ok(p) => {
                slot.seq = p.seq;
                slot.seq_n = p.seq_n;
                slot.depth = p.depth;
                slot.shape = base_shape;
                depth = p.depth;
                self.ok_cases = self.ok_cases.wrapping_add(1);
            }
            Err(code) => {
                slot.verdict = (code - SEG_BASE) as u8;
                slot.shape = match code {
                    ND_ECYCLE => SHAPE_CYCLE,
                    ND_ESELF => SHAPE_SELF,
                    _ => base_shape,
                };
                if code == ND_ECYCLE {
                    self.cycles = self.cycles.wrapping_add(1);
                }
            }
        }
        if let Some(c) = self.depth_hist.get_mut(depth as usize) {
            *c = c.wrapping_add(1);
        }
        if let Some(c) = self.shapes.get_mut(slot.shape as usize) {
            *c = c.wrapping_add(1);
        }
        let at = self.n;
        if let Some(cell) = self.slots.get_mut(at) {
            *cell = Some(slot);
        }
        self.n = self.n.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        if expect != UNRULED && (slot.verdict != 0 || depth != expect) {
            self.mismatch = self.mismatch.wrapping_add(1);
            return Err(ND_EMISMATCH);
        }
        Ok(self.fold())
    }

    /// 按样本号登记，判定由规则算出。
    pub fn register(&mut self, id: usize) -> Result<u64, u32> {
        self.put(id, UNRULED)
    }

    /// 登记并声明期望链深；真实链深不符记一次失配。
    pub fn register_ruled(&mut self, id: usize, expect_depth: u8) -> Result<u64, u32> {
        self.put(id, expect_depth)
    }
}

impl DepLedger {
    /// 观测一条样本账：未登记不配观测，重号观测拒，冻结后拒。
    pub fn observe(&mut self, id: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(ND_EFROZEN));
        }
        if id >= self.n {
            return Err(self.fail(ND_EORDER));
        }
        let hit = match self.slots.get(id) {
            Some(v) => *v,
            None => None,
        };
        match hit {
            Some(mut s) => {
                if s.observed {
                    return Err(self.fail(ND_EDUP));
                }
                s.observed = true;
                if let Some(cell) = self.slots.get_mut(id) {
                    *cell = Some(s);
                }
                self.observed = self.observed.wrapping_add(1);
                Ok(())
            }
            None => Err(self.fail(ND_EORDER)),
        }
    }

    /// 全账指纹：逐 slot（样本位图 + 判定 + 序列）再卷全部分桶与计数。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let Some(Some(s)) = self.slots.get(i) {
                let mut k = 0usize;
                while k < MAX_NODES {
                    h = (h ^ s.case.node(k) as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ s.case.edges(k) as u64).wrapping_mul(FNV_PRIME);
                    let sq = match s.seq.get(k) {
                        Some(v) => *v as u64,
                        None => 0,
                    };
                    h = (h ^ sq).wrapping_mul(FNV_PRIME);
                    k = k.wrapping_add(1);
                }
                h = (h ^ s.seq_n as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.depth as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.shape as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.verdict as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.expect as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.observed as u64).wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        let mut d = 0usize;
        while d < DEPTH_BUCKETS {
            if let Some(v) = self.depth_hist.get(d) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            d = d.wrapping_add(1);
        }
        let mut sh = 0usize;
        while sh < SHAPE_BUCKETS {
            if let Some(v) = self.shapes.get(sh) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            sh = sh.wrapping_add(1);
        }
        h = (h ^ self.n as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.ok_cases as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.cycles as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects).wrapping_mul(FNV_PRIME);
        h = (h ^ self.mismatch as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 重放：观测覆盖齐全、逐条规则重算一致、分桶合计等于账本条数、
    /// 已有定锚则指纹必须对得上。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.n == 0 {
            return Err(self.fail(ND_EEMPTY));
        }
        if self.observed as usize != self.n {
            return Err(self.fail(ND_ENOBS));
        }
        if self.mismatch != 0 {
            return Err(self.fail(ND_EMISMATCH));
        }
        let mut i = 0usize;
        let mut ok_sum = 0u32;
        while i < self.n {
            let cell = self.slots.get(i).copied();
            let s = match cell {
                Some(Some(v)) => v,
                _ => return Err(self.fail(ND_EORDER)),
            };
            let again = plan(&s.case);
            let coherent = match again {
                Ok(p) => s.verdict == 0 && p.seq_n == s.seq_n && p.depth == s.depth,
                Err(code) => (code - SEG_BASE) as u8 == s.verdict,
            };
            if !coherent {
                return Err(self.fail(ND_EMISMATCH));
            }
            if s.verdict == 0 {
                ok_sum = ok_sum.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        if ok_sum != self.ok_cases {
            return Err(self.fail(ND_EBAL));
        }
        let mut hist_sum = 0u32;
        let mut d = 0usize;
        while d < DEPTH_BUCKETS {
            if let Some(v) = self.depth_hist.get(d) {
                hist_sum = hist_sum.wrapping_add(*v);
            }
            d = d.wrapping_add(1);
        }
        if hist_sum as usize != self.n {
            return Err(self.fail(ND_EBAL));
        }
        let mut shape_sum = 0u32;
        let mut sh = 0usize;
        while sh < SHAPE_BUCKETS {
            if let Some(v) = self.shapes.get(sh) {
                shape_sum = shape_sum.wrapping_add(*v);
            }
            sh = sh.wrapping_add(1);
        }
        if shape_sum as usize != self.n {
            return Err(self.fail(ND_EBAL));
        }
        let v = self.fold();
        match self.root {
            Some(r) if r != v => Err(self.fail(ND_EMISMATCH)),
            _ => Ok(v),
        }
    }

    /// 先重放再封账；二次冻结一律拒（不幂等放行）。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(ND_EFROZEN));
        }
        let v = self.replay()?;
        self.root = Some(v);
        self.frozen = true;
        Ok(v)
    }
}

// ---------------------------------------------------------------- 样本表
/// 样本表：名号 + 邻接位图 + 节点数。
///
/// 0 三层链 A→B→C；1 四层链（超深）；2 菱形共享叶；3 二元环；4 自指；
/// 5 悬空引用；6 含孤岛；7 一拖三扇出；8 共享叶分叉；9 单件；
/// 10 树杈；11 三元环。
pub fn spec_of(id: usize) -> ([u16; MAX_NODES], [u8; MAX_NODES], u8) {
    match id {
        0 => ([10, 11, 12, 0], [0b0010, 0b0100, 0, 0], 3),
        1 => ([20, 21, 22, 23], [0b0010, 0b0100, 0b1000, 0], 4),
        2 => ([30, 31, 32, 33], [0b0110, 0b1000, 0b1000, 0], 4),
        3 => ([40, 41, 0, 0], [0b0010, 0b0001, 0, 0], 2),
        4 => ([50, 0, 0, 0], [0b0001, 0, 0, 0], 1),
        5 => ([60, 61, 0, 0], [0b1000, 0, 0, 0], 2),
        6 => ([70, 71, 72, 0], [0b0010, 0, 0, 0], 3),
        7 => ([80, 81, 82, 83], [0b1110, 0, 0, 0], 4),
        8 => ([90, 91, 92, 93], [0b0110, 0b1000, 0, 0], 4),
        9 => ([100, 0, 0, 0], [0, 0, 0, 0], 1),
        10 => ([110, 111, 112, 113], [0b0010, 0b1100, 0, 0], 4),
        11 => ([120, 121, 122, 0], [0b0010, 0b0100, 0b0001, 0], 3),
        _ => ([0; MAX_NODES], [0; MAX_NODES], 0),
    }
}

/// 由样本号取图。
pub fn case_of(id: usize) -> Case {
    let (ids, adj, n) = spec_of(id);
    Case::make(ids, adj, n)
}

/// 建全账：12 样本登记 → 全部观测 → 封账。
pub fn build_ledger() -> DepLedger {
    let mut l = DepLedger::new();
    let mut i = 0usize;
    while i < MAX_CASES {
        let _ = l.register(i);
        i = i.wrapping_add(1);
    }
    let mut k = 0usize;
    while k < l.len() {
        let _ = l.observe(k);
        k = k.wrapping_add(1);
    }
    let _ = l.freeze();
    l
}

/// 四路独立拒止：登记序 / 重号 / 越界样本号 / 冻结后登记。
pub fn inject_four() -> [bool; 4] {
    // ① 空账跳号登记
    let mut a = DepLedger::new();
    let order = a.register(1).err() == Some(ND_EORDER) && a.len() == 0 && a.rejects == 1;

    // ② 同号二次登记
    let mut b = DepLedger::new();
    let first = b.register(0).is_ok();
    let dup = b.register(0).err() == Some(ND_EDUP) && b.len() == 1 && b.rejects == 1;

    // ③ 越界样本号
    let mut c = DepLedger::new();
    let range = c.register(MAX_CASES).err() == Some(ND_EID) && c.len() == 0;

    // ④ 冻结后登记
    let mut d = build_ledger();
    let frozen = d.register(0).err() == Some(ND_EFROZEN) && d.is_frozen();

    [order, first && dup, range, frozen]
}

// ---------------------------------------------------------------- 自检
/// 判据：UNX-F11243 三层依赖链装载序 + 循环依赖有账。
pub fn run_unx11243_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11243");
    let c0 = case_of(0);
    let c1 = case_of(1);
    let c2 = case_of(2);
    let c3 = case_of(3);
    let c4 = case_of(4);
    let c5 = case_of(5);
    let c6 = case_of(6);

    // D1 三层链后序装载（判据①：A→B→C 装载序与 Linux 一致）
    let mut d1 = false;
    let mut order0 = [0u16; MAX_NODES];
    if let Ok(p) = plan(&c0) {
        order0 = p.order_ids(&c0);
        d1 = p.seq_n == 3 && p.depth == 3 && p.loaded == 3 && order0 == [12, 11, 10, 0];
    }
    s.add_dyn("D1-三层链后序装载", d1, &format!("order={:?}", order0));

    // D2 菱形共享叶只装一次
    let mut d2 = false;
    let mut order2 = [0u16; MAX_NODES];
    if let Ok(p) = plan(&c2) {
        order2 = p.order_ids(&c2);
        d2 = p.seq_n == 4 && order2 == [33, 31, 32, 30];
    }
    s.add_dyn("D2-菱形共享依赖去重", d2, &format!("order={:?}", order2));

    // D3 深度上限：四层链拒
    s.add(
        "D3-链深上限",
        plan(&c1).err() == Some(ND_EDEPTH),
        "四层链超过 MAX_DEPTH=3 判 EDEPTH",
    );

    // D4 循环检测（判据②：循环依赖检测有账）
    s.add(
        "D4-循环依赖检测",
        plan(&c3).err() == Some(ND_ECYCLE) && plan(&case_of(11)).err() == Some(ND_ECYCLE),
        "二元环与三元环都判 ECYCLE",
    );

    // D5 自指拒止
    s.add(
        "D5-自指拒止",
        plan(&c4).err() == Some(ND_ESELF),
        "根自指判 ESELF 而非泛化为环",
    );

    // D6 悬空引用
    s.add(
        "D6-悬空引用拒止",
        plan(&c5).err() == Some(ND_ENODE),
        "依赖位指向节点数之外判 ENODE",
    );

    // D7 不可达节点不装载
    let mut d7 = false;
    let mut order6 = [0u16; MAX_NODES];
    if let Ok(p) = plan(&c6) {
        order6 = p.order_ids(&c6);
        d7 = p.loaded == 2 && p.ignored == 1 && order6 == [71, 70, 0, 0];
    }
    s.add_dyn("D7-孤岛不装载", d7, &format!("order={:?}", order6));

    // D8 全账逐边复验：依赖先于依赖者
    let l = build_ledger();
    let mut d8 = true;
    let mut checked = 0u32;
    let mut i = 0usize;
    while i < l.len() {
        match l.slot(i) {
            Some(sl) => {
                if sl.ok() {
                    if let Ok(p) = plan(&sl.case) {
                        checked = checked.wrapping_add(1);
                        if !deps_first(&sl.case, &p) {
                            d8 = false;
                        }
                    }
                }
            }
            None => d8 = false,
        }
        i = i.wrapping_add(1);
    }
    s.add_dyn("D8-依赖先于依赖者", d8 && checked >= 4, &format!("ok_samples={}", checked));

    // D9 形状分桶
    let chain = l.shape_count(SHAPE_CHAIN as usize);
    let branch = l.shape_count(SHAPE_BRANCH as usize);
    let cyc = l.shape_count(SHAPE_CYCLE as usize);
    let slf = l.shape_count(SHAPE_SELF as usize);
    let other = l.shape_count(SHAPE_OTHER as usize);
    s.add_dyn(
        "D9-形状分桶",
        chain == 3 && branch == 4 && cyc == 2 && slf == 1 && other == 2,
        &format!("chain={} branch={} cyc={} self={} other={}", chain, branch, cyc, slf, other),
    );

    // D10 深度分桶（坏样本落 0 号桶）
    s.add_dyn(
        "D10-深度分桶",
        l.depth_bucket(0) == 5 && l.depth_bucket(1) == 1 && l.depth_bucket(2) == 2
            && l.depth_bucket(3) == 4,
        &format!(
            "d0={} d1={} d2={} d3={}",
            l.depth_bucket(0), l.depth_bucket(1), l.depth_bucket(2), l.depth_bucket(3)
        ),
    );

    // D11 全账重放 + 二次冻结拒
    let mut r = l.clone();
    let rp = r.replay();
    let twice = r.freeze().err() == Some(ND_EFROZEN);
    s.add_dyn(
        "D11-全账重放与二次冻结",
        rp.is_ok() && r.root_value().is_some() && twice && l.observed() == MAX_CASES as u32,
        &format!("observed={} twice_frozen={}", l.observed(), twice),
    );

    // D12 登记序 / 重号 / 越界三路拒止
    let inj = inject_four();
    s.add(
        "D12-登记序重号越界拒止",
        inj[0] && inj[1] && inj[2],
        "跳号登记、重号登记、越界样本号一律拒",
    );

    // D13 冻结后写入拒止
    s.add(
        "D13-冻结后写入拒止",
        inj[3] && l.is_frozen() && l.cycles() == 2,
        "封账后 register 拒，环计数恰为 2",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11243_chain_order() {
        let c = case_of(0);
        match plan(&c) {
            Ok(p) => {
                assert_eq!(p.order_ids(&c), [12, 11, 10, 0]);
                assert_eq!(p.depth, 3);
                assert_eq!(p.loaded, 3);
                assert!(deps_first(&c, &p));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn f11243_bad_edges() {
        assert_eq!(plan(&case_of(1)).err(), Some(ND_EDEPTH));
        assert_eq!(plan(&case_of(3)).err(), Some(ND_ECYCLE));
        assert_eq!(plan(&case_of(4)).err(), Some(ND_ESELF));
        assert_eq!(plan(&case_of(5)).err(), Some(ND_ENODE));
        assert_eq!(plan(&case_of(11)).err(), Some(ND_ECYCLE));
        let zero = Case::blank();
        assert_eq!(plan(&zero).err(), Some(ND_EID));
        // 越界取字段安全回落为零，不 panic
        let c = case_of(0);
        assert_eq!(c.node(9), 0);
        assert_eq!(c.edges(9), 0);
    }

    #[test]
    fn f11243_ledger_replay() {
        let l = build_ledger();
        assert_eq!(l.len(), MAX_CASES);
        assert_eq!(l.observed(), MAX_CASES as u32);
        assert_eq!(l.ok_cases(), 7);
        assert_eq!(l.cycles(), 2);
        let root = l.root_value();
        assert!(root.is_some());
        let mut f = l.clone();
        assert_eq!(f.replay().ok(), root);
        assert_eq!(f.freeze().err(), Some(ND_EFROZEN));
        // 观测覆盖不全必须拒
        let mut u = DepLedger::new();
        assert!(u.register(0).is_ok());
        assert!(u.register(1).is_ok());
        assert!(u.observe(0).is_ok());
        assert_eq!(u.freeze().err(), Some(ND_ENOBS));
        // 重号观测拒
        assert_eq!(u.observe(0).err(), Some(ND_EDUP));
        assert_eq!(u.observe(9).err(), Some(ND_EORDER));
    }

    #[test]
    fn f11243_gates() {
        let mut a = DepLedger::new();
        assert_eq!(a.register(2).err(), Some(ND_EORDER));
        assert!(a.register(0).is_ok());
        assert_eq!(a.register(0).err(), Some(ND_EDUP));
        assert_eq!(a.register(MAX_CASES).err(), Some(ND_EID));
        assert_eq!(a.register_ruled(1, 99).err(), Some(ND_EMISMATCH));
        assert_eq!(a.len(), 2);
    }

    #[test]
    fn f11243_checks_green() {
        let cs = run_unx11243_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed());
    }
}
