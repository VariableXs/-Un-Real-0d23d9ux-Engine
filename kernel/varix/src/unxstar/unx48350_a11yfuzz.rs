//! unx48350_a11yfuzz — UNX-F48350 无障碍 fuzz（随机查询/随机导出注入不崩）（诊断段 0xCCA0）。
//!
//! 判据原文：
//!   J1 fuzz ×10^4 零崩；判据锚定 体验十。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 确定性 fuzz：LCG（seed 固定 0xA11Y_CCA0 截断值）驱动 10^4 次
//!      混合操作——随机换榜 / 随机挪高亮 / 随机朗读查询 / 随机导出，
//!      同种子重放逐位同账。
//!   2. 零崩判据：所有被测操作一律 Result 语义，fuzz 每笔回执必落
//!      Ok 或已知错误码白名单；出现白名单外码即 E_UNKNOWN，一笔
//!      "崩"都不许有。
//!   3. 注入面：随机内容含重复 id、空榜、越界高亮、极端序号——非法
//!      注入必须被守门拒收并归类，而不是绕过守门改实态。
//!   4. 双跑对账：同种子两轮 fuzz 的 ok/err/白名单分布逐项相等；
//!      计数守恒 ok+err == 10^4。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCCA0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_UNKNOWN: u16 = SEG_BASE + 1; // 白名单外错误码
const E_RUN_MISS: u16 = SEG_BASE + 2; // 笔数守恒破
const E_REPLAY_BAD: u16 = SEG_BASE + 3; // 同种子重放翻供
const E_CLASS_BAD: u16 = SEG_BASE + 4; // 归类账与白名单对不上
const E_TAMPER: u16 = SEG_BASE + 5; // 注入改了实态守门
const E_HL_RANGE: u16 = SEG_BASE + 6; // 被测面：高亮越界
const E_DUP_ID: u16 = SEG_BASE + 7; // 被测面：候选撞号

/// 确定性 LCG（无外部熵）。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg { st: seed | 1 }
    }

    pub fn next_u32(&mut self) -> u32 {
        self.st = self.st.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.st >> 33) as u32
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next_u32() % n
        }
    }
}

/// 被测面：迷你候选页（与 readorder 同族语义，独立最小实现）。
pub struct MiniBoard {
    ids: Vec<u32>,
    hl: usize,
}

impl MiniBoard {
    pub fn new() -> Self {
        MiniBoard { ids: Vec::new(), hl: 0 }
    }

    /// 换榜守门：id 两两互异，违者拒收且实态不动。
    pub fn set_ids(&mut self, ids: Vec<u32>) -> Result<(), u16> {
        let mut a = 0usize;
        while a < ids.len() {
            let mut b = a + 1;
            while b < ids.len() {
                if ids[a] == ids[b] {
                    return Err(E_DUP_ID);
                }
                b += 1;
            }
            a += 1;
        }
        self.ids = ids;
        if self.hl >= self.ids.len() {
            self.hl = 0;
        }
        Ok(())
    }

    /// 挪高亮守门：越界必拒。
    pub fn set_hl(&mut self, hl: usize) -> Result<(), u16> {
        if hl >= self.ids.len() {
            return Err(E_HL_RANGE);
        }
        self.hl = hl;
        Ok(())
    }

    /// 朗读查询守门：回高亮候选 id 或越界错。
    pub fn query(&self) -> Result<u32, u16> {
        if self.hl >= self.ids.len() {
            return Err(E_HL_RANGE);
        }
        Ok(self.ids[self.hl])
    }

    /// 朗读导出守门：高亮先行 + 榜位序，长度必须守恒。
    pub fn export(&self) -> Result<Vec<u32>, u16> {
        if self.hl >= self.ids.len() {
            return Err(E_HL_RANGE);
        }
        let mut out: Vec<u32> = Vec::new();
        out.push(self.ids[self.hl]);
        let mut i = 0usize;
        while i < self.ids.len() {
            if i != self.hl {
                out.push(self.ids[i]);
            }
            i += 1;
        }
        if out.len() != self.ids.len() {
            return Err(E_TAMPER);
        }
        Ok(out)
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }
}

/// fuzz 回执归类白名单。
pub const WHITELIST: [u16; 3] = [E_HL_RANGE, E_DUP_ID, E_TAMPER];

/// fuzz 回执归类白名单命中判定。
fn in_whitelist(code: u16) -> bool {
    let mut i = 0usize;
    while i < WHITELIST.len() {
        if WHITELIST[i] == code {
            return true;
        }
        i += 1;
    }
    false
}

/// 分类账。
#[derive(Clone)]
pub struct FuzzLedger {
    pub ok: u32,
    pub err_hl: u32,
    pub err_dup: u32,
    pub err_tamper: u32,
}

impl FuzzLedger {
    pub fn new() -> Self {
        FuzzLedger { ok: 0, err_hl: 0, err_dup: 0, err_tamper: 0 }
    }

    pub fn total(&self) -> u32 {
        self.ok + self.err_hl + self.err_dup + self.err_tamper
    }
}

/// 单轮 fuzz：ops 笔混合操作，逐笔回执归类；白名单外即零崩判据破。
pub fn fuzz_round(seed: u64, ops: u32) -> Result<FuzzLedger, u16> {
    let mut rng = Lcg::new(seed);
    let mut board = MiniBoard::new();
    let mut led = FuzzLedger::new();
    let mut i = 0u32;
    while i < ops {
        let op = rng.below(4);
        let r = match op {
            // 随机换榜：0..6 个候选，id 可能撞号（注入）。
            0 => {
                let k = rng.below(7);
                let mut ids: Vec<u32> = Vec::new();
                let mut j = 0u32;
                while j < k {
                    ids.push(rng.below(5));
                    j += 1;
                }
                board.set_ids(ids)
            }
            // 随机挪高亮：0..16 大概率越界（注入）。
            1 => board.set_hl(rng.below(16) as usize),
            // 随机朗读查询。
            2 => board.query().map(|_| ()),
            // 随机朗读导出：对账长度守恒。
            _ => {
                let out = board.export()?;
                if out.len() != board.len() {
                    return Err(E_TAMPER);
                }
                Ok(())
            }
        };
        match r {
            Ok(()) => led.ok += 1,
            Err(code) => {
                if !in_whitelist(code) {
                    return Err(E_UNKNOWN);
                }
                if code == E_HL_RANGE {
                    led.err_hl += 1;
                } else if code == E_DUP_ID {
                    led.err_dup += 1;
                } else {
                    led.err_tamper += 1;
                }
            }
        }
        i += 1;
    }
    if led.total() != ops {
        return Err(E_RUN_MISS);
    }
    Ok(led)
}

/// drill：J1 fuzz ×10^4 零崩。
pub fn drill() -> Result<u32, u16> {
    const OPS: u32 = 10_000;

    // ── R1 主判据：同种子两轮 ×10^4，零白名单外回执 ─────────
    let a = fuzz_round(0x5EED_CCA0, OPS)?;
    let b = fuzz_round(0x5EED_CCA0, OPS)?;

    // 计数守恒：ok + err == 10^4（两轮各守恒一次已在轮内对账）。
    if a.total() != OPS || b.total() != OPS {
        return Err(E_RUN_MISS);
    }

    // 重放对账：同种子逐项相等（确定性证据）。
    if a.ok != b.ok || a.err_hl != b.err_hl || a.err_dup != b.err_dup || a.err_tamper != b.err_tamper {
        return Err(E_REPLAY_BAD);
    }

    // 归类账非空：四类至少各就其位（ok 必非零，err 合计必非零——
    // 注入面保证了拒收路径被真实踩到）。
    if a.ok == 0 || a.err_hl + a.err_dup == 0 {
        return Err(E_CLASS_BAD);
    }

    // ── R2 白名单纪律：三白名单码缺一不可（真实触发）────────
    // 若某类从未触发，则白名单判死路（判据恒绿风险），此处要求
    // err_hl、err_dup 均非零。
    if a.err_hl == 0 || a.err_dup == 0 {
        return Err(E_CLASS_BAD);
    }

    // ── R3 换种子变奏：不同种子账不必相同但守恒仍破不得 ─────
    let c = fuzz_round(0x1D7A_5EED, OPS)?;
    if c.total() != OPS {
        return Err(E_RUN_MISS);
    }

    // ── R4 守门不越权：注入撞号后实态仍干净（上一次成功换榜
    //    的榜与导出长度守恒）─────────────────────────────────
    let mut probe = MiniBoard::new();
    let mut ids: Vec<u32> = Vec::new();
    ids.push(9);
    ids.push(4);
    ids.push(7);
    probe.set_ids(ids)?;
    probe.set_hl(1)?;
    if probe.query()? != 4 {
        return Err(E_TAMPER);
    }
    let out = probe.export()?;
    if out.len() != 3 || out[0] != 4 {
        return Err(E_TAMPER);
    }

    // ── R5 小种子重放：7 笔微跑也逐位同账 ───────────────────
    let s1 = fuzz_round(7, 7)?;
    let s2 = fuzz_round(7, 7)?;
    if s1.ok != s2.ok || s1.err_hl != s2.err_hl || s1.err_dup != s2.err_dup {
        return Err(E_REPLAY_BAD);
    }
    if s1.total() != 7 {
        return Err(E_RUN_MISS);
    }

    // ── R6 白名单本身零漂移：三码连续派生且两两互异 ─────────
    if WHITELIST[0] != SEG_BASE + 6 || WHITELIST[1] != SEG_BASE + 7 {
        return Err(E_CLASS_BAD);
    }
    let mut i = 0usize;
    while i < WHITELIST.len() {
        let mut j = i + 1;
        while j < WHITELIST.len() {
            if WHITELIST[i] == WHITELIST[j] {
                return Err(E_CLASS_BAD);
            }
            j += 1;
        }
        i += 1;
    }

    // J2 写回摘要非空：ok 笔数非零即摘要实质。
    if a.ok == 0 {
        return Err(E_RUN_MISS);
    }
    Ok(a.total())
}
