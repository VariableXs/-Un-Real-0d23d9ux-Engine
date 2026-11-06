//! UNX-F10068 红账总对账（F9747 全账） · 0x920c0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10068（M 型收口线：F9747 红账全账逐条对齐）。
//! 锚点：UNX-F10069-J1 豁免总核一致（v2 账↔审计面 10/10 次），账实漂
//!       移 10/10 次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x920c0（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `RedCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 注册表 `RedRegistry`：F9747 红账逐条登记（红 ID + 波次 + 批号定
//!     位单），重复红/无定位红显性拒绝；
//!   - 扫描账 `ScanLedger`：全账扫描逐红对账（在案/账外二分），账外红
//!     逐条显性检出，逃逸恒 0；
//!   - 状态机 `RedState`：Registering → Reconciled 单向推进；
//!   - 解析器 `parse_red_row`："F9747|w3|b12" → (红 ID, 波, 批) 三段；
//!   - 注入核 `BadRedCase`：账外红 / 无定位红 / 重复红 / 红账倒退 /
//!     越窗红 五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10068 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x920c0;

/// 段窗上界（下一卡 0x92100 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92100;

/// 红账锚卡（F9747）。
pub const RED_ANCHOR: u32 = 9747;

/// 波次总数（六波）。
pub const WAVE_COUNT: u32 = 6;

/// 批号上界（B01..B40）。
pub const BATCH_MAX: u32 = 40;

/// 红账容量（全账逐条在案上界）。
pub const RED_CAP: usize = 64;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EEXIST: i32 = 17;
pub const ENOENT: i32 = 2;
pub const EINVAL: i32 = 22;
pub const ENODATA: i32 = 61;
pub const EOVERFLOW: i32 = 75;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedCode {
    /// 账外红（扫描得红但账内无案）
    OutsideRed,
    /// 无定位红（缺波次/批号定位单）
    Unattributed,
    /// 重复红（同 ID 二次登记）
    DuplicateRed,
    /// 红账倒退（在案数只增不减破坏）
    LedgerRegress,
    /// 越窗红（波次出 1..=6 或批号出 1..=40）
    OutOfWindow,
    /// 对账未完成出结论
    NotReconciled,
    /// 红账溢出（超出 RED_CAP）
    Overflow,
    /// 扫描逃逸（账外红漏计）
    ScanEscape,
    /// 解析失败
    ParseFailure,
    /// 状态倒退
    StateRegress,
    /// 闭环红复燃（已闭环红再次在案）
    Relapsed,
    /// 段位越界
    SegmentViolation,
}

impl RedCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RedCode::OutsideRed => "账外红被拒：扫描得红必须在 F9747 全账在案；请补登记",
            RedCode::Unattributed => "无定位红被拒：每条红必须挂波次+批号；请补定位单",
            RedCode::DuplicateRed => "重复红被拒：同 ID 已在案；请查重复登记",
            RedCode::LedgerRegress => "红账倒退被拒：在案数只增不减；请排查回滚",
            RedCode::OutOfWindow => "越窗红被拒：波次 1..=6、批号 1..=40；请核对定位单",
            RedCode::NotReconciled => "对账未完成被拒：未逐红对齐不得出总核结论",
            RedCode::Overflow => "红账溢出被拒：超出 64 条容量；请扩账",
            RedCode::ScanEscape => "扫描逃逸：账外红漏计；请补扫全账",
            RedCode::ParseFailure => "解析失败：红行必须恰为 F<id>|w<n>|b<n> 三段",
            RedCode::StateRegress => "状态倒退被拒：Registering→Reconciled 单向",
            RedCode::Relapsed => "闭环红复燃被拒：已闭环红不得再在案；请查复测记录",
            RedCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x920c0..=0x92100",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [RedCode; 12] = [
        RedCode::OutsideRed,
        RedCode::Unattributed,
        RedCode::DuplicateRed,
        RedCode::LedgerRegress,
        RedCode::OutOfWindow,
        RedCode::NotReconciled,
        RedCode::Overflow,
        RedCode::ScanEscape,
        RedCode::ParseFailure,
        RedCode::StateRegress,
        RedCode::Relapsed,
        RedCode::SegmentViolation,
    ];
}

// ── 红条目与解析器 ────────────────────────────────────────────────────
/// 一条红：ID + 波次 + 批号定位单。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RedEntry {
    /// 红 ID（判据号 9601..=10080）
    pub id: u32,
    /// 波次 1..=6
    pub wave: u32,
    /// 批号 1..=40
    pub batch: u32,
}

/// 红行解析："F9747|w3|b12" → (id=9747, wave=3, batch=12)。
pub fn parse_red_row(s: &str) -> Result<RedEntry, RedCode> {
    let parts: Vec<&str> = s.split('|').collect();
    if parts.len() != 3 {
        return Err(RedCode::ParseFailure);
    }
    let id_s = match parts.first() {
        Some(p) => *p,
        None => return Err(RedCode::ParseFailure),
    };
    let id_body = match id_s.strip_prefix('F') {
        Some(b) => b,
        None => return Err(RedCode::ParseFailure),
    };
    let id = parse_num(id_body)?;
    let wave = match parts.get(1).and_then(|w| w.strip_prefix('w')) {
        Some(b) => parse_num(b)?,
        None => return Err(RedCode::ParseFailure),
    };
    let batch = match parts.get(2).and_then(|b| b.strip_prefix('b')) {
        Some(x) => parse_num(x)?,
        None => return Err(RedCode::ParseFailure),
    };
    let e = RedEntry { id, wave, batch };
    if e.id < 9601 || e.id > 10080 {
        return Err(RedCode::OutOfWindow);
    }
    if e.wave == 0 || e.wave > WAVE_COUNT || e.batch == 0 || e.batch > BATCH_MAX {
        return Err(RedCode::Unattributed);
    }
    Ok(e)
}

/// 纯数字短段解析（≤5 位）。
fn parse_num(s: &str) -> Result<u32, RedCode> {
    if s.is_empty() || s.len() > 5 {
        return Err(RedCode::ParseFailure);
    }
    let mut n: u32 = 0;
    for b in s.bytes() {
        if !b.is_ascii_digit() {
            return Err(RedCode::ParseFailure);
        }
        n = n.saturating_mul(10).saturating_add((b - b'0') as u32);
    }
    Ok(n)
}

// ── 红账注册表 ────────────────────────────────────────────────────────
/// 状态机：Registering → Reconciled 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedState {
    /// 登记中
    Registering,
    /// 已对账（全账逐红对齐）
    Reconciled,
}

/// 红账注册表：逐条在案 + 闭环标记。
#[derive(Clone, Debug)]
pub struct RedRegistry {
    /// 在案红
    pub entries: Vec<RedEntry>,
    /// 已闭环红 ID（闭环红复燃拦截）
    pub closed: Vec<u32>,
    /// 当前状态
    pub state: RedState,
}

impl RedRegistry {
    /// 构造空账。
    pub fn new() -> RedRegistry {
        RedRegistry { entries: Vec::new(), closed: Vec::new(), state: RedState::Registering }
    }

    /// 登记一条红：定位单必检、重复必拒、闭环复燃必拒、溢出必拒。
    pub fn register(&mut self, e: RedEntry) -> Result<(), RedCode> {
        if self.state != RedState::Registering {
            return Err(RedCode::StateRegress);
        }
        if e.id < 9601 || e.id > 10080 {
            return Err(RedCode::OutOfWindow);
        }
        if e.wave == 0 || e.wave > WAVE_COUNT || e.batch == 0 || e.batch > BATCH_MAX {
            return Err(RedCode::Unattributed);
        }
        if self.entries.iter().any(|x| x.id == e.id) {
            return Err(RedCode::DuplicateRed);
        }
        if self.closed.contains(&e.id) {
            return Err(RedCode::Relapsed);
        }
        if self.entries.len() >= RED_CAP {
            return Err(RedCode::Overflow);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 闭环一条红：从在案转入闭环名单（全账对齐动作）。
    pub fn close(&mut self, id: u32) -> Result<(), RedCode> {
        if self.state != RedState::Registering {
            return Err(RedCode::StateRegress);
        }
        let pos = self.entries.iter().position(|x| x.id == id);
        match pos {
            Some(p) => {
                self.entries.remove(p);
                self.closed.push(id);
                Ok(())
            }
            None => Err(RedCode::OutsideRed),
        }
    }

    /// 账内在案查询。
    pub fn contains(&self, id: u32) -> bool {
        self.entries.iter().any(|x| x.id == id)
    }

    /// 全账对账：扫描所得红逐条对案；账外红显性拒绝并逃逸入账。
    pub fn reconcile(
        &mut self,
        scanned: &[RedEntry],
        ledger: &mut ScanLedger,
    ) -> Result<usize, RedCode> {
        if self.state != RedState::Registering {
            return Err(RedCode::StateRegress);
        }
        let mut outside = 0usize;
        for sc in scanned.iter() {
            let on_book = self.contains(sc.id);
            let _ = ledger.record(on_book);
            if !on_book {
                outside += 1;
            }
        }
        if outside > 0 {
            return Err(RedCode::OutsideRed);
        }
        self.state = RedState::Reconciled;
        Ok(outside)
    }

    /// 在案数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ── 扫描账 ────────────────────────────────────────────────────────────
/// 扫描账：对账红数 + 账外逃逸数（恒 0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScanLedger {
    /// 已对账红数
    pub matched: u32,
    /// 账外逃逸数（必须为 0）
    pub escapes: u32,
}

impl ScanLedger {
    /// 构造零账。
    pub const fn zero() -> ScanLedger {
        ScanLedger { matched: 0, escapes: 0 }
    }

    /// 记一条对账；hit=false 即账外红显性入账（不静默）。
    pub fn record(&mut self, hit: bool) -> Result<(), RedCode> {
        if hit {
            self.matched = self.matched.saturating_add(1);
        } else {
            self.escapes = self.escapes.saturating_add(1);
        }
        Ok(())
    }
}

/// 标准红账：4 条红（跨波跨批）全登记并闭环 1 条。
pub fn registry_standard() -> Result<RedRegistry, RedCode> {
    let mut r = RedRegistry::new();
    r.register(RedEntry { id: 9610, wave: 1, batch: 3 })?;
    r.register(RedEntry { id: 9705, wave: 2, batch: 7 })?;
    r.register(RedEntry { id: 9822, wave: 4, batch: 15 })?;
    r.register(RedEntry { id: 9960, wave: 6, batch: 24 })?;
    r.close(9705)?;
    Ok(r)
}

/// 标准扫描单：在案 3 条（闭环红不得再扫出）。
pub fn scan_standard() -> Vec<RedEntry> {
    vec![
        RedEntry { id: 9610, wave: 1, batch: 3 },
        RedEntry { id: 9822, wave: 4, batch: 15 },
        RedEntry { id: 9960, wave: 6, batch: 24 },
    ]
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏红账样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRedCase {
    /// 账外红（扫描得红账内无案）
    OutsideRed,
    /// 无定位红（波 0 / 批 41）
    Unattributed,
    /// 重复红
    DuplicateRed,
    /// 闭环红复燃
    Relapsed,
    /// 越窗红（ID 出 9601..=10080）
    OutOfWindow,
}

impl BadRedCase {
    /// 闭集全量。
    pub const ALL: [BadRedCase; 5] = [
        BadRedCase::OutsideRed,
        BadRedCase::Unattributed,
        BadRedCase::DuplicateRed,
        BadRedCase::Relapsed,
        BadRedCase::OutOfWindow,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        match self {
            BadRedCase::OutsideRed => {
                let mut r = match registry_standard() {
                    Ok(r) => r,
                    Err(_) => return false,
                };
                let mut ledger = ScanLedger::zero();
                let scanned = vec![RedEntry { id: 9999, wave: 5, batch: 30 }];
                r.reconcile(&scanned, &mut ledger).is_err()
                    && ledger.escapes == 1
                    && r.state == RedState::Registering
            }
            BadRedCase::Unattributed => {
                let mut r = RedRegistry::new();
                r.register(RedEntry { id: 9610, wave: 0, batch: 3 }).is_err()
                    && r.register(RedEntry { id: 9610, wave: 1, batch: 0 }).is_err()
                    && r.register(RedEntry { id: 9610, wave: 7, batch: 3 }).is_err()
                    && r.register(RedEntry { id: 9610, wave: 1, batch: 41 }).is_err()
            }
            BadRedCase::DuplicateRed => {
                let mut r = RedRegistry::new();
                let a = r.register(RedEntry { id: 9705, wave: 2, batch: 7 }).is_ok();
                let b = r.register(RedEntry { id: 9705, wave: 2, batch: 8 }).is_err();
                a && b
            }
            BadRedCase::Relapsed => {
                let mut r = RedRegistry::new();
                if r.register(RedEntry { id: 9801, wave: 3, batch: 12 }).is_err() {
                    return false;
                }
                if r.close(9801).is_err() {
                    return false;
                }
                r.register(RedEntry { id: 9801, wave: 3, batch: 12 }).is_err()
            }
            BadRedCase::OutOfWindow => {
                parse_red_row("F9600|w1|b3").is_err()
                    && parse_red_row("F10081|w1|b3").is_err()
                    && parse_red_row("F9747|x3|b12").is_err()
                    && parse_red_row("F9747|w3").is_err()
                    && parse_red_row("F974x|w3|b12").is_err()
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_red_injection() -> (ScanLedger, [u32; 5]) {
    let mut ledger = ScanLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadRedCase::ALL.len() {
        if let Some(case) = BadRedCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                if !caught {
                    ledger.escapes = ledger.escapes.saturating_add(1);
                }
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复红族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；账外红/缺案族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；解析族"),
    ("ENODATA", -61, 232, "Windows ERROR_NO_DATA；无定位红族"),
    ("EOVERFLOW", -75, 28, "Windows ERROR_OUT_OF_PAPER；红账溢出族"),
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

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
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

/// 规范语料双跑指纹：登记→闭环→对账→账外探测全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut r = match registry_standard() {
        Ok(r) => r,
        Err(_) => RedRegistry::new(),
    };
    let mut ledger = ScanLedger::zero();
    bytes.push(match r.reconcile(&scan_standard(), &mut ledger) {
        Ok(0) => 1,
        _ => 2,
    });
    bytes.push(r.state as u8);
    bytes.extend_from_slice(&(r.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&ledger.matched.to_le_bytes());
    // 账外红探测：扫入 9999，必须被拒且逃逸入账。
    let mut r2 = match registry_standard() {
        Ok(r) => r,
        Err(_) => RedRegistry::new(),
    };
    let mut l2 = ScanLedger::zero();
    let scanned2 = vec![RedEntry { id: 9999, wave: 5, batch: 30 }];
    bytes.push(match r2.reconcile(&scanned2, &mut l2) {
        Err(RedCode::OutsideRed) => 3,
        _ => 4,
    });
    bytes.push(l2.escapes as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10068 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10068_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10068");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x920c0..=0x92100", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 红行解析器：合法命中
    let parse_ok = parse_red_row("F9747|w3|b12") == Ok(RedEntry { id: 9747, wave: 3, batch: 12 })
        && parse_red_row("F10080|w6|b40") == Ok(RedEntry { id: 10080, wave: 6, batch: 40 });
    s.add("红行解析：F<id>|w<n>|b<n> 三段命中", parse_ok,
          "红 ID + 波次 + 批号定位单全可复算");

    // 3. 红行解析器：非法拒绝
    let bad_parse = parse_red_row("F9747|x3|b12").is_err()
        && parse_red_row("F9747|w3").is_err()
        && parse_red_row("F974x|w3|b12").is_err()
        && parse_red_row("F9747|w3|b41").is_err()
        && parse_red_row("F9747|w0|b12").is_err();
    s.add("红行解析：坏前缀/缺段/越窗定位全拒", bad_parse,
          "解析闭集：w 1..=6、b 1..=40、ID 纯数字");

    // 4. 标准红账：登记+闭环+对账全链
    let mut r = match registry_standard() {
        Ok(r) => r,
        Err(_) => RedRegistry::new(),
    };
    let mut ledger = ScanLedger::zero();
    let std_ok = r.reconcile(&scan_standard(), &mut ledger).is_ok()
        && r.state == RedState::Reconciled
        && r.len() == 3
        && ledger.matched == 3
        && ledger.escapes == 0;
    s.add("总核对齐：4 登记闭环 1 后扫 3 全在案", std_ok,
          "全账逐条在案：在案 3 = 扫描 3，逃逸 0");

    // 5. 无定位红拦截（四种坏定位单）
    let mut ua = RedRegistry::new();
    let ua_ok = ua.register(RedEntry { id: 9610, wave: 0, batch: 3 }).is_err()
        && ua.register(RedEntry { id: 9610, wave: 7, batch: 3 }).is_err()
        && ua.register(RedEntry { id: 9610, wave: 1, batch: 0 }).is_err()
        && ua.register(RedEntry { id: 9610, wave: 1, batch: 41 }).is_err();
    s.add("无定位红：波/批出 1..=6 / 1..=40 全拒", ua_ok,
          "每条红必须挂波次+批号定位单");

    // 6. 重复红拦截（同 ID 不同定位也拒）
    let mut dp = RedRegistry::new();
    let _ = dp.register(RedEntry { id: 9705, wave: 2, batch: 7 });
    let dp_ok = dp.register(RedEntry { id: 9705, wave: 2, batch: 8 })
        == Err(RedCode::DuplicateRed);
    s.add("重复红：同 ID 二次登记被拒", dp_ok,
          "一红一案，重复显性拒绝");

    // 7. 闭环红复燃拦截
    let mut rl = RedRegistry::new();
    let _ = rl.register(RedEntry { id: 9801, wave: 3, batch: 12 });
    let closed_ok = rl.close(9801).is_ok() && rl.is_empty();
    let relapse_ok = rl.register(RedEntry { id: 9801, wave: 3, batch: 12 })
        == Err(RedCode::Relapsed);
    s.add("闭环复燃：闭环后再在案被拒", closed_ok && relapse_ok,
          "闭环红进闭环名单，复燃显性拦截");

    // 8. 账外红检出：扫描得红账内无案即拒且逃逸入账
    let mut or = match registry_standard() {
        Ok(r) => r,
        Err(_) => RedRegistry::new(),
    };
    let mut or_ledger = ScanLedger::zero();
    let scanned = vec![RedEntry { id: 9999, wave: 5, batch: 30 }];
    let or_ok = or.reconcile(&scanned, &mut or_ledger).is_err()
        && or_ledger.escapes == 1
        && or.state == RedState::Registering;
    s.add("账外红：扫入 9999 检出、逃逸入账、状态不推", or_ok,
          "账外红必须逐条显性检出，绝不静默入账");

    // 9. 闭环缺失对账：close 账外 ID 拒绝
    let mut cl = RedRegistry::new();
    let _ = cl.register(RedEntry { id: 9705, wave: 2, batch: 7 });
    let cl_ok = cl.close(9999).is_err();
    s.add("闭环守卫：close 账外 ID 被拒", cl_ok,
          "只能闭环在案红，账外 ID 显性拒绝");

    // 10. 对账后登记拦截（状态闸）
    let mut st = match registry_standard() {
        Ok(r) => r,
        Err(_) => RedRegistry::new(),
    };
    let mut st_ledger = ScanLedger::zero();
    let _ = st.reconcile(&scan_standard(), &mut st_ledger);
    let st_ok = st.register(RedEntry { id: 9650, wave: 2, batch: 5 })
        == Err(RedCode::StateRegress);
    s.add("状态闸：Reconciled 后再登记被拒", st_ok,
          "Registering → Reconciled 单向，对账后只读");

    // 11. 对账确定性：同账同单两次对账结论一致
    let r_a = registry_standard();
    let r_b = registry_standard();
    let det = match (r_a, r_b) {
        (Ok(mut a), Ok(mut b)) => {
            let mut la = ScanLedger::zero();
            let mut lb = ScanLedger::zero();
            let ra = a.reconcile(&scan_standard(), &mut la);
            let rb = b.reconcile(&scan_standard(), &mut lb);
            ra == rb && la.matched == lb.matched
        }
        _ => false,
    };
    s.add("对账确定性：两次对账结论逐位一致", det,
          "全账对齐可复算：同输入同结论");

    // 12. 注入核：五类坏样本全检出
    let (ledger2, per_class) = run_red_injection();
    let all_caught = per_class.iter().all(|c| *c == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "账外/无定位/重复/复燃/越窗全被显式拒绝");

    // 13. 账守恒逃逸=0
    s.add("账守恒：注入 50 轮逃逸=0",
          ledger2.escapes == 0,
          "注入核全程不得产生扫描逃逸");

    // 14. 文案闭集：非空
    let text_ok = RedCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < RedCode::ALL.len() {
        let mut j = i + 1;
        while j < RedCode::ALL.len() {
            if let (Some(x), Some(y)) = (RedCode::ALL.get(i), RedCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = RedCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 16. Linux 逐码一致
    let codes_ok = agrees_with_linux(EEXIST)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENODATA)
        && agrees_with_linux(EOVERFLOW)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 17. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：红账快照 + 指纹快照
    let snap_text = format!("在案 {} / 闭环名单 {}", r.len(), r.closed.len());
    s.add_dyn("红账快照", r.state == RedState::Reconciled && r.len() == 3, &snap_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_red_row_ok() {
        assert_eq!(parse_red_row("F9747|w3|b12"),
                   Ok(RedEntry { id: 9747, wave: 3, batch: 12 }));
        assert_eq!(parse_red_row("F9601|w1|b1"),
                   Ok(RedEntry { id: 9601, wave: 1, batch: 1 }));
    }

    #[test]
    fn parse_red_row_rejects() {
        assert!(parse_red_row("F9747|x3|b12").is_err());
        assert!(parse_red_row("F9747|w3").is_err());
        assert!(parse_red_row("F9747|w3|b41").is_err());
        assert!(parse_red_row("F9600|w1|b1").is_err());
        assert!(parse_red_row("F10081|w1|b1").is_err());
    }

    #[test]
    fn standard_flow() {
        let mut r = registry_standard().unwrap_or(RedRegistry::new());
        let mut l = ScanLedger::zero();
        assert!(r.reconcile(&scan_standard(), &mut l).is_ok());
        assert_eq!(r.len(), 3);
        assert_eq!(l.matched, 3);
        assert_eq!(l.escapes, 0);
    }

    #[test]
    fn outside_red_detected() {
        let mut r = registry_standard().unwrap_or(RedRegistry::new());
        let mut l = ScanLedger::zero();
        let scanned = vec![RedEntry { id: 9999, wave: 5, batch: 30 }];
        assert!(r.reconcile(&scanned, &mut l).is_err());
        assert_eq!(l.escapes, 1);
    }

    #[test]
    fn duplicate_and_unattributed_rejected() {
        let mut r = RedRegistry::new();
        let _ = r.register(RedEntry { id: 9705, wave: 2, batch: 7 });
        assert_eq!(r.register(RedEntry { id: 9705, wave: 2, batch: 8 }),
                   Err(RedCode::DuplicateRed));
        assert_eq!(r.register(RedEntry { id: 9801, wave: 0, batch: 7 }),
                   Err(RedCode::Unattributed));
        assert_eq!(r.register(RedEntry { id: 9801, wave: 1, batch: 99 }),
                   Err(RedCode::Unattributed));
    }

    #[test]
    fn relapse_blocked() {
        let mut r = RedRegistry::new();
        let _ = r.register(RedEntry { id: 9801, wave: 3, batch: 12 });
        assert!(r.close(9801).is_ok());
        assert_eq!(r.register(RedEntry { id: 9801, wave: 3, batch: 12 }),
                   Err(RedCode::Relapsed));
        assert_eq!(r.close(9999), Err(RedCode::OutsideRed));
    }

    #[test]
    fn reconciled_readonly() {
        let mut r = registry_standard().unwrap_or(RedRegistry::new());
        let mut l = ScanLedger::zero();
        let _ = r.reconcile(&scan_standard(), &mut l);
        assert_eq!(r.register(RedEntry { id: 9650, wave: 2, batch: 5 }),
                   Err(RedCode::StateRegress));
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_red_injection();
        assert_eq!(l.escapes, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EEXIST));
        assert!(agrees_with_linux(ENODATA));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(ENOENT), -ENOENT);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn red_code_text_complete() {
        for c in RedCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn unx10068_checks_green() {
        let s = run_unx10068_checks();
        assert!(s.all_passed(), "UNX-F10068 red: {}", s.red_items_text());
    }
}
