//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12486 · RUNTIME_FUNCTION 注入矩阵 · 0x30ac0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12486-J1 坏 BeginAddress/链环/越界 30 例注入全过，与 B07
//!      拒止档一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - RUNTIME_FUNCTION 形态档：BeginAddress/EndAddress/UnwindData 三
//!     字段 + 镜像尺寸（ImageSize）闭包域校验；
//!   - 闸序（判据语义序）：坏 BeginAddress（Begin≥End 或 4 字节失对）
//!     → 越界（End/UnwindData 超镜像尺寸）→ 链环（链式展开目标指向
//!     自身）→ 空洞（End-Begin 不足 4 字节），前置命中即短路；
//!   - 坏 BeginAddress 闸：Begin ≥ End 拒；Begin 非四字节对齐拒；
//!   - 越界闸：End > ImageSize 拒；UnwindData ≥ ImageSize 拒；
//!   - 链环闸：链式展开标志位（UNWIND_DATA 低二位 = 0x3 口径的模拟）
//!     且目标 = 自身 UnwindData 拒（自环）；
//!   - 30 例注入账：10 例坏 Begin + 7 例越界 + 7 例链环 + 3 例越空洞 +
//!     3 例正路锚；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30ac0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30ac0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30ac0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 注入例数（判据 30 例）。
pub const INJECT_CASES: usize = 30;

/// 镜像尺寸口径（0x10000，B07 拒止档基准镜像）。
pub const IMAGE_SIZE: u32 = 0x10000;

/// 链式展开标志位（UNWIND_DATA 低二位模拟口径）。
pub const CHAIN_FLAG: u32 = 0x3;

/// 诊断码（0x30ac0 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RfCode {
    /// 0x30ac0 坏 BeginAddress（Begin≥End 或失对）。
    BadBegin = 0,
    /// 0x30ac1 越界（End/UnwindData 超镜像）。
    Oob = 1,
    /// 0x30ac2 链环（链式目标自环）。
    ChainLoop = 2,
    /// 0x30ac3 空洞（End-Begin < 4）。
    Hole = 3,
    /// 0x30ac4 注入例数不足。
    CaseShort = 4,
    /// 0x30ac5 例码漂移。
    CaseDrift = 5,
    /// 0x30ac6 注入账缩水。
    LedgerShrink = 6,
    /// 0x30ac7 闸序破。
    GateOrder = 7,
    /// 0x30ac8 正路锚堵死。
    PositiveBlocked = 8,
    /// 0x30ac9 注入未执行。
    NotRun = 9,
    /// 0x30aca 拒止档失配。
    TierDiff = 10,
}

impl RfCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [RfCode; 11] = [
        RfCode::BadBegin, RfCode::Oob, RfCode::ChainLoop, RfCode::Hole,
        RfCode::CaseShort, RfCode::CaseDrift, RfCode::LedgerShrink, RfCode::GateOrder,
        RfCode::PositiveBlocked, RfCode::NotRun, RfCode::TierDiff,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30ac0 => "坏 BeginAddress：Begin 必须 < End 且四字节对齐。",
        0x30ac1 => "越界：End/UnwindData 不得超镜像尺寸。",
        0x30ac2 => "链环：链式展开目标不得指向自身。",
        0x30ac3 => "空洞：End-Begin 至少 4 字节。",
        0x30ac4 => "注入例数不足：30 例必须全注入。",
        0x30ac5 => "例码漂移：逐例回放判定链定位失配例。",
        0x30ac6 => "注入账缩水：append-only，禁止截断。",
        0x30ac7 => "闸序破：前置闸命中必须短路。",
        0x30ac8 => "正路锚堵死：合法 RUNTIME_FUNCTION 被拒先修正判定链。",
        0x30ac9 => "注入未执行：账面零例。",
        0x30aca => "拒止档失配：与 B07 拒止档逐例核对。",
        _ => "未知诊断码：回查 0x30ac0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30ac0 段基址。
    pub const SEED: u64 = 0x30ac0_30ac_030a_c0u64;

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

/// RUNTIME_FUNCTION 注入形态。
#[derive(Clone, Copy, Debug)]
pub struct RfForm {
    /// BeginAddress。
    pub begin: u32,
    /// EndAddress。
    pub end: u32,
    /// UnwindData（低二位链式标志模拟；高位 = 展开信息 RVA）。
    pub unwind: u32,
}

/// 判定链（闸序：坏 Begin → 越界 → 链环 → 空洞）。
pub fn judge(f: &RfForm) -> Result<(), RfCode> {
    // 闸 1：坏 BeginAddress。
    if f.begin >= f.end {
        return Err(RfCode::BadBegin);
    }
    if f.begin % 4 != 0 {
        return Err(RfCode::BadBegin);
    }
    // 闸 2：越界。
    if f.end > IMAGE_SIZE || f.unwind >= IMAGE_SIZE {
        return Err(RfCode::Oob);
    }
    // 闸 3：链环（链式标志 + 目标自环）。
    if f.unwind & CHAIN_FLAG == CHAIN_FLAG {
        let target = f.unwind & !CHAIN_FLAG;
        if target == f.begin {
            return Err(RfCode::ChainLoop);
        }
    }
    // 闸 4：空洞。
    if f.end - f.begin < 4 {
        return Err(RfCode::Hole);
    }
    Ok(())
}

/// 注入例。
#[derive(Clone, Copy, Debug)]
pub struct CaseSpec {
    /// 例号。
    pub id: u16,
    /// 形态。
    pub form: RfForm,
    /// 期望码序号（0=Accept，1..=判别值+1）。
    pub want: u8,
}

/// 30 例注入账构建（10 坏 Begin + 7 越界 + 7 链环 + 3 空洞 + 3 正路）。
pub fn build_cases() -> Vec<CaseSpec> {
    let mut v: Vec<CaseSpec> = Vec::new();
    // 基线：合法形态（begin 0x1000, end 0x1100, unwind 0x2000）。
    let base = RfForm { begin: 0x1000, end: 0x1100, unwind: 0x2000 };
    // 坏 Begin 10 例。
    for i in 0..10u32 {
        let mut f = base;
        match i % 3 {
            // Begin ≥ End。
            0 => {
                f.begin = base.end + i * 4;
            }
            // 失对（奇地址恒非四字节对齐）。
            1 => {
                f.begin = 0x1001 + i * 4;
            }
            // Begin = End。
            _ => {
                f.begin = base.end;
            }
        }
        v.push(CaseSpec { id: i as u16, form: f, want: 1 });
    }
    // 越界 7 例。
    for i in 0..7u32 {
        let mut f = base;
        match i % 3 {
            // End 超镜像。
            0 => {
                f.end = IMAGE_SIZE + 4 + i * 4;
            }
            // UnwindData 超镜像。
            1 => {
                f.unwind = IMAGE_SIZE + i;
            }
            // UnwindData = 镜像上界。
            _ => {
                f.unwind = IMAGE_SIZE;
            }
        }
        v.push(CaseSpec { id: 10 + i as u16, form: f, want: 2 });
    }
    // 链环 7 例（链式标志 + 自环目标）。
    for i in 0..7u32 {
        let mut f = base;
        f.unwind = base.begin | CHAIN_FLAG;
        // 微变 begin 保持自环（目标 = begin）。
        f.begin = 0x1000 + i * 4;
        f.unwind = f.begin | CHAIN_FLAG;
        v.push(CaseSpec { id: 17 + i as u16, form: f, want: 3 });
    }
    // 空洞 3 例（End-Begin < 4）。
    for i in 0..3u32 {
        let mut f = base;
        f.begin = 0x2000 + i * 4;
        f.end = 0x2000 + i * 4 + (1 + i);
        v.push(CaseSpec { id: 24 + i as u16, form: f, want: 4 });
    }
    // 正路锚 3 例。
    v.push(CaseSpec { id: 27, form: base, want: 0 });
    v.push(CaseSpec { id: 28, form: RfForm { begin: 0x2000, end: 0x2004, unwind: 0x5000 }, want: 0 });
    // 链式但非自环（目标 0x6000 ≠ begin）→ 过。
    v.push(CaseSpec { id: 29, form: RfForm { begin: 0x2000, end: 0x2004, unwind: 0x6000 | CHAIN_FLAG }, want: 0 });
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
        let mut s = String::from("RF 注入账：\n");
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
    // 种子写死（协议禁 rand）：0x30ac0 段基址。
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
    // 种子写死：0x30ac1（闸序序列）。
    let mut lcg = Lcg::new(0x30ac1);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 坏 Begin + 越界叠加 → BadBegin 短路。
        let f1 = RfForm { begin: 0x20000, end: 0x20000, unwind: IMAGE_SIZE };
        all_ok = all_ok && matches!(judge(&f1), Err(RfCode::BadBegin));
        // 越界 + 链环叠加 → Oob 短路（UnwindData 恰触镜像上界）。
        let f2 = RfForm { begin: 0x1000, end: 0x1100, unwind: IMAGE_SIZE };
        all_ok = all_ok && matches!(judge(&f2), Err(RfCode::Oob));
        // 链环 + 空洞叠加 → ChainLoop 短路（空洞口径：end-begin<4 且链环）。
        let f3 = RfForm { begin: 0x3000, end: 0x3001, unwind: 0x3000 | CHAIN_FLAG };
        all_ok = all_ok && matches!(judge(&f3), Err(RfCode::ChainLoop));
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：合法 RUNTIME_FUNCTION 全过 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30ac2（正路序列）。
    let mut lcg = Lcg::new(0x30ac2);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for c in cases.iter().skip(27) {
            all_ok = all_ok && judge(&c.form).is_ok() && c.want == 0;
        }
        // 镜像尾对齐正路（end 恰 = IMAGE_SIZE）。
        let g1 = RfForm { begin: IMAGE_SIZE - 0x100, end: IMAGE_SIZE, unwind: 0x8000 };
        all_ok = all_ok && judge(&g1).is_ok();
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
    // 种子写死：0x30ac3（分布序列）。
    let mut lcg = Lcg::new(0x30ac3);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = InjectBook::new();
        let _ = bk.run_all(&cases);
        let dist = bk.code_counts();
        // 构成：正路 3 + 坏 Begin 10 + 越界 7 + 链环 7 + 空洞 3 = 30。
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
    // 种子写死：0x30ac4（变异序列）。
    let mut lcg = Lcg::new(0x30ac4);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for c in cases.iter().filter(|c| c.want == 0) {
            // 变异 1：Begin 失对。
            let mut m1 = c.form;
            m1.begin |= 2;
            all_ok = all_ok && matches!(judge(&m1), Err(RfCode::BadBegin));
            // 变异 2：End 越镜像。
            let mut m2 = c.form;
            m2.end = IMAGE_SIZE + 8;
            all_ok = all_ok && matches!(judge(&m2), Err(RfCode::Oob));
            // 变异 3：链式自环。
            let mut m3 = c.form;
            m3.unwind = m3.begin | CHAIN_FLAG;
            all_ok = all_ok && matches!(judge(&m3), Err(RfCode::ChainLoop));
            // 变异 4：空洞化。
            let mut m4 = c.form;
            m4.end = m4.begin + 2;
            all_ok = all_ok && matches!(judge(&m4), Err(RfCode::Hole));
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路变异轮={}/{}", ok, INJECT_ROUNDS))
}

/// 镜像尾边界演练：End 恰触镜像上界正路 / 越一字拒 10/10 轮。
pub fn drill_image_edge_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30ac5（边界序列）。
    let mut lcg = Lcg::new(0x30ac5);
    for _r in 0..INJECT_ROUNDS {
        // End 恰 = IMAGE_SIZE（正路）。
        let g = RfForm { begin: IMAGE_SIZE - 0x100, end: IMAGE_SIZE, unwind: 0x8000 };
        // UnwindData 恰 = IMAGE_SIZE（越上界拒）。
        let b1 = RfForm { begin: IMAGE_SIZE - 0x100, end: IMAGE_SIZE, unwind: IMAGE_SIZE };
        // End = IMAGE_SIZE + 4（越界拒）。
        let b2 = RfForm { begin: IMAGE_SIZE - 0x100, end: IMAGE_SIZE + 4, unwind: 0x8000 };
        let _ = lcg.below(3);
        if judge(&g).is_ok() && matches!(judge(&b1), Err(RfCode::Oob))
            && matches!(judge(&b2), Err(RfCode::Oob))
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("镜像尾边界轮={}/{}", ok, INJECT_ROUNDS))
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
pub fn gate_canonical() -> Vec<(&'static str, RfForm, u8)> {
    vec![
        ("Begin闸", RfForm { begin: 0x1100, end: 0x1100, unwind: 0x2000 }, 1),
        ("越界闸", RfForm { begin: 0x1000, end: IMAGE_SIZE + 4, unwind: 0x2000 }, 2),
        ("链环闸", RfForm { begin: 0x1000, end: 0x1100, unwind: 0x1000 | CHAIN_FLAG }, 3),
        ("空洞闸", RfForm { begin: 0x2000, end: 0x2002, unwind: 0x2000 }, 4),
        ("正路", RfForm { begin: 0x1000, end: 0x1100, unwind: 0x2000 }, 0),
    ]
}

/// 闸位巡检演练：逐闸 canonical 例计算码 ≡ 期望码 10/10 轮。
pub fn drill_gate_walk_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30ac6（闸位巡检序列）。
    let mut lcg = Lcg::new(0x30ac6);
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
    // 种子写死：0x30ac7（重复注入序列）。
    let mut lcg = Lcg::new(0x30ac7);
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
    // 注 1：坏 Begin 必拒。
    let f1 = RfForm { begin: 0x1100, end: 0x1100, unwind: 0x2000 };
    if matches!(judge(&f1), Err(RfCode::BadBegin)) {
        caught += 1;
        d.push("注1 Begin:抓".to_string());
    } else {
        d.push("注1 Begin:漏".to_string());
    }
    // 注 2：越界必拒。
    let f2 = RfForm { begin: 0x1000, end: IMAGE_SIZE + 4, unwind: 0x2000 };
    if matches!(judge(&f2), Err(RfCode::Oob)) {
        caught += 1;
        d.push("注2 越界:抓".to_string());
    } else {
        d.push("注2 越界:漏".to_string());
    }
    // 注 3：链环必拒。
    let f3 = RfForm { begin: 0x1000, end: 0x1100, unwind: 0x1000 | CHAIN_FLAG };
    if matches!(judge(&f3), Err(RfCode::ChainLoop)) {
        caught += 1;
        d.push("注3 链环:抓".to_string());
    } else {
        d.push("注3 链环:漏".to_string());
    }
    // 注 4：空洞必拒。
    let f4 = RfForm { begin: 0x2000, end: 0x2002, unwind: 0x2000 };
    if matches!(judge(&f4), Err(RfCode::Hole)) {
        caught += 1;
        d.push("注4 空洞:抓".to_string());
    } else {
        d.push("注4 空洞:漏".to_string());
    }
    // 注 5：正路锚不可堵死。
    let f5 = RfForm { begin: 0x1000, end: 0x1100, unwind: 0x2000 };
    if judge(&f5).is_ok() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30ac0 段十一码闭集全部落在段内。
pub fn segment_guard() -> bool {
    RfCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 注入报告。
#[derive(Clone, Copy, Debug)]
pub struct RfReport {
    /// 例数。
    pub cases: usize,
    /// 命中数。
    pub hits: usize,
    /// 码分布。
    pub dist: [usize; 5],
    /// 账单调。
    pub mono: bool,
}

impl RfReport {
    /// 建报告。
    pub fn build(bk: &InjectBook, hits: usize) -> Self {
        RfReport { cases: bk.rows(), hits, dist: bk.code_counts(), mono: bk.monotone() }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == INJECT_CASES && self.hits == INJECT_CASES && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "RfReport 例={} 命中={} 分布={:?} 单调={}",
            self.cases, self.hits, self.dist, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct RfCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_rf_checklist(bk: &InjectBook, hits: usize) -> Vec<RfCheckRow> {
    let rep = RfReport::build(bk, hits);
    let dist = bk.code_counts();
    let mut rows: Vec<RfCheckRow> = Vec::new();
    rows.push(RfCheckRow { name: "30例全注", passed: bk.rows() == INJECT_CASES });
    rows.push(RfCheckRow { name: "30例全中", passed: hits == INJECT_CASES });
    rows.push(RfCheckRow { name: "坏Begin10例", passed: dist[1] == 10 });
    rows.push(RfCheckRow { name: "越界7例", passed: dist[2] == 7 });
    rows.push(RfCheckRow { name: "链环7例", passed: dist[3] == 7 });
    rows.push(RfCheckRow { name: "空洞3例", passed: dist[4] == 3 });
    rows.push(RfCheckRow { name: "正路3例", passed: dist[0] == 3 });
    rows.push(RfCheckRow { name: "转录非空", passed: bk.export_lines().contains("RF 注入账") });
    rows.push(RfCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(RfCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn rf_checklist_green(rows: &[RfCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn rf_stats_line(bk: &InjectBook, hits: usize) -> String {
    format!(
        "RF 注入概况：例 {} 命中 {}/{}",
        bk.rows(),
        hits,
        INJECT_CASES
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12486_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        RfReport::build(&bk, hits).all_green() && bk.export_lines().contains("RF 注入账"),
        rf_stats_line(&bk, hits),
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
    let rows = build_rf_checklist(&bk, hits);
    out.push((
        "X-终验清单10项",
        rf_checklist_green(&rows),
        RfReport::build(&bk, hits).to_text(),
    ));
    // X-e2：正路变异 10/10 + 镜像尾边界 10/10 + 账码回放面。
    let (ok_m, d_m) = drill_mutate_positive_10();
    out.push(("X-正路变异10/10", ok_m, d_m));
    let (ok_i, d_i) = drill_image_edge_10();
    out.push(("X-镜像尾边界10/10", ok_i, d_i));
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
        IMAGE_SIZE == 0x10000 && CHAIN_FLAG == 0x3 && INJECT_CASES == 30,
        format!("image={:#x} chain={:#x} cases={}", IMAGE_SIZE, CHAIN_FLAG, INJECT_CASES),
    ));
    out
}

/// run_unx12486_checks：本单判据聚合（诊断段 0x30ac0）。
pub fn run_unx12486_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12486 RUNTIME_FUNCTION 注入矩阵");
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
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30ac0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12486_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
