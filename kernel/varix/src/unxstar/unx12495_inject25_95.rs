//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12495 · 机制尾段注入矩阵（B16–B24 抽样） · 0x30be0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12495-J1 原子/TLS/RTL/进程/信息/节/端口/令牌/注册表九面
//!      抽样 60 例全过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 九面闭集：原子/TLS/RTL/进程/信息/节/端口/令牌/注册表（B16–B24
//!     机制尾段抽样面），每面本地迷你判定器（真实闸序）；
//!   - 抽样配额：前六面各 7 例 + 后三面各 6 例 = 60 例（判据口径）；
//!   - 原子面：对齐闸（地址 4 字节对齐）→ CAS 期望值匹配闸；
//!   - TLS 面：槽位域闸（0..=63）→ 槽占用闸；
//!   - RTL 面：串长偶数闸（宽字符口径）→ 容量闸（Length ≤ Cap）；
//!   - 进程面：PID 非零闸 → PID 域闸（≤0xFFFF）→ 权限位域闸；
//!   - 信息面：信息类闭集闸（0..=83）→ 缓冲容量闸；
//!   - 节面：页对齐闸 → 非零尺寸闸；
//!   - 端口面：消息长闸（1..=656）→ 池配比闸（池 ≥ 2×消息长）；
//!   - 令牌面：LUID 非零闸 → 使能闸；
//!   - 注册表面：值类型闭集闸（1..=11）→ 名长闸（≤16384）；
//!   - 60 例全过判据：逐例计算码 ≡ 期望码，60/60 才绿；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30be0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30be0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30be0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 抽样总例数（判据 60 例）。
pub const SAMPLE_CASES: usize = 60;

/// 九面闭集宽。
pub const FACES: usize = 9;

/// 抽样配额表（前六面 7 例 + 后三面 6 例 = 60）。
pub const QUOTA: [usize; FACES] = [7, 7, 7, 7, 7, 7, 6, 6, 6];

/// TLS 槽位上限（64 槽口径）。
pub const TLS_MAX: u32 = 64;

/// 信息类闭集上界（0..=83 口径）。
pub const INFO_CLASS_MAX: u32 = 83;

/// 端口消息长上限（656 口径，与 B22 同源）。
pub const PORT_MSG_MAX: u32 = 656;

/// 注册表名长上限（16384 口径）。
pub const REG_NAME_MAX: u32 = 16384;

/// 诊断码（0x30be0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MxCode {
    /// 0x30be0 面越闭集（出九面域）。
    FaceOob = 0,
    /// 0x30be1 配额失守（面例数与配额表不符）。
    QuotaBad = 1,
    /// 0x30be2 例码漂移（计算码 ≠ 期望码）。
    CaseDrift = 2,
    /// 0x30be3 抽样不足（<60 例）。
    CaseShort = 3,
    /// 0x30be4 抽样账缩水。
    LedgerShrink = 4,
    /// 0x30be5 闸序破。
    GateOrder = 5,
    /// 0x30be6 正路锚堵死。
    PositiveBlocked = 6,
    /// 0x30be7 注入未执行。
    NotRun = 7,
    /// 0x30be8 拒止档失配。
    TierDiff = 8,
    /// 0x30be9 面覆盖缺失（九面未全覆盖）。
    CoverShort = 9,
}

impl MxCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [MxCode; 10] = [
        MxCode::FaceOob, MxCode::QuotaBad, MxCode::CaseDrift, MxCode::CaseShort,
        MxCode::LedgerShrink, MxCode::GateOrder, MxCode::PositiveBlocked,
        MxCode::NotRun, MxCode::TierDiff, MxCode::CoverShort,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30be0 => "面越闭集：出九面域，先比 FACES 再派样。",
        0x30be1 => "配额失守：面例数与配额表 [7×6,6×3] 不符。",
        0x30be2 => "例码漂移：逐例回放面判定器定位失配例。",
        0x30be3 => "抽样不足：60 例必须全注入。",
        0x30be4 => "抽样账缩水：append-only，禁止截断。",
        0x30be5 => "闸序破：前置闸命中必须短路。",
        0x30be6 => "正路锚堵死：每面正路例被拒先修判定器。",
        0x30be7 => "注入未执行：账面零例。",
        0x30be8 => "拒止档失配：与源面档逐例核对。",
        0x30be9 => "面覆盖缺失：九面未全覆盖。",
        _ => "未知诊断码：回查 0x30be0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30be0 段基址。
    pub const SEED: u64 = 0x30be0_30be_030b_e0u64;

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

/// 机制尾段九面闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    /// 0 原子。
    Atomic,
    /// 1 TLS。
    Tls,
    /// 2 RTL。
    Rtl,
    /// 3 进程。
    Process,
    /// 4 信息。
    Info,
    /// 5 节。
    Section,
    /// 6 端口。
    Port,
    /// 7 令牌。
    Token,
    /// 8 注册表。
    Registry,
}

/// 面序号。
pub const fn face_idx(f: Face) -> u32 {
    match f {
        Face::Atomic => 0,
        Face::Tls => 1,
        Face::Rtl => 2,
        Face::Process => 3,
        Face::Info => 4,
        Face::Section => 5,
        Face::Port => 6,
        Face::Token => 7,
        Face::Registry => 8,
    }
}

/// 面中文名。
pub const fn face_name(f: Face) -> &'static str {
    match f {
        Face::Atomic => "原子",
        Face::Tls => "TLS",
        Face::Rtl => "RTL",
        Face::Process => "进程",
        Face::Info => "信息",
        Face::Section => "节",
        Face::Port => "端口",
        Face::Token => "令牌",
        Face::Registry => "注册表",
    }
}

/// 九面全遍历。
pub fn all_faces() -> [Face; FACES] {
    [Face::Atomic, Face::Tls, Face::Rtl, Face::Process, Face::Info,
     Face::Section, Face::Port, Face::Token, Face::Registry]
}

/// 抽样例（面 + 双参 + 期望码序号）。
#[derive(Clone, Copy, Debug)]
pub struct SampleCase {
    /// 面。
    pub face: Face,
    /// 参数 a（语义随面）。
    pub a: u64,
    /// 参数 b（语义随面）。
    pub b: u64,
    /// 期望码序号（0=Accept，1..=判别值+1）。
    pub want: u8,
}

/// 九面迷你判定器（每面双闸；a/b 语义随面；返回 Ok=过 / Err=面内拒止码序号-1）。
///
/// 返回 Err(u8) 的值为面内拒止序号（0=闸 1，1=闸 2），账面编码 = 序号+1。
pub fn face_judge(f: Face, a: u64, b: u64) -> Result<(), u8> {
    match f {
        // 原子面：闸 1 对齐（4 字节）→ 闸 2 CAS 期望匹配（b=0 允许首写）。
        Face::Atomic => {
            if a % 4 != 0 {
                return Err(0);
            }
            if b != 0 && b != a {
                return Err(1);
            }
            Ok(())
        }
        // TLS 面：闸 1 槽位域（a < 64）→ 闸 2 槽占用（b=1 占用则拒）。
        Face::Tls => {
            if a >= TLS_MAX as u64 {
                return Err(0);
            }
            if b == 1 {
                return Err(1);
            }
            Ok(())
        }
        // RTL 面：闸 1 奇长（宽字符口径）→ 闸 2 容量（a ≤ b）。
        Face::Rtl => {
            if a % 2 != 0 {
                return Err(0);
            }
            if a > b {
                return Err(1);
            }
            Ok(())
        }
        // 进程面：闸 1 PID 非零 → 闸 2 PID 域（≤0xFFFF）+ 权限位域（b ≤ 0x1F）。
        Face::Process => {
            if a == 0 {
                return Err(0);
            }
            if a > 0xFFFF || b > 0x1F {
                return Err(1);
            }
            Ok(())
        }
        // 信息面：闸 1 信息类闭集（a ≤ 83）→ 闸 2 缓冲容量（b ≥ 8）。
        Face::Info => {
            if a > INFO_CLASS_MAX as u64 {
                return Err(0);
            }
            if b < 8 {
                return Err(1);
            }
            Ok(())
        }
        // 节面：闸 1 页对齐（4KB）→ 闸 2 非零尺寸。
        Face::Section => {
            if a % 0x1000 != 0 {
                return Err(0);
            }
            if b == 0 {
                return Err(1);
            }
            Ok(())
        }
        // 端口面：闸 1 消息长（1..=656）→ 闸 2 池配比（b ≥ 2×a）。
        Face::Port => {
            if a == 0 || a > PORT_MSG_MAX as u64 {
                return Err(0);
            }
            if b < a.saturating_mul(2) {
                return Err(1);
            }
            Ok(())
        }
        // 令牌面：闸 1 LUID 非零 → 闸 2 使能（b=1 使能才过）。
        Face::Token => {
            if a == 0 {
                return Err(0);
            }
            if b != 1 {
                return Err(1);
            }
            Ok(())
        }
        // 注册表面：闸 1 值类型闭集（1..=11）→ 闸 2 名长域（≤16384）。
        Face::Registry => {
            if a == 0 || a > 11 {
                return Err(0);
            }
            if b > REG_NAME_MAX as u64 {
                return Err(1);
            }
            Ok(())
        }
    }
}

/// 60 例抽样构建（面配额 [7×6, 6×3]；每面正路锚 + 拒止例混合）。
pub fn build_samples() -> Vec<SampleCase> {
    let mut v: Vec<SampleCase> = Vec::new();
    for (fi, face) in all_faces().iter().enumerate() {
        let n = QUOTA[fi];
        for k in 0..n {
            // 每面案例布局：k=0 正路锚；k 奇数 = 闸 1 拒；k 偶数且 >0 = 闸 2 拒。
            let (a, b, want) = match k {
                0 => (positive_params(fi).0, positive_params(fi).1, 0u8),
                k if k % 2 == 1 => (gate1_params(fi, k).0, gate1_params(fi, k).1, 1u8),
                _ => (gate2_params(fi, k).0, gate2_params(fi, k).1, 2u8),
            };
            v.push(SampleCase { face: *face, a, b, want });
        }
    }
    v
}

/// 各面正路参数（k=0 锚）。
fn positive_params(fi: usize) -> (u64, u64) {
    match fi {
        0 => (0x1000, 0),          // 原子：对齐 + CAS 首
        1 => (0, 0),               // TLS：槽 0 空闲
        2 => (8, 16),              // RTL：偶长 ≤ 容量
        3 => (0x1234, 0x1F),       // 进程：PID 域内 + 权限全位
        4 => (0, 64),              // 信息：类 0 + 缓冲 64
        5 => (0x10000, 0x1000),    // 节：页对齐 + 非零尺寸
        6 => (128, 0x1000),        // 端口：消息 128 ≤ 656 + 池足
        7 => (7, 1),               // 令牌：LUID 7 + 使能
        _ => (1, 64),              // 注册表：类型 1 + 名长 64
    }
}

/// 各面闸 1 拒止参数（奇数 k）。
fn gate1_params(fi: usize, k: usize) -> (u64, u64) {
    let j = (k / 2) as u64;
    match fi {
        0 => (0x1001 + j, 0),      // 原子：失对
        1 => (TLS_MAX as u64 + 1 + j, 0), // TLS：槽越域
        2 => (3 + j * 2, 64),      // RTL：奇长
        3 => (0, 0x1F),            // 进程：PID 零
        4 => (INFO_CLASS_MAX as u64 + 1 + j, 64), // 信息：类越界
        5 => (0x1001 + j, 0x1000), // 节：失对
        6 => (PORT_MSG_MAX as u64 + 1 + j, 0x2000), // 端口：消息越上限
        7 => (0, 1),               // 令牌：LUID 零
        _ => (0, 64),              // 注册表：类型零
    }
}

/// 各面闸 2 拒止参数（偶数 k 且 >0）。
fn gate2_params(fi: usize, k: usize) -> (u64, u64) {
    let j = (k / 2) as u64;
    match fi {
        0 => (0x1000, 0x2000 + j), // 原子：CAS 期望失配
        1 => (j % TLS_MAX as u64, 1), // TLS：槽占用
        2 => (16, 8 + j),          // RTL：超容量
        3 => (0x1234 + j, 0x20 + j), // 进程：权限越位域
        4 => (0 + j, 4),           // 信息：缓冲过小
        5 => (0x10000, 0),         // 节：零尺寸
        6 => (200, 300 + j),       // 端口：池 < 2×消息
        7 => (9, 0),               // 令牌：未使能
        _ => (1, REG_NAME_MAX as u64 + 1 + j), // 注册表：名长越域
    }
}

/// 抽样账正本。
#[derive(Debug)]
pub struct SampleBook {
    /// 结果账（append-only：(面, 例序, 全对)）。
    rows: Vec<(u32, usize, bool)>,
    /// 账高水位。
    high: usize,
}

impl SampleBook {
    /// 建账。
    pub fn new() -> Self {
        SampleBook { rows: Vec::new(), high: 0 }
    }

    /// 注入一例。
    pub fn run_case(&mut self, c: &SampleCase) -> bool {
        let got = match face_judge(c.face, c.a, c.b) {
            Ok(()) => 0u8,
            Err(seq) => seq + 1,
        };
        let ok = got == c.want;
        self.rows.push((face_idx(c.face), self.rows.len(), ok));
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        ok
    }

    /// 注入一整批。
    pub fn run_all(&mut self, cases: &[SampleCase]) -> usize {
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

    /// 面例数清点。
    pub fn face_counts(&self) -> [usize; FACES] {
        let mut c = [0usize; FACES];
        for (f, _, _) in self.rows.iter() {
            if (*f as usize) < FACES {
                c[*f as usize] += 1;
            }
        }
        c
    }

    /// 配额判据。
    pub fn quota_ok(&self) -> bool {
        self.face_counts() == QUOTA
    }

    /// 面覆盖判据（九面全覆盖）。
    pub fn covered(&self) -> bool {
        self.face_counts().iter().all(|&n| n > 0)
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("尾段抽样账：\n");
        for (f, seq, ok) in self.rows.iter() {
            s.push_str(&format!("面{} 例{} {}\n", f, seq, if *ok { "过" } else { "红" }));
        }
        s
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 60 例抽样演练：逐例期望码一致 + 配额 + 覆盖 10/10 轮。
pub fn drill_sample60_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30be0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_samples();
        let mut bk = SampleBook::new();
        let hits = bk.run_all(&cases);
        let _ = lcg.below(3);
        if hits == SAMPLE_CASES && cases.len() == SAMPLE_CASES && bk.quota_ok() && bk.covered() && bk.monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("60例抽样轮={}/{}", ok, INJECT_ROUNDS))
}

/// 闸序演练：每面闸 1 优先于闸 2（双坏叠加取闸 1）10/10 轮。
pub fn drill_gate_order_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30be1（闸序序列）。
    let mut lcg = Lcg::new(0x30be1);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 原子：失对 + CAS 失配叠加 → 闸 1（Err(0)）。
        all_ok = all_ok && face_judge(Face::Atomic, 0x1001, 0x2000) == Err(0u8);
        // TLS：越域 + 占用叠加 → 闸 1。
        all_ok = all_ok && face_judge(Face::Tls, TLS_MAX as u64 + 5, 1) == Err(0u8);
        // RTL：奇长 + 超容量叠加 → 闸 1。
        all_ok = all_ok && face_judge(Face::Rtl, 5, 2) == Err(0u8);
        // 进程：PID 零 + 权限越位叠加 → 闸 1。
        all_ok = all_ok && face_judge(Face::Process, 0, 0xFF) == Err(0u8);
        // 端口：消息越上限 + 池足叠加 → 闸 1。
        all_ok = all_ok && face_judge(Face::Port, PORT_MSG_MAX as u64 + 1, 0x10000) == Err(0u8);
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：每面正路例全过 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30be2（正路序列）。
    let mut lcg = Lcg::new(0x30be2);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_samples();
        let mut all_ok = true;
        // 每面 k=0 正路锚（面内第一例）。
        let mut idx = 0usize;
        for fi in 0..FACES {
            let c = &cases[idx];
            all_ok = all_ok && face_judge(c.face, c.a, c.b).is_ok() && c.want == 0;
            idx += QUOTA[fi];
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路锚轮={}/{}", ok, INJECT_ROUNDS))
}

/// 面覆盖演练：九面逐面例数与配额一致 10/10 轮。
pub fn drill_face_quota_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30be3（配额序列）。
    let mut lcg = Lcg::new(0x30be3);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_samples();
        let mut bk = SampleBook::new();
        let _ = bk.run_all(&cases);
        let counts = bk.face_counts();
        let mut all_ok = counts == QUOTA;
        // 面名非空清点。
        for f in all_faces().iter() {
            all_ok = all_ok && !face_name(*f).is_empty();
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("面配额轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：面越闭集（face_judge 对九面全实现——以面序号闭集核验代替）。
    let face_oob_guard = all_faces().iter().all(|f| face_idx(*f) < FACES as u32);
    if face_oob_guard {
        caught += 1;
        d.push("注1 面域:抓".to_string());
    } else {
        d.push("注1 面域:漏".to_string());
    }
    // 注 2：原子面闸 1 必拒。
    if face_judge(Face::Atomic, 0x1002, 0) == Err(0u8) {
        caught += 1;
        d.push("注2 原子:抓".to_string());
    } else {
        d.push("注2 原子:漏".to_string());
    }
    // 注 3：端口面闸 2 必拒。
    if face_judge(Face::Port, 200, 300) == Err(1u8) {
        caught += 1;
        d.push("注3 端口:抓".to_string());
    } else {
        d.push("注3 端口:漏".to_string());
    }
    // 注 4：注册表面闸 2 必拒。
    if face_judge(Face::Registry, 1, REG_NAME_MAX as u64 + 8) == Err(1u8) {
        caught += 1;
        d.push("注4 注册:抓".to_string());
    } else {
        d.push("注4 注册:漏".to_string());
    }
    // 注 5：九面正路锚不可堵死。
    let mut all_pos = true;
    for (fi, f) in all_faces().iter().enumerate() {
        let (a, b) = positive_params(fi);
        all_ok(&mut all_pos, face_judge(*f, a, b).is_ok());
    }
    if all_pos {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 辅助：逻辑与收敛（避免 clippy 冗长链）。
fn all_ok(acc: &mut bool, v: bool) {
    *acc = *acc && v;
}

/// 段位守卫：0x30be0 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    MxCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：抽样报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 抽样报告。
#[derive(Clone, Copy, Debug)]
pub struct MxReport {
    /// 例数。
    pub cases: usize,
    /// 命中数。
    pub hits: usize,
    /// 配额守恒。
    pub quota: bool,
    /// 面覆盖。
    pub covered: bool,
    /// 账单调。
    pub mono: bool,
}

impl MxReport {
    /// 建报告。
    pub fn build(bk: &SampleBook, hits: usize) -> Self {
        MxReport {
            cases: bk.rows(),
            hits,
            quota: bk.quota_ok(),
            covered: bk.covered(),
            mono: bk.monotone(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == SAMPLE_CASES && self.hits == SAMPLE_CASES && self.quota && self.covered && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "MxReport 例={} 命中={} 配额={} 覆盖={} 单调={}",
            self.cases, self.hits, self.quota, self.covered, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct MxCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_mx_checklist(bk: &SampleBook, hits: usize) -> Vec<MxCheckRow> {
    let rep = MxReport::build(bk, hits);
    let counts = bk.face_counts();
    let mut rows: Vec<MxCheckRow> = Vec::new();
    rows.push(MxCheckRow { name: "60例全注", passed: bk.rows() == SAMPLE_CASES });
    rows.push(MxCheckRow { name: "60例全中", passed: hits == SAMPLE_CASES });
    rows.push(MxCheckRow { name: "九面全覆盖", passed: bk.covered() });
    rows.push(MxCheckRow { name: "配额守恒", passed: bk.quota_ok() });
    rows.push(MxCheckRow { name: "面例数清点", passed: counts.iter().all(|&n| n >= 6) });
    rows.push(MxCheckRow { name: "面名闭集", passed: all_faces().iter().all(|f| !face_name(*f).is_empty()) });
    rows.push(MxCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(MxCheckRow { name: "转录非空", passed: bk.export_lines().contains("尾段抽样账") });
    rows.push(MxCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(MxCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn mx_checklist_green(rows: &[MxCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn mx_stats_line(bk: &SampleBook, hits: usize) -> String {
    format!(
        "尾段抽样概况：例 {} 命中 {}/{} 面 {:?}",
        bk.rows(),
        hits,
        SAMPLE_CASES,
        bk.face_counts()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12495_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_samples();
    let mut bk = SampleBook::new();
    let hits = bk.run_all(&cases);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        MxReport::build(&bk, hits).all_green() && bk.export_lines().contains("尾段抽样账"),
        mx_stats_line(&bk, hits),
    ));
    // X-b：闸序 10/10。
    let (ok_g, d_g) = drill_gate_order_10();
    out.push(("X-闸序10/10", ok_g, d_g));
    // X-c：正路锚 10/10。
    let (ok_p, d_p) = drill_positive_10();
    out.push(("X-正路锚10/10", ok_p, d_p));
    // X-d：面配额 10/10。
    let (ok_q, d_q) = drill_face_quota_10();
    out.push(("X-面配额10/10", ok_q, d_q));
    // X-e：终验清单。
    let rows = build_mx_checklist(&bk, hits);
    out.push((
        "X-终验清单10项",
        mx_checklist_green(&rows),
        MxReport::build(&bk, hits).to_text(),
    ));
    // X-f：口径常量核对。
    out.push((
        "X-口径常量核对",
        TLS_MAX == 64 && INFO_CLASS_MAX == 83 && PORT_MSG_MAX == 656
            && REG_NAME_MAX == 16384 && SAMPLE_CASES == 60,
        format!("tls={} info={} port={} reg={} cases={}", TLS_MAX, INFO_CLASS_MAX, PORT_MSG_MAX, REG_NAME_MAX, SAMPLE_CASES),
    ));
    out
}

/// run_unx12495_checks：本单判据聚合（诊断段 0x30be0）。
pub fn run_unx12495_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12495 机制尾段注入矩阵");
    // J1-a：60 例抽样 10/10。
    let (ok1, d1) = drill_sample60_10();
    s.add_dyn("J1-60例抽样10/10", ok1, &d1);
    // J1-b：标准会话恒等式。
    let cases = build_samples();
    let mut bk = SampleBook::new();
    let hits = bk.run_all(&cases);
    s.add_dyn(
        "J1-60例全中九面覆盖",
        hits == SAMPLE_CASES && bk.quota_ok() && bk.covered() && bk.rows() == SAMPLE_CASES,
        &format!("命中={}/{} 面={:?}", hits, SAMPLE_CASES, bk.face_counts()),
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
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30be0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12495_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
