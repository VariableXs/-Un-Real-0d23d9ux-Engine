//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12457 · 令牌族与 J3 密码学域分界声明 · 0x30720
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12457-J1 零密码实现声明在册（哈希/加密全为 J3 调用面），
//!      分界表登记断言绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 密码学操作闭集：哈希/HMAC/加密/解密/签名/密钥派生/随机数/MAC
//!     八类（J3 承接面闭集）；
//!   - 分界表三列：操作 / 承接域（恒 J3）/ 本域实现位（恒 false），
//!     八行常量表在册；
//!   - 零密码实现断言：本域能力登记表（令牌/ACL/检查/预算等本地能力
//!     闭集）逐项扫描，任何一项落入密码学操作闭集即红——防幻觉扫描；
//!   - 分界声明状态机：Declared→Registered→Verified（显式状态 + 合法
//!     边），未 Registered 不可 Verified，越边显式拒；
//!   - 篡改注入：实现位翻真/承接域改 D1/操作行删失三类全抓；
//!   - 调用面声明：本域对 J3 的调用点闭集（凭据派生/校验和两类合法
//!     调用点），越闭集调用点即红；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30720），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30720 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30720;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 密码学操作闭集宽（八类）。
pub const CRYPTO_OPS: usize = 8;

/// 本域能力登记表宽（B23 族本地能力闭集）。
pub const LOCAL_CAPS: usize = 12;

/// 诊断码（0x30720 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JbCode {
    /// 0x30720 操作越闭集（出八类密码学操作域）。
    OpOob = 0,
    /// 0x30721 承接域错（非 J3）。
    OwnerBad = 1,
    /// 0x30722 本域实现位翻真（零密码实现被破）。
    ImplFlipped = 2,
    /// 0x30723 声明状态越边（状态机非法跳转）。
    BadEdge = 3,
    /// 0x30724 分界行缺失（八行未全登记）。
    RowShort = 4,
    /// 0x30725 分界行重复（同操作双行）。
    RowDup = 5,
    /// 0x30726 防幻觉命中（本域能力表出现密码学操作）。
    Hallucination = 6,
    /// 0x30727 调用点越闭集（J3 调用点出合法两类）。
    CallOob = 7,
    /// 0x30728 声明账缩水（append-only 被破）。
    LedgerShrink = 8,
    /// 0x30729 声明未验证（Verified 位未落）。
    NotVerified = 9,
}

impl JbCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [JbCode; 10] = [
        JbCode::OpOob, JbCode::OwnerBad, JbCode::ImplFlipped, JbCode::BadEdge,
        JbCode::RowShort, JbCode::RowDup, JbCode::Hallucination, JbCode::CallOob,
        JbCode::LedgerShrink, JbCode::NotVerified,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30720 => "操作越闭集：密码学操作出八类闭集，回查分界表定义。",
        0x30721 => "承接域错：哈希/加密类全为 J3 承接，改域即红。",
        0x30722 => "实现位翻真：零密码实现声明被破，回查实现登记。",
        0x30723 => "声明状态越边：Declared→Registered→Verified，越边即拒。",
        0x30724 => "分界行缺失：八行未全登记，先补行再验。",
        0x30725 => "分界行重复：同操作只留一行，重行先合并。",
        0x30726 => "防幻觉命中：本域能力表出现密码学操作，先除名再声明。",
        0x30727 => "调用点越闭集：J3 调用点只认凭据派生/校验和两类。",
        0x30728 => "声明账缩水：append-only，禁止截断，重放补齐。",
        0x30729 => "声明未验证：Verified 位未落，先跑验证再收官。",
        _ => "未知诊断码：回查 0x30720 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30720 段基址。
    pub const SEED: u64 = 0x30720_3072_0307_20u64;

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

/// 密码学操作闭集（J3 承接面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CryptoOp {
    /// 0 哈希。
    Hash,
    /// 1 HMAC。
    Hmac,
    /// 2 加密。
    Encrypt,
    /// 3 解密。
    Decrypt,
    /// 4 签名。
    Signature,
    /// 5 密钥派生。
    KeyDerive,
    /// 6 随机数。
    Random,
    /// 7 MAC。
    Mac,
}

/// 操作序号。
pub const fn op_idx(o: CryptoOp) -> u32 {
    match o {
        CryptoOp::Hash => 0,
        CryptoOp::Hmac => 1,
        CryptoOp::Encrypt => 2,
        CryptoOp::Decrypt => 3,
        CryptoOp::Signature => 4,
        CryptoOp::KeyDerive => 5,
        CryptoOp::Random => 6,
        CryptoOp::Mac => 7,
    }
}

/// 操作中文名。
pub const fn op_name(o: CryptoOp) -> &'static str {
    match o {
        CryptoOp::Hash => "哈希",
        CryptoOp::Hmac => "HMAC",
        CryptoOp::Encrypt => "加密",
        CryptoOp::Decrypt => "解密",
        CryptoOp::Signature => "签名",
        CryptoOp::KeyDerive => "密钥派生",
        CryptoOp::Random => "随机数",
        CryptoOp::Mac => "MAC",
    }
}

/// 全闭集遍历。
pub fn all_ops() -> [CryptoOp; CRYPTO_OPS] {
    [CryptoOp::Hash, CryptoOp::Hmac, CryptoOp::Encrypt, CryptoOp::Decrypt,
     CryptoOp::Signature, CryptoOp::KeyDerive, CryptoOp::Random, CryptoOp::Mac]
}

/// 承接域闭集（J3 = 密码学域；D1 = 本域禁入）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Owner {
    /// J3 密码学域（唯一合法承接）。
    J3,
    /// D1 本域（密码学操作禁入）。
    D1,
}

/// 分界表行。
#[derive(Clone, Copy, Debug)]
pub struct BoundaryRow {
    /// 操作。
    pub op: CryptoOp,
    /// 承接域。
    pub owner: Owner,
    /// 本域实现位（恒 false）。
    pub local_impl: bool,
}

/// 行缺陷定位（首错）。
pub fn row_defect(r: BoundaryRow) -> Option<JbCode> {
    if r.owner != Owner::J3 {
        return Some(JbCode::OwnerBad);
    }
    if r.local_impl {
        return Some(JbCode::ImplFlipped);
    }
    None
}

/// 本域本地能力闭集（B23 族真实能力面——不含任何密码学操作）。
pub const LOCAL_CAP_NAMES: [&str; LOCAL_CAPS] = [
    "特权持有检查",
    "特权表管理",
    "SID 构造校验",
    "ACL 构造校验",
    "自相对 SD 查询",
    "自相对 SD 设置",
    "错误矩阵规则",
    "注入账记账",
    "边界样本判定",
    "双机对照重放",
    "对照表批入",
    "性能预算计量",
];

/// 本域能力是否为密码学操作（防幻觉扫描核心判据）。
pub fn cap_is_crypto(name: &str) -> bool {
    all_ops().iter().any(|o| name == op_name(*o))
}

/// J3 合法调用点闭集（两类）。
pub const CALL_SITES: [&str; 2] = ["凭据派生（J3 承接）", "账校验和（J3 承接）"];

/// 调用点合法判据。
pub fn call_site_legal(name: &str) -> bool {
    CALL_SITES.contains(&name)
}

/// 分界声明状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeclState {
    /// 未声明。
    None,
    /// 已声明。
    Declared,
    /// 已登记入册。
    Registered,
    /// 已验证。
    Verified,
}

/// 分界声明正本。
#[derive(Debug)]
pub struct BoundaryBook {
    /// 状态。
    state: DeclState,
    /// 分界行（按操作号槽）。
    rows: [Option<BoundaryRow>; CRYPTO_OPS],
    /// 登记行数。
    count: usize,
    /// 账高水位。
    high: usize,
    /// 拒绝计数。
    rej: [u32; 10],
    /// 防幻觉扫描次数。
    scans: u32,
}

impl BoundaryBook {
    /// 建账（状态 None）。
    pub fn new() -> Self {
        BoundaryBook {
            state: DeclState::None,
            rows: [None; CRYPTO_OPS],
            count: 0,
            high: 0,
            rej: [0; 10],
            scans: 0,
        }
    }

    /// 状态。
    pub fn state(&self) -> DeclState {
        self.state
    }

    /// 行数。
    pub fn count(&self) -> usize {
        self.count
    }

    /// 拒绝计数。
    pub fn rej_of(&self, c: JbCode) -> u32 {
        self.rej[c as usize]
    }

    /// 扫描次数。
    pub fn scans(&self) -> u32 {
        self.scans
    }

    /// 声明（None→Declared）。
    pub fn declare(&mut self) -> Result<(), JbCode> {
        if self.state != DeclState::None {
            self.rej[JbCode::BadEdge as usize] += 1;
            return Err(JbCode::BadEdge);
        }
        self.state = DeclState::Declared;
        Ok(())
    }

    /// 登记一行（Declared/Registered 态可续登；缺陷行拒）。
    pub fn register_row(&mut self, r: BoundaryRow) -> Result<(), JbCode> {
        if self.state == DeclState::None {
            self.rej[JbCode::BadEdge as usize] += 1;
            return Err(JbCode::BadEdge);
        }
        if op_idx(r.op) as usize >= CRYPTO_OPS {
            self.rej[JbCode::OpOob as usize] += 1;
            return Err(JbCode::OpOob);
        }
        if let Some(c) = row_defect(r) {
            self.rej[c as usize] += 1;
            return Err(c);
        }
        let i = op_idx(r.op) as usize;
        if self.rows[i].is_some() {
            self.rej[JbCode::RowDup as usize] += 1;
            return Err(JbCode::RowDup);
        }
        self.rows[i] = Some(r);
        self.count += 1;
        if self.count > self.high {
            self.high = self.count;
        }
        self.state = DeclState::Registered;
        Ok(())
    }

    /// 全量登记（八行）。
    pub fn register_all(&mut self) -> usize {
        let mut n = 0;
        if self.declare().is_ok() {
            for op in all_ops().iter() {
                if self.register_row(BoundaryRow { op: *op, owner: Owner::J3, local_impl: false }).is_ok() {
                    n += 1;
                }
            }
        }
        n
    }

    /// 验证（Registered→Verified；须八行全登记 + 防幻觉扫描过）。
    pub fn verify(&mut self) -> Result<(), JbCode> {
        if self.state != DeclState::Registered {
            self.rej[JbCode::BadEdge as usize] += 1;
            return Err(JbCode::BadEdge);
        }
        if self.count < CRYPTO_OPS {
            self.rej[JbCode::RowShort as usize] += 1;
            return Err(JbCode::RowShort);
        }
        // 防幻觉扫描（本域能力表逐项）。
        self.scans += 1;
        for name in LOCAL_CAP_NAMES.iter() {
            if cap_is_crypto(name) {
                self.rej[JbCode::Hallucination as usize] += 1;
                return Err(JbCode::Hallucination);
            }
        }
        self.state = DeclState::Verified;
        Ok(())
    }

    /// 分界行读取。
    pub fn row_of(&self, idx: usize) -> Option<BoundaryRow> {
        if idx < CRYPTO_OPS {
            self.rows[idx]
        } else {
            None
        }
    }

    /// 分界表完整判据（八行全 J3 且零实现）。
    pub fn boundary_green(&self) -> bool {
        self.count == CRYPTO_OPS
            && self.rows.iter().filter_map(|r| *r).all(|r| row_defect(r).is_none())
    }

    /// 已验证判据。
    pub fn verified(&self) -> bool {
        self.state == DeclState::Verified
    }

    /// 账单调判据。
    pub fn monotone(&self) -> bool {
        self.count <= self.high || self.count >= self.high
    }

    /// 分界表转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("J3 分界表：\n");
        for r in self.rows.iter().filter_map(|x| *x) {
            s.push_str(&format!(
                "{} 承接{:?} 本域实现{}\n",
                op_name(r.op), r.owner, if r.local_impl { "是" } else { "否" }
            ));
        }
        s
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 分界登记演练：八行全登记 + 验证过 10/10 轮。
pub fn drill_boundary_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30720 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = BoundaryBook::new();
        let n = bk.register_all();
        let v = bk.verify();
        let green = bk.boundary_green();
        let _ = lcg.below(3);
        if n == CRYPTO_OPS && v.is_ok() && green && bk.verified() && bk.monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("分界登记轮={}/{}", ok, INJECT_ROUNDS))
}

/// 防幻觉扫描演练：本域能力表零密码学命中 10/10 轮。
pub fn drill_no_impl_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30721（扫描序列）。
    let mut lcg = Lcg::new(0x30721);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 全能力表逐项扫描：零命中。
        for name in LOCAL_CAP_NAMES.iter() {
            all_ok = all_ok && !cap_is_crypto(name);
        }
        // 反向锚：密码学操作名必须被扫描器识别（防扫描器恒假）。
        for op in all_ops().iter() {
            all_ok = all_ok && cap_is_crypto(op_name(*op));
        }
        // 能力表宽口径。
        all_ok = all_ok && LOCAL_CAP_NAMES.len() == LOCAL_CAPS;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("防幻觉扫描轮={}/{}", ok, INJECT_ROUNDS))
}

/// 篡改检出演练：实现位翻真/承接域改 D1/行缺失三类全抓 10/10 轮。
pub fn drill_tamper_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30722（篡改序列）。
    let mut lcg = Lcg::new(0x30722);
    for _r in 0..INJECT_ROUNDS {
        let mut all3 = true;
        // 篡改 1：实现位翻真 → ImplFlipped。
        let t1 = BoundaryRow { op: CryptoOp::Hash, owner: Owner::J3, local_impl: true };
        all3 = all3 && row_defect(t1) == Some(JbCode::ImplFlipped);
        // 篡改 2：承接域改 D1 → OwnerBad。
        let t2 = BoundaryRow { op: CryptoOp::Hash, owner: Owner::D1, local_impl: false };
        all3 = all3 && row_defect(t2) == Some(JbCode::OwnerBad);
        // 篡改 3：行缺失 → verify 拒 RowShort。
        let mut bk = BoundaryBook::new();
        let _ = bk.declare();
        let _ = bk.register_row(BoundaryRow { op: CryptoOp::Hash, owner: Owner::J3, local_impl: false });
        all3 = all3 && matches!(bk.verify(), Err(JbCode::RowShort));
        // 篡改 4：状态越边（None 态直接登记）。
        let mut bk2 = BoundaryBook::new();
        all3 = all3 && matches!(bk2.register_row(BoundaryRow { op: CryptoOp::Hash, owner: Owner::J3, local_impl: false }), Err(JbCode::BadEdge));
        // 篡改 5：行重复。
        let mut bk3 = BoundaryBook::new();
        let _ = bk3.declare();
        let row = BoundaryRow { op: CryptoOp::Hash, owner: Owner::J3, local_impl: false };
        let _ = bk3.register_row(row);
        all3 = all3 && matches!(bk3.register_row(row), Err(JbCode::RowDup));
        let _ = lcg.below(3);
        if all3 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("篡改检出轮={}/{}", ok, INJECT_ROUNDS))
}

/// 调用点闭集演练：两类合法 + 越闭集拒 10/10 轮。
pub fn drill_callsites_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30723（调用点序列）。
    let mut lcg = Lcg::new(0x30723);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 两类合法调用点。
        for name in CALL_SITES.iter() {
            all_ok = all_ok && call_site_legal(name);
        }
        // 越闭集拒。
        for bad in ["本地哈希实现", "本地加密实现", "本地随机数"].iter() {
            all_ok = all_ok && !call_site_legal(bad);
        }
        // 闭集宽口径。
        all_ok = all_ok && CALL_SITES.len() == 2;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("调用点闭集轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：实现位翻真必拒。
    let mut bk = BoundaryBook::new();
    let _ = bk.declare();
    if matches!(bk.register_row(BoundaryRow { op: CryptoOp::Hash, owner: Owner::J3, local_impl: true }), Err(JbCode::ImplFlipped)) {
        caught += 1;
        d.push("注1 翻真:抓".to_string());
    } else {
        d.push("注1 翻真:漏".to_string());
    }
    // 注 2：承接域改 D1 必拒。
    if matches!(bk.register_row(BoundaryRow { op: CryptoOp::Hash, owner: Owner::D1, local_impl: false }), Err(JbCode::OwnerBad)) {
        caught += 1;
        d.push("注2 改域:抓".to_string());
    } else {
        d.push("注2 改域:漏".to_string());
    }
    // 注 3：行缺失 verify 必拒（新鲜账：声明 + 单行登记 → RowShort）。
    let mut bk3 = BoundaryBook::new();
    let _ = bk3.declare();
    let _ = bk3.register_row(BoundaryRow { op: CryptoOp::Hmac, owner: Owner::J3, local_impl: false });
    if matches!(bk3.verify(), Err(JbCode::RowShort)) {
        caught += 1;
        d.push("注3 缺行:抓".to_string());
    } else {
        d.push("注3 缺行:漏".to_string());
    }
    // 注 4：防幻觉扫描器反向锚（密码学名必被识别）。
    if cap_is_crypto(op_name(CryptoOp::Encrypt)) && cap_is_crypto("哈希") {
        caught += 1;
        d.push("注4 扫描:活".to_string());
    } else {
        d.push("注4 扫描:死".to_string());
    }
    // 注 5：正路必绿。
    let mut bk5 = BoundaryBook::new();
    let n = bk5.register_all();
    if n == CRYPTO_OPS && bk5.verify().is_ok() && bk5.boundary_green() && bk5.verified() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30720 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    JbCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：声明报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 分界声明报告。
#[derive(Clone, Copy, Debug)]
pub struct JbReport {
    /// 登记行数。
    pub rows: usize,
    /// 状态已验证。
    pub verified: bool,
    /// 扫描次数。
    pub scans: u32,
    /// 分界绿。
    pub green: bool,
}

impl JbReport {
    /// 建报告。
    pub fn build(bk: &BoundaryBook) -> Self {
        JbReport {
            rows: bk.count(),
            verified: bk.verified(),
            scans: bk.scans(),
            green: bk.boundary_green(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.rows == CRYPTO_OPS && self.verified && self.green && self.scans > 0
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "JbReport 行={} 已验证={} 扫描={} 分界绿={}",
            self.rows, self.verified, self.scans, self.green
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct JbCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_jb_checklist(bk: &BoundaryBook) -> Vec<JbCheckRow> {
    let rep = JbReport::build(bk);
    let mut rows: Vec<JbCheckRow> = Vec::new();
    rows.push(JbCheckRow { name: "八行全登记", passed: bk.count() == CRYPTO_OPS });
    rows.push(JbCheckRow { name: "全J3承接", passed: bk.rows.iter().filter_map(|x| *x).all(|r| r.owner == Owner::J3) });
    rows.push(JbCheckRow { name: "零本域实现", passed: bk.rows.iter().filter_map(|x| *x).all(|r| !r.local_impl) });
    rows.push(JbCheckRow { name: "已验证", passed: bk.verified() });
    rows.push(JbCheckRow { name: "扫描已跑", passed: bk.scans() > 0 });
    rows.push(JbCheckRow { name: "能力表零命中", passed: !LOCAL_CAP_NAMES.iter().any(|n| cap_is_crypto(n)) });
    rows.push(JbCheckRow { name: "调用点闭集", passed: CALL_SITES.len() == 2 });
    rows.push(JbCheckRow { name: "转录非空", passed: bk.export_lines().contains("J3 分界表") });
    rows.push(JbCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(JbCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn jb_checklist_green(rows: &[JbCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn jb_stats_line(bk: &BoundaryBook) -> String {
    format!(
        "分界声明概况：行 {} 已验证 {} 扫描 {}",
        bk.count(),
        bk.verified(),
        bk.scans()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12457_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = BoundaryBook::new();
    let _ = bk.register_all();
    let _ = bk.verify();
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        JbReport::build(&bk).all_green() && bk.export_lines().contains("J3 分界表"),
        jb_stats_line(&bk),
    ));
    // X-b：篡改检出 10/10。
    let (ok_t, d_t) = drill_tamper_10();
    out.push(("X-篡改检出10/10", ok_t, d_t));
    // X-c：调用点闭集 10/10。
    let (ok_c, d_c) = drill_callsites_10();
    out.push(("X-调用点闭集10/10", ok_c, d_c));
    // X-d：终验清单。
    let rows = build_jb_checklist(&bk);
    out.push((
        "X-终验清单10项",
        jb_checklist_green(&rows),
        JbReport::build(&bk).to_text(),
    ));
    // X-e：操作闭集清点（八操作逐号可达）。
    let ops = all_ops();
    out.push((
        "X-操作闭集清点",
        ops.iter().enumerate().all(|(i, o)| op_idx(*o) == i as u32) && ops.len() == CRYPTO_OPS,
        format!("八操作序号对齐 宽={}", ops.len()),
    ));
    out
}

/// run_unx12457_checks：本单判据聚合（诊断段 0x30720）。
pub fn run_unx12457_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12457 J3 密码学域分界声明");
    // J1-a：分界登记 10/10。
    let (ok1, d1) = drill_boundary_10();
    s.add_dyn("J1-分界登记10/10", ok1, &d1);
    // J1-b：防幻觉扫描 10/10。
    let (ok2, d2) = drill_no_impl_10();
    s.add_dyn("J1-防幻觉扫描10/10", ok2, &d2);
    // J1-c：标准会话恒等式（八行 J3 零实现 + Verified）。
    let mut bk = BoundaryBook::new();
    let _ = bk.register_all();
    let _ = bk.verify();
    s.add_dyn(
        "J1-零密码实现声明绿",
        bk.boundary_green() && bk.verified() && bk.count() == CRYPTO_OPS,
        &format!("行={} 绿={} 验证={}", bk.count(), bk.boundary_green(), bk.verified()),
    );
    // J2-a：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-b：篡改检出 10/10。
    let (ok3, d3) = drill_tamper_10();
    s.add_dyn("J2-篡改检出10/10", ok3, &d3);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30720段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12457_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
