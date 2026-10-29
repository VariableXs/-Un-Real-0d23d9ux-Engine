//! UNX-F11239 嫁接适配层边界声明（0x1f270 段）
//!
//! 锚点：UNX-F11239-J1「Varix 侧适配只经内核语义对齐」边界声明落账，适配
//! 手段白名单三项冻结，越界手段 0 容忍。
//!
//! 纯功能模型：嫁接适配层不是任意改写的自由通道，而是一份**先声明后放行**
//! 的边界契约。契约由六条边界条款组成（drafe 期可写、封板后不可动），封板
//! 时把三项白名单手段（动态段语义对齐 / 系统调用号对齐 / 错误码值域对齐）
//! 一并冻结；此后每一次适配动作都必须落在「已声明条款 × 已白名单手段 ×
//! 语义对齐标志」三重交集内，并写明对齐理由与责任属主。
//!
//! 规则面：手段号分三段——0..3 白名单三手段可放行；3..8 是已知的非白名单
//! 手段（如 ld.so 私有注入、ptrace 改写、内核符号直取），一律拒；≥8 为越界
//! 手段，记入越界账且 0 容忍。语义对齐标志为假等同越界：只要适配意图不是
//! 「语义对齐」而是「行为替换」，就不属于本边界可容纳的嫁接。
//!
//! 拒止面：阶段错（DRAFT 期适配、二次封板）、条款缺项/空文本/无主/重号、
//! 手段越界或未白名单、条款未声明、理由为空、同条款同手段重复、容量超限、
//! 冻结后任何改写。冻结要求三手段全覆盖且越界账为零——越界一旦发生，本次
//! 边界声明作废，必须重开新账，不提供「清除越界账」的接口。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f270;
/// 边界条款缺项、空文本或条款号越界
pub const AD_ECLAUSE: u32 = SEG_BASE + 0x01;
/// 手段不在三项白名单内（已知但未授权）
pub const AD_EMEAN: u32 = SEG_BASE + 0x02;
/// 越界手段 / 非语义对齐意图（0 容忍）
pub const AD_EOOB: u32 = SEG_BASE + 0x03;
/// 条款或适配无责任属主
pub const AD_EOWNER: u32 = SEG_BASE + 0x04;
/// 语义对齐理由为空
pub const AD_ENOTE: u32 = SEG_BASE + 0x05;
/// 同条款同手段重复登记
pub const AD_EDUP: u32 = SEG_BASE + 0x06;
/// 状态机不允许该动作
pub const AD_ESTATE: u32 = SEG_BASE + 0x07;
/// 封板/冻结后再次改写
pub const AD_EFROZEN: u32 = SEG_BASE + 0x08;
/// 适配引用了未声明的条款
pub const AD_EUNBOUND: u32 = SEG_BASE + 0x09;
/// 适配账容量超限
pub const AD_EFULL: u32 = SEG_BASE + 0x0a;

// ---------------------------------------------------------------- 域常量
/// 边界条款条数
pub const CLAUSES: usize = 6;
/// 白名单手段数（三项冻结）
pub const MEANS: usize = 3;
/// 已知手段号空间（0..MEANS_SPAN）；≥ 上界即越界手段
pub const MEANS_SPAN: usize = 8;
/// 适配动作账容量
pub const ATTEMPTS_CAP: usize = 24;
/// 越界容忍度：0
pub const OOB_TOLERANCE: u8 = 0;
/// 阶段：草拟（条款可写，禁止适配）
pub const STAGE_DRAFT: u8 = 0;
/// 阶段：已封板（可适配，条款只读）
pub const STAGE_DECLARED: u8 = 1;
/// 阶段：已冻结（只读，可回放）
pub const STAGE_FROZEN: u8 = 2;
/// 边界声明原文（落账主语）
pub const DECL_TEXT: &str = "Varix 侧适配只经内核语义对齐";
/// 三项白名单手段名
pub const MEAN_NAMES: [&str; MEANS] = ["动态段语义对齐", "系统调用号对齐", "错误码值域对齐"];
/// 六条边界条款原文
pub const CLAUSE_TEXTS: [&str; CLAUSES] = [
    "只消费内核已承诺的语义，不消费实现细节",
    "只对齐数据面形状，不替换控制流",
    "只做值域映射，不改写错误码含义",
    "不注入私有符号，不直取内核地址",
    "不绕过 VFS/MM 的公开入口",
    "越界即作废，重开新账不留后门",
];
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

// ---------------------------------------------------------------- 数据结构
/// 一条边界条款：条款号 / 原文 / 责任属主。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clause {
    pub id: u8,
    pub text: &'static str,
    pub owner: u16,
}

impl Clause {
    /// 条款自洽：有文本、有属主、条款号在域内。
    pub fn well_formed(&self) -> bool {
        !self.text.is_empty() && self.owner != 0 && (self.id as usize) < CLAUSES
    }
}

/// 一次适配动作：落哪条条款、用什么手段、谁负责、为什么算语义对齐。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Graft {
    pub clause: u8,
    pub mean: u8,
    pub owner: u16,
    pub note: &'static str,
    pub semantic_only: bool,
}

impl Graft {
    /// 构造一条语义对齐适配（测试与建场共用）。
    pub fn aligned(clause: u8, mean: u8, owner: u16, note: &'static str) -> Graft {
        Graft { clause, mean, owner, note, semantic_only: true }
    }

    /// 构造一条行为替换型适配（越界样本）。
    pub fn replacing(clause: u8, mean: u8, owner: u16) -> Graft {
        Graft { clause, mean, owner, note: "行为替换", semantic_only: false }
    }
}

/// 嫁接适配层边界账：条款表 + 手段白名单 + 适配流水 + 阶段机。
#[derive(Clone, Debug)]
pub struct AdapterBoundary {
    clauses: [Option<Clause>; CLAUSES],
    gates: Vec<Graft>,
    whitelist: [bool; MEANS],
    wl_sealed: bool,
    stage: u8,
    root: Option<u64>,
    /// 接受的适配动作数
    pub accepts: u64,
    /// 被拒的动作数（含声明期错误）
    pub rejects: u64,
    /// 越界手段命中数（0 容忍账）
    pub oob_denied: u64,
    /// 回放次数
    pub replays: u64,
}

impl AdapterBoundary {
    /// 空账：草拟阶段，白名单三项预置为真但未封板。
    pub fn new() -> Self {
        AdapterBoundary {
            clauses: [None; CLAUSES],
            gates: Vec::new(),
            whitelist: [true; MEANS],
            wl_sealed: false,
            stage: STAGE_DRAFT,
            root: None,
            accepts: 0,
            rejects: 0,
            oob_denied: 0,
            replays: 0,
        }
    }

    /// 当前阶段。
    pub fn stage(&self) -> u8 {
        self.stage
    }

    /// 已落条款数。
    pub fn clause_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CLAUSES {
            if self.clauses.get(i).and_then(|c| c.as_ref()).is_some() {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 取某条款。
    pub fn clause_at(&self, id: u8) -> Option<Clause> {
        if (id as usize) >= CLAUSES {
            return None;
        }
        self.clauses.get(id as usize).and_then(|c| *c)
    }

    /// 白名单是否已封板。
    pub fn whitelist_frozen(&self) -> bool {
        self.wl_sealed && self.whitelist.iter().all(|w| *w)
    }

    /// 某手段是否在白名单内。
    pub fn mean_whitelisted(&self, mean: u8) -> bool {
        (mean as usize) < MEANS && self.whitelist.get(mean as usize).copied().unwrap_or(false)
    }

    /// 已登记适配数。
    pub fn gate_count(&self) -> usize {
        self.gates.len()
    }

    /// 被三手段覆盖了几种。
    pub fn means_used(&self) -> usize {
        let mut n = 0usize;
        let mut m = 0usize;
        while m < MEANS {
            let mut hit = false;
            let mut i = 0usize;
            while i < self.gates.len() {
                if let Some(g) = self.gates.get(i) {
                    if g.mean as usize == m {
                        hit = true;
                    }
                }
                i = i.wrapping_add(1);
            }
            if hit {
                n = n.wrapping_add(1);
            }
            m = m.wrapping_add(1);
        }
        n
    }

    /// 拒止计数加一后返回错误码。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }
}

impl AdapterBoundary {
    /// 落一条边界条款。仅草拟期可写；条款号唯一、文本非空、属主非零。
    pub fn declare(&mut self, id: u8, text: &'static str, owner: u16) -> Result<u64, u32> {
        if self.stage == STAGE_FROZEN {
            return self.fail(AD_EFROZEN);
        }
        if self.stage != STAGE_DRAFT {
            return self.fail(AD_ESTATE);
        }
        if (id as usize) >= CLAUSES {
            return self.fail(AD_ECLAUSE);
        }
        if text.is_empty() || DECL_TEXT.is_empty() {
            return self.fail(AD_ECLAUSE);
        }
        if owner == 0 {
            return self.fail(AD_EOWNER);
        }
        if self.clause_at(id).is_some() {
            return self.fail(AD_EDUP);
        }
        let c = Clause { id, text, owner };
        if let Some(slot) = self.clauses.get_mut(id as usize) {
            *slot = Some(c);
        }
        Ok(self.fold())
    }

    /// 封板：六条条款齐备 + 三项白名单全真，白名单自此冻结。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.stage == STAGE_FROZEN {
            return self.fail(AD_EFROZEN);
        }
        if self.stage == STAGE_DECLARED {
            return self.fail(AD_ESTATE);
        }
        if self.clause_count() != CLAUSES {
            return self.fail(AD_ECLAUSE);
        }
        let mut i = 0usize;
        while i < MEANS {
            if !self.whitelist.get(i).copied().unwrap_or(false) {
                return self.fail(AD_EMEAN);
            }
            i = i.wrapping_add(1);
        }
        self.wl_sealed = true;
        self.stage = STAGE_DECLARED;
        Ok(self.fold())
    }

    /// 撤销一项白名单手段。草拟期允许（重新排版），封板后一律拒止。
    pub fn revoke_mean(&mut self, mean: u8) -> Result<u64, u32> {
        if self.wl_sealed {
            return self.fail(AD_EFROZEN);
        }
        if (mean as usize) >= MEANS {
            return self.fail(AD_EMEAN);
        }
        if let Some(slot) = self.whitelist.get_mut(mean as usize) {
            *slot = false;
        }
        Ok(self.fold())
    }

    /// 登记一次适配。优先级：阶段 → 手段越界 → 未白名单 → 非语义对齐 →
    /// 条款未声明 → 无主 → 理由空 → 重复 → 容量。
    pub fn graft(&mut self, g: Graft) -> Result<u64, u32> {
        if self.stage == STAGE_FROZEN {
            return self.fail(AD_EFROZEN);
        }
        if self.stage != STAGE_DECLARED {
            return self.fail(AD_ESTATE);
        }
        if (g.mean as usize) >= MEANS_SPAN {
            self.oob_denied = self.oob_denied.wrapping_add(1);
            return self.fail(AD_EOOB);
        }
        if !g.semantic_only {
            self.oob_denied = self.oob_denied.wrapping_add(1);
            return self.fail(AD_EOOB);
        }
        if !self.mean_whitelisted(g.mean) {
            self.oob_denied = self.oob_denied.wrapping_add(1);
            return self.fail(AD_EMEAN);
        }
        if self.clause_at(g.clause).is_none() {
            return self.fail(AD_EUNBOUND);
        }
        if g.owner == 0 {
            return self.fail(AD_EOWNER);
        }
        if g.note.is_empty() {
            return self.fail(AD_ENOTE);
        }
        let mut i = 0usize;
        while i < self.gates.len() {
            if let Some(x) = self.gates.get(i) {
                if x.clause == g.clause && x.mean == g.mean {
                    return self.fail(AD_EDUP);
                }
            }
            i = i.wrapping_add(1);
        }
        if self.gates.len() >= ATTEMPTS_CAP {
            return self.fail(AD_EFULL);
        }
        self.gates.push(g);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 逐条复核已存适配是否仍满足边界（回放用），返回违规条数。
    pub fn violation_scan(&self) -> usize {
        let mut bad = 0usize;
        let mut i = 0usize;
        while i < self.gates.len() {
            if let Some(g) = self.gates.get(i) {
                let ok = g.semantic_only
                    && (g.mean as usize) < MEANS
                    && self.mean_whitelisted(g.mean)
                    && self.clause_at(g.clause).is_some()
                    && g.owner != 0
                    && !g.note.is_empty();
                if !ok {
                    bad = bad.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        bad
    }
}

impl AdapterBoundary {
    /// FNV-1a 指纹：条款表 + 白名单位 + 适配流水 + 阶段与计数。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = (h ^ (self.stage as u64)).wrapping_mul(FNV_PRIME);
        h = (h ^ (self.wl_sealed as u64)).wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < CLAUSES {
            match self.clauses.get(i).and_then(|c| *c) {
                Some(c) => {
                    h = (h ^ (c.id as u64)).wrapping_mul(FNV_PRIME);
                    h = fnv_bytes(h, c.text.as_bytes());
                    h = (h ^ (c.owner as u64)).wrapping_mul(FNV_PRIME);
                }
                None => {
                    h = (h ^ 0x00ff_00ffu64).wrapping_mul(FNV_PRIME);
                }
            }
            i = i.wrapping_add(1);
        }
        let mut m = 0usize;
        while m < MEANS {
            let bit = self.whitelist.get(m).copied().unwrap_or(false) as u64;
            h = (h ^ bit).wrapping_mul(FNV_PRIME);
            m = m.wrapping_add(1);
        }
        let mut k = 0usize;
        while k < self.gates.len() {
            if let Some(g) = self.gates.get(k) {
                h = (h ^ (g.clause as u64)).wrapping_mul(FNV_PRIME);
                h = (h ^ (g.mean as u64)).wrapping_mul(FNV_PRIME);
                h = (h ^ (g.owner as u64)).wrapping_mul(FNV_PRIME);
                h = fnv_bytes(h, g.note.as_bytes());
                h = (h ^ (g.semantic_only as u64)).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        h = (h ^ self.gates.len() as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts).wrapping_mul(FNV_PRIME);
        h
    }

    /// 冻结：条款齐、三手段全覆盖、越界账为零，缺一即拒；成功后幂等。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.stage == STAGE_FROZEN {
            return match self.root {
                Some(v) => Ok(v),
                None => self.fail(AD_ESTATE),
            };
        }
        if self.stage != STAGE_DECLARED {
            return self.fail(AD_ESTATE);
        }
        if self.clause_count() != CLAUSES {
            return self.fail(AD_ECLAUSE);
        }
        if self.means_used() != MEANS {
            return self.fail(AD_EMEAN);
        }
        if self.oob_denied != OOB_TOLERANCE as u64 {
            return self.fail(AD_EOOB);
        }
        if self.violation_scan() != 0 {
            return self.fail(AD_EOOB);
        }
        self.stage = STAGE_FROZEN;
        let v = self.fold();
        self.root = Some(v);
        Ok(v)
    }

    /// 回放：逐条复核适配流水并把当前指纹与冻结值比对。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.stage != STAGE_FROZEN {
            return self.fail(AD_ESTATE);
        }
        if self.violation_scan() != 0 {
            return self.fail(AD_EOOB);
        }
        let v = self.fold();
        if Some(v) != self.root {
            return self.fail(AD_ESTATE);
        }
        self.replays = self.replays.wrapping_add(1);
        Ok(v)
    }

    /// 冻结值。
    pub fn root(&self) -> Option<u64> {
        self.root
    }
}

fn fnv_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = (h ^ (*b as u64)).wrapping_mul(FNV_PRIME);
        }
        i = i.wrapping_add(1);
    }
    h
}

/// 草拟期补齐六条条款但不封板（供封板失败路径复用）。
pub fn drafted() -> (AdapterBoundary, bool) {
    let mut b = AdapterBoundary::new();
    let mut i = 0u8;
    while (i as usize) < CLAUSES {
        let text = CLAUSE_TEXTS.get(i as usize).copied().unwrap_or("条款");
        if b.declare(i, text, 0x11u16.wrapping_add(i as u16)).is_err() {
            return (b, false);
        }
        i = i.wrapping_add(1);
    }
    (b, true)
}

/// 草拟 + 封板（可继续登记适配，未冻结）。
pub fn fresh_sealed() -> (AdapterBoundary, bool) {
    let (mut b, ok0) = drafted();
    if !ok0 {
        return (b, false);
    }
    let ok1 = b.seal().is_ok();
    (b, ok1)
}

/// 建场：六条款 + 封板 + 三手段各两次适配 + 冻结 + 回放。
pub fn build_boundary() -> (AdapterBoundary, bool) {
    let (mut b, ok0) = fresh_sealed();
    if !ok0 {
        return (b, false);
    }
    let mut m = 0u8;
    while (m as usize) < MEANS {
        let c1 = m;
        let c2 = ((m as usize).wrapping_add(3) % CLAUSES) as u8;
        let g1 = Graft::aligned(c1, m, 0x30u16.wrapping_add(m as u16), "语义对齐");
        let g2 = Graft::aligned(c2, m, 0x31u16.wrapping_add(m as u16), "对齐验证");
        if b.graft(g1).is_err() {
            return (b, false);
        }
        if b.graft(g2).is_err() {
            return (b, false);
        }
        m = m.wrapping_add(1);
    }
    let ok1 = b.freeze().is_ok();
    let ok2 = b.replay().is_ok();
    (b, ok1 && ok2)
}

/// 四类独立注入：越界手段 / 未白名单手段 / 未声明条款与空理由 / 冻结后改写。
pub fn inject_four() -> [bool; 4] {
    // ① 越界手段（≥MEANS_SPAN）：记越界账、不放行、连带冻结被拒
    let (mut a0, s0) = fresh_sealed();
    let base0 = a0.graft(Graft::aligned(0, 0, 7, "对齐")).is_ok()
        && a0.graft(Graft::aligned(1, 1, 7, "对齐")).is_ok()
        && a0.graft(Graft::aligned(2, 2, 7, "对齐")).is_ok();
    let oob = a0
        .graft(Graft::aligned(3, MEANS_SPAN as u8, 7, "越界"))
        .err()
        == Some(AD_EOOB);
    let r0 = s0
        && base0
        && oob
        && a0.oob_denied == 1
        && a0.gate_count() == 3
        && a0.freeze().err() == Some(AD_EOOB);

    // ② 已知非白名单手段（3..MEANS_SPAN）
    let (mut a1, s1) = fresh_sealed();
    let r1 = s1
        && a1.graft(Graft::aligned(0, 3, 7, "私有注入")).err() == Some(AD_EMEAN)
        && a1.oob_denied == 1
        && a1.accepts == 0;

    // ③ 未声明条款 / 无主 / 理由空
    let (mut a2, s2) = fresh_sealed();
    let mut g = Graft::aligned(0, 0, 7, "对齐");
    g.clause = CLAUSES as u8;
    let r2 = s2
        && a2.graft(g).err() == Some(AD_EUNBOUND)
        && a2.graft(Graft::aligned(0, 0, 0, "对齐")).err() == Some(AD_EOWNER)
        && a2.graft(Graft::aligned(0, 0, 7, "")).err() == Some(AD_ENOTE)
        && a2.gate_count() == 0;

    // ④ 冻结后改写：适配、加条款、二次封板、撤白名单全拒
    let (mut a3, s3) = build_boundary();
    let r3 = s3
        && a3.graft(Graft::aligned(0, 0, 7, "对齐")).err() == Some(AD_EFROZEN)
        && a3.declare(0, "追加", 7).err() == Some(AD_EFROZEN)
        && a3.seal().err() == Some(AD_EFROZEN)
        && a3.revoke_mean(0).err() == Some(AD_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11239 嫁接适配层边界声明。
pub fn run_unx11239_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11239");
    let (mut b, ok) = build_boundary();

    // AD1 边界声明落账
    let mut clauses_ok = true;
    let mut i = 0usize;
    while i < CLAUSES {
        match b.clause_at(i as u8) {
            Some(c) => {
                if !c.well_formed() {
                    clauses_ok = false;
                }
            }
            None => clauses_ok = false,
        }
        i = i.wrapping_add(1);
    }
    s.add_dyn(
        "AD1-边界声明落账",
        ok && b.clause_count() == CLAUSES && clauses_ok && !DECL_TEXT.is_empty(),
        &format!("clauses={} text_len={}", b.clause_count(), DECL_TEXT.len()),
    );

    // AD2 白名单三项冻结
    let frozen_ok = b.whitelist_frozen() && MEANS == 3 && b.mean_whitelisted(0) && b.mean_whitelisted(2);
    s.add(
        "AD2-白名单三项冻结",
        frozen_ok && b.revoke_mean(2).err() == Some(AD_EFROZEN) && b.mean_whitelisted(2),
        "封板后三项白名单只读，撤销请求被拒且白名单不变",
    );

    // AD3 越界零容忍
    let (mut p3, s3) = fresh_sealed();
    let base3 = p3.graft(Graft::aligned(0, 0, 3, "对齐")).is_ok()
        && p3.graft(Graft::aligned(1, 1, 3, "对齐")).is_ok()
        && p3.graft(Graft::aligned(2, 2, 3, "对齐")).is_ok();
    let oob_hit = p3.graft(Graft::aligned(3, MEANS_SPAN as u8, 3, "越界")).err() == Some(AD_EOOB);
    s.add_dyn(
        "AD3-越界零容忍",
        s3 && base3 && oob_hit && p3.oob_denied == 1 && p3.freeze().err() == Some(AD_EOOB),
        &format!("oob={} freeze=EOOB", p3.oob_denied),
    );

    // AD4 已知未白名单拒止
    let (mut p4, s4) = fresh_sealed();
    s.add(
        "AD4-已知未白名单拒止",
        s4 && p4.graft(Graft::aligned(0, 3, 3, "私有注入")).err() == Some(AD_EMEAN)
            && p4.gate_count() == 0,
        "3..MEANS_SPAN 的已知手段一律不授权",
    );

    // AD5 无主适配拒止
    let (mut p5, s5) = fresh_sealed();
    s.add(
        "AD5-无主适配拒止",
        s5 && p5.graft(Graft::aligned(0, 0, 0, "对齐")).err() == Some(AD_EOWNER),
        "属主为零的适配不得落账",
    );

    // AD6 理由空拒止
    let (mut p6, s6) = fresh_sealed();
    s.add(
        "AD6-理由空拒止",
        s6 && p6.graft(Graft::aligned(0, 0, 5, "")).err() == Some(AD_ENOTE),
        "语义对齐理由为空即拒",
    );

    // AD7 非语义对齐拒止
    let (mut p7, s7) = fresh_sealed();
    s.add(
        "AD7-非语义对齐拒止",
        s7 && p7.graft(Graft::replacing(0, 0, 5)).err() == Some(AD_EOOB)
            && p7.gate_count() == 0
            && p7.oob_denied == 1,
        "行为替换型嫁接视同越界",
    );

    // AD8 未声明条款拒止
    let (mut p8, s8) = fresh_sealed();
    let mut g8 = Graft::aligned(0, 0, 5, "对齐");
    g8.clause = CLAUSES as u8;
    s.add(
        "AD8-未声明条款拒止",
        s8 && p8.graft(g8).err() == Some(AD_EUNBOUND) && p8.graft(Graft::aligned(0, 0, 5, "对齐")).is_ok(),
        "适配只能挂在已声明条款上",
    );

    // AD9 重复手段拒止
    let (mut p9, s9) = fresh_sealed();
    let first = p9.graft(Graft::aligned(2, 1, 4, "对齐")).is_ok();
    s.add(
        "AD9-重复手段拒止",
        s9 && first && p9.graft(Graft::aligned(2, 1, 9, "再对齐")).err() == Some(AD_EDUP)
            && p9.accepts == 1,
        "同条款同手段只许登记一次",
    );

    // AD10 冻结幂等
    let r1 = b.freeze();
    let r2 = b.freeze();
    s.add_dyn(
        "AD10-冻结幂等",
        r1 == r2 && r1.is_ok() && r1.ok() == b.root(),
        &format!("root={:x?}", b.root()),
    );

    // AD11 冻结后改写全拒
    let g11 = b.graft(Graft::aligned(0, 0, 2, "对齐")).err() == Some(AD_EFROZEN);
    s.add(
        "AD11-冻结后改写全拒",
        g11 && b.declare(3, "追加", 2).err() == Some(AD_EFROZEN)
            && b.seal().err() == Some(AD_EFROZEN)
            && b.revoke_mean(1).err() == Some(AD_EFROZEN),
        "冻结账四类改写路径全部拒止",
    );

    // AD12 回放一致
    let rp = b.replay();
    s.add_dyn(
        "AD12-回放一致",
        rp.is_ok() && rp.ok() == b.root() && b.violation_scan() == 0 && b.replays >= 2,
        &format!("replays={} viol={}", b.replays, b.violation_scan()),
    );

    // AD13 注入四拒
    let hits = inject_four();
    let mut all = true;
    let mut k = 0usize;
    while k < hits.len() {
        if !hits.get(k).copied().unwrap_or(false) {
            all = false;
        }
        k = k.wrapping_add(1);
    }
    s.add_dyn(
        "AD13-注入四拒",
        all && b.oob_denied == 0 && b.gate_count() == MEANS * 2,
        &format!(
            "inject={}{}{}{} gates={} oob={}",
            hits[0] as u8, hits[1] as u8, hits[2] as u8, hits[3] as u8,
            b.gate_count(), b.oob_denied
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11239_boundary_flow() {
        let (mut b, ok) = build_boundary();
        assert!(ok);
        assert_eq!(b.stage(), STAGE_FROZEN);
        assert_eq!(b.clause_count(), CLAUSES);
        assert_eq!(b.gate_count(), MEANS * 2);
        assert!(b.whitelist_frozen());
        let root = b.root();
        assert_eq!(root, Some(b.fold()));
        assert_eq!(b.freeze().ok(), root);
        assert_eq!(b.freeze().ok(), root);
        assert_eq!(b.replay().ok(), root);
        assert_eq!(b.oob_denied, 0);
        assert_eq!(b.rejects, 0);
        assert_eq!(b.violation_scan(), 0);
    }

    #[test]
    fn f11239_gates_reject_count() {
        let mut b = AdapterBoundary::new();
        assert_eq!(b.graft(Graft::aligned(0, 0, 1, "n")), Err(AD_ESTATE));
        assert!(b.declare(0, "c0", 1).is_ok());
        assert_eq!(b.declare(0, "dup", 1), Err(AD_EDUP));
        assert_eq!(b.declare(CLAUSES as u8, "c6", 1), Err(AD_ECLAUSE));
        assert_eq!(b.declare(1, "", 1), Err(AD_ECLAUSE));
        assert_eq!(b.declare(1, "c1", 0), Err(AD_EOWNER));
        assert_eq!(b.seal(), Err(AD_ECLAUSE));
        let mut i = 1u8;
        while (i as usize) < CLAUSES {
            assert!(b.declare(i, "cx", 1).is_ok());
            i = i.wrapping_add(1);
        }
        assert!(b.seal().is_ok());
        assert_eq!(b.seal(), Err(AD_ESTATE));
        assert_eq!(b.graft(Graft::aligned(0, MEANS_SPAN as u8, 1, "n")), Err(AD_EOOB));
        assert_eq!(b.graft(Graft::aligned(0, 3, 1, "n")), Err(AD_EMEAN));
        assert_eq!(b.graft(Graft::replacing(0, 0, 1)), Err(AD_EOOB));
        let mut g = Graft::aligned(0, 0, 1, "n");
        g.clause = 9;
        assert_eq!(b.graft(g), Err(AD_EUNBOUND));
        assert_eq!(b.graft(Graft::aligned(0, 0, 0, "n")), Err(AD_EOWNER));
        assert_eq!(b.graft(Graft::aligned(0, 0, 1, "")), Err(AD_ENOTE));
        assert!(b.graft(Graft::aligned(0, 0, 1, "n")).is_ok());
        assert_eq!(b.graft(Graft::aligned(0, 0, 1, "n2")), Err(AD_EDUP));
        assert_eq!(b.freeze(), Err(AD_EMEAN));
        assert!(b.graft(Graft::aligned(1, 1, 1, "n")).is_ok());
        assert!(b.graft(Graft::aligned(2, 2, 1, "n")).is_ok());
        assert_eq!(b.freeze(), Err(AD_EOOB));
        assert_eq!(b.rejects, 16);
        assert_eq!(b.accepts, 3);
        assert_eq!(b.oob_denied, 3);
    }

    #[test]
    fn f11239_rules_precedence() {
        let (mut b, ok) = fresh_sealed();
        assert!(ok);
        let mut g = Graft::aligned(0, MEANS_SPAN as u8, 1, "");
        g.semantic_only = false;
        assert_eq!(b.graft(g), Err(AD_EOOB));
        let mut h = Graft::aligned(0, 3, 0, "");
        h.clause = 9;
        assert_eq!(b.graft(h), Err(AD_EMEAN));
        assert!(b.graft(Graft::aligned(4, 2, 3, "值域映射")).is_ok());
        assert!(b.graft(Graft::aligned(5, 2, 3, "错码映射")).is_err() == false);
        assert_eq!(b.violation_scan(), 0);
        assert_eq!(b.mean_whitelisted(0), true);
        assert_eq!(b.mean_whitelisted(3), false);
        assert_eq!(b.clause_at(4).map(|c| c.owner), Some(0x11 + 4));
    }

    #[test]
    fn f11239_injection_four() {
        let hits = inject_four();
        assert!(hits.iter().all(|x| *x));
    }

    #[test]
    fn f11239_checks_green() {
        let cs = run_unx11239_checks();
        assert!(cs.all_passed());
        assert_eq!(cs.len(), 13);
    }
}
