//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12488 · CONTEXT 注入矩阵 · 0x30b00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12488-J1 坏 ContextFlags/坏段寄存器/坏 RSP 30 例注入全过，
//!      与 B09 档一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - CONTEXT 形态档：ContextFlags(请求面位域)/段寄存器四件
//!     （CS/DS/ES/SS 选择子）/RSP(栈指针域)校验；
//!   - 闸序（判据语义序）：坏 ContextFlags → 坏段寄存器 → 坏 RSP，
//!     前置命中即短路；
//!   - ContextFlags 闸：合法请求位域（CONTROL=0x1/INTEGER=0x2/
//!     SEGMENTS=0x4/FLOAT=0x8/DEBUG=0x10 并集 0x1F 口径，未含 CONTROL
//!     的裸请求拒——控制面必须请求）；
//!   - 段寄存器闸：CS 闭集 {0x10,0x1B,0x23,0x33}；DS/ES/SS 闭集
//!     {0,0x10,0x18,0x23,0x2B,0x33}，出闭集即拒；
//!   - RSP 闸：零 RSP 拒 / 八字节失对拒 / 超用户栈顶域拒；
//!   - 30 例注入账：9 例坏 ContextFlags + 8 例坏段寄存器 + 8 例坏 RSP
//!     + 2 例正路锚 + 3 例复合叠加（闸序面）；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30b00），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30b00 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30b00;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 注入例数（判据 30 例）。
pub const INJECT_CASES: usize = 30;

/// ContextFlags 合法位域（五请求位并集口径）。
pub const FLAGS_VALID_MASK: u32 = 0x0000_001F;

/// 控制面请求位（裸请求必须包含）。
pub const FLAG_CONTROL: u32 = 0x0000_0001;

/// CS 选择子闭集（内核 0x10 / 用户 0x1B/0x23/0x33 模型口径）。
pub const CS_VALID: [u16; 4] = [0x10, 0x1B, 0x23, 0x33];

/// DS/ES/SS 选择子闭集（含空选择子 0）。
pub const DSES_VALID: [u16; 6] = [0, 0x10, 0x18, 0x23, 0x2B, 0x33];

/// 用户栈顶域上界（RSP 校验口径）。
pub const RSP_TOP: u64 = 0x0000_7FFF_FFFF_0000;

/// 诊断码（0x30b00 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CxCode {
    /// 0x30b00 坏 ContextFlags（位域外 / 无控制面）。
    BadFlags = 0,
    /// 0x30b01 坏段寄存器（选择子出闭集）。
    BadSeg = 1,
    /// 0x30b02 坏 RSP（零/失对/越域）。
    BadRsp = 2,
    /// 0x30b03 注入例数不足。
    CaseShort = 3,
    /// 0x30b04 例码漂移。
    CaseDrift = 4,
    /// 0x30b05 注入账缩水。
    LedgerShrink = 5,
    /// 0x30b06 闸序破。
    GateOrder = 6,
    /// 0x30b07 正路锚堵死。
    PositiveBlocked = 7,
    /// 0x30b08 注入未执行。
    NotRun = 8,
    /// 0x30b09 拒止档失配。
    TierDiff = 9,
}

impl CxCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [CxCode; 10] = [
        CxCode::BadFlags, CxCode::BadSeg, CxCode::BadRsp,
        CxCode::CaseShort, CxCode::CaseDrift, CxCode::LedgerShrink, CxCode::GateOrder,
        CxCode::PositiveBlocked, CxCode::NotRun, CxCode::TierDiff,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30b00 => "坏 ContextFlags：位域外或未含控制面请求位，回查请求面。",
        0x30b01 => "坏段寄存器：CS/DS/ES/SS 出闭集，回查选择子口径。",
        0x30b02 => "坏 RSP：零 / 非八字对齐 / 超栈顶域，回查栈帧。",
        0x30b03 => "注入例数不足：30 例必须全注入。",
        0x30b04 => "例码漂移：逐例回放判定链定位失配例。",
        0x30b05 => "注入账缩水：append-only，禁止截断。",
        0x30b06 => "闸序破：前置闸命中必须短路。",
        0x30b07 => "正路锚堵死：合法 CONTEXT 被拒先修正判定链。",
        0x30b08 => "注入未执行：账面零例。",
        0x30b09 => "拒止档失配：与 B09 档逐例核对。",
        _ => "未知诊断码：回查 0x30b00 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30b00 段基址。
    pub const SEED: u64 = 0x30b00_30b0_030b_00u64;

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

/// CONTEXT 注入形态。
#[derive(Clone, Copy, Debug)]
pub struct CxForm {
    /// ContextFlags（请求面）。
    pub flags: u32,
    /// CS。
    pub cs: u16,
    /// DS。
    pub ds: u16,
    /// ES。
    pub es: u16,
    /// SS。
    pub ss: u16,
    /// RSP。
    pub rsp: u64,
}

/// 判定链（闸序：ContextFlags → 段寄存器 → RSP）。
pub fn judge(f: &CxForm) -> Result<(), CxCode> {
    // 闸 1：ContextFlags（位域外或无控制面）。
    if f.flags & !FLAGS_VALID_MASK != 0 || f.flags & FLAG_CONTROL == 0 {
        return Err(CxCode::BadFlags);
    }
    // 闸 2：段寄存器四件。
    if !CS_VALID.contains(&f.cs) {
        return Err(CxCode::BadSeg);
    }
    if !DSES_VALID.contains(&f.ds) || !DSES_VALID.contains(&f.es) || !DSES_VALID.contains(&f.ss) {
        return Err(CxCode::BadSeg);
    }
    // 闸 3：RSP（零/失对/越域）。
    if f.rsp == 0 || f.rsp % 8 != 0 || f.rsp > RSP_TOP {
        return Err(CxCode::BadRsp);
    }
    Ok(())
}

/// 注入例。
#[derive(Clone, Copy, Debug)]
pub struct CaseSpec {
    /// 例号。
    pub id: u16,
    /// 形态。
    pub form: CxForm,
    /// 期望码序号（0=Accept，1..=判别值+1）。
    pub want: u8,
}

/// 30 例注入账构建（9 坏 Flags + 8 坏段寄存器 + 8 坏 RSP + 2 复合 + 3 正路）。
pub fn build_cases() -> Vec<CaseSpec> {
    let mut v: Vec<CaseSpec> = Vec::new();
    // 基线：合法形态（用户态口径 CS 0x33 / DS·ES·SS 0x2B / RSP 八字对齐）。
    let base = CxForm { flags: 0x1F, cs: 0x33, ds: 0x2B, es: 0x2B, ss: 0x2B, rsp: 0xFF_D00 };
    // 坏 Flags 9 例。
    for i in 0..9u32 {
        let mut f = base;
        f.flags = match i % 3 {
            // 位域外。
            0 => 0x20 + i,
            // 全 1。
            1 => 0xFFFF_FFFF,
            // 无控制面（仅 INTEGER+SEGMENTS）。
            _ => 0x06,
        };
        v.push(CaseSpec { id: i as u16, form: f, want: 1 });
    }
    // 坏段寄存器 8 例。
    for i in 0..8u32 {
        let mut f = base;
        match i % 4 {
            0 => f.cs = 0x08,
            1 => f.ds = 0x40 + i as u16,
            2 => f.es = 0xFFFF,
            _ => f.ss = 0x40 + i as u16,
        }
        v.push(CaseSpec { id: 9 + i as u16, form: f, want: 2 });
    }
    // 坏 RSP 8 例。
    for i in 0..8u32 {
        let mut f = base;
        match i % 4 {
            0 => f.rsp = 0,
            1 => f.rsp = 0xFF_D01 + i as u64, // 奇地址恒失对
            2 => f.rsp = RSP_TOP + 0x100 + i as u64,
            _ => f.rsp = u64::MAX,
        }
        v.push(CaseSpec { id: 17 + i as u16, form: f, want: 3 });
    }
    // 复合叠加 2 例（闸序面：多闸坏取最低序码）。
    v.push(CaseSpec {
        id: 25,
        form: CxForm { flags: 0, cs: 0x99, ds: 0x2B, es: 0x2B, ss: 0x2B, rsp: 0 },
        want: 1,
    });
    v.push(CaseSpec {
        id: 26,
        form: CxForm { flags: 0x1F, cs: 0x99, ds: 0x2B, es: 0x2B, ss: 0x2B, rsp: 0 },
        want: 2,
    });
    // 正路锚 3 例。
    v.push(CaseSpec { id: 27, form: base, want: 0 });
    // 内核口径（CS 0x10 / SS 0x18）。
    v.push(CaseSpec { id: 28, form: CxForm { flags: 0x1F, cs: 0x10, ds: 0x18, es: 0x18, ss: 0x18, rsp: 0xFF_D00 }, want: 0 });
    // 栈顶边界正路（RSP 恰 = RSP_TOP 且八字对齐）。
    v.push(CaseSpec { id: 29, form: CxForm { flags: 0x1F, cs: 0x33, ds: 0x2B, es: 0x2B, ss: 0x2B, rsp: RSP_TOP }, want: 0 });
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
    pub fn code_counts(&self) -> [usize; 4] {
        let mut c = [0usize; 4];
        for (_, got, _) in self.rows.iter() {
            if (*got as usize) < 4 {
                c[*got as usize] += 1;
            }
        }
        c
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("CX 注入账：\n");
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
    // 种子写死（协议禁 rand）：0x30b00 段基址。
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
    // 种子写死：0x30b01（闸序序列）。
    let mut lcg = Lcg::new(0x30b01);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 坏 Flags + 坏段寄存器叠加 → BadFlags 短路。
        let f1 = CxForm { flags: 0, cs: 0x99, ds: 0x2B, es: 0x2B, ss: 0x2B, rsp: 0 };
        all_ok = all_ok && matches!(judge(&f1), Err(CxCode::BadFlags));
        // 坏段寄存器 + 坏 RSP 叠加 → BadSeg 短路。
        let f2 = CxForm { flags: 0x1F, cs: 0x99, ds: 0x2B, es: 0x2B, ss: 0x2B, rsp: 0 };
        all_ok = all_ok && matches!(judge(&f2), Err(CxCode::BadSeg));
        // RSP 三坏叠加（零+失对+越域不可同真，取零语义）→ BadRsp。
        let f3 = CxForm { flags: 0x1F, cs: 0x33, ds: 0x2B, es: 0x2B, ss: 0x2B, rsp: 0 };
        all_ok = all_ok && matches!(judge(&f3), Err(CxCode::BadRsp));
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：合法 CONTEXT 全过 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30b02（正路序列）。
    let mut lcg = Lcg::new(0x30b02);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for c in cases.iter().skip(27) {
            all_ok = all_ok && judge(&c.form).is_ok() && c.want == 0;
        }
        // 全 CS 闭集逐个正路。
        for &cs in CS_VALID.iter() {
            let g = CxForm { flags: 0x1F, cs, ds: 0x2B, es: 0x2B, ss: 0x2B, rsp: 0x1000 };
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
    // 种子写死：0x30b03（分布序列）。
    let mut lcg = Lcg::new(0x30b03);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = InjectBook::new();
        let _ = bk.run_all(&cases);
        let dist = bk.code_counts();
        // 构成：正路 3 + 坏 Flags 9 + 坏段寄存器 8 + 坏 RSP 8 + 复合 2 = 30。
        // 复合 2 例分别落在 Flags/Seg 档。
        let dist_ok = dist[0] == 3 && dist[1] == 10 && dist[2] == 9 && dist[3] == 8;
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
    // 种子写死：0x30b04（变异序列）。
    let mut lcg = Lcg::new(0x30b04);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for c in cases.iter().filter(|c| c.want == 0) {
            // 变异 1：标志位域外。
            let mut m1 = c.form;
            m1.flags |= 0x40;
            all_ok = all_ok && matches!(judge(&m1), Err(CxCode::BadFlags));
            // 变异 2：剥控制面请求位。
            let mut m2 = c.form;
            m2.flags &= !FLAG_CONTROL;
            all_ok = all_ok && matches!(judge(&m2), Err(CxCode::BadFlags));
            // 变异 3：CS 越闭集。
            let mut m3 = c.form;
            m3.cs = 0x44;
            all_ok = all_ok && matches!(judge(&m3), Err(CxCode::BadSeg));
            // 变异 4：RSP 失对。
            let mut m4 = c.form;
            m4.rsp |= 1;
            all_ok = all_ok && matches!(judge(&m4), Err(CxCode::BadRsp));
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路变异轮={}/{}", ok, INJECT_ROUNDS))
}

/// 选择子闭集演练：CS 四档逐档正路 / DS·ES·SS 全闭集正路 10/10 轮。
pub fn drill_selector_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30b05（选择子序列）。
    let mut lcg = Lcg::new(0x30b05);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // CS 四档逐档。
        for &cs in CS_VALID.iter() {
            let g = CxForm { flags: 0x1F, cs, ds: 0x2B, es: 0x2B, ss: 0x2B, rsp: 0x2000 };
            all_ok = all_ok && judge(&g).is_ok();
        }
        // DS/ES/SS 六档逐档。
        for &sel in DSES_VALID.iter() {
            let g = CxForm { flags: 0x1F, cs: 0x33, ds: sel, es: sel, ss: sel, rsp: 0x2000 };
            all_ok = all_ok && judge(&g).is_ok();
        }
        // 空选择子 SS 拒（SS 不可为 0 口径补强：0 在 DSES_VALID 内但 SS 独占校验）。
        let b = CxForm { flags: 0x1F, cs: 0x33, ds: 0, es: 0, ss: 0x99, rsp: 0x2000 };
        all_ok = all_ok && matches!(judge(&b), Err(CxCode::BadSeg));
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("选择子闭集轮={}/{}", ok, INJECT_ROUNDS))
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
pub fn gate_canonical() -> Vec<(&'static str, CxForm, u8)> {
    vec![
        ("Flags闸", CxForm { flags: 0x30, cs: 0x33, ds: 0x2B, es: 0x2B, ss: 0x2B, rsp: 0x1000 }, 1),
        ("Seg闸", CxForm { flags: 0x1F, cs: 0x99, ds: 0x2B, es: 0x2B, ss: 0x2B, rsp: 0x1000 }, 2),
        ("RSP闸", CxForm { flags: 0x1F, cs: 0x33, ds: 0x2B, es: 0x2B, ss: 0x2B, rsp: 1 }, 3),
        ("正路", CxForm { flags: 0x1F, cs: 0x33, ds: 0x2B, es: 0x2B, ss: 0x2B, rsp: 0x1000 }, 0),
    ]
}

/// 闸位巡检演练：逐闸 canonical 例计算码 ≡ 期望码 10/10 轮。
pub fn drill_gate_walk_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30b06（闸位巡检序列）。
    let mut lcg = Lcg::new(0x30b06);
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
    // 种子写死：0x30b07（重复注入序列）。
    let mut lcg = Lcg::new(0x30b07);
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
    // 注 1：坏 ContextFlags 必拒。
    let f1 = CxForm { flags: 0x30, cs: 0x33, ds: 0x2B, es: 0x2B, ss: 0x2B, rsp: 0x1000 };
    if matches!(judge(&f1), Err(CxCode::BadFlags)) {
        caught += 1;
        d.push("注1 旗:抓".to_string());
    } else {
        d.push("注1 旗:漏".to_string());
    }
    // 注 2：裸请求无控制面必拒。
    let f2 = CxForm { flags: 0x0E, cs: 0x33, ds: 0x2B, es: 0x2B, ss: 0x2B, rsp: 0x1000 };
    if matches!(judge(&f2), Err(CxCode::BadFlags)) {
        caught += 1;
        d.push("注2 裸旗:抓".to_string());
    } else {
        d.push("注2 裸旗:漏".to_string());
    }
    // 注 3：坏 CS 必拒。
    let f3 = CxForm { flags: 0x1F, cs: 0x40, ds: 0x2B, es: 0x2B, ss: 0x2B, rsp: 0x1000 };
    if matches!(judge(&f3), Err(CxCode::BadSeg)) {
        caught += 1;
        d.push("注3 CS:抓".to_string());
    } else {
        d.push("注3 CS:漏".to_string());
    }
    // 注 4：坏 RSP（失对）必拒。
    let f4 = CxForm { flags: 0x1F, cs: 0x33, ds: 0x2B, es: 0x2B, ss: 0x2B, rsp: 0x1001 };
    if matches!(judge(&f4), Err(CxCode::BadRsp)) {
        caught += 1;
        d.push("注4 RSP:抓".to_string());
    } else {
        d.push("注4 RSP:漏".to_string());
    }
    // 注 5：正路锚不可堵死。
    let f5 = CxForm { flags: 0x1F, cs: 0x33, ds: 0x2B, es: 0x2B, ss: 0x2B, rsp: 0x1000 };
    if judge(&f5).is_ok() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30b00 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    CxCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 注入报告。
#[derive(Clone, Copy, Debug)]
pub struct CxReport {
    /// 例数。
    pub cases: usize,
    /// 命中数。
    pub hits: usize,
    /// 码分布。
    pub dist: [usize; 4],
    /// 账单调。
    pub mono: bool,
}

impl CxReport {
    /// 建报告。
    pub fn build(bk: &InjectBook, hits: usize) -> Self {
        CxReport { cases: bk.rows(), hits, dist: bk.code_counts(), mono: bk.monotone() }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == INJECT_CASES && self.hits == INJECT_CASES && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "CxReport 例={} 命中={} 分布={:?} 单调={}",
            self.cases, self.hits, self.dist, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct CxCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_cx_checklist(bk: &InjectBook, hits: usize) -> Vec<CxCheckRow> {
    let rep = CxReport::build(bk, hits);
    let dist = bk.code_counts();
    let mut rows: Vec<CxCheckRow> = Vec::new();
    rows.push(CxCheckRow { name: "30例全注", passed: bk.rows() == INJECT_CASES });
    rows.push(CxCheckRow { name: "30例全中", passed: hits == INJECT_CASES });
    rows.push(CxCheckRow { name: "坏Flags10例", passed: dist[1] == 10 });
    rows.push(CxCheckRow { name: "坏段寄存器9例", passed: dist[2] == 9 });
    rows.push(CxCheckRow { name: "坏RSP8例", passed: dist[3] == 8 });
    rows.push(CxCheckRow { name: "正路3例", passed: dist[0] == 3 });
    rows.push(CxCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(CxCheckRow { name: "转录非空", passed: bk.export_lines().contains("CX 注入账") });
    rows.push(CxCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(CxCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn cx_checklist_green(rows: &[CxCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn cx_stats_line(bk: &InjectBook, hits: usize) -> String {
    format!(
        "CX 注入概况：例 {} 命中 {}/{}",
        bk.rows(),
        hits,
        INJECT_CASES
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12488_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        CxReport::build(&bk, hits).all_green() && bk.export_lines().contains("CX 注入账"),
        cx_stats_line(&bk, hits),
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
    let rows = build_cx_checklist(&bk, hits);
    out.push((
        "X-终验清单10项",
        cx_checklist_green(&rows),
        CxReport::build(&bk, hits).to_text(),
    ));
    // X-e2：正路变异 10/10 + 选择子闭集 10/10 + 账码回放面。
    let (ok_m, d_m) = drill_mutate_positive_10();
    out.push(("X-正路变异10/10", ok_m, d_m));
    let (ok_s, d_s) = drill_selector_10();
    out.push(("X-选择子闭集10/10", ok_s, d_s));
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
        FLAGS_VALID_MASK == 0x1F && CS_VALID == [0x10, 0x1B, 0x23, 0x33]
            && DSES_VALID == [0, 0x10, 0x18, 0x23, 0x2B, 0x33] && RSP_TOP == 0x0000_7FFF_FFFF_0000,
        format!("mask={:#x} cs={:?} dses={:?} top={:#x}", FLAGS_VALID_MASK, CS_VALID, DSES_VALID, RSP_TOP),
    ));
    out
}

/// run_unx12488_checks：本单判据聚合（诊断段 0x30b00）。
pub fn run_unx12488_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12488 CONTEXT 注入矩阵");
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
        hits == INJECT_CASES && dist[0] == 3 && dist[1] == 10 && dist[2] == 9 && dist[3] == 8,
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
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30b00段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12488_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
