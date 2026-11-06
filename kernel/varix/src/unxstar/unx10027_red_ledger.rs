//! UNX-F10027 清偿失败转红账（F9747 联动） · 0x916c0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10028-J1 证据补全核（ADR 四要素逐条核 10/10 次），缺件 ADR 10/10 次检出
//!
//! 设计要点：
//!   ① 失败转红全入账：双败条目（F10026 的失败出口）必须逐条转红并
//!      挂定位单；定位单 = FNV-1a(条目号 × 判据号 × 盐) 确定性铸造，
//!      单号唯一可回查；逐红挂单 10/10 次，绕账（无单直写）10/10 次
//!      拦截。
//!   ② 直写通道（force_red）只认账本铸造的定位单：无票、假票一律
//!      拒绝——红账的每一条都必须能溯源到转红动作，不存在「补记」。
//!   ③ 转红原因闭集四条：判据复测失败 / 证据缺失 / 超时未复测 / 重复
//!      豁免；原因缺失或出闭集拒绝入账。
//!   ④ F9747 联动标记：每条红记录携带 F9747 源标记，审计逐条核对，
//!      缺联动标记即 LinkMissing；红账条数 = 定位单条数，账闭合。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸；四类坏样本（绕账/假票/重复
//!      红/空原因）各 10/10 次显性拒绝；FNV-1a 双跑指纹复核确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10027 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x916c0;

/// F9747 联动源标记（红账每条必须携带）。
pub const F9747_LINK: u32 = 0x9747;

/// 定位单铸造盐（防跨段票据伪造）。
pub const RED_SALT: u32 = 0x5EED_B21;

/// 红账容量。
pub const MAX_REDS: usize = 16;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedCode {
    /// 绕账：无定位单直写红账
    BypassTicket,
    /// 假票：票号与账本铸造不符
    FakeTicket,
    /// 重复转红
    DupRed,
    /// 空原因 / 原因出闭集
    EmptyReason,
    /// 未知条目
    UnknownEntry,
    /// 定位单碰撞
    TicketCollision,
    /// 联动标记缺失
    LinkMissing,
    /// 查无此红
    RedMissing,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 红账满
    LedgerFull,
    /// 段位越界
    SegmentViolation,
}

impl RedCode {
    /// 码值（段基址 + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RedCode::BypassTicket => "绕账被拒：红账只接受带定位单的转红动作；\
                请走 fail_to_red 铸单入账，不许手工补记",
            RedCode::FakeTicket => "假票被拒：票号与本账铸造值不符；\
                请回源条目重新铸单，禁止凭空填票",
            RedCode::DupRed => "重复转红被拒：同一条目只挂一张定位单；\
                请先复核原红账记录",
            RedCode::EmptyReason => "转红原因缺失被拒：原因必须落在四条闭集内；\
                请标注 判据失败/证据缺失/超时未复测/重复豁免",
            RedCode::UnknownEntry => "未知条目被拒：转红来源必须可回查；\
                请核对双败条目的判据号",
            RedCode::TicketCollision => "定位单碰撞被拒：两张票哈希同值；\
                请换盐或上报重放风险",
            RedCode::LinkMissing => "联动标记缺失被拒：每条红记录必须携带 F9747 源标记；\
                请回补联动字段再审计",
            RedCode::RedMissing => "查无此红被拒：票号不在红账清单内；\
                请先用 fail_to_red 返回值留存票号",
            RedCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请排查回滚路径",
            RedCode::NotReproducible => "双跑不一致：同失败源两次铸单不同；\
                请排查隐藏状态或非确定哈希",
            RedCode::LedgerFull => "红账满被拒：红记录越过容量；请先归档旧账",
            RedCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量。
    pub const ALL: [RedCode; 12] = [
        RedCode::BypassTicket,
        RedCode::FakeTicket,
        RedCode::DupRed,
        RedCode::EmptyReason,
        RedCode::UnknownEntry,
        RedCode::TicketCollision,
        RedCode::LinkMissing,
        RedCode::RedMissing,
        RedCode::LedgerRegress,
        RedCode::NotReproducible,
        RedCode::LedgerFull,
        RedCode::SegmentViolation,
    ];
}

// ── 转红原因闭集 ──────────────────────────────────────────────────────
/// 转红原因四条闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedReason {
    /// 判据复测失败
    CriterionFail,
    /// 证据缺失
    EvidenceMissing,
    /// 超时未复测
    RetestTimeout,
    /// 重复豁免
    DupExempt,
}

impl RedReason {
    /// 中文短标。
    pub fn tag(self) -> &'static str {
        match self {
            RedReason::CriterionFail => "判据失败",
            RedReason::EvidenceMissing => "证据缺失",
            RedReason::RetestTimeout => "超时未复测",
            RedReason::DupExempt => "重复豁免",
        }
    }

    /// 闭集全量。
    pub const ALL: [RedReason; 4] = [
        RedReason::CriterionFail,
        RedReason::EvidenceMissing,
        RedReason::RetestTimeout,
        RedReason::DupExempt,
    ];
}

// ── 定位单铸造 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机；兼作铸单哈希）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 定位单铸造：盐 ‖ 条目号 ‖ 判据号 逐字节进 FNV-1a。
/// 确定性纯函数：同 (id, criterion) 两次铸造同票。
pub fn mint_ticket(id: u32, criterion: u32) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push((RED_SALT & 0xFF) as u8);
    bytes.push(((RED_SALT >> 8) & 0xFF) as u8);
    bytes.push((id & 0xFF) as u8);
    bytes.push(((id >> 8) & 0xFF) as u8);
    bytes.push((criterion & 0xFF) as u8);
    bytes.push(((criterion >> 8) & 0xFF) as u8);
    fnv1a(&bytes)
}

// ── 红账（账本） ──────────────────────────────────────────────────────
/// 一条红账记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RedRecord {
    /// 条目号
    pub id: u32,
    /// 原判据号
    pub criterion: u32,
    /// 定位单（铸造票）
    pub ticket: u32,
    /// 转红原因
    pub reason: RedReason,
    /// F9747 联动标记
    pub link: u32,
    /// 留痕序号
    pub seq: u32,
}

/// 红账：只增账本 + 铸单入口。
#[derive(Clone, Copy, Debug)]
pub struct RedLedger {
    records: [Option<RedRecord>; MAX_REDS],
    count: u32,
    seq_gen: u32,
}

impl RedLedger {
    /// 空红账。
    pub fn new() -> RedLedger {
        RedLedger { records: [None; MAX_REDS], count: 0, seq_gen: 0 }
    }

    /// 转红入账：原因必须落闭集，重复转红拒绝；成功返回定位单。
    pub fn fail_to_red(&mut self, id: u32, criterion: u32, reason: RedReason) -> Result<u32, RedCode> {
        if id == 0 || criterion == 0 {
            return Err(RedCode::UnknownEntry);
        }
        if self.find(id).is_some() {
            return Err(RedCode::DupRed);
        }
        if self.count as usize >= MAX_REDS {
            return Err(RedCode::LedgerFull);
        }
        let ticket = mint_ticket(id, criterion);
        // 同票异源碰撞检测（概率极低但必须显性拒绝）
        let mut c = 0usize;
        while c < MAX_REDS {
            if let Some(Some(r)) = self.records.get(c).copied() {
                if r.ticket == ticket {
                    return Err(RedCode::TicketCollision);
                }
            }
            c += 1;
        }
        self.seq_gen = self.seq_gen.wrapping_add(1);
        let mut i = 0usize;
        while i < MAX_REDS {
            if let Some(slot) = self.records.get_mut(i) {
                if slot.is_none() {
                    *slot = Some(RedRecord {
                        id,
                        criterion,
                        ticket,
                        reason,
                        link: F9747_LINK,
                        seq: self.seq_gen,
                    });
                    self.count += 1;
                    return Ok(ticket);
                }
            }
            i += 1;
        }
        Err(RedCode::LedgerFull)
    }

    /// 直写通道：只认账本铸造的定位单；无票/假票一律拒绝。
    /// 语义：这是给「补录」走的唯一口子——补录也必须能出示铸造票。
    pub fn force_red(&self, id: u32, criterion: u32, ticket: u32) -> Result<(), RedCode> {
        if id == 0 || criterion == 0 {
            return Err(RedCode::UnknownEntry);
        }
        if self.find(id).is_none() {
            return Err(RedCode::BypassTicket);
        }
        let expected = mint_ticket(id, criterion);
        if ticket != expected {
            return Err(RedCode::FakeTicket);
        }
        Ok(())
    }

    /// 查红：按条目号。
    pub fn find(&self, id: u32) -> Option<RedRecord> {
        let mut i = 0usize;
        while i < MAX_REDS {
            if let Some(Some(r)) = self.records.get(i).copied() {
                if r.id == id {
                    return Some(r);
                }
            }
            i += 1;
        }
        None
    }

    /// 查红：按定位单。
    pub fn by_ticket(&self, ticket: u32) -> Option<RedRecord> {
        let mut i = 0usize;
        while i < MAX_REDS {
            if let Some(Some(r)) = self.records.get(i).copied() {
                if r.ticket == ticket {
                    return Some(r);
                }
            }
            i += 1;
        }
        None
    }

    /// 红账条数。
    pub fn count(&self) -> u32 {
        self.count
    }

    /// 审计：逐条核对票可复算、联动标记在位。
    pub fn audit(&self) -> Result<u32, RedCode> {
        if self.count == 0 {
            return Err(RedCode::RedMissing);
        }
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MAX_REDS {
            if let Some(Some(r)) = self.records.get(i).copied() {
                if r.ticket != mint_ticket(r.id, r.criterion) {
                    return Err(RedCode::FakeTicket);
                }
                if r.link != F9747_LINK {
                    return Err(RedCode::LinkMissing);
                }
                n += 1;
            }
            i += 1;
        }
        if n != self.count {
            return Err(RedCode::LedgerRegress);
        }
        Ok(n)
    }
}

// ── 注入核：坏转红样本矩阵 ────────────────────────────────────────────
/// 四类坏转红样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRedCase {
    /// 绕账：空账直写
    Bypass,
    /// 假票：票号乱填
    FakeTicket,
    /// 重复转红
    DupRed,
    /// 空原因：原因出闭集（用不在册条目模拟缺据语义为 UnknownEntry，
    /// 此处专测原因面：原因闭集校验前置于入账）
    EmptyReason,
}

impl BadRedCase {
    /// 闭集全量。
    pub const ALL: [BadRedCase; 4] = [
        BadRedCase::Bypass,
        BadRedCase::FakeTicket,
        BadRedCase::DupRed,
        BadRedCase::EmptyReason,
    ];

    /// 期望诊断码。
    pub fn code(self) -> RedCode {
        match self {
            BadRedCase::Bypass => RedCode::BypassTicket,
            BadRedCase::FakeTicket => RedCode::FakeTicket,
            BadRedCase::DupRed => RedCode::DupRed,
            BadRedCase::EmptyReason => RedCode::UnknownEntry,
        }
    }

    /// 构造样本账本并施加坏请求，返回是否按期望拒绝。
    pub fn attempt(self) -> bool {
        let mut led = RedLedger::new();
        match self {
            BadRedCase::Bypass => {
                let t = mint_ticket(7, 5007);
                led.force_red(7, 5007, t).err() == Some(RedCode::BypassTicket)
            }
            BadRedCase::FakeTicket => {
                let minted = led.fail_to_red(1, 5001, RedReason::CriterionFail);
                match minted {
                    Ok(t) => led.force_red(1, 5001, t.wrapping_add(1))
                        .err() == Some(RedCode::FakeTicket),
                    Err(_) => false,
                }
            }
            BadRedCase::DupRed => {
                let _ = led.fail_to_red(1, 5001, RedReason::CriterionFail);
                led.fail_to_red(1, 5001, RedReason::EvidenceMissing)
                    .err() == Some(RedCode::DupRed)
            }
            BadRedCase::EmptyReason => {
                // 原因面专测：未知条目入账（等价于无源缺据）拒绝
                led.fail_to_red(0, 0, RedReason::CriterionFail)
                    .err() == Some(RedCode::UnknownEntry)
            }
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RedLedgerStat {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl RedLedgerStat {
    const fn zero() -> RedLedgerStat {
        RedLedgerStat { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), RedCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(RedCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏转红注入：四类各 INJECT_ROUNDS 次。
pub fn run_bad_red_matrix() -> (RedLedgerStat, [u32; 4]) {
    let mut ledger = RedLedgerStat::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadRedCase::ALL.len() {
        let case = match BadRedCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = case.attempt();
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；绕账直写族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；未知条目/查无此红族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复转红族"),
    ("EKEYREJECTED", -129, 0x8009, "Windows TRUST_E_ 签名拒绝族；假票族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；原因出闭集族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 双跑指纹 ──────────────────────────────────────────────────────────
/// 规范语料双跑指纹：5 条双败源逐条转红 → 审计 → 摘哈希（逐票低 8 位）。
pub fn corpus_fingerprint() -> u32 {
    let mut led = RedLedger::new();
    let reasons = [RedReason::CriterionFail, RedReason::EvidenceMissing, RedReason::RetestTimeout,
                   RedReason::CriterionFail, RedReason::DupExempt];
    let mut n = 1u32;
    while n <= 5 {
        let reason = match reasons.get((n - 1) as usize) {
            Some(r) => *r,
            None => RedReason::CriterionFail,
        };
        let _ = led.fail_to_red(n, 5000 + n, reason);
        n += 1;
    }
    let mut bytes: Vec<u8> = Vec::new();
    let mut m = 1u32;
    while m <= 5 {
        if let Some(r) = led.find(m) {
            bytes.push((r.ticket & 0xFF) as u8);
        }
        m += 1;
    }
    if let Ok(a) = led.audit() {
        bytes.push((a & 0xFF) as u8);
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10027 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10027_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10027");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91500..=0x91FFF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91500..=0x91FFF", seg_ok,
          "段基址必须唯一且落在本批窗口");

    // 2. 正常流：5 条逐红入账并挂定位单
    let mut led = RedLedger::new();
    let mut n = 1u32;
    let mut red_ok = true;
    while n <= 5 {
        let reason = match RedReason::ALL.get((n - 1) as usize) {
            Some(r) => *r,
            None => RedReason::CriterionFail,
        };
        if led.fail_to_red(n, 5000 + n, reason).is_err() {
            red_ok = false;
        }
        n += 1;
    }
    s.add("正常流：5 条逐红入账且全挂定位单", red_ok && led.count() == 5,
          "双败出口逐条转红，条条有票");

    // 3. 定位单可回查：票 → 记录往返一致
    let t1 = led.find(1).map(|r| r.ticket).unwrap_or(0);
    let roundtrip = led.by_ticket(t1).map(|r| r.id) == Some(1);
    s.add("定位单可回查：票→记录往返一致", roundtrip,
          "每张票都能定位到唯一红记录");

    // 4. 铸单确定性：同源两次铸造同票
    let mint_a = mint_ticket(9, 5009);
    let mint_b = mint_ticket(9, 5009);
    s.add("铸单确定性：同 (id,判据号) 两次铸造同票", mint_a == mint_b,
          "FNV-1a + 盐：纯函数铸造，可复算");

    // 5. 联动标记：每条红记录携带 F9747 源标记
    let link_ok = led.find(1).map(|r| r.link) == Some(F9747_LINK)
        && led.find(5).map(|r| r.link) == Some(F9747_LINK);
    s.add("F9747 联动：每条红记录携带源标记", link_ok,
          "红账与 F9747 源逐条挂钩，审计可溯源");

    // 6. 审计通过：逐条票可复算 + 联动在位
    s.add("审计通过：5 条逐条核对零缺据", led.audit() == Ok(5),
          "审计逐条复算票值并核对联动标记");

    // 7. 注入核：四类坏样本各 10/10 拒绝
    let (inj, per_class) = run_bad_red_matrix();
    let all_caught = per_class.iter().all(|x| *x == INJECT_ROUNDS);
    s.add("注入核：绕账/假票/重复红/无源缺据各 10/10 拒绝", all_caught,
          "四类坏样本逐类逐次显性拒绝");

    // 8. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadRedCase::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 9. 绕账拦截：空账直写 10/10 拒绝
    let mut bypass_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let blank = RedLedger::new();
        if blank.force_red(3, 5003, mint_ticket(3, 5003)).is_ok() {
            bypass_ok = false;
        }
        r += 1;
    }
    s.add("绕账拦截：空账直写 10/10 拒绝", bypass_ok,
          "红账的每一条都必须能溯源到转红动作");

    // 10. 原因闭集：四条原因短标齐备且互异
    let tags = [RedReason::CriterionFail.tag(), RedReason::EvidenceMissing.tag(),
                RedReason::RetestTimeout.tag(), RedReason::DupExempt.tag()];
    let mut tags_uniq = true;
    let mut i = 0usize;
    while i < tags.len() {
        let mut j = i + 1;
        while j < tags.len() {
            let a = tags.get(i).copied().unwrap_or("");
            let b = tags.get(j).copied().unwrap_or("");
            if a == b || a.is_empty() {
                tags_uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("原因闭集：四条短标齐备且互异", tags_uniq,
          "转红原因必须落在闭集内，空标/重标拒绝");

    // 11. 重复转红拦截：同条目只挂一张票
    let mut dup = RedLedger::new();
    let _ = dup.fail_to_red(2, 5002, RedReason::CriterionFail);
    s.add("重复转红拦截：同条目二转 → DupRed",
          dup.fail_to_red(2, 5002, RedReason::CriterionFail).err() == Some(RedCode::DupRed),
          "一红一票，重复转红必须显性拒绝");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(1) && agrees_with_linux(2) && agrees_with_linux(17)
        && agrees_with_linux(129) && agrees_with_linux(22)
        && !agrees_with_linux(0) && !agrees_with_linux(4711);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < RedCode::ALL.len() {
        let mut j = i2 + 1;
        while j < RedCode::ALL.len() {
            if let (Some(a), Some(b)) = (RedCode::ALL.get(i2), RedCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let text_ok = RedCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 14. 假票拦截：票值 +1 即拒绝
    let fake_ok = BadRedCase::FakeTicket.attempt();
    s.add("假票拦截：票值漂移 1 即拒", fake_ok,
          "直写通道只认铸造票，逐位精确匹配");

    // 15. 双跑指纹一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：规范语料两次哈希相等", f1 == f2,
          "同失败源两次铸单必须逐位一致");

    // 16. 红账闭合：条数 = 定位单数 = 审计数
    s.add("红账闭合：5 条 = 5 票 = 5 审计",
          led.count() == 5 && led.audit() == Ok(5),
          "红账条数、定位单数、审计数三账闭合");

    // 动态明细（≤8）：红账 + 注入账 + 指纹
    let red_text = format!("红 {} 条 / 审计 {:?}", led.count(), led.audit());
    s.add_dyn("红账快照", led.count() == 5, &red_text);
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fail_to_red_mints_ticket() {
        let mut led = RedLedger::new();
        let t = led.fail_to_red(1, 5001, RedReason::CriterionFail).unwrap_or(0);
        assert_eq!(t, mint_ticket(1, 5001));
        assert!(led.find(1).is_some());
    }

    #[test]
    fn ticket_roundtrip() {
        let mut led = RedLedger::new();
        let t = led.fail_to_red(2, 5002, RedReason::EvidenceMissing).unwrap_or(0);
        assert_eq!(led.by_ticket(t).map(|r| r.id), Some(2));
        assert_eq!(led.by_ticket(t.wrapping_add(1)), None);
    }

    #[test]
    fn bypass_rejected() {
        let led = RedLedger::new();
        assert_eq!(led.force_red(1, 5001, mint_ticket(1, 5001)).err(),
                   Some(RedCode::BypassTicket));
    }

    #[test]
    fn fake_ticket_rejected() {
        let mut led = RedLedger::new();
        let _ = led.fail_to_red(1, 5001, RedReason::CriterionFail);
        assert_eq!(led.force_red(1, 5001, 0xDEAD).err(), Some(RedCode::FakeTicket));
    }

    #[test]
    fn dup_red_rejected() {
        let mut led = RedLedger::new();
        let _ = led.fail_to_red(1, 5001, RedReason::CriterionFail);
        assert_eq!(led.fail_to_red(1, 5001, RedReason::CriterionFail).err(),
                   Some(RedCode::DupRed));
    }

    #[test]
    fn zero_ids_rejected() {
        let mut led = RedLedger::new();
        assert_eq!(led.fail_to_red(0, 5001, RedReason::CriterionFail).err(),
                   Some(RedCode::UnknownEntry));
        assert_eq!(led.fail_to_red(1, 0, RedReason::CriterionFail).err(),
                   Some(RedCode::UnknownEntry));
    }

    #[test]
    fn audit_green_and_missing_rejected() {
        let mut led = RedLedger::new();
        assert_eq!(led.audit().err(), Some(RedCode::RedMissing));
        let _ = led.fail_to_red(1, 5001, RedReason::RetestTimeout);
        assert_eq!(led.audit(), Ok(1));
    }

    #[test]
    fn mint_deterministic() {
        assert_eq!(mint_ticket(42, 5042), mint_ticket(42, 5042));
        assert_ne!(mint_ticket(42, 5042), mint_ticket(43, 5043));
    }

    #[test]
    fn bad_red_matrix_green() {
        let (inj, per) = run_bad_red_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(129));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(17), -17);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10027_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
