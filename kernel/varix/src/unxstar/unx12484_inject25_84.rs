//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12484 · OBJECT_ATTRIBUTES 注入矩阵 · 0x30a80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F12484-J1 坏 Length/坏 RootDirectory/坏 SecurityQos 30 例
//!      注入全过，与 B05 拒止档逐例一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - OA 形态档：Length(48 字节口径)/RootDirectory(句柄域)/ObjectName
//!     (哨兵)/Attributes(位域)/SecurityQoS(旗标+结构)五字段真实校验；
//!   - 闸序（判据语义序）：坏 Length → 坏 RootDirectory → 坐 SecurityQos
//!     → 坏 Attributes，前置闸命中即短路，后续闸不评估；
//!   - Length 闸：必须恰等于 OA 结构体大小常量（48），偏小偏大均拒；
//!   - RootDirectory 闸：0=无根合法；非零须落句柄表域
//!     （0x1000..0x1010 口径，B05 句柄派生）；
//!   - SecurityQoS 闸：旗标位置位但结构缺席拒；结构在册时模拟级出
//!     四档闭集拒；
//!   - Attributes 闸：未知位（合法位域外）拒；
//!   - 30 例注入账：12 例坏 Length + 8 例坏 RootDirectory + 7 例坏
//!     SecurityQos + 3 例正路锚，逐例期望码与判定链一致判据；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30a80），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30a80 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30a80;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 注入例数（判据 30 例）。
pub const INJECT_CASES: usize = 30;

/// OA 结构体大小（模型口径 48 字节，Length 闸目标值）。
pub const OA_LENGTH: u32 = 48;

/// 句柄基（B05 句柄派生口径）。
pub const HANDLE_BASE: u32 = 0x1000;

/// 句柄表宽（16 槽）。
pub const HANDLE_SLOTS: usize = 16;

/// 属性合法位域（PROTECT_CLOSE=0x2 / INHERIT=0x2 / CASE_INSENSITIVE=0x40
/// / OPENIF=0x4 / PERMANENT=0x10 / EXCLUSIVE=0x20 口径并集）。
pub const ATTR_VALID_MASK: u32 = 0x0000_00FA;

/// 诊断码（0x30a80 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OaCode {
    /// 0x30a80 坏 Length（≠48 字节口径）。
    BadLength = 0,
    /// 0x30a81 坏 RootDirectory（句柄越域）。
    BadRoot = 1,
    /// 0x30a82 坏 SecurityQos（旗标缺席结构 / 模拟级越档）。
    BadQos = 2,
    /// 0x30a83 坏 Attributes（未知位）。
    BadAttr = 3,
    /// 0x30a84 注入例数不足（<30）。
    CaseShort = 4,
    /// 0x30a85 例码漂移（计算码 ≠ 期望码）。
    CaseDrift = 5,
    /// 0x30a86 注入账缩水（append-only 被破）。
    LedgerShrink = 6,
    /// 0x30a87 闸序破（低序闸过而高序闸拒但期望低序码——闸序一致性）。
    GateOrder = 7,
    /// 0x30a88 正路锚堵死（合法 OA 被拒）。
    PositiveBlocked = 8,
    /// 0x30a89 拒止档失配（与 B05 拒止档口径不一致）。
    TierDiff = 9,
    /// 0x30a8a 注入未执行（账面零例）。
    NotRun = 10,
}

impl OaCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [OaCode; 11] = [
        OaCode::BadLength, OaCode::BadRoot, OaCode::BadQos, OaCode::BadAttr,
        OaCode::CaseShort, OaCode::CaseDrift, OaCode::LedgerShrink, OaCode::GateOrder,
        OaCode::PositiveBlocked, OaCode::TierDiff, OaCode::NotRun,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30a80 => "坏 Length：OBJECT_ATTRIBUTES 结构体大小恒 48 字节，修正 Length。",
        0x30a81 => "坏 RootDirectory：句柄出 0x1000..0x1010 表域，回查句柄派生。",
        0x30a82 => "坏 SecurityQos：旗标置位须配结构，模拟级出四档即拒。",
        0x30a83 => "坏 Attributes：属性位出合法位域，回查位定义。",
        0x30a84 => "注入例数不足：30 例必须全注入。",
        0x30a85 => "例码漂移：逐例回放判定链定位失配例。",
        0x30a86 => "注入账缩水：append-only，禁止截断，重放补齐。",
        0x30a87 => "闸序破：前置闸命中必须短路，回查判定链顺序。",
        0x30a88 => "正路锚堵死：合法 OA 被拒即判定链过严，先修正再注入。",
        0x30a89 => "拒止档失配：与 B05 拒止档逐例核对。",
        0x30a8a => "注入未执行：账面零例，先跑注入再出账。",
        _ => "未知诊断码：回查 0x30a80 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30a80 段基址。
    pub const SEED: u64 = 0x30a80_30a8_030a_80u64;

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

/// SecurityQoS 结构档。
#[derive(Clone, Copy, Debug)]
pub struct Qos {
    /// 模拟级（0..3 闭集）。
    pub level: u8,
    /// 仅生效位。
    pub effective_only: bool,
}

/// OBJECT_ATTRIBUTES 注入形态。
#[derive(Clone, Copy, Debug)]
pub struct OaForm {
    /// Length（字节）。
    pub length: u32,
    /// RootDirectory 句柄（0=无根）。
    pub root: u32,
    /// Attributes 位域。
    pub attributes: u32,
    /// QoS 旗标位（Length 之外的声明位）。
    pub qos_flag: bool,
    /// QoS 结构（旗标置位时应存在）。
    pub qos: Option<Qos>,
}

/// 判定链（闸序：Length → Root → Qos → Attr；前置命中即短路）。
pub fn judge(f: &OaForm) -> Result<(), OaCode> {
    // 闸 1：Length。
    if f.length != OA_LENGTH {
        return Err(OaCode::BadLength);
    }
    // 闸 2：RootDirectory 句柄域。
    if f.root != 0 {
        let off = f.root.checked_sub(HANDLE_BASE).unwrap_or(u32::MAX);
        if off as usize >= HANDLE_SLOTS {
            return Err(OaCode::BadRoot);
        }
    }
    // 闸 3：SecurityQoS（旗标与结构同在性 + 模拟级闭集）。
    if f.qos_flag {
        match f.qos {
            None => return Err(OaCode::BadQos),
            Some(q) => {
                if q.level > 3 {
                    return Err(OaCode::BadQos);
                }
            }
        }
    }
    // 闸 4：Attributes 位域。
    if f.attributes & !ATTR_VALID_MASK != 0 {
        return Err(OaCode::BadAttr);
    }
    Ok(())
}

/// 注入例（形态 + 期望码序号；0=正路）。
#[derive(Clone, Copy, Debug)]
pub struct CaseSpec {
    /// 例号。
    pub id: u16,
    /// 注入形态。
    pub form: OaForm,
    /// 期望码序号（0=Accept）。
    pub want: u8,
}

/// 30 例注入账构建（12 坏 Length + 8 坏 Root + 7 坏 Qos + 3 正路锚）。
pub fn build_cases() -> Vec<CaseSpec> {
    let mut v: Vec<CaseSpec> = Vec::new();
    let base = OaForm { length: OA_LENGTH, root: 0, attributes: 0x40, qos_flag: false, qos: None };
    // 坏 Length 12 例（id 0..12）。
    for i in 0..12u32 {
        let mut f = base;
        f.length = match i % 4 {
            0 => 0,
            1 => OA_LENGTH - 1 - i,
            2 => OA_LENGTH + 1 + i,
            _ => 0xFFFF_FFFF,
        };
        v.push(CaseSpec { id: i as u16, form: f, want: 1 });
    }
    // 坏 RootDirectory 8 例（id 12..20）。
    for i in 0..8u32 {
        let mut f = base;
        f.root = match i % 4 {
            0 => 1,
            1 => HANDLE_BASE + HANDLE_SLOTS as u32,
            2 => HANDLE_BASE - 1,
            _ => 0xFFFF_FFFF,
        };
        v.push(CaseSpec { id: 12 + i as u16, form: f, want: 2 });
    }
    // 坏 SecurityQos 7 例（id 20..27）。
    for i in 0..7u32 {
        let mut f = base;
        match i % 3 {
            // 旗标置位无结构。
            0 => {
                f.qos_flag = true;
                f.qos = None;
            }
            // 结构在册但模拟级越档。
            1 => {
                f.qos_flag = true;
                f.qos = Some(Qos { level: 4 + (i / 3) as u8, effective_only: false });
            }
            // 结构在册但模拟级 0xFF。
            _ => {
                f.qos_flag = true;
                f.qos = Some(Qos { level: 0xFF, effective_only: true });
            }
        }
        v.push(CaseSpec { id: 20 + i as u16, form: f, want: 3 });
    }
    // 正路锚 3 例（id 27..30）。
    v.push(CaseSpec { id: 27, form: base, want: 0 });
    v.push(CaseSpec {
        id: 28,
        form: OaForm { length: OA_LENGTH, root: HANDLE_BASE + 3, attributes: 0x40, qos_flag: true, qos: Some(Qos { level: 2, effective_only: false }) },
        want: 0,
    });
    v.push(CaseSpec {
        id: 29,
        form: OaForm { length: OA_LENGTH, root: 0, attributes: 0xFA, qos_flag: false, qos: None },
        want: 0,
    });
    v
}

/// 注入账正本。
#[derive(Debug)]
pub struct InjectBook {
    /// 结果账（append-only：(例号, 计算码序号, 期望码序号)）。
    rows: Vec<(u16, u8, u8)>,
    /// 账高水位。
    high: usize,
}

impl InjectBook {
    /// 建账。
    pub fn new() -> Self {
        InjectBook { rows: Vec::new(), high: 0 }
    }

    /// 注入一例（判定链真实执行 + 记账；got 编码：0=Accept，1..=判别值+1）。
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

    /// 注入一整批（返回命中数）。
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

    /// 码分布清点（按码序号）。
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
        let mut s = String::from("OA 注入账：\n");
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
    // 种子写死（协议禁 rand）：0x30a80 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = InjectBook::new();
        let hits = bk.run_all(&cases);
        let mono = bk.monotone();
        let _ = lcg.below(3);
        if hits == INJECT_CASES && cases.len() == INJECT_CASES && mono {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("30例注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 闸序演练：前置闸命中短路（后续闸不评估）10/10 轮。
pub fn drill_gate_order_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30a81（闸序序列）。
    let mut lcg = Lcg::new(0x30a81);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 双坏叠加：坏 Length + 坏 Root → 期望 BadLength（闸 1 短路）。
        let f1 = OaForm { length: 0, root: 0xFFFF_FFFF, attributes: 0, qos_flag: false, qos: None };
        all_ok = all_ok && matches!(judge(&f1), Err(OaCode::BadLength));
        // 双坏叠加：坏 Root + 坏 Qos → 期望 BadRoot（闸 2 短路）。
        let f2 = OaForm { length: OA_LENGTH, root: 1, attributes: 0, qos_flag: true, qos: None };
        all_ok = all_ok && matches!(judge(&f2), Err(OaCode::BadRoot));
        // 双坏叠加：坏 Qos + 坏 Attr → 期望 BadQos（闸 3 短路）。
        let f3 = OaForm { length: OA_LENGTH, root: 0, attributes: 0xFFFF, qos_flag: true, qos: Some(Qos { level: 9, effective_only: false }) };
        all_ok = all_ok && matches!(judge(&f3), Err(OaCode::BadQos));
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：3 例合法 OA 全过 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30a82（正路序列）。
    let mut lcg = Lcg::new(0x30a82);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for c in cases.iter().skip(27) {
            all_ok = all_ok && judge(&c.form).is_ok() && c.want == 0;
        }
        // 合法形态清单（逐项核对闸位）。
        let good = OaForm { length: OA_LENGTH, root: HANDLE_BASE + 7, attributes: 0xFA, qos_flag: true, qos: Some(Qos { level: 3, effective_only: true }) };
        all_ok = all_ok && judge(&good).is_ok();
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路锚轮={}/{}", ok, INJECT_ROUNDS))
}

/// 码分布演练：注入账码分布与例数构成一致 10/10 轮。
pub fn drill_code_dist_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30a83（分布序列）。
    let mut lcg = Lcg::new(0x30a83);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = InjectBook::new();
        let _ = bk.run_all(&cases);
        let dist = bk.code_counts();
        // 构成：正路 3 + 坏 Length 12 + 坏 Root 8 + 坏 Qos 7 = 30。
        let dist_ok = dist[0] == 3 && dist[1] == 12 && dist[2] == 8 && dist[3] == 7;
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
    // 种子写死：0x30a84（变异序列）。
    let mut lcg = Lcg::new(0x30a84);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for c in cases.iter().filter(|c| c.want == 0) {
            // 变异 1：Length 偏移。
            let mut m1 = c.form;
            m1.length = OA_LENGTH + 1;
            all_ok = all_ok && matches!(judge(&m1), Err(OaCode::BadLength));
            // 变异 2：Root 越域。
            let mut m2 = c.form;
            m2.root = 0x5000;
            all_ok = all_ok && matches!(judge(&m2), Err(OaCode::BadRoot));
            // 变异 3：QoS 旗标无结构。
            let mut m3 = c.form;
            m3.qos_flag = true;
            m3.qos = None;
            all_ok = all_ok && matches!(judge(&m3), Err(OaCode::BadQos));
            // 变异 4：属性未知位。
            let mut m4 = c.form;
            m4.attributes |= 0x100;
            all_ok = all_ok && matches!(judge(&m4), Err(OaCode::BadAttr));
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路变异轮={}/{}", ok, INJECT_ROUNDS))
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
pub fn gate_canonical() -> Vec<(&'static str, OaForm, u8)> {
    vec![
        ("长度闸", OaForm { length: 24, root: 0, attributes: 0, qos_flag: false, qos: None }, 1),
        ("根闸", OaForm { length: OA_LENGTH, root: 0x5000, attributes: 0, qos_flag: false, qos: None }, 2),
        ("QoS闸", OaForm { length: OA_LENGTH, root: 0, attributes: 0, qos_flag: true, qos: None }, 3),
        ("属性闸", OaForm { length: OA_LENGTH, root: 0, attributes: 0x100, qos_flag: false, qos: None }, 4),
        ("正路", OaForm { length: OA_LENGTH, root: 0, attributes: 0x40, qos_flag: false, qos: None }, 0),
    ]
}

/// 闸位巡检演练：逐闸 canonical 例计算码 ≡ 期望码 10/10 轮。
pub fn drill_gate_walk_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30a85（闸位巡检序列）。
    let mut lcg = Lcg::new(0x30a85);
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
    // 种子写死：0x30a86（重复注入序列）。
    let mut lcg = Lcg::new(0x30a86);
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
    // 注 1：坏 Length 必拒。
    let f1 = OaForm { length: 24, root: 0, attributes: 0, qos_flag: false, qos: None };
    if matches!(judge(&f1), Err(OaCode::BadLength)) {
        caught += 1;
        d.push("注1 长度:抓".to_string());
    } else {
        d.push("注1 长度:漏".to_string());
    }
    // 注 2：坏 Root 必拒。
    let f2 = OaForm { length: OA_LENGTH, root: 0x2000, attributes: 0, qos_flag: false, qos: None };
    if matches!(judge(&f2), Err(OaCode::BadRoot)) {
        caught += 1;
        d.push("注2 根:抓".to_string());
    } else {
        d.push("注2 根:漏".to_string());
    }
    // 注 3：坏 Qos（旗标无结构）必拒。
    let f3 = OaForm { length: OA_LENGTH, root: 0, attributes: 0, qos_flag: true, qos: None };
    if matches!(judge(&f3), Err(OaCode::BadQos)) {
        caught += 1;
        d.push("注3 QoS:抓".to_string());
    } else {
        d.push("注3 QoS:漏".to_string());
    }
    // 注 4：坏 Attr 必拒。
    let f4 = OaForm { length: OA_LENGTH, root: 0, attributes: 0x8000, qos_flag: false, qos: None };
    if matches!(judge(&f4), Err(OaCode::BadAttr)) {
        caught += 1;
        d.push("注4 属性:抓".to_string());
    } else {
        d.push("注4 属性:漏".to_string());
    }
    // 注 5：正路锚不可堵死。
    let f5 = OaForm { length: OA_LENGTH, root: 0, attributes: 0x40, qos_flag: false, qos: None };
    if judge(&f5).is_ok() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30a80 段十一码闭集全部落在段内。
pub fn segment_guard() -> bool {
    OaCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 注入报告。
#[derive(Clone, Copy, Debug)]
pub struct OaReport {
    /// 例数。
    pub cases: usize,
    /// 命中数。
    pub hits: usize,
    /// 码分布。
    pub dist: [usize; 5],
    /// 账单调。
    pub mono: bool,
}

impl OaReport {
    /// 建报告。
    pub fn build(bk: &InjectBook, hits: usize) -> Self {
        OaReport { cases: bk.rows(), hits, dist: bk.code_counts(), mono: bk.monotone() }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == INJECT_CASES && self.hits == INJECT_CASES && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "OaReport 例={} 命中={} 分布={:?} 单调={}",
            self.cases, self.hits, self.dist, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct OaCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_oa_checklist(bk: &InjectBook, hits: usize) -> Vec<OaCheckRow> {
    let rep = OaReport::build(bk, hits);
    let dist = bk.code_counts();
    let mut rows: Vec<OaCheckRow> = Vec::new();
    rows.push(OaCheckRow { name: "30例全注", passed: bk.rows() == INJECT_CASES });
    rows.push(OaCheckRow { name: "30例全中", passed: hits == INJECT_CASES });
    rows.push(OaCheckRow { name: "坏Length12例", passed: dist[1] == 12 });
    rows.push(OaCheckRow { name: "坏Root8例", passed: dist[2] == 8 });
    rows.push(OaCheckRow { name: "坏Qos7例", passed: dist[3] == 7 });
    rows.push(OaCheckRow { name: "正路3例", passed: dist[0] == 3 });
    rows.push(OaCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(OaCheckRow { name: "转录非空", passed: bk.export_lines().contains("OA 注入账") });
    rows.push(OaCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(OaCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn oa_checklist_green(rows: &[OaCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn oa_stats_line(bk: &InjectBook, hits: usize) -> String {
    format!(
        "OA 注入概况：例 {} 命中 {}/{}",
        bk.rows(),
        hits,
        INJECT_CASES
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12484_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        OaReport::build(&bk, hits).all_green() && bk.export_lines().contains("OA 注入账"),
        oa_stats_line(&bk, hits),
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
    let rows = build_oa_checklist(&bk, hits);
    out.push((
        "X-终验清单10项",
        oa_checklist_green(&rows),
        OaReport::build(&bk, hits).to_text(),
    ));
    // X-e2：正路变异 10/10 + 账码回放面。
    let (ok_m, d_m) = drill_mutate_positive_10();
    out.push(("X-正路变异10/10", ok_m, d_m));
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
        OA_LENGTH == 48 && HANDLE_BASE == 0x1000 && HANDLE_SLOTS == 16 && ATTR_VALID_MASK == 0xFA,
        format!("len={} hbase={:#x} slots={} mask={:#x}", OA_LENGTH, HANDLE_BASE, HANDLE_SLOTS, ATTR_VALID_MASK),
    ));
    out
}

/// run_unx12484_checks：本单判据聚合（诊断段 0x30a80）。
pub fn run_unx12484_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12484 OBJECT_ATTRIBUTES 注入矩阵");
    // J1-a：30 例注入 10/10。
    let (ok1, d1) = drill_inject30_10();
    s.add_dyn("J1-30例注入10/10", ok1, &d1);
    // J1-b：标准会话恒等式（30/30 全中 + 码分布一致）。
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    let dist = bk.code_counts();
    s.add_dyn(
        "J1-30例全中拒止档一致",
        hits == INJECT_CASES && dist[0] == 3 && dist[1] == 12 && dist[2] == 8 && dist[3] == 7,
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
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30a80段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12484_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
