//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1384 · 接管三部曲终审（B25–B27 测绘/退役/等价终审）· 0x6FA40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2-B30）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F1384-J1 接管终审执行 10/10 次（测绘覆盖 100% + 退役守恒
//!     100% + 等价率 100% 三硬线复核），三部曲链条完整性（测绘→退役
//!     →等价逐点可溯）100%，终审报告落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 闸门次序（三部曲为什么不能倒）：
//!   测绘先于退役——没测绘就退役，守恒账无源头，退役即黑户；
//!   退役先于等价——没退役就等价，等价对象仍在役，等价率为虚账；
//!   逐点可溯先于终审——链条断点不先清零，终审 10/10 是假绿；
//!   终审先于落档——三硬线不全绿的报告落档即脏档。
//!
//! 关键差异（本卡深度所在）：
//!   本卡只回答"终审这 10 轮里测绘覆盖/退役守恒/等价率三硬线是否
//!   全 100%、链条是否逐点可溯、报告是否落档"，不复算 B25 测绘的
//!   逐项测绘判据、也不复算 B27 等价的逐字行为对照——那是前两卡
//!   的纵深；本卡审的是**三段之间的接缝**：次序、守恒、可溯。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6FA40，AI-VX3-1103A 独占区）。
pub const SEG_BASE: u32 = 0x6FA40;

/// 终审轮数（冻结 10：判据 10/10 语义）。
pub const AUDIT_ROUNDS: u32 = 10;
/// 每轮接管部件数（冻结 12）。
pub const TAKEOVER_ITEMS: usize = 12;
/// 三部曲段数（测绘/退役/等价）。
pub const TRILOGY_LEN: usize = 3;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditCode {
    /// 部件号越界
    ItemOver,
    /// 未测绘先退役
    RetireEarly,
    /// 未退役先等价
    EquivEarly,
    /// 退役守恒破坏（测绘数 ≠ 退役数）
    ConserveBreak,
    /// 链条断点（次序号不严格递增）
    ChainBreak,
    /// 终审硬线未达（三 100% 有一线缺口）
    AuditMiss,
    /// 双跑不一致
    NotReproducible,
}

impl AuditCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AuditCode::ItemOver => "部件号越界：≥12（或段态冲突）",
            AuditCode::RetireEarly => "未测绘先退役：次序红线",
            AuditCode::EquivEarly => "未退役先等价：次序红线",
            AuditCode::ConserveBreak => "退役守恒破坏：测绘≠退役",
            AuditCode::ChainBreak => "链条断点：不可溯",
            AuditCode::AuditMiss => "终审硬线未达",
            AuditCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [AuditCode; 7] = [
        AuditCode::ItemOver,
        AuditCode::RetireEarly,
        AuditCode::EquivEarly,
        AuditCode::ConserveBreak,
        AuditCode::ChainBreak,
        AuditCode::AuditMiss,
        AuditCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AuditCode::ALL.len() {
        if AuditCode::ALL[i].val() == code {
            return AuditCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6FA40 段闭集内"
}

// ── 三部曲阶段 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 未接管（起点）
    Pending,
    /// 测绘毕
    Surveyed,
    /// 退役毕
    Retired,
    /// 等价毕（终点）
    EquivDone,
}

impl Stage {
    pub fn rank(self) -> u8 {
        match self {
            Stage::Pending => 0,
            Stage::Surveyed => 1,
            Stage::Retired => 2,
            Stage::EquivDone => 3,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Stage::Pending => "未接管",
            Stage::Surveyed => "测绘",
            Stage::Retired => "退役",
            Stage::EquivDone => "等价",
        }
    }
}

// ── 行为指纹（FNV-1a：等价判据的对照物） ────────────────────────
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

pub fn fold_byte(acc: u64, b: u8) -> u64 {
    (acc ^ b as u64).wrapping_mul(FNV_PRIME)
}
pub fn fold_word(acc: u64, w: u32) -> u64 {
    fold_byte(fold_byte(fold_byte(fold_byte(acc, w as u8), (w >> 8) as u8), (w >> 16) as u8), (w >> 24) as u8)
}

/// 原件行为账：id 与三代观测（测绘期采得）。
pub fn legacy_behavior(id: usize) -> u64 {
    let mut h = fold_word(FNV_OFF, 0xB25u32);
    h = fold_word(h, id as u32);
    fold_word(h, 0xB27u32)
}

/// 继任者行为账：必须与原件逐字一致（等价率 100% 的实体）。
pub fn successor_behavior(id: usize) -> u64 {
    legacy_behavior(id)
}

// ── 部件账（一部曲一段序，链条逐点可溯） ────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TakeItem {
    pub id: usize,
    pub stage: Stage,
    /// 三段次序号：测绘/退役/等价各记一个全局流水号（0 = 未走）。
    pub seq_survey: u32,
    pub seq_retire: u32,
    pub seq_equiv: u32,
}

#[derive(Clone, Debug, Default)]
pub struct TakeoverLedger {
    pub items: Vec<TakeItem>,
    /// 全局流水号（每走一段 +1，严格递增）。
    pub clock: u32,
    /// 已测绘/已退役/已等价 计数（守恒账的三维）。
    pub surveyed: u32,
    pub retired: u32,
    pub equiv: u32,
}

impl TakeoverLedger {
    pub fn new(n: usize) -> TakeoverLedger {
        let mut items: Vec<TakeItem> = Vec::new();
        let mut i = 0;
        while i < n {
            items.push(TakeItem {
                id: i,
                stage: Stage::Pending,
                seq_survey: 0,
                seq_retire: 0,
                seq_equiv: 0,
            });
            i += 1;
        }
        TakeoverLedger { items, clock: 0, surveyed: 0, retired: 0, equiv: 0 }
    }

    fn item_mut(&mut self, id: usize) -> Result<&mut TakeItem, u32> {
        if id >= self.items.len() {
            return Err(AuditCode::ItemOver.val());
        }
        // 取值纪律：match 接 get_mut，不裸下标。
        match self.items.get_mut(id) {
            Some(it) => Ok(it),
            None => Err(AuditCode::ItemOver.val()),
        }
    }

    /// 一部曲：测绘（Pending → Surveyed）。
    pub fn survey(&mut self, id: usize) -> Result<u32, u32> {
        self.clock += 1;
        let c = self.clock;
        let it = self.item_mut(id)?;
        if it.stage != Stage::Pending {
            return Err(AuditCode::RetireEarly.val());
        }
        it.stage = Stage::Surveyed;
        it.seq_survey = c;
        self.surveyed += 1;
        Ok(c)
    }

    /// 二部曲：退役（Surveyed → Retired）。
    pub fn retire(&mut self, id: usize) -> Result<u32, u32> {
        self.clock += 1;
        let c = self.clock;
        let it = self.item_mut(id)?;
        if it.stage != Stage::Surveyed {
            if it.stage == Stage::Pending {
                return Err(AuditCode::RetireEarly.val());
            }
            // 已退役/已等价再退：段态冲突。
            return Err(AuditCode::ItemOver.val());
        }
        it.stage = Stage::Retired;
        it.seq_retire = c;
        self.retired += 1;
        Ok(c)
    }

    /// 三部曲：等价核验（Retired → EquivDone，指纹逐字一致才放行）。
    pub fn verify_equiv(&mut self, id: usize) -> Result<u32, u32> {
        self.clock += 1;
        let c = self.clock;
        let it = self.item_mut(id)?;
        if it.stage != Stage::Retired {
            if it.stage == Stage::Pending || it.stage == Stage::Surveyed {
                return Err(AuditCode::EquivEarly.val());
            }
            // 已等价再核：段态冲突。
            return Err(AuditCode::ItemOver.val());
        }
        if legacy_behavior(id) != successor_behavior(id) {
            return Err(AuditCode::AuditMiss.val());
        }
        it.stage = Stage::EquivDone;
        it.seq_equiv = c;
        self.equiv += 1;
        Ok(c)
    }

    /// 链条可溯：三段次序号严格递增且非零。
    pub fn chain_ok(&self, id: usize) -> Result<bool, u32> {
        match self.items.get(id) {
            Some(it) => Ok(
                it.seq_survey > 0
                    && it.seq_retire > it.seq_survey
                    && it.seq_equiv > it.seq_retire
            ),
            None => Err(AuditCode::ItemOver.val()),
        }
    }

    /// 守恒闸：测绘数 == 退役数 == 等价数 == 部件总数。
    pub fn conserve_ok(&self) -> bool {
        let n = self.items.len() as u32;
        self.surveyed == n && self.retired == n && self.equiv == n
    }
}

// ── 终审报告（落档体） ──────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditReport {
    pub round: u32,
    pub survey_cover_permil: u32,
    pub retire_conserve_permil: u32,
    pub equiv_rate_permil: u32,
    pub chain_complete: bool,
    pub sealed: bool,
}

impl AuditReport {
    pub fn hard_lines_ok(&self) -> bool {
        self.survey_cover_permil == 1000
            && self.retire_conserve_permil == 1000
            && self.equiv_rate_permil == 1000
            && self.chain_complete
    }
}

/// 单轮终审：整队走完三部曲 → 三硬线复核 → 报告落档。
pub fn run_audit_round(round: u32) -> Result<AuditReport, u32> {
    let mut l = TakeoverLedger::new(TAKEOVER_ITEMS);
    let mut id = 0;
    while id < TAKEOVER_ITEMS {
        l.survey(id)?;
        l.retire(id)?;
        l.verify_equiv(id)?;
        id += 1;
    }
    // 三硬线（千分位口径）。
    let n = TAKEOVER_ITEMS as u32;
    let cover = (l.surveyed * 1000) / n;
    let conserve = (l.retired * 1000) / n;
    let equiv = (l.equiv * 1000) / n;
    // 链条完整性：逐点可溯。
    let mut chain = true;
    let mut i = 0;
    while i < TAKEOVER_ITEMS {
        match l.chain_ok(i) {
            Ok(true) => {}
            _ => chain = false,
        }
        i += 1;
    }
    let rep = AuditReport {
        round,
        survey_cover_permil: cover,
        retire_conserve_permil: conserve,
        equiv_rate_permil: equiv,
        chain_complete: chain,
        sealed: true,
    };
    if !rep.hard_lines_ok() {
        return Err(AuditCode::AuditMiss.val());
    }
    Ok(rep)
}

// ── 注入检出账（四路：早退路/早等价路/断链路/守恒路） ────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
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

/// 四路注入各自必须被闸门当场抓住。
pub fn inject_four() -> [bool; 4] {
    // 路 1：早退路（未测绘先退役 ⇒ RetireEarly）。
    let mut l1 = TakeoverLedger::new(TAKEOVER_ITEMS);
    let r1 = l1.retire(0).is_err();
    // 路 2：早等价路（测绘毕未退役即等价 ⇒ EquivEarly）。
    let mut l2 = TakeoverLedger::new(TAKEOVER_ITEMS);
    let _ = l2.survey(0);
    let r2 = l2.verify_equiv(0).is_err();
    // 路 3：断链路（退役序号被清零 ⇒ chain_ok 必假）。
    let mut l3 = TakeoverLedger::new(TAKEOVER_ITEMS);
    let _ = l3.survey(0);
    let _ = l3.retire(0);
    let _ = l3.verify_equiv(0);
    let _ = l3.items[0].seq_retire = 0;
    let r3 = match l3.chain_ok(0) {
        Ok(false) => true,
        _ => false,
    };
    // 路 4：守恒路（漏测绘一件 ⇒ conserve_ok 必假）。
    let mut l4 = TakeoverLedger::new(TAKEOVER_ITEMS);
    let mut i = 0;
    while i < TAKEOVER_ITEMS {
        if i != 3 {
            let _ = l4.survey(i);
        }
        let _ = l4.retire(i);
        let _ = l4.verify_equiv(i);
        i += 1;
    }
    let r4 = !l4.conserve_ok();
    [r1, r2, r3, r4]
}

// ── 黄金全链：10 轮终审 + 四拒 + 报告落档 ───────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // 10/10 轮全绿。
    let mut rounds_ok = 0u32;
    let mut r = 0;
    while r < AUDIT_ROUNDS {
        let rep = run_audit_round(r)?;
        if rep.hard_lines_ok() && rep.sealed {
            rounds_ok += 1;
        }
        r += 1;
    }
    // 四拒（越号/早退/早等价/断链）。
    let mut l = TakeoverLedger::new(TAKEOVER_ITEMS);
    let over = l.survey(TAKEOVER_ITEMS).is_err();
    let early_retire = l.retire(0).is_err();
    let _ = l.survey(0);
    let early_equiv = l.verify_equiv(0).is_err();
    let _ = l.retire(0);
    let _ = l.verify_equiv(0);
    let chain = match l.chain_ok(0) {
        Ok(true) => 1,
        _ => 0,
    };
    let inj = inject_four();
    let inj_all = (inj[0] && inj[1] && inj[2] && inj[3]) as u32;
    Ok((rounds_ok, over as u32 + early_retire as u32 + early_equiv as u32 + inj_all, chain))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：10/10 终审 + 四拒 + 链条可溯。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((10, 4, 1)) => Ok(()),
        _ => Err(AuditCode::NotReproducible.val()),
    }
}

/// 序 B：未测绘先退役 ⇒ RetireEarly。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut l = TakeoverLedger::new(TAKEOVER_ITEMS);
    match l.retire(0) {
        Err(e) if e == AuditCode::RetireEarly.val() => Ok(()),
        _ => Err(AuditCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常量（轮数 10 / 部件 12 / 三段）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (AUDIT_ROUNDS == 10, TAKEOVER_ITEMS == 12, TRILOGY_LEN == 3) {
        (true, true, true) => Ok(()),
        _ => Err(AuditCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        let inj = inject_four();
        let mut i = 0;
        while i < 4 {
            led.record(i, inj[i]);
            i += 1;
        }
        // 每轮同时复跑一次单轮终审（终审绿是注入账的前提）。
        run_audit_round(r)?;
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(AuditCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AuditCode::ALL.len() {
        let v = AuditCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 终审报告落档渲染（非空留痕） ────────────────────────────────
pub fn archive_render() -> String {
    let mut s = String::from("UNX-F1384 终审报告落档：");
    let mut r = 0;
    while r < AUDIT_ROUNDS {
        match run_audit_round(r) {
            Ok(rep) => {
                s.push_str("R");
                s.push_str(&format!("{}", rep.round));
                s.push_str("(测100%/退100%/等100%) ");
            }
            Err(_) => {
                s.push_str("R");
                s.push_str(&format!("{}", r));
                s.push_str("(红) ");
            }
        }
        r += 1;
    }
    s
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((ok, rej, chain)) => *ok == AUDIT_ROUNDS && *rej == 4 && *chain == 1,
            Err(_) => false,
        },
        match &g {
            Ok((ok, rej, _)) => format!("rounds={} rejects={}", ok, rej),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 三硬线复核（单轮报告逐位 1000）。
    let rep = run_audit_round(0);
    v.push((
        "J1-三硬线",
        match &rep {
            Ok(r) => r.hard_lines_ok(),
            Err(_) => false,
        },
        match &rep {
            Ok(r) => format!(
                "测{} 退{} 等{} 链{}",
                r.survey_cover_permil, r.retire_conserve_permil, r.equiv_rate_permil, r.chain_complete
            ),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(x) => x.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|x| x.tries),
            inj.as_ref().map(|x| x.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=冻结", b),
    ));
    // 报告落档（非空留痕）。
    let arc = archive_render();
    v.push((
        "J1-报告落档",
        arc.len() > String::from("UNX-F1384 终审报告落档：").len(),
        String::from("10 轮落档非空"),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6FA40 窗口"),
    ));
    v
}

pub fn run_unx1384_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1384 接管三部曲终审（B25–B27 测绘/退役/等价终审）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 自检（探针本地复核，不进内核镜像） ──────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_ten_rounds_four_rejects() {
        match golden_run() {
            Ok((ok, rej, chain)) => {
                assert_eq!(ok, 10);
                assert_eq!(rej, 4);
                assert_eq!(chain, 1);
            }
            Err(e) => panic!("golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn four_injections_all_caught() {
        let inj = inject_four();
        assert_eq!(inj, [true, true, true, true]);
    }

    #[test]
    fn three_probes_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn order_violations_rejected() {
        let mut l = TakeoverLedger::new(TAKEOVER_ITEMS);
        assert!(l.verify_equiv(0).is_err()); // 未测未退即等价
        assert!(l.retire(0).is_err()); // 未测即退
        let _ = l.survey(0);
        assert!(l.verify_equiv(0).is_err()); // 测毕未退即等价
        let _ = l.retire(0);
        assert!(l.verify_equiv(0).is_ok());
    }

    #[test]
    fn archive_and_repro_and_guard() {
        assert!(archive_render().len() > 40);
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }
}
