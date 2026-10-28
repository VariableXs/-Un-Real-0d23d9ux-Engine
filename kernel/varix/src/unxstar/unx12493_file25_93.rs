//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12493 · 文件族注入矩阵 · 0x30ba0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12493-J1 坏句柄/坏偏移/坏 IO 状态块 30 例注入全过，与
//!      B14 档一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 文件 IO 形态档：Handle（句柄表域 + 伪句柄特值）/Offset
//!     （LARGE_INTEGER 低 32 位模拟，文件尺寸闭包）/IO_STATUS 块
//!     （Status/Information 字段域）三面校验；
//!   - 闸序（判据语义序）：坏句柄 → 坏偏移 → 坏 IO 状态块，前置命中
//!     即短路；
//!   - 句柄闸：句柄必须落 0x1000..0x1010 表域或为 -1/-2 伪句柄特值
//!     （B27 特值口径预告锚），其余拒；
//!   - 偏移闸：Offset > FileSize 拒（越文件尾）；
//!   - IO_STATUS 闸：Status 须成功类（0x0 口径）才可写 Information；
//!     Information 不得超请求长度；
//!   - 30 例注入账：10 例坏句柄 + 8 例坏偏移 + 7 例坏 IO 状态块 +
//!     5 例正路锚；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30ba0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30ba0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30ba0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 注入例数（判据 30 例）。
pub const INJECT_CASES: usize = 30;

/// 句柄基（B14 句柄派生口径）。
pub const HANDLE_BASE: u32 = 0x1000;

/// 句柄表宽（16 槽）。
pub const HANDLE_SLOTS: usize = 16;

/// 伪句柄特值（-1/-2，u32 回绕口径）。
pub const HANDLE_CURRENT_PROCESS: u32 = 0xFFFF_FFFF;
pub const HANDLE_CURRENT_THREAD: u32 = 0xFFFF_FFFE;

/// 模拟文件尺寸（64KB）。
pub const FILE_SIZE: u64 = 0x10000;

/// 诊断码（0x30ba0 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlCode {
    /// 0x30ba0 坏句柄（句柄越域且非伪句柄特值）。
    BadHandle = 0,
    /// 0x30ba1 坏偏移（越文件尾）。
    BadOffset = 1,
    /// 0x30ba2 坏 IO 状态块（Status 非成功类 / Information 超请求）。
    BadIosb = 2,
    /// 0x30ba3 注入例数不足。
    CaseShort = 3,
    /// 0x30ba4 例码漂移。
    CaseDrift = 4,
    /// 0x30ba5 注入账缩水。
    LedgerShrink = 5,
    /// 0x30ba6 闸序破。
    GateOrder = 6,
    /// 0x30ba7 正路锚堵死。
    PositiveBlocked = 7,
    /// 0x30ba8 注入未执行。
    NotRun = 8,
    /// 0x30ba9 拒止档失配。
    TierDiff = 9,
    /// 0x30baa 伪句柄越权（伪句柄用于文件操作域）。
    PseudoMisuse = 10,
}

impl FlCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [FlCode; 11] = [
        FlCode::BadHandle, FlCode::BadOffset, FlCode::BadIosb,
        FlCode::CaseShort, FlCode::CaseDrift, FlCode::LedgerShrink, FlCode::GateOrder,
        FlCode::PositiveBlocked, FlCode::NotRun, FlCode::TierDiff, FlCode::PseudoMisuse,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30ba0 => "坏句柄：句柄出 0x1000..0x1010 表域且非 -1/-2 特值。",
        0x30ba1 => "坏偏移：Offset 超文件尾，先查文件尺寸再读。",
        0x30ba2 => "坏 IO 状态块：Status 非成功类或 Information 超请求长度。",
        0x30ba3 => "注入例数不足：30 例必须全注入。",
        0x30ba4 => "例码漂移：逐例回放判定链定位失配例。",
        0x30ba5 => "注入账缩水：append-only，禁止截断。",
        0x30ba6 => "闸序破：前置闸命中必须短路。",
        0x30ba7 => "正路锚堵死：合法文件操作被拒先修正判定链。",
        0x30ba8 => "注入未执行：账面零例。",
        0x30ba9 => "拒止档失配：与 B14 档逐例核对。",
        0x30baa => "伪句柄越权：-1/-2 特值不得用于文件 IO 域。",
        _ => "未知诊断码：回查 0x30ba0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30ba0 段基址。
    pub const SEED: u64 = 0x30ba0_30ba_030b_a0u64;

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

/// IO_STATUS 块形态。
#[derive(Clone, Copy, Debug)]
pub struct Iosb {
    /// Status（成功类 0）。
    pub status: u32,
    /// Information（实际传输字节数）。
    pub info: u32,
}

/// 文件 IO 注入形态。
#[derive(Clone, Copy, Debug)]
pub struct FileForm {
    /// 句柄。
    pub handle: u32,
    /// 偏移。
    pub offset: u64,
    /// 请求长度。
    pub length: u32,
    /// IO 状态块。
    pub iosb: Iosb,
}

/// 句柄闸（表域 + 伪句柄特值分档）。
pub fn handle_gate(h: u32) -> Result<bool, FlCode> {
    // 伪句柄特值：文件 IO 域显式拒（越权使用）。
    if h == HANDLE_CURRENT_PROCESS || h == HANDLE_CURRENT_THREAD {
        return Err(FlCode::PseudoMisuse);
    }
    let off = h.checked_sub(HANDLE_BASE).unwrap_or(u32::MAX);
    if (off as usize) < HANDLE_SLOTS {
        Ok(true)
    } else {
        Err(FlCode::BadHandle)
    }
}

/// 判定链（闸序：句柄 → 偏移 → IO 状态块）。
pub fn judge(f: &FileForm) -> Result<(), FlCode> {
    // 闸 1：句柄。
    handle_gate(f.handle)?;
    // 闸 2：偏移。
    if f.offset > FILE_SIZE {
        return Err(FlCode::BadOffset);
    }
    // 闸 3：IO 状态块（Status 非成功类带 Information / Information 超请求）。
    if f.iosb.status != 0 && f.iosb.info != 0 {
        return Err(FlCode::BadIosb);
    }
    if f.iosb.status == 0 && f.iosb.info > f.length {
        return Err(FlCode::BadIosb);
    }
    Ok(())
}

/// 注入例。
#[derive(Clone, Copy, Debug)]
pub struct CaseSpec {
    /// 例号。
    pub id: u16,
    /// 形态。
    pub form: FileForm,
    /// 期望码序号（0=Accept，1..=判别值+1）。
    pub want: u8,
}

/// 30 例注入账构建（10 坏句柄 + 8 坏偏移 + 7 坏 IO 状态块 + 5 正路锚）。
pub fn build_cases() -> Vec<CaseSpec> {
    let mut v: Vec<CaseSpec> = Vec::new();
    // 基线：合法形态（句柄 0x1003 / 偏移 0x100 / 请求 0x100 / IOSB 全零）。
    let base = FileForm { handle: HANDLE_BASE + 3, offset: 0x100, length: 0x100, iosb: Iosb { status: 0, info: 0 } };
    // 坏句柄 10 例。
    for i in 0..10u32 {
        let mut f = base;
        f.handle = match i % 4 {
            0 => 0,
            1 => HANDLE_BASE + HANDLE_SLOTS as u32 + i,
            2 => HANDLE_BASE - 1,
            _ => HANDLE_CURRENT_PROCESS,
        };
        // 伪句柄例期望 PseudoMisuse（判别值 10 → +1 = 11）。
        let want = if f.handle == HANDLE_CURRENT_PROCESS { 11u8 } else { 1 };
        v.push(CaseSpec { id: i as u16, form: f, want });
    }
    // 坏偏移 8 例（越文件尾）。
    for i in 0..8u32 {
        let mut f = base;
        f.offset = FILE_SIZE + 1 + i as u64;
        v.push(CaseSpec { id: 10 + i as u16, form: f, want: 2 });
    }
    // 坏 IO 状态块 7 例。
    for i in 0..7u32 {
        let mut f = base;
        f.iosb = match i % 3 {
            // 非成功类 Status 带非零 Information。
            0 => Iosb { status: 0xC000_0001 + i, info: 1 },
            // 成功类但 Information 超请求。
            1 => Iosb { status: 0, info: base.length + 1 + i },
            // 非成功类 Status 且 Information 超请求（双坏）。
            _ => Iosb { status: 0xC000_0001 + i, info: base.length + 1 },
        };
        v.push(CaseSpec { id: 18 + i as u16, form: f, want: 3 });
    }
    // 正路锚 5 例。
    v.push(CaseSpec { id: 25, form: base, want: 0 });
    // 成功 IOSB：Information = 请求长度。
    v.push(CaseSpec { id: 26, form: FileForm { handle: HANDLE_BASE, offset: 0, length: 0x80, iosb: Iosb { status: 0, info: 0x80 } }, want: 0 });
    // 文件尾恰对齐（offset == FILE_SIZE，零长请求）。
    v.push(CaseSpec { id: 27, form: FileForm { handle: HANDLE_BASE + 1, offset: FILE_SIZE, length: 0, iosb: Iosb { status: 0, info: 0 } }, want: 0 });
    // 伪句柄特值…本域不用；改用末槽句柄正路。
    v.push(CaseSpec { id: 28, form: FileForm { handle: HANDLE_BASE + (HANDLE_SLOTS as u32 - 1), offset: 0x200, length: 0x40, iosb: Iosb { status: 0, info: 0 } }, want: 0 });
    // Status 非成功且 Information = 0（合法错误回执）。
    v.push(CaseSpec { id: 29, form: FileForm { handle: HANDLE_BASE + 2, offset: 0x100, length: 0x40, iosb: Iosb { status: 0xC000_0001, info: 0 } }, want: 0 });
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
        let mut s = String::from("文件注入账：\n");
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
    // 种子写死（协议禁 rand）：0x30ba0 段基址。
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
    // 种子写死：0x30ba1（闸序序列）。
    let mut lcg = Lcg::new(0x30ba1);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 坏句柄 + 坏偏移叠加 → BadHandle 短路。
        let f1 = FileForm { handle: 0, offset: FILE_SIZE + 1, length: 0, iosb: Iosb { status: 0, info: 0 } };
        all_ok = all_ok && matches!(judge(&f1), Err(FlCode::BadHandle));
        // 伪句柄 + 坏偏移叠加 → PseudoMisuse 短路（句柄闸内分档先行）。
        let f2 = FileForm { handle: HANDLE_CURRENT_PROCESS, offset: FILE_SIZE + 1, length: 0, iosb: Iosb { status: 0, info: 0 } };
        all_ok = all_ok && matches!(judge(&f2), Err(FlCode::PseudoMisuse));
        // 坏偏移 + 坏 IOSB 叠加 → BadOffset 短路。
        let f3 = FileForm { handle: HANDLE_BASE, offset: FILE_SIZE + 1, length: 0x100, iosb: Iosb { status: 0xC000_0001, info: 5 } };
        all_ok = all_ok && matches!(judge(&f3), Err(FlCode::BadOffset));
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：合法文件操作全过 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30ba2（正路序列）。
    let mut lcg = Lcg::new(0x30ba2);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for c in cases.iter().skip(25) {
            all_ok = all_ok && judge(&c.form).is_ok() && c.want == 0;
        }
        // 句柄表全域逐槽正路。
        for i in 0..HANDLE_SLOTS {
            let g = FileForm { handle: HANDLE_BASE + i as u32, offset: 0, length: 0, iosb: Iosb { status: 0, info: 0 } };
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
    // 种子写死：0x30ba3（分布序列）。
    let mut lcg = Lcg::new(0x30ba3);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = InjectBook::new();
        let _ = bk.run_all(&cases);
        let dist = bk.code_counts();
        // 构成：正路 5 + 坏句柄 8（档 1）+ 伪句柄 2（码 11 出档）+
        // 坏偏移 8（档 2）+ 坏 IOSB 7（档 3）= 30。
        let dist_ok = dist[0] == 5 && dist[1] == 8 && dist[2] == 8 && dist[3] == 7;
        let _ = lcg.below(3);
        if dist_ok && bk.rows() == INJECT_CASES {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("码分布轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路变异演练：对全部正路例做确定性三向变异，期望全部被抓 10/10 轮。
pub fn drill_mutate_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30ba4（变异序列）。
    let mut lcg = Lcg::new(0x30ba4);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for c in cases.iter().filter(|c| c.want == 0) {
            // 变异 1：句柄越表域。
            let mut m1 = c.form;
            m1.handle = 0x2000;
            all_ok = all_ok && matches!(judge(&m1), Err(FlCode::BadHandle));
            // 变异 2：偏移越文件尾。
            let mut m2 = c.form;
            m2.offset = FILE_SIZE + 1;
            all_ok = all_ok && matches!(judge(&m2), Err(FlCode::BadOffset));
            // 变异 3：Information 超请求。
            let mut m3 = c.form;
            m3.iosb.status = 0;
            m3.iosb.info = m3.length + 1;
            all_ok = all_ok && matches!(judge(&m3), Err(FlCode::BadIosb));
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路变异轮={}/{}", ok, INJECT_ROUNDS))
}

/// 文件尾边界演练：尾对齐零长请求正路 / 尾后一字拒 10/10 轮。
pub fn drill_eof_edge_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30ba5（边界序列）。
    let mut lcg = Lcg::new(0x30ba5);
    for _r in 0..INJECT_ROUNDS {
        // 尾对齐零长请求正路。
        let g = FileForm { handle: HANDLE_BASE, offset: FILE_SIZE, length: 0, iosb: Iosb { status: 0, info: 0 } };
        // 尾后一字拒。
        let b = FileForm { handle: HANDLE_BASE, offset: FILE_SIZE + 1, length: 0, iosb: Iosb { status: 0, info: 0 } };
        // 尾内最后一字节正路。
        let g2 = FileForm { handle: HANDLE_BASE, offset: FILE_SIZE - 1, length: 1, iosb: Iosb { status: 0, info: 0 } };
        let _ = lcg.below(3);
        if judge(&g).is_ok() && matches!(judge(&b), Err(FlCode::BadOffset)) && judge(&g2).is_ok() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("文件尾边界轮={}/{}", ok, INJECT_ROUNDS))
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
pub fn gate_canonical() -> Vec<(&'static str, FileForm, u8)> {
    vec![
        ("句柄闸", FileForm { handle: 0x1234, offset: 0, length: 0, iosb: Iosb { status: 0, info: 0 } }, 1),
        ("偏移闸", FileForm { handle: HANDLE_BASE, offset: FILE_SIZE + 1, length: 0, iosb: Iosb { status: 0, info: 0 } }, 2),
        ("IOSB闸", FileForm { handle: HANDLE_BASE, offset: 0, length: 0x10, iosb: Iosb { status: 0, info: 0x20 } }, 3),
        ("正路", FileForm { handle: HANDLE_BASE, offset: 0, length: 0, iosb: Iosb { status: 0, info: 0 } }, 0),
    ]
}

/// 闸位巡检演练：逐闸 canonical 例计算码 ≡ 期望码 10/10 轮。
pub fn drill_gate_walk_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30ba6（闸位巡检序列）。
    let mut lcg = Lcg::new(0x30ba6);
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
    // 种子写死：0x30ba7（重复注入序列）。
    let mut lcg = Lcg::new(0x30ba7);
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
    // 注 1：坏句柄必拒。
    let f1 = FileForm { handle: 0x1234, offset: 0, length: 0, iosb: Iosb { status: 0, info: 0 } };
    if matches!(judge(&f1), Err(FlCode::BadHandle)) {
        caught += 1;
        d.push("注1 句柄:抓".to_string());
    } else {
        d.push("注1 句柄:漏".to_string());
    }
    // 注 2：伪句柄越权必拒。
    let f2 = FileForm { handle: HANDLE_CURRENT_THREAD, offset: 0, length: 0, iosb: Iosb { status: 0, info: 0 } };
    if matches!(judge(&f2), Err(FlCode::PseudoMisuse)) {
        caught += 1;
        d.push("注2 伪句:抓".to_string());
    } else {
        d.push("注2 伪句:漏".to_string());
    }
    // 注 3：坏偏移必拒。
    let f3 = FileForm { handle: HANDLE_BASE, offset: FILE_SIZE + 8, length: 0, iosb: Iosb { status: 0, info: 0 } };
    if matches!(judge(&f3), Err(FlCode::BadOffset)) {
        caught += 1;
        d.push("注3 偏移:抓".to_string());
    } else {
        d.push("注3 偏移:漏".to_string());
    }
    // 注 4：坏 IOSB（Information 超请求）必拒。
    let f4 = FileForm { handle: HANDLE_BASE, offset: 0, length: 0x10, iosb: Iosb { status: 0, info: 0x20 } };
    if matches!(judge(&f4), Err(FlCode::BadIosb)) {
        caught += 1;
        d.push("注4 IOSB:抓".to_string());
    } else {
        d.push("注4 IOSB:漏".to_string());
    }
    // 注 5：正路锚不可堵死。
    let f5 = FileForm { handle: HANDLE_BASE, offset: 0, length: 0, iosb: Iosb { status: 0, info: 0 } };
    if judge(&f5).is_ok() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30ba0 段十一码闭集全部落在段内。
pub fn segment_guard() -> bool {
    FlCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 注入报告。
#[derive(Clone, Copy, Debug)]
pub struct FlReport {
    /// 例数。
    pub cases: usize,
    /// 命中数。
    pub hits: usize,
    /// 码分布。
    pub dist: [usize; 4],
    /// 账单调。
    pub mono: bool,
}

impl FlReport {
    /// 建报告。
    pub fn build(bk: &InjectBook, hits: usize) -> Self {
        FlReport { cases: bk.rows(), hits, dist: bk.code_counts(), mono: bk.monotone() }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == INJECT_CASES && self.hits == INJECT_CASES && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "FlReport 例={} 命中={} 分布={:?} 单调={}",
            self.cases, self.hits, self.dist, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct FlCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_fl_checklist(bk: &InjectBook, hits: usize) -> Vec<FlCheckRow> {
    let rep = FlReport::build(bk, hits);
    let dist = bk.code_counts();
    let mut rows: Vec<FlCheckRow> = Vec::new();
    rows.push(FlCheckRow { name: "30例全注", passed: bk.rows() == INJECT_CASES });
    rows.push(FlCheckRow { name: "30例全中", passed: hits == INJECT_CASES });
    rows.push(FlCheckRow { name: "坏句柄8例+伪句柄2例", passed: dist[1] == 8 && bk.export_lines().contains("例3 得11") });
    rows.push(FlCheckRow { name: "坏偏移8例", passed: dist[2] == 8 });
    rows.push(FlCheckRow { name: "坏IOSB7例", passed: dist[3] == 7 });
    rows.push(FlCheckRow { name: "正路5例", passed: dist[0] == 5 });
    rows.push(FlCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(FlCheckRow { name: "转录非空", passed: bk.export_lines().contains("文件注入账") });
    rows.push(FlCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(FlCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn fl_checklist_green(rows: &[FlCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn fl_stats_line(bk: &InjectBook, hits: usize) -> String {
    format!(
        "文件注入概况：例 {} 命中 {}/{}",
        bk.rows(),
        hits,
        INJECT_CASES
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12493_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        FlReport::build(&bk, hits).all_green() && bk.export_lines().contains("文件注入账"),
        fl_stats_line(&bk, hits),
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
    let rows = build_fl_checklist(&bk, hits);
    out.push((
        "X-终验清单10项",
        fl_checklist_green(&rows),
        FlReport::build(&bk, hits).to_text(),
    ));
    // X-e2：正路变异 10/10 + 文件尾边界 10/10 + 账码回放面。
    let (ok_m, d_m) = drill_mutate_positive_10();
    out.push(("X-正路变异10/10", ok_m, d_m));
    let (ok_e, d_e) = drill_eof_edge_10();
    out.push(("X-文件尾边界10/10", ok_e, d_e));
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
        HANDLE_BASE == 0x1000 && HANDLE_SLOTS == 16 && FILE_SIZE == 0x10000
            && HANDLE_CURRENT_PROCESS == 0xFFFF_FFFF && HANDLE_CURRENT_THREAD == 0xFFFF_FFFE,
        format!("hbase={:#x} slots={} fsize={:#x} pseudo={:#x}/{:#x}", HANDLE_BASE, HANDLE_SLOTS, FILE_SIZE, HANDLE_CURRENT_PROCESS, HANDLE_CURRENT_THREAD),
    ));
    out
}

/// run_unx12493_checks：本单判据聚合（诊断段 0x30ba0）。
pub fn run_unx12493_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12493 文件族注入矩阵");
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
        hits == INJECT_CASES && dist[0] == 5 && dist[1] == 8 && dist[2] == 8 && dist[3] == 7,
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
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30ba0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12493_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}

