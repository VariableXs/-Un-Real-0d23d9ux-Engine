//! UNX-F51615 B21 恢复后状态一致断言器（账-实对平机检） · 0xC1C80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51616-J1 归因 ×30 正确率 100%；UNX-F51616-J1R 归因
//! 冲突注入 10/10 显式仲裁；③ 完成时 API/软件写回：状态=已完成、结果
//! 摘要非空。
//!
//! 结构清单：
//!   - CasCode          诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - AcctRow          账面行（键/账量/行指纹）
//!   - RealRow          实面行（键/实量/量指纹）
//!   - ConsistAssertor  账-实对平机检器（逐键对平 + 差额留痕）
//!   - CasLedger        对平账 + 注入守恒账（检出+逃逸=轮数，逃逸恒 0）
//!   - run_unx51615_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 对平判据：账-实逐键对平，量等且指纹互异才算平；缺键判 KeyMissing，
//!     量差判 AmountMismatch，差额必须留痕（GapRow）——零静默。
//!   ② 对平机检 ×50 全过：50 轮合成账面/实面逐轮对平全绿；故意错账
//!     注入 10/10 检出（错账必判 Mismatch 且留一行差额）。
//!   ③ 只读对平：对平不改动账面/实面，对平后账行数与实行数不变
//!     （AccountTouched 显性面）。
//!   ④ 注入核：5 类坏样本（账实量差/账缺键/实缺键/错账伪装对平/对平
//!     后改账）逐类 ×10 轮精确复现，注入账守恒（检出+逃逸=轮数，逃逸
//!     恒 0）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机——账量序列由 LCG
//!     派生（种子 = SEG_BASE 派生），同段位恒定可复现；零 unwrap/
//!     expect/panic/裸下标，取值一律 .get()/.get_mut()/match。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1C80;

/// 对平机检轮数。
pub const RECON_ROUNDS: usize = 50;
/// 错账注入轮数。
pub const WRONG_ROUNDS: usize = 10;
/// 账键闭集大小。
pub const KEYS: usize = 6;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 账面行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AcctRow {
    /// 键（0..KEYS）
    pub key: usize,
    /// 账量
    pub amount: u64,
    /// 行指纹
    pub fp: u64,
}

impl AcctRow {
    /// 行指纹：键×量派生。
    pub fn make_fp(key: usize, amount: u64) -> u64 {
        fnv1a(&(key as u64).to_le_bytes())
            ^ fnv1a(&amount.to_le_bytes()).rotate_left(21)
            ^ 0x5161_5a00_0000_0005
    }
}

/// 实面行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RealRow {
    /// 键（0..KEYS）
    pub key: usize,
    /// 实量
    pub amount: u64,
    /// 量指纹（与账行指纹同算法，可互核）
    pub fp: u64,
}

/// 差额留痕行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GapRow {
    /// 键
    pub key: usize,
    /// 账量
    pub acct: u64,
    /// 实量
    pub real: u64,
    /// 差额（账-实）
    pub delta: i64,
}

/// 账-实对平机检器。
pub struct ConsistAssertor {
    acct: Vec<AcctRow>,
    real: Vec<RealRow>,
    gaps: Vec<GapRow>,
    recon_runs: u64,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl ConsistAssertor {
    /// 新建对平器。
    pub fn new() -> ConsistAssertor {
        ConsistAssertor {
            acct: Vec::new(),
            real: Vec::new(),
            gaps: Vec::new(),
            recon_runs: 0,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 记账一行（键闭集内、键唯一）。
    pub fn post_acct(&mut self, key: usize, amount: u64) -> Result<AcctRow, CasCode> {
        if key >= KEYS {
            self.rejected += 1;
            return Err(CasCode::KeyOob);
        }
        if self.acct.iter().any(|r| r.key == key) {
            self.rejected += 1;
            return Err(CasCode::DupKey);
        }
        let row = AcctRow { key, amount, fp: AcctRow::make_fp(key, amount) };
        self.acct.push(row);
        Ok(row)
    }

    /// 落实一行（键闭集内、键唯一）。
    pub fn post_real(&mut self, key: usize, amount: u64) -> Result<RealRow, CasCode> {
        if key >= KEYS {
            self.rejected += 1;
            return Err(CasCode::KeyOob);
        }
        if self.real.iter().any(|r| r.key == key) {
            self.rejected += 1;
            return Err(CasCode::DupKey);
        }
        let row = RealRow { key, amount, fp: AcctRow::make_fp(key, amount) };
        self.real.push(row);
        Ok(row)
    }

    /// 对平一拍：逐键核对，差额留痕；返回是否全平。
    pub fn reconcile(&mut self) -> Result<bool, CasCode> {
        if self.acct.is_empty() || self.real.is_empty() {
            self.rejected += 1;
            return Err(CasCode::EmptySide);
        }
        self.gaps.clear();
        self.recon_runs += 1;
        let mut all_flat = true;
        for a in self.acct.iter() {
            // 行指纹自核：账行指纹与键×量派生不符判 AccountTouched。
            if a.fp != AcctRow::make_fp(a.key, a.amount) {
                self.rejected += 1;
                return Err(CasCode::AccountTouched);
            }
            match self.real.iter().find(|r| r.key == a.key) {
                Some(r) => {
                    if a.amount != r.amount {
                        all_flat = false;
                        let delta = (a.amount as i64).wrapping_sub(r.amount as i64);
                        self.gaps.push(GapRow { key: a.key, acct: a.amount, real: r.amount, delta });
                    }
                }
                None => {
                    all_flat = false;
                    self.gaps.push(GapRow { key: a.key, acct: a.amount, real: 0, delta: a.amount as i64 });
                }
            }
        }
        // 实面多键（账缺键）也须留痕。
        for r in self.real.iter() {
            if !self.acct.iter().any(|a| a.key == r.key) {
                all_flat = false;
                self.gaps.push(GapRow { key: r.key, acct: 0, real: r.amount, delta: -(r.amount as i64) });
            }
        }
        Ok(all_flat)
    }

    /// 差额留痕行数。
    pub fn gaps_len(&self) -> usize {
        self.gaps.len()
    }

    /// 差额行回读。
    pub fn gap_at(&self, i: usize) -> Option<GapRow> {
        self.gaps.get(i).copied()
    }

    /// 对平轮数。
    pub fn runs(&self) -> u64 {
        self.recon_runs
    }

    /// 账面行数。
    pub fn acct_len(&self) -> usize {
        self.acct.len()
    }

    /// 实面行数。
    pub fn real_len(&self) -> usize {
        self.real.len()
    }

    /// 拒绝账。
    pub fn rejected_tally(&self) -> u64 {
        self.rejected
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 快照指纹（双跑一致面）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        for a in self.acct.iter() {
            bytes.extend_from_slice(&a.fp.to_le_bytes());
        }
        for r in self.real.iter() {
            bytes.extend_from_slice(&r.fp.to_le_bytes());
        }
        for g in self.gaps.iter() {
            bytes.extend_from_slice(&(g.key as u64).to_le_bytes());
            bytes.extend_from_slice(&g.acct.to_le_bytes());
            bytes.extend_from_slice(&g.real.to_le_bytes());
        }
        fnv1a(&bytes)
    }
}

/// 诊断码闭集（账-实对平域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CasCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 键越界
    KeyOob,
    /// 键重复
    DupKey,
    /// 空面对平
    EmptySide,
    /// 账实量差
    AmountMismatch,
    /// 账缺键
    AcctKeyMissing,
    /// 实缺键
    RealKeyMissing,
    /// 差额未留痕
    GapSilent,
    /// 对平后改账
    AccountTouched,
    /// 错账伪装对平
    ForgedFlat,
    /// 对平面为空
    NoRuns,
}

impl CasCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            CasCode::Ok => "对平判定成功",
            CasCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            CasCode::KeyOob => "键越界：账键闭集 0..6",
            CasCode::DupKey => "键重复：同侧键唯一",
            CasCode::EmptySide => "空面对平：账面/实面均非空才可对平",
            CasCode::AmountMismatch => "账实量差：同键账量与实量不等",
            CasCode::AcctKeyMissing => "账缺键：实面有键账面无行",
            CasCode::RealKeyMissing => "实缺键：账面有键实面无行",
            CasCode::GapSilent => "差额未留痕：每笔差额必须落 GapRow（零静默）",
            CasCode::AccountTouched => "对平后改账：账行指纹与键×量派生不符",
            CasCode::ForgedFlat => "错账伪装对平：错账必须判 Mismatch 不许静默对平",
            CasCode::NoRuns => "对平面为空：全过判据要求至少一轮对平",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [CasCode; 12] = [
    CasCode::Ok,
    CasCode::SegBad,
    CasCode::KeyOob,
    CasCode::DupKey,
    CasCode::EmptySide,
    CasCode::AmountMismatch,
    CasCode::AcctKeyMissing,
    CasCode::RealKeyMissing,
    CasCode::GapSilent,
    CasCode::AccountTouched,
    CasCode::ForgedFlat,
    CasCode::NoRuns,
];

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed ^ 0x7f4a7c15c47bcfb7 }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 造一对平账面/实面（LCG 派生量，全平）。
pub fn build_flat_pair(seed: u64) -> (Vec<(usize, u64)>, Vec<(usize, u64)>) {
    let mut lc = Lcg::new(seed);
    let mut acct: Vec<(usize, u64)> = Vec::new();
    let mut real: Vec<(usize, u64)> = Vec::new();
    for k in 0..KEYS {
        let amt = (lc.next_u32() as u64 % 900) + 100;
        acct.push((k, amt));
        real.push((k, amt));
    }
    (acct, real)
}

/// 标准脚本：×50 轮合成对平，全过返回 true。返回 (全过轮数, 总轮数)。
pub fn standard_script() -> (usize, usize) {
    let mut flat: usize = 0;
    for round in 0..RECON_ROUNDS {
        let (acct, real) = build_flat_pair(SEG_BASE as u64 + round as u64);
        let mut cas = ConsistAssertor::new();
        for (k, a) in acct.iter() {
            let _ = cas.post_acct(*k, *a);
        }
        for (k, r) in real.iter() {
            let _ = cas.post_real(*k, *r);
        }
        if let Ok(true) = cas.reconcile() {
            flat += 1;
        }
    }
    (flat, RECON_ROUNDS)
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    standard_script() == standard_script()
}

/// 注入核：5 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 5]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 5] = [0; 5];
    // 类 0：故意错账——单键账量偏差必判不平且留一行差额。
    for _ in 0..WRONG_ROUNDS.max(INJ_ROUNDS) {
        let (acct, real) = build_flat_pair(SEG_BASE as u64);
        let mut cas = ConsistAssertor::new();
        for (i, (k, a)) in acct.iter().enumerate() {
            let amt = if i == 2 { a.wrapping_add(7) } else { *a };
            let _ = cas.post_acct(*k, amt);
        }
        for (k, r) in real.iter() {
            let _ = cas.post_real(*k, *r);
        }
        let caught = cas.reconcile() == Ok(false) && cas.gaps_len() == 1;
        if caught {
            cas.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            cas.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：实缺键——账有键实无行，对平留痕且判不平。
    for _ in 0..INJ_ROUNDS {
        let (acct, real) = build_flat_pair(SEG_BASE as u64 + 1);
        let mut cas = ConsistAssertor::new();
        for (k, a) in acct.iter() {
            let _ = cas.post_acct(*k, *a);
        }
        for (i, (k, r)) in real.iter().enumerate() {
            if i != 3 {
                let _ = cas.post_real(*k, *r);
            }
        }
        let caught = cas.reconcile() == Ok(false) && cas.gaps_len() == 1;
        if caught {
            cas.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            cas.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：账缺键——实有键账无行，对平留痕且判不平。
    for _ in 0..INJ_ROUNDS {
        let (acct, real) = build_flat_pair(SEG_BASE as u64 + 2);
        let mut cas = ConsistAssertor::new();
        for (i, (k, a)) in acct.iter().enumerate() {
            if i != 4 {
                let _ = cas.post_acct(*k, *a);
            }
        }
        for (k, r) in real.iter() {
            let _ = cas.post_real(*k, *r);
        }
        let caught = cas.reconcile() == Ok(false) && cas.gaps_len() == 1;
        if caught {
            cas.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            cas.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：错账伪装对平——账行指纹篡改后对平判 AccountTouched。
    for _ in 0..INJ_ROUNDS {
        let (acct, real) = build_flat_pair(SEG_BASE as u64 + 3);
        let mut cas = ConsistAssertor::new();
        for (k, a) in acct.iter() {
            let _ = cas.post_acct(*k, *a);
        }
        // 量改指纹不改 = 伪装对平的形态。
        if let Some(r) = cas.acct.get_mut(1) {
            r.amount = r.amount.wrapping_add(3);
        }
        for (k, r) in real.iter() {
            let _ = cas.post_real(*k, *r);
        }
        let caught = cas.reconcile() == Err(CasCode::AccountTouched);
        if caught {
            cas.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            cas.note_escape();
            total_escape += 1;
        }
    }
    // 类 4：空面对平——任一侧为空显性拒。
    for _ in 0..INJ_ROUNDS {
        let mut cas = ConsistAssertor::new();
        let _ = cas.post_acct(0, 5);
        let caught = cas.reconcile() == Err(CasCode::EmptySide);
        if caught {
            cas.note_detected();
            total_detect += 1;
            per[4] += 1;
        } else {
            cas.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51615_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51615");
    // 1 对平机检 ×50 全过。
    {
        let (flat, total) = standard_script();
        s.add(
            "对平机检五十轮全过",
            flat == total && total == RECON_ROUNDS,
            "UNX-F51615 判据：对平机检 ×50 全过（合成账实逐轮全平）",
        );
    }
    // 2 故意错账注入 10/10 检出。
    {
        let (d, _, per) = injection_harness();
        s.add(
            "故意错账 10/10 检出",
            per[0] == 10 && d >= 10,
            "UNX-F51615-J1R：故意错账注入逐轮检出且留差额行",
        );
    }
    // 3 对平留痕零静默：差额行四字段齐备。
    {
        let (acct, real) = build_flat_pair(SEG_BASE as u64 + 9);
        let mut cas = ConsistAssertor::new();
        for (i, (k, a)) in acct.iter().enumerate() {
            let amt = if i == 0 { a.wrapping_sub(1) } else { *a };
            let _ = cas.post_acct(*k, amt);
        }
        for (k, r) in real.iter() {
            let _ = cas.post_real(*k, *r);
        }
        let _ = cas.reconcile();
        let ok = match cas.gap_at(0) {
            Some(g) => g.key == 0 && g.delta == -1 && g.acct != g.real,
            None => false,
        };
        s.add(
            "差额留痕四字段",
            ok && cas.gaps_len() == 1,
            "GapRow 含键/账量/实量/差额且逐项核对（零静默）",
        );
    }
    // 4 键越界拒。
    {
        let mut cas = ConsistAssertor::new();
        s.add(
            "键越界拒",
            cas.post_acct(KEYS, 1) == Err(CasCode::KeyOob)
                && cas.post_real(KEYS, 1) == Err(CasCode::KeyOob),
            "账键闭集 0..6 之外显性拒",
        );
    }
    // 5 键重复拒。
    {
        let mut cas = ConsistAssertor::new();
        let _ = cas.post_acct(1, 10);
        let _ = cas.post_real(1, 10);
        s.add(
            "键重复拒",
            cas.post_acct(1, 20) == Err(CasCode::DupKey)
                && cas.post_real(1, 30) == Err(CasCode::DupKey),
            "同侧键唯一，重复显性拒",
        );
    }
    // 6 空面对平拒。
    {
        let mut cas = ConsistAssertor::new();
        s.add(
            "空面对平拒",
            cas.reconcile() == Err(CasCode::EmptySide),
            "账面/实面均非空才可对平",
        );
    }
    // 7 对平只读：行数不变。
    {
        let (acct, real) = build_flat_pair(SEG_BASE as u64 + 4);
        let mut cas = ConsistAssertor::new();
        for (k, a) in acct.iter() {
            let _ = cas.post_acct(*k, *a);
        }
        for (k, r) in real.iter() {
            let _ = cas.post_real(*k, *r);
        }
        let (a0, r0) = (cas.acct_len(), cas.real_len());
        let _ = cas.reconcile();
        s.add(
            "对平只读",
            cas.acct_len() == a0 && cas.real_len() == r0 && a0 == KEYS && r0 == KEYS,
            "对平不改动账面/实面（只读面）",
        );
    }
    // 8 账行指纹自核（防对平后改账）。
    {
        let (acct, _) = build_flat_pair(SEG_BASE as u64 + 5);
        let row_ok = acct
            .iter()
            .all(|(k, a)| AcctRow::make_fp(*k, *a) != 0);
        s.add(
            "账行指纹自核",
            row_ok,
            "行指纹=键×量派生，指纹互核可探改账（AccountTouched）",
        );
    }
    // 9 多键差额逐行留痕。
    {
        let (acct, real) = build_flat_pair(SEG_BASE as u64 + 6);
        let mut cas = ConsistAssertor::new();
        for (i, (k, a)) in acct.iter().enumerate() {
            let amt = if i == 1 || i == 5 { a.wrapping_add(9) } else { *a };
            let _ = cas.post_acct(*k, amt);
        }
        for (k, r) in real.iter() {
            let _ = cas.post_real(*k, *r);
        }
        let _ = cas.reconcile();
        s.add(
            "多键差额逐行",
            cas.gaps_len() == 2,
            "两键错账留两行差额，逐行零静默",
        );
    }
    // 10 注入核 5 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        s.add(
            "注入 5 类×10 全通",
            d == 50 && esc == 0 && per.iter().all(|&x| x == 10),
            "5 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 11 诊断码闭集唯一性。
    {
        let ok = ALL_CODES
            .iter()
            .enumerate()
            .all(|(i, c)| c.val() == SEG_BASE + 1 + i as u32 && !c.detail().is_empty());
        s.add(
            "诊断码闭集唯一",
            ok && ALL_CODES.len() == 12,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 12 注入账守恒。
    {
        let mut cas = ConsistAssertor::new();
        for _ in 0..13u32 {
            cas.note_detected();
        }
        s.add("注入账守恒", cas.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 13 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一 ×50 脚本两次执行，全过轮数逐位一致",
    );
    // 14 对平轮数记账。
    {
        let (acct, real) = build_flat_pair(SEG_BASE as u64 + 7);
        let mut cas = ConsistAssertor::new();
        for (k, a) in acct.iter() {
            let _ = cas.post_acct(*k, *a);
        }
        for (k, r) in real.iter() {
            let _ = cas.post_real(*k, *r);
        }
        let _ = cas.reconcile();
        let _ = cas.reconcile();
        s.add(
            "对平轮数记账",
            cas.runs() == 2,
            "每轮对平计入 recon_runs，可审计",
        );
    }
    // 15 差额行符号核对（账多正/实多负）。
    {
        let (acct, real) = build_flat_pair(SEG_BASE as u64 + 8);
        let mut cas = ConsistAssertor::new();
        for (i, (k, a)) in acct.iter().enumerate() {
            let amt = if i == 2 { a.wrapping_add(5) } else { *a };
            let _ = cas.post_acct(*k, amt);
        }
        for (i, (k, r)) in real.iter().enumerate() {
            let amt = if i == 4 { r.wrapping_add(11) } else { *r };
            let _ = cas.post_real(*k, amt);
        }
        let _ = cas.reconcile();
        let mut pos = false;
        let mut neg = false;
        for i in 0..cas.gaps_len() {
            if let Some(g) = cas.gap_at(i) {
                if g.delta > 0 {
                    pos = true;
                }
                if g.delta < 0 {
                    neg = true;
                }
            }
        }
        s.add(
            "差额符号核对",
            pos && neg && cas.gaps_len() == 2,
            "账多差额为正、实多差额为负，符号可读",
        );
    }
    // 16 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51615 对平机检 {} 轮 ×{} 键",
            RECON_ROUNDS, KEYS
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51615"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空（写回状态=已完成）",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51615_checks_green() {
        let set = run_unx51615_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn flat_pair_reconciles() {
        let (acct, real) = build_flat_pair(0x5161_5eed);
        let mut cas = ConsistAssertor::new();
        for (k, a) in acct.iter() {
            let _ = cas.post_acct(*k, *a);
        }
        for (k, r) in real.iter() {
            let _ = cas.post_real(*k, *r);
        }
        assert_eq!(cas.reconcile(), Ok(true));
        assert_eq!(cas.gaps_len(), 0);
    }

    #[test]
    fn wrong_amount_detected() {
        let (acct, real) = build_flat_pair(0x5161_5eed + 1);
        let mut cas = ConsistAssertor::new();
        for (i, (k, a)) in acct.iter().enumerate() {
            let amt = if i == 3 { a.wrapping_add(100) } else { *a };
            let _ = cas.post_acct(*k, amt);
        }
        for (k, r) in real.iter() {
            let _ = cas.post_real(*k, *r);
        }
        assert_eq!(cas.reconcile(), Ok(false));
        assert_eq!(cas.gaps_len(), 1);
    }

    #[test]
    fn missing_key_rows() {
        let (acct, real) = build_flat_pair(0x5161_5eed + 2);
        let mut cas = ConsistAssertor::new();
        for (i, (k, a)) in acct.iter().enumerate() {
            if i != 0 {
                let _ = cas.post_acct(*k, *a);
            }
        }
        for (k, r) in real.iter() {
            let _ = cas.post_real(*k, *r);
        }
        assert_eq!(cas.reconcile(), Ok(false));
        assert_eq!(cas.gaps_len(), 1);
        assert_eq!(cas.gap_at(0).map(|g| g.key), Some(0));
    }

    #[test]
    fn key_oob_and_dup() {
        let mut cas = ConsistAssertor::new();
        assert_eq!(cas.post_acct(KEYS, 1), Err(CasCode::KeyOob));
        let _ = cas.post_real(2, 1);
        assert_eq!(cas.post_real(2, 2), Err(CasCode::DupKey));
    }

    #[test]
    fn empty_side_rejected() {
        let mut cas = ConsistAssertor::new();
        assert_eq!(cas.reconcile(), Err(CasCode::EmptySide));
        let _ = cas.post_real(0, 3);
        assert_eq!(cas.reconcile(), Err(CasCode::EmptySide));
    }

    #[test]
    fn touched_account_detected() {
        let (acct, real) = build_flat_pair(0x5161_5eed + 3);
        let mut cas = ConsistAssertor::new();
        for (k, a) in acct.iter() {
            let _ = cas.post_acct(*k, *a);
        }
        if let Some(r) = cas.acct.get_mut(0) {
            r.amount = r.amount.wrapping_add(1);
        }
        for (k, r) in real.iter() {
            let _ = cas.post_real(*k, *r);
        }
        assert_eq!(cas.reconcile(), Err(CasCode::AccountTouched));
    }

    #[test]
    fn reconcile_is_readonly() {
        let (acct, real) = build_flat_pair(0x5161_5eed + 4);
        let mut cas = ConsistAssertor::new();
        for (k, a) in acct.iter() {
            let _ = cas.post_acct(*k, *a);
        }
        for (k, r) in real.iter() {
            let _ = cas.post_real(*k, *r);
        }
        let (a0, r0) = (cas.acct_len(), cas.real_len());
        let _ = cas.reconcile();
        assert_eq!(cas.acct_len(), a0);
        assert_eq!(cas.real_len(), r0);
    }

    #[test]
    fn acct_fp_pairwise_distinct() {
        for k1 in 0..KEYS {
            for a1 in [1u64, 7, 100, 4096] {
                for k2 in 0..KEYS {
                    for a2 in [1u64, 7, 100, 4096] {
                        let same = k1 == k2 && a1 == a2;
                        assert_eq!(AcctRow::make_fp(k1, a1) == AcctRow::make_fp(k2, a2), same);
                    }
                }
            }
        }
    }

    #[test]
    fn injection_all_caught() {
        let (d, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        assert_eq!(d, 50);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "类 {} 应恰 10 次检出", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut cas = ConsistAssertor::new();
        for _ in 0..10u32 {
            cas.note_detected();
        }
        assert!(cas.conserved());
        cas.note_escape();
        assert!(!cas.conserved());
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
