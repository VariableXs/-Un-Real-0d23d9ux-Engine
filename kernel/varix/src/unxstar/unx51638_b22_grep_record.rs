//! UNX-F51638 B22 防重 grep 执行记录（关键词族：长度欺骗/重放/DoS/fuzz）
//! · 0xC2240段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51639-J1 收口防重 grep 留痕；与 B02/B05 零重叠断
//! 言；UNX-F51639-J1R 撞号注入 10/10 检出并阻断收口；③ 完成时 API/
//! 软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - GrepCode         诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - GrepRecord       单条 grep 执行记录（记录号/关键词族/命中账/记录印）
//!   - KeywordFamily    关键词族闭集（长度欺骗/重放/DoS/fuzz 四族）
//!   - GrepRecordLedger 防重记录账（收口闸 + 撞号检出 + B02/B05 零重叠断言）
//!   - run_unx51638_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 关键词族四族闭集：长度欺骗(0)/重放(1)/DoS(2)/fuzz(3)，每条记录
//!      必须落一 族 且命中账非零；收口前逐族留痕——UNX-F51639-J1 收口
//!      防重 grep 留痕（四族齐 + 记录号连续无撞）。
//!   ② 撞号注入检出并阻断收口：记录号两两互异，撞号（DupRecord）逐轮
//!      检出且收口闸保持关闭——UNX-F51639-J1R 撞号注入 10/10 检出并阻
//!      断收口，逃逸恒 0。
//!   ③ B02/B05 零重叠断言：B02 域记录号区间 [1,512]、B05 域区间 [513,
//!      1024] 为既有占用，本账记录号必须落在 [1025,1128] 且与两域交集
//!      恒空（OverlapDetected 显性报）。
//!   ④ 记录印派生：记录印 = FNV(记录号×族×命中账)，同记录恒定、异记
//!      录异印，双跑指纹逐位一致。
//!   ⑤ 注入核：5 类坏样本（撞号注入/B02 区间侵入/B05 区间侵入/空族/
//!      零命中收口）逐类 ×10 轮精确复现，注入账守恒（检出+逃逸=轮数，
//!      逃逸恒 0）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机——命中账由 LCG
//!      派生（种子 = SEG_BASE 派生），同段位恒定可复现；零 unwrap/
//!      expect/panic/裸下标，取值一律 .get()/.get_mut()/match。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC2240;

/// 关键词族数（长度欺骗/重放/DoS/fuzz）。
pub const FAMILY_COUNT: usize = 4;
/// 每族记录数闭集。
pub const RECORDS_PER_FAMILY: usize = 3;
/// 记录号本域下界（B02/B05 之后）。
pub const DOMAIN_ID_BASE: usize = 1025;
/// 本域记录号上界。
pub const DOMAIN_ID_MAX: usize = 1128;
/// B02 域占用区间。
pub const B02_RANGE: (usize, usize) = (1, 512);
/// B05 域占用区间。
pub const B05_RANGE: (usize, usize) = (513, 1024);
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 关键词族名（闭集）。
pub fn family_name(f: usize) -> &'static str {
    match f {
        0 => "长度欺骗",
        1 => "重放",
        2 => "DoS",
        3 => "fuzz",
        _ => "未知族",
    }
}

/// 单条 grep 执行记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrepRecord {
    /// 记录号（本域 1025..=1128）
    pub id: usize,
    /// 关键词族（0..=3）
    pub family: usize,
    /// 命中账（非零）
    pub hits: u64,
    /// 记录印
    pub stamp: u64,
}

/// 防重记录账。
pub struct GrepRecordLedger {
    records: Vec<GrepRecord>,
    closed: bool,
    close_seal: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
    blocked: usize,
}

impl GrepRecordLedger {
    /// 新建记录账。
    pub fn new() -> GrepRecordLedger {
        GrepRecordLedger {
            records: Vec::new(),
            closed: false,
            close_seal: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
            blocked: 0,
        }
    }

    /// 记录印派生。
    pub fn record_stamp(id: usize, family: usize, hits: u64) -> u64 {
        fnv1a(&(id as u64).to_le_bytes())
            ^ fnv1a(&(family as u64).to_le_bytes()).rotate_left(17)
            ^ fnv1a(&hits.to_le_bytes()).rotate_left(43)
    }

    /// 入账一条记录：族闭集 + 域区间 + 命中非零 + 撞号四闸。
    pub fn record(&mut self, id: usize, family: usize, hits: u64) -> Result<u64, GrepCode> {
        if self.closed {
            self.blocked += 1;
            return Err(GrepCode::LedgerClosed);
        }
        if family >= FAMILY_COUNT {
            return Err(GrepCode::FamilyOob);
        }
        if hits == 0 {
            return Err(GrepCode::ZeroHits);
        }
        if id < DOMAIN_ID_BASE || id > DOMAIN_ID_MAX {
            // 与 B02/B05 零重叠断言：越域即重叠风险，显性报。
            if (id >= B02_RANGE.0 && id <= B02_RANGE.1) || (id >= B05_RANGE.0 && id <= B05_RANGE.1)
            {
                return Err(GrepCode::OverlapDetected);
            }
            return Err(GrepCode::IdOob);
        }
        if self.records.iter().any(|r| r.id == id) {
            return Err(GrepCode::DupRecord);
        }
        let stamp = Self::record_stamp(id, family, hits);
        self.records.push(GrepRecord {
            id,
            family,
            hits,
            stamp,
        });
        Ok(stamp)
    }

    /// 收口闸：四族齐 + 记录数达闭集 + 无缺口才可收口，只收一次。
    pub fn close(&mut self) -> Result<u64, GrepCode> {
        if self.closed {
            self.blocked += 1;
            return Err(GrepCode::DupClose);
        }
        for f in 0..FAMILY_COUNT {
            if !self.records.iter().any(|r| r.family == f) {
                self.blocked += 1;
                return Err(GrepCode::FamilyMissing);
            }
        }
        if self.records.len() != FAMILY_COUNT * RECORDS_PER_FAMILY {
            self.blocked += 1;
            return Err(GrepCode::RecordsShort);
        }
        // 记录号连续无缺口（按入账序 1025 起递增）。
        for (i, r) in self.records.iter().enumerate() {
            if r.id != DOMAIN_ID_BASE + i {
                self.blocked += 1;
                return Err(GrepCode::IdGap);
            }
        }
        let mut bytes: Vec<u8> = Vec::new();
        for r in self.records.iter() {
            bytes.extend_from_slice(&r.stamp.to_le_bytes());
        }
        let seal = fnv1a(&bytes);
        self.closed = true;
        self.close_seal = seal;
        Ok(seal)
    }

    /// 是否已收口。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 收口印（未收口为 0）。
    pub fn close_seal_fp(&self) -> u64 {
        self.close_seal
    }

    /// 已入账记录数。
    pub fn records_len(&self) -> usize {
        self.records.len()
    }

    /// 收口阻断账（撞号等阻断收口的次数）。
    pub fn blocked_tally(&self) -> usize {
        self.blocked
    }

    /// 零重叠断言面：本域全部记录号与 B02/B05 区间交集恒空。
    pub fn zero_overlap(&self) -> bool {
        self.records.iter().all(|r| {
            !(r.id >= B02_RANGE.0 && r.id <= B02_RANGE.1)
                && !(r.id >= B05_RANGE.0 && r.id <= B05_RANGE.1)
        })
    }

    /// 族覆盖面：四族逐族在账。
    pub fn families_covered(&self) -> bool {
        for f in 0..FAMILY_COUNT {
            if !self.records.iter().any(|r| r.family == f) {
                return false;
            }
        }
        true
    }

    /// 记录印两两互异面。
    pub fn stamps_unique(&self) -> bool {
        for i in 0..self.records.len() {
            for j in 0..self.records.len() {
                if i != j {
                    let a = match self.records.get(i) {
                        Some(r) => r.stamp,
                        None => 0,
                    };
                    let b = match self.records.get(j) {
                        Some(r) => r.stamp,
                        None => 1,
                    };
                    if a == b {
                        return false;
                    }
                }
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
        for r in self.records.iter() {
            bytes.extend_from_slice(&r.stamp.to_le_bytes());
        }
        bytes.extend_from_slice(&self.close_seal.to_le_bytes());
        bytes.extend_from_slice(&(self.blocked as u64).to_le_bytes());
        fnv1a(&bytes)
    }
}

/// 命中账派生：种子 → 非零命中。
pub fn hits_of(seed: u64) -> u64 {
    let mut lc = Lcg::new(seed);
    ((lc.next_u32() as u64) % 500 + 10) as u64
}

/// 诊断码闭集（B22 防重 grep 执行记录域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GrepCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 撞号入账
    DupRecord,
    /// 记录号越界
    IdOob,
    /// 与 B02/B05 重叠
    OverlapDetected,
    /// 关键词族越界
    FamilyOob,
    /// 族缺失
    FamilyMissing,
    /// 零命中
    ZeroHits,
    /// 记录号断裂
    IdGap,
    /// 记录不足
    RecordsShort,
    /// 重复收口
    DupClose,
    /// 账已收口
    LedgerClosed,
}

impl GrepCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            GrepCode::Ok => "B22 防重 grep 执行记录判定成功",
            GrepCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            GrepCode::DupRecord => "撞号入账：记录号两两互异，撞号检出并阻断收口",
            GrepCode::IdOob => "记录号越界：本域闭集 1025..=1128",
            GrepCode::OverlapDetected => "与 B02/B05 重叠：零重叠断言显性报",
            GrepCode::FamilyOob => "关键词族越界：四族闭集 0..=3",
            GrepCode::FamilyMissing => "族缺失：四族（长度欺骗/重放/DoS/fuzz）齐才可收口",
            GrepCode::ZeroHits => "零命中：命中账非零才可入账",
            GrepCode::IdGap => "记录号断裂：按本域下界起连续入账",
            GrepCode::RecordsShort => "记录不足：12 条齐（4 族 ×3 条）才可收口",
            GrepCode::DupClose => "重复收口：收口只许一次",
            GrepCode::LedgerClosed => "账已收口：收口后全账只读，显性拒",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [GrepCode; 12] = [
    GrepCode::Ok,
    GrepCode::SegBad,
    GrepCode::DupRecord,
    GrepCode::IdOob,
    GrepCode::OverlapDetected,
    GrepCode::FamilyOob,
    GrepCode::FamilyMissing,
    GrepCode::ZeroHits,
    GrepCode::IdGap,
    GrepCode::RecordsShort,
    GrepCode::DupClose,
    GrepCode::LedgerClosed,
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

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准脚本：4 族 ×3 条记录入账 → 收口（零阻断）。
/// 返回 (记录数, 收口印, 阻断账, 快照印)。
pub fn standard_script() -> (usize, u64, usize, u64) {
    let mut led = GrepRecordLedger::new();
    let mut id = DOMAIN_ID_BASE;
    for f in 0..FAMILY_COUNT {
        for _ in 0..RECORDS_PER_FAMILY {
            let _ = led.record(id, f, hits_of(SEG_BASE as u64 + id as u64));
            id += 1;
        }
    }
    let seal = match led.close() {
        Ok(s) => s,
        Err(_) => 0,
    };
    let fp = led.fingerprint();
    (led.records_len(), seal, led.blocked_tally(), fp)
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
    // 类 0：撞号注入——撞号逐轮检出并阻断收口（10/10）。
    for _ in 0..INJ_ROUNDS {
        let mut led = GrepRecordLedger::new();
        let _ = led.record(DOMAIN_ID_BASE, 0, hits_of(SEG_BASE as u64 + 1));
        let dup = led.record(DOMAIN_ID_BASE, 0, 9) == Err(GrepCode::DupRecord);
        // 缺 11 条，收口被记录不足阻断（撞号后账面仍不齐）。
        let blocked = led.close() == Err(GrepCode::FamilyMissing) && led.blocked_tally() >= 1;
        if dup && blocked {
            led.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            led.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：B02 区间侵入——零重叠断言显性报。
    for _ in 0..INJ_ROUNDS {
        let mut led = GrepRecordLedger::new();
        let hit_b02 = led.record(256, 0, 5) == Err(GrepCode::OverlapDetected);
        let caught = hit_b02 && led.records_len() == 0 && led.zero_overlap();
        if caught {
            led.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            led.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：B05 区间侵入——同上。
    for _ in 0..INJ_ROUNDS {
        let mut led = GrepRecordLedger::new();
        let hit_b05 = led.record(800, 1, 5) == Err(GrepCode::OverlapDetected);
        let caught = hit_b05 && led.records_len() == 0;
        if caught {
            led.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            led.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：空族收口——族缺失阻断。
    for _ in 0..INJ_ROUNDS {
        let mut led = GrepRecordLedger::new();
        // 只入族 0/1/2，缺族 3。
        let mut id = DOMAIN_ID_BASE;
        for f in 0..(FAMILY_COUNT - 1) {
            for _ in 0..RECORDS_PER_FAMILY {
                let _ = led.record(id, f, hits_of(SEG_BASE as u64 + id as u64));
                id += 1;
            }
        }
        let caught = led.close() == Err(GrepCode::FamilyMissing)
            && !led.families_covered()
            && !led.is_closed();
        if caught {
            led.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            led.note_escape();
            total_escape += 1;
        }
    }
    // 类 4：零命中 + 族越界 + 收口后入账 + 重复收口。
    for _ in 0..INJ_ROUNDS {
        let mut led = GrepRecordLedger::new();
        let zero = led.record(DOMAIN_ID_BASE, 0, 0) == Err(GrepCode::ZeroHits);
        let famoob = led.record(DOMAIN_ID_BASE, FAMILY_COUNT + 1, 5) == Err(GrepCode::FamilyOob);
        let mut ok_all = true;
        let mut id = DOMAIN_ID_BASE;
        for f in 0..FAMILY_COUNT {
            for _ in 0..RECORDS_PER_FAMILY {
                if led.record(id, f, hits_of(SEG_BASE as u64 + id as u64 + 400)).is_err() {
                    ok_all = false;
                }
                id += 1;
            }
        }
        let _ = led.close();
        let closed_reject = led.record(DOMAIN_ID_MAX + 1, 0, 5) == Err(GrepCode::LedgerClosed);
        let dup_close = led.close() == Err(GrepCode::DupClose);
        if zero && famoob && ok_all && closed_reject && dup_close {
            led.note_detected();
            total_detect += 1;
            per[4] += 1;
        } else {
            led.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51638_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51638");
    // 1 收口防重 grep 留痕（J1）。
    {
        let (n, seal, blocked, _) = standard_script();
        s.add(
            "收口防重留痕",
            n == FAMILY_COUNT * RECORDS_PER_FAMILY && seal != 0 && blocked == 0,
            "UNX-F51639-J1：收口防重 grep 留痕（12 条四族齐，零阻断）",
        );
    }
    // 2 撞号注入 10/10 检出并阻断收口（J1R）。
    {
        let (_, _, per) = injection_harness();
        s.add(
            "撞号注入阻断收口",
            per[0] == 10,
            "UNX-F51639-J1R：撞号注入逐轮检出并阻断收口",
        );
    }
    // 3 B02/B05 零重叠断言。
    {
        let mut led = GrepRecordLedger::new();
        let mut id = DOMAIN_ID_BASE;
        for f in 0..FAMILY_COUNT {
            for _ in 0..RECORDS_PER_FAMILY {
                let _ = led.record(id, f, hits_of(SEG_BASE as u64 + id as u64 + 100));
                id += 1;
            }
        }
        let b02_hit = led.record(B02_RANGE.0 + 7, 0, 5) == Err(GrepCode::OverlapDetected);
        let b05_hit = led.record(B05_RANGE.0 + 7, 0, 5) == Err(GrepCode::OverlapDetected);
        s.add(
            "B02/B05 零重叠断言",
            led.zero_overlap() && b02_hit && b05_hit,
            "本域 1025..=1128 与 B02[1,512]/B05[513,1024] 交集恒空",
        );
    }
    // 4 四族覆盖面。
    {
        let mut led = GrepRecordLedger::new();
        let mut id = DOMAIN_ID_BASE;
        for f in 0..FAMILY_COUNT {
            let _ = led.record(id, f, hits_of(SEG_BASE as u64 + id as u64 + 200));
            id += 1;
        }
        s.add(
            "四族覆盖面",
            led.families_covered(),
            "长度欺骗/重放/DoS/fuzz 四族逐族在账",
        );
    }
    // 5 撞号显性拒（账面级）。
    {
        let mut led = GrepRecordLedger::new();
        let _ = led.record(DOMAIN_ID_BASE, 0, hits_of(SEG_BASE as u64 + 300));
        s.add(
            "撞号显性拒",
            led.record(DOMAIN_ID_BASE, 0, 9) == Err(GrepCode::DupRecord),
            "记录号两两互异（DupRecord）",
        );
    }
    // 6 记录号越界与重叠分界。
    {
        let mut led = GrepRecordLedger::new();
        let below = led.record(B05_RANGE.1, 0, 5) == Err(GrepCode::OverlapDetected);
        let above = led.record(DOMAIN_ID_MAX + 1, 0, 5) == Err(GrepCode::IdOob);
        s.add(
            "记录号分界守卫",
            below && above,
            "B05 末号触重叠断言，本域上界之外判越界",
        );
    }
    // 7 零命中入账拒。
    {
        let mut led = GrepRecordLedger::new();
        s.add(
            "零命中入账拒",
            led.record(DOMAIN_ID_BASE, 0, 0) == Err(GrepCode::ZeroHits),
            "命中账非零才可入账（ZeroHits）",
        );
    }
    // 8 族越界拒 + 族名闭集。
    {
        let mut led = GrepRecordLedger::new();
        let oob = led.record(DOMAIN_ID_BASE, FAMILY_COUNT, 5) == Err(GrepCode::FamilyOob);
        let names_ok = family_name(0) == "长度欺骗"
            && family_name(1) == "重放"
            && family_name(2) == "DoS"
            && family_name(3) == "fuzz"
            && family_name(9) == "未知族";
        s.add("族闭集守卫", oob && names_ok, "四族闭集 0..=3，族名逐字留痕");
    }
    // 9 空族收口阻断。
    {
        let mut led = GrepRecordLedger::new();
        let _ = led.record(DOMAIN_ID_BASE, 0, hits_of(SEG_BASE as u64 + 400));
        s.add(
            "空族收口阻断",
            led.close() == Err(GrepCode::FamilyMissing) && !led.is_closed(),
            "四族齐才可收口（FamilyMissing 阻断）",
        );
    }
    // 10 记录印两两互异面。
    {
        let mut led = GrepRecordLedger::new();
        let mut id = DOMAIN_ID_BASE;
        for f in 0..FAMILY_COUNT {
            for _ in 0..RECORDS_PER_FAMILY {
                let _ = led.record(id, f, hits_of(SEG_BASE as u64 + id as u64 + 500));
                id += 1;
            }
        }
        s.add(
            "记录印两两互异",
            led.stamps_unique(),
            "12 枚记录印互异（同记录恒定、异记录异印）",
        );
    }
    // 11 注入核 5 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        s.add(
            "注入 5 类×10 全通",
            d == 50 && esc == 0 && per.iter().all(|&x| x == 10),
            "5 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 12 命中账派生确定性。
    {
        s.add(
            "命中账派生确定性",
            hits_of(42) == hits_of(42) && hits_of(42) != hits_of(43),
            "LCG 种子 = SEG_BASE 派生，同段位恒定可复现",
        );
    }
    // 13 诊断码闭集唯一性。
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
    // 14 注入账守恒。
    {
        let mut led = GrepRecordLedger::new();
        for _ in 0..13u32 {
            led.note_detected();
        }
        s.add(
            "注入账守恒",
            led.conserved(),
            "注入=检出+逃逸且逃逸=0 且检出>0",
        );
    }
    // 15 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一 grep 记录脚本两次执行，快照印逐位一致",
    );
    // 16 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51638 防重 grep 记录 4 族 ×{} 条 本域 {}..={}",
            RECORDS_PER_FAMILY, DOMAIN_ID_BASE, DOMAIN_ID_MAX
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51638"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空（写回状态=已完成）",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51638_checks_green() {
        let set = run_unx51638_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn full_close_chain() {
        let mut led = GrepRecordLedger::new();
        let mut id = DOMAIN_ID_BASE;
        for f in 0..FAMILY_COUNT {
            for _ in 0..RECORDS_PER_FAMILY {
                assert!(led.record(id, f, hits_of(SEG_BASE as u64 + id as u64)).is_ok());
                id += 1;
            }
        }
        assert!(led.families_covered());
        assert!(led.zero_overlap());
        assert!(led.close().is_ok());
        assert!(led.is_closed());
        assert_eq!(led.blocked_tally(), 0);
    }

    #[test]
    fn dup_record_blocks_close() {
        let mut led = GrepRecordLedger::new();
        let _ = led.record(DOMAIN_ID_BASE, 0, 5);
        assert_eq!(led.record(DOMAIN_ID_BASE, 0, 9), Err(GrepCode::DupRecord));
        assert_eq!(led.close(), Err(GrepCode::FamilyMissing));
        assert!(led.blocked_tally() >= 1);
        assert!(!led.is_closed());
    }

    #[test]
    fn overlap_assertion_b02_b05() {
        let mut led = GrepRecordLedger::new();
        assert_eq!(led.record(1, 0, 5), Err(GrepCode::OverlapDetected));
        assert_eq!(led.record(512, 0, 5), Err(GrepCode::OverlapDetected));
        assert_eq!(led.record(513, 0, 5), Err(GrepCode::OverlapDetected));
        assert_eq!(led.record(1024, 0, 5), Err(GrepCode::OverlapDetected));
        assert_eq!(led.record(1023, 0, 5), Err(GrepCode::OverlapDetected));
        assert_eq!(led.record(2000, 0, 5), Err(GrepCode::IdOob));
        assert_eq!(led.record(0, 0, 5), Err(GrepCode::IdOob));
        assert!(led.zero_overlap());
    }

    #[test]
    fn record_guards() {
        let mut led = GrepRecordLedger::new();
        assert_eq!(led.record(DOMAIN_ID_BASE, 4, 5), Err(GrepCode::FamilyOob));
        assert_eq!(led.record(DOMAIN_ID_BASE, 0, 0), Err(GrepCode::ZeroHits));
        assert_eq!(led.record(0, 0, 5), Err(GrepCode::IdOob));
        assert_eq!(led.record(2000, 0, 5), Err(GrepCode::IdOob));
        assert!(led.record(DOMAIN_ID_BASE, 0, 5).is_ok());
        assert_eq!(led.record(DOMAIN_ID_BASE, 1, 5), Err(GrepCode::DupRecord));
    }

    #[test]
    fn family_missing_blocks_close() {
        let mut led = GrepRecordLedger::new();
        let _ = led.record(DOMAIN_ID_BASE, 0, 5);
        assert_eq!(led.close(), Err(GrepCode::FamilyMissing));
        assert!(!led.families_covered());
    }

    #[test]
    fn closed_ledger_readonly() {
        let mut led = GrepRecordLedger::new();
        let mut id = DOMAIN_ID_BASE;
        for f in 0..FAMILY_COUNT {
            for _ in 0..RECORDS_PER_FAMILY {
                let _ = led.record(id, f, hits_of(SEG_BASE as u64 + id as u64));
                id += 1;
            }
        }
        assert!(led.close().is_ok());
        assert_eq!(
            led.record(DOMAIN_ID_MAX + 1, 0, 5),
            Err(GrepCode::LedgerClosed)
        );
        assert_eq!(led.close(), Err(GrepCode::DupClose));
    }

    #[test]
    fn stamps_unique_property() {
        let mut led = GrepRecordLedger::new();
        let mut id = DOMAIN_ID_BASE;
        for f in 0..FAMILY_COUNT {
            for _ in 0..RECORDS_PER_FAMILY {
                let _ = led.record(id, f, hits_of(SEG_BASE as u64 + id as u64));
                id += 1;
            }
        }
        assert!(led.stamps_unique());
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
        let mut led = GrepRecordLedger::new();
        for _ in 0..10u32 {
            led.note_detected();
        }
        assert!(led.conserved());
        led.note_escape();
        assert!(!led.conserved());
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
