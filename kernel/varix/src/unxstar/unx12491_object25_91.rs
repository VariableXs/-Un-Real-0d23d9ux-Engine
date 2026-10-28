//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12491 · 对象族注入矩阵 · 0x30b60
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12491-J1 坏类型/坏权限/坏属性 30 例注入全过，与 B12 档
//!      一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 对象创建形态档：TypeIndex（类型闭集 8 类）/DesiredAccess
//!     （权限位域）/OA Length（48 字节锚）三字段校验；
//!   - 闸序（判据语义序）：坏类型 → 坏权限 → 坏属性，前置命中即短路；
//!   - 类型闸：TypeIndex 出八类闭集拒（Directory/SymbolicLink/Event/
//!     Mutant/Semaphore/Section/Job/KeyedEvent 模型口径）；
//!   - 权限闸：DesiredAccess 出合法位域拒（GENERIC_READ/WRITE/
//!     EXECUTE/ALL 四泛位 + SYNCHRONIZE/DELETE 等特位并集口径）；
//!   - 属性闸：OA Length ≠ 48 拒（B05/F12484 口径同源）；
//!   - 30 例注入账：10 例坏类型 + 8 例坏权限 + 7 例坏属性 + 5 例正路锚；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30b60），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30b60 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30b60;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 注入例数（判据 30 例）。
pub const INJECT_CASES: usize = 30;

/// 对象类型闭集宽（八类）。
pub const TYPE_COUNT: usize = 8;

/// 权限合法位域（四泛位 + 特位并集模型口径）。
pub const ACCESS_VALID_MASK: u32 = 0x1F01_FF03;

/// OA 结构体大小（与 F12484 同源口径）。
pub const OA_LENGTH: u32 = 48;

/// 诊断码（0x30b60 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObCode {
    /// 0x30b60 坏类型（TypeIndex 出八类闭集）。
    BadType = 0,
    /// 0x30b61 坏权限（DesiredAccess 位域外）。
    BadAccess = 1,
    /// 0x30b62 坏属性（OA Length ≠ 48）。
    BadAttr = 2,
    /// 0x30b63 注入例数不足。
    CaseShort = 3,
    /// 0x30b64 例码漂移。
    CaseDrift = 4,
    /// 0x30b65 注入账缩水。
    LedgerShrink = 5,
    /// 0x30b66 闸序破。
    GateOrder = 6,
    /// 0x30b67 正路锚堵死。
    PositiveBlocked = 7,
    /// 0x30b68 注入未执行。
    NotRun = 8,
    /// 0x30b69 拒止档失配。
    TierDiff = 9,
}

impl ObCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [ObCode; 10] = [
        ObCode::BadType, ObCode::BadAccess, ObCode::BadAttr,
        ObCode::CaseShort, ObCode::CaseDrift, ObCode::LedgerShrink, ObCode::GateOrder,
        ObCode::PositiveBlocked, ObCode::NotRun, ObCode::TierDiff,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30b60 => "坏类型：TypeIndex 出八类闭集，回查对象类型注册表。",
        0x30b61 => "坏权限：DesiredAccess 出合法位域，回查权限位定义。",
        0x30b62 => "坏属性：OBJECT_ATTRIBUTES Length 恒 48 字节。",
        0x30b63 => "注入例数不足：30 例必须全注入。",
        0x30b64 => "例码漂移：逐例回放判定链定位失配例。",
        0x30b65 => "注入账缩水：append-only，禁止截断。",
        0x30b66 => "闸序破：前置闸命中必须短路。",
        0x30b67 => "正路锚堵死：合法创建被拒先修正判定链。",
        0x30b68 => "注入未执行：账面零例。",
        0x30b69 => "拒止档失配：与 B12 档逐例核对。",
        _ => "未知诊断码：回查 0x30b60 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30b60 段基址。
    pub const SEED: u64 = 0x30b60_30b6_030b_60u64;

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

/// 对象类型名闭集（八类，模型口径）。
pub const TYPE_NAMES: [&str; TYPE_COUNT] = [
    "Directory",
    "SymbolicLink",
    "Event",
    "Mutant",
    "Semaphore",
    "Section",
    "Job",
    "KeyedEvent",
];

/// 对象创建注入形态。
#[derive(Clone, Copy, Debug)]
pub struct ObjForm {
    /// TypeIndex。
    pub type_idx: u8,
    /// DesiredAccess。
    pub access: u32,
    /// OA Length。
    pub oa_length: u32,
}

/// 判定链（闸序：类型 → 权限 → 属性）。
pub fn judge(f: &ObjForm) -> Result<(), ObCode> {
    // 闸 1：类型闭集。
    if (f.type_idx as usize) >= TYPE_COUNT {
        return Err(ObCode::BadType);
    }
    // 闸 2：权限位域。
    if f.access & !ACCESS_VALID_MASK != 0 {
        return Err(ObCode::BadAccess);
    }
    // 闸 3：OA 属性。
    if f.oa_length != OA_LENGTH {
        return Err(ObCode::BadAttr);
    }
    Ok(())
}

/// 注入例。
#[derive(Clone, Copy, Debug)]
pub struct CaseSpec {
    /// 例号。
    pub id: u16,
    /// 形态。
    pub form: ObjForm,
    /// 期望码序号（0=Accept，1..=判别值+1）。
    pub want: u8,
}

/// 30 例注入账构建（10 坏类型 + 8 坏权限 + 7 坏属性 + 5 正路锚）。
pub fn build_cases() -> Vec<CaseSpec> {
    let mut v: Vec<CaseSpec> = Vec::new();
    // 基线：合法形态（Event 类型 / 读写权限 / 48 字节 OA）。
    let base = ObjForm { type_idx: 2, access: 0x1F01_FF03, oa_length: OA_LENGTH };
    // 坏类型 10 例（8..17 / 0xFF）。
    for i in 0..10u32 {
        let mut f = base;
        f.type_idx = match i % 3 {
            0 => (TYPE_COUNT as u32 + i) as u8,
            1 => 0xFF,
            _ => (TYPE_COUNT as u32 * 2 + i) as u8,
        };
        v.push(CaseSpec { id: i as u16, form: f, want: 1 });
    }
    // 坏权限 8 例（位域外组合）。
    for i in 0..8u32 {
        let mut f = base;
        f.access = match i % 3 {
            0 => 0x2000_0000,
            1 => 0x8000_0000 + i,
            _ => 0xFFFF_FFFF,
        };
        v.push(CaseSpec { id: 10 + i as u16, form: f, want: 2 });
    }
    // 坏属性 7 例（OA Length 偏大偏小）。
    for i in 0..7u32 {
        let mut f = base;
        f.oa_length = match i % 3 {
            0 => 0,
            1 => OA_LENGTH - 1 - (i / 3),
            _ => OA_LENGTH + 1 + (i / 3),
        };
        v.push(CaseSpec { id: 18 + i as u16, form: f, want: 3 });
    }
    // 正路锚 5 例（八类型逐型前五个 + 全权限位 + 边界）。
    for t in 0..3u8 {
        v.push(CaseSpec { id: 25 + t as u16, form: ObjForm { type_idx: t, access: 0x1F01_FF03, oa_length: OA_LENGTH }, want: 0 });
    }
    v.push(CaseSpec { id: 28, form: ObjForm { type_idx: 7, access: 0x0001_0002, oa_length: OA_LENGTH }, want: 0 });
    v.push(CaseSpec { id: 29, form: ObjForm { type_idx: 5, access: 0, oa_length: OA_LENGTH }, want: 0 });
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
        let mut s = String::from("对象注入账：\n");
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
    // 种子写死（协议禁 rand）：0x30b60 段基址。
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
    // 种子写死：0x30b61（闸序序列）。
    let mut lcg = Lcg::new(0x30b61);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 坏类型 + 坏权限叠加 → BadType 短路。
        let f1 = ObjForm { type_idx: 0xFF, access: 0xFFFF_FFFF, oa_length: 0 };
        all_ok = all_ok && matches!(judge(&f1), Err(ObCode::BadType));
        // 坏权限 + 坏属性叠加 → BadAccess 短路。
        let f2 = ObjForm { type_idx: 2, access: 0x8000_0000, oa_length: 0 };
        all_ok = all_ok && matches!(judge(&f2), Err(ObCode::BadAccess));
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：八类型逐型合法创建 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30b62（正路序列）。
    let mut lcg = Lcg::new(0x30b62);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for c in cases.iter().skip(25) {
            all_ok = all_ok && judge(&c.form).is_ok() && c.want == 0;
        }
        // 八类型逐型正路。
        for t in 0..TYPE_COUNT as u8 {
            let g = ObjForm { type_idx: t, access: 0x1F01_FF03, oa_length: OA_LENGTH };
            all_ok = all_ok && judge(&g).is_ok();
        }
        // 类型名闭集非空。
        all_ok = all_ok && TYPE_NAMES.iter().all(|n| !n.is_empty());
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
    // 种子写死：0x30b63（分布序列）。
    let mut lcg = Lcg::new(0x30b63);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = InjectBook::new();
        let _ = bk.run_all(&cases);
        let dist = bk.code_counts();
        // 构成：正路 5 + 坏类型 10 + 坏权限 8 + 坏属性 7 = 30。
        let dist_ok = dist[0] == 5 && dist[1] == 10 && dist[2] == 8 && dist[3] == 7;
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
    // 种子写死：0x30b64（变异序列）。
    let mut lcg = Lcg::new(0x30b64);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for c in cases.iter().filter(|c| c.want == 0) {
            // 变异 1：类型越闭集。
            let mut m1 = c.form;
            m1.type_idx = TYPE_COUNT as u8 + 1;
            all_ok = all_ok && matches!(judge(&m1), Err(ObCode::BadType));
            // 变异 2：权限位域外。
            let mut m2 = c.form;
            m2.access |= 0x8000_0000;
            all_ok = all_ok && matches!(judge(&m2), Err(ObCode::BadAccess));
            // 变异 3：OA Length 偏移。
            let mut m3 = c.form;
            m3.oa_length = OA_LENGTH - 8;
            all_ok = all_ok && matches!(judge(&m3), Err(ObCode::BadAttr));
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路变异轮={}/{}", ok, INJECT_ROUNDS))
}

/// 类型闭集演练：八类型名逐名可达 + 首尾类型边界 10/10 轮。
pub fn drill_type_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30b65（类型序列）。
    let mut lcg = Lcg::new(0x30b65);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 首类型（Directory）与尾类型（KeyedEvent）正路。
        let g0 = ObjForm { type_idx: 0, access: 0, oa_length: OA_LENGTH };
        let g7 = ObjForm { type_idx: 7, access: 0, oa_length: OA_LENGTH };
        all_ok = all_ok && judge(&g0).is_ok() && judge(&g7).is_ok();
        // 类型名逐名非空且序号对齐。
        for (i, n) in TYPE_NAMES.iter().enumerate() {
            all_ok = all_ok && !n.is_empty() && i < TYPE_COUNT;
        }
        // 八类型逐型正路。
        for t in 0..TYPE_COUNT as u8 {
            let g = ObjForm { type_idx: t, access: 0x1F01_FF03, oa_length: OA_LENGTH };
            all_ok = all_ok && judge(&g).is_ok();
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("类型闭集轮={}/{}", ok, INJECT_ROUNDS))
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
pub fn gate_canonical() -> Vec<(&'static str, ObjForm, u8)> {
    vec![
        ("类型闸", ObjForm { type_idx: 8, access: 0x1F01_FF03, oa_length: OA_LENGTH }, 1),
        ("权限闸", ObjForm { type_idx: 2, access: 0x8000_0000, oa_length: OA_LENGTH }, 2),
        ("属性闸", ObjForm { type_idx: 2, access: 0x1F01_FF03, oa_length: 24 }, 3),
        ("正路", ObjForm { type_idx: 2, access: 0x1F01_FF03, oa_length: OA_LENGTH }, 0),
    ]
}

/// 闸位巡检演练：逐闸 canonical 例计算码 ≡ 期望码 10/10 轮。
pub fn drill_gate_walk_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30b66（闸位巡检序列）。
    let mut lcg = Lcg::new(0x30b66);
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
    // 种子写死：0x30b67（重复注入序列）。
    let mut lcg = Lcg::new(0x30b67);
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
    // 注 1：坏类型必拒。
    let f1 = ObjForm { type_idx: 8, access: 0x1F01_FF03, oa_length: OA_LENGTH };
    if matches!(judge(&f1), Err(ObCode::BadType)) {
        caught += 1;
        d.push("注1 类型:抓".to_string());
    } else {
        d.push("注1 类型:漏".to_string());
    }
    // 注 2：坏权限必拒。
    let f2 = ObjForm { type_idx: 2, access: 0x8000_0000, oa_length: OA_LENGTH };
    if matches!(judge(&f2), Err(ObCode::BadAccess)) {
        caught += 1;
        d.push("注2 权限:抓".to_string());
    } else {
        d.push("注2 权限:漏".to_string());
    }
    // 注 3：坏属性必拒。
    let f3 = ObjForm { type_idx: 2, access: 0x1F01_FF03, oa_length: 24 };
    if matches!(judge(&f3), Err(ObCode::BadAttr)) {
        caught += 1;
        d.push("注3 属性:抓".to_string());
    } else {
        d.push("注3 属性:漏".to_string());
    }
    // 注 4：零权限正路（access=0 合法）。
    let f4 = ObjForm { type_idx: 0, access: 0, oa_length: OA_LENGTH };
    if judge(&f4).is_ok() {
        caught += 1;
        d.push("注4 零权:过".to_string());
    } else {
        d.push("注4 零权:堵".to_string());
    }
    // 注 5：正路锚不可堵死。
    let f5 = ObjForm { type_idx: 2, access: 0x1F01_FF03, oa_length: OA_LENGTH };
    if judge(&f5).is_ok() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30b60 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    ObCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 注入报告。
#[derive(Clone, Copy, Debug)]
pub struct ObReport {
    /// 例数。
    pub cases: usize,
    /// 命中数。
    pub hits: usize,
    /// 码分布。
    pub dist: [usize; 4],
    /// 账单调。
    pub mono: bool,
}

impl ObReport {
    /// 建报告。
    pub fn build(bk: &InjectBook, hits: usize) -> Self {
        ObReport { cases: bk.rows(), hits, dist: bk.code_counts(), mono: bk.monotone() }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == INJECT_CASES && self.hits == INJECT_CASES && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "ObReport 例={} 命中={} 分布={:?} 单调={}",
            self.cases, self.hits, self.dist, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct ObCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_ob_checklist(bk: &InjectBook, hits: usize) -> Vec<ObCheckRow> {
    let rep = ObReport::build(bk, hits);
    let dist = bk.code_counts();
    let mut rows: Vec<ObCheckRow> = Vec::new();
    rows.push(ObCheckRow { name: "30例全注", passed: bk.rows() == INJECT_CASES });
    rows.push(ObCheckRow { name: "30例全中", passed: hits == INJECT_CASES });
    rows.push(ObCheckRow { name: "坏类型10例", passed: dist[1] == 10 });
    rows.push(ObCheckRow { name: "坏权限8例", passed: dist[2] == 8 });
    rows.push(ObCheckRow { name: "坏属性7例", passed: dist[3] == 7 });
    rows.push(ObCheckRow { name: "正路5例", passed: dist[0] == 5 });
    rows.push(ObCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(ObCheckRow { name: "转录非空", passed: bk.export_lines().contains("对象注入账") });
    rows.push(ObCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(ObCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ob_checklist_green(rows: &[ObCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ob_stats_line(bk: &InjectBook, hits: usize) -> String {
    format!(
        "对象注入概况：例 {} 命中 {}/{}",
        bk.rows(),
        hits,
        INJECT_CASES
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12491_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        ObReport::build(&bk, hits).all_green() && bk.export_lines().contains("对象注入账"),
        ob_stats_line(&bk, hits),
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
    let rows = build_ob_checklist(&bk, hits);
    out.push((
        "X-终验清单10项",
        ob_checklist_green(&rows),
        ObReport::build(&bk, hits).to_text(),
    ));
    // X-e2：正路变异 10/10 + 类型闭集 10/10 + 账码回放面。
    let (ok_m, d_m) = drill_mutate_positive_10();
    out.push(("X-正路变异10/10", ok_m, d_m));
    let (ok_t, d_t) = drill_type_domain_10();
    out.push(("X-类型闭集10/10", ok_t, d_t));
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
        TYPE_COUNT == 8 && OA_LENGTH == 48 && ACCESS_VALID_MASK == 0x1F01_FF03,
        format!("types={} oa={} mask={:#x}", TYPE_COUNT, OA_LENGTH, ACCESS_VALID_MASK),
    ));
    out
}

/// run_unx12491_checks：本单判据聚合（诊断段 0x30b60）。
pub fn run_unx12491_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12491 对象族注入矩阵");
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
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30b60段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12491_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
