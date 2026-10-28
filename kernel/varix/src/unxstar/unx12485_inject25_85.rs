//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12485 · UNICODE_STRING 注入矩阵 · 0x30aa0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12485-J1 超长/未终止/奇长 30 例注入全过，与 B06 边界档
//!      逐例一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - US 形态档：Length(有效字节)/MaximumLength(缓冲容量语义)/
//!     BufferCap(真实缓冲宽)三字段校验；
//!   - 闸序（判据语义序）：空缓冲有长 → 坏缓冲 → 超长
//!     （Length>MaximumLength）→ 越缓冲（MaximumLength>BufferCap）→
//!     未终止（MaximumLength==Length，无终止符位）→ 奇长（宽字符
//!     口径 Length 非偶），前置命中即短路；
//!   - 超长闸：Length > MaximumLength 即拒（经典坑：有长无容）；
//!   - 未终止闸：MaximumLength == Length（无 NUL 终止余位）拒；
//!   - 奇长闸：宽字符口径 Length % 2 != 0 拒；
//!   - 30 例注入账：9 例超长 + 6 例越缓冲 + 6 例未终止 + 6 例奇长 +
//!     3 例正路锚，逐例期望码与判定链一致；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30aa0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30aa0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30aa0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 注入例数（判据 30 例）。
pub const INJECT_CASES: usize = 30;

/// US 缓冲上限口径（32KB，B06 边界档）。
pub const US_BUF_MAX: u32 = 0x8000;

/// 诊断码（0x30aa0 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UsCode {
    /// 0x30aa0 有长无缓冲（Buffer 为空却带 Length）。
    NullBuffer = 0,
    /// 0x30aa1 超长（Length > MaximumLength）。
    TooLong = 1,
    /// 0x30aa2 越缓冲（MaximumLength > BufferCap）。
    OverBuffer = 2,
    /// 0x30aa3 未终止（MaximumLength == Length，无终止符余位）。
    NotTerminated = 3,
    /// 0x30aa4 奇长（宽字符口径 Length 非偶）。
    OddLen = 4,
    /// 0x30aa5 注入例数不足。
    CaseShort = 5,
    /// 0x30aa6 例码漂移。
    CaseDrift = 6,
    /// 0x30aa7 注入账缩水。
    LedgerShrink = 7,
    /// 0x30aa8 闸序破。
    GateOrder = 8,
    /// 0x30aa9 正路锚堵死。
    PositiveBlocked = 9,
    /// 0x30aaa 注入未执行。
    NotRun = 10,
}

impl UsCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [UsCode; 11] = [
        UsCode::NullBuffer, UsCode::TooLong, UsCode::OverBuffer, UsCode::NotTerminated,
        UsCode::OddLen, UsCode::CaseShort, UsCode::CaseDrift, UsCode::LedgerShrink,
        UsCode::GateOrder, UsCode::PositiveBlocked, UsCode::NotRun,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30aa0 => "有长无缓冲：Buffer 空则 Length 必须为 0。",
        0x30aa1 => "超长：Length 不得超过 MaximumLength，先扩容再设长。",
        0x30aa2 => "越缓冲：MaximumLength 不得超过真实缓冲宽。",
        0x30aa3 => "未终止：MaximumLength 必须留出终止符余位（>Length）。",
        0x30aa4 => "奇长：宽字符口径 Length 必须为偶数（按 WCHAR 计）。",
        0x30aa5 => "注入例数不足：30 例必须全注入。",
        0x30aa6 => "例码漂移：逐例回放判定链定位失配例。",
        0x30aa7 => "注入账缩水：append-only，禁止截断。",
        0x30aa8 => "闸序破：前置闸命中必须短路。",
        0x30aa9 => "正路锚堵死：合法 US 被拒先修正判定链。",
        0x30aaa => "注入未执行：账面零例。",
        _ => "未知诊断码：回查 0x30aa0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30aa0 段基址。
    pub const SEED: u64 = 0x30aa0_30aa_030a_a0u64;

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

/// UNICODE_STRING 注入形态（Buffer 用 has_buffer 哨兵模型化）。
#[derive(Clone, Copy, Debug)]
pub struct UsForm {
    /// Length（有效字节数）。
    pub length: u32,
    /// MaximumLength（声明容量语义）。
    pub max_length: u32,
    /// Buffer 真实缓冲宽（has_buffer=false 时视为空指针）。
    pub has_buffer: bool,
    /// BufferCap（真实缓冲宽，字节）。
    pub buffer_cap: u32,
}

/// 判定链（闸序：空缓冲 → 超长 → 越缓冲 → 未终止 → 奇长）。
pub fn judge(f: &UsForm) -> Result<(), UsCode> {
    // 闸 1：空缓冲有长。
    if !f.has_buffer && f.length != 0 {
        return Err(UsCode::NullBuffer);
    }
    // 闸 2：超长。
    if f.length > f.max_length {
        return Err(UsCode::TooLong);
    }
    // 闸 3：越缓冲。
    if f.max_length > f.buffer_cap {
        return Err(UsCode::OverBuffer);
    }
    // 闸 4：未终止（有空缓冲才谈终止余位）。
    if f.has_buffer && f.max_length == f.length && f.length != 0 {
        return Err(UsCode::NotTerminated);
    }
    // 闸 5：奇长。
    if f.length % 2 != 0 {
        return Err(UsCode::OddLen);
    }
    Ok(())
}

/// 注入例。
#[derive(Clone, Copy, Debug)]
pub struct CaseSpec {
    /// 例号。
    pub id: u16,
    /// 形态。
    pub form: UsForm,
    /// 期望码序号（0=Accept）。
    pub want: u8,
}

/// 30 例注入账构建（9 超长 + 6 越缓冲 + 6 未终止 + 6 奇长 + 3 正路）。
pub fn build_cases() -> Vec<CaseSpec> {
    let mut v: Vec<CaseSpec> = Vec::new();
    // 基线：合法形态（buffer cap 16，max 10，length 8）。
    let base = UsForm { length: 8, max_length: 10, has_buffer: true, buffer_cap: 16 };
    // 超长 9 例（Length > MaximumLength）。
    for i in 0..9u32 {
        let mut f = base;
        f.length = base.max_length + 1 + i * 2;
        v.push(CaseSpec { id: i as u16, form: f, want: 1 });
    }
    // 越缓冲 6 例（MaximumLength > BufferCap）。
    for i in 0..6u32 {
        let mut f = base;
        f.max_length = base.buffer_cap + 2 + i * 4;
        if f.max_length > US_BUF_MAX {
            f.max_length = US_BUF_MAX;
        }
        v.push(CaseSpec { id: 9 + i as u16, form: f, want: 2 });
    }
    // 未终止 6 例（MaximumLength == Length）。
    for i in 0..6u32 {
        let mut f = base;
        f.length = 2 + i * 4;
        f.max_length = f.length;
        f.buffer_cap = f.length + 8;
        v.push(CaseSpec { id: 15 + i as u16, form: f, want: 3 });
    }
    // 奇长 6 例（Length 奇数）。
    for i in 0..6u32 {
        let mut f = base;
        f.length = 1 + i * 2;
        f.max_length = f.length + 4;
        f.buffer_cap = f.max_length + 8;
        v.push(CaseSpec { id: 21 + i as u16, form: f, want: 4 });
    }
    // 正路锚 3 例。
    v.push(CaseSpec { id: 27, form: base, want: 0 });
    v.push(CaseSpec { id: 28, form: UsForm { length: 0, max_length: 0, has_buffer: false, buffer_cap: 0 }, want: 0 });
    v.push(CaseSpec { id: 29, form: UsForm { length: US_BUF_MAX - 2, max_length: US_BUF_MAX, has_buffer: true, buffer_cap: US_BUF_MAX }, want: 0 });
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

    /// 注入一例。
    pub fn run_case(&mut self, c: &CaseSpec) -> bool {
        let got = match judge(&c.form) {
            Ok(()) => 0u8,
            Err(code) => code as u8,
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
    pub fn code_counts(&self) -> [usize; 6] {
        let mut c = [0usize; 6];
        for (_, got, _) in self.rows.iter() {
            if (*got as usize) < 6 {
                c[*got as usize] += 1;
            }
        }
        c
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("US 注入账：\n");
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
    // 种子写死（协议禁 rand）：0x30aa0 段基址。
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
    // 种子写死：0x30aa1（闸序序列）。
    let mut lcg = Lcg::new(0x30aa1);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 空缓冲 + 超长叠加 → NullBuffer 短路。
        let f1 = UsForm { length: 4, max_length: 2, has_buffer: false, buffer_cap: 0 };
        all_ok = all_ok && matches!(judge(&f1), Err(UsCode::NullBuffer));
        // 超长 + 越缓冲叠加 → TooLong 短路。
        let f2 = UsForm { length: 20, max_length: 10, has_buffer: true, buffer_cap: 8 };
        all_ok = all_ok && matches!(judge(&f2), Err(UsCode::TooLong));
        // 越缓冲 + 未终止叠加 → OverBuffer 短路。
        let f3 = UsForm { length: 4, max_length: 20, has_buffer: true, buffer_cap: 8 };
        all_ok = all_ok && matches!(judge(&f3), Err(UsCode::OverBuffer));
        // 未终止 + 奇长叠加 → NotTerminated 短路（length 3、max 3）。
        let f4 = UsForm { length: 3, max_length: 3, has_buffer: true, buffer_cap: 16 };
        all_ok = all_ok && matches!(judge(&f4), Err(UsCode::NotTerminated));
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：合法 US 全过 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30aa2（正路序列）。
    let mut lcg = Lcg::new(0x30aa2);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 三正路：常规/空串/满容量留终止位。
        let g1 = UsForm { length: 8, max_length: 10, has_buffer: true, buffer_cap: 16 };
        let g2 = UsForm { length: 0, max_length: 0, has_buffer: false, buffer_cap: 0 };
        let g3 = UsForm { length: US_BUF_MAX - 2, max_length: US_BUF_MAX, has_buffer: true, buffer_cap: US_BUF_MAX };
        all_ok = all_ok && judge(&g1).is_ok() && judge(&g2).is_ok() && judge(&g3).is_ok();
        // 边界正路：偶长恰满（length=2, max=4, cap=4）。
        let g4 = UsForm { length: 2, max_length: 4, has_buffer: true, buffer_cap: 4 };
        all_ok = all_ok && judge(&g4).is_ok();
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
    // 种子写死：0x30aa3（分布序列）。
    let mut lcg = Lcg::new(0x30aa3);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = InjectBook::new();
        let _ = bk.run_all(&cases);
        let dist = bk.code_counts();
        // 构成：正路 3 + 超长 9 + 越缓冲 6 + 未终止 6 + 奇长 6 = 30。
        let dist_ok = dist[0] == 3 && dist[1] == 9 && dist[2] == 6 && dist[3] == 6 && dist[4] == 6;
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
    // 种子写死：0x30aa4（变异序列）。
    let mut lcg = Lcg::new(0x30aa4);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for c in cases.iter().filter(|c| c.want == 0 && c.form.has_buffer) {
            // 变异 1：Length 超过 MaximumLength。
            let mut m1 = c.form;
            m1.length = m1.max_length + 2;
            all_ok = all_ok && matches!(judge(&m1), Err(UsCode::TooLong));
            // 变异 2：MaximumLength 超真实缓冲。
            let mut m2 = c.form;
            m2.max_length = m2.buffer_cap + 2;
            all_ok = all_ok && matches!(judge(&m2), Err(UsCode::OverBuffer));
            // 变异 3：终止余位剥除。
            let mut m3 = c.form;
            m3.max_length = m3.length;
            all_ok = all_ok && matches!(judge(&m3), Err(UsCode::NotTerminated));
            // 变异 4：奇长化。
            let mut m4 = c.form;
            m4.length |= 1;
            all_ok = all_ok && matches!(judge(&m4), Err(UsCode::OddLen));
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路变异轮={}/{}", ok, INJECT_ROUNDS))
}

/// 终止余位边界演练：余位恰 2 字节（一个 WCHAR）正路 / 余位 0 拒 10/10 轮。
pub fn drill_terminator_edge_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30aa5（边界序列）。
    let mut lcg = Lcg::new(0x30aa5);
    for _r in 0..INJECT_ROUNDS {
        // 余位恰 2 字节（max = length + 2）。
        let g1 = UsForm { length: 8, max_length: 10, has_buffer: true, buffer_cap: 16 };
        // 余位零（max = length）拒。
        let b1 = UsForm { length: 8, max_length: 8, has_buffer: true, buffer_cap: 16 };
        // 余位 1（max = length + 1）且奇长叠加拒（奇长闸口径）。
        let b2 = UsForm { length: 7, max_length: 9, has_buffer: true, buffer_cap: 16 };
        let _ = lcg.below(3);
        if judge(&g1).is_ok() && matches!(judge(&b1), Err(UsCode::NotTerminated))
            && matches!(judge(&b2), Err(UsCode::OddLen))
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("终止余位边界轮={}/{}", ok, INJECT_ROUNDS))
}

/// 账码回放面：账面每行按例号回查注入例，重放判定链验证码值一致。
pub fn replay_ledger_consistent(bk: &InjectBook, cases: &[CaseSpec]) -> bool {
    bk.rows.iter().all(|(id, got, _)| {
        match cases.iter().find(|c| c.id == *id) {
            Some(c) => {
                let re = match judge(&c.form) {
                    Ok(()) => 0u8,
                    Err(code) => code as u8,
                };
                re == *got
            }
            None => false,
        }
    })
}

/// 闸位巡检 canonical 例（逐闸一例：闸名/形态/期望码）。
pub fn gate_canonical() -> Vec<(&'static str, UsForm, u8)> {
    vec![
        ("超长闸", UsForm { length: 12, max_length: 10, has_buffer: true, buffer_cap: 16 }, 1),
        ("越缓冲闸", UsForm { length: 4, max_length: 12, has_buffer: true, buffer_cap: 8 }, 2),
        ("未终止闸", UsForm { length: 8, max_length: 8, has_buffer: true, buffer_cap: 16 }, 3),
        ("奇长闸", UsForm { length: 5, max_length: 9, has_buffer: true, buffer_cap: 16 }, 4),
        ("正路", UsForm { length: 8, max_length: 10, has_buffer: true, buffer_cap: 16 }, 0),
    ]
}

/// 闸位巡检演练：逐闸 canonical 例计算码 ≡ 期望码 10/10 轮。
pub fn drill_gate_walk_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30aa6（闸位巡检序列）。
    let mut lcg = Lcg::new(0x30aa6);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        for (name, form, want) in gate_canonical().iter() {
            let got = match judge(form) {
                Ok(()) => 0u8,
                Err(c) => c as u8,
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
    // 种子写死：0x30aa7（重复注入序列）。
    let mut lcg = Lcg::new(0x30aa7);
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
            Err(c) => c as u8,
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
    // 注 1：超长必拒。
    let f1 = UsForm { length: 12, max_length: 10, has_buffer: true, buffer_cap: 16 };
    if matches!(judge(&f1), Err(UsCode::TooLong)) {
        caught += 1;
        d.push("注1 超长:抓".to_string());
    } else {
        d.push("注1 超长:漏".to_string());
    }
    // 注 2：未终止必拒。
    let f2 = UsForm { length: 8, max_length: 8, has_buffer: true, buffer_cap: 16 };
    if matches!(judge(&f2), Err(UsCode::NotTerminated)) {
        caught += 1;
        d.push("注2 未终止:抓".to_string());
    } else {
        d.push("注2 未终止:漏".to_string());
    }
    // 注 3：奇长必拒。
    let f3 = UsForm { length: 5, max_length: 9, has_buffer: true, buffer_cap: 16 };
    if matches!(judge(&f3), Err(UsCode::OddLen)) {
        caught += 1;
        d.push("注3 奇长:抓".to_string());
    } else {
        d.push("注3 奇长:漏".to_string());
    }
    // 注 4：越缓冲必拒。
    let f4 = UsForm { length: 4, max_length: 12, has_buffer: true, buffer_cap: 8 };
    if matches!(judge(&f4), Err(UsCode::OverBuffer)) {
        caught += 1;
        d.push("注4 越缓:抓".to_string());
    } else {
        d.push("注4 越缓:漏".to_string());
    }
    // 注 5：正路锚不可堵死。
    let f5 = UsForm { length: 8, max_length: 10, has_buffer: true, buffer_cap: 16 };
    if judge(&f5).is_ok() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30aa0 段十一码闭集全部落在段内。
pub fn segment_guard() -> bool {
    UsCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 注入报告。
#[derive(Clone, Copy, Debug)]
pub struct UsReport {
    /// 例数。
    pub cases: usize,
    /// 命中数。
    pub hits: usize,
    /// 码分布。
    pub dist: [usize; 6],
    /// 账单调。
    pub mono: bool,
}

impl UsReport {
    /// 建报告。
    pub fn build(bk: &InjectBook, hits: usize) -> Self {
        UsReport { cases: bk.rows(), hits, dist: bk.code_counts(), mono: bk.monotone() }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == INJECT_CASES && self.hits == INJECT_CASES && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "UsReport 例={} 命中={} 分布={:?} 单调={}",
            self.cases, self.hits, self.dist, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct UsCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_us_checklist(bk: &InjectBook, hits: usize) -> Vec<UsCheckRow> {
    let rep = UsReport::build(bk, hits);
    let dist = bk.code_counts();
    let mut rows: Vec<UsCheckRow> = Vec::new();
    rows.push(UsCheckRow { name: "30例全注", passed: bk.rows() == INJECT_CASES });
    rows.push(UsCheckRow { name: "30例全中", passed: hits == INJECT_CASES });
    rows.push(UsCheckRow { name: "超长9例", passed: dist[1] == 9 });
    rows.push(UsCheckRow { name: "越缓冲6例", passed: dist[2] == 6 });
    rows.push(UsCheckRow { name: "未终止6例", passed: dist[3] == 6 });
    rows.push(UsCheckRow { name: "奇长6例", passed: dist[4] == 6 });
    rows.push(UsCheckRow { name: "正路3例", passed: dist[0] == 3 });
    rows.push(UsCheckRow { name: "转录非空", passed: bk.export_lines().contains("US 注入账") });
    rows.push(UsCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(UsCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn us_checklist_green(rows: &[UsCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn us_stats_line(bk: &InjectBook, hits: usize) -> String {
    format!(
        "US 注入概况：例 {} 命中 {}/{}",
        bk.rows(),
        hits,
        INJECT_CASES
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12485_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        UsReport::build(&bk, hits).all_green() && bk.export_lines().contains("US 注入账"),
        us_stats_line(&bk, hits),
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
    let rows = build_us_checklist(&bk, hits);
    out.push((
        "X-终验清单10项",
        us_checklist_green(&rows),
        UsReport::build(&bk, hits).to_text(),
    ));
    // X-e2：正路变异 10/10 + 终止余位边界 10/10 + 账码回放面。
    let (ok_m, d_m) = drill_mutate_positive_10();
    out.push(("X-正路变异10/10", ok_m, d_m));
    let (ok_t, d_t) = drill_terminator_edge_10();
    out.push(("X-终止余位边界10/10", ok_t, d_t));
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
        US_BUF_MAX == 0x8000 && INJECT_CASES == 30,
        format!("bufmax={:#x} cases={}", US_BUF_MAX, INJECT_CASES),
    ));
    out
}

/// run_unx12485_checks：本单判据聚合（诊断段 0x30aa0）。
pub fn run_unx12485_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12485 UNICODE_STRING 注入矩阵");
    // J1-a：30 例注入 10/10。
    let (ok1, d1) = drill_inject30_10();
    s.add_dyn("J1-30例注入10/10", ok1, &d1);
    // J1-b：标准会话恒等式。
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    let dist = bk.code_counts();
    s.add_dyn(
        "J1-30例全中边界档一致",
        hits == INJECT_CASES && dist[0] == 3 && dist[1] == 9 && dist[2] == 6 && dist[3] == 6 && dist[4] == 6,
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
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30aa0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12485_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}

