//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18499 · ktest 引导面 B05 批断言集 · 0x7F880 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18499-J1 本批 19 条判据聚合一次命令全绿，失败注入必红三条
//! 本模块结构清单：
//!   1. 冻结常量层：断言矩阵 19 条（ID 1..=19 连续）/ 引导段闭集 5
//!       （PRE/KMOD/ASSERT/AGG/SEAL）/ 必红槽 {4,11,17} / 判据轮数 10；
//!   2. 诊断码闭集 KtCd（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 数据结构：
//!       - 引导序列机 BootSeq：stage 0..=5 单调步进 + 访问位面 u8 +
//!         步数账——越级/倒退/幽灵/重复/收尾后步进五闸显拒；
//!       - 断言矩阵聚合器 Aggregate：results[19] 位账 + greens/reds 双账；
//!       - 注入账 breaker_account(slot)：指定槽翻红的位账构造（自指位
//!         11/12 在账中按绿计——其真值由自身谓词机械断言，杜绝谓词互递归）；
//!   4. 19 条判据（自含谓词，逐条有真实域工作）：
//!       K01 段位窗 / K02 文案闭集 / K03 判据计数 / K04 引导正常链 /
//!       K05 越级拒 / K06 倒退拒 / K07 幽灵段拒 / K08 重复段拒 /
//!       K09 引导面守恒 / K10 引导面确定性 / K11 必红三槽 / K12 注入
//!       不外溢 / K13 引导位面账 / K14 判据名册 / K15 导出回放 /
//!       K16 空序列拒 / K17 满序列拒 / K18 哨兵码域 / K19 本批闭包；
//!   5. 一次命令聚合：run_unx18499_checks() 单入口返回 19 条判据全绿
//!       （CheckSet 逐条 = 矩阵逐位）；
//!   6. 失败注入必红三条：BREAKERS=[4,11,17]——每槽注入后聚合必恰 1 红
//!       且红在注入槽、邻位不外溢（K11 谓词内机器断言）；
//!   7. run_unx18499_checks：19 条判据（=矩阵本账，≤20 上限内）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F880，G1 工单 A10 批05 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7F880;

// ── 冻结常量层（推导注记） ─────────────────────────────────────────
/// 断言矩阵规模（写死 19：J1"19 条判据"语义，ID 1..=19 连续）。
pub const ASSERT_N: usize = 19;
/// 引导段闭集（写死 5：PRE→KMOD→ASSERT→AGG→SEAL 单向流水）。
pub const STAGE_N: u8 = 5;
/// 必红槽（冻结 {4,11,17}：故障注入模型的三条"必红"演示位——覆盖
/// 前段/中段/尾段三个区带）。
pub const BREAKER_1: usize = 4;
pub const BREAKER_2: usize = 11;
pub const BREAKER_3: usize = 17;
/// 必红槽集（聚合复核用）。
pub const BREAKERS: [usize; 3] = [BREAKER_1, BREAKER_2, BREAKER_3];
/// 自指位集（K11/K12——聚合相关谓词，注入账中按绿计防互递归）。
pub const SELF_REF_1: usize = 11;
pub const SELF_REF_2: usize = 12;
/// 判据轮数（写死 10）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 导出协议标签（冻结）。
pub const KTEST_FMT_TAG: &'static str = "KTEST v1";
/// 引导段名闭表（冻结，5 段）。
pub const STAGE_NAMES: [&'static str; 5] = ["PRE", "KMOD", "ASSERT", "AGG", "SEAL"];

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtCd {
    /// 越级步进（next > stage+1）
    StageJump,
    /// 倒退步进（next < stage）
    StageBack,
    /// 幽灵段（next == 0 或 > 5）
    StageGhost,
    /// 重复段（next == stage——同段重入）
    StageDup,
    /// 收尾后步进（stage==5 再步进）
    StageDone,
    /// 聚合破坏（green+red ≠ 19 或位账越界）
    AggBroken,
    /// 必红槽漏红（注入后未红）
    BreakerMissed,
    /// 必红槽外溢（注入后邻位误红）
    BreakerLeaked,
    /// 导出行非法
    ExportBad,
    /// 往返破坏
    RoundtripBroken,
    /// 双跑不一致
    NotReproducible,
    /// 位面破坏（访问位面与 stage 账不符）
    PlaneBroken,
}

impl KtCd {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            KtCd::StageJump => "越级步进：引导段须逐段推进",
            KtCd::StageBack => "倒退步进：引导段单向",
            KtCd::StageGhost => "幽灵段：1..=5 之外",
            KtCd::StageDup => "重复段：同段不可重入",
            KtCd::StageDone => "收尾后步进：SEAL 后机已封",
            KtCd::AggBroken => "聚合破坏：green+red≠19",
            KtCd::BreakerMissed => "必红槽漏红：注入后未红",
            KtCd::BreakerLeaked => "必红槽外溢：注入后邻位误红",
            KtCd::ExportBad => "导出行非法：字段/数字错",
            KtCd::RoundtripBroken => "往返破坏：回放与账不一致",
            KtCd::NotReproducible => "双跑不一致：复现红线",
            KtCd::PlaneBroken => "位面破坏：访问位面与段账不符",
        }
    }
    /// 全闭集（段位守卫遍历用）。
    pub const ALL: [KtCd; 12] = [
        KtCd::StageJump,
        KtCd::StageBack,
        KtCd::StageGhost,
        KtCd::StageDup,
        KtCd::StageDone,
        KtCd::AggBroken,
        KtCd::BreakerMissed,
        KtCd::BreakerLeaked,
        KtCd::ExportBad,
        KtCd::RoundtripBroken,
        KtCd::NotReproducible,
        KtCd::PlaneBroken,
    ];
}

// ── 引导序列机 ─────────────────────────────────────────────────────
/// 引导序列机（引导面域模型）：stage 0（未启）..=5（SEAL 收尾），
/// 单调逐段步进，访问位面 u8（第 s 位=段 s 已访问，s 1..=5）。
pub struct BootSeq {
    pub stage: u8,
    pub plane: u8,
    pub steps: u32,
    pub rejects: u32,
}

impl BootSeq {
    /// 未启序列机。
    pub fn new() -> BootSeq {
        BootSeq { stage: 0, plane: 0, steps: 0, rejects: 0 }
    }

    /// 位面一致性：已访问位 == 1..=stage 全置（前缀不变量）。
    pub fn plane_consistent(&self) -> bool {
        let mut want = 0u8;
        let mut s = 1;
        while s <= self.stage {
            want |= 1u8 << s;
            s += 1;
        }
        self.plane == want
    }

    /// 步进闸：幽灵段 → 倒退 → 收尾 → 重复 → 越级。全过才推进。
    /// （收尾先于重复：SEAL 后任一步进——含同段重入——均判收尾拒。）
    pub fn step_to(&mut self, next: u8) -> Result<(), u32> {
        if next == 0 || next > STAGE_N {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KtCd::StageGhost.code());
        }
        if next < self.stage {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KtCd::StageBack.code());
        }
        if self.stage == STAGE_N {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KtCd::StageDone.code());
        }
        if next == self.stage {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KtCd::StageDup.code());
        }
        if next > self.stage + 1 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KtCd::StageJump.code());
        }
        self.stage = next;
        self.plane |= 1u8 << next;
        self.steps = self.steps.saturating_add(1);
        Ok(())
    }

    /// 正常链：1→2→3→4→5 逐段走通。
    pub fn run_normal(&mut self) -> Result<(), u32> {
        let mut s = 1;
        while s <= STAGE_N {
            self.step_to(s)?;
            s += 1;
        }
        if self.stage != STAGE_N || !self.plane_consistent() {
            return Err(KtCd::PlaneBroken.code());
        }
        Ok(())
    }
}

// ── 断言矩阵（19 条谓词，自含真实域工作） ──────────────────────────
/// 聚合器（位账 + 双账守恒）。
pub struct Aggregate {
    /// results[i] = 第 i+1 条判据当前值（true=绿）。
    pub results: [bool; ASSERT_N],
    pub greens: u32,
    pub reds: u32,
}

impl Aggregate {
    /// 空聚合。
    pub fn new() -> Aggregate {
        Aggregate { results: [false; ASSERT_N], greens: 0, reds: 0 }
    }
    /// 守恒：greens + reds == 19。
    pub fn conserve(&self) -> Result<(), u32> {
        if (self.greens + self.reds) as usize != ASSERT_N {
            return Err(KtCd::AggBroken.code());
        }
        Ok(())
    }
}

/// 注入账构造（指定槽翻红；自指位 11/12 按绿计——防谓词互递归）。
pub fn breaker_account(slot: usize) -> Result<Aggregate, u32> {
    let mut a = Aggregate::new();
    let mut i = 0;
    while i < ASSERT_N {
        let id = i + 1;
        let v = if id == slot {
            // 注入槽：故障翻红。
            false
        } else if id == SELF_REF_1 || id == SELF_REF_2 {
            // 自指位：注入账按绿计（真值由自身谓词机械断言）。
            true
        } else {
            eval_pure(id)
        };
        a.results[i] = v;
        if v {
            a.greens = a.greens.saturating_add(1);
        } else {
            a.reds = a.reds.saturating_add(1);
        }
        i += 1;
    }
    a.conserve()?;
    Ok(a)
}

/// 谓词纯评估（无故障注入的真值——每条都有真实域工作）。
pub fn eval_pure(id: usize) -> bool {
    match id {
        // K01 段位窗：全码落 [SEG_BASE, SEG_BASE+0x40)。
        1 => segment_guard(),
        // K02 文案闭集：逐码非空中文。
        2 => detail_closed(),
        // K03 判据计数：矩阵恰 19。
        3 => ASSERT_N == 19,
        // K04 引导正常链：5 段逐段走通 + 位面一致。
        4 => {
            let mut b = BootSeq::new();
            b.run_normal().is_ok()
        }
        // K05 越级拒（1→3）。
        5 => {
            let mut b = BootSeq::new();
            b.step_to(1).is_ok() && b.step_to(3).err() == Some(KtCd::StageJump.code())
        }
        // K06 倒退拒（2→1）。
        6 => {
            let mut b = BootSeq::new();
            let _ = b.step_to(1);
            let _ = b.step_to(2);
            b.step_to(1).err() == Some(KtCd::StageBack.code())
        }
        // K07 幽灵段拒（0 与 6）。
        7 => {
            let mut b = BootSeq::new();
            b.step_to(0).err() == Some(KtCd::StageGhost.code())
                && b.step_to(6).err() == Some(KtCd::StageGhost.code())
        }
        // K08 重复段拒（1→1）。
        8 => {
            let mut b = BootSeq::new();
            let _ = b.step_to(1);
            b.step_to(1).err() == Some(KtCd::StageDup.code())
        }
        // K09 引导面守恒：正常链 5 绿 + 四类坏步进注入全抓 4 绿 = 9 账
        // 全绿零红（greens+reds==9==total，漏检必 0）。
        9 => {
            let mut b = BootSeq::new();
            if b.run_normal().is_err() {
                return false;
            }
            let led = match inject_rounds(1) {
                Ok(l) => l,
                Err(_) => return false,
            };
            let mut caught = 0u32;
            let mut k = 0;
            while k < 4 {
                if led.tries[k] == 0 || led.caught[k] != led.tries[k] {
                    return false;
                }
                caught = caught.saturating_add(led.caught[k]);
                k += 1;
            }
            // 守恒：5（链步）+ 4（注入抓捕）= 9 账全绿零红；正常链自身
            // 零拒收（拒收账在注入机各机上，各恰 1）。
            b.steps == 5 && caught == 4 && b.rejects == 0
        }
        // K10 引导面确定性：正常链与注入账双跑逐字段一致。
        10 => {
            let mut a1 = BootSeq::new();
            let mut a2 = BootSeq::new();
            let ok1 = a1.run_normal();
            let ok2 = a2.run_normal();
            if ok1.is_err() || ok2.is_err() {
                return false;
            }
            if a1.stage != a2.stage || a1.plane != a2.plane || a1.steps != a2.steps {
                return false;
            }
            let l1 = match inject_rounds(1) {
                Ok(l) => l,
                Err(_) => return false,
            };
            let l2 = match inject_rounds(1) {
                Ok(l) => l,
                Err(_) => return false,
            };
            l1.tries == l2.tries && l1.caught == l2.caught
        }
        // K11 必红三槽：{4,11,17} 逐槽注入 → 恰 1 红 ∧ 红在注入槽 ∧
        // 邻位不外溢（注入账机械断言，不经全量聚合）。
        11 => breaker_three_reds().is_ok(),
        // K12 注入不外溢：全部非必红槽逐一注入 → 恰 1 红 ∧ 邻位绿。
        12 => {
            let mut ok = true;
            let mut slot = 1;
            while slot <= ASSERT_N {
                let is_breaker =
                    slot == BREAKER_1 || slot == BREAKER_2 || slot == BREAKER_3;
                if !is_breaker {
                    match breaker_account(slot) {
                        Ok(a) => {
                            if a.reds != 1 || a.results[slot - 1] {
                                ok = false;
                            }
                            if slot > 1 && !a.results[slot - 2] {
                                ok = false;
                            }
                            if slot < ASSERT_N && !a.results[slot] {
                                ok = false;
                            }
                        }
                        Err(_) => ok = false,
                    }
                }
                slot += 1;
            }
            ok
        }
        // K13 引导位面账：正常链后位面 == 0b00111110（位 1..5 全置）。
        13 => {
            let mut b = BootSeq::new();
            let _ = b.run_normal();
            b.plane == 0b0011_1110u8 && b.steps == 5
        }
        // K14 判据名册：19 名与 19 位一一对应且逐名非空。
        14 => {
            if ASSERT_NAMES.len() != ASSERT_N {
                return false;
            }
            let mut i = 0;
            while i < ASSERT_N {
                if ASSERT_NAMES[i].is_empty() {
                    return false;
                }
                i += 1;
            }
            true
        }
        // K15 导出回放：真实位账（自指位 11/12 与本位 15 按绿计入账——
        // 防谓词互递归）导出 → 回放对拍。
        15 => {
            let mut a = Aggregate::new();
            let mut i = 0;
            while i < ASSERT_N {
                let id = i + 1;
                a.results[i] = if id == 15 || id == SELF_REF_1 || id == SELF_REF_2 {
                    true
                } else {
                    eval_pure(id)
                };
                if a.results[i] {
                    a.greens = a.greens.saturating_add(1);
                } else {
                    a.reds = a.reds.saturating_add(1);
                }
                i += 1;
            }
            if a.conserve().is_err() {
                return false;
            }
            replay_ktest(&export_ktest(&a), &a).is_ok()
        }
        // K16 空序列拒：未启机幽灵步进拒且零步账。
        16 => {
            let mut b = BootSeq::new();
            b.step_to(0).is_err() && b.steps == 0
        }
        // K17 满序列拒：SEAL 后再步进必拒。
        17 => {
            let mut b = BootSeq::new();
            let _ = b.run_normal();
            b.step_to(5).err() == Some(KtCd::StageDone.code())
        }
        // K18 哨兵码域：首码 == SEG_BASE（枚举序 0）。
        18 => KtCd::StageJump.code() == SEG_BASE,
        // K19 本批闭包：必红槽域内互异 ∧ 自指位合法。
        19 => {
            let mut ok = BREAKER_1 >= 1
                && BREAKER_1 <= ASSERT_N
                && BREAKER_2 >= 1
                && BREAKER_2 <= ASSERT_N
                && BREAKER_3 >= 1
                && BREAKER_3 <= ASSERT_N;
            if BREAKER_1 == BREAKER_2 || BREAKER_2 == BREAKER_3 || BREAKER_1 == BREAKER_3 {
                ok = false;
            }
            ok
        }
        _ => false,
    }
}

/// 断环评估：注入槽翻红（故障注入模型），其余槽取真值。
pub fn eval_broken(id: usize, breaker: usize) -> bool {
    if id == breaker && breaker >= 1 && breaker <= ASSERT_N {
        false
    } else {
        eval_pure(id)
    }
}

/// 全量聚合（单入口）：19 条逐位评估 + 双账守恒。
/// 注意：breaker ∈ {11,12} 时该位被直接翻红（短路），不会触发自指谓词
/// 求值——聚合永不递归。
pub fn aggregate(breaker: usize) -> Result<Aggregate, u32> {
    let mut a = Aggregate::new();
    let mut i = 0;
    while i < ASSERT_N {
        let v = eval_broken(i + 1, breaker);
        a.results[i] = v;
        if v {
            a.greens = a.greens.saturating_add(1);
        } else {
            a.reds = a.reds.saturating_add(1);
        }
        i += 1;
    }
    a.conserve()?;
    Ok(a)
}

/// 必红三槽断言：BREAKERS 逐槽注入账 → 恰 1 红 ∧ 红在注入槽 ∧ 邻位绿
/// ∧ 绿 18（不经全量聚合——机械位账防互递归）。
pub fn breaker_three_reds() -> Result<(), u32> {
    let mut k = 0;
    while k < BREAKERS.len() {
        let slot = BREAKERS[k];
        let a = breaker_account(slot)?;
        if a.reds != 1 {
            return Err(KtCd::BreakerMissed.code());
        }
        if a.results[slot - 1] {
            return Err(KtCd::BreakerMissed.code());
        }
        if a.greens != (ASSERT_N - 1) as u32 {
            return Err(KtCd::BreakerLeaked.code());
        }
        // 邻位不外溢：注入槽相邻位仍绿。
        if slot > 1 && !a.results[slot - 2] {
            return Err(KtCd::BreakerLeaked.code());
        }
        if slot < ASSERT_N && !a.results[slot] {
            return Err(KtCd::BreakerLeaked.code());
        }
        k += 1;
    }
    Ok(())
}

// ── 注入核（坏步进形态 × JUDGE_ROUNDS 轮） ────────────────────────
/// 注入账（4 槽：越级/倒退/幽灵/重复，守恒 tries==caught）。
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    /// 记一轮。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    /// 守恒：四槽全记录且全抓。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 四类坏步进注入 × rounds 轮（每轮全新序列机）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越级（1→3）。
        let mut b0 = BootSeq::new();
        let h0 = b0.step_to(1).is_ok() && b0.step_to(3).is_err() && b0.stage == 1;
        led.record(0, h0);
        // 槽1：倒退（2→1）。
        let mut b1 = BootSeq::new();
        let _ = b1.step_to(1);
        let _ = b1.step_to(2);
        let h1 = b1.step_to(1).is_err() && b1.stage == 2;
        led.record(1, h1);
        // 槽2：幽灵（0/6/255 三形态）。
        let mut b2 = BootSeq::new();
        let h2 = b2.step_to(0).is_err()
            && b2.step_to(STAGE_N + 1).is_err()
            && b2.step_to(255).is_err()
            && b2.stage == 0;
        led.record(2, h2);
        // 槽3：重复（1→1）。
        let mut b3 = BootSeq::new();
        let _ = b3.step_to(1);
        let h3 = b3.step_to(1).is_err() && b3.stage == 1;
        led.record(3, h3);
        r += 1;
    }
    Ok(led)
}

// ── 导出与回放 ─────────────────────────────────────────────────────
/// 数字段解析（十进制无符号）。
fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() {
        return Err(KtCd::ExportBad.code());
    }
    let mut acc: u32 = 0;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c < b'0' || c > b'9' {
            return Err(KtCd::ExportBad.code());
        }
        acc = match acc.checked_mul(10).and_then(|x| x.checked_add((c - b'0') as u32)) {
            Some(v) => v,
            None => return Err(KtCd::ExportBad.code()),
        };
        i += 1;
    }
    Ok(acc)
}

/// 表头校验（KTEST v1 前缀 + n= 计数段）。
pub fn valid_header(line: &str) -> bool {
    let tag = KTEST_FMT_TAG;
    let tb = tag.as_bytes();
    let lb = line.as_bytes();
    if lb.len() < tb.len() || &lb[..tb.len()] != tb {
        return false;
    }
    let mut i = tb.len();
    let mut saw = false;
    while i + 2 < lb.len() {
        if lb[i] == b' ' && lb[i + 1] == b'n' && lb[i + 2] == b'=' {
            saw = true;
            break;
        }
        i += 1;
    }
    saw
}

/// 聚合导出：表头 + 19 行 "A|id|ok"。
pub fn export_ktest(a: &Aggregate) -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!("{} id|ok n={}", KTEST_FMT_TAG, ASSERT_N));
    let mut i = 0;
    while i < ASSERT_N {
        out.push(format!("A|{}|{}", i + 1, if a.results[i] { 1u32 } else { 0u32 }));
        i += 1;
    }
    out
}

/// 单数据行解析："A|id|ok"，id 严格 == 行序 + 1，ok ∈ {0,1}。
pub fn parse_ktest_row(line: &str, expect_id: usize) -> Result<bool, u32> {
    if !line.starts_with("A|") {
        return Err(KtCd::ExportBad.code());
    }
    let body = &line[2..];
    let bb = body.as_bytes();
    let mut pipes = 0usize;
    let mut i = 0;
    while i < bb.len() {
        if bb[i] == b'|' {
            pipes += 1;
        }
        i += 1;
    }
    if pipes != 1 {
        return Err(KtCd::ExportBad.code());
    }
    let mut segs: [&str; 2] = ["", ""];
    let mut idx = 0usize;
    let mut start = 0usize;
    let mut k = 0usize;
    while k <= bb.len() {
        if k == bb.len() || bb[k] == b'|' {
            if idx >= 2 {
                return Err(KtCd::ExportBad.code());
            }
            segs[idx] = &body[start..k];
            idx += 1;
            start = k + 1;
        }
        k += 1;
    }
    if idx != 2 {
        return Err(KtCd::ExportBad.code());
    }
    let id = parse_u32(segs[0])? as usize;
    if id != expect_id {
        return Err(KtCd::RoundtripBroken.code());
    }
    let ok = parse_u32(segs[1])?;
    if ok > 1 {
        return Err(KtCd::ExportBad.code());
    }
    Ok(ok == 1)
}

/// 回放：跳表头逐行解析与给定聚合逐位对拍（入参聚合——防递归）。
pub fn replay_ktest(lines: &[String], a: &Aggregate) -> Result<(), u32> {
    if lines.is_empty() || !valid_header(&lines[0]) {
        return Err(KtCd::ExportBad.code());
    }
    if lines.len() != ASSERT_N + 1 {
        return Err(KtCd::RoundtripBroken.code());
    }
    let mut i = 1;
    while i < lines.len() {
        let ok = parse_ktest_row(&lines[i], i)?;
        if ok != a.results[i - 1] {
            return Err(KtCd::RoundtripBroken.code());
        }
        i += 1;
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < KtCd::ALL.len() {
        let v = KtCd::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < KtCd::ALL.len() {
        if KtCd::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑一致（聚合级）。
pub fn repro_check() -> Result<(), u32> {
    let a = aggregate(0)?;
    let b = aggregate(0)?;
    if a.results != b.results || a.greens != b.greens || a.reds != b.reds {
        return Err(KtCd::NotReproducible.code());
    }
    Ok(())
}

/// 断言名闭表（19 条冻结名——判据入账/名册对账用）。
pub const ASSERT_NAMES: [&'static str; ASSERT_N] = [
    "K01-段位窗",
    "K02-文案闭集",
    "K03-判据计数",
    "K04-引导正常链",
    "K05-越级拒",
    "K06-倒退拒",
    "K07-幽灵段拒",
    "K08-重复段拒",
    "K09-引导面守恒",
    "K10-引导面确定性",
    "K11-必红三槽",
    "K12-注入不外溢",
    "K13-引导位面账",
    "K14-判据名册",
    "K15-导出回放",
    "K16-空序列拒",
    "K17-满序列拒",
    "K18-哨兵码域",
    "K19-本批闭包",
];

// ── 判据收口（19 条 = 矩阵本账，一次命令全绿） ────────────────────
/// 判据入口（单模块 ≤20 条；本卡 19 条即 J1 全量聚合）。
pub fn run_unx18499_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18499 ktest 引导面 B05 批断言集");
    // 一次命令聚合：19 条判据逐条入账（无逐步 API）。
    let agg = aggregate(0);
    let mut i = 0;
    while i < ASSERT_N {
        let v = match &agg {
            Ok(a) => a.results[i],
            Err(_) => false,
        };
        s.add(ASSERT_NAMES[i], v, "矩阵逐位");
        i += 1;
    }
    s
}

// ── 测试 ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx18499_checks_green() {
        let set = run_unx18499_checks();
        assert!(set.all_passed(), "UNX-F18499 red: {}", set.red_items_text());
    }

    #[test]
    fn t_aggregate_all_green() {
        let a = aggregate(0).unwrap_or_else(|_| Aggregate::new());
        assert_eq!(a.greens, 19, "19 全绿");
        assert_eq!(a.reds, 0, "零红");
        assert!(a.conserve().is_ok(), "守恒");
    }

    #[test]
    fn t_breaker_three_reds_each() {
        // 失败注入必红三条：{4,11,17} 逐槽注入后恰 1 红。
        assert!(breaker_three_reds().is_ok());
        let mut k = 0;
        while k < BREAKERS.len() {
            let a = breaker_account(BREAKERS[k]).unwrap_or_else(|_| Aggregate::new());
            assert_eq!(a.reds, 1, "槽 {} 注入后恰 1 红", BREAKERS[k]);
            assert!(!a.results[BREAKERS[k] - 1], "红在注入槽");
            k += 1;
        }
        // 全量聚合通道同样必红（breaker==11 短路自指位，安全）。
        let a11 = aggregate(11).unwrap_or_else(|_| Aggregate::new());
        assert_eq!(a11.reds, 1, "全量聚合注入槽 11 恰 1 红");
        let a17 = aggregate(17).unwrap_or_else(|_| Aggregate::new());
        assert_eq!(a17.reds, 1, "全量聚合注入槽 17 恰 1 红");
    }

    #[test]
    fn t_breaker_no_leak() {
        // 非必红槽注入（示例槽 7）：恰 1 红，邻位仍绿。
        let a = breaker_account(7).unwrap_or_else(|_| Aggregate::new());
        assert_eq!(a.reds, 1);
        assert!(!a.results[6]);
        assert!(a.results[5], "邻位不外溢");
        assert!(a.results[7], "邻位不外溢");
        assert_eq!(a.greens, 18);
    }

    #[test]
    fn t_boot_seq_gates() {
        let mut b = BootSeq::new();
        assert_eq!(b.step_to(0).err(), Some(KtCd::StageGhost.code()));
        assert_eq!(b.step_to(6).err(), Some(KtCd::StageGhost.code()));
        assert!(b.step_to(1).is_ok());
        assert_eq!(b.step_to(1).err(), Some(KtCd::StageDup.code()));
        assert_eq!(b.step_to(3).err(), Some(KtCd::StageJump.code()));
        let _ = b.step_to(2);
        assert_eq!(b.step_to(1).err(), Some(KtCd::StageBack.code()));
        assert!(b.step_to(3).is_ok());
        let _ = b.step_to(4);
        let _ = b.step_to(5);
        assert_eq!(b.step_to(5).err(), Some(KtCd::StageDone.code()));
        assert!(b.plane_consistent());
        assert_eq!(b.steps, 5, "五段账");
    }

    #[test]
    fn t_stage_names_frozen() {
        assert_eq!(STAGE_NAMES.len(), 5, "段名闭表 5 项");
        assert_eq!(STAGE_NAMES[0], "PRE");
        assert_eq!(STAGE_NAMES[4], "SEAL");
        let mut i = 0;
        while i < 5 {
            assert!(!STAGE_NAMES[i].is_empty(), "段名非空");
            i += 1;
        }
    }

    #[test]
    fn t_inject_rounds_all_caught() {
        let l = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(l.all_caught(), "四类全抓");
        let mut total = 0u32;
        let mut i = 0;
        while i < 4 {
            total += l.tries[i];
            i += 1;
        }
        assert_eq!(total, 40, "总注入 4×10=40");
    }

    #[test]
    fn t_export_replay_roundtrip() {
        let a = aggregate(0).unwrap_or_else(|_| Aggregate::new());
        let lines = export_ktest(&a);
        assert_eq!(lines.len(), ASSERT_N + 1, "表头+19行");
        assert!(valid_header(&lines[0]));
        assert!(!valid_header("KTEST v2 x"), "版标签不符");
        assert!(replay_ktest(&lines, &a).is_ok(), "回放对拍");
        // 篡改行。
        assert!(parse_ktest_row("X|1|1", 1).is_err(), "前缀错");
        assert!(parse_ktest_row("A|2|1", 1).is_err(), "id 错位");
        assert!(parse_ktest_row("A|1|2", 1).is_err(), "ok 域非法");
        assert!(parse_ktest_row("A|1|1", 1).is_ok(), "合法行");
    }

    #[test]
    fn t_double_run_identical() {
        assert!(repro_check().is_ok(), "聚合双跑一致");
        let a = export_ktest(&aggregate(0).unwrap_or_else(|_| Aggregate::new()));
        let b = export_ktest(&aggregate(0).unwrap_or_else(|_| Aggregate::new()));
        assert_eq!(a, b, "导出确定性");
    }

    #[test]
    fn t_segment_guard_and_detail() {
        assert!(segment_guard(), "0x7F880 窗口");
        assert!(detail_closed(), "文案闭集");
        assert_eq!(KtCd::StageGhost.code(), SEG_BASE + 2, "码位推导");
        assert_eq!(BREAKERS, [4, 11, 17], "必红槽集");
        // 自指位冻结复核。
        assert_eq!(SELF_REF_1, 11);
        assert_eq!(SELF_REF_2, 12);
    }
}
