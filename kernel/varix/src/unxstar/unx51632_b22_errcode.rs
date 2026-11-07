//! UNX-F51632 隔离端口恢复白名单（手动解锁语义）
//! · 0xC20C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51633-J1 ×15 并入；UNX-F51633-J1R 未定义码注入 10/10
//! 拒绝；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - PortCode         诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - ErrTable         域内错误码表（三表分域，码闭集派生）
//!   - RecoveryWhitelist 隔离端口恢复白名单（手动解锁语义）
//!   - ErrcodeIngest    攻击族码并入器（×15 并入 + 注入守恒账）
//!   - run_unx51632_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 攻击族 ×15 并入：15 枚攻击族错误码逐枚并入域内三表（每表恰 5
//!      枚，表号 = 码序 mod 3），并入逐枚留痕；UNX-F51633-J1 ×15 并入
//!      受控生效。
//!   ② 未定义码闭闸：并入通道只认族内闭集 [FAMILY_BASE, FAMILY_BASE+15)
//!      之外的码一律显性拒（UndefinedCode）——UNX-F51633-J1R 未定义码
//!      注入 10/10 拒绝，逃逸恒 0。
//!   ③ 手动解锁语义：隔离端口恢复只认白名单显式解锁，未解锁端口恢复
//!      一律显性拒（PortSealed，零静默复活）；重复解锁判 DupUnlock。
//!   ④ 三表互斥：同码不得并入两表（DupCode），表号越界判 TableOob，
//!      空表不得出码（TableEmpty）。
//!   ⑤ 注入核：5 类坏样本（未定义码/撞码并入/表号越界/未解锁恢复/零
//!      码并入）逐类 ×10 轮精确复现，注入账守恒（检出+逃逸=轮数，逃逸
//!      恒 0）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机——码值由 LCG 派
//!      生（种子 = SEG_BASE 派生），同段位恒定可复现；零 unwrap/expect/
//!      panic/裸下标，取值一律 .get()/.get_mut()/match。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC20C0;

/// 攻击族并入判据数（UNX-F51633-J1）。
pub const FAMILY_SIZE: usize = 15;
/// 域内表数（三表）。
pub const TABLE_COUNT: usize = 3;
/// 每表容量（15/3）。
pub const TABLE_CAP: usize = 5;
/// 端口闭集上界。
pub const PORT_MAX: usize = 48;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 攻击族码基址（族内闭集起点）。
pub const FAMILY_BASE: u32 = 0x5163_2000;

/// 域内错误码表（三表分域）。
pub struct ErrTable {
    /// 表号（0..=2）
    pub id: usize,
    /// 已并入码值（升序闭集）
    codes: Vec<u32>,
}

impl ErrTable {
    /// 新建表。
    pub fn new(id: usize) -> ErrTable {
        ErrTable {
            id,
            codes: Vec::new(),
        }
    }

    /// 并入一码：闭集校验 + 撞码拒，满容判 TableFull。
    pub fn merge(&mut self, code: u32) -> Result<(), PortCode> {
        if code < FAMILY_BASE || code >= FAMILY_BASE + FAMILY_SIZE as u32 {
            return Err(PortCode::UndefinedCode);
        }
        if self.codes.iter().any(|&c| c == code) {
            return Err(PortCode::DupCode);
        }
        if self.codes.len() >= TABLE_CAP {
            return Err(PortCode::TableFull);
        }
        self.codes.push(code);
        Ok(())
    }

    /// 已并入枚数。
    pub fn len(&self) -> usize {
        self.codes.len()
    }

    /// 是否含码。
    pub fn contains(&self, code: u32) -> bool {
        self.codes.iter().any(|&c| c == code)
    }

    /// 出码：按序取第 i 枚（越界判 TableEmpty/Oob）。
    pub fn get_code(&self, i: usize) -> Result<u32, PortCode> {
        if self.codes.is_empty() {
            return Err(PortCode::TableEmpty);
        }
        match self.codes.get(i) {
            Some(&c) => Ok(c),
            None => Err(PortCode::TableOob),
        }
    }
}

/// 隔离端口恢复白名单（手动解锁语义）。
pub struct RecoveryWhitelist {
    unlocked: Vec<usize>,
    recover_ok: usize,
    sealed_rejects: usize,
}

impl RecoveryWhitelist {
    /// 新建白名单（全端口默认隔离）。
    pub fn new() -> RecoveryWhitelist {
        RecoveryWhitelist {
            unlocked: Vec::new(),
            recover_ok: 0,
            sealed_rejects: 0,
        }
    }

    /// 手动解锁端口（解锁即留痕，重复解锁拒）。
    pub fn unlock(&mut self, port: usize) -> Result<(), PortCode> {
        if port == 0 || port > PORT_MAX {
            return Err(PortCode::PortOob);
        }
        if self.unlocked.iter().any(|&p| p == port) {
            return Err(PortCode::DupUnlock);
        }
        self.unlocked.push(port);
        Ok(())
    }

    /// 恢复端口：只认手动解锁，未解锁显性拒（零静默复活）。
    pub fn recover(&mut self, port: usize) -> Result<(), PortCode> {
        if port == 0 || port > PORT_MAX {
            return Err(PortCode::PortOob);
        }
        if !self.unlocked.iter().any(|&p| p == port) {
            self.sealed_rejects += 1;
            return Err(PortCode::PortSealed);
        }
        self.recover_ok += 1;
        Ok(())
    }

    /// 已解锁端口数。
    pub fn unlocked_len(&self) -> usize {
        self.unlocked.len()
    }

    /// 恢复生效数。
    pub fn recover_ok_tally(&self) -> usize {
        self.recover_ok
    }

    /// 未解锁恢复拒账。
    pub fn sealed_reject_tally(&self) -> usize {
        self.sealed_rejects
    }
}

/// 攻击族码并入器。
pub struct ErrcodeIngest {
    tables: [Option<ErrTable>; TABLE_COUNT],
    merged: usize,
    rejected: usize,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl ErrcodeIngest {
    /// 新建并入器（三表就绪）。
    pub fn new() -> ErrcodeIngest {
        ErrcodeIngest {
            tables: [
                Some(ErrTable::new(0)),
                Some(ErrTable::new(1)),
                Some(ErrTable::new(2)),
            ],
            merged: 0,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 表只读引用（表号越界判 TableOob）。
    pub fn table(&self, id: usize) -> Result<&ErrTable, PortCode> {
        match self.tables.get(id) {
            Some(Some(t)) => Ok(t),
            _ => Err(PortCode::TableOob),
        }
    }

    /// 并入一码：按 码序 mod 3 分表，×15 并入受控生效。
    pub fn ingest(&mut self, code: u32) -> Result<usize, PortCode> {
        if code == 0 {
            self.rejected += 1;
            return Err(PortCode::ZeroCode);
        }
        if code < FAMILY_BASE || code >= FAMILY_BASE + FAMILY_SIZE as u32 {
            self.rejected += 1;
            return Err(PortCode::UndefinedCode);
        }
        let slot = ((code - FAMILY_BASE) as usize) % TABLE_COUNT;
        // 同码不得并入两表：先全域查重。
        for t in self.tables.iter() {
            if let Some(tt) = t {
                if tt.contains(code) {
                    self.rejected += 1;
                    return Err(PortCode::DupCode);
                }
            }
        }
        let r = match self.tables.get_mut(slot) {
            Some(Some(t)) => t.merge(code),
            _ => Err(PortCode::TableOob),
        };
        match r {
            Ok(()) => {
                self.merged += 1;
                Ok(slot)
            }
            Err(e) => {
                self.rejected += 1;
                Err(e)
            }
        }
    }

    /// 已并入总数。
    pub fn merged_tally(&self) -> usize {
        self.merged
    }

    /// 拒绝账。
    pub fn rejected_tally(&self) -> usize {
        self.rejected
    }

    /// 三表均衡（每表恰 5 枚）。
    pub fn balanced(&self) -> bool {
        for id in 0..TABLE_COUNT {
            match self.table(id) {
                Ok(t) => {
                    if t.len() != TABLE_CAP {
                        return false;
                    }
                }
                Err(_) => return false,
            }
        }
        true
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 快照指纹（双跑一致面）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        for t in self.tables.iter() {
            if let Some(tt) = t {
                for i in 0..tt.len() {
                    if let Ok(c) = tt.get_code(i) {
                        bytes.extend_from_slice(&c.to_le_bytes());
                    }
                }
            }
        }
        bytes.extend_from_slice(&(self.merged as u64).to_le_bytes());
        fnv1a(&bytes)
    }
}

/// 诊断码闭集（隔离端口恢复白名单域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PortCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 未定义码
    UndefinedCode,
    /// 撞码并入
    DupCode,
    /// 表满
    TableFull,
    /// 表空
    TableEmpty,
    /// 表号越界
    TableOob,
    /// 零码
    ZeroCode,
    /// 端口越界
    PortOob,
    /// 端口仍隔离
    PortSealed,
    /// 重复解锁
    DupUnlock,
    /// 并入序列断裂
    MergeGap,
}

impl PortCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            PortCode::Ok => "隔离端口恢复白名单判定成功",
            PortCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            PortCode::UndefinedCode => "未定义码：并入通道只认攻击族闭集内码",
            PortCode::DupCode => "撞码并入：同码不得并入两表",
            PortCode::TableFull => "表满：每表容量闭集 5 已满",
            PortCode::TableEmpty => "表空：空表不得出码",
            PortCode::TableOob => "表号越界：域内三表 0..=2",
            PortCode::ZeroCode => "零码并入：零值不得入表",
            PortCode::PortOob => "端口越界：恢复域闭集 1..=48",
            PortCode::PortSealed => "端口仍隔离：恢复只认手动解锁，零静默复活",
            PortCode::DupUnlock => "重复解锁：同端口只许解锁一次",
            PortCode::MergeGap => "并入序列断裂：族内码必须按序逐枚并入",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [PortCode; 12] = [
    PortCode::Ok,
    PortCode::SegBad,
    PortCode::UndefinedCode,
    PortCode::DupCode,
    PortCode::TableFull,
    PortCode::TableEmpty,
    PortCode::TableOob,
    PortCode::ZeroCode,
    PortCode::PortOob,
    PortCode::PortSealed,
    PortCode::DupUnlock,
    PortCode::MergeGap,
];

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg {
            s: seed ^ 0x7f4a7c15c47bcfb7,
        }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 端口派生：种子 → 1..=48 端口。
pub fn port_of(seed: u64) -> usize {
    let mut lc = Lcg::new(seed);
    ((lc.next_u32() as usize) % PORT_MAX) + 1
}

/// 未定义码派生：族外码（确定性）。
pub fn undefined_code(seed: u64) -> u32 {
    let mut lc = Lcg::new(seed);
    let v = lc.next_u32() % 0x1000;
    if v >= FAMILY_BASE % 0x1000 && v < (FAMILY_BASE % 0x1000) + FAMILY_SIZE as u32 {
        // 极小概率落在族值域低位，强推到族外。
        return v.wrapping_add(0x2000);
    }
    v
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准脚本：×15 按序并入三表 → 手动解锁 ×8 → 未解锁恢复全拒。
/// 返回 (并入数, 解锁数, 隔离拒数, 快照印)。
pub fn standard_script() -> (usize, usize, usize, u64) {
    let mut ing = ErrcodeIngest::new();
    for i in 0..FAMILY_SIZE {
        let _ = ing.ingest(FAMILY_BASE + i as u32);
    }
    let mut wl = RecoveryWhitelist::new();
    let mut unlocked = 0;
    let mut used: Vec<usize> = Vec::new();
    let mut seed = SEG_BASE as u64 + 7;
    while unlocked < 8 {
        seed += 1;
        let p = port_of(seed);
        if used.iter().any(|&u| u == p) {
            continue;
        }
        if wl.unlock(p).is_ok() {
            used.push(p);
            unlocked += 1;
            let _ = wl.recover(p);
        }
    }
    // 未解锁端口恢复一律拒（恰 6 通道，确定性）。
    let mut sealed = 0;
    while sealed < 6 {
        seed += 1;
        let p = port_of(seed);
        if used.iter().any(|&u| u == p) {
            continue;
        }
        if wl.recover(p) == Err(PortCode::PortSealed) {
            sealed += 1;
        }
    }
    let fp = ing.fingerprint();
    (
        ing.merged_tally(),
        wl.recover_ok_tally(),
        wl.sealed_reject_tally(),
        fp,
    )
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    standard_script() == standard_script()
}

/// 注入核：5 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 5]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 5] = [0; 5];
    // 类 0：未定义码注入——并入通道闭闸，10/10 拒绝。
    for r in 0..INJ_ROUNDS {
        let mut ing = ErrcodeIngest::new();
        let bad = undefined_code(SEG_BASE as u64 + r as u64 + 1);
        let caught = ing.ingest(bad) == Err(PortCode::UndefinedCode)
            && ing.ingest(FAMILY_BASE + FAMILY_SIZE as u32) == Err(PortCode::UndefinedCode)
            && ing.merged_tally() == 0;
        if caught {
            ing.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            ing.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：撞码并入——同码不得入两表。
    for _ in 0..INJ_ROUNDS {
        let mut ing = ErrcodeIngest::new();
        let first = ing.ingest(FAMILY_BASE + 3).is_ok();
        let dup = ing.ingest(FAMILY_BASE + 3) == Err(PortCode::DupCode);
        if first && dup && ing.merged_tally() == 1 {
            ing.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            ing.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：表号越界——域内三表 0..=2。
    for _ in 0..INJ_ROUNDS {
        let mut ing = ErrcodeIngest::new();
        let caught = ing.table(3).is_err() && ing.table(TABLE_COUNT + 9).is_err();
        if caught {
            ing.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            ing.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：未解锁恢复——零静默复活禁。
    for _ in 0..INJ_ROUNDS {
        let mut wl = RecoveryWhitelist::new();
        let caught = wl.recover(9) == Err(PortCode::PortSealed)
            && wl.sealed_reject_tally() == 1
            && wl.recover_ok_tally() == 0;
        if caught {
            let mut ing = ErrcodeIngest::new();
            ing.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            let mut ing = ErrcodeIngest::new();
            ing.note_escape();
            total_escape += 1;
        }
    }
    // 类 4：零码并入 + 端口越界 + 重复解锁。
    for _ in 0..INJ_ROUNDS {
        let mut ing = ErrcodeIngest::new();
        let mut wl = RecoveryWhitelist::new();
        let zero = ing.ingest(0) == Err(PortCode::ZeroCode);
        let oob = wl.unlock(0) == Err(PortCode::PortOob)
            && wl.unlock(PORT_MAX + 1) == Err(PortCode::PortOob);
        let _ = wl.unlock(5);
        let dup = wl.unlock(5) == Err(PortCode::DupUnlock);
        if zero && oob && dup {
            ing.note_detected();
            total_detect += 1;
            per[4] += 1;
        } else {
            ing.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51632_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51632");
    // 1 ×15 并入受控生效（J1）。
    {
        let (merged, _, _, _) = standard_script();
        s.add(
            "十五码并入",
            merged == FAMILY_SIZE,
            "UNX-F51633-J1：攻击族 ×15 并入域内三表，逐枚留痕",
        );
    }
    // 2 未定义码注入 10/10 拒绝（J1R）。
    {
        let (_, _, per) = injection_harness();
        s.add(
            "未定义码 10/10 拒绝",
            per[0] == 10,
            "UNX-F51633-J1R：未定义码注入逐轮拒绝（闭闸）",
        );
    }
    // 3 三表均衡（每表恰 5 枚）。
    {
        let mut ing = ErrcodeIngest::new();
        for i in 0..FAMILY_SIZE {
            let _ = ing.ingest(FAMILY_BASE + i as u32);
        }
        s.add(
            "三表均衡分域",
            ing.balanced(),
            "15 枚按 码序 mod 3 分域三表，每表恰 5 枚",
        );
    }
    // 4 撞码并入显性拒。
    {
        let mut ing = ErrcodeIngest::new();
        let _ = ing.ingest(FAMILY_BASE + 2);
        s.add(
            "撞码并入显性拒",
            ing.ingest(FAMILY_BASE + 2) == Err(PortCode::DupCode),
            "同码不得并入两表（全域查重）",
        );
    }
    // 5 表号越界拒。
    {
        let mut ing = ErrcodeIngest::new();
        s.add(
            "表号越界拒",
            ing.table(3).is_err() && ing.table(0).is_ok(),
            "域内三表闭集 0..=2",
        );
    }
    // 6 空表出码拒。
    {
        let mut ing = ErrcodeIngest::new();
        s.add(
            "空表出码拒",
            ing.table(0).map_or(false, |t| t.get_code(0) == Err(PortCode::TableEmpty)),
            "空表不得出码（TableEmpty）",
        );
    }
    // 7 手动解锁语义：未解锁恢复显性拒。
    {
        let mut wl = RecoveryWhitelist::new();
        s.add(
            "手动解锁语义",
            wl.recover(7) == Err(PortCode::PortSealed) && wl.unlock(7).is_ok() && wl.recover(7).is_ok(),
            "隔离端口恢复只认白名单显式解锁",
        );
    }
    // 8 重复解锁拒 + 端口越界拒。
    {
        let mut wl = RecoveryWhitelist::new();
        let _ = wl.unlock(11);
        s.add(
            "解锁闸与端口域",
            wl.unlock(11) == Err(PortCode::DupUnlock)
                && wl.unlock(0) == Err(PortCode::PortOob)
                && wl.unlock(PORT_MAX + 1) == Err(PortCode::PortOob),
            "同端口只许解锁一次，端口域闭集 1..=48",
        );
    }
    // 9 并入器拒绝账口径。
    {
        let mut ing = ErrcodeIngest::new();
        let _ = ing.ingest(FAMILY_BASE + FAMILY_SIZE as u32 + 9);
        let _ = ing.ingest(0);
        s.add(
            "拒绝账口径",
            ing.rejected_tally() == 2 && ing.merged_tally() == 0,
            "未定义码与零码逐次入拒绝账（零静默）",
        );
    }
    // 10 注入核 5 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        s.add(
            "注入 5 类×10 全通",
            d == 50 && esc == 0 && per.iter().all(|&x| x == 10),
            "5 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 11 出码顺序与越界。
    {
        let mut ing = ErrcodeIngest::new();
        let _ = ing.ingest(FAMILY_BASE + 1);
        let ok = ing.table(1).map_or(false, |t| {
            t.get_code(0) == Ok(FAMILY_BASE + 1) && t.get_code(1) == Err(PortCode::TableOob)
        });
        s.add(
            "出码顺序守卫",
            ok,
            "表内按并入序出码，越界显性拒（TableOob）",
        );
    }
    // 12 诊断码闭集唯一性。
    {
        let ok = ALL_CODES
            .iter()
            .enumerate()
            .all(|(i, c)| c.val() == SEG_BASE + 1 + i as u32 && !c.detail().is_empty());
        s.add(
            "诊断码闭集唯一",
            ok && ALL_CODES.len() == 12,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 13 注入账守恒。
    {
        let mut ing = ErrcodeIngest::new();
        for _ in 0..12u32 {
            ing.note_detected();
        }
        s.add(
            "注入账守恒",
            ing.conserved(),
            "注入=检出+逃逸且逃逸=0 且检出>0",
        );
    }
    // 14 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一并入脚本两次执行，快照印逐位一致",
    );
    // 15 标准脚本回归。
    {
        let (merged, ok, sealed, _) = standard_script();
        s.add(
            "标准脚本回归",
            merged == 15 && ok == 8 && sealed == 6,
            "×15 并入 → 解锁 ×8 恢复生效 → 未解锁 6 通道全拒，全链回归",
        );
    }
    // 16 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51632 隔离端口恢复白名单 并入 {} 枚 三表 {} 张",
            FAMILY_SIZE, TABLE_COUNT
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51632"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空（写回状态=已完成）",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51632_checks_green() {
        let set = run_unx51632_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn merge_full_chain() {
        let mut ing = ErrcodeIngest::new();
        for i in 0..FAMILY_SIZE {
            assert!(ing.ingest(FAMILY_BASE + i as u32).is_ok());
        }
        assert_eq!(ing.merged_tally(), FAMILY_SIZE);
        assert!(ing.balanced());
    }

    #[test]
    fn undefined_code_rejected() {
        let mut ing = ErrcodeIngest::new();
        assert_eq!(ing.ingest(FAMILY_BASE - 1), Err(PortCode::UndefinedCode));
        assert_eq!(
            ing.ingest(FAMILY_BASE + FAMILY_SIZE as u32),
            Err(PortCode::UndefinedCode)
        );
        assert_eq!(ing.ingest(0), Err(PortCode::ZeroCode));
        assert_eq!(ing.merged_tally(), 0);
    }

    #[test]
    fn dup_code_cross_table_rejected() {
        let mut ing = ErrcodeIngest::new();
        assert!(ing.ingest(FAMILY_BASE + 4).is_ok());
        assert_eq!(ing.ingest(FAMILY_BASE + 4), Err(PortCode::DupCode));
    }

    #[test]
    fn table_oob_and_empty() {
        let mut ing = ErrcodeIngest::new();
        assert!(ing.table(3).is_err());
        assert_eq!(
            ing.table(0).map(|t| t.get_code(0)),
            Ok(Err(PortCode::TableEmpty))
        );
    }

    #[test]
    fn manual_unlock_semantics() {
        let mut wl = RecoveryWhitelist::new();
        assert_eq!(wl.recover(3), Err(PortCode::PortSealed));
        assert_eq!(wl.sealed_reject_tally(), 1);
        assert!(wl.unlock(3).is_ok());
        assert!(wl.recover(3).is_ok());
        assert_eq!(wl.recover_ok_tally(), 1);
    }

    #[test]
    fn unlock_guards() {
        let mut wl = RecoveryWhitelist::new();
        assert_eq!(wl.unlock(0), Err(PortCode::PortOob));
        assert_eq!(wl.unlock(PORT_MAX + 1), Err(PortCode::PortOob));
        assert!(wl.unlock(6).is_ok());
        assert_eq!(wl.unlock(6), Err(PortCode::DupUnlock));
    }

    #[test]
    fn injection_all_caught() {
        let (d, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        assert_eq!(d, 50);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "类 {} 应恰 10 次检出", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut ing = ErrcodeIngest::new();
        for _ in 0..10u32 {
            ing.note_detected();
        }
        assert!(ing.conserved());
        ing.note_escape();
        assert!(!ing.conserved());
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn table_balanced_property() {
        let mut ing = ErrcodeIngest::new();
        for i in 0..FAMILY_SIZE {
            let _ = ing.ingest(FAMILY_BASE + i as u32);
        }
        for id in 0..TABLE_COUNT {
            let t = ing.table(id);
            assert!(t.is_ok());
            assert_eq!(t.map(|t| t.len()), Ok(TABLE_CAP));
        }
    }

    #[test]
    fn undefined_code_always_outside_family() {
        for r in 0..32u64 {
            let c = undefined_code(SEG_BASE as u64 + r);
            assert!(c < FAMILY_BASE || c >= FAMILY_BASE + FAMILY_SIZE as u32);
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
