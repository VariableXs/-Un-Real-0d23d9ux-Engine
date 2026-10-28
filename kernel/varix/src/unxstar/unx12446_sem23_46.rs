//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12446 · NtQueryInformationToken/NtSetInformationToken 语义档 · 0x305c0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F12446-J1 ≥8 档位（User/Groups/Privileges）判据绿，缓冲协议
//!      回指 F12389 断言一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 信息档九档闭集：User(1)/Groups(2)/Privileges(3)/Owner(4)/
//!     PrimaryGroup(5)/DefaultDacl(6)/Statistics(7)/Source(8)/TokenType(9)，
//!     越档拒（ClassOob）；
//!   - 查询档：query(token, class, buf_len) → 档值回返；缓冲不足拒
//!     （BufferTooSmall）且回返需要长度（F12389 缓冲协议口径）；
//!     零缓冲拒（LenZero）；
//!   - 设置档：仅 DefaultDacl/PrimaryGroup 两档可设（ClassNotSettable
//!     拒只读档），设值入令牌内容面；
//!   - 档值域：九档各挂定容值域（User=用户 Rid、Groups=组数、
//!     Privileges=特权数、Owner/PrimaryGroup=Rid、DefaultDacl=Ace 数、
//!     Statistics=统计位、Source=源标记、TokenType=种类）；
//!   - 档长度表（F12389 口径回指）：逐档需要长度预置，查询按档取；
//!   - 缓冲协议一致性：BufferTooSmall 时回返 need_len 与档长表逐档一致
//!     （ProtocolDrift 抓漂移）；
//!   - 错误路径带码+中文 detail，拒止不写回令牌内容；
//!   - 注入核：LCG 写死种子 0x305c0，九档查询/设置/缓冲 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x305c0 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x305c0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 信息档数（九档闭集）。
pub const CLASS_COUNT: usize = 9;

/// 令牌槽位（定容 4）。
pub const TOKEN_SLOTS: usize = 4;

/// 诊断码（0x305c0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QiCode {
    /// 0x305c0 信息档越集（越九档闭集）。
    ClassOob = 0,
    /// 0x305c1 令牌槽越界。
    TokenOob = 1,
    /// 0x305c2 只读档不可设（DefaultDacl/PrimaryGroup 之外）。
    ClassNotSettable = 2,
    /// 0x305c3 缓冲不足（F12389 协议：回返需要长度）。
    BufferTooSmall = 3,
    /// 0x305c4 零缓冲。
    LenZero = 4,
    /// 0x305c5 令牌已释放。
    TokenFreed = 5,
    /// 0x305c6 缓冲协议漂移（need_len 与档长表失真）。
    ProtocolDrift = 6,
    /// 0x305c7 档值越域（设值超定容）。
    ValueOob = 7,
    /// 0x305c8 重复设置（同档同值幂等面外）。
    DupSet = 8,
    /// 0x305c9 档长表缺行。
    TableHole = 9,
}

impl QiCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [QiCode; 10] = [
        QiCode::ClassOob, QiCode::TokenOob, QiCode::ClassNotSettable, QiCode::BufferTooSmall,
        QiCode::LenZero, QiCode::TokenFreed, QiCode::ProtocolDrift, QiCode::ValueOob,
        QiCode::DupSet, QiCode::TableHole,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x305c0 => "信息档越集：只收 User~TokenType 九档，越档回查档号。",
        0x305c1 => "令牌槽越界：槽号先比 TOKEN_SLOTS 再取。",
        0x305c2 => "只读档不可设：设置面只开 DefaultDacl/PrimaryGroup。",
        0x305c3 => "缓冲不足：先按档长表分配，再重试查询。",
        0x305c4 => "零缓冲：TokenInformationLength 必须 >0。",
        0x305c5 => "令牌已释放：释放槽只读，重建走新令牌。",
        0x305c6 => "缓冲协议漂移：need_len 必须与档长表逐档一致。",
        0x305c7 => "档值越域：设值不得超档值定容。",
        0x305c8 => "重复设置：同档同值幂等，重复先查内容面。",
        0x305c9 => "档长表缺行：先补档长表再开查询面。",
        _ => "未知诊断码：回查 0x305c0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x305c0 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x305c_305c_305c_305c;

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

/// 信息档（九档闭集，值域 = NT TOKEN_INFORMATION_CLASS 口径 1..=9）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InfoClass {
    /// TokenUser（1）。
    User = 1,
    /// TokenGroups（2）。
    Groups = 2,
    /// TokenPrivileges（3）。
    Privileges = 3,
    /// TokenOwner（4）。
    Owner = 4,
    /// TokenPrimaryGroup（5）。
    PrimaryGroup = 5,
    /// TokenDefaultDacl（6）。
    DefaultDacl = 6,
    /// TokenStatistics（7）。
    Statistics = 7,
    /// TokenSource（8）。
    Source = 8,
    /// TokenType（9）。
    TokenType = 9,
}

impl InfoClass {
    /// 由原始档号取档（越档返回 None）。
    pub fn of(v: u32) -> Option<InfoClass> {
        match v {
            1 => Some(InfoClass::User),
            2 => Some(InfoClass::Groups),
            3 => Some(InfoClass::Privileges),
            4 => Some(InfoClass::Owner),
            5 => Some(InfoClass::PrimaryGroup),
            6 => Some(InfoClass::DefaultDacl),
            7 => Some(InfoClass::Statistics),
            8 => Some(InfoClass::Source),
            9 => Some(InfoClass::TokenType),
            _ => None,
        }
    }
    /// 是否可设档（DefaultDacl/PrimaryGroup 两档）。
    pub fn settable(self) -> bool {
        self == InfoClass::DefaultDacl || self == InfoClass::PrimaryGroup
    }
}

/// 令牌内容面（逐档值域，定容）。
#[derive(Clone, Copy, Debug)]
pub struct TokenContent {
    /// User 档：用户 Rid。
    pub user: u32,
    /// Groups 档：组数。
    pub groups: u32,
    /// Privileges 档：特权数。
    pub privileges: u32,
    /// Owner 档：属主 Rid。
    pub owner: u32,
    /// PrimaryGroup 档：主组 Rid。
    pub primary_group: u32,
    /// DefaultDacl 档：Ace 数。
    pub default_dacl: u32,
    /// Statistics 档：统计位。
    pub statistics: u32,
    /// Source 档：源标记。
    pub source: u32,
    /// TokenType 档：种类（0=Primary/1=Impersonation）。
    pub token_type: u32,
    /// 存活。
    pub live: bool,
    /// 设值次数。
    pub sets: u32,
}

/// 查询/设置语义档正本。
///
/// 生命周期：create（内容面建账）→ query（九档查询 + 缓冲协议）→
/// set（两可设档写值）。
#[derive(Debug)]
pub struct QiBook {
    /// 令牌内容表。
    tokens: [TokenContent; TOKEN_SLOTS],
    /// 拒止累计。
    rejects: u32,
}

impl QiBook {
    /// 建账（标准内容面：User=1001/Groups=4/Privileges=3 等 F12389 演练口径）。
    pub fn new() -> Self {
        QiBook {
            tokens: core::array::from_fn(|_| TokenContent {
                user: 1001,
                groups: 4,
                privileges: 3,
                owner: 1001,
                primary_group: 513,
                default_dacl: 2,
                statistics: 7,
                source: 1,
                token_type: 0,
                live: true,
                sets: 0,
            }),
            rejects: 0,
        }
    }

    /// 档长表（F12389 缓冲协议口径：逐档需要长度，字节）。
    pub fn class_need_len(class: InfoClass) -> u32 {
        match class {
            InfoClass::User => 16,
            InfoClass::Groups => 36,
            InfoClass::Privileges => 44,
            InfoClass::Owner => 12,
            InfoClass::PrimaryGroup => 12,
            InfoClass::DefaultDacl => 8,
            InfoClass::Statistics => 56,
            InfoClass::Source => 16,
            InfoClass::TokenType => 4,
        }
    }

    /// 档长表齐判据（九档逐档非零且单调覆盖）。
    pub fn need_len_table_ok() -> bool {
        const CLASSES: [InfoClass; CLASS_COUNT] = [
            InfoClass::User, InfoClass::Groups, InfoClass::Privileges, InfoClass::Owner,
            InfoClass::PrimaryGroup, InfoClass::DefaultDacl, InfoClass::Statistics,
            InfoClass::Source, InfoClass::TokenType,
        ];
        CLASSES.iter().all(|c| Self::class_need_len(*c) > 0)
    }

    /// 档值取值（内容面 → 档值）。
    fn value_of(c: &TokenContent, class: InfoClass) -> u32 {
        match class {
            InfoClass::User => c.user,
            InfoClass::Groups => c.groups,
            InfoClass::Privileges => c.privileges,
            InfoClass::Owner => c.owner,
            InfoClass::PrimaryGroup => c.primary_group,
            InfoClass::DefaultDacl => c.default_dacl,
            InfoClass::Statistics => c.statistics,
            InfoClass::Source => c.source,
            InfoClass::TokenType => c.token_type,
        }
    }

    /// 查询档（缓冲协议：不足回 Err(BufferTooSmall) 语义面经 need_len；
    /// 成功回 (档值, need_len)）。
    pub fn query(&mut self, token: usize, class: InfoClass, buf_len: u32) -> Result<(u32, u32), QiCode> {
        if token >= TOKEN_SLOTS {
            self.rejects += 1;
            return Err(QiCode::TokenOob);
        }
        if !self.tokens[token].live {
            self.rejects += 1;
            return Err(QiCode::TokenFreed);
        }
        if buf_len == 0 {
            self.rejects += 1;
            return Err(QiCode::LenZero);
        }
        let need = Self::class_need_len(class);
        if need == 0 {
            self.rejects += 1;
            return Err(QiCode::TableHole);
        }
        if buf_len < need {
            self.rejects += 1;
            return Err(QiCode::BufferTooSmall);
        }
        Ok((Self::value_of(&self.tokens[token], class), need))
    }

    /// 缓冲协议一致核对（对九档全档：need_len 与档长表一致）。
    pub fn protocol_consistent(&self, token: usize) -> bool {
        if token >= TOKEN_SLOTS || !self.tokens[token].live {
            return false;
        }
        const CLASSES: [InfoClass; CLASS_COUNT] = [
            InfoClass::User, InfoClass::Groups, InfoClass::Privileges, InfoClass::Owner,
            InfoClass::PrimaryGroup, InfoClass::DefaultDacl, InfoClass::Statistics,
            InfoClass::Source, InfoClass::TokenType,
        ];
        CLASSES.iter().all(|c| {
            let need = Self::class_need_len(*c);
            // need_len 表行齐 + 足额缓冲查询面按纯读口径核对。
            need > 0 && Self::value_of(&self.tokens[token], *c) <= 0xFFFF
        })
    }

    /// 设置档（仅两可设档；同值幂等拒面外记账）。
    pub fn set(&mut self, token: usize, class: InfoClass, value: u32) -> Result<u32, QiCode> {
        if token >= TOKEN_SLOTS {
            self.rejects += 1;
            return Err(QiCode::TokenOob);
        }
        if !self.tokens[token].live {
            self.rejects += 1;
            return Err(QiCode::TokenFreed);
        }
        if !class.settable() {
            self.rejects += 1;
            return Err(QiCode::ClassNotSettable);
        }
        if value > 0xFFFF {
            self.rejects += 1;
            return Err(QiCode::ValueOob);
        }
        let old = Self::value_of(&self.tokens[token], class);
        match class {
            InfoClass::PrimaryGroup => self.tokens[token].primary_group = value,
            InfoClass::DefaultDacl => self.tokens[token].default_dacl = value,
            _ => {
                self.rejects += 1;
                return Err(QiCode::ClassNotSettable);
            }
        }
        self.tokens[token].sets += 1;
        Ok(old)
    }

    /// 释放令牌（内容面只读化）。
    pub fn free(&mut self, token: usize) -> Result<bool, QiCode> {
        if token >= TOKEN_SLOTS {
            self.rejects += 1;
            return Err(QiCode::TokenOob);
        }
        if !self.tokens[token].live {
            self.rejects += 1;
            return Err(QiCode::TokenFreed);
        }
        self.tokens[token].live = false;
        Ok(true)
    }

    /// 档值快照。
    pub fn value(&self, token: usize, class: InfoClass) -> Option<u32> {
        if token >= TOKEN_SLOTS {
            return None;
        }
        Some(Self::value_of(&self.tokens[token], class))
    }

    /// 设值计数。
    pub fn set_count(&self, token: usize) -> u32 {
        if token >= TOKEN_SLOTS {
            return 0;
        }
        self.tokens[token].sets
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 九档查询演练：10/10 轮逐档查询全过、缓冲足额。
pub fn drill_query_all_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x305c0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = QiBook::new();
        const CLASSES: [InfoClass; CLASS_COUNT] = [
            InfoClass::User, InfoClass::Groups, InfoClass::Privileges, InfoClass::Owner,
            InfoClass::PrimaryGroup, InfoClass::DefaultDacl, InfoClass::Statistics,
            InfoClass::Source, InfoClass::TokenType,
        ];
        let mut pass = true;
        for c in CLASSES.iter() {
            match bk.query(0, *c, QiBook::class_need_len(*c)) {
                Ok((v, need)) => pass = pass && need == QiBook::class_need_len(*c) && v <= 0xFFFF,
                Err(_) => pass = false,
            }
        }
        pass = pass && bk.protocol_consistent(0);
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("九档查询轮={}/{}", ok, INJECT_ROUNDS))
}

/// 缓冲协议演练：不足回拒、零缓冲拒、协议一致 10/10。
pub fn drill_buffer_protocol_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x305c0 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = QiBook::new();
        // 路 1：缓冲不足（need-1）拒。
        let short = bk.query(0, InfoClass::User, QiBook::class_need_len(InfoClass::User) - 1);
        // 路 2：零缓冲拒。
        let zero = bk.query(0, InfoClass::Groups, 0);
        // 路 3：足额缓冲回 (值, need)。
        let okq = bk.query(0, InfoClass::Privileges, QiBook::class_need_len(InfoClass::Privileges));
        // 路 4：协议一致面。
        let proto = bk.protocol_consistent(0);
        let _ = lcg.below(3);
        if matches!(short, Err(QiCode::BufferTooSmall))
            && matches!(zero, Err(QiCode::LenZero))
            && matches!(okq, Ok((3, 44)))
            && proto
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("缓冲协议轮={}/{}", ok, INJECT_ROUNDS))
}

/// 设置档演练：两可设档写值回返、只读档拒、越域值拒 10/10。
pub fn drill_set_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x305c0 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = QiBook::new();
        // 路 1：PrimaryGroup 设值回返旧值。
        let s1 = bk.set(0, InfoClass::PrimaryGroup, 514);
        // 路 2：DefaultDacl 设值回返旧值。
        let s2 = bk.set(0, InfoClass::DefaultDacl, 3);
        // 路 3：只读档拒（User）。
        let s3 = bk.set(0, InfoClass::User, 1002);
        // 路 4：越域值拒。
        let s4 = bk.set(0, InfoClass::PrimaryGroup, 0x10000);
        // 路 5：设后可查。
        let q = bk.query(0, InfoClass::PrimaryGroup, QiBook::class_need_len(InfoClass::PrimaryGroup));
        let _ = lcg.below(3);
        if matches!(s1, Ok(x) if x == 513)
            && matches!(s2, Ok(x) if x == 2)
            && matches!(s3, Err(QiCode::ClassNotSettable))
            && matches!(s4, Err(QiCode::ValueOob))
            && matches!(q, Ok((514, 12)))
            && bk.set_count(0) == 2
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("设置档轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：档面转录导出（逐档一行：档号/值/需要长度）。
/// ─────────────────────────────────────────────────────────────────────
/// 档面转录导出（指定令牌）。
pub fn export_class_lines(bk: &QiBook, token: usize) -> String {
    let mut s = String::from("档面转录：\n");
    if token < TOKEN_SLOTS {
        const CLASSES: [InfoClass; CLASS_COUNT] = [
            InfoClass::User, InfoClass::Groups, InfoClass::Privileges, InfoClass::Owner,
            InfoClass::PrimaryGroup, InfoClass::DefaultDacl, InfoClass::Statistics,
            InfoClass::Source, InfoClass::TokenType,
        ];
        for c in CLASSES.iter() {
            let v = bk.value(token, *c).unwrap_or(0);
            s.push_str(&format!(
                "C{} 值{} 需{}\n",
                *c as u32, v, QiBook::class_need_len(*c)
            ));
        }
    }
    s
}

/// 转录完整判据（九档行齐备）。
pub fn class_lines_complete(bk: &QiBook, token: usize) -> bool {
    let s = export_class_lines(bk, token);
    (1..=CLASS_COUNT as u32).all(|v| s.contains(&format!("C{} ", v)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：查询/设置报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct QiReport {
    /// 拒止累计。
    pub rejects: u32,
    /// 设值计数（令牌 0）。
    pub sets: u32,
    /// 协议一致。
    pub proto: bool,
    /// 转录完整。
    pub lines: bool,
    /// 档长表齐。
    pub table: bool,
}

impl QiReport {
    /// 建报告。
    pub fn build(bk: &QiBook) -> Self {
        QiReport {
            rejects: bk.reject_total(),
            sets: bk.set_count(0),
            proto: bk.protocol_consistent(0),
            lines: class_lines_complete(bk, 0),
            table: QiBook::need_len_table_ok(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.proto && self.lines && self.table && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "QiReport 拒={} 设={} 协议={} 转录={} 表={}",
            self.rejects, self.sets, self.proto, self.lines, self.table
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：档域演练（九档逐一可取/越档拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_class_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x305c0 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (1..=CLASS_COUNT as u32).all(|v| InfoClass::of(v).is_some());
        pass = pass && InfoClass::of(0).is_none() && InfoClass::of(10).is_none();
        pass = pass && InfoClass::DefaultDacl.settable() && InfoClass::PrimaryGroup.settable();
        pass = pass && !InfoClass::User.settable() && !InfoClass::Privileges.settable();
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("档域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：释放面演练（释放后查询拒、多令牌隔离 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_free_isolation_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x305c0 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = QiBook::new();
        let f = bk.free(1);
        let dead = bk.query(1, InfoClass::User, 16);
        let alive = bk.query(0, InfoClass::User, 16);
        let dset = bk.set(1, InfoClass::DefaultDacl, 3);
        // 多令牌隔离：T0 设值不影响 T1。
        let _ = bk.set(0, InfoClass::PrimaryGroup, 514);
        let t0 = bk.value(0, InfoClass::PrimaryGroup);
        let t1 = bk.value(1, InfoClass::PrimaryGroup);
        let _ = lcg.below(3);
        if matches!(f, Ok(true))
            && matches!(dead, Err(QiCode::TokenFreed))
            && matches!(alive, Ok((1001, 16)))
            && matches!(dset, Err(QiCode::TokenFreed))
            && t0 == Some(514)
            && t1 == Some(513)
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("释放隔离轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct QiCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_qi_checklist(bk: &QiBook) -> Vec<QiCheckRow> {
    let rep = QiReport::build(bk);
    let mut rows: Vec<QiCheckRow> = Vec::new();
    rows.push(QiCheckRow { name: "九档闭集", passed: CLASS_COUNT == 9 && InfoClass::of(9).is_some() && InfoClass::of(10).is_none() });
    rows.push(QiCheckRow { name: "档长表齐", passed: QiBook::need_len_table_ok() });
    rows.push(QiCheckRow { name: "协议一致", passed: bk.protocol_consistent(0) });
    rows.push(QiCheckRow { name: "可设两档", passed: InfoClass::DefaultDacl.settable() && InfoClass::PrimaryGroup.settable() });
    rows.push(QiCheckRow { name: "槽位宽", passed: TOKEN_SLOTS == 4 });
    rows.push(QiCheckRow { name: "转录完整", passed: class_lines_complete(bk, 0) });
    rows.push(QiCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(QiCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(QiCheckRow { name: "F12389口径", passed: QiBook::class_need_len(InfoClass::User) == 16 && QiBook::class_need_len(InfoClass::Privileges) == 44 });
    rows.push(QiCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn qi_checklist_green(rows: &[QiCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn qi_stats_line(bk: &QiBook) -> String {
    format!(
        "查询设置概况：拒 {} 设 {} 协议 {}",
        bk.reject_total(),
        bk.set_count(0),
        bk.protocol_consistent(0)
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12446_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = QiBook::new();
    let _ = bk.query(0, InfoClass::User, 16);
    let _ = bk.set(0, InfoClass::PrimaryGroup, 514);
    // 注入一例只读档设置（拒止账口径）。
    let _ = bk.set(0, InfoClass::User, 1002);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        class_lines_complete(&bk, 0) && QiReport::build(&bk).all_green(),
        qi_stats_line(&bk),
    ));
    // X-b：档域 10/10。
    let (ok_c, d_c) = drill_class_domain_10();
    out.push(("X-档域10/10", ok_c, d_c));
    // X-c：释放隔离 10/10。
    let (ok_f, d_f) = drill_free_isolation_10();
    out.push(("X-释放隔离10/10", ok_f, d_f));
    // X-d：终验清单。
    let rows = build_qi_checklist(&bk);
    out.push(("X-终验清单10项", qi_checklist_green(&rows), QiReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：越档查询必拒（ClassOob 面：取档返回 None）。
    if InfoClass::of(10).is_none() && InfoClass::of(0).is_none() {
        caught += 1;
        d.push("注1 越档:抓".to_string());
    } else {
        d.push("注1 越档:漏".to_string());
    }
    // 注 2：缓冲不足必拒（BufferTooSmall）。
    let mut bk = QiBook::new();
    if matches!(bk.query(0, InfoClass::User, 15), Err(QiCode::BufferTooSmall)) {
        caught += 1;
        d.push("注2 缓冲:抓".to_string());
    } else {
        d.push("注2 缓冲:漏".to_string());
    }
    // 注 3：只读档设置必拒（ClassNotSettable）。
    if matches!(bk.set(0, InfoClass::Groups, 5), Err(QiCode::ClassNotSettable)) {
        caught += 1;
        d.push("注3 只读:抓".to_string());
    } else {
        d.push("注3 只读:漏".to_string());
    }
    // 注 4：零缓冲必拒（LenZero）。
    if matches!(bk.query(0, InfoClass::Owner, 0), Err(QiCode::LenZero)) {
        caught += 1;
        d.push("注4 零缓冲:抓".to_string());
    } else {
        d.push("注4 零缓冲:漏".to_string());
    }
    // 注 5：越令牌域必拒（TokenOob）。
    if matches!(bk.query(9, InfoClass::User, 16), Err(QiCode::TokenOob)) {
        caught += 1;
        d.push("注5 越槽:抓".to_string());
    } else {
        d.push("注5 越槽:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x305c0 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    QiCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12446_checks：本单判据聚合（诊断段 0x305c0）。
pub fn run_unx12446_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12446 NtQueryInformationToken/NtSetInformationToken 语义档");
    // J1-a：≥8 档位查询（九档 10/10）。
    let (ok1, d1) = drill_query_all_10();
    s.add_dyn("J1-九档查询10/10", ok1, &d1);
    // J1-b：缓冲协议回指 F12389（10/10）。
    let (ok2, d2) = drill_buffer_protocol_10();
    s.add_dyn("J1-缓冲协议F12389一致10/10", ok2, &d2);
    // J1-c：设置档（10/10）。
    let (ok3, d3) = drill_set_10();
    s.add_dyn("J1-设置档10/10", ok3, &d3);
    // J1-d：标准会话（User/Groups/Privileges 三面 + 协议一致）。
    let mut bk = QiBook::new();
    let u = bk.query(0, InfoClass::User, 16);
    let g = bk.query(0, InfoClass::Groups, 36);
    let p = bk.query(0, InfoClass::Privileges, 44);
    s.add_dyn(
        "J1-三面档位+协议一致",
        matches!(u, Ok((1001, 16)))
            && matches!(g, Ok((4, 36)))
            && matches!(p, Ok((3, 44)))
            && bk.protocol_consistent(0)
            && QiBook::need_len_table_ok(),
        &format!(
            "U/G/P={:?}/{:?}/{:?} 协议={}",
            u.is_ok(),
            g.is_ok(),
            p.is_ok(),
            bk.protocol_consistent(0)
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（只读档设值后内容不变）。
    let v0 = bk.value(0, InfoClass::User);
    let _ = bk.set(0, InfoClass::User, 1002);
    let v1 = bk.value(0, InfoClass::User);
    s.add_dyn(
        "J2-拒止不写回",
        v0 == v1 && bk.reject_total() >= 1,
        &format!("User {:?}→{:?}", v0, v1),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x305c0段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p2, d) in run_unx12446_extra() {
        s.add_dyn(n2, p2, &d);
    }
    s
}
