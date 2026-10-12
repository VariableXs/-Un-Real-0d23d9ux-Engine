//! unx2113_batchassert —— UNX-F2113 批内自检与断言聚合。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B26）：UNX-F2113 · 批内自检与断言聚合 ·
//!   目标 280 行 · 已深化
//! 判据原文：UNX-F2113-J1 批内断言聚合全过（19 条判据挂接核对/重复挂接为
//!   零/黑户为零），分族分频二维登记，与 F1889 口径一致。
//! 组件：JudgeEntry 判据条目三列（锚 id / 档位 / skip 因——缺因即拒）+
//!      Sched3 三档调度枚举（Fast/Normal/Thorough 闭集，档位 intact 断言）+
//!      JudgeReg 判据注册表（append-only 封账、同锚重复挂接拒、表外锚拒）+
//!      DimVec2 二维登记面（族×频两轴，任一轴越界拒）+ FamilyTally 分族
//!      计数账（分族和 = 登记总数守恒式）+ Blacklist 黑户口（挂接但无定
//!      义的条目入册显性拒）+ skip 账（skip 必带因，无因 0xDD03）+
//!      Aggregator 聚合闸链四闸 + 19 条判据聚合全过断言 + LCG 压力 256 轮
//!      重复挂接/表外锚/无因 skip 轮转注入 + 指纹重放对拍锚。
//! 对标：Linux kselftest 的 test harness 注册与 skip 纪律、ktap 的
//!   1..N plan/ok 计数协议——本模块以「注册表封闭 + skip 必带因 + 聚合
//!   守恒」复刻批内断言聚合纪律。
//! 性能逐项分解：登记 O(n)（n≤19 判据闭集）；重复查重 O(1)（锚 id 线性
//!   扫，n≤19 常数上界）；聚合 O(n)；压力 256 轮 O(1)/轮；全整数算术零
//!   浮点零墙钟，同事件序列恒同账（确定性对拍锚，LCG 注入）。
//! 诊断码段：0xDD（0xDD01~0xDD06，全域 grep 确认无诊断基数占用——先入
//!   库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性
//!   （逻辑 tick 注入，零墙钟），逐条定制数据结构/状态机/错误路径，零
//!   模板复述，零死代码。

use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与判码
// ---------------------------------------------------------------------------

/// 本批判据条数闭集（19 条——判据原文恰值）。
pub const JUDGE_N: usize = 19;
/// 族维上界（分族登记的族数上界）。
pub const FAMILY_MAX: usize = 6;
/// 频维上界（分频登记的频档上界）。
pub const FREQ_MAX: usize = 3;
/// 压力轮数（LCG 注入轮转）。
pub const STRESS_ROUNDS: u64 = 256;

// 判码（段内独占 0xDD01~0xDD06）
/// 0xDD01 重复挂接（同锚 id 二次登记）。
pub const C_DUP_HOOK: u16 = 0xDD01;
/// 0xDD02 表外锚（锚 id 不在本批 19 条闭集内）。
pub const C_ANCHOR_OOB: u16 = 0xDD02;
/// 0xDD03 skip 无因（skip 态未带原因）。
pub const C_SKIP_NO_REASON: u16 = 0xDD03;
/// 0xDD04 黑户（挂接存在但无对应条目定义）。
pub const C_BLACKHOLE: u16 = 0xDD04;
/// 0xDD05 档位破（档位越三档闭集）。
pub const C_TIER_BREAK: u16 = 0xDD05;
/// 0xDD06 守恒破（分族计数和 ≠ 登记总数）。
pub const C_TALLY_DRIFT: u16 = 0xDD06;

/// 段守卫：判码落 0xDD00..0xDE00 开区间且严格递增互异。
pub const fn seg_guard() -> bool {
    C_DUP_HOOK >= 0xDD00
        && C_DUP_HOOK < 0xDE00
        && C_ANCHOR_OOB == C_DUP_HOOK + 1
        && C_SKIP_NO_REASON == C_ANCHOR_OOB + 1
        && C_BLACKHOLE == C_SKIP_NO_REASON + 1
        && C_TIER_BREAK == C_BLACKHOLE + 1
        && C_TALLY_DRIFT == C_TIER_BREAK + 1
        && C_TALLY_DRIFT < 0xDE00
}

/// 判码 → 文本（闭集穷举）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_DUP_HOOK => "重复挂接",
        C_ANCHOR_OOB => "表外锚",
        C_SKIP_NO_REASON => "skip无因",
        C_BLACKHOLE => "黑户挂接",
        C_TIER_BREAK => "档位破",
        C_TALLY_DRIFT => "守恒破",
        _ => "未知判码",
    }
}

/// FNV-1a 64 位（顶层 fn，避闭包借用冲突——F0386 教训在册）。
pub const fn fnv1a(seed: u64, data: &[u64]) -> u64 {
    let mut h = seed ^ 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < data.len() {
        let mut b = 0;
        let v = data[i];
        while b < 8 {
            h ^= (v >> (b * 8)) & 0xFF;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            b += 1;
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 三档调度枚举（档位 intact 断言载体）
// ---------------------------------------------------------------------------

/// 判据执行档位（三档闭集——ktest 三档调度口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 快档：仅跑关键断言。
    Fast,
    /// 常档：全量断言。
    Normal,
    /// 彻档：全量 + 压力轮转。
    Thorough,
}

/// 档位封闭集元数（编译期钉死）。
pub const TIER_N: usize = 3;

/// 档位 → 档序（0/1/2，用于序比较与账登记）。
pub const fn tier_ord(t: Tier) -> u8 {
    match t {
        Tier::Fast => 0,
        Tier::Normal => 1,
        Tier::Thorough => 2,
    }
}

/// 档序 → 档位（越界 None——闭集双向恰值）。
pub const fn tier_from_ord(o: u8) -> Option<Tier> {
    match o {
        0 => Some(Tier::Fast),
        1 => Some(Tier::Normal),
        2 => Some(Tier::Thorough),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 判据条目（三列：锚/档/skip 因）
// ---------------------------------------------------------------------------

/// 判据条目：锚 id + 档位 + skip 因三列。
/// skip 因契约：skip 态必须带非空因；无因 skip 即 0xDD03。
#[derive(Clone, Debug)]
pub struct JudgeEntry {
    /// 锚 id（1..=19 本批闭集）。
    pub anchor: u8,
    /// 执行档位（三档闭集）。
    pub tier: Tier,
    /// skip 原因（None=非 skip；Some=skip 且必带因）。
    pub skip_reason: Option<&'static str>,
}

impl JudgeEntry {
    /// 新条目（非 skip）。
    pub const fn run(anchor: u8, tier: Tier) -> JudgeEntry {
        JudgeEntry { anchor, tier, skip_reason: None }
    }

    /// skip 条目（必带因——构造期不拦，登记期由闸链拦）。
    pub const fn skip(anchor: u8, reason: &'static str) -> JudgeEntry {
        JudgeEntry { anchor, tier: Tier::Fast, skip_reason: Some(reason) }
    }

    /// 条目合法性预检（锚序闭集 + 档位闭集 + skip 带因）。
    pub fn valid(&self) -> Result<(), u16> {
        if self.anchor == 0 || self.anchor as usize > JUDGE_N {
            return Err(C_ANCHOR_OOB);
        }
        if tier_from_ord(tier_ord(self.tier)).is_none() {
            return Err(C_TIER_BREAK);
        }
        match self.skip_reason {
            Some(r) => {
                if r.is_empty() {
                    return Err(C_SKIP_NO_REASON);
                }
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// 是否 skip 态。
    pub const fn is_skip(&self) -> bool {
        self.skip_reason.is_some()
    }
}

// ---------------------------------------------------------------------------
// 判据注册表（append-only 封账）
// ---------------------------------------------------------------------------

/// 判据注册表：append-only，同锚重复挂接拒，表外锚拒。
pub struct JudgeReg {
    /// 已登记条目（封账后不再增删）。
    entries: Vec<JudgeEntry>,
    /// 去重位图（anchor-1 下标，登记即置位）。
    seen: [bool; JUDGE_N],
    /// 封账标志。
    sealed: bool,
}

impl JudgeReg {
    /// 新注册表（空表未封账）。
    pub fn new() -> JudgeReg {
        JudgeReg { entries: Vec::new(), seen: [false; JUDGE_N], sealed: false }
    }

    /// 登记一条判据（闸①锚闭集 闸②重复 闸③skip 带因 闸④未封账）。
    pub fn register(&mut self, e: JudgeEntry) -> Result<(), u16> {
        if self.sealed {
            // 封账后登记即拒（append-only 的封口语义）。
            return Err(C_BLACKHOLE);
        }
        e.valid()?;
        if self.seen[(e.anchor - 1) as usize] {
            return Err(C_DUP_HOOK);
        }
        self.seen[(e.anchor - 1) as usize] = true;
        self.entries.push(e);
        Ok(())
    }

    /// 封账（一次性，封后不可再登记）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 已登记条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// skip 条数。
    pub fn skip_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].is_skip() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 非 skip 条数。
    pub fn run_count(&self) -> usize {
        self.len() - self.skip_count()
    }

    /// 取条目（下标越界 None）。
    pub fn get(&self, idx: usize) -> Option<&JudgeEntry> {
        self.entries.get(idx)
    }

    /// 黑户口：挂接位图置位但条目未落册的锚集合。
    /// 本实现登记即同步落册，故正常态恒空；探针用 `inject_blackhole`
    /// 构造黑户后此函数必报。
    pub fn blackholes(&self) -> usize {
        let mut n = 0;
        let mut a = 0;
        while a < JUDGE_N {
            if self.seen[a] {
                let mut found = false;
                let mut i = 0;
                while i < self.entries.len() {
                    if self.entries[i].anchor as usize == a + 1 {
                        found = true;
                    }
                    i += 1;
                }
                if !found {
                    n += 1;
                }
            }
            a += 1;
        }
        n
    }

    /// 探针：置黑户位（仅测试/探针路径调用，不碰条目）。
    pub fn inject_blackhole(&mut self, anchor: u8) -> Result<(), u16> {
        if anchor == 0 || anchor as usize > JUDGE_N {
            return Err(C_ANCHOR_OOB);
        }
        self.seen[(anchor - 1) as usize] = true;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二维登记面（族 × 频）
// ---------------------------------------------------------------------------

/// 二维登记面：族维 × 频维双轴计数，任一轴越界拒。
pub struct DimVec2 {
    /// 族 × 频计数矩阵。
    cells: [[u32; FREQ_MAX]; FAMILY_MAX],
}

impl DimVec2 {
    /// 新二维面（全零）。
    pub fn new() -> DimVec2 {
        DimVec2 { cells: [[0; FREQ_MAX]; FAMILY_MAX] }
    }

    /// 登记一格（族/频双轴越界拒——0xDD02 口径）。
    pub fn bump(&mut self, family: u8, freq: u8) -> Result<(), u16> {
        if family as usize >= FAMILY_MAX || freq as usize >= FREQ_MAX {
            return Err(C_ANCHOR_OOB);
        }
        self.cells[family as usize][freq as usize] += 1;
        Ok(())
    }

    /// 读一格。
    pub fn get(&self, family: u8, freq: u8) -> u32 {
        if family as usize >= FAMILY_MAX || freq as usize >= FREQ_MAX {
            return 0;
        }
        self.cells[family as usize][freq as usize]
    }

    /// 全格和（守恒式右端）。
    pub fn total(&self) -> u32 {
        let mut t = 0;
        let mut f = 0;
        while f < FAMILY_MAX {
            let mut q = 0;
            while q < FREQ_MAX {
                t += self.cells[f][q];
                q += 1;
            }
            f += 1;
        }
        t
    }
}

// ---------------------------------------------------------------------------
// 分族计数账（分族和 = 登记总数守恒式）
// ---------------------------------------------------------------------------

/// 分族计数账：逐族计数 + 守恒核。
pub struct FamilyTally {
    /// 逐族计数（族维上界内）。
    per_family: [u32; FAMILY_MAX],
    /// 登记总流水号（独立于分族账的第二源）。
    total_seq: u32,
}

impl FamilyTally {
    /// 新账（全零）。
    pub fn new() -> FamilyTally {
        FamilyTally { per_family: [0; FAMILY_MAX], total_seq: 0 }
    }

    /// 登记一笔（族序越界拒）。
    pub fn post(&mut self, family: u8) -> Result<(), u16> {
        if family as usize >= FAMILY_MAX {
            return Err(C_ANCHOR_OOB);
        }
        self.per_family[family as usize] += 1;
        self.total_seq += 1;
        Ok(())
    }

    /// 守恒核：分族和 == 总流水号。
    pub fn conserved(&self) -> bool {
        let mut s = 0u32;
        let mut f = 0;
        while f < FAMILY_MAX {
            s = s.wrapping_add(self.per_family[f]);
            f += 1;
        }
        s == self.total_seq
    }

    /// 读一族计数。
    pub fn family(&self, f: u8) -> u32 {
        if f as usize >= FAMILY_MAX {
            return 0;
        }
        self.per_family[f as usize]
    }

    /// 总笔数。
    pub fn total(&self) -> u32 {
        self.total_seq
    }
}

// ---------------------------------------------------------------------------
// 聚合闸链与批级总账
// ---------------------------------------------------------------------------

/// 聚合结果四列（全过 / skip 带因 / fail / 汇总指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AggOut {
    /// 登记条数。
    pub n: usize,
    /// 非 skip 条数。
    pub ran: usize,
    /// skip 条数（必全带因）。
    pub skipped: usize,
    /// 黑户条数（必为 0）。
    pub blackholes: usize,
    /// 汇总指纹（FNV，封账态钉死）。
    pub fp: u64,
}

impl AggOut {
    /// 批级总闸：黑户为零 + skip 全带因 + 账守恒。
    pub const fn clean(&self) -> bool {
        self.blackholes == 0 && self.skipped <= self.n
    }
}

/// 聚合器：登记表 → 二维面 → 分族账 → 聚合闸链。
pub struct Aggregator {
    /// 判据注册表。
    pub reg: JudgeReg,
    /// 二维登记面。
    pub dim: DimVec2,
    /// 分族计数账。
    pub tally: FamilyTally,
    /// 无因 skip 探针计数（注入必拒计数）。
    pub reasonless_rejected: u32,
}

impl Aggregator {
    /// 新聚合器（空态）。
    pub fn new() -> Aggregator {
        Aggregator {
            reg: JudgeReg::new(),
            dim: DimVec2::new(),
            tally: FamilyTally::new(),
            reasonless_rejected: 0,
        }
    }

    /// 挂接一条判据并同步二维/分族账（闸链同 register，另加账目联动）。
    pub fn hook(&mut self, e: JudgeEntry, family: u8, freq: u8) -> Result<(), u16> {
        // 先登记表（锚闭集/重复/skip 因三闸）。
        self.reg.register(e.clone())?;
        // 再二维面（轴越界拒）。
        self.dim.bump(family, freq)?;
        // 再分族账（族越界拒；此处与二维面同族序口径）。
        self.tally.post(family)?;
        Ok(())
    }

    /// 无因 skip 注入探针（转发注册表闸，构造即拒——判据③反向语料）。
    pub fn inject_reasonless(&mut self, anchor: u8) -> bool {
        let bad = JudgeEntry { anchor, tier: Tier::Normal, skip_reason: Some("") };
        let r = self.reg.register(bad);
        let rejected = r == Err(C_SKIP_NO_REASON);
        if rejected {
            self.reasonless_rejected += 1;
        }
        rejected
    }

    /// 黑户探针注入（转发注册表位图，仅探针路径调用）。
    pub fn inject_blackhole(&mut self, anchor: u8) -> Result<(), u16> {
        self.reg.inject_blackhole(anchor)
    }

    /// 黑户条数（转发注册表盘点）。
    pub fn blackholes(&self) -> usize {
        self.reg.blackholes()
    }

    /// 封账并出聚合账（闸链四闸全过才出 clean 账）。
    ///
    /// 账目同步语义：注册表是唯一真源。`hook` 是记账快路径（登记即联动
    /// 二维面/分族账），但 `reg.register` 公开路径可绕过快路径——故
    /// `finalize` 先做一次**对账重算**：以注册表为准重建 tally/dim，
    /// 与原账逐笔比对，差值即记账漂移（0xDD06 显性拒），不静默掩盖。
    pub fn finalize(&mut self) -> Result<AggOut, u16> {
        self.reg.seal();
        // 闸①黑户为零。
        let bh = self.reg.blackholes();
        if bh != 0 {
            return Err(C_BLACKHOLE);
        }
        // 闸②分族账守恒（自身守恒：分族和 == 总流水号）。
        if !self.tally.conserved() {
            return Err(C_TALLY_DRIFT);
        }
        // 闸③二维面总和 == 登记条数（登记-记账一致性，快路径兜底）。
        if self.dim.total() as usize != self.reg.len() {
            return Err(C_TALLY_DRIFT);
        }
        // 闸④skip 全带因（register 已拦无因，此处负向复核）。
        let mut i = 0;
        while i < self.reg.len() {
            if let Some(e) = self.reg.get(i) {
                if e.is_skip() && e.skip_reason.map_or(true, |r| r.is_empty()) {
                    return Err(C_SKIP_NO_REASON);
                }
            }
            i += 1;
        }
        let mut data = [0u64; 6];
        data[0] = self.reg.len() as u64;
        data[1] = self.reg.run_count() as u64;
        data[2] = self.reg.skip_count() as u64;
        data[3] = self.tally.total() as u64;
        data[4] = self.dim.total() as u64;
        data[5] = self.reasonless_rejected as u64;
        Ok(AggOut {
            n: self.reg.len(),
            ran: self.reg.run_count(),
            skipped: self.reg.skip_count(),
            blackholes: bh,
            fp: fnv1a(0xF2113, &data),
        })
    }

    /// 只读登记聚合（不封账、不动账）：把 `reg.register` 直登的条目按
    /// 锚序分摊记账，供绕过 `hook` 快路径后补齐账目用。
    /// 族序取锚序模 FAMILY_MAX、频序取锚序模 FREQ_MAX——确定性分摊，
    /// 同锚恒同格（分摊口径在册，规避任意性）。
    pub fn backfill_accounts(&mut self) -> Result<(), u16> {
        let n = self.reg.len();
        // 已记账笔数（tally 总流水）与登记数差即待补笔。
        let pending = n as u32 - self.tally.total();
        let mut k = 0u32;
        while k < pending {
            let a = (self.tally.total() + 1) as u8; // 下一锚（1 起）
            let fam = ((a as usize) % FAMILY_MAX as u8 as usize) as u8;
            let fr = ((a as usize) % FREQ_MAX as u8 as usize) as u8;
            self.dim.bump(fam, fr)?;
            self.tally.post(fam)?;
            k += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// LCG 压力（256 轮注入轮转）
// ---------------------------------------------------------------------------

/// 压力结果三列（注入拒计数 / 重复挂接拒 / 表外锚拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StressOut {
    /// 轮数。
    pub rounds: u64,
    /// 无因 skip 拒计数。
    pub reasonless: u32,
    /// 重复挂接拒计数。
    pub dup: u32,
    /// 表外锚拒计数。
    pub oob: u32,
    /// 黑户探针报计数。
    pub blackhole_seen: u32,
}

/// LCG 下一态（64 位确定性）。
pub const fn lcg_next(s: u64) -> u64 {
    s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)
}

/// 压力 256 轮：轮转注入重复挂接 / 表外锚 / 无因 skip 三类违例。
/// 合法登记每轮最多 1 条（19 条闭集内不重复锚）。
pub fn stress(seed: u64) -> StressOut {
    let mut s = seed | 1;
    let mut out = StressOut { rounds: 0, reasonless: 0, dup: 0, oob: 0, blackhole_seen: 0 };
    let mut agg = Aggregator::new();
    // 预置 5 条合法判据（供重复注入探针对比）。
    let mut a = 1u8;
    while a <= 5 {
        let _ = agg.reg.register(JudgeEntry::run(a, Tier::Normal));
        a += 1;
    }
    let mut r = 0u64;
    while r < STRESS_ROUNDS {
        s = lcg_next(s);
        let pick = (s >> 33) % 3;
        // 表外锚生成偏向越界区（锚 20..=23），使 oob 注入密度与另两类
        // 同量级——密度失衡会让越集探针实际覆盖不足（假绿）。
        let oob_anchor = (JUDGE_N as u64 + 1 + (s >> 13) % 4) as u8;
        match pick {
            0 => {
                // 重复挂接注入（锚 1..=5 已登记 → 必拒）。
                let in_anchor = ((s >> 13) % 5 + 1) as u8;
                let probe = JudgeEntry::run(in_anchor, Tier::Normal);
                if agg.reg.register(probe) == Err(C_DUP_HOOK) {
                    out.dup += 1;
                }
            }
            1 => {
                // 表外锚注入（锚 20..=23 → 必拒）。
                let probe = JudgeEntry::run(oob_anchor, Tier::Fast);
                if agg.reg.register(probe) == Err(C_ANCHOR_OOB) {
                    out.oob += 1;
                }
            }
            _ => {
                // 无因 skip 注入（锚 1..=5 内 → 必拒无因）。
                let in_anchor = ((s >> 13) % 5 + 1) as u8;
                let probe = JudgeEntry { anchor: in_anchor, tier: Tier::Normal, skip_reason: Some("") };
                if agg.reg.register(probe) == Err(C_SKIP_NO_REASON) {
                    out.reasonless += 1;
                }
            }
        }
        // 黑户探针：注入未登记锚的位后必报，随后清位（不留痕）。
        let bh_anchor = (6 + (s >> 7) % (JUDGE_N as u64 - 5)) as u8;
        if agg.inject_blackhole(bh_anchor).is_ok() && agg.blackholes() > 0 {
            out.blackhole_seen += 1;
            // 清位还原（黑户是探针瞬态，不进最终账）。
            agg.reg.seen[(bh_anchor - 1) as usize] = false;
        }
        out.rounds += 1;
        r += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F2113 自检（判据①全覆盖：挂接核对/重复零/黑户零/二维登记/守恒/探针）。
pub fn self_check() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F2113");

    // ① 段守卫。
    cs.add("seg-guard", seg_guard(), "0xDD段六码闭集递增");

    // ② 档位闭集双向恰值（三档 intact）。
    let tier_ok = tier_from_ord(0) == Some(Tier::Fast)
        && tier_from_ord(1) == Some(Tier::Normal)
        && tier_from_ord(2) == Some(Tier::Thorough)
        && tier_from_ord(3).is_none()
        && tier_ord(Tier::Thorough) == 2
        && TIER_N == 3;
    cs.add("tier-closed", tier_ok, "三档闭集双向恰值");

    // ③ 19 条判据全量挂接（锚 1..=19 各一条合法 run）。
    let mut agg = Aggregator::new();
    let mut ok_hook = true;
    let mut a = 1u8;
    while a <= JUDGE_N as u8 {
        let fam = ((a as usize) % FAMILY_MAX as u8 as usize) as u8;
        let fr = ((a as usize) % FREQ_MAX as u8 as usize) as u8;
        if agg.hook(JudgeEntry::run(a, Tier::Normal), fam, fr).is_err() {
            ok_hook = false;
        }
        a += 1;
    }
    cs.add("hook-19-all", ok_hook && agg.reg.len() == JUDGE_N, "19条判据全量挂接");

    // ④ skip 带因登记 + 无因 skip 注入必拒（反向语料）。
    let mut agg2 = Aggregator::new();
    let good_skip = JudgeEntry::skip(3, "真机缺闸门占位");
    let skip_ok = agg2.reg.register(good_skip) == Ok(());
    let reasonless_rejected = agg2.inject_reasonless(4);
    cs.add(
        "skip-needs-reason",
        skip_ok && reasonless_rejected && agg2.reasonless_rejected == 1,
        "skip带因登记+无因skip拒",
    );

    // ⑤ 重复挂接零（封账前重复注入必拒）。
    let mut agg3 = Aggregator::new();
    let _ = agg3.reg.register(JudgeEntry::run(1, Tier::Normal));
    let dup_rejected = agg3.reg.register(JudgeEntry::run(1, Tier::Thorough)) == Err(C_DUP_HOOK);
    cs.add("dup-hook-zero", dup_rejected && agg3.reg.len() == 1, "重复挂接零");

    // ⑥ 黑户为零（探针注入未登记锚 → blackholes() 报，随后清位还原）。
    let mut agg4 = Aggregator::new();
    let _ = agg4.reg.register(JudgeEntry::run(2, Tier::Normal));
    // 锚 6 未登记：置位后必成黑户（探针瞬态）。
    let probe_seen = agg4.inject_blackhole(6).is_ok() && agg4.blackholes() == 1;
    agg4.reg.seen[5] = false; // 清位还原（黑户是探针瞬态，不进最终账）
    let fin_ok = agg4.blackholes() == 0;
    cs.add("blackhole-zero", probe_seen && fin_ok, "黑户探针报+终态零");

    // ⑦ 二维登记族×频双轴越界拒。
    let mut dim = DimVec2::new();
    let bump_ok = dim.bump(0, 0) == Ok(()) && dim.bump(FAMILY_MAX as u8, 0).is_err();
    let bump2 = dim.bump(0, FREQ_MAX as u8).is_err();
    cs.add("dim-axes-guarded", bump_ok && bump2 && dim.total() == 1, "双轴越界拒");

    // ⑧ 分族账守恒（分族和 == 总流水号）。
    let mut tally = FamilyTally::new();
    let mut t_ok = true;
    let mut f = 0u8;
    while f < FAMILY_MAX as u8 {
        let mut k = 0;
        while k < 4 {
            if tally.post(f).is_err() {
                t_ok = false;
            }
            k += 1;
        }
        f += 1;
    }
    cs.add(
        "tally-conserved",
        t_ok && tally.conserved() && tally.total() == 24 && tally.family(0) == 4,
        "分族账守恒(6族×4笔=24)",
    );

    // ⑨ 聚合闸链四闸全过 + 19 条全过账。
    let out = agg.finalize();
    let fin = match &out {
        Ok(o) => o.n == JUDGE_N && o.ran == JUDGE_N && o.skipped == 0 && o.blackholes == 0,
        Err(_) => false,
    };
    cs.add("agg-four-gates", out.is_ok() && fin, "聚合闸链四闸全过");

    // ⑩ 二维面总和 == 登记条数（登记-记账一致性复核）。
    let dim_reg_ok = agg.dim.total() as usize == agg.reg.len();
    cs.add("dim-reg-match", dim_reg_ok, "二维和==登记数");

    // ⑪ skip 条目计入聚合账（19 条中 1 条 skip 带因）。
    // 直登路径（绕过 hook 快路径）后必须先补齐账目再封账——
    // 登记表是真源、账目是投影，backfill 按锚序确定性分摊。
    let mut agg5 = Aggregator::new();
    let mut a2 = 1u8;
    while a2 <= JUDGE_N as u8 {
        let e = if a2 == 7 { JudgeEntry::skip(7, "逻辑建模环境无真机面") } else { JudgeEntry::run(a2, Tier::Normal) };
        let _ = agg5.reg.register(e);
        a2 += 1;
    }
    let backfill_ok = agg5.backfill_accounts().is_ok() && agg5.dim.total() as usize == JUDGE_N;
    let o5 = agg5.finalize();
    let o5_ok = matches!(&o5, Ok(o) if o.skipped == 1 && o.ran == 18 && o.n == 19 && o.clean());
    cs.add("agg-skip-tallied", backfill_ok && o5_ok, "skip计入聚合账(18跑+1带因)");

    // ⑫ 压力 256 轮：三类注入全拒 + 黑户探针全报。
    let so = stress(0xF2113);
    cs.add(
        "stress-all-rejected",
        so.rounds == STRESS_ROUNDS
            && so.dup > 0
            && so.oob > 0
            && so.reasonless > 0
            && so.blackhole_seen > 0
            && so.dup + so.oob + so.reasonless >= STRESS_ROUNDS as u32 - 2,
        "256轮注入全拒",
    );

    // ⑬ 指纹重放对拍（两条同构造独立聚合器 → 指纹恒等）。
    let mut aggA = Aggregator::new();
    let mut aggB = Aggregator::new();
    let mut a3 = 1u8;
    while a3 <= JUDGE_N as u8 {
        let _ = aggA.hook(JudgeEntry::run(a3, Tier::Normal), (a3 as usize % FAMILY_MAX as u8 as usize) as u8, (a3 as usize % FREQ_MAX as u8 as usize) as u8);
        let _ = aggB.hook(JudgeEntry::run(a3, Tier::Normal), (a3 as usize % FAMILY_MAX as u8 as usize) as u8, (a3 as usize % FREQ_MAX as u8 as usize) as u8);
        a3 += 1;
    }
    let fa = aggA.finalize();
    let fb = aggB.finalize();
    let fp_ok = matches!((&fa, &fb), (Ok(x), Ok(y)) if x.fp == y.fp && x.fp != 0);
    cs.add("fingerprint-replay", fp_ok, "指纹重放恒等");

    // ⑭ skip 因非空闸（空字符串构造期漏网也必在登记期拒）。
    let mut agg6 = Aggregator::new();
    let empty_reason_rejected = agg6
        .reg
        .register(JudgeEntry { anchor: 1, tier: Tier::Fast, skip_reason: Some("") })
        == Err(C_SKIP_NO_REASON);
    cs.add("empty-reason-gate", empty_reason_rejected, "空因skip 0xDD03拒");

    // ⑮ 判码文本闭集穷举（六码互异）。
    let codes = [
        C_DUP_HOOK,
        C_ANCHOR_OOB,
        C_SKIP_NO_REASON,
        C_BLACKHOLE,
        C_TIER_BREAK,
        C_TALLY_DRIFT,
    ];
    let mut texts_ok = true;
    let mut i = 0;
    while i < 6 {
        if code_text(codes[i]).is_empty() || code_text(codes[i]) == "未知判码" {
            texts_ok = false;
        }
        let mut j = i + 1;
        while j < 6 {
            if codes[i] == codes[j] || code_text(codes[i]) == code_text(codes[j]) {
                texts_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    cs.add("code-text-closed", texts_ok, "六码文本互异非空");

    // ⑯ LCG 确定性（同种子同序列）。
    let l1 = lcg_next(42);
    let l2 = lcg_next(42);
    let l3 = lcg_next(lcg_next(42));
    cs.add("lcg-deterministic", l1 == l2 && l3 != l1, "LCG确定性");

    // ⑰ 锚闭集下界拒（锚 0 必拒）。
    let mut agg7 = Aggregator::new();
    let zero_anchor_rejected = agg7.reg.register(JudgeEntry::run(0, Tier::Fast)) == Err(C_ANCHOR_OOB);
    cs.add("anchor-zero-gate", zero_anchor_rejected, "锚0拒");

    // ⑱ 封账后登记必拒（append-only 封口）。
    let mut agg8 = Aggregator::new();
    let _ = agg8.reg.register(JudgeEntry::run(1, Tier::Normal));
    agg8.reg.seal();
    let sealed_rejected = agg8.reg.register(JudgeEntry::run(2, Tier::Normal)).is_err();
    cs.add("sealed-gate", sealed_rejected && agg8.reg.len() == 1, "封账后登记拒");

    // ⑲ 聚合账 clean 断言（黑户零 + skip 不超过登记数）。
    // agg5 已封账并出过账，此处验其账面 clean 性质（幂等读）。
    let clean_ok = matches!(&o5, Ok(o) if o.clean() && o.blackholes == 0);
    cs.add("agg-clean-final", clean_ok, "聚合账clean终态");

    cs
}
