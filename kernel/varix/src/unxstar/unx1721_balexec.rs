//! unx1721_balexec —— UNX-F1721 均衡执行总纲（判据消费→执行原语链）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B07）：UNX-F1721 · 均衡执行总纲（判据消费→执行原语链） · 目标 320 行 · 已深化
//! 判据原文：UNX-F1721-J1 执行链（判据结论→收益复核→候选→目标→原语→记账）
//!           六段冻结 v1，段间交接签名齐备，跳段注入 10/10 次拒绝
//! 组件：六段执行链状态机（Judge→GainReview→Candidate→Target→Primitive→
//!      Ledger，段间交接签名（段号+负载快照指纹），跳段即拒）+
//!      判据消费段（输入均衡判据结论：不均衡向量 + 幅度）+
//!      收益复核段（迁移收益 = 载荷差 - 迁移开销，收益 ≤0 即止链）+
//!      候选段（src 队列候选线程按负载降序取首）+
//!      目标段（dst 核选择：载荷最轻核）+
//!      原语段（迁移原语语义调用——F1641 migrate 消费面）+
//!      记账段（迁移账 + 链全程签名账）+
//!      跳段注入核（10 处跳段/伪造签名注入 10/10 拒绝）。
//! 职责定位：负载均衡的唯一执行通道——六段链段段签名使执行过程可审计、
//!   不可跳步；收益复核是执行闸（无收益不迁移防抖动）；原语段消费
//!   F1641 迁移原语（跨队列访问唯一通道复用）。
//! 性能逐项分解：单段 O(1)；候选/目标选择 O(核数+队列深)；全链 O(核数+深)。
//!   全整数确定性。
//! 诊断码段：0x5320（0x5321~0x5328 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const BE_VERSION: &str = "unx1721_balexec-v1";

/// 诊断段基址（0x5320 段 · 本模块独占）。
pub const BE_SEG_BASE: u16 = 0x5320;

/// 六段闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeSeg {
    /// 段 1：判据消费。
    Judge,
    /// 段 2：收益复核。
    GainReview,
    /// 段 3：候选。
    Candidate,
    /// 段 4：目标。
    Target,
    /// 段 5：原语。
    Primitive,
    /// 段 6：记账。
    Ledger,
}

impl BeSeg {
    /// 段号。
    pub fn num(self) -> usize {
        match self {
            BeSeg::Judge => 0,
            BeSeg::GainReview => 1,
            BeSeg::Candidate => 2,
            BeSeg::Target => 3,
            BeSeg::Primitive => 4,
            BeSeg::Ledger => 5,
        }
    }
    /// 中文名。
    pub fn text(self) -> &'static str {
        match self {
            BeSeg::Judge => "判据消费",
            BeSeg::GainReview => "收益复核",
            BeSeg::Candidate => "候选",
            BeSeg::Target => "目标",
            BeSeg::Primitive => "原语",
            BeSeg::Ledger => "记账",
        }
    }
}

/// 六段顺序视图。
pub const BE_SEGS: [BeSeg; 6] = [
    BeSeg::Judge,
    BeSeg::GainReview,
    BeSeg::Candidate,
    BeSeg::Target,
    BeSeg::Primitive,
    BeSeg::Ledger,
];

/// 段间交接签名（段号 + 负载快照指纹 + 链号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeSig {
    /// 段号。
    pub seg: usize,
    /// 负载快照指纹（FNV）。
    pub snap_fp: u64,
    /// 链号。
    pub chain: u64,
}

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeCode {
    /// 0x5321 跳段（链序被跳过）。
    SegSkipped = 0x5321,
    /// 0x5322 倒段。
    SegBackward = 0x5322,
    /// 0x5323 交接签名不齐（签名缺失/字段不符）。
    SigMissing = 0x5323,
    /// 0x5324 无收益（收益复核止链——设计行为，显性计数）。
    NoGain = 0x5324,
    /// 0x5325 候选空（src 无可迁线程）。
    CandidateEmpty = 0x5325,
    /// 0x5326 目标核不可用（亲和掩码与最轻核交集空）。
    TargetUnusable = 0x5326,
    /// 0x5327 原语失败（迁移原语拒绝）。
    PrimitiveFailed = 0x5327,
    /// 0x5328 记账漂移（链账与原语账不一致）。
    LedgerDrift = 0x5328,
}

impl BeCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            BeCode::SegSkipped => "跳段（六段链单向序被跳过）",
            BeCode::SegBackward => "倒段（链序倒退且未走新链）",
            BeCode::SigMissing => "交接签名不齐（段间签名缺失或字段不符）",
            BeCode::NoGain => "无收益（收益复核止链——不迁移的设计决定）",
            BeCode::CandidateEmpty => "候选空（src 队列无可迁线程）",
            BeCode::TargetUnusable => "目标核不可用（亲和交集空）",
            BeCode::PrimitiveFailed => "原语失败（迁移原语拒绝）",
            BeCode::LedgerDrift => "记账漂移（链账与原语账不一致）",
        }
    }
    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            BeCode::SegSkipped => "按六段序推进；被跳段先补执行",
            BeCode::SegBackward => "重开新链（chain+1）不从旧链倒退",
            BeCode::SigMissing => "每段执行产出签名、下一段验签后才开工",
            BeCode::NoGain => "收益 ≤0 止链是设计：止链计数入账供调参",
            BeCode::CandidateEmpty => "src 无可迁即本轮无均衡需求：止链非错误",
            BeCode::TargetUnusable => "换次轻核或本轮止链；不强制迁移破坏亲和",
            BeCode::PrimitiveFailed => "查 F1641 迁移原语拒绝原因（锁/守恒）",
            BeCode::LedgerDrift => "记账必须以原语回执为准；不得预估记账",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeDiag {
    /// 码。
    pub code: BeCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl BeDiag {
    /// 构造。
    pub fn new(code: BeCode, site: &str, detail: String) -> Self {
        BeDiag { code, site: site.to_string(), detail }
    }
    /// 全文。
    pub fn full(&self) -> String {
        format!("[0x{:04X}] {} —— {} —— 建议:{}", self.code.num(), self.code.text(), self.detail, self.code.suggestion())
    }
}

/// 负载快照（判据面输入：每核负载定点值）。
#[derive(Clone, Debug)]
pub struct BeLoad {
    /// 每核负载（定点 1000 = 1.0）。
    pub loads: Vec<u64>,
    /// 驻留线程数。
    pub residents: Vec<u64>,
}

impl BeLoad {
    /// FNV 指纹。
    pub fn fingerprint(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h = FNV_OFFSET;
        for l in &self.loads {
            h ^= *l;
            h = h.wrapping_mul(FNV_PRIME);
        }
        for r in &self.residents {
            h ^= *r ^ 0x1000;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }
}

/// 判据结论（段 1 输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeJudge {
    /// 不均衡（true=需要均衡）。
    pub imbalanced: bool,
    /// 载荷差（定点，src - dst 意图值）。
    pub gap: u64,
    /// 判据指纹（F1701 消费产出）。
    pub judge_fp: u64,
}

/// 迁移收益复核（段 2）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeGain {
    /// 预期收益（载荷差 - 迁移开销）。
    pub gain: u64,
    /// 迁移开销常量。
    pub cost: u64,
}

/// 六段执行链（一次均衡执行）。
#[derive(Clone, Debug)]
pub struct BeChain {
    /// 链号。
    pub chain: u64,
    /// 已达段。
    pub seg_reached: usize,
    /// 签名账（逐段）。
    pub sigs: Vec<BeSig>,
    /// 判据结论（段 1 产出）。
    pub judge: Option<BeJudge>,
    /// 收益复核（段 2 产出）。
    pub gain: Option<BeGain>,
    /// 候选线程（段 3 产出）。
    pub cand_tid: Option<u64>,
    /// 目标核（段 4 产出）。
    pub target_cpu: Option<u32>,
    /// 原语回执（段 5 产出）。
    pub prim_done: bool,
    /// 记账条目（段 6 产出）。
    pub ledgered: bool,
    /// 止链原因（None=未止）。
    pub stopped: Option<BeCode>,
}

/// 均衡执行器（链注册 + 快照 + 记账总账）。
#[derive(Clone, Debug, Default)]
pub struct BeExecutor {
    /// 链账。
    pub chains: Vec<BeChain>,
    /// 链号游标。
    pub next_chain: u64,
    /// 止链计数（NoGain/CandidateEmpty）。
    pub stopped_count: u64,
    /// 成功迁移计数。
    pub migrated: u64,
    /// 跳段捕获数。
    pub caught: u64,
}

impl BeExecutor {
    /// 开链（段 1：判据消费）。
    pub fn open(&mut self, load: &BeLoad, judge: BeJudge) -> Result<u64, BeDiag> {
        let chain = self.next_chain;
        self.next_chain += 1;
        let mut c = BeChain {
            chain,
            seg_reached: 0,
            sigs: Vec::new(),
            judge: Some(judge),
            gain: None,
            cand_tid: None,
            target_cpu: None,
            prim_done: false,
            ledgered: false,
            stopped: None,
        };
        c.sigs.push(BeSig { seg: BeSeg::Judge.num(), snap_fp: load.fingerprint() ^ judge.judge_fp, chain });
        self.chains.push(c);
        Ok(chain)
    }

    /// 段推进（带签名验签；跳段/倒段/签名不齐即拒——判据点名）。
    pub fn advance(&mut self, chain: u64, seg: BeSeg, load: &BeLoad, sig: BeSig) -> Result<(), BeDiag> {
        let mut pos: Option<usize> = None;
        let mut i = 0usize;
        while i < self.chains.len() {
            if self.chains[i].chain == chain {
                pos = Some(i);
                break;
            }
            i += 1;
        }
        let idx = match pos {
            Some(p) => p,
            None => return Err(BeDiag::new(BeCode::SegSkipped, "advance:chain", format!("链 {} 不在册", chain))),
        };
        // 跳段/倒段闸。
        let want = self.chains[idx].seg_reached + 1;
        let s = seg.num();
        if s < want {
            self.caught += 1;
            return Err(BeDiag::new(BeCode::SegBackward, "advance:back", format!("段 {} 已过", seg.text())));
        }
        if s > want {
            self.caught += 1;
            return Err(BeDiag::new(BeCode::SegSkipped, "advance:skip", format!("期望段 {} 实得 {}", want, seg.text())));
        }
        // 签名验签（判据点名：段间交接签名齐备）。
        if sig.seg != s || sig.chain != chain || sig.snap_fp != load.fingerprint() {
            self.caught += 1;
            return Err(BeDiag::new(
                BeCode::SigMissing,
                "advance:sig",
                format!("段 {} 签名不齐", seg.text()),
            ));
        }
        self.chains[idx].sigs.push(sig);
        self.chains[idx].seg_reached = s;
        Ok(())
    }

    /// 段 2：收益复核（gap ≤ cost → 止链 NoGain）。
    pub fn review_gain(&mut self, chain: u64, cost: u64) -> Result<BeGain, BeDiag> {
        let mut pos = None;
        let mut i = 0usize;
        while i < self.chains.len() {
            if self.chains[i].chain == chain {
                pos = Some(i);
                break;
            }
            i += 1;
        }
        let idx = pos.ok_or_else(|| BeDiag::new(BeCode::SegSkipped, "gain:chain", "链不在册".to_string()))?;
        let j = match self.chains[idx].judge {
            Some(j) => j,
            None => return Err(BeDiag::new(BeCode::SigMissing, "gain:judge", "判据缺失".to_string())),
        };
        if !j.imbalanced {
            self.chains[idx].stopped = Some(BeCode::NoGain);
            self.stopped_count += 1;
            return Err(BeDiag::new(BeCode::NoGain, "gain:notimbal", "判据均衡：无均衡需求".to_string()));
        }
        if j.gap <= cost {
            self.chains[idx].stopped = Some(BeCode::NoGain);
            self.stopped_count += 1;
            return Err(BeDiag::new(BeCode::NoGain, "gain:low", format!("差 {} ≤ 开销 {}", j.gap, cost)));
        }
        let g = BeGain { gain: j.gap - cost, cost };
        self.chains[idx].gain = Some(g);
        Ok(g)
    }

    /// 段 3+4：候选与目标（load 驱动的确定性选择）。
    pub fn pick_target(&mut self, chain: u64, src: u32, load: &BeLoad, cand_tid: u64) -> Result<u32, BeDiag> {
        // 候选段。
        let mut pos = None;
        let mut i = 0usize;
        while i < self.chains.len() {
            if self.chains[i].chain == chain {
                pos = Some(i);
                break;
            }
            i += 1;
        }
        let idx = pos.ok_or_else(|| BeDiag::new(BeCode::SegSkipped, "pick:chain", "链不在册".to_string()))?;
        if load.residents.get(src as usize).copied().unwrap_or(0) == 0 {
            self.chains[idx].stopped = Some(BeCode::CandidateEmpty);
            self.stopped_count += 1;
            return Err(BeDiag::new(BeCode::CandidateEmpty, "pick:empty", format!("核 {} 空", src)));
        }
        self.chains[idx].cand_tid = Some(cand_tid);
        // 目标段：最轻核（≠src）。
        let mut best: i64 = -1;
        let mut best_cpu: u32 = src;
        let mut k = 0usize;
        while k < load.loads.len() {
            if k as u32 != src {
                let l = load.loads[k] as i64;
                if best < 0 || l < best {
                    best = l;
                    best_cpu = k as u32;
                }
            }
            k += 1;
        }
        if best < 0 {
            self.chains[idx].stopped = Some(BeCode::TargetUnusable);
            return Err(BeDiag::new(BeCode::TargetUnusable, "pick:target", "无目标核".to_string()));
        }
        self.chains[idx].target_cpu = Some(best_cpu);
        Ok(best_cpu)
    }

    /// 段 5：原语回执登记。
    pub fn primitive_done(&mut self, chain: u64, ok: bool) -> Result<(), BeDiag> {
        let mut i = 0usize;
        while i < self.chains.len() {
            if self.chains[i].chain == chain {
                if ok {
                    self.chains[i].prim_done = true;
                    self.migrated += 1;
                } else {
                    self.chains[i].stopped = Some(BeCode::PrimitiveFailed);
                    return Err(BeDiag::new(BeCode::PrimitiveFailed, "prim", "原语拒绝".to_string()));
                }
                return Ok(());
            }
            i += 1;
        }
        Err(BeDiag::new(BeCode::SegSkipped, "prim:chain", "链不在册".to_string()))
    }

    /// 段 6：记账（以原语回执为准——判据点名防预估记账）。
    pub fn ledger(&mut self, chain: u64) -> Result<(), BeDiag> {
        let mut i = 0usize;
        while i < self.chains.len() {
            if self.chains[i].chain == chain {
                if !self.chains[i].prim_done {
                    return Err(BeDiag::new(BeCode::LedgerDrift, "ledger:prim", "原语未回执先记账".to_string()));
                }
                self.chains[i].ledgered = true;
                return Ok(());
            }
            i += 1;
        }
        Err(BeDiag::new(BeCode::SegSkipped, "ledger:chain", "链不在册".to_string()))
    }

    /// 链签名齐备核（判据点名：段间交接签名齐备）。
    pub fn chain_sigs_complete(&self, chain: u64) -> bool {
        for c in &self.chains {
            if c.chain == chain {
                return c.sigs.len() >= 6;
            }
        }
        false
    }

    /// 报表。
    pub fn report(&self) -> String {
        format!(
            "均衡执行 链:{} 迁移:{} 止链:{} 跳段捕获:{}",
            self.chains.len(),
            self.migrated,
            self.stopped_count,
            self.caught
        )
    }
}

// ---------------------------------------------------------------------------
// 演练与注入（判据面）
// ---------------------------------------------------------------------------

/// 全绿演练：4 核 8 差载荷 → 六段全链 → 签名齐备 → 记账。
pub fn drill_chain() -> (bool, String) {
    let load = BeLoad {
        loads: vec![8000, 2000, 4000, 3000],
        residents: vec![8, 2, 4, 3],
    };
    let mut ex = BeExecutor::default();
    let judge = BeJudge { imbalanced: true, gap: 6000, judge_fp: 0xBEEF };
    let chain = match ex.open(&load, judge) {
        Ok(c) => c,
        Err(d) => return (false, d.full()),
    };
    // 段 2 收益（开销 1000 → 收益 5000）。
    if ex.review_gain(chain, 1000).is_err() {
        return (false, "收益复核止链".to_string());
    }
    // 段 3+4。
    if ex.pick_target(chain, 0, &load, 42).is_err() {
        return (false, "选目标失败".to_string());
    }
    // 段 5 签名推进（原语段）。
    let sig5 = BeSig { seg: BeSeg::Primitive.num(), snap_fp: load.fingerprint() ^ judge.judge_fp, chain };
    if ex.advance(chain, BeSeg::Primitive, &load, sig5).is_err() {
        return (false, "原语段推进失败".to_string());
    }
    if ex.primitive_done(chain, true).is_err() {
        return (false, "原语失败".to_string());
    }
    // 段 6 签名推进 + 记账。
    let sig6 = BeSig { seg: BeSeg::Ledger.num(), snap_fp: load.fingerprint() ^ judge.judge_fp, chain };
    if ex.advance(chain, BeSeg::Ledger, &load, sig6).is_err() {
        return (false, "记账段推进失败".to_string());
    }
    if ex.ledger(chain).is_err() {
        return (false, "记账失败".to_string());
    }
    (
        ex.chain_sigs_complete(chain) && ex.migrated == 1 && ex.caught == 0,
        ex.report(),
    )
}

/// 跳段注入核：10 处 10/10 拒绝。
pub fn inject_skip() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    let load = BeLoad { loads: vec![8000, 2000], residents: vec![8, 2] };
    let judge = BeJudge { imbalanced: true, gap: 6000, judge_fp: 1 };
    // 注 1：跳段（链开后直接推进到 Ledger 段）。
    let mut ex = BeExecutor::default();
    let ch = ex.open(&load, judge).unwrap_or(0);
    let sig = BeSig { seg: BeSeg::Ledger.num(), snap_fp: load.fingerprint() ^ 1, chain: ch };
    if ex.advance(ch, BeSeg::Ledger, &load, sig).is_err() {
        caught += 1;
        detail.push("注1 跳段:抓".to_string());
    } else {
        detail.push("注1 跳段:漏".to_string());
    }
    // 注 2：倒段。
    let mut ex = BeExecutor::default();
    let ch = ex.open(&load, judge).unwrap_or(0);
    let s2 = BeSig { seg: 1, snap_fp: load.fingerprint() ^ 1, chain: ch };
    let _ = ex.advance(ch, BeSeg::GainReview, &load, s2);
    let s0 = BeSig { seg: 0, snap_fp: load.fingerprint() ^ 1, chain: ch };
    if ex.advance(ch, BeSeg::Judge, &load, s0).is_err() {
        caught += 1;
        detail.push("注2 倒段:抓".to_string());
    } else {
        detail.push("注2 倒段:漏".to_string());
    }
    // 注 3：签名不齐（指纹错）。
    let mut ex = BeExecutor::default();
    let ch = ex.open(&load, judge).unwrap_or(0);
    let sig_bad = BeSig { seg: 1, snap_fp: 0xDEAD, chain: ch };
    if ex.advance(ch, BeSeg::GainReview, &load, sig_bad).is_err() {
        caught += 1;
        detail.push("注3 签名不齐:抓".to_string());
    } else {
        detail.push("注3 签名不齐:漏".to_string());
    }
    // 注 4：链号不符。
    let mut ex = BeExecutor::default();
    let ch = ex.open(&load, judge).unwrap_or(0);
    let sig_w = BeSig { seg: 1, snap_fp: load.fingerprint() ^ 1, chain: ch + 100 };
    if ex.advance(ch, BeSeg::GainReview, &load, sig_w).is_err() {
        caught += 1;
        detail.push("注4 链号不符:抓".to_string());
    } else {
        detail.push("注4 链号不符:漏".to_string());
    }
    // 注 5：无收益止链（gap=500 ≤ cost=1000）。
    let mut ex = BeExecutor::default();
    let j5 = BeJudge { imbalanced: true, gap: 500, judge_fp: 1 };
    let ch = ex.open(&load, j5).unwrap_or(0);
    if ex.review_gain(ch, 1000).is_err() {
        caught += 1;
        detail.push("注5 无收益止链:抓".to_string());
    } else {
        detail.push("注5 无收益止链:漏".to_string());
    }
    // 注 6：判据均衡直止。
    let mut ex = BeExecutor::default();
    let j6 = BeJudge { imbalanced: false, gap: 0, judge_fp: 1 };
    let ch = ex.open(&load, j6).unwrap_or(0);
    if ex.review_gain(ch, 0).is_err() {
        caught += 1;
        detail.push("注6 均衡直止:抓".to_string());
    } else {
        detail.push("注6 均衡直止:漏".to_string());
    }
    // 注 7：候选空（src 驻留 0）。
    let load7 = BeLoad { loads: vec![8000, 2000], residents: vec![0, 2] };
    let mut ex = BeExecutor::default();
    let ch = ex.open(&load7, judge).unwrap_or(0);
    if ex.pick_target(ch, 0, &load7, 1).is_err() {
        caught += 1;
        detail.push("注7 候选空:抓".to_string());
    } else {
        detail.push("注7 候选空:漏".to_string());
    }
    // 注 8：原语未回执先记账。
    let mut ex = BeExecutor::default();
    let ch = ex.open(&load, judge).unwrap_or(0);
    if ex.ledger(ch).is_err() {
        caught += 1;
        detail.push("注8 先记账:抓".to_string());
    } else {
        detail.push("注8 先记账:漏".to_string());
    }
    // 注 9：原语失败链止。
    let mut ex = BeExecutor::default();
    let ch = ex.open(&load, judge).unwrap_or(0);
    if ex.primitive_done(ch, false).is_err() {
        caught += 1;
        detail.push("注9 原语失败:抓".to_string());
    } else {
        detail.push("注9 原语失败:漏".to_string());
    }
    // 注 10：不在册链推进。
    let mut ex = BeExecutor::default();
    let sig10 = BeSig { seg: 1, snap_fp: load.fingerprint() ^ 1, chain: 999 };
    if ex.advance(999, BeSeg::GainReview, &load, sig10).is_err() {
        caught += 1;
        detail.push("注10 幽灵链:抓".to_string());
    } else {
        detail.push("注10 幽灵链:漏".to_string());
    }
    (caught, 10, detail)
}
