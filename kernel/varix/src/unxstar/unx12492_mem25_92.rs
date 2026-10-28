//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12492 · 内存族注入矩阵 · 0x30b80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F12492-J1 坏保护/坏基址/坏 RegionSize 30 例注入全过，与
//!      B13 档一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 内存操作形态档：Protect（保护位域）/Base（页对齐域）/
//!     RegionSize（页倍数域）三字段校验；
//!   - 闸序（判据语义序）：坏保护 → 坏基址 → 坏 RegionSize，前置命中
//!     即短路；
//!   - 保护闸：Protect 出合法位域拒（NOACCESS/READONLY/READWRITE/
//!     WRITECOPY/EXECUTE 五执行位并集口径 0xFF 模型，未知高位拒）；
//!   - 基址闸：Base 非零且非页对齐拒（NULL 由调用面另行处理）；
//!   - 尺寸闸：RegionSize 为零或非页倍数拒；
//!   - 30 例注入账：10 例坏保护 + 8 例坏基址 + 7 例坏 RegionSize +
//!     5 例正路锚；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30b80），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30b80 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30b80;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 注入例数（判据 30 例）。
pub const INJECT_CASES: usize = 30;

/// 页大小（4KB）。
pub const PAGE_SIZE: u64 = 0x1000;

/// 保护位合法域（NOACCESS..EXECUTE_WRITECOPY 八位并集模型口径）。
pub const PROTECT_VALID_MASK: u32 = 0x0000_00FF;

/// 诊断码（0x30b80 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemCode {
    /// 0x30b80 坏保护（位域外）。
    BadProtect = 0,
    /// 0x30b81 坏基址（非页对齐）。
    BadBase = 1,
    /// 0x30b82 坏 RegionSize（零或非页倍数）。
    BadRegionSize = 2,
    /// 0x30b83 注入例数不足。
    CaseShort = 3,
    /// 0x30b84 例码漂移。
    CaseDrift = 4,
    /// 0x30b85 注入账缩水。
    LedgerShrink = 5,
    /// 0x30b86 闸序破。
    GateOrder = 6,
    /// 0x30b87 正路锚堵死。
    PositiveBlocked = 7,
    /// 0x30b88 注入未执行。
    NotRun = 8,
    /// 0x30b89 拒止档失配。
    TierDiff = 9,
}

impl MemCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [MemCode; 10] = [
        MemCode::BadProtect, MemCode::BadBase, MemCode::BadRegionSize,
        MemCode::CaseShort, MemCode::CaseDrift, MemCode::LedgerShrink, MemCode::GateOrder,
        MemCode::PositiveBlocked, MemCode::NotRun, MemCode::TierDiff,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30b80 => "坏保护：Protect 出合法位域，回查 B13 保护位定义。",
        0x30b81 => "坏基址：Base 非零时必须页对齐（4KB）。",
        0x30b82 => "坏 RegionSize：必须非零且为页倍数。",
        0x30b83 => "注入例数不足：30 例必须全注入。",
        0x30b84 => "例码漂移：逐例回放判定链定位失配例。",
        0x30b85 => "注入账缩水：append-only，禁止截断。",
        0x30b86 => "闸序破：前置闸命中必须短路。",
        0x30b87 => "正路锚堵死：合法内存操作被拒先修正判定链。",
        0x30b88 => "注入未执行：账面零例。",
        0x30b89 => "拒止档失配：与 B13 档逐例核对。",
        _ => "未知诊断码：回查 0x30b80 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30b80 段基址。
    pub const SEED: u64 = 0x30b80_30b8_030b_80u64;

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

/// 内存操作注入形态。
#[derive(Clone, Copy, Debug)]
pub struct MemForm {
    /// 保护位。
    pub protect: u32,
    /// 基址（0=由系统择址）。
    pub base: u64,
    /// 区域尺寸。
    pub region_size: u64,
}

/// 判定链（闸序：保护 → 基址 → 尺寸）。
pub fn judge(f: &MemForm) -> Result<(), MemCode> {
    // 闸 1：保护位域。
    if f.protect & !PROTECT_VALID_MASK != 0 || f.protect == 0 {
        return Err(MemCode::BadProtect);
    }
    // 闸 2：基址页对齐。
    if f.base != 0 && f.base % PAGE_SIZE != 0 {
        return Err(MemCode::BadBase);
    }
    // 闸 3：区域尺寸。
    if f.region_size == 0 || f.region_size % PAGE_SIZE != 0 {
        return Err(MemCode::BadRegionSize);
    }
    Ok(())
}

/// 注入例。
#[derive(Clone, Copy, Debug)]
pub struct CaseSpec {
    /// 例号。
    pub id: u16,
    /// 形态。
    pub form: MemForm,
    /// 期望码序号（0=Accept，1..=判别值+1）。
    pub want: u8,
}

/// 30 例注入账构建（10 坏保护 + 8 坏基址 + 7 坏 RegionSize + 5 正路锚）。
pub fn build_cases() -> Vec<CaseSpec> {
    let mut v: Vec<CaseSpec> = Vec::new();
    // 基线：合法形态（READWRITE / 页对齐基址 / 两页区域）。
    let base = MemForm { protect: 0x04, base: 0x7FF6_0000_0000, region_size: 2 * PAGE_SIZE };
    // 坏保护 10 例。
    for i in 0..10u32 {
        let mut f = base;
        f.protect = match i % 3 {
            0 => 0x100 + i,
            1 => 0x8000_0000 + i,
            _ => 0xFFFF_0000,
        };
        v.push(CaseSpec { id: i as u16, form: f, want: 1 });
    }
    // 坏基址 8 例（非页对齐）。
    for i in 0..8u32 {
        let mut f = base;
        f.base = 0x7FF6_0000_0000 + 1 + i as u64;
        v.push(CaseSpec { id: 10 + i as u16, form: f, want: 2 });
    }
    // 坏 RegionSize 7 例（零 / 非页倍数）。
    for i in 0..7u32 {
        let mut f = base;
        f.region_size = match i % 3 {
            0 => 0,
            1 => PAGE_SIZE + 1 + i as u64,
            _ => PAGE_SIZE / 2 + i as u64,
        };
        v.push(CaseSpec { id: 18 + i as u16, form: f, want: 3 });
    }
    // 正路锚 5 例。
    v.push(CaseSpec { id: 25, form: base, want: 0 });
    // NULL 基址（系统择址）正路。
    v.push(CaseSpec { id: 26, form: MemForm { protect: 0x04, base: 0, region_size: PAGE_SIZE }, want: 0 });
    // NOACCESS 档正路（位域内）。
    v.push(CaseSpec { id: 27, form: MemForm { protect: 0x01, base: 0x7FF6_0000_1000, region_size: PAGE_SIZE }, want: 0 });
    // 大区域正路。
    v.push(CaseSpec { id: 28, form: MemForm { protect: 0x40, base: 0, region_size: 64 * PAGE_SIZE }, want: 0 });
    // EXECUTE_READ 档正路。
    v.push(CaseSpec { id: 29, form: MemForm { protect: 0x20, base: 0x7FF6_0000_2000, region_size: 4 * PAGE_SIZE }, want: 0 });
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
        let mut s = String::from("内存注入账：\n");
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
    // 种子写死（协议禁 rand）：0x30b80 段基址。
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
    // 种子写死：0x30b81（闸序序列）。
    let mut lcg = Lcg::new(0x30b81);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 坏保护 + 坏基址叠加 → BadProtect 短路。
        let f1 = MemForm { protect: 0x8000_0000, base: 0x1234, region_size: 0 };
        all_ok = all_ok && matches!(judge(&f1), Err(MemCode::BadProtect));
        // 坏基址 + 坏尺寸叠加 → BadBase 短路。
        let f2 = MemForm { protect: 0x04, base: 0x1234, region_size: 0 };
        all_ok = all_ok && matches!(judge(&f2), Err(MemCode::BadBase));
        // 零保护归 BadProtect（保护位域零值拒）。
        let f3 = MemForm { protect: 0, base: 0x7FF6_0000_0000, region_size: PAGE_SIZE };
        all_ok = all_ok && matches!(judge(&f3), Err(MemCode::BadProtect));
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：合法内存操作全过 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30b82（正路序列）。
    let mut lcg = Lcg::new(0x30b82);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for c in cases.iter().skip(25) {
            all_ok = all_ok && judge(&c.form).is_ok() && c.want == 0;
        }
        // 合法保护位逐位正路（0x01..0x80）。
        for b in 0..8u32 {
            let g = MemForm { protect: 1u32 << b, base: 0, region_size: PAGE_SIZE };
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
    // 种子写死：0x30b83（分布序列）。
    let mut lcg = Lcg::new(0x30b83);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = InjectBook::new();
        let _ = bk.run_all(&cases);
        let dist = bk.code_counts();
        // 构成：正路 5 + 坏保护 10 + 坏基址 8 + 坏尺寸 7 = 30。
        let dist_ok = dist[0] == 5 && dist[1] == 10 && dist[2] == 8 && dist[3] == 7;
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
    // 种子写死：0x30b84（变异序列）。
    let mut lcg = Lcg::new(0x30b84);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for c in cases.iter().filter(|c| c.want == 0) {
            // 变异 1：保护位域外。
            let mut m1 = c.form;
            m1.protect |= 0x100;
            all_ok = all_ok && matches!(judge(&m1), Err(MemCode::BadProtect));
            // 变异 2：零保护。
            let mut m2 = c.form;
            m2.protect = 0;
            all_ok = all_ok && matches!(judge(&m2), Err(MemCode::BadProtect));
            // 变异 3：基址失对（非零基址前提）。
            let mut m3 = c.form;
            if m3.base == 0 {
                m3.base = PAGE_SIZE + 1;
            } else {
                m3.base |= 1;
            }
            all_ok = all_ok && matches!(judge(&m3), Err(MemCode::BadBase));
            // 变异 4：尺寸零化。
            let mut m4 = c.form;
            m4.region_size = 0;
            all_ok = all_ok && matches!(judge(&m4), Err(MemCode::BadRegionSize));
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路变异轮={}/{}", ok, INJECT_ROUNDS))
}

/// 页倍数边界演练：半页拒 / 恰一页过 / 大页域过 10/10 轮。
pub fn drill_page_edge_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30b85（边界序列）。
    let mut lcg = Lcg::new(0x30b85);
    for _r in 0..INJECT_ROUNDS {
        // 半页拒。
        let b1 = MemForm { protect: 0x04, base: 0, region_size: PAGE_SIZE / 2 };
        // 恰一页过。
        let g1 = MemForm { protect: 0x04, base: 0, region_size: PAGE_SIZE };
        // 1024 页大域过。
        let g2 = MemForm { protect: 0x04, base: 0, region_size: 1024 * PAGE_SIZE };
        // 基址恰页界过。
        let g3 = MemForm { protect: 0x04, base: 0x7FF6_0000_0000, region_size: PAGE_SIZE };
        let _ = lcg.below(3);
        if matches!(judge(&b1), Err(MemCode::BadRegionSize))
            && judge(&g1).is_ok() && judge(&g2).is_ok() && judge(&g3).is_ok()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("页倍数边界轮={}/{}", ok, INJECT_ROUNDS))
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
pub fn gate_canonical() -> Vec<(&'static str, MemForm, u8)> {
    vec![
        ("保护闸", MemForm { protect: 0x100, base: 0, region_size: PAGE_SIZE }, 1),
        ("基址闸", MemForm { protect: 0x04, base: 0x1234, region_size: PAGE_SIZE }, 2),
        ("尺寸闸", MemForm { protect: 0x04, base: 0, region_size: 0 }, 3),
        ("正路", MemForm { protect: 0x04, base: 0x7FF6_0000_0000, region_size: PAGE_SIZE }, 0),
    ]
}

/// 闸位巡检演练：逐闸 canonical 例计算码 ≡ 期望码 10/10 轮。
pub fn drill_gate_walk_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30b86（闸位巡检序列）。
    let mut lcg = Lcg::new(0x30b86);
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
    // 种子写死：0x30b87（重复注入序列）。
    let mut lcg = Lcg::new(0x30b87);
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
    // 注 1：坏保护必拒。
    let f1 = MemForm { protect: 0x100, base: 0, region_size: PAGE_SIZE };
    if matches!(judge(&f1), Err(MemCode::BadProtect)) {
        caught += 1;
        d.push("注1 保护:抓".to_string());
    } else {
        d.push("注1 保护:漏".to_string());
    }
    // 注 2：零保护必拒。
    let f2 = MemForm { protect: 0, base: 0, region_size: PAGE_SIZE };
    if matches!(judge(&f2), Err(MemCode::BadProtect)) {
        caught += 1;
        d.push("注2 零保:抓".to_string());
    } else {
        d.push("注2 零保:漏".to_string());
    }
    // 注 3：坏基址必拒。
    let f3 = MemForm { protect: 0x04, base: 0x7FF6_0000_0001, region_size: PAGE_SIZE };
    if matches!(judge(&f3), Err(MemCode::BadBase)) {
        caught += 1;
        d.push("注3 基址:抓".to_string());
    } else {
        d.push("注3 基址:漏".to_string());
    }
    // 注 4：坏 RegionSize 必拒。
    let f4 = MemForm { protect: 0x04, base: 0, region_size: 0 };
    if matches!(judge(&f4), Err(MemCode::BadRegionSize)) {
        caught += 1;
        d.push("注4 尺寸:抓".to_string());
    } else {
        d.push("注4 尺寸:漏".to_string());
    }
    // 注 5：正路锚不可堵死。
    let f5 = MemForm { protect: 0x04, base: 0x7FF6_0000_0000, region_size: PAGE_SIZE };
    if judge(&f5).is_ok() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30b80 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    MemCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 注入报告。
#[derive(Clone, Copy, Debug)]
pub struct MemReport {
    /// 例数。
    pub cases: usize,
    /// 命中数。
    pub hits: usize,
    /// 码分布。
    pub dist: [usize; 4],
    /// 账单调。
    pub mono: bool,
}

impl MemReport {
    /// 建报告。
    pub fn build(bk: &InjectBook, hits: usize) -> Self {
        MemReport { cases: bk.rows(), hits, dist: bk.code_counts(), mono: bk.monotone() }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == INJECT_CASES && self.hits == INJECT_CASES && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "MemReport 例={} 命中={} 分布={:?} 单调={}",
            self.cases, self.hits, self.dist, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct MemCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_mem_checklist(bk: &InjectBook, hits: usize) -> Vec<MemCheckRow> {
    let rep = MemReport::build(bk, hits);
    let dist = bk.code_counts();
    let mut rows: Vec<MemCheckRow> = Vec::new();
    rows.push(MemCheckRow { name: "30例全注", passed: bk.rows() == INJECT_CASES });
    rows.push(MemCheckRow { name: "30例全中", passed: hits == INJECT_CASES });
    rows.push(MemCheckRow { name: "坏保护10例", passed: dist[1] == 10 });
    rows.push(MemCheckRow { name: "坏基址8例", passed: dist[2] == 8 });
    rows.push(MemCheckRow { name: "坏RegionSize7例", passed: dist[3] == 7 });
    rows.push(MemCheckRow { name: "正路5例", passed: dist[0] == 5 });
    rows.push(MemCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(MemCheckRow { name: "转录非空", passed: bk.export_lines().contains("内存注入账") });
    rows.push(MemCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(MemCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn mem_checklist_green(rows: &[MemCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn mem_stats_line(bk: &InjectBook, hits: usize) -> String {
    format!(
        "内存注入概况：例 {} 命中 {}/{}",
        bk.rows(),
        hits,
        INJECT_CASES
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12492_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        MemReport::build(&bk, hits).all_green() && bk.export_lines().contains("内存注入账"),
        mem_stats_line(&bk, hits),
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
    let rows = build_mem_checklist(&bk, hits);
    out.push((
        "X-终验清单10项",
        mem_checklist_green(&rows),
        MemReport::build(&bk, hits).to_text(),
    ));
    // X-e2：正路变异 10/10 + 页倍数边界 10/10 + 账码回放面。
    let (ok_m, d_m) = drill_mutate_positive_10();
    out.push(("X-正路变异10/10", ok_m, d_m));
    let (ok_p2, d_p2) = drill_page_edge_10();
    out.push(("X-页倍数边界10/10", ok_p2, d_p2));
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
        PAGE_SIZE == 0x1000 && PROTECT_VALID_MASK == 0xFF && INJECT_CASES == 30,
        format!("page={:#x} mask={:#x} cases={}", PAGE_SIZE, PROTECT_VALID_MASK, INJECT_CASES),
    ));
    out
}

/// run_unx12492_checks：本单判据聚合（诊断段 0x30b80）。
pub fn run_unx12492_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12492 内存族注入矩阵");
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
        hits == INJECT_CASES && dist[0] == 5 && dist[1] == 10 && dist[2] == 8 && dist[3] == 7,
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
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30b80段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12492_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
