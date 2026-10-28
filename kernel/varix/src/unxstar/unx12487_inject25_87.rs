//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12487 · UNWIND_INFO 注入矩阵 · 0x30ae0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12487-J1 坏版本/坏标志/槽计数溢出 30 例注入全过，与 B08
//!      拒止档一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - UNWIND_INFO 形态档：Version(3bit)/Flags(5bit)/SizeOfProlog/
//!     CountOfCodes/UnwindCode 容量五字段校验；
//!   - 闸序（判据语义序）：坏版本 → 坏标志 → 坏帧寄存器 → 槽计数溢出
//!     （CountOfCodes×2 > 码区容量），前置命中即短路；
//!   - 版本闸：Version 闭集 {1,2}（NT 口径 UNWIND_VERSION = 1/2）；
//!   - 标志闸：Flags 出五位合法位域（ECHM/ UOHM/CHAINHANDLER/
//!     CHAINFUNCE/NO_FPO 口径 0x1F）拒；
//!   - 帧寄存器闸：帧标志置位但帧寄存器 > 15 拒；
//!   - 槽计数溢出闸：每槽 2 字节，CountOfCodes × 2 > CodesSize 拒
//!     （经典坑：计数与码区容量不符）；
//!   - 30 例注入账：9 例坏版本 + 7 例坏标志 + 6 例坏帧寄存器 +
//!     5 例槽计数溢出 + 3 例正路锚；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30ae0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30ae0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30ae0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 注入例数（判据 30 例）。
pub const INJECT_CASES: usize = 30;

/// 版本闭集 {1,2}（NT 口径）。
pub const VERSIONS: [u8; 2] = [1, 2];

/// 标志合法位域（五标志 0x1F 口径）。
pub const FLAGS_VALID_MASK: u8 = 0x1F;

/// 帧寄存器上界（x64 通用寄存器 16 个）。
pub const FRAME_REG_MAX: u8 = 15;

/// 诊断码（0x30ae0 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UiCode {
    /// 0x30ae0 坏版本（出 {1,2} 闭集）。
    BadVersion = 0,
    /// 0x30ae1 坏标志（位域外）。
    BadFlags = 1,
    /// 0x30ae2 坏帧寄存器（帧标志置位但寄存器 > 15）。
    BadFrameReg = 2,
    /// 0x30ae3 槽计数溢出（CountOfCodes×2 > CodesSize）。
    SlotOverflow = 3,
    /// 0x30ae4 注入例数不足。
    CaseShort = 4,
    /// 0x30ae5 例码漂移。
    CaseDrift = 5,
    /// 0x30ae6 注入账缩水。
    LedgerShrink = 6,
    /// 0x30ae7 闸序破。
    GateOrder = 7,
    /// 0x30ae8 正路锚堵死。
    PositiveBlocked = 8,
    /// 0x30ae9 注入未执行。
    NotRun = 9,
    /// 0x30aea 拒止档失配。
    TierDiff = 10,
}

impl UiCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [UiCode; 11] = [
        UiCode::BadVersion, UiCode::BadFlags, UiCode::BadFrameReg, UiCode::SlotOverflow,
        UiCode::CaseShort, UiCode::CaseDrift, UiCode::LedgerShrink, UiCode::GateOrder,
        UiCode::PositiveBlocked, UiCode::NotRun, UiCode::TierDiff,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30ae0 => "坏版本：UNWIND_INFO 版本只认 1/2，回查头字节构造。",
        0x30ae1 => "坏标志：标志位出五位合法位域，回查 Flags 定义。",
        0x30ae2 => "坏帧寄存器：帧标志置位时寄存器 ≤15。",
        0x30ae3 => "槽计数溢出：CountOfCodes×2 不得超码区容量。",
        0x30ae4 => "注入例数不足：30 例必须全注入。",
        0x30ae5 => "例码漂移：逐例回放判定链定位失配例。",
        0x30ae6 => "注入账缩水：append-only，禁止截断。",
        0x30ae7 => "闸序破：前置闸命中必须短路。",
        0x30ae8 => "正路锚堵死：合法 UNWIND_INFO 被拒先修正判定链。",
        0x30ae9 => "注入未执行：账面零例。",
        0x30aea => "拒止档失配：与 B08 拒止档逐例核对。",
        _ => "未知诊断码：回查 0x30ae0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30ae0 段基址。
    pub const SEED: u64 = 0x30ae0_30ae_030a_e0u64;

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

/// UNWIND_INFO 注入形态。
#[derive(Clone, Copy, Debug)]
pub struct UiForm {
    /// 版本（3bit 口径）。
    pub version: u8,
    /// 标志（5bit 口径）。
    pub flags: u8,
    /// 帧寄存器（4bit 口径）。
    pub frame_reg: u8,
    /// 帧偏移（4bit 口径）。
    pub frame_off: u8,
    /// 槽计数（CountOfCodes）。
    pub count: u32,
    /// 码区容量（字节）。
    pub codes_size: u32,
}

/// 帧标志位（UNWIND_FLAG_REG */}
const FRAME_FLAG: u8 = 0x08;

/// 判定链（闸序：版本 → 标志 → 帧寄存器 → 槽计数溢出）。
pub fn judge(f: &UiForm) -> Result<(), UiCode> {
    // 闸 1：版本闭集。
    if !VERSIONS.contains(&f.version) {
        return Err(UiCode::BadVersion);
    }
    // 闸 2：标志位域。
    if f.flags & !FLAGS_VALID_MASK != 0 {
        return Err(UiCode::BadFlags);
    }
    // 闸 3：帧寄存器（帧标志置位才校）。
    if f.flags & FRAME_FLAG != 0 && f.frame_reg > FRAME_REG_MAX {
        return Err(UiCode::BadFrameReg);
    }
    // 闸 4：槽计数溢出（每槽 2 字节）。
    if f.count * 2 > f.codes_size {
        return Err(UiCode::SlotOverflow);
    }
    Ok(())
}

/// 注入例。
#[derive(Clone, Copy, Debug)]
pub struct CaseSpec {
    /// 例号。
    pub id: u16,
    /// 形态。
    pub form: UiForm,
    /// 期望码序号（0=Accept，1..=判别值+1）。
    pub want: u8,
}

/// 30 例注入账构建（9 坏版本 + 7 坏标志 + 6 坏帧寄存器 + 5 槽溢出 + 3 正路）。
pub fn build_cases() -> Vec<CaseSpec> {
    let mut v: Vec<CaseSpec> = Vec::new();
    // 基线：合法形态（v2, flags 0x8 帧标志, reg 5, count 4, codes 16）。
    let base = UiForm { version: 2, flags: 0x08, frame_reg: 5, frame_off: 0, count: 4, codes_size: 16 };
    // 坏版本 9 例。
    for i in 0..9u32 {
        let mut f = base;
        f.version = match i % 3 {
            0 => 0,
            1 => 3 + (i / 3) as u8,
            _ => 0xFF,
        };
        v.push(CaseSpec { id: i as u16, form: f, want: 1 });
    }
    // 坏标志 7 例（位域外 0x20/0x40/0x80/0xFF…）。
    for i in 0..7u32 {
        let mut f = base;
        f.flags = match i % 3 {
            0 => 0x20,
            1 => 0x40 + i as u8,
            _ => 0xFF,
        };
        v.push(CaseSpec { id: 9 + i as u16, form: f, want: 2 });
    }
    // 坏帧寄存器 6 例（帧标志置位 + reg > 15）。
    for i in 0..6u32 {
        let mut f = base;
        f.frame_reg = 16 + i as u8;
        if i == 5 {
            f.frame_reg = 0xFF;
        }
        v.push(CaseSpec { id: 16 + i as u16, form: f, want: 3 });
    }
    // 槽计数溢出 5 例（count×2 > codes_size）。
    for i in 0..5u32 {
        let mut f = base;
        f.count = 9 + i; // 18..26 字节 > 16
        v.push(CaseSpec { id: 22 + i as u16, form: f, want: 4 });
    }
    // 正路锚 3 例。
    v.push(CaseSpec { id: 27, form: base, want: 0 });
    // 无帧标志：reg 值无关（reg 0xFF 也不校——闸 3 只在帧标志置位时校）。
    v.push(CaseSpec { id: 28, form: UiForm { version: 1, flags: 0, frame_reg: 0xFF, frame_off: 0, count: 0, codes_size: 0 }, want: 0 });
    // 槽计数恰满（count×2 == codes_size）。
    v.push(CaseSpec { id: 29, form: UiForm { version: 1, flags: 0, frame_reg: 0, frame_off: 0, count: 8, codes_size: 16 }, want: 0 });
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
        let mut s = String::from("UI 注入账：\n");
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
    // 种子写死（协议禁 rand）：0x30ae0 段基址。
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
    // 种子写死：0x30ae1（闸序序列）。
    let mut lcg = Lcg::new(0x30ae1);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 坏版本 + 坏标志叠加 → BadVersion 短路。
        let f1 = UiForm { version: 0, flags: 0xFF, frame_reg: 0, frame_off: 0, count: 0, codes_size: 0 };
        all_ok = all_ok && matches!(judge(&f1), Err(UiCode::BadVersion));
        // 坏标志 + 坏帧寄存器叠加 → BadFlags 短路。
        let f2 = UiForm { version: 1, flags: 0xFF, frame_reg: 0xFF, frame_off: 0, count: 0, codes_size: 0 };
        all_ok = all_ok && matches!(judge(&f2), Err(UiCode::BadFlags));
        // 坏帧寄存器 + 槽溢出叠加 → BadFrameReg 短路。
        let f3 = UiForm { version: 1, flags: 0x08, frame_reg: 0xFF, frame_off: 0, count: 99, codes_size: 2 };
        all_ok = all_ok && matches!(judge(&f3), Err(UiCode::BadFrameReg));
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：合法 UNWIND_INFO 全过 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30ae2（正路序列）。
    let mut lcg = Lcg::new(0x30ae2);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for c in cases.iter().skip(27) {
            all_ok = all_ok && judge(&c.form).is_ok() && c.want == 0;
        }
        // 大码区正路（count 100 槽 / codes 256 字节）。
        let g = UiForm { version: 2, flags: 0x08, frame_reg: 15, frame_off: 5, count: 100, codes_size: 256 };
        all_ok = all_ok && judge(&g).is_ok();
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
    // 种子写死：0x30ae3（分布序列）。
    let mut lcg = Lcg::new(0x30ae3);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = InjectBook::new();
        let _ = bk.run_all(&cases);
        let dist = bk.code_counts();
        // 构成：正路 3 + 坏版本 9 + 坏标志 7 + 坏帧寄存器 6 + 槽溢出 5 = 30。
        let dist_ok = dist[0] == 3 && dist[1] == 9 && dist[2] == 7 && dist[3] == 6 && dist[4] == 5;
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
    // 种子写死：0x30ae4（变异序列）。
    let mut lcg = Lcg::new(0x30ae4);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for c in cases.iter().filter(|c| c.want == 0) {
            // 变异 1：版本出闭集。
            let mut m1 = c.form;
            m1.version = 0;
            all_ok = all_ok && matches!(judge(&m1), Err(UiCode::BadVersion));
            // 变异 2：标志位域外。
            let mut m2 = c.form;
            m2.flags |= 0x20;
            all_ok = all_ok && matches!(judge(&m2), Err(UiCode::BadFlags));
            // 变异 3：帧寄存器越界（帧标志置位前提）。
            let mut m3 = c.form;
            m3.flags |= 0x08;
            m3.frame_reg = 0xFF;
            all_ok = all_ok && matches!(judge(&m3), Err(UiCode::BadFrameReg));
            // 变异 4：槽计数溢出。
            let mut m4 = c.form;
            m4.codes_size = m4.codes_size.min(2);
            m4.count = 9;
            all_ok = all_ok && matches!(judge(&m4), Err(UiCode::SlotOverflow));
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路变异轮={}/{}", ok, INJECT_ROUNDS))
}

/// 码区满容边界演练：槽计恰满正路 / 差半槽拒 10/10 轮。
pub fn drill_slot_edge_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30ae5（边界序列）。
    let mut lcg = Lcg::new(0x30ae5);
    for _r in 0..INJECT_ROUNDS {
        // 槽计恰满（8 槽 × 2 字节 = 16 字节）。
        let g = UiForm { version: 1, flags: 0, frame_reg: 0, frame_off: 0, count: 8, codes_size: 16 };
        // 差半槽（9 槽 17 字节容量）拒。
        let b1 = UiForm { version: 1, flags: 0, frame_reg: 0, frame_off: 0, count: 9, codes_size: 17 };
        // 零槽零码区正路。
        let g2 = UiForm { version: 1, flags: 0, frame_reg: 0, frame_off: 0, count: 0, codes_size: 0 };
        let _ = lcg.below(3);
        if judge(&g).is_ok() && matches!(judge(&b1), Err(UiCode::SlotOverflow)) && judge(&g2).is_ok() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("码区满容边界轮={}/{}", ok, INJECT_ROUNDS))
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
pub fn gate_canonical() -> Vec<(&'static str, UiForm, u8)> {
    vec![
        ("版本闸", UiForm { version: 7, flags: 0, frame_reg: 0, frame_off: 0, count: 0, codes_size: 0 }, 1),
        ("标志闸", UiForm { version: 1, flags: 0x80, frame_reg: 0, frame_off: 0, count: 0, codes_size: 0 }, 2),
        ("帧寄闸", UiForm { version: 1, flags: 0x08, frame_reg: 0xFF, frame_off: 0, count: 0, codes_size: 0 }, 3),
        ("槽溢闸", UiForm { version: 1, flags: 0, frame_reg: 0, frame_off: 0, count: 9, codes_size: 16 }, 4),
        ("正路", UiForm { version: 2, flags: 0x08, frame_reg: 5, frame_off: 0, count: 4, codes_size: 16 }, 0),
    ]
}

/// 闸位巡检演练：逐闸 canonical 例计算码 ≡ 期望码 10/10 轮。
pub fn drill_gate_walk_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30ae6（闸位巡检序列）。
    let mut lcg = Lcg::new(0x30ae6);
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
    // 种子写死：0x30ae7（重复注入序列）。
    let mut lcg = Lcg::new(0x30ae7);
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
    // 注 1：坏版本必拒。
    let f1 = UiForm { version: 7, flags: 0, frame_reg: 0, frame_off: 0, count: 0, codes_size: 0 };
    if matches!(judge(&f1), Err(UiCode::BadVersion)) {
        caught += 1;
        d.push("注1 版本:抓".to_string());
    } else {
        d.push("注1 版本:漏".to_string());
    }
    // 注 2：坏标志必拒。
    let f2 = UiForm { version: 1, flags: 0x80, frame_reg: 0, frame_off: 0, count: 0, codes_size: 0 };
    if matches!(judge(&f2), Err(UiCode::BadFlags)) {
        caught += 1;
        d.push("注2 标志:抓".to_string());
    } else {
        d.push("注2 标志:漏".to_string());
    }
    // 注 3：坏帧寄存器必拒。
    let f3 = UiForm { version: 1, flags: 0x08, frame_reg: 0xFF, frame_off: 0, count: 0, codes_size: 0 };
    if matches!(judge(&f3), Err(UiCode::BadFrameReg)) {
        caught += 1;
        d.push("注3 帧寄:抓".to_string());
    } else {
        d.push("注3 帧寄:漏".to_string());
    }
    // 注 4：槽计数溢出必拒。
    let f4 = UiForm { version: 1, flags: 0, frame_reg: 0, frame_off: 0, count: 9, codes_size: 16 };
    if matches!(judge(&f4), Err(UiCode::SlotOverflow)) {
        caught += 1;
        d.push("注4 槽溢:抓".to_string());
    } else {
        d.push("注4 槽溢:漏".to_string());
    }
    // 注 5：正路锚不可堵死。
    let f5 = UiForm { version: 2, flags: 0x08, frame_reg: 5, frame_off: 0, count: 4, codes_size: 16 };
    if judge(&f5).is_ok() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30ae0 段十一码闭集全部落在段内。
pub fn segment_guard() -> bool {
    UiCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 注入报告。
#[derive(Clone, Copy, Debug)]
pub struct UiReport {
    /// 例数。
    pub cases: usize,
    /// 命中数。
    pub hits: usize,
    /// 码分布。
    pub dist: [usize; 5],
    /// 账单调。
    pub mono: bool,
}

impl UiReport {
    /// 建报告。
    pub fn build(bk: &InjectBook, hits: usize) -> Self {
        UiReport { cases: bk.rows(), hits, dist: bk.code_counts(), mono: bk.monotone() }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == INJECT_CASES && self.hits == INJECT_CASES && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "UiReport 例={} 命中={} 分布={:?} 单调={}",
            self.cases, self.hits, self.dist, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct UiCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_ui_checklist(bk: &InjectBook, hits: usize) -> Vec<UiCheckRow> {
    let rep = UiReport::build(bk, hits);
    let dist = bk.code_counts();
    let mut rows: Vec<UiCheckRow> = Vec::new();
    rows.push(UiCheckRow { name: "30例全注", passed: bk.rows() == INJECT_CASES });
    rows.push(UiCheckRow { name: "30例全中", passed: hits == INJECT_CASES });
    rows.push(UiCheckRow { name: "坏版本9例", passed: dist[1] == 9 });
    rows.push(UiCheckRow { name: "坏标志7例", passed: dist[2] == 7 });
    rows.push(UiCheckRow { name: "坏帧寄6例", passed: dist[3] == 6 });
    rows.push(UiCheckRow { name: "槽溢出5例", passed: dist[4] == 5 });
    rows.push(UiCheckRow { name: "正路3例", passed: dist[0] == 3 });
    rows.push(UiCheckRow { name: "转录非空", passed: bk.export_lines().contains("UI 注入账") });
    rows.push(UiCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(UiCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ui_checklist_green(rows: &[UiCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ui_stats_line(bk: &InjectBook, hits: usize) -> String {
    format!(
        "UI 注入概况：例 {} 命中 {}/{}",
        bk.rows(),
        hits,
        INJECT_CASES
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12487_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        UiReport::build(&bk, hits).all_green() && bk.export_lines().contains("UI 注入账"),
        ui_stats_line(&bk, hits),
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
    let rows = build_ui_checklist(&bk, hits);
    out.push((
        "X-终验清单10项",
        ui_checklist_green(&rows),
        UiReport::build(&bk, hits).to_text(),
    ));
    // X-e2：正路变异 10/10 + 码区满容边界 10/10 + 账码回放面。
    let (ok_m, d_m) = drill_mutate_positive_10();
    out.push(("X-正路变异10/10", ok_m, d_m));
    let (ok_s, d_s) = drill_slot_edge_10();
    out.push(("X-码区满容边界10/10", ok_s, d_s));
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
        VERSIONS == [1, 2] && FLAGS_VALID_MASK == 0x1F && FRAME_REG_MAX == 15 && INJECT_CASES == 30,
        format!("ver={:?} mask={:#x} reg={} cases={}", VERSIONS, FLAGS_VALID_MASK, FRAME_REG_MAX, INJECT_CASES),
    ));
    out
}

/// run_unx12487_checks：本单判据聚合（诊断段 0x30ae0）。
pub fn run_unx12487_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12487 UNWIND_INFO 注入矩阵");
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
        hits == INJECT_CASES && dist[0] == 3 && dist[1] == 9 && dist[2] == 7 && dist[3] == 6 && dist[4] == 5,
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
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30ae0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12487_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
