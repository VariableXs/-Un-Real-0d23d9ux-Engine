//! UNX-F11240 B02 批 ktest 断言集聚合（0x1f280 段）
//!
//! 锚点：UNX-F11240-J1 本批 19 条判据聚合入 ktest 生态面，一次命令全跑，
//! 通过率 100% 才算绿。
//!
//! 纯功能模型：B02 批的 19 条判据在此收敛为一张断言集账。每条判据登记时
//! 声明自己覆盖的断言种类（状态/值域/时序/文本四类位图），跑批时真实记账
//! 落断言流水；`run_all` 一次遍历全账，要求「19/19 已登记、每条至少一条
//! 断言、全部绿」三条件同时成立才给绿，否则回传首个失败面的错误码。
//!
//! 规则面：判据号 0..19 唯一；名字非空、属主非零、种类位图落在四位掩码内。
//! 断言只许挂在已登记判据上，且种类必须是该判据声明过的位——这避免了
//! 「拿状态断言冒充时序断言」的假绿。通过率按万分比（basis point）计算，
//! 100% 即 10000 bp，缺一 bp 都不绿。
//!
//! 拒止面：判据越界/重号/空名/无主/位图野位、断言挂空判据或野种类、容量
//! 超限、未集齐即跑、有判据未跑、有红、未跑即冻结、冻结后改账。红判据不
//! 是「警告」——`reds` 计数非零时 `run_all` 恒返回 ERED，且计数只增不减。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f280;
/// 判据号越界 / 种类位图野位
pub const KT_ECRIT: u32 = SEG_BASE + 0x01;
/// 判据号重复登记
pub const KT_EDUP: u32 = SEG_BASE + 0x02;
/// 判据名为空
pub const KT_ENAME: u32 = SEG_BASE + 0x03;
/// 判据无责任属主
pub const KT_EOWNER: u32 = SEG_BASE + 0x04;
/// 判据未集齐 19 条即跑批
pub const KT_ESHORT: u32 = SEG_BASE + 0x05;
/// 有判据一条断言都没跑
pub const KT_ESKIP: u32 = SEG_BASE + 0x06;
/// 存在红断言，整批不绿
pub const KT_ERED: u32 = SEG_BASE + 0x07;
/// 状态机不允许该动作
pub const KT_ESTATE: u32 = SEG_BASE + 0x08;
/// 冻结后改账
pub const KT_EFROZEN: u32 = SEG_BASE + 0x09;
/// 断言流水容量超限
pub const KT_EFULL: u32 = SEG_BASE + 0x0a;

// ---------------------------------------------------------------- 域常量
/// 本批判据条数
pub const CRITERIA: usize = 19;
/// 断言种类数：状态 / 值域 / 时序 / 文本
pub const KINDS: usize = 4;
/// 种类全掩码
pub const KIND_MASK: u8 = 0x0f;
/// 通过率满分（万分比）
pub const PASS_BP: u32 = 10_000;
/// 断言流水容量
pub const RUN_CAP: usize = 96;
/// 每条判据的必跑断言条数（建场用）
pub const RUNS_PER_CRIT: usize = 3;
/// 阶段：收集
pub const STAGE_COLLECT: u8 = 0;
/// 阶段：已跑批（有聚合值）
pub const STAGE_EVALUATED: u8 = 1;
/// 阶段：冻结
pub const STAGE_FROZEN: u8 = 2;
/// 判据名表（19 条 B02 批口径）
pub const CRIT_NAMES: [&str; CRITERIA] = [
    "B02-J01-适配边界声明存在", "B02-J02-白名单三项冻结", "B02-J03-越界手段零容忍",
    "B02-J04-微差四列齐全", "B02-J05-微差危害级合法", "B02-J06-处置白名单",
    "B02-J07-微差零静默", "B02-J08-条目号唯一", "B02-J09-账本可冻结",
    "B02-J10-冻结幂等", "B02-J11-装载序单调", "B02-J12-依赖图无环",
    "B02-J13-搜索序六级", "B02-J14-缓存命中等价", "B02-J15-错误文本逐字",
    "B02-J16-退出码一致", "B02-J17-诊断开关矩阵", "B02-J18-聚合一次跑全",
    "B02-J19-通过率百分百",
];
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

// ---------------------------------------------------------------- 数据结构
/// 一条判据登记：编号 / 名 / 属主 / 断言种类位图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Criterion {
    pub id: u8,
    pub name: &'static str,
    pub owner: u16,
    pub kind_mask: u8,
}

impl Criterion {
    /// 判据自洽：名非空、属主非零、位图落在四位掩码且非零。
    pub fn well_formed(&self) -> bool {
        !self.name.is_empty()
            && self.owner != 0
            && self.kind_mask != 0
            && (self.kind_mask & !KIND_MASK) == 0
    }

    /// 是否声明覆盖某断言种类。
    pub fn covers(&self, kind: u8) -> bool {
        (kind as usize) < KINDS && (self.kind_mask & (1u8 << kind)) != 0
    }
}

/// 一条断言流水：挂哪条判据、哪类断言、绿红、失败错误码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Assertion {
    pub crit: u8,
    pub kind: u8,
    pub ok: bool,
    pub code: u32,
}

/// B02 批判据聚合账。
#[derive(Clone, Debug)]
pub struct KtestBatch {
    crits: [Option<Criterion>; CRITERIA],
    runs: Vec<Assertion>,
    stage: u8,
    root: Option<u64>,
    /// 接受的断言流水条数
    pub accepts: u64,
    /// 被拒的 API 动作数
    pub rejects: u64,
    /// 红断言条数（只增不减）
    pub reds: u64,
    /// 回放次数
    pub replays: u64,
}

impl KtestBatch {
    /// 空批。
    pub fn new() -> Self {
        KtestBatch {
            crits: [None; CRITERIA],
            runs: Vec::new(),
            stage: STAGE_COLLECT,
            root: None,
            accepts: 0,
            rejects: 0,
            reds: 0,
            replays: 0,
        }
    }

    /// 当前阶段。
    pub fn stage(&self) -> u8 {
        self.stage
    }

    /// 已登记判据数。
    pub fn registered(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CRITERIA {
            if self.crits.get(i).and_then(|c| *c).is_some() {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 取判据。
    pub fn crit_at(&self, id: u8) -> Option<Criterion> {
        if (id as usize) >= CRITERIA {
            return None;
        }
        self.crits.get(id as usize).and_then(|c| *c)
    }

    /// 某判据已跑断言条数。
    pub fn runs_of(&self, id: u8) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.runs.len() {
            if let Some(a) = self.runs.get(i) {
                if a.crit == id {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 某判据已覆盖的断言种类位图。
    pub fn kinds_covered(&self, id: u8) -> u8 {
        let mut m = 0u8;
        let mut i = 0usize;
        while i < self.runs.len() {
            if let Some(a) = self.runs.get(i) {
                if a.crit == id && (a.kind as usize) < KINDS {
                    m |= 1u8 << a.kind;
                }
            }
            i = i.wrapping_add(1);
        }
        m
    }

    /// 某判据是否已跑且全绿。
    pub fn crit_green(&self, id: u8) -> bool {
        let mut seen = false;
        let mut ok = true;
        let mut i = 0usize;
        while i < self.runs.len() {
            if let Some(a) = self.runs.get(i) {
                if a.crit == id {
                    seen = true;
                    if !a.ok {
                        ok = false;
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        seen && ok
    }

    /// 绿判据条数。
    pub fn green_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0u8;
        while (i as usize) < CRITERIA {
            if self.crit_green(i) {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 通过率（万分比）。
    pub fn pass_bp(&self) -> u32 {
        let g = self.green_count() as u32;
        g.wrapping_mul(PASS_BP) / (CRITERIA as u32)
    }

    /// 首个红判据号。
    pub fn first_red(&self) -> Option<u8> {
        let mut i = 0u8;
        while (i as usize) < CRITERIA {
            if !self.crit_green(i) && self.runs_of(i) != 0 {
                return Some(i);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }
}

impl KtestBatch {
    /// 登记一条判据。优先级：阶段 → 号域 → 名 → 属主 → 位图 → 重号。
    pub fn register(&mut self, id: u8, name: &'static str, owner: u16, kind_mask: u8) -> Result<u64, u32> {
        if self.stage == STAGE_FROZEN {
            return self.fail(KT_EFROZEN);
        }
        if self.stage != STAGE_COLLECT {
            return self.fail(KT_ESTATE);
        }
        if (id as usize) >= CRITERIA {
            return self.fail(KT_ECRIT);
        }
        if name.is_empty() {
            return self.fail(KT_ENAME);
        }
        if owner == 0 {
            return self.fail(KT_EOWNER);
        }
        if kind_mask == 0 || (kind_mask & !KIND_MASK) != 0 {
            return self.fail(KT_ECRIT);
        }
        if self.crit_at(id).is_some() {
            return self.fail(KT_EDUP);
        }
        let c = Criterion { id, name, owner, kind_mask };
        if let Some(slot) = self.crits.get_mut(id as usize) {
            *slot = Some(c);
        }
        Ok(self.fold())
    }

    /// 落一条断言流水。判据必须已登记，种类必须是该判据声明过的位。
    pub fn record(&mut self, crit: u8, kind: u8, ok: bool, code: u32) -> Result<u64, u32> {
        if self.stage == STAGE_FROZEN {
            return self.fail(KT_EFROZEN);
        }
        if self.stage != STAGE_COLLECT {
            return self.fail(KT_ESTATE);
        }
        let c = match self.crit_at(crit) {
            Some(c) => c,
            None => return self.fail(KT_ECRIT),
        };
        if (kind as usize) >= KINDS || !c.covers(kind) {
            return self.fail(KT_ECRIT);
        }
        if self.runs.len() >= RUN_CAP {
            return self.fail(KT_EFULL);
        }
        self.runs.push(Assertion { crit, kind, ok, code });
        self.accepts = self.accepts.wrapping_add(1);
        if !ok {
            self.reds = self.reds.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 一次命令全跑：19 齐、每条有断言、零红，缺一即拒。
    pub fn run_all(&mut self) -> Result<u64, u32> {
        if self.stage == STAGE_FROZEN {
            return self.fail(KT_EFROZEN);
        }
        if self.stage != STAGE_COLLECT {
            return self.fail(KT_ESTATE);
        }
        if self.registered() != CRITERIA {
            return self.fail(KT_ESHORT);
        }
        let mut i = 0u8;
        while (i as usize) < CRITERIA {
            if self.runs_of(i) == 0 {
                return self.fail(KT_ESKIP);
            }
            i = i.wrapping_add(1);
        }
        if self.reds != 0 || self.green_count() != CRITERIA {
            return self.fail(KT_ERED);
        }
        let v = self.fold();
        self.root = Some(v);
        self.stage = STAGE_EVALUATED;
        Ok(v)
    }

    /// 冻结：只有跑过批的账才可冻结，幂等。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.stage == STAGE_FROZEN {
            return match self.root {
                Some(v) => Ok(v),
                None => self.fail(KT_ESTATE),
            };
        }
        if self.stage != STAGE_EVALUATED {
            return self.fail(KT_ESTATE);
        }
        self.stage = STAGE_FROZEN;
        match self.root {
            Some(v) => Ok(v),
            None => self.fail(KT_ESTATE),
        }
    }

    /// 回放：重算聚合指纹并与冻结值比对。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.stage != STAGE_FROZEN {
            return self.fail(KT_ESTATE);
        }
        if self.reds != 0 {
            return self.fail(KT_ERED);
        }
        let v = self.fold();
        if Some(v) != self.root {
            return self.fail(KT_ESTATE);
        }
        self.replays = self.replays.wrapping_add(1);
        Ok(v)
    }

    /// 聚合指纹：判据表 + 断言流水 + 计数（不含阶段）。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < CRITERIA {
            match self.crits.get(i).and_then(|c| *c) {
                Some(c) => {
                    h = (h ^ (c.id as u64)).wrapping_mul(FNV_PRIME);
                    h = fnv_bytes(h, c.name.as_bytes());
                    h = (h ^ (c.owner as u64)).wrapping_mul(FNV_PRIME);
                    h = (h ^ (c.kind_mask as u64)).wrapping_mul(FNV_PRIME);
                }
                None => {
                    h = (h ^ 0x0000_00ffu64).wrapping_mul(FNV_PRIME);
                }
            }
            i = i.wrapping_add(1);
        }
        let mut k = 0usize;
        while k < self.runs.len() {
            if let Some(a) = self.runs.get(k) {
                h = (h ^ (a.crit as u64)).wrapping_mul(FNV_PRIME);
                h = (h ^ (a.kind as u64)).wrapping_mul(FNV_PRIME);
                h = (h ^ (a.ok as u64)).wrapping_mul(FNV_PRIME);
                h = (h ^ (a.code as u64)).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        h = (h ^ self.runs.len() as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts).wrapping_mul(FNV_PRIME);
        h = (h ^ self.reds).wrapping_mul(FNV_PRIME);
        h
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

/// 收集齐 19 条判据但一条断言都不跑。
pub fn full_collect() -> (KtestBatch, bool) {
    let mut b = KtestBatch::new();
    let mut i = 0u8;
    while (i as usize) < CRITERIA {
        let name = CRIT_NAMES.get(i as usize).copied().unwrap_or("B02");
        let owner = 0x40u16.wrapping_add(i as u16);
        if b.register(i, name, owner, KIND_MASK).is_err() {
            return (b, false);
        }
        i = i.wrapping_add(1);
    }
    (b, true)
}

/// 给除 `skip` 外的每条判据各跑 RUNS_PER_CRIT 条绿断言。
pub fn fill_green(b: &mut KtestBatch, skip: Option<u8>, red_at: Option<u8>) -> bool {
    let mut i = 0u8;
    while (i as usize) < CRITERIA {
        let mut skipped = false;
        if let Some(s) = skip {
            if s == i {
                skipped = true;
            }
        }
        if !skipped {
            let mut k = 0u8;
            while (k as usize) < RUNS_PER_CRIT {
                let mut ok = true;
                if let Some(r) = red_at {
                    if r == i && k == 0 {
                        ok = false;
                    }
                }
                let code = if ok { 0 } else { KT_ERED };
                if b.record(i, k, ok, code).is_err() {
                    return false;
                }
                k = k.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    true
}

/// 建场：19 条判据 × 3 条绿断言 → 跑批 → 冻结 → 回放。
pub fn build_batch() -> (KtestBatch, bool) {
    let (mut b, ok0) = full_collect();
    if !ok0 {
        return (b, false);
    }
    if !fill_green(&mut b, None, None) {
        return (b, false);
    }
    let ok1 = b.run_all().is_ok();
    let ok2 = b.freeze().is_ok();
    let ok3 = b.replay().is_ok();
    (b, ok1 && ok2 && ok3)
}

/// 四类独立注入：未集齐即跑 / 有判据未跑 / 有红 / 冻结后改账。
pub fn inject_four() -> [bool; 4] {
    // ① 只登记 5 条就开跑：ESHORT
    let mut a0 = KtestBatch::new();
    let mut i = 0u8;
    while i < 5 {
        let _ = a0.register(i, "短批", 1, KIND_MASK);
        i = i.wrapping_add(1);
    }
    let r0 = a0.run_all().err() == Some(KT_ESHORT) && a0.registered() == 5;

    // ② 19 条齐但第 7 条没跑：ESKIP
    let (mut a1, s1) = full_collect();
    let filled = fill_green(&mut a1, Some(7), None);
    let r1 = s1
        && filled
        && a1.runs_of(7) == 0
        && a1.run_all().err() == Some(KT_ESKIP);

    // ③ 第 11 条一条红：ERED，通过率不足 100%
    let (mut a2, s2) = full_collect();
    let filled2 = fill_green(&mut a2, None, Some(11));
    let r2 = s2
        && filled2
        && a2.reds == 1
        && a2.run_all().err() == Some(KT_ERED)
        && a2.pass_bp() < PASS_BP;

    // ④ 冻结后加断言 / 加判据 / 重跑
    let (mut a3, s3) = build_batch();
    let r3 = s3
        && a3.record(0, 0, true, 0).err() == Some(KT_EFROZEN)
        && a3.register(0, "追加", 1, KIND_MASK).err() == Some(KT_EFROZEN)
        && a3.run_all().err() == Some(KT_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11240 B02 批 ktest 断言集聚合。
pub fn run_unx11240_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11240");
    let (mut b, ok) = build_batch();

    // KT1 判据集齐
    let mut wf = true;
    let mut i = 0u8;
    while (i as usize) < CRITERIA {
        match b.crit_at(i) {
            Some(c) => {
                if !c.well_formed() {
                    wf = false;
                }
            }
            None => wf = false,
        }
        i = i.wrapping_add(1);
    }
    s.add_dyn(
        "KT1-判据集齐",
        ok && wf && b.registered() == CRITERIA,
        &format!("reg={}/{}", b.registered(), CRITERIA),
    );

    // KT2 一次命令全跑
    let mut all_ran = true;
    let mut i = 0u8;
    while (i as usize) < CRITERIA {
        if b.runs_of(i) < RUNS_PER_CRIT {
            all_ran = false;
        }
        i = i.wrapping_add(1);
    }
    s.add(
        "KT2-一次命令全跑",
        all_ran && b.accepts == (CRITERIA * RUNS_PER_CRIT) as u64,
        "19 条判据各自至少跑满三条断言，一次跑批覆盖全账",
    );

    // KT3 通过率百分百
    s.add_dyn(
        "KT3-通过率百分百",
        b.pass_bp() == PASS_BP && b.green_count() == CRITERIA && b.reds == 0,
        &format!("bp={} green={} red={}", b.pass_bp(), b.green_count(), b.reds),
    );

    // KT4 红判据拒绿
    let (mut p4, s4) = full_collect();
    let f4 = fill_green(&mut p4, None, Some(3));
    s.add_dyn(
        "KT4-红判据拒绿",
        s4 && f4 && p4.run_all().err() == Some(KT_ERED) && p4.first_red() == Some(3),
        &format!("red={} bp={}", p4.reds, p4.pass_bp()),
    );

    // KT5 未跑判据拒绿
    let (mut p5, s5) = full_collect();
    let f5 = fill_green(&mut p5, Some(17), None);
    s.add_dyn(
        "KT5-未跑判据拒绿",
        s5 && f5 && p5.runs_of(17) == 0 && p5.run_all().err() == Some(KT_ESKIP),
        &format!("crit17_runs={}", p5.runs_of(17)),
    );

    // KT6 未集齐拒跑
    let mut p6 = KtestBatch::new();
    let mut k = 0u8;
    while k < 6 {
        let _ = p6.register(k, "半批", 2, KIND_MASK);
        k = k.wrapping_add(1);
    }
    s.add(
        "KT6-未集齐拒跑",
        p6.run_all().err() == Some(KT_ESHORT) && p6.registered() == 6,
        "19 条未集齐即跑批一律拒",
    );

    // KT7 判据号越界
    let mut p7 = KtestBatch::new();
    s.add(
        "KT7-判据号越界",
        p7.register(CRITERIA as u8, "越界", 1, KIND_MASK).err() == Some(KT_ECRIT)
            && p7.record(0, 0, true, 0).err() == Some(KT_ECRIT),
        "判据号域外与挂空判据同罪",
    );

    // KT8 重号拒止
    let mut p8 = KtestBatch::new();
    let a = p8.register(0, "首登", 1, KIND_MASK).is_ok();
    s.add(
        "KT8-重号拒止",
        a && p8.register(0, "再登", 2, KIND_MASK).err() == Some(KT_EDUP) && p8.registered() == 1,
        "同一判据号只许登记一次",
    );

    // KT9 空名无主拒止
    let mut p9 = KtestBatch::new();
    s.add(
        "KT9-空名无主拒止",
        p9.register(0, "", 1, KIND_MASK).err() == Some(KT_ENAME)
            && p9.register(0, "有名", 0, KIND_MASK).err() == Some(KT_EOWNER),
        "判据必须有名有主",
    );

    // KT10 野位图野种类拒止
    let mut p10 = KtestBatch::new();
    let ok10 = p10.register(0, "窄判据", 1, 0b0011).is_ok();
    s.add(
        "KT10-野位图野种类拒止",
        ok10
            && p10.register(1, "野位", 1, 0x1f).err() == Some(KT_ECRIT)
            && p10.record(0, 2, true, 0).err() == Some(KT_ECRIT)
            && p10.record(0, 9, true, 0).err() == Some(KT_ECRIT)
            && p10.record(0, 1, true, 0).is_ok(),
        "断言种类必须落在判据声明位图内",
    );

    // KT11 冻结幂等
    let f1 = b.freeze();
    let f2 = b.freeze();
    s.add_dyn(
        "KT11-冻结幂等",
        f1 == f2 && f1.ok() == b.root() && f1.is_ok(),
        &format!("root={:x?} replays={}", b.root(), b.replays),
    );

    // KT12 冻结后改账拒止
    s.add(
        "KT12-冻结后改账拒止",
        b.record(0, 0, true, 0).err() == Some(KT_EFROZEN)
            && b.register(2, "追加", 1, KIND_MASK).err() == Some(KT_EFROZEN)
            && b.run_all().err() == Some(KT_EFROZEN),
        "冻结账三类改写入径全拒",
    );

    // KT13 注入四拒
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
        "KT13-注入四拒",
        all && b.rejects == 3 && b.accepts == (CRITERIA * RUNS_PER_CRIT) as u64,
        &format!(
            "inject={}{}{}{} acc={} rej={}",
            hits[0] as u8, hits[1] as u8, hits[2] as u8, hits[3] as u8,
            b.accepts, b.rejects
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11240_batch_flow() {
        let (mut b, ok) = build_batch();
        assert!(ok);
        assert_eq!(b.registered(), CRITERIA);
        assert_eq!(b.stage(), STAGE_FROZEN);
        assert_eq!(b.accepts, (CRITERIA * RUNS_PER_CRIT) as u64);
        assert_eq!(b.reds, 0);
        assert_eq!(b.green_count(), CRITERIA);
        assert_eq!(b.pass_bp(), PASS_BP);
        assert_eq!(b.first_red(), None);
        let root = b.root();
        assert_eq!(root, Some(b.fold()));
        assert_eq!(b.freeze().ok(), root);
        assert_eq!(b.replay().ok(), root);
        assert_eq!(b.rejects, 0);
    }

    #[test]
    fn f11240_gates_reject_count() {
        let mut b = KtestBatch::new();
        assert_eq!(b.record(0, 0, true, 0), Err(KT_ECRIT));
        assert_eq!(b.register(CRITERIA as u8, "越界", 1, KIND_MASK), Err(KT_ECRIT));
        assert_eq!(b.register(0, "", 1, KIND_MASK), Err(KT_ENAME));
        assert_eq!(b.register(0, "x", 0, KIND_MASK), Err(KT_EOWNER));
        assert_eq!(b.register(0, "x", 1, 0), Err(KT_ECRIT));
        assert_eq!(b.register(0, "x", 1, 0x1f), Err(KT_ECRIT));
        let mut i = 0u8;
        while (i as usize) < CRITERIA {
            assert!(b.register(i, "c", 1, KIND_MASK).is_ok());
            i = i.wrapping_add(1);
        }
        assert_eq!(b.register(0, "dup", 1, KIND_MASK), Err(KT_EDUP));
        assert_eq!(b.record(0, 4, true, 0), Err(KT_ECRIT));
        assert_eq!(b.record(CRITERIA as u8, 0, true, 0), Err(KT_ECRIT));
        assert_eq!(b.freeze(), Err(KT_ESTATE));
        assert_eq!(b.run_all(), Err(KT_ESKIP));
        let mut n = 0usize;
        while n < RUN_CAP {
            assert!(b.record((n % CRITERIA) as u8, 0, true, 0).is_ok());
            n = n.wrapping_add(1);
        }
        assert_eq!(b.record(0, 0, true, 0), Err(KT_EFULL));
        assert!(b.run_all().is_ok());
        assert_eq!(b.record(0, 0, true, 0), Err(KT_ESTATE));
        assert_eq!(b.register(1, "x", 1, KIND_MASK), Err(KT_ESTATE));
        assert_eq!(b.run_all(), Err(KT_ESTATE));
        assert!(b.freeze().is_ok());
        assert_eq!(b.record(0, 0, true, 0), Err(KT_EFROZEN));
        assert_eq!(b.run_all(), Err(KT_EFROZEN));
        assert_eq!(b.rejects, 17);
        assert_eq!(b.accepts, RUN_CAP as u64);
    }

    #[test]
    fn f11240_rules_rates() {
        let (mut b, ok) = full_collect();
        assert!(ok);
        assert_eq!(b.register(0, "", 1, 0x1f), Err(KT_ENAME));
        assert!(fill_green(&mut b, None, None));
        assert_eq!(b.pass_bp(), PASS_BP);
        assert_eq!(b.kinds_covered(0), 0b0111);
        let (mut c, _) = full_collect();
        assert!(fill_green(&mut c, None, Some(0)));
        assert_eq!(c.green_count(), CRITERIA - 1);
        assert_eq!(c.pass_bp(), 9473);
        assert_eq!(c.first_red(), Some(0));
        assert_eq!(c.run_all(), Err(KT_ERED));
        let crit = c.crit_at(0).expect("crit0");
        assert!(crit.covers(0) && crit.covers(3) && !crit.covers(4));
    }

    #[test]
    fn f11240_injection_four() {
        let hits = inject_four();
        assert!(hits.iter().all(|x| *x));
    }

    #[test]
    fn f11240_checks_green() {
        let cs = run_unx11240_checks();
        assert!(cs.all_passed());
        assert_eq!(cs.len(), 13);
    }
}
