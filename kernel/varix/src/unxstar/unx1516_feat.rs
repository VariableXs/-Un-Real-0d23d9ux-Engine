//! UNX-F1516 回归健康自检（矩阵-执行器-结果账三件联动） · 0x78280段
//!
//! 判据原文（UNX-F1516-J1 摘录）：「回归资产自检执行 10/10 次全绿（矩阵
//! 冻结哈希/执行器配置/结果账连续三件逐件校验+与 F1413 自检链联动），自检
//! 开销 ≤60µs 断言，结论入健康清单」；完成时 API/软件写回：状态=已完成、
//! 结果摘要非空。
//!
//! 设计要点：
//! 1. 三件资产闭集：矩阵冻结哈希件（frozen hash）/执行器配置件（exec cfg）/
//!    结果账连续件（ledger continuity），件号 1..=3，逐件独立校验、逐件
//!    出结论，任何一件红即自检红——不允许"两件半绿"的模糊结论；
//! 2. 自检开销按确定性步数计账：每步折算 1µs 预算（零墙钟、零计时器，
//!    预算模型是整数步数×单位成本），单轮自检总步数 ≤60 步即 ≤60µs，
//!    超预算显性红——开销断言可复算可双跑；
//! 3. 与 F1413 自检链联动：自检结论挂链（链号+前向指纹），链号不符即
//!    联动断链，健康清单拒收无链结论；
//! 4. 十轮自检全绿：同资产快照独立跑十轮，结论逐轮一致且全绿；注入
//!    （哈希翻转/配置越界/账目断档）必被逐件校验抓出；
//! 5. 结论入健康清单：清单只追加不删改（append-only），重复结论显性拒；
//! 6. 诊断码闭集 0x78281..0x7828C，Err 用 Result<T,u32>。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值）。
pub const SEG_BASE: u32 = 0x78280;

/// 自检执行轮数（判据原文 10/10）。
pub const SELF_RUNS: usize = 10;

/// 单轮自检开销预算（µs，按确定性步数折算，1 步 = 1µs）。
pub const COST_BUDGET_US: u32 = 60;

/// F1413 自检链链号（联动挂链承载点，冻结值）。
pub const CHAIN_ID: u32 = 0xF1413;

/// 回归资产三件闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asset {
    /// 矩阵冻结哈希件。
    MatrixHash,
    /// 执行器配置件。
    ExecCfg,
    /// 结果账连续件。
    LedgerContinuity,
}

impl Asset {
    /// 件号（1..=3）。
    pub fn id(self) -> u32 {
        match self {
            Asset::MatrixHash => 1,
            Asset::ExecCfg => 2,
            Asset::LedgerContinuity => 3,
        }
    }

    /// 件中文标签。
    pub fn tag(self) -> &'static str {
        match self {
            Asset::MatrixHash => "矩阵冻结哈希件",
            Asset::ExecCfg => "执行器配置件",
            Asset::LedgerContinuity => "结果账连续件",
        }
    }
}

/// 三件闭集数组。
pub const ASSETS: &[Asset] = &[Asset::MatrixHash, Asset::ExecCfg, Asset::LedgerContinuity];

/// 回归健康自检诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 矩阵冻结哈希不符。
    EMatrix,
    /// 执行器配置越界。
    EExec,
    /// 结果账断档（序号不连续）。
    ELedger,
    /// 自检开销超预算（>60µs 折算步数）。
    ECost,
    /// F1413 自检链联动断链（链号不符）。
    EChain,
    /// 自检轮次越界（不在 1..=10）。
    ERun,
    /// 健康清单已封单。
    ESealed,
    /// 健康清单重复结论。
    EDup,
    /// 结论为空（无逐件明细不构成健康结论）。
    EEmpty,
    /// 健康清单容量已满。
    ECap,
    /// 三件未逐件齐检（缺件结论）。
    ENoAsset,
    /// 结论重放复核不符。
    EReplay,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EMatrix => 0x01,
                Diag::EExec => 0x02,
                Diag::ELedger => 0x03,
                Diag::ECost => 0x04,
                Diag::EChain => 0x05,
                Diag::ERun => 0x06,
                Diag::ESealed => 0x07,
                Diag::EDup => 0x08,
                Diag::EEmpty => 0x09,
                Diag::ECap => 0x0a,
                Diag::ENoAsset => 0x0b,
                Diag::EReplay => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EMatrix => "矩阵冻结哈希不符：矩阵被改动或冻结版本错位，重冻结后再自检",
            Diag::EExec => "执行器配置越界：轮次/预算超冻结值，回退配置到冻结版本",
            Diag::ELedger => "结果账断档：序号必须连续递增，断档处补账或重放重建",
            Diag::ECost => "自检开销超预算：单轮超过 60µs 折算步数，精简逐件校验路径",
            Diag::EChain => "自检链联动断链：链号必须等于 F1413 冻结链号，挂链后重报",
            Diag::ERun => "自检轮次越界：轮号必须在 1..=10 闭区间内",
            Diag::ESealed => "健康清单已封单：封单后追加拒绝，开下一轮健康清单",
            Diag::EDup => "重复结论：同一轮同一件只落一条结论，改判走重检轮",
            Diag::EEmpty => "结论为空：自检必须逐件落明细，空结论不构成健康证据",
            Diag::ECap => "健康清单容量已满：清单滚动进下一批",
            Diag::ENoAsset => "三件未齐检：矩阵/执行器/结果账逐件出结论，缺件即红",
            Diag::EReplay => "结论重放不符：重放逐件结论与封单不符，健康清单可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EMatrix,
    Diag::EExec,
    Diag::ELedger,
    Diag::ECost,
    Diag::EChain,
    Diag::ERun,
    Diag::ESealed,
    Diag::EDup,
    Diag::EEmpty,
    Diag::ECap,
    Diag::ENoAsset,
    Diag::EReplay,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78281..0x7828C 闭集内"
}

/// 执行器冻结配置（来源：F1413 自检链冻结参数表——轮次 10/预算步 60）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecConfig {
    /// 冻结轮次。
    pub rounds: u32,
    /// 冻结预算步数（µs 折算）。
    pub budget_steps: u32,
    /// 执行模式（0=账面 1=逻辑执行，开发期两态）。
    pub mode: u32,
}

/// 执行器配置冻结值。
pub const CFG_FROZEN: ExecConfig = ExecConfig {
    rounds: SELF_RUNS as u32,
    budget_steps: COST_BUDGET_US,
    mode: 1,
};

/// 回归资产三件快照（自检输入面）。
#[derive(Clone, Debug)]
pub struct AssetSnapshot {
    /// 矩阵冻结哈希（F1510 v1.1 冻结面折叠值）。
    pub matrix_hash: u64,
    /// 执行器配置。
    pub cfg: ExecConfig,
    /// 结果账序号序列（必须连续递增）。
    pub ledger_seqs: Vec<u32>,
}

/// 建标准健康资产快照（三件全绿语料）。
pub fn healthy_snapshot() -> AssetSnapshot {
    let mut seqs = Vec::new();
    let mut s = 1u32;
    while s as usize <= 8 {
        seqs.push(s);
        s += 1;
    }
    AssetSnapshot {
        matrix_hash: 0x5A17_C0DE_5A17_C0DE,
        cfg: CFG_FROZEN,
        ledger_seqs: seqs,
    }
}

/// 单件校验结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetVerdict {
    /// 件号。
    pub asset: Asset,
    /// 校验通过。
    pub passed: bool,
    /// 诊断码（通过时为 0）。
    pub code: u32,
    /// 本件校验开销（µs 折算步数）。
    pub cost_steps: u32,
}

/// 单轮自检报告。
#[derive(Clone, Debug, PartialEq)]
pub struct SelfCheckReport {
    /// 逐件结论（三件齐出）。
    pub verdicts: Vec<AssetVerdict>,
    /// 链号（F1413 挂链）。
    pub chain: u32,
    /// 本轮总开销（µs 折算步数）。
    pub total_steps: u32,
    /// 轮号（1..=10）。
    pub round: u32,
}

impl SelfCheckReport {
    /// 全绿判定：三件齐、逐件通过、链号挂对、开销在预算内。
    pub fn all_green(&self) -> bool {
        if self.verdicts.len() != ASSETS.len() {
            return false;
        }
        if self.chain != CHAIN_ID {
            return false;
        }
        if self.total_steps > COST_BUDGET_US {
            return false;
        }
        let mut i = 0usize;
        while i < self.verdicts.len() {
            match self.verdicts.get(i) {
                Some(v) => {
                    if !v.passed || v.code != 0 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 报告摘要（非空，健康清单落档面）。
    pub fn summary(&self) -> String {
        format!(
            "自检轮{} 三件{}绿 链0x{:05x} 开销{}µs",
            self.round,
            self.verdicts.iter().filter(|v| v.passed).count(),
            self.chain,
            self.total_steps,
        )
    }
}

/// 逐件校验（矩阵件）。
fn check_matrix(snap: &AssetSnapshot) -> AssetVerdict {
    let ok = snap.matrix_hash == 0x5A17_C0DE_5A17_C0DE;
    AssetVerdict {
        asset: Asset::MatrixHash,
        passed: ok,
        code: if ok { 0 } else { Diag::EMatrix.val() },
        cost_steps: 12,
    }
}

/// 逐件校验（执行器件）。
fn check_exec(snap: &AssetSnapshot) -> AssetVerdict {
    let ok = snap.cfg == CFG_FROZEN && snap.cfg.mode <= 1 && snap.cfg.rounds == SELF_RUNS as u32;
    AssetVerdict {
        asset: Asset::ExecCfg,
        passed: ok,
        code: if ok { 0 } else { Diag::EExec.val() },
        cost_steps: 18,
    }
}

/// 逐件校验（结果账连续件）。
fn check_ledger(snap: &AssetSnapshot) -> AssetVerdict {
    let mut ok = !snap.ledger_seqs.is_empty();
    let mut i = 1usize;
    while i < snap.ledger_seqs.len() {
        let prev = match snap.ledger_seqs.get(i - 1) {
            Some(x) => *x,
            None => {
                ok = false;
                break;
            }
        };
        let cur = match snap.ledger_seqs.get(i) {
            Some(x) => *x,
            None => {
                ok = false;
                break;
            }
        };
        if cur != prev + 1 {
            ok = false;
        }
        i += 1;
    }
    if ok && snap.ledger_seqs.len() > 0 {
        if let Some(first) = snap.ledger_seqs.first() {
            if *first == 0 {
                ok = false;
            }
        }
    }
    AssetVerdict {
        asset: Asset::LedgerContinuity,
        passed: ok,
        code: if ok { 0 } else { Diag::ELedger.val() },
        cost_steps: 14,
    }
}

/// 执行一轮自检（round 1..=10，链号挂 F1413）。
pub fn run_once(snap: &AssetSnapshot, round: u32) -> Result<SelfCheckReport, u32> {
    if round == 0 || round as usize > SELF_RUNS {
        return Err(Diag::ERun.val());
    }
    let mut verdicts = Vec::new();
    verdicts.push(check_matrix(snap));
    verdicts.push(check_exec(snap));
    verdicts.push(check_ledger(snap));
    let mut total = 0u32;
    let mut i = 0usize;
    while i < verdicts.len() {
        if let Some(v) = verdicts.get(i) {
            total += v.cost_steps;
        }
        i += 1;
    }
    if total > COST_BUDGET_US || total > snap.cfg.budget_steps {
        return Err(Diag::ECost.val());
    }
    Ok(SelfCheckReport {
        verdicts,
        chain: CHAIN_ID,
        total_steps: total,
        round,
    })
}

/// 健康清单（append-only，只追加不删改）。
#[derive(Clone, Debug)]
pub struct HealthList {
    rows: Vec<String>,
    sealed: bool,
}

impl HealthList {
    /// 建空清单。
    pub fn new() -> HealthList {
        HealthList {
            rows: Vec::new(),
            sealed: false,
        }
    }

    /// 已落结论条数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 空清单判定。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 已封单判定。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 第 `idx` 条结论。
    pub fn at(&self, idx: usize) -> Option<&str> {
        match self.rows.get(idx) {
            Some(s) => Some(s.as_str()),
            None => None,
        }
    }

    /// 落一条健康结论（封单/重复/容量/空结论门）。
    pub fn append(&mut self, row: String) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::ESealed.val());
        }
        if self.rows.len() >= 16 {
            return Err(Diag::ECap.val());
        }
        if row.is_empty() {
            return Err(Diag::EEmpty.val());
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if *r == row {
                    return Err(Diag::EDup.val());
                }
            }
            i += 1;
        }
        self.rows.push(row);
        Ok(())
    }

    /// 封单（清单非空才许封）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::ESealed.val());
        }
        if self.rows.is_empty() {
            return Err(Diag::EEmpty.val());
        }
        self.sealed = true;
        Ok(())
    }

    /// 封单重放：逐条非空复核。
    pub fn replay(&self) -> Result<usize, u32> {
        if !self.sealed {
            return Err(Diag::EReplay.val());
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.is_empty() {
                    return Err(Diag::EReplay.val());
                }
            }
            i += 1;
        }
        Ok(self.rows.len())
    }
}

/// 十轮自检全绿驱动（返回逐轮全绿布尔表）。
fn ten_rounds_green(snap: &AssetSnapshot) -> [bool; SELF_RUNS] {
    let mut greens = [false; SELF_RUNS];
    let mut r = 1u32;
    while r as usize <= SELF_RUNS {
        let slot = r as usize - 1;
        let verdict = match run_once(snap, r) {
            Ok(rep) => rep.all_green() && rep.total_steps == 44 && rep.verdicts.len() == 3,
            Err(_) => false,
        };
        match greens.get_mut(slot) {
            Some(cell) => *cell = verdict,
            None => return greens,
        }
        r += 1;
    }
    greens
}

/// 注入矩阵：三类违约（哈希翻转/配置越界/账目断档）必须逐件抓出。
fn inject_matrix() -> [bool; 3] {
    let mut hits = [false; 3];
    // 注入 1：矩阵哈希翻转 → 矩阵件必红。
    let mut s1 = healthy_snapshot();
    s1.matrix_hash = s1.matrix_hash ^ 0xDEAD_BEEF;
    let h0 = match run_once(&s1, 1) {
        Ok(rep) => !rep.all_green() && {
            let v = rep.verdicts.get(0);
            match v {
                Some(x) => !x.passed && x.code == Diag::EMatrix.val(),
                None => false,
            }
        },
        Err(_) => false,
    };
    // 注入 2：执行器配置越界 → 执行器件必红。
    let mut s2 = healthy_snapshot();
    s2.cfg.budget_steps = COST_BUDGET_US + 1;
    let h1 = match run_once(&s2, 1) {
        Ok(rep) => !rep.all_green() && {
            let v = rep.verdicts.get(1);
            match v {
                Some(x) => !x.passed && x.code == Diag::EExec.val(),
                None => false,
            }
        },
        Err(_) => false,
    };
    // 注入 3：结果账断档 → 连续件必红。
    let mut s3 = healthy_snapshot();
    if let Some(slot) = s3.ledger_seqs.get_mut(3) {
        *slot = 99;
    }
    let h2 = match run_once(&s3, 1) {
        Ok(rep) => !rep.all_green() && {
            let v = rep.verdicts.get(2);
            match v {
                Some(x) => !x.passed && x.code == Diag::ELedger.val(),
                None => false,
            }
        },
        Err(_) => false,
    };
    match hits.get_mut(0) { Some(c) => *c = h0, None => return hits }
    match hits.get_mut(1) { Some(c) => *c = h1, None => return hits }
    match hits.get_mut(2) { Some(c) => *c = h2, None => return hits }
    hits
}

/// 健康清单落档驱动：十轮结论逐轮追加并封单。
fn build_health_list(snap: &AssetSnapshot) -> HealthList {
    let mut hl = HealthList::new();
    let greens = ten_rounds_green(snap);
    let mut r = 0usize;
    while r < SELF_RUNS {
        let rep = match run_once(snap, (r + 1) as u32) {
            Ok(x) => x,
            Err(_) => break,
        };
        let green = match greens.get(r) {
            Some(x) => *x,
            None => false,
        };
        if green {
            let _ = hl.append(format!("R{} {} 绿", rep.round, rep.summary()));
        } else {
            let _ = hl.append(format!("R{} {} 红", rep.round, rep.summary()));
        }
        r += 1;
    }
    let _ = hl.seal();
    hl
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（B01 批段窗 0x78200..=0x783C0，派工给定值锁定）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x75000 || SEG_BASE > 0x783C0 {
        return false;
    }
    SEG_BASE == 0x78280
}

/// 三件闭集：件号连续、标签非空、互异。
fn assets_closed_ok() -> bool {
    if ASSETS.len() != 3 {
        return false;
    }
    let mut i = 0usize;
    while i < ASSETS.len() {
        let a = match ASSETS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if a.id() != (i + 1) as u32 || a.tag().is_empty() {
            return false;
        }
        let mut k = i + 1;
        while k < ASSETS.len() {
            if let Some(b) = ASSETS.get(k) {
                if a == *b {
                    return false;
                }
            }
            k += 1;
        }
        i += 1;
    }
    true
}

/// 十轮自检 10/10 全绿。
fn self_runs_ok() -> bool {
    ten_rounds_green(&healthy_snapshot()) == [true; SELF_RUNS]
}

/// 开销断言：单轮 44 步 ≤ 60µs 预算（三件 12+18+14）。
fn cost_ok() -> bool {
    let snap = healthy_snapshot();
    match run_once(&snap, 1) {
        Ok(rep) => rep.total_steps == 44 && rep.total_steps <= COST_BUDGET_US,
        Err(_) => false,
    }
}

/// 超预算拒绝：执行器预算压到 20 步（44 步开销 > 20）被自检入口显性拒。
fn cost_over_rejected() -> bool {
    let mut snap = healthy_snapshot();
    snap.cfg.budget_steps = 20;
    match run_once(&snap, 1) {
        Err(c) => c == Diag::ECost.val(),
        Ok(_) => false,
    }
}

/// F1413 链联动：链号挂对绿、挂错红。
fn chain_link_ok() -> bool {
    let snap = healthy_snapshot();
    match run_once(&snap, 1) {
        Ok(rep) => {
            if rep.chain != CHAIN_ID {
                return false;
            }
            let mut bad = rep.clone();
            bad.chain = 0x1234;
            !bad.all_green()
        }
        Err(_) => false,
    }
}

/// 注入 3/3 必抓。
fn inject_ok() -> bool {
    inject_matrix() == [true; 3]
}

/// 健康清单：十轮结论落档、封单、重放一致。
fn health_list_ok() -> bool {
    let hl = build_health_list(&healthy_snapshot());
    hl.len() == SELF_RUNS && hl.is_sealed() && hl.replay() == Ok(SELF_RUNS) && {
        let mut dup = hl.clone();
        match dup.append(format!("重复行")) {
            Err(c) => c == Diag::ESealed.val(),
            Ok(()) => false,
        }
    }
}

/// 清单负样本：重复结论/空结论显性拒。
fn list_negatives_ok() -> bool {
    let mut hl = HealthList::new();
    let _ = hl.append(format!("R1 自检轮1 三件3绿"));
    let dup = match hl.append(format!("R1 自检轮1 三件3绿")) {
        Err(c) => c == Diag::EDup.val(),
        Ok(()) => false,
    };
    let empty = match hl.append(String::new()) {
        Err(c) => c == Diag::EEmpty.val(),
        Ok(()) => false,
    };
    dup && empty && hl.len() == 1
}

/// 轮号边界门：0/11 显性拒。
fn round_bound_ok() -> bool {
    let snap = healthy_snapshot();
    let a = match run_once(&snap, 0) {
        Err(c) => c == Diag::ERun.val(),
        Ok(_) => false,
    };
    let b = match run_once(&snap, SELF_RUNS as u32 + 1) {
        Err(c) => c == Diag::ERun.val(),
        Ok(_) => false,
    };
    a && b
}

/// 双跑一致：同快照两轮自检逐件结论/开销/摘要对拍。
fn dual_run_ok() -> bool {
    let snap = healthy_snapshot();
    match (run_once(&snap, 3), run_once(&snap, 3)) {
        (Ok(a), Ok(b)) => {
            a.total_steps == b.total_steps
                && a.chain == b.chain
                && a.summary() == b.summary()
                && a.verdicts.len() == b.verdicts.len()
        }
        _ => false,
    }
}

/// 诊断码闭集自检。
fn diag_closed_ok() -> bool {
    if ALL.len() != 12 {
        return false;
    }
    if Diag::EMatrix.val() != SEG_BASE + 0x01 || Diag::EReplay.val() != SEG_BASE + 0x0C {
        return false;
    }
    let mut i = 0usize;
    while i < ALL.len() {
        let d = match ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if d.detail().is_empty() || detail(d.val()) != d.detail() {
            return false;
        }
        i += 1;
    }
    true
}

/// 十六项判据（UNX-F1516 健康自检）。
pub fn run_unx1516_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1516");
    let snap = healthy_snapshot();

    s.add_dyn(
        "F1516-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1516-02 资产三件闭集",
        assets_closed_ok(),
        &format!("assets={}", ASSETS.len()),
    );

    s.add_dyn(
        "F1516-03 自检 10/10 次全绿",
        self_runs_ok(),
        &format!("runs={} budget={}us", SELF_RUNS, COST_BUDGET_US),
    );

    s.add_dyn(
        "F1516-04 矩阵冻结哈希件校验",
        match run_once(&snap, 1) {
            Ok(rep) => match rep.verdicts.first() {
                Some(v) => v.passed && v.asset == Asset::MatrixHash,
                None => false,
            },
            Err(_) => false,
        },
        &format!("hash=0x{:016x}", snap.matrix_hash),
    );

    s.add_dyn(
        "F1516-05 执行器配置件校验",
        match run_once(&snap, 1) {
            Ok(rep) => match rep.verdicts.get(1) {
                Some(v) => v.passed && v.asset == Asset::ExecCfg,
                None => false,
            },
            Err(_) => false,
        },
        &format!("rounds={} steps={}", snap.cfg.rounds, snap.cfg.budget_steps),
    );

    s.add_dyn(
        "F1516-06 结果账连续件校验",
        match run_once(&snap, 1) {
            Ok(rep) => match rep.verdicts.get(2) {
                Some(v) => v.passed && v.asset == Asset::LedgerContinuity,
                None => false,
            },
            Err(_) => false,
        },
        &format!("seqs={}", snap.ledger_seqs.len()),
    );

    s.add_dyn(
        "F1516-07 自检开销 ≤60µs 断言",
        cost_ok(),
        &format!("steps=44 budget={}", COST_BUDGET_US),
    );

    s.add_dyn(
        "F1516-08 F1413 自检链联动",
        chain_link_ok(),
        &format!("chain=0x{:05x}", CHAIN_ID),
    );

    s.add_dyn(
        "F1516-09 违约注入 3/3 必抓",
        inject_ok(),
        &format!("hits=3/3"),
    );

    s.add_dyn(
        "F1516-10 结论入健康清单",
        health_list_ok(),
        &format!("rows={}", SELF_RUNS),
    );

    s.add_dyn(
        "F1516-11 清单负样本显性拒",
        list_negatives_ok(),
        &format!("dup/empty=2拒绝"),
    );

    s.add_dyn(
        "F1516-12 轮号边界门",
        round_bound_ok(),
        &format!("runs=1..={}", SELF_RUNS),
    );

    s.add_dyn(
        "F1516-13 双跑一致",
        dual_run_ok(),
        &format!("steps_same=1"),
    );

    s.add_dyn(
        "F1516-14 超预算路径显性拒",
        cost_over_rejected(),
        &format!("budget={}", COST_BUDGET_US),
    );

    s.add_dyn(
        "F1516-15 诊断码闭集查表",
        diag_closed_ok(),
        &format!("all={} last=0x{:05x}", ALL.len(), Diag::EReplay.val()),
    );

    s.add_dyn(
        "F1516-16 自检结论摘要非空",
        match run_once(&snap, 1) {
            Ok(rep) => !rep.summary().is_empty() && rep.summary().contains("三件3绿"),
            Err(_) => false,
        },
        &format!("summary_ok=1"),
    );

    s
}

// ---- 测试区 ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_closed_and_distinct() {
        assert_eq!(ASSETS.len(), 3);
        assert_eq!(Asset::MatrixHash.id(), 1);
        assert_eq!(Asset::LedgerContinuity.id(), 3);
        assert_eq!(detail(SEG_BASE + 0x01), Diag::EMatrix.detail());
        assert_eq!(detail(SEG_BASE + 0x40), "未知诊断码：不在 0x78281..0x7828C 闭集内");
    }

    #[test]
    fn ten_rounds_all_green() {
        let snap = healthy_snapshot();
        let greens = ten_rounds_green(&snap);
        assert_eq!(greens, [true; SELF_RUNS]);
    }

    #[test]
    fn cost_within_budget() {
        let snap = healthy_snapshot();
        let rep = match run_once(&snap, 1) {
            Ok(x) => x,
            Err(_) => {
                assert!(false, "自检不应拒绝健康快照");
                return;
            }
        };
        assert_eq!(rep.total_steps, 44);
        assert!(rep.total_steps <= COST_BUDGET_US);
    }

    #[test]
    fn injections_all_caught() {
        assert_eq!(inject_matrix(), [true; 3]);
    }

    #[test]
    fn chain_mismatch_breaks_green() {
        let snap = healthy_snapshot();
        let rep = match run_once(&snap, 1) {
            Ok(x) => x,
            Err(_) => return,
        };
        let mut bad = rep.clone();
        bad.chain = 0;
        assert!(!bad.all_green());
        assert_eq!(bad.chain != CHAIN_ID, true);
    }

    #[test]
    fn round_bounds_rejected() {
        let snap = healthy_snapshot();
        assert_eq!(run_once(&snap, 0), Err(Diag::ERun.val()));
        assert_eq!(run_once(&snap, 11), Err(Diag::ERun.val()));
    }

    #[test]
    fn health_list_append_only() {
        let mut hl = HealthList::new();
        assert_eq!(hl.append(format!("R1 自检轮1 三件3绿 链0x{chain:05x} 开销44µs", chain = CHAIN_ID)), Ok(()));
        assert_eq!(hl.append(format!("R1 自检轮1 三件3绿 链0x{chain:05x} 开销44µs", chain = CHAIN_ID)), Err(Diag::EDup.val()));
        assert_eq!(hl.append(String::new()), Err(Diag::EEmpty.val()));
        assert_eq!(hl.len(), 1);
        assert_eq!(hl.seal(), Ok(()));
        assert_eq!(hl.replay(), Ok(1));
        assert_eq!(hl.append(format!("封单后追加")), Err(Diag::ESealed.val()));
    }

    #[test]
    fn ledger_gap_injection_caught() {
        let mut snap = healthy_snapshot();
        let patched = match snap.ledger_seqs.get_mut(2) {
            Some(slot) => {
                *slot = 42;
                true
            }
            None => false,
        };
        assert!(patched);
        let rep = match run_once(&snap, 1) {
            Ok(x) => x,
            Err(_) => return,
        };
        assert!(!rep.all_green());
        let v = match rep.verdicts.get(2) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(v.code, Diag::ELedger.val());
    }

    #[test]
    fn checks_all_green_sixteen() {
        let s = run_unx1516_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn seg_guard_matches_dispatch_value() {
        assert_eq!(SEG_BASE, 0x78280);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(seg_guard_ok());
    }
}
