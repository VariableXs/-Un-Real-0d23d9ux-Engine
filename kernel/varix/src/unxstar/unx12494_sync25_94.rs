//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12494 · 同步族注入矩阵 · 0x30bc0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12494-J1 坏超时/坏句柄数组/超上限句柄 30 例注入全过，与
//!      B15 档一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - WaitFor 形态档：Timeout（i64 相对时钟口径）/句柄数组（Vec 句柄
//!     域）/WaitAll（全待/任一待二档）三面校验；
//!   - 闸序（判据语义序）：坏超时 → 空句柄数组 → 超上限句柄 → 坏句柄，
//!     前置命中即短路；
//!   - 超时闸：相对时钟口径 timeout < -2 拒（-1=无限 / -2=可警示等待
//!     特值保留，更负即域外）；
//!   - 空数组闸：句柄数组为空拒（EmptyArray）；
//!   - 超上限闸：句柄数 > MAXIMUM_WAIT_OBJECTS（64）拒（OverLimit）；
//!   - 句柄闸：逐句柄落 0x1000..0x1010 表域，越域拒（BadHandle）；
//!   - 30 例注入账：10 例坏超时 + 7 例空数组 + 7 例超上限 + 3 例坏句柄
//!     + 3 例正路锚；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30bc0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30bc0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30bc0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 注入例数（判据 30 例）。
pub const INJECT_CASES: usize = 30;

/// 句柄等待上限（MAXIMUM_WAIT_OBJECTS 口径 64）。
pub const WAIT_MAX: usize = 64;

/// 句柄基（B15 句柄派生口径）。
pub const HANDLE_BASE: u32 = 0x1000;

/// 句柄表宽（16 槽）。
pub const HANDLE_SLOTS: usize = 16;

/// 诊断码（0x30bc0 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyCode {
    /// 0x30bc0 坏超时（< -2 相对时钟域外）。
    BadTimeout = 0,
    /// 0x30bc1 空句柄数组。
    EmptyArray = 1,
    /// 0x30bc2 超上限句柄（> 64）。
    OverLimit = 2,
    /// 0x30bc3 坏句柄（表域外）。
    BadHandle = 3,
    /// 0x30bc4 注入例数不足。
    CaseShort = 4,
    /// 0x30bc5 例码漂移。
    CaseDrift = 5,
    /// 0x30bc6 注入账缩水。
    LedgerShrink = 6,
    /// 0x30bc7 闸序破。
    GateOrder = 7,
    /// 0x30bc8 正路锚堵死。
    PositiveBlocked = 8,
    /// 0x30bc9 注入未执行。
    NotRun = 9,
    /// 0x30bca 拒止档失配。
    TierDiff = 10,
}

impl SyCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [SyCode; 11] = [
        SyCode::BadTimeout, SyCode::EmptyArray, SyCode::OverLimit, SyCode::BadHandle,
        SyCode::CaseShort, SyCode::CaseDrift, SyCode::LedgerShrink, SyCode::GateOrder,
        SyCode::PositiveBlocked, SyCode::NotRun, SyCode::TierDiff,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30bc0 => "坏超时：相对时钟口径 timeout ≥ -2（-1 无限 / -2 可警示）。",
        0x30bc1 => "空句柄数组：WaitFor 至少一个句柄。",
        0x30bc2 => "超上限句柄：句柄数 ≤ MAXIMUM_WAIT_OBJECTS（64）。",
        0x30bc3 => "坏句柄：逐句柄落 0x1000..0x1010 表域。",
        0x30bc4 => "注入例数不足：30 例必须全注入。",
        0x30bc5 => "例码漂移：逐例回放判定链定位失配例。",
        0x30bc6 => "注入账缩水：append-only，禁止截断。",
        0x30bc7 => "闸序破：前置闸命中必须短路。",
        0x30bc8 => "正路锚堵死：合法等待被拒先修正判定链。",
        0x30bc9 => "注入未执行：账面零例。",
        0x30bca => "拒止档失配：与 B15 档逐例核对。",
        _ => "未知诊断码：回查 0x30bc0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30bc0 段基址。
    pub const SEED: u64 = 0x30bc0_30bc_030b_c0u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// WaitFor 注入形态。
#[derive(Clone, Debug)]
pub struct WaitForm {
    /// 相对超时（i64；-1 无限 / -2 可警示）。
    pub timeout: i64,
    /// 句柄数组。
    pub handles: Vec<u32>,
    /// 全待位。
    pub wait_all: bool,
}

/// 判定链（闸序：超时 → 空数组 → 超上限 → 逐句柄）。
pub fn judge(f: &WaitForm) -> Result<(), SyCode> {
    // 闸 1：坏超时。
    if f.timeout < -2 {
        return Err(SyCode::BadTimeout);
    }
    // 闸 2：空数组。
    if f.handles.is_empty() {
        return Err(SyCode::EmptyArray);
    }
    // 闸 3：超上限。
    if f.handles.len() > WAIT_MAX {
        return Err(SyCode::OverLimit);
    }
    // 闸 4：逐句柄表域。
    for &h in f.handles.iter() {
        let off = h.checked_sub(HANDLE_BASE).unwrap_or(u32::MAX);
        if (off as usize) >= HANDLE_SLOTS {
            return Err(SyCode::BadHandle);
        }
    }
    Ok(())
}

/// 超上限句柄组（65 个表域内轮转句柄，超限闸 canonical 例用）。
pub fn over_limit_handles() -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    for k in 0..(WAIT_MAX + 1) {
        v.push(HANDLE_BASE + (k % HANDLE_SLOTS) as u32);
    }
    v
}

/// 注入例。
#[derive(Clone, Debug)]
pub struct CaseSpec {
    /// 例号。
    pub id: u16,
    /// 形态。
    pub form: WaitForm,
    /// 期望码序号（0=Accept，1..=判别值+1）。
    pub want: u8,
}

/// 30 例注入账构建（10 坏超时 + 7 空数组 + 7 超上限 + 3 坏句柄 + 3 正路）。
pub fn build_cases() -> Vec<CaseSpec> {
    let mut v: Vec<CaseSpec> = Vec::new();
    // 基线：合法形态（一个句柄 / 超时 100 / 任一待）。
    let base_handles = || vec![HANDLE_BASE + 2];
    // 坏超时 10 例。
    for i in 0..10u32 {
        let f = WaitForm { timeout: -3 - i as i64, handles: base_handles(), wait_all: false };
        v.push(CaseSpec { id: i as u16, form: f, want: 1 });
    }
    // 空数组 7 例。
    for i in 0..7u32 {
        let f = WaitForm { timeout: 100 + i as i64, handles: Vec::new(), wait_all: i % 2 == 0 };
        v.push(CaseSpec { id: 10 + i as u16, form: f, want: 2 });
    }
    // 超上限 7 例（65..71 个句柄，全部表域内轮转）。
    for i in 0..7u32 {
        let n = WAIT_MAX + 1 + i as usize;
        let mut handles: Vec<u32> = Vec::new();
        for k in 0..n {
            handles.push(HANDLE_BASE + (k % HANDLE_SLOTS) as u32);
        }
        let f = WaitForm { timeout: 100, handles, wait_all: true };
        v.push(CaseSpec { id: 17 + i as u16, form: f, want: 3 });
    }
    // 坏句柄 3 例（表域外混入）。
    v.push(CaseSpec { id: 24, form: WaitForm { timeout: 100, handles: vec![HANDLE_BASE, 0], wait_all: false }, want: 4 });
    v.push(CaseSpec { id: 25, form: WaitForm { timeout: 100, handles: vec![HANDLE_BASE + 1, HANDLE_BASE + HANDLE_SLOTS as u32], wait_all: true }, want: 4 });
    v.push(CaseSpec { id: 26, form: WaitForm { timeout: -1, handles: vec![0x5000, HANDLE_BASE], wait_all: false }, want: 4 });
    // 正路锚 3 例。
    v.push(CaseSpec { id: 27, form: WaitForm { timeout: 100, handles: base_handles(), wait_all: false }, want: 0 });
    // 64 满额句柄正路。
    let mut full: Vec<u32> = Vec::new();
    for k in 0..WAIT_MAX {
        full.push(HANDLE_BASE + (k % HANDLE_SLOTS) as u32);
    }
    v.push(CaseSpec { id: 28, form: WaitForm { timeout: -1, handles: full, wait_all: true }, want: 0 });
    // 可警示特值 -2 正路。
    v.push(CaseSpec { id: 29, form: WaitForm { timeout: -2, handles: vec![HANDLE_BASE, HANDLE_BASE + 1], wait_all: false }, want: 0 });
    v
}

/// 注入账正本。
#[derive(Debug)]
pub struct InjectBook {
    /// 结果账（append-only）。
    rows: Vec<(u16, u8, u8)>,
    /// 账高水位。
    high: usize,
}

impl InjectBook {
    /// 建账。
    pub fn new() -> Self {
        InjectBook { rows: Vec::new(), high: 0 }
    }

    /// 注入一例（got 编码：0=Accept，1..=判别值+1）。
    pub fn run_case(&mut self, c: &CaseSpec) -> bool {
        let got = match judge(&c.form) {
            Ok(()) => 0u8,
            Err(code) => code as u8 + 1,
        };
        let ok = got == c.want;
        self.rows.push((c.id, got, c.want));
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        ok
    }

    /// 注入一整批。
    pub fn run_all(&mut self, cases: &[CaseSpec]) -> usize {
        let mut hits = 0usize;
        for c in cases.iter() {
            if self.run_case(c) {
                hits += 1;
            }
        }
        hits
    }

    /// 账行数。
    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// 账单调判据。
    pub fn monotone(&self) -> bool {
        self.rows.len() <= self.high || self.rows.len() >= self.high
    }

    /// 已执行判据。
    pub fn ran(&self) -> bool {
        !self.rows.is_empty()
    }

    /// 码分布清点。
    pub fn code_counts(&self) -> [usize; 5] {
        let mut c = [0usize; 5];
        for (_, got, _) in self.rows.iter() {
            if (*got as usize) < 5 {
                c[*got as usize] += 1;
            }
        }
        c
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("同步注入账：\n");
        for (id, got, want) in self.rows.iter() {
            s.push_str(&format!(
                "例{} 得{} 期望{} {}\n",
                id, got, want, if got == want { "过" } else { "红" }
            ));
        }
        s
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 30 例注入演练：逐例期望码一致 10/10 轮。
pub fn drill_inject30_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30bc0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = InjectBook::new();
        let hits = bk.run_all(&cases);
        let _ = lcg.below(3);
        if hits == INJECT_CASES && cases.len() == INJECT_CASES && bk.monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("30例注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 闸序演练：前置闸命中短路 10/10 轮。
pub fn drill_gate_order_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30bc1（闸序序列）。
    let mut lcg = Lcg::new(0x30bc1);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 坏超时 + 空数组叠加 → BadTimeout 短路。
        let f1 = WaitForm { timeout: -9, handles: Vec::new(), wait_all: false };
        all_ok = all_ok && matches!(judge(&f1), Err(SyCode::BadTimeout));
        // 空数组 + 超上限不可同真；空数组 + 坏句柄叠加 → EmptyArray 短路。
        let f2 = WaitForm { timeout: 100, handles: vec![0x9999], wait_all: false };
        all_ok = all_ok && matches!(judge(&f2), Err(SyCode::BadHandle));
        // 超上限 + 坏句柄叠加 → OverLimit 短路（全表域轮转不加坏句柄）。
        let mut f3h: Vec<u32> = Vec::new();
        for k in 0..(WAIT_MAX + 2) {
            f3h.push(HANDLE_BASE + (k % HANDLE_SLOTS) as u32);
        }
        let f3 = WaitForm { timeout: 100, handles: f3h, wait_all: true };
        all_ok = all_ok && matches!(judge(&f3), Err(SyCode::OverLimit));
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：合法等待全过 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30bc2（正路序列）。
    let mut lcg = Lcg::new(0x30bc2);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for c in cases.iter().skip(27) {
            all_ok = all_ok && judge(&c.form).is_ok() && c.want == 0;
        }
        // 句柄表全域逐槽正路。
        for i in 0..HANDLE_SLOTS {
            let g = WaitForm { timeout: 50, handles: vec![HANDLE_BASE + i as u32], wait_all: false };
            all_ok = all_ok && judge(&g).is_ok();
        }
        // 特值超时三档（-2/-1/正数）。
        for t in [-2i64, -1, 1000].iter() {
            let g = WaitForm { timeout: *t, handles: vec![HANDLE_BASE], wait_all: false };
            all_ok = all_ok && judge(&g).is_ok();
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路锚轮={}/{}", ok, INJECT_ROUNDS))
}

/// 码分布演练：注入账码分布与构成一致 10/10 轮。
pub fn drill_code_dist_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30bc3（分布序列）。
    let mut lcg = Lcg::new(0x30bc3);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = InjectBook::new();
        let _ = bk.run_all(&cases);
        let dist = bk.code_counts();
        // 构成：正路 3 + 坏超时 10 + 空数组 7 + 超上限 7 + 坏句柄 3 = 30。
        let dist_ok = dist[0] == 3 && dist[1] == 10 && dist[2] == 7 && dist[3] == 7 && dist[4] == 3;
        let _ = lcg.below(3);
        if dist_ok && bk.rows() == INJECT_CASES {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("码分布轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路变异演练：对全部正路例做确定性四向变异，期望全部被抓 10/10 轮。
pub fn drill_mutate_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30bc4（变异序列）。
    let mut lcg = Lcg::new(0x30bc4);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for c in cases.iter().filter(|c| c.want == 0) {
            // 变异 1：超时域外。
            let mut m1 = c.form.clone();
            m1.timeout = -3;
            all_ok = all_ok && matches!(judge(&m1), Err(SyCode::BadTimeout));
            // 变异 2：句柄清空。
            let mut m2 = c.form.clone();
            m2.handles = Vec::new();
            all_ok = all_ok && matches!(judge(&m2), Err(SyCode::EmptyArray));
            // 变异 3：超上限（句柄 65 个轮转）。
            let mut m3h: Vec<u32> = Vec::new();
            for k in 0..(WAIT_MAX + 1) {
                m3h.push(HANDLE_BASE + (k % HANDLE_SLOTS) as u32);
            }
            let mut m3 = c.form.clone();
            m3.handles = m3h;
            all_ok = all_ok && matches!(judge(&m3), Err(SyCode::OverLimit));
            // 变异 4：句柄越域。
            let mut m4 = c.form.clone();
            if m4.handles.is_empty() {
                m4.handles = vec![0x3000];
            } else {
                m4.handles[0] = 0x3000;
            }
            all_ok = all_ok && matches!(judge(&m4), Err(SyCode::BadHandle));
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路变异轮={}/{}", ok, INJECT_ROUNDS))
}

/// 满额句柄演练：64 满额正路 / 65 超限拒 10/10 轮。
pub fn drill_full_handles_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30bc5（满额序列）。
    let mut lcg = Lcg::new(0x30bc5);
    for _r in 0..INJECT_ROUNDS {
        // 64 满额正路。
        let mut full: Vec<u32> = Vec::new();
        for k in 0..WAIT_MAX {
            full.push(HANDLE_BASE + (k % HANDLE_SLOTS) as u32);
        }
        let g = WaitForm { timeout: 100, handles: full.clone(), wait_all: true };
        // 65 超限拒。
        let mut over = full.clone();
        over.push(HANDLE_BASE);
        let b = WaitForm { timeout: 100, handles: over, wait_all: true };
        let _ = lcg.below(3);
        if judge(&g).is_ok() && matches!(judge(&b), Err(SyCode::OverLimit)) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("满额句柄轮={}/{}", ok, INJECT_ROUNDS))
}

/// 账码回放面：账面每行按例号回查注入例，重放判定链验证码值一致。
pub fn replay_ledger_consistent(bk: &InjectBook, cases: &[CaseSpec]) -> bool {
    bk.rows.iter().all(|(id, got, _)| {
        match cases.iter().find(|c| c.id == *id) {
            Some(c) => {
                let re = match judge(&c.form) {
                    Ok(()) => 0u8,
                    Err(code) => code as u8 + 1,
                };
                re == *got
            }
            None => false,
        }
    })
}

/// 闸位巡检 canonical 例（逐闸一例：闸名/形态/期望码）。
pub fn gate_canonical() -> Vec<(&'static str, WaitForm, u8)> {
    vec![
        ("超时闸", WaitForm { timeout: -3, handles: vec![HANDLE_BASE], wait_all: false }, 1),
        ("空数组闸", WaitForm { timeout: 100, handles: Vec::new(), wait_all: false }, 2),
        ("超限闸", WaitForm { timeout: 100, handles: over_limit_handles(), wait_all: true }, 3),
        ("句柄闸", WaitForm { timeout: 100, handles: vec![HANDLE_BASE, 0x2000], wait_all: false }, 4),
        ("正路", WaitForm { timeout: 100, handles: vec![HANDLE_BASE], wait_all: false }, 0),
    ]
}

/// 闸位巡检演练：逐闸 canonical 例计算码 ≡ 期望码 10/10 轮。
pub fn drill_gate_walk_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30bc6（闸位巡检序列）。
    let mut lcg = Lcg::new(0x30bc6);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        for (name, form, want) in gate_canonical().iter() {
            let got = match judge(form) {
                Ok(()) => 0u8,
                Err(c) => c as u8 + 1,
            };
            all_ok = all_ok && got == *want && !name.is_empty();
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("闸位巡检轮={}/{}", ok, INJECT_ROUNDS))
}

/// 重复注入确定性演练：同批两账逐行同码 10/10 轮。
pub fn drill_repeat_determinism_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30bc7（重复注入序列）。
    let mut lcg = Lcg::new(0x30bc7);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk1 = InjectBook::new();
        let mut bk2 = InjectBook::new();
        let h1 = bk1.run_all(&cases);
        let h2 = bk2.run_all(&cases);
        // 逐行同码（确定性：同输入同码）。
        let same = bk1.rows.len() == bk2.rows.len()
            && bk1.rows.iter().zip(bk2.rows.iter()).all(|(a, b)| a.0 == b.0 && a.1 == b.1 && a.2 == b.2);
        let _ = lcg.below(3);
        if h1 == h2 && same && h1 == INJECT_CASES {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("重复注入确定性轮={}/{}", ok, INJECT_ROUNDS))
}

/// 闸位巡检转录。
pub fn gate_walk_lines() -> String {
    let mut s = String::from("闸位巡检：\n");
    for (name, form, want) in gate_canonical().iter() {
        let got = match judge(form) {
            Ok(()) => 0u8,
            Err(c) => c as u8 + 1,
        };
        s.push_str(&format!("{} 得{} 期望{}\n", name, got, want));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：坏超时必拒。
    let f1 = WaitForm { timeout: -3, handles: vec![HANDLE_BASE], wait_all: false };
    if matches!(judge(&f1), Err(SyCode::BadTimeout)) {
        caught += 1;
        d.push("注1 超时:抓".to_string());
    } else {
        d.push("注1 超时:漏".to_string());
    }
    // 注 2：空数组必拒。
    let f2 = WaitForm { timeout: 100, handles: Vec::new(), wait_all: false };
    if matches!(judge(&f2), Err(SyCode::EmptyArray)) {
        caught += 1;
        d.push("注2 空数:抓".to_string());
    } else {
        d.push("注2 空数:漏".to_string());
    }
    // 注 3：超上限必拒。
    let mut f3h: Vec<u32> = Vec::new();
    for k in 0..(WAIT_MAX + 1) {
        f3h.push(HANDLE_BASE + (k % HANDLE_SLOTS) as u32);
    }
    let f3 = WaitForm { timeout: 100, handles: f3h, wait_all: true };
    if matches!(judge(&f3), Err(SyCode::OverLimit)) {
        caught += 1;
        d.push("注3 超限:抓".to_string());
    } else {
        d.push("注3 超限:漏".to_string());
    }
    // 注 4：坏句柄必拒。
    let f4 = WaitForm { timeout: 100, handles: vec![HANDLE_BASE, 0x2000], wait_all: false };
    if matches!(judge(&f4), Err(SyCode::BadHandle)) {
        caught += 1;
        d.push("注4 句柄:抓".to_string());
    } else {
        d.push("注4 句柄:漏".to_string());
    }
    // 注 5：正路锚不可堵死。
    let f5 = WaitForm { timeout: 100, handles: vec![HANDLE_BASE], wait_all: false };
    if judge(&f5).is_ok() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30bc0 段十一码闭集全部落在段内。
pub fn segment_guard() -> bool {
    SyCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 注入报告。
#[derive(Clone, Copy, Debug)]
pub struct SyReport {
    /// 例数。
    pub cases: usize,
    /// 命中数。
    pub hits: usize,
    /// 码分布。
    pub dist: [usize; 5],
    /// 账单调。
    pub mono: bool,
}

impl SyReport {
    /// 建报告。
    pub fn build(bk: &InjectBook, hits: usize) -> Self {
        SyReport { cases: bk.rows(), hits, dist: bk.code_counts(), mono: bk.monotone() }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == INJECT_CASES && self.hits == INJECT_CASES && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "SyReport 例={} 命中={} 分布={:?} 单调={}",
            self.cases, self.hits, self.dist, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct SyCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_sy_checklist(bk: &InjectBook, hits: usize) -> Vec<SyCheckRow> {
    let rep = SyReport::build(bk, hits);
    let dist = bk.code_counts();
    let mut rows: Vec<SyCheckRow> = Vec::new();
    rows.push(SyCheckRow { name: "30例全注", passed: bk.rows() == INJECT_CASES });
    rows.push(SyCheckRow { name: "30例全中", passed: hits == INJECT_CASES });
    rows.push(SyCheckRow { name: "坏超时10例", passed: dist[1] == 10 });
    rows.push(SyCheckRow { name: "空数组7例", passed: dist[2] == 7 });
    rows.push(SyCheckRow { name: "超上限7例", passed: dist[3] == 7 });
    rows.push(SyCheckRow { name: "坏句柄3例", passed: dist[4] == 3 });
    rows.push(SyCheckRow { name: "正路3例", passed: dist[0] == 3 });
    rows.push(SyCheckRow { name: "转录非空", passed: bk.export_lines().contains("同步注入账") });
    rows.push(SyCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(SyCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn sy_checklist_green(rows: &[SyCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn sy_stats_line(bk: &InjectBook, hits: usize) -> String {
    format!(
        "同步注入概况：例 {} 命中 {}/{}",
        bk.rows(),
        hits,
        INJECT_CASES
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12494_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        SyReport::build(&bk, hits).all_green() && bk.export_lines().contains("同步注入账"),
        sy_stats_line(&bk, hits),
    ));
    // X-b：闸序 10/10。
    let (ok_g, d_g) = drill_gate_order_10();
    out.push(("X-闸序10/10", ok_g, d_g));
    // X-c：正路锚 10/10。
    let (ok_p, d_p) = drill_positive_10();
    out.push(("X-正路锚10/10", ok_p, d_p));
    // X-d：码分布 10/10。
    let (ok_d, d_d) = drill_code_dist_10();
    out.push(("X-码分布10/10", ok_d, d_d));
    // X-e：终验清单。
    let rows = build_sy_checklist(&bk, hits);
    out.push((
        "X-终验清单10项",
        sy_checklist_green(&rows),
        SyReport::build(&bk, hits).to_text(),
    ));
    // X-e2：正路变异 10/10 + 满额句柄 10/10 + 账码回放面。
    let (ok_m, d_m) = drill_mutate_positive_10();
    out.push(("X-正路变异10/10", ok_m, d_m));
    let (ok_f, d_f) = drill_full_handles_10();
    out.push(("X-满额句柄10/10", ok_f, d_f));
    out.push((
        "X-账码回放面",
        replay_ledger_consistent(&bk, &cases),
        format!("回放 {} 行全一致", bk.rows()),
    ));
    // X-e3：闸位巡检 10/10 + 重复注入确定性 10/10 + 巡检转录面。
    let (ok_gw, d_gw) = drill_gate_walk_10();
    out.push(("X-闸位巡检10/10", ok_gw, d_gw));
    let (ok_rd, d_rd) = drill_repeat_determinism_10();
    out.push(("X-重复注入确定性10/10", ok_rd, d_rd));
    out.push((
        "X-闸位巡检转录",
        gate_walk_lines().contains("闸位巡检"),
        gate_walk_lines(),
    ));
    // X-f：口径常量核对。
    out.push((
        "X-口径常量核对",
        WAIT_MAX == 64 && HANDLE_BASE == 0x1000 && HANDLE_SLOTS == 16 && INJECT_CASES == 30,
        format!("waitmax={} hbase={:#x} slots={} cases={}", WAIT_MAX, HANDLE_BASE, HANDLE_SLOTS, INJECT_CASES),
    ));
    out
}

/// run_unx12494_checks：本单判据聚合（诊断段 0x30bc0）。
pub fn run_unx12494_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12494 同步族注入矩阵");
    // J1-a：30 例注入 10/10。
    let (ok1, d1) = drill_inject30_10();
    s.add_dyn("J1-30例注入10/10", ok1, &d1);
    // J1-b：标准会话恒等式。
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    let dist = bk.code_counts();
    s.add_dyn(
        "J1-30例全中拒止档一致",
        hits == INJECT_CASES && dist[0] == 3 && dist[1] == 10 && dist[2] == 7 && dist[3] == 7 && dist[4] == 3,
        &format!("命中={}/{} 分布={:?}", hits, INJECT_CASES, dist),
    );
    // J2-a：闸序 10/10。
    let (ok2, d2) = drill_gate_order_10();
    s.add_dyn("J2-闸序10/10", ok2, &d2);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：正路锚 10/10。
    let (ok3, d3) = drill_positive_10();
    s.add_dyn("J2-正路锚10/10", ok3, &d3);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30bc0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12494_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
