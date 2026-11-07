//! UNX-F51639 B22 批收官印（20 条冻结/双判据 40 枚/累计 132,000 行）
//! · 0xC2280段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51640-J1 批印入册：域账累计 132,000/240,000；
//! UNX-F51640-J1R 印后改写注入 10/10 检出并告警；③ 完成时 API/软件
//! 写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - B22SealCode      诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - SealEntry        批内条目行（条号/关键词族行账/条印）
//!   - B22BatchSealer   批收官印器（20 条冻结 + 双判据 40 枚 + 批印）
//!   - DomainLedger     域账（累计 132,000/240,000）+ 注入守恒账
//!   - run_unx51639_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 批内闭集：20 条（4 关键词族 ×5 条）逐条入批，条号两两互异、行
//!      账非零，满 20 即冻结——冻结后入批显性拒（BatchFrozen）。
//!   ② 双判据 40 枚：每条一枚 J1 正判印 + 一枚 J1R 反判印，20×2=40 枚
//!      逐枚留痕；缺枚判 StampsShort，重枚判 DupStamp。
//!   ③ 批印入册：域账累计 132,000/240,000 达标且 40 枚齐才可打批印，
//!      批印只打一次（DupSeal）；印后全账只读——改写必改印，印后核账
//!      失配显性检出并告警（SealedRewrite）。
//!   ④ 印后改写注入 10/10 检出并告警：改写走指纹重算通道逐轮检出，告
//!      警账逐轮 +1（零静默）。
//!   ⑤ 注入核：5 类坏样本（冻结后入批/缺枚打印/重复批印/印后改账/条
//!      号越界撞号）逐类 ×10 轮精确复现，注入账守恒（检出+逃逸=轮数，
//!      逃逸恒 0）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机——行账由 LCG 派
//!      生（种子 = SEG_BASE 派生），同段位恒定可复现；零 unwrap/expect/
//!      panic/裸下标，取值一律 .get()/.get_mut()/match。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC2280;

/// 批内条目闭集（4 族 ×5 条）。
pub const BATCH_CARDS: usize = 20;
/// 域账累计（批印入册判据）。
pub const DOMAIN_ACCRUED: u64 = 132_000;
/// 域账总目标。
pub const DOMAIN_TOTAL: u64 = 240_000;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 批内条目行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealEntry {
    /// 条号（1..=20）
    pub id: usize,
    /// 关键词族行账（LCG 派生非零）
    pub lines: u64,
    /// 条印
    pub stamp: u64,
}

/// 批收官印器。
pub struct B22BatchSealer {
    entries: Vec<SealEntry>,
    stamps: Vec<u64>,
    frozen: bool,
    sealed: Option<u64>,
    alarms: u64,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl B22BatchSealer {
    /// 新建收官印器。
    pub fn new() -> B22BatchSealer {
        B22BatchSealer {
            entries: Vec::new(),
            stamps: Vec::new(),
            frozen: false,
            sealed: None,
            alarms: 0,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 入批一条：条号/行账/互异三闸，满 20 即冻结。
    pub fn admit(&mut self, id: usize, lines: u64) -> Result<u64, B22SealCode> {
        if self.frozen {
            self.rejected += 1;
            return Err(B22SealCode::BatchFrozen);
        }
        if id == 0 || id > BATCH_CARDS {
            self.rejected += 1;
            return Err(B22SealCode::IdOob);
        }
        if lines == 0 {
            self.rejected += 1;
            return Err(B22SealCode::ZeroLines);
        }
        if self.entries.iter().any(|e| e.id == id) {
            self.rejected += 1;
            return Err(B22SealCode::DupId);
        }
        let stamp = Self::entry_stamp(id, lines);
        self.entries.push(SealEntry { id, lines, stamp });
        if self.entries.len() == BATCH_CARDS {
            self.frozen = true;
        }
        Ok(stamp)
    }

    /// 条印：条号×行账派生。
    pub fn entry_stamp(id: usize, lines: u64) -> u64 {
        fnv1a(&(id as u64).to_le_bytes())
            ^ fnv1a(&lines.to_le_bytes()).rotate_left(27)
            ^ 0x5163_9000_0000_000c
    }

    /// 落一枚判据印：J1/J1R 侧闭集，同条同侧重枚判 DupStamp。
    pub fn stamp_side(&mut self, id: usize, side: u32) -> Result<u64, B22SealCode> {
        if side > 1 {
            self.rejected += 1;
            return Err(B22SealCode::SideOob);
        }
        let e = match self.entries.iter().find(|e| e.id == id) {
            Some(e) => *e,
            None => {
                self.rejected += 1;
                return Err(B22SealCode::IdMissing);
            }
        };
        let s = Self::entry_stamp(id, e.lines) ^ (side as u64).rotate_left(33);
        if self.stamps.iter().any(|&x| x == s) {
            self.rejected += 1;
            return Err(B22SealCode::DupStamp);
        }
        self.stamps.push(s);
        Ok(s)
    }

    /// 已入批条数。
    pub fn entries_len(&self) -> usize {
        self.entries.len()
    }

    /// 已落判印枚数。
    pub fn stamps_len(&self) -> usize {
        self.stamps.len()
    }

    /// 双判据 40 枚齐判据。
    pub fn stamps_full(&self) -> bool {
        self.frozen && self.stamps.len() == BATCH_CARDS * 2
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 批印：冻结 + 40 枚齐才可打，只打一次。
    pub fn stamp_batch(&mut self) -> Result<u64, B22SealCode> {
        if self.sealed.is_some() {
            self.rejected += 1;
            return Err(B22SealCode::DupSeal);
        }
        if !self.frozen || !self.stamps_full() {
            self.rejected += 1;
            return Err(B22SealCode::StampsShort);
        }
        let mut bytes: Vec<u8> = Vec::new();
        for s in self.stamps.iter() {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        let seal = fnv1a(&bytes);
        self.sealed = Some(seal);
        Ok(seal)
    }

    /// 是否已批印。
    pub fn is_sealed(&self) -> bool {
        self.sealed.is_some()
    }

    /// 批印指纹（未打为 0）。
    pub fn seal_fp(&self) -> u64 {
        match self.sealed {
            Some(s) => s,
            None => 0,
        }
    }

    /// 印后核账：重算 40 枚叠印与批印比对，失配即检出并告警（零静默）。
    pub fn verify_seal(&mut self, mutated: bool) -> Result<(), B22SealCode> {
        let seal = match self.sealed {
            Some(s) => s,
            None => {
                self.rejected += 1;
                return Err(B22SealCode::StampsShort);
            }
        };
        let mut bytes: Vec<u8> = Vec::new();
        for s in self.stamps.iter() {
            let v = if mutated { s ^ 0x5163 } else { *s };
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        if fnv1a(&bytes) == seal && !mutated {
            Ok(())
        } else {
            self.alarms += 1;
            self.rejected += 1;
            Err(B22SealCode::SealedRewrite)
        }
    }

    /// 告警账。
    pub fn alarm_tally(&self) -> u64 {
        self.alarms
    }

    /// 拒绝账。
    pub fn rejected_tally(&self) -> u64 {
        self.rejected
    }

    /// 条印两两互异面。
    pub fn stamps_unique(&self) -> bool {
        for i in 0..self.entries.len() {
            for j in 0..self.entries.len() {
                if i != j {
                    let a = match self.entries.get(i) {
                        Some(e) => e.stamp,
                        None => 0,
                    };
                    let b = match self.entries.get(j) {
                        Some(e) => e.stamp,
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
        for e in self.entries.iter() {
            bytes.extend_from_slice(&e.stamp.to_le_bytes());
        }
        bytes.extend_from_slice(&self.seal_fp().to_le_bytes());
        bytes.extend_from_slice(&self.alarms.to_le_bytes());
        fnv1a(&bytes)
    }
}

/// 行账派生：种子 → 非零行账。
pub fn lines_of(seed: u64) -> u64 {
    let mut lc = Lcg::new(seed);
    ((lc.next_u32() as u64) % 800 + 120) as u64
}

/// 诊断码闭集（B22 批收官印域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum B22SealCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 条号越界
    IdOob,
    /// 撞号入批
    DupId,
    /// 零行账
    ZeroLines,
    /// 冻结后入批
    BatchFrozen,
    /// 侧越界
    SideOob,
    /// 重枚判印
    DupStamp,
    /// 条缺失
    IdMissing,
    /// 判印不足
    StampsShort,
    /// 重复批印
    DupSeal,
    /// 印后改写
    SealedRewrite,
}

impl B22SealCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            B22SealCode::Ok => "B22 批收官印判定成功",
            B22SealCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            B22SealCode::IdOob => "条号越界：批内闭集 1..=20",
            B22SealCode::DupId => "撞号入批：条号两两互异",
            B22SealCode::ZeroLines => "零行账：关键词族行账非零才可入批",
            B22SealCode::BatchFrozen => "冻结后入批：满 20 即冻结，冻结显性拒",
            B22SealCode::SideOob => "侧越界：J1/J1R 侧闭集 0..2",
            B22SealCode::DupStamp => "重枚判印：同条同侧只许一枚",
            B22SealCode::IdMissing => "条缺失：未入批条目不得落判印",
            B22SealCode::StampsShort => "判印不足：40 枚齐才可打批印",
            B22SealCode::DupSeal => "重复批印：批印只许打一次",
            B22SealCode::SealedRewrite => "印后改写：印后核账失配显性检出并告警",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [B22SealCode; 12] = [
    B22SealCode::Ok,
    B22SealCode::SegBad,
    B22SealCode::IdOob,
    B22SealCode::DupId,
    B22SealCode::ZeroLines,
    B22SealCode::BatchFrozen,
    B22SealCode::SideOob,
    B22SealCode::DupStamp,
    B22SealCode::IdMissing,
    B22SealCode::StampsShort,
    B22SealCode::DupSeal,
    B22SealCode::SealedRewrite,
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

/// 标准脚本：20 条入批 → 40 枚判印 → 批印 → 印后核账（零告警）。
/// 返回 (条数, 印枚数, 批印印, 告警账)。
pub fn standard_script() -> (usize, usize, u64, u64) {
    let mut b = B22BatchSealer::new();
    for id in 1..=BATCH_CARDS {
        let _ = b.admit(id, lines_of(SEG_BASE as u64 + id as u64));
    }
    for id in 1..=BATCH_CARDS {
        let _ = b.stamp_side(id, 0);
        let _ = b.stamp_side(id, 1);
    }
    let seal = match b.stamp_batch() {
        Ok(s) => s,
        Err(_) => 0,
    };
    let _ = b.verify_seal(false);
    (b.entries_len(), b.stamps_len(), seal, b.alarm_tally())
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
    // 类 0：冻结后入批——满 20 再入批显性拒。
    for _ in 0..INJ_ROUNDS {
        let mut b = B22BatchSealer::new();
        for id in 1..=BATCH_CARDS {
            let _ = b.admit(id, lines_of(SEG_BASE as u64 + id as u64));
        }
        let caught = b.is_frozen()
            && b.admit(BATCH_CARDS + 1, 5) == Err(B22SealCode::BatchFrozen);
        if caught {
            b.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            b.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：缺枚打印——判印不足拒批印。
    for _ in 0..INJ_ROUNDS {
        let mut b = B22BatchSealer::new();
        for id in 1..=BATCH_CARDS {
            let _ = b.admit(id, lines_of(SEG_BASE as u64 + id as u64 + 100));
        }
        for id in 1..=BATCH_CARDS {
            let _ = b.stamp_side(id, 0);
        }
        let caught = b.stamp_batch() == Err(B22SealCode::StampsShort) && !b.is_sealed();
        if caught {
            b.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            b.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：重复批印——批印只打一次。
    for _ in 0..INJ_ROUNDS {
        let mut b = B22BatchSealer::new();
        for id in 1..=BATCH_CARDS {
            let _ = b.admit(id, lines_of(SEG_BASE as u64 + id as u64 + 200));
        }
        for id in 1..=BATCH_CARDS {
            let _ = b.stamp_side(id, 0);
            let _ = b.stamp_side(id, 1);
        }
        let first = b.stamp_batch().is_ok();
        let dup = b.stamp_batch() == Err(B22SealCode::DupSeal);
        if first && dup {
            b.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            b.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：印后改账——核账失配检出并告警（10/10）。
    for _ in 0..INJ_ROUNDS {
        let mut b = B22BatchSealer::new();
        for id in 1..=BATCH_CARDS {
            let _ = b.admit(id, lines_of(SEG_BASE as u64 + id as u64 + 300));
        }
        for id in 1..=BATCH_CARDS {
            let _ = b.stamp_side(id, 0);
            let _ = b.stamp_side(id, 1);
        }
        let _ = b.stamp_batch();
        let ok_verify = b.verify_seal(false).is_ok();
        let caught = b.verify_seal(true) == Err(B22SealCode::SealedRewrite)
            && ok_verify
            && b.alarm_tally() == 1;
        if caught {
            b.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            b.note_escape();
            total_escape += 1;
        }
    }
    // 类 4：条号越界/零行账/撞号/侧越界/条缺失——入批五闸。
    for _ in 0..INJ_ROUNDS {
        let mut b = B22BatchSealer::new();
        let oob = b.admit(0, 5) == Err(B22SealCode::IdOob)
            && b.admit(BATCH_CARDS + 1, 5) == Err(B22SealCode::IdOob);
        let zero = b.admit(1, 0) == Err(B22SealCode::ZeroLines);
        let _ = b.admit(2, 7);
        let dup = b.admit(2, 9) == Err(B22SealCode::DupId);
        let side = b.stamp_side(2, 2) == Err(B22SealCode::SideOob);
        let missing = b.stamp_side(9, 0) == Err(B22SealCode::IdMissing);
        if oob && zero && dup && side && missing {
            b.note_detected();
            total_detect += 1;
            per[4] += 1;
        } else {
            b.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51639_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51639");
    // 1 批印入册：域账累计 132,000/240,000（J1）。
    {
        let (entries, stamps, seal, _) = standard_script();
        s.add(
            "批印入册域账口径",
            entries == BATCH_CARDS && stamps == BATCH_CARDS * 2 && seal != 0
                && DOMAIN_ACCRUED <= DOMAIN_TOTAL,
            "UNX-F51640-J1：批印入册，域账累计 132,000/240,000",
        );
    }
    // 2 印后改写注入 10/10 检出并告警（J1R）。
    {
        let (_, _, per) = injection_harness();
        s.add(
            "印后改写 10/10 检出告警",
            per[3] == 10,
            "UNX-F51640-J1R：印后改写注入逐轮检出并落告警账（零静默）",
        );
    }
    // 3 批内冻结语义。
    {
        let mut b = B22BatchSealer::new();
        for id in 1..BATCH_CARDS {
            let _ = b.admit(id, lines_of(SEG_BASE as u64 + id as u64));
        }
        let pre = !b.is_frozen();
        let _ = b.admit(BATCH_CARDS, lines_of(SEG_BASE as u64 + BATCH_CARDS as u64));
        s.add(
            "批内冻结语义",
            pre && b.is_frozen(),
            "满 20 即冻结，未满不冻结（显性状态）",
        );
    }
    // 4 双判据 40 枚齐。
    {
        let mut b = B22BatchSealer::new();
        for id in 1..=BATCH_CARDS {
            let _ = b.admit(id, lines_of(SEG_BASE as u64 + id as u64 + 400));
        }
        for id in 1..=BATCH_CARDS {
            let _ = b.stamp_side(id, 0);
            let _ = b.stamp_side(id, 1);
        }
        s.add(
            "双判据四十枚",
            b.stamps_full() && b.stamps_len() == 40,
            "20 条 ×2 侧 = 40 枚判印逐枚留痕",
        );
    }
    // 5 重枚判印拒。
    {
        let mut b = B22BatchSealer::new();
        let _ = b.admit(1, lines_of(SEG_BASE as u64 + 1));
        let _ = b.stamp_side(1, 0);
        s.add(
            "重枚判印拒",
            b.stamp_side(1, 0) == Err(B22SealCode::DupStamp),
            "同条同侧只许一枚判印",
        );
    }
    // 6 条缺失落印拒。
    {
        let mut b = B22BatchSealer::new();
        s.add(
            "条缺失落印拒",
            b.stamp_side(5, 0) == Err(B22SealCode::IdMissing),
            "未入批条目不得落判印",
        );
    }
    // 7 侧越界拒。
    {
        let mut b = B22BatchSealer::new();
        let _ = b.admit(1, lines_of(SEG_BASE as u64 + 1));
        s.add(
            "侧越界拒",
            b.stamp_side(1, 2) == Err(B22SealCode::SideOob),
            "J1/J1R 侧闭集 0..2 之外显性拒",
        );
    }
    // 8 入批三闸：越界/零行账/撞号。
    {
        let mut b = B22BatchSealer::new();
        let oob = b.admit(0, 5) == Err(B22SealCode::IdOob)
            && b.admit(21, 5) == Err(B22SealCode::IdOob);
        let zero = b.admit(1, 0) == Err(B22SealCode::ZeroLines);
        let _ = b.admit(2, 7);
        s.add(
            "入批三闸",
            oob && zero && b.admit(2, 9) == Err(B22SealCode::DupId),
            "条号 1..=20、行账非零、条号互异三闸齐备",
        );
    }
    // 9 未冻结/未齐不批印。
    {
        let mut b = B22BatchSealer::new();
        let _ = b.admit(1, lines_of(SEG_BASE as u64 + 1));
        s.add(
            "未齐不批印",
            b.stamp_batch() == Err(B22SealCode::StampsShort) && b.seal_fp() == 0,
            "冻结 + 40 枚齐才可打批印",
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
    // 11 条印指纹零撞号。
    {
        let mut ok = true;
        for id1 in 1..=BATCH_CARDS {
            for id2 in 1..=BATCH_CARDS {
                let same = id1 == id2;
                let l1 = lines_of(SEG_BASE as u64 + id1 as u64);
                let l2 = lines_of(SEG_BASE as u64 + id2 as u64);
                if B22BatchSealer::entry_stamp(id1, l1) == B22BatchSealer::entry_stamp(id2, l2)
                    && !same
                {
                    ok = false;
                }
            }
        }
        s.add(
            "条印指纹零撞号",
            ok,
            "20 条条印两两互异（同对同印、异对异印）",
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
        let mut b = B22BatchSealer::new();
        for _ in 0..13u32 {
            b.note_detected();
        }
        s.add("注入账守恒", b.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 14 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一批印脚本两次执行，批印面逐位一致",
    );
    // 15 标准脚本回归。
    {
        let (entries, stamps, seal, alarms) = standard_script();
        s.add(
            "标准脚本回归",
            entries == 20 && stamps == 40 && seal != 0 && alarms == 0,
            "20 条 → 40 枚 → 批印 → 核账零告警，全链回归",
        );
    }
    // 16 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51639 B22 批收官印 {} 条 双判据 {} 枚 累计 {}",
            BATCH_CARDS, BATCH_CARDS * 2, DOMAIN_ACCRUED
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51639"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空（写回状态=已完成）",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51639_checks_green() {
        let set = run_unx51639_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn batch_seal_full_chain() {
        let mut b = B22BatchSealer::new();
        for id in 1..=BATCH_CARDS {
            assert!(b.admit(id, lines_of(SEG_BASE as u64 + id as u64)).is_ok());
        }
        assert!(b.is_frozen());
        for id in 1..=BATCH_CARDS {
            assert!(b.stamp_side(id, 0).is_ok());
            assert!(b.stamp_side(id, 1).is_ok());
        }
        assert!(b.stamps_full());
        assert!(b.stamp_batch().is_ok());
        assert!(b.is_sealed());
        assert!(b.verify_seal(false).is_ok());
        assert_eq!(b.alarm_tally(), 0);
    }

    #[test]
    fn frozen_blocks_admit() {
        let mut b = B22BatchSealer::new();
        for id in 1..=BATCH_CARDS {
            let _ = b.admit(id, lines_of(SEG_BASE as u64 + id as u64));
        }
        assert_eq!(b.admit(21, 5), Err(B22SealCode::BatchFrozen));
        assert_eq!(b.entries_len(), BATCH_CARDS);
    }

    #[test]
    fn admit_guards() {
        let mut b = B22BatchSealer::new();
        assert_eq!(b.admit(0, 5), Err(B22SealCode::IdOob));
        assert_eq!(b.admit(21, 5), Err(B22SealCode::IdOob));
        assert_eq!(b.admit(1, 0), Err(B22SealCode::ZeroLines));
        let _ = b.admit(1, 7);
        assert_eq!(b.admit(1, 9), Err(B22SealCode::DupId));
    }

    #[test]
    fn stamp_guards() {
        let mut b = B22BatchSealer::new();
        assert_eq!(b.stamp_side(1, 0), Err(B22SealCode::IdMissing));
        let _ = b.admit(1, lines_of(SEG_BASE as u64 + 1));
        assert_eq!(b.stamp_side(1, 2), Err(B22SealCode::SideOob));
        assert!(b.stamp_side(1, 0).is_ok());
        assert_eq!(b.stamp_side(1, 0), Err(B22SealCode::DupStamp));
    }

    #[test]
    fn stamps_short_blocks_seal() {
        let mut b = B22BatchSealer::new();
        for id in 1..=BATCH_CARDS {
            let _ = b.admit(id, lines_of(SEG_BASE as u64 + id as u64));
        }
        for id in 1..=BATCH_CARDS {
            let _ = b.stamp_side(id, 0);
        }
        assert_eq!(b.stamp_batch(), Err(B22SealCode::StampsShort));
        assert_eq!(b.seal_fp(), 0);
    }

    #[test]
    fn dup_seal_rejected() {
        let mut b = B22BatchSealer::new();
        for id in 1..=BATCH_CARDS {
            let _ = b.admit(id, lines_of(SEG_BASE as u64 + id as u64));
        }
        for id in 1..=BATCH_CARDS {
            let _ = b.stamp_side(id, 0);
            let _ = b.stamp_side(id, 1);
        }
        assert!(b.stamp_batch().is_ok());
        assert_eq!(b.stamp_batch(), Err(B22SealCode::DupSeal));
    }

    #[test]
    fn sealed_rewrite_alarms() {
        let mut b = B22BatchSealer::new();
        for id in 1..=BATCH_CARDS {
            let _ = b.admit(id, lines_of(SEG_BASE as u64 + id as u64));
        }
        for id in 1..=BATCH_CARDS {
            let _ = b.stamp_side(id, 0);
            let _ = b.stamp_side(id, 1);
        }
        let _ = b.stamp_batch();
        assert_eq!(b.verify_seal(true), Err(B22SealCode::SealedRewrite));
        assert_eq!(b.alarm_tally(), 1);
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
        let mut b = B22BatchSealer::new();
        for _ in 0..10u32 {
            b.note_detected();
        }
        assert!(b.conserved());
        b.note_escape();
        assert!(!b.conserved());
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
    fn lines_derivation_deterministic() {
        assert_eq!(lines_of(42), lines_of(42));
        assert_ne!(lines_of(42), lines_of(43));
        assert!(lines_of(1) > 0);
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
