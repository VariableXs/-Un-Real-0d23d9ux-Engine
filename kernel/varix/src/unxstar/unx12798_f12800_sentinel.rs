//! UNX-F12798 F12800 前哨：八百条互引零悬空预验 · 0x83180段
//!
//! 判据原文（派工卡 12798 逐字锚定）：
//!   UNX-F12799-J1 三断言绿：40 批齐备、800 判据唯一、域账 233,674+6,326=240,000 满账对平。
//!
//! 设计要点：
//! 1. 互引预验域模型——号面登记表：F12001–F12800 共 800 条号面闭集登记，
//!    每条冻结三元组（号 / 批号 1..=40 / 判据指纹派生公式），登记后只读；
//! 2. 派生公式冻结：号 = 12001 + i，批号 = i/20 + 1（每批 20 条、共 40 批），
//!    指纹 F(i) = ((号×黄金率 + 批号×混响率) 循环左移 11 位) 异或 号字节反转，
//!    三者全部可由公式复算，登记表不灌水、不存冗余副本；
//! 3. 三断言判定器族：assert_batches()（40 批齐备，逐批 20 条点验）→
//!    assert_unique()（800 判据号两两唯一，按批内连续 + 批间推进公式复算 +
//!    鸽笼位图去重）→ assert_ledger()（域账 233,674+6,326=240,000 对平，
//!    checked_add 溢出守卫 + 十进制逐位进位复算双通道），次序冻结不可跳跃；
//! 4. 悬空注入判定器 probe_dangling(id)：号面窗外号一律 ESRCH 语义
//!    （查无此号）显性拒，10 轮注入必须全检出；
//! 5. 注入器三类各 10 轮（LCG 种子写死、序列确定）：悬空号 / 批号越界 /
//!    账行篡改，每一轮违约必须被对应判定器抓住，逃脱即判据失效；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!    无 unwrap/expect/panic/裸下标；状态机 开账→满账→冻结 单向推进，倒退显性拒。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12798 − 12600) = 0x83180）。
pub const SEG_BASE: u32 = 0x83180;

// ── 号面互引预验诊断码闭集 ───────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0E 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SentDiag {
    /// 号面窗外号（ESRCH 语义：查无此号，12001..=12800 之外一律拒）。
    ENoRange,
    /// 批号越界或与推进公式不符（闭集 1..=40 之外、或与 i/20+1 不符）。
    EBatchRange,
    /// 号序乱序（未按 12001 起步逐号推进登记）。
    ENoOrder,
    /// 判据号重复（两两唯一性被破坏，鸽笼位图检出双占）。
    EDupNo,
    /// 容量耗尽（800 条满账后再写被拒）。
    ECapFull,
    /// 登记表已冻结，写入被拒（冻结后只读）。
    EFrozen,
    /// 未冻结先判定（三断言必须等冻结口过完才能跑，判定次序守卫）。
    ENotFrozen,
    /// 状态机倒退（重复冻结或封死后回退）。
    ERewind,
    /// 批未齐（某批不满 20 条或总行数不足 800）。
    EBatchNotFull,
    /// 唯一性破坏（号与批内连续/批间推进公式复算不符）。
    EUniqueBreak,
    /// 指纹复算不符（冻结指纹与派生公式 F(i) 重算值不一致）。
    EFpMismatch,
    /// 域账对平失败（三项之和不等于 240,000 满账）。
    ELedgerBreak,
    /// 加法溢出守卫（checked_add 溢出或逐位进位复算越界）。
    EOverflow,
    /// 注入逃脱（注入轮未被判定器抓住，判定面失效）。
    EInjectEscape,
}

impl SentDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0E 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                SentDiag::ENoRange => 0x01,
                SentDiag::EBatchRange => 0x02,
                SentDiag::ENoOrder => 0x03,
                SentDiag::EDupNo => 0x04,
                SentDiag::ECapFull => 0x05,
                SentDiag::EFrozen => 0x06,
                SentDiag::ENotFrozen => 0x07,
                SentDiag::ERewind => 0x08,
                SentDiag::EBatchNotFull => 0x09,
                SentDiag::EUniqueBreak => 0x0a,
                SentDiag::EFpMismatch => 0x0b,
                SentDiag::ELedgerBreak => 0x0c,
                SentDiag::EOverflow => 0x0d,
                SentDiag::EInjectEscape => 0x0e,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            SentDiag::ENoRange => "号面窗外号（ESRCH 语义）：只认 F12001..=F12800 闭集，窗外号先查派工卡对号再登记",
            SentDiag::EBatchRange => "批号越界或与公式不符：闭集 1..=40，且必须等于 i/20+1，先复算推进公式再入表",
            SentDiag::ENoOrder => "号序乱序：从 12001 起逐号推进登记，跳号/回退先补齐前号",
            SentDiag::EDupNo => "判据号重复：800 号两两唯一，双占位先查鸽笼位图定位重号",
            SentDiag::ECapFull => "容量耗尽：800 条满账后不再收新号，新增走下一张派工卡",
            SentDiag::EFrozen => "登记表已冻结：冻结后只读，改动走前哨复核通道另开新表",
            SentDiag::ENotFrozen => "未冻结先判定：三断言必须过冻结口后再跑，先补满 800 条并冻结",
            SentDiag::ERewind => "状态机倒退：开账→满账→冻结只进不退，重复冻结即表可疑",
            SentDiag::EBatchNotFull => "批未齐：每批恰 20 条共 40 批，先补齐缺号再走断言",
            SentDiag::EUniqueBreak => "唯一性破坏：号与批内连续/批间推进公式复算不符，按批号回修对应行",
            SentDiag::EFpMismatch => "指纹复算不符：冻结指纹与 F(i) 重算值不一致，该行涉嫌账行篡改，整批复核",
            SentDiag::ELedgerBreak => "域账对平失败：233,674+6,326 必须=240,000，逐位复算定位错位行",
            SentDiag::EOverflow => "加法溢出守卫：checked_add 溢出或进位越界，先查账行是否被灌入超界值",
            SentDiag::EInjectEscape => "注入逃脱：注入轮未被判定器抓住，判定面失效须全表复核",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const SENT_ALL: &[SentDiag] = &[
    SentDiag::ENoRange,
    SentDiag::EBatchRange,
    SentDiag::ENoOrder,
    SentDiag::EDupNo,
    SentDiag::ECapFull,
    SentDiag::EFrozen,
    SentDiag::ENotFrozen,
    SentDiag::ERewind,
    SentDiag::EBatchNotFull,
    SentDiag::EUniqueBreak,
    SentDiag::EFpMismatch,
    SentDiag::ELedgerBreak,
    SentDiag::EOverflow,
    SentDiag::EInjectEscape,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in SENT_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x83181..0x8318E 闭集内"
}

// ── 号面闭集派生公式（冻结，不许改） ─────────────────────────────
/// 号面闭集首号：F12001（派工卡判据闭集起点）。
pub const FACE_FIRST: u32 = 12001;
/// 号面闭集末号：F12800（派工卡判据闭集终点）。
pub const FACE_LAST: u32 = 12800;
/// 号面总条数：800 条。
pub const FACE_COUNT: usize = 800;
/// 批总数：40 批。
pub const BATCH_COUNT: u32 = 40;
/// 每批条数：20 条。
pub const FACES_PER_BATCH: u32 = 20;

/// 域账三行（判据原文冻结值）：主账 233,674。
pub const LEDGER_MAIN: u64 = 233_674;
/// 域账三行：辅账 6,326。
pub const LEDGER_AUX: u64 = 6_326;
/// 域账三行：满账总额 240,000。
pub const LEDGER_TOTAL: u64 = 240_000;

/// 批号派生公式：批号 = (号 − 12001) / 20 + 1（号须在闭集窗内，窗外回落 0）。
pub fn derive_batch(no: u32) -> u32 {
    if no < FACE_FIRST || no > FACE_LAST {
        return 0;
    }
    (no - FACE_FIRST) / FACES_PER_BATCH + 1
}

/// 指纹派生公式 F(i)：以（号，批号）为原料的纯整数确定函数，可复算。
/// 常数取黄金分割率 0x9E3779B1 与 murmur 混响率 0x85EBCA6B（整数混排惯用闭集）。
pub fn derive_fingerprint(no: u32, batch: u32) -> u32 {
    let h = no
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(batch.wrapping_mul(0x85EB_CA6B));
    h.rotate_left(11) ^ no.swap_bytes()
}

// ── 号面登记行 ───────────────────────────────────────────────────
/// 号面登记行：冻结三元组（号 / 批号 / 指纹），登记后只读。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceRow {
    /// 判据号（12001..=12800 闭集）。
    pub no: u32,
    /// 批号（1..=40 闭集，= i/20+1）。
    pub batch: u32,
    /// 判据指纹（F(i) 派生公式冻结值）。
    pub fingerprint: u32,
}

impl FaceRow {
    /// 行指纹复算校验：冻结指纹必须等于派生公式重算值。
    pub fn fp_recompute_ok(&self) -> bool {
        self.fingerprint == derive_fingerprint(self.no, self.batch)
    }
}

// ── 登记表状态机：开账 → 满账 → 冻结，只进不退 ───────────────────
/// 登记表状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryState {
    /// 开账：逐号登记中。
    Open,
    /// 满账：800 条齐，等待冻结口。
    Full,
    /// 冻结：只读，三断言判定器方可运行。
    Frozen,
}

impl RegistryState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            RegistryState::Open => 0,
            RegistryState::Full => 1,
            RegistryState::Frozen => 2,
        }
    }
}

/// 号面登记表（互引预验域模型核心结构）。
#[derive(Clone, Debug)]
pub struct FaceRegistry {
    /// 已登记行（按号序推进排列）。
    rows: Vec<FaceRow>,
    /// 状态机。
    state: RegistryState,
}

impl FaceRegistry {
    /// 开一张空表。
    pub fn new() -> FaceRegistry {
        FaceRegistry {
            rows: Vec::new(),
            state: RegistryState::Open,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> RegistryState {
        self.state
    }

    /// 已登记行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<FaceRow> {
        self.rows.get(idx).copied()
    }

    /// 全表指纹：FNV 式逐行折叠（双跑一致性比对口）。
    pub fn fp(&self) -> u64 {
        let mut acc: u64 = SEG_BASE as u64;
        for r in self.rows.iter() {
            acc = acc
                .wrapping_mul(0x0100_0000_01B3)
                .wrapping_add(r.fingerprint as u64);
        }
        acc
    }

    /// 按公式登记一号（批号与指纹全部由派生公式冻结生成）。
    pub fn register(&mut self, no: u32) -> Result<(), u32> {
        let batch = derive_batch(no);
        self.register_with_batch(no, batch)
    }

    /// 带批号登记（注入面入口）：批号闭集闸 → 批号一致闸 → 容量闸 → 号序闸。
    /// 闸序冻结：批号两道闸先于号序闸，越界/错位批号无论落点一律 EBatchRange。
    pub fn register_with_batch(&mut self, no: u32, batch: u32) -> Result<(), u32> {
        // 冻结闸：冻结后一切写入拒绝。
        if self.state.rank() >= RegistryState::Frozen.rank() {
            return Err(SentDiag::EFrozen.val());
        }
        // 号面闭集闸：窗外号 ESRCH 语义显性拒。
        if no < FACE_FIRST || no > FACE_LAST {
            return Err(SentDiag::ENoRange.val());
        }
        // 批号闭集闸：1..=40 之外一律拒（先于号序闸，注入面保证必抓）。
        if batch < 1 || batch > BATCH_COUNT {
            return Err(SentDiag::EBatchRange.val());
        }
        // 批号一致闸：批号必须等于推进公式 i/20+1（先于号序闸，错位批号必抓）。
        if batch != derive_batch(no) {
            return Err(SentDiag::EBatchRange.val());
        }
        // 容量闸：800 条满账后再写拒绝。
        if self.rows.len() >= FACE_COUNT {
            return Err(SentDiag::ECapFull.val());
        }
        // 号序闸：从 12001 起逐号推进，跳号/回退拒。
        let expect_no = FACE_FIRST + self.rows.len() as u32;
        if no != expect_no {
            return Err(SentDiag::ENoOrder.val());
        }
        // 重复闸（推进序下理论不可达，防御性保留）。
        if let Some(last) = self.rows.last() {
            if last.no == no {
                return Err(SentDiag::EDupNo.val());
            }
        }
        self.rows.push(FaceRow {
            no,
            batch,
            fingerprint: derive_fingerprint(no, batch),
        });
        // 满账自动推进状态机（只进不退）。
        if self.rows.len() == FACE_COUNT {
            self.state = RegistryState::Full;
        }
        Ok(())
    }

    /// 冻结口：800 条满账才许冻结；冻结后重复冻结倒退拒。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.state.rank() >= RegistryState::Frozen.rank() {
            return Err(SentDiag::ERewind.val());
        }
        if self.rows.len() != FACE_COUNT {
            return Err(SentDiag::EBatchNotFull.val());
        }
        self.state = RegistryState::Frozen;
        Ok(())
    }

    /// 断言一：40 批齐备——逐批点验每批恰 20 条且号批相符、批内连续。
    /// 判定次序守卫：未冻结先判一律 ENotFrozen。
    pub fn assert_batches(&self) -> Result<u32, u32> {
        if self.state.rank() < RegistryState::Frozen.rank() {
            return Err(SentDiag::ENotFrozen.val());
        }
        if self.rows.len() != FACE_COUNT {
            return Err(SentDiag::EBatchNotFull.val());
        }
        let mut b = 1u32;
        while b <= BATCH_COUNT {
            // 逐批点验：第 b 批全表计数必须恰为 20。
            let mut cnt = 0u32;
            for r in self.rows.iter() {
                if r.batch == b {
                    cnt += 1;
                }
            }
            if cnt != FACES_PER_BATCH {
                return Err(SentDiag::EBatchNotFull.val());
            }
            // 批内连续点验：第 b 批 20 条的号必须恰为 base..base+19，批号必为 b。
            let base = FACE_FIRST + (b - 1) * FACES_PER_BATCH;
            let mut k = 0u32;
            while k < FACES_PER_BATCH {
                let idx = ((b - 1) * FACES_PER_BATCH + k) as usize;
                match self.rows.get(idx) {
                    Some(r) => {
                        if r.no != base + k || r.batch != b {
                            return Err(SentDiag::EBatchNotFull.val());
                        }
                    }
                    None => return Err(SentDiag::EBatchNotFull.val()),
                }
                k += 1;
            }
            b += 1;
        }
        Ok(BATCH_COUNT)
    }

    /// 断言二：800 判据唯一——批内连续 + 批间推进公式复算 + 鸽笼位图去重。
    pub fn assert_unique(&self) -> Result<u32, u32> {
        if self.state.rank() < RegistryState::Frozen.rank() {
            return Err(SentDiag::ENotFrozen.val());
        }
        if self.rows.len() != FACE_COUNT {
            return Err(SentDiag::EUniqueBreak.val());
        }
        // 批间推进复算：第 b 批起点 = 12001 + (b−1)×20，批内逐号连续。
        let mut b = 1u32;
        while b <= BATCH_COUNT {
            let base = FACE_FIRST + (b - 1) * FACES_PER_BATCH;
            let mut k = 0u32;
            while k < FACES_PER_BATCH {
                let idx = ((b - 1) * FACES_PER_BATCH + k) as usize;
                match self.rows.get(idx) {
                    Some(r) => {
                        if r.no != base + k {
                            return Err(SentDiag::EUniqueBreak.val());
                        }
                        if r.batch != b {
                            return Err(SentDiag::EBatchRange.val());
                        }
                        if !r.fp_recompute_ok() {
                            return Err(SentDiag::EFpMismatch.val());
                        }
                    }
                    None => return Err(SentDiag::EUniqueBreak.val()),
                }
                k += 1;
            }
            b += 1;
        }
        // 鸽笼位图去重：800 位各占一格，双占即重号（两两唯一性的线性等价判定）。
        let mut seen: Vec<bool> = Vec::new();
        let mut i = 0usize;
        while i < FACE_COUNT {
            seen.push(false);
            i += 1;
        }
        for r in self.rows.iter() {
            if r.no < FACE_FIRST || r.no > FACE_LAST {
                return Err(SentDiag::ENoRange.val());
            }
            let slot = (r.no - FACE_FIRST) as usize;
            match seen.get_mut(slot) {
                Some(flag) => {
                    if *flag {
                        return Err(SentDiag::EDupNo.val());
                    }
                    *flag = true;
                }
                None => return Err(SentDiag::ENoRange.val()),
            }
        }
        Ok(FACE_COUNT as u32)
    }
}

// ── 域账与断言三：满账对平（逐位复算 + 溢出守卫双通道） ──────────
/// 域账三行：主账 / 辅账 / 满账总额。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainLedger {
    /// 主账（判据冻结 233,674）。
    pub main: u64,
    /// 辅账（判据冻结 6,326）。
    pub aux: u64,
    /// 满账总额（判据冻结 240,000）。
    pub total: u64,
}

impl DomainLedger {
    /// 判据冻结原账。
    pub fn frozen() -> DomainLedger {
        DomainLedger {
            main: LEDGER_MAIN,
            aux: LEDGER_AUX,
            total: LEDGER_TOTAL,
        }
    }

    /// 断言三：域账对平。三通道缺一不可：
    /// ① checked_add 溢出守卫；② 十进制逐位进位复算；
    /// ③ 满账恒等 total == 240,000。
    pub fn close(&self) -> Result<u64, u32> {
        // 通道①：溢出守卫。
        let sum = match self.main.checked_add(self.aux) {
            Some(s) => s,
            None => return Err(SentDiag::EOverflow.val()),
        };
        // 通道②：逐位进位复算（主账+辅账的十进制竖式，逐位比对总额）。
        let mut a = self.main;
        let mut b = self.aux;
        let mut t = self.total;
        let mut carry = 0u64;
        let mut guard = 0u32;
        loop {
            if a == 0 && b == 0 && carry == 0 {
                break;
            }
            guard += 1;
            if guard > 24 {
                // u64 十进制至多 20 位 + 进位 1 位，越界即账行被灌入超界值。
                return Err(SentDiag::EOverflow.val());
            }
            let da = a % 10;
            let db = b % 10;
            let s = da + db + carry;
            if t == 0 {
                return Err(SentDiag::ELedgerBreak.val());
            }
            if s % 10 != t % 10 {
                return Err(SentDiag::ELedgerBreak.val());
            }
            t /= 10;
            carry = s / 10;
            a /= 10;
            b /= 10;
        }
        if carry != 0 || t != 0 {
            return Err(SentDiag::ELedgerBreak.val());
        }
        // 通道③：满账恒等（判据原文 240,000）。
        if sum != LEDGER_TOTAL || self.total != LEDGER_TOTAL {
            return Err(SentDiag::ELedgerBreak.val());
        }
        Ok(sum)
    }
}

/// 断言三判定器（自由函数口）：对冻结原账必须绿。
pub fn assert_ledger(l: &DomainLedger) -> Result<u64, u32> {
    l.close()
}

// ── 确定性 LCG（种子 = SEG_BASE，零随机红线合规） ────────────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性）。
    pub fn next_u32(&mut self) -> u32 {
        // LCG 常数取自 Knuth MMIX 31 位推荐值（整数手册常用闭集）。
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 悬空注入判定器（ESRCH 语义） ─────────────────────────────────
/// 悬空注入判定器：号面窗外号 → Err（ESRCH 语义：查无此号）；
/// 窗内号 → Ok。前哨预验的唯一放行口。
pub fn probe_dangling(id: u32) -> Result<(), u32> {
    if id < FACE_FIRST || id > FACE_LAST {
        return Err(SentDiag::ENoRange.val());
    }
    Ok(())
}

// ── 构建器：满账登记表（800 条全公式冻结生成） ───────────────────
/// 构建满账登记表：从 12001 逐号推进登记 800 条并冻结。
/// 正常流不出错；出错即中断（缺行账本会被断言抓住变红）。
pub fn build_full_registry() -> FaceRegistry {
    let mut reg = FaceRegistry::new();
    let mut i = 0u32;
    while i < FACE_COUNT as u32 {
        match reg.register(FACE_FIRST + i) {
            Ok(()) => i += 1,
            Err(e) => {
                let _ = e;
                break;
            }
        }
    }
    let _ = reg.freeze();
    reg
}

/// 构建满账但不冻结（判定次序守卫用）。
pub fn build_rows_only() -> FaceRegistry {
    let mut reg = FaceRegistry::new();
    let mut i = 0u32;
    while i < FACE_COUNT as u32 {
        match reg.register(FACE_FIRST + i) {
            Ok(()) => i += 1,
            Err(e) => {
                let _ = e;
                break;
            }
        }
    }
    reg
}

// ── 注入器一类：悬空号注入 10 轮（窗外号必被 probe_dangling 抓） ──
/// 悬空号注入：10 轮 LCG 定点生成窗外号（窗下/窗上/远处三路轮转），
/// 每轮必须 Err（ESRCH 语义）。返回（抓到轮数, 总轮数, 首个逃脱描述）。
pub fn dangling_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x11) as u64);
    let mut k = 0u32;
    while k < 10 {
        let mode = rng.below(3);
        let off = rng.below(100);
        let id = match mode {
            0 => FACE_FIRST.saturating_sub(1 + off),
            1 => FACE_LAST.saturating_add(1 + off),
            _ => {
                let v = rng.next_u32();
                if v >= FACE_FIRST && v <= FACE_LAST {
                    FACE_LAST.saturating_add(1 + v % 100)
                } else {
                    v
                }
            }
        };
        total += 1;
        if probe_dangling(id) == Err(SentDiag::ENoRange.val()) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("悬空号注入第{}轮逃脱：id={} 未被判为窗外", k, id);
        }
        k += 1;
    }
    // 阳性对照：窗内首尾号必须放行，否则判定面反向失效。
    if probe_dangling(FACE_FIRST).is_err() && escape.is_empty() {
        escape = "阳性对照失败：F12001 被误判悬空".to_string();
    }
    if probe_dangling(FACE_LAST).is_err() && escape.is_empty() {
        escape = "阳性对照失败：F12800 被误判悬空".to_string();
    }
    (caught, total, escape)
}

// ── 注入器二类：批号越界注入 10 轮（越界批号必被闭集闸抓） ───────
/// 批号越界注入：10 轮 LCG 定点构造越界/错位批号（零批/超上界/批号错位），
/// register_with_batch 必须逐轮 EBatchRange。返回（抓到轮数, 总轮数, 逃脱描述）。
pub fn batch_inject_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x22) as u64);
    let mut k = 0u32;
    while k < 10 {
        // 落点号取窗内任一号（闸序保证批号闸先于号序闸必抓）。
        let no = FACE_FIRST + rng.below(FACE_COUNT as u32);
        let derived = derive_batch(no);
        let bad_batch = match k % 3 {
            0 => 0u32,
            1 => BATCH_COUNT + 1 + rng.below(9),
            _ => {
                if derived >= BATCH_COUNT {
                    1u32
                } else {
                    derived + 1
                }
            }
        };
        let mut reg = FaceRegistry::new();
        total += 1;
        if reg.register_with_batch(no, bad_batch) == Err(SentDiag::EBatchRange.val()) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("批号越界注入第{}轮逃脱：no={} batch={}", k, no, bad_batch);
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 注入器三类：账行篡改注入 10 轮（对平判定必抓） ───────────────
/// 账行篡改注入：10 轮六路轮转篡改域账三行（主账+/辅账+/总额+/主账清零/
/// 总额自引用/溢出灌入），close() 必须逐轮 Err。返回（抓到轮数, 总轮数, 逃脱描述）。
pub fn ledger_tamper_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x33) as u64);
    let mut k = 0u32;
    while k < 10 {
        let delta = 1 + rng.below(50) as u64;
        let base = DomainLedger::frozen();
        let tampered = match k % 6 {
            0 => DomainLedger {
                main: base.main + delta,
                aux: base.aux,
                total: base.total,
            },
            1 => DomainLedger {
                main: base.main,
                aux: base.aux + delta,
                total: base.total,
            },
            2 => DomainLedger {
                main: base.main,
                aux: base.aux,
                total: base.total + delta,
            },
            3 => DomainLedger {
                main: 0,
                aux: base.aux,
                total: base.total,
            },
            4 => DomainLedger {
                main: base.main,
                aux: base.aux,
                total: base.main,
            },
            _ => DomainLedger {
                main: u64::MAX,
                aux: base.aux,
                total: base.total,
            },
        };
        total += 1;
        if tampered.close().is_err() {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("账行篡改注入第{}轮逃脱：对平判定未抓到", k);
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 窗口 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：全部落在 [SEG_BASE, SEG_BASE+0x40) 窗口且查表可命中。
fn diag_closed_ok() -> bool {
    for d in SENT_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if detail(v) != d.detail() {
            return false;
        }
        if d.detail().is_empty() {
            return false;
        }
    }
    // 未知码必须回落。
    if detail(SEG_BASE + 0x3F) == SentDiag::ENoRange.detail() {
        return false;
    }
    true
}

/// 正常流：800 条满账 + 冻结 + 三断言全绿。
fn happy_path_ok() -> bool {
    let reg = build_full_registry();
    if reg.state() != RegistryState::Frozen || reg.len() != FACE_COUNT {
        return false;
    }
    match reg.assert_batches() {
        Ok(b) => {
            if b != BATCH_COUNT {
                return false;
            }
        }
        Err(_) => return false,
    }
    match reg.assert_unique() {
        Ok(n) => {
            if n != FACE_COUNT as u32 {
                return false;
            }
        }
        Err(_) => return false,
    }
    assert_ledger(&DomainLedger::frozen()) == Ok(LEDGER_TOTAL)
}

/// 悬空注入 10 轮全抓。
fn dangling_sweep_ok() -> bool {
    let (caught, total, escape) = dangling_sweep();
    if total != 10 || caught != 10 {
        return false;
    }
    escape.is_empty()
}

/// 批号越界注入 10 轮全抓。
fn batch_inject_ok() -> bool {
    let (caught, total, escape) = batch_inject_sweep();
    if total != 10 || caught != 10 {
        return false;
    }
    escape.is_empty()
}

/// 账行篡改注入 10 轮全抓。
fn ledger_tamper_ok() -> bool {
    let (caught, total, escape) = ledger_tamper_sweep();
    if total != 10 || caught != 10 {
        return false;
    }
    escape.is_empty()
}

/// 判定次序守卫：未冻结先判必拒、冻结后三断言按序放行、重复冻结倒退拒、
/// 冻结后写入拒。
fn judge_order_ok() -> bool {
    // 半账表（3 行）未冻结：断言一必须 ENotFrozen。
    let mut half = FaceRegistry::new();
    let _ = half.register(FACE_FIRST);
    let _ = half.register(FACE_FIRST + 1);
    let _ = half.register(FACE_FIRST + 2);
    if half.assert_batches() != Err(SentDiag::ENotFrozen.val()) {
        return false;
    }
    if half.assert_unique() != Err(SentDiag::ENotFrozen.val()) {
        return false;
    }
    // 满账未冻结：断言仍拒。
    let mut full = build_rows_only();
    if full.state() != RegistryState::Full {
        return false;
    }
    if full.assert_batches() != Err(SentDiag::ENotFrozen.val()) {
        return false;
    }
    // 满账未齐不许冻结。
    if half.freeze() != Err(SentDiag::EBatchNotFull.val()) {
        return false;
    }
    // 冻结后三断言按序放行。
    if full.freeze().is_err() {
        return false;
    }
    if full.assert_batches().is_err() || full.assert_unique().is_err() {
        return false;
    }
    // 重复冻结倒退拒；冻结后写入拒。
    if full.freeze() != Err(SentDiag::ERewind.val()) {
        return false;
    }
    full.register(FACE_FIRST) == Err(SentDiag::EFrozen.val())
}

/// 双跑一致指纹：两次完整构建逐行一致、全表指纹一致、三路注入 sweep 一致。
fn dual_run_ok() -> bool {
    let a = build_full_registry();
    let b = build_full_registry();
    if a.len() != b.len() || a.state() != b.state() || a.fp() != b.fp() {
        return false;
    }
    let mut i = 0usize;
    while i < FACE_COUNT {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    dangling_sweep() == dangling_sweep()
        && batch_inject_sweep() == batch_inject_sweep()
        && ledger_tamper_sweep() == ledger_tamper_sweep()
}

/// 边界四点：首号/末号批号归属、窗外首尾拒、次号乱序拒、指纹公式确定性。
fn boundary_ok() -> bool {
    // 首号 12001 → 批 1；末号 12800 → 批 40（派生公式端点）。
    if derive_batch(FACE_FIRST) != 1 || derive_batch(FACE_LAST) != BATCH_COUNT {
        return false;
    }
    // 批间推进端点：第 2 批首号 = 12021，第 40 批首号 = 12781。
    if FACE_FIRST + FACES_PER_BATCH != 12021
        || FACE_FIRST + (BATCH_COUNT - 1) * FACES_PER_BATCH != 12781
    {
        return false;
    }
    // 窗外首尾拒（ESRCH 语义）。
    if probe_dangling(FACE_FIRST - 1).is_ok() || probe_dangling(FACE_LAST + 1).is_ok() {
        return false;
    }
    // 次号乱序：首条不登 12001 而登 12003 被拒。
    let mut reg = FaceRegistry::new();
    if reg.register(FACE_FIRST + 2) != Err(SentDiag::ENoOrder.val()) {
        return false;
    }
    // 指纹公式确定性：同参同值、异参异值（抽样三点）。
    let f1 = derive_fingerprint(FACE_FIRST, 1);
    if f1 != derive_fingerprint(FACE_FIRST, 1) {
        return false;
    }
    if f1 == derive_fingerprint(FACE_FIRST + 1, 1) {
        return false;
    }
    if derive_fingerprint(FACE_LAST, BATCH_COUNT)
        == derive_fingerprint(FACE_LAST - 1, BATCH_COUNT)
    {
        return false;
    }
    true
}

/// 容量耗尽：800 条满账后第 801 号（12801 窗外）与窗内重推一律拒。
fn capacity_ok() -> bool {
    let mut reg = build_rows_only();
    // 满账未冻结仍可走冻结口；先验满账态再写入必拒。
    if reg.len() != FACE_COUNT {
        return false;
    }
    // 满账后（未冻结）注册窗内号 → 容量闸/号序闸必拒其一，窗内顺序号落在容量闸。
    if reg.register(FACE_FIRST) != Err(SentDiag::ECapFull.val()) {
        return false;
    }
    // 冻结后再写 → 冻结闸优先。
    let _ = reg.freeze();
    reg.register(FACE_FIRST) == Err(SentDiag::EFrozen.val())
}

/// 域账双通道一致性：竖式复算结果与 checked_add 恒等，冻结原账恒绿。
fn ledger_channels_ok() -> bool {
    let l = DomainLedger::frozen();
    let checked = l.main.checked_add(l.aux);
    match l.close() {
        Ok(s) => checked == Some(s) && s == LEDGER_TOTAL,
        Err(_) => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12798 自检（判据 12798 卡：F12800 前哨八百条互引零悬空预验三断言）。
pub fn run_unx12798_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12798");
    let reg = build_full_registry();

    s.add_dyn(
        "F12799-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12799-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "n={} first=0x{:05x}",
            SENT_ALL.len(),
            SENT_ALL.first().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12799-03 号面登记表满账正常流",
        happy_path_ok(),
        &format!(
            "rows={} state={:?} fp={:016x}",
            reg.len(),
            reg.state(),
            reg.fp()
        ),
    );

    // 断言一：40 批齐备。
    let batch_verdict = reg.assert_batches();
    s.add_dyn(
        "F12799-04 断言一 40 批齐备",
        batch_verdict == Ok(BATCH_COUNT),
        &format!(
            "batches={}/{} rows={}",
            batch_verdict.unwrap_or(0),
            BATCH_COUNT,
            reg.len()
        ),
    );

    // 断言二：800 判据唯一。
    let unique_verdict = reg.assert_unique();
    s.add_dyn(
        "F12799-05 断言二 800 判据唯一",
        unique_verdict == Ok(FACE_COUNT as u32),
        &format!(
            "unique={}/{}",
            unique_verdict.unwrap_or(0),
            FACE_COUNT
        ),
    );

    // 断言三：域账对平。
    let ledger_verdict = assert_ledger(&DomainLedger::frozen());
    s.add_dyn(
        "F12799-06 断言三 域账满账对平",
        ledger_verdict == Ok(LEDGER_TOTAL),
        &format!(
            "close={:?} main={} aux={} total={}",
            ledger_verdict.ok(),
            LEDGER_MAIN,
            LEDGER_AUX,
            LEDGER_TOTAL
        ),
    );

    // 注入必抓一类：悬空号 10 轮。
    let (d_caught, d_total, d_escape) = dangling_sweep();
    s.add_dyn(
        "F12799-07 悬空号注入 10 轮全抓",
        dangling_sweep_ok(),
        &format!("caught={}/{} {}", d_caught, d_total, d_escape),
    );

    // 注入必抓二类：批号越界 10 轮。
    let (b_caught, b_total, b_escape) = batch_inject_sweep();
    s.add_dyn(
        "F12799-08 批号越界注入 10 轮全抓",
        batch_inject_ok(),
        &format!("caught={}/{} {}", b_caught, b_total, b_escape),
    );

    // 注入必抓三类：账行篡改 10 轮。
    let (l_caught, l_total, l_escape) = ledger_tamper_sweep();
    s.add_dyn(
        "F12799-09 账行篡改注入 10 轮全抓",
        ledger_tamper_ok(),
        &format!("caught={}/{} {}", l_caught, l_total, l_escape),
    );

    s.add_dyn(
        "F12799-10 判定次序守卫",
        judge_order_ok(),
        "未冻结先判拒/半账冻结拒/冻结后按序放行/重复冻结倒退拒/冻结后写入拒",
    );

    s.add_dyn(
        "F12799-11 双跑一致指纹",
        dual_run_ok(),
        "两次构建逐行一致 + 全表指纹一致 + 三路注入序列一致",
    );

    s.add_dyn(
        "F12799-12 边界四点",
        boundary_ok(),
        "首末号批号归属/批间推进端点/窗外首尾拒/次号乱序拒/指纹确定性",
    );

    s.add_dyn(
        "F12799-13 容量耗尽",
        capacity_ok(),
        "800 满账后再写拒，冻结后写入拒",
    );

    s.add_dyn(
        "F12799-14 域账双通道一致",
        ledger_channels_ok(),
        "竖式逐位复算与 checked_add 恒等且满账 240000",
    );

    // 收口：判据条数声明（前有 14 条，本条为第 15 条收口）。
    let declared = 14usize;
    let actual = s.len();
    s.add_dyn(
        "F12799-15 收口 判据条数声明",
        actual == declared,
        &format!("declared={} actual={}", declared, actual),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 窗口 [0x80000, 0x831C0]。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x83180);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：14 个码互异、全部落窗口、查表可命中、文案非空。
        assert_eq!(SENT_ALL.len(), 14);
        let mut i = 0usize;
        while i < SENT_ALL.len() {
            let vi = SENT_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            assert!(!detail(vi).is_empty());
            let mut j = i + 1;
            while j < SENT_ALL.len() {
                let vj = SENT_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            i += 1;
        }
        // 未知码回落。
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
        assert_eq!(
            detail(SentDiag::EOverflow.val()),
            SentDiag::EOverflow.detail()
        );
    }

    #[test]
    fn happy_path_three_assertions_green() {
        // 正常流：800 条满账 + 冻结 + 三断言全绿。
        let reg = build_full_registry();
        assert_eq!(reg.len(), 800);
        assert_eq!(reg.state(), RegistryState::Frozen);
        assert_eq!(reg.assert_batches(), Ok(40));
        assert_eq!(reg.assert_unique(), Ok(800));
        assert_eq!(assert_ledger(&DomainLedger::frozen()), Ok(240_000));
        assert!(happy_path_ok());
    }

    #[test]
    fn dangling_injection_ten_rounds_all_caught() {
        // 注入一类：悬空号 10 轮全检出（ESRCH 语义），阳性对照放行。
        let (caught, total, escape) = dangling_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱轮：{}", escape);
        assert!(escape.is_empty());
        assert!(probe_dangling(12001).is_ok());
        assert!(probe_dangling(12800).is_ok());
        assert_eq!(probe_dangling(12000), Err(SentDiag::ENoRange.val()));
        assert_eq!(probe_dangling(12801), Err(SentDiag::ENoRange.val()));
        assert!(dangling_sweep_ok());
    }

    #[test]
    fn batch_injection_ten_rounds_all_caught() {
        // 注入二类：批号越界 10 轮全抓（零批/超上界/错位）。
        let (caught, total, escape) = batch_inject_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱轮：{}", escape);
        assert!(escape.is_empty());
        // 定点复核：零批与 41 批无论落点一律 EBatchRange。
        let mut reg = FaceRegistry::new();
        assert_eq!(reg.register_with_batch(12001, 0), Err(SentDiag::EBatchRange.val()));
        let mut reg2 = FaceRegistry::new();
        assert_eq!(reg2.register_with_batch(12001, 41), Err(SentDiag::EBatchRange.val()));
        assert!(batch_inject_ok());
    }

    #[test]
    fn ledger_tamper_ten_rounds_all_caught() {
        // 注入三类：账行篡改 10 轮全抓（六路轮转含溢出灌入）。
        let (caught, total, escape) = ledger_tamper_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱轮：{}", escape);
        assert!(escape.is_empty());
        // 溢出守卫单独定点：u64::MAX 主账必须 EOverflow。
        let ovf = DomainLedger {
            main: u64::MAX,
            aux: LEDGER_AUX,
            total: LEDGER_TOTAL,
        };
        assert_eq!(ovf.close(), Err(SentDiag::EOverflow.val()));
        assert!(ledger_tamper_ok());
    }

    #[test]
    fn dual_run_fingerprint_identical() {
        // 双跑一致：两次构建逐行一致、全表指纹一致、注入序列一致。
        let a = build_full_registry();
        let b = build_full_registry();
        assert_eq!(a.len(), b.len());
        assert_eq!(a.fp(), b.fp());
        let mut i = 0usize;
        while i < FACE_COUNT {
            assert_eq!(a.row_at(i), b.row_at(i));
            i += 1;
        }
        assert_eq!(dangling_sweep(), dangling_sweep());
        assert_eq!(batch_inject_sweep(), batch_inject_sweep());
        assert_eq!(ledger_tamper_sweep(), ledger_tamper_sweep());
        assert!(dual_run_ok());
    }

    #[test]
    fn boundaries_first_last_and_order() {
        // 边界：首末号批号归属、批间推进端点、窗外拒、乱序拒、指纹确定性。
        assert_eq!(derive_batch(12001), 1);
        assert_eq!(derive_batch(12020), 1);
        assert_eq!(derive_batch(12021), 2);
        assert_eq!(derive_batch(12800), 40);
        assert_eq!(derive_batch(12000), 0);
        assert_eq!(derive_batch(12801), 0);
        assert!(boundary_ok());
    }

    #[test]
    fn capacity_exhaustion_rejects() {
        // 容量耗尽：800 满账后再写拒，冻结后写入拒。
        let mut reg = build_rows_only();
        assert_eq!(reg.len(), 800);
        assert_eq!(reg.register(FACE_FIRST), Err(SentDiag::ECapFull.val()));
        assert!(reg.freeze().is_ok());
        assert_eq!(reg.register(FACE_FIRST), Err(SentDiag::EFrozen.val()));
        assert!(capacity_ok());
    }

    #[test]
    fn judge_order_never_skips_freeze_gate() {
        // 判定次序：半账/满账未冻结先判必拒；冻结后放行；倒退拒。
        let mut half = FaceRegistry::new();
        let _ = half.register(FACE_FIRST);
        assert_eq!(half.assert_batches(), Err(SentDiag::ENotFrozen.val()));
        assert_eq!(half.assert_unique(), Err(SentDiag::ENotFrozen.val()));
        assert_eq!(half.freeze(), Err(SentDiag::EBatchNotFull.val()));
        let mut full = build_rows_only();
        assert_eq!(full.assert_batches(), Err(SentDiag::ENotFrozen.val()));
        assert!(full.freeze().is_ok());
        assert_eq!(full.freeze(), Err(SentDiag::ERewind.val()));
        assert!(full.assert_batches().is_ok());
        assert!(full.assert_unique().is_ok());
        assert!(judge_order_ok());
    }

    #[test]
    fn ledger_digit_chain_matches_checked_add() {
        // 域账双通道：竖式逐位复算与 checked_add 恒等，冻结原账恒绿。
        let l = DomainLedger::frozen();
        assert_eq!(l.close(), Ok(LEDGER_TOTAL));
        assert_eq!(l.main.checked_add(l.aux), Some(LEDGER_TOTAL));
        // 错一位的总额必被逐位复算抓住。
        let shifted = DomainLedger {
            main: l.main,
            aux: l.aux,
            total: 24_000,
        };
        assert_eq!(shifted.close(), Err(SentDiag::ELedgerBreak.val()));
        assert!(ledger_channels_ok());
    }

    #[test]
    fn fingerprint_formula_is_recomputable() {
        // 指纹派生公式：同参同值、异参异值，冻结行全部复算通过。
        let f = derive_fingerprint(12001, 1);
        assert_eq!(f, derive_fingerprint(12001, 1));
        assert_ne!(f, derive_fingerprint(12002, 1));
        assert_ne!(f, derive_fingerprint(12001, 2));
        let reg = build_full_registry();
        let mut i = 0usize;
        while i < FACE_COUNT {
            match reg.row_at(i) {
                Some(row) => {
                    assert!(row.fp_recompute_ok());
                    assert_eq!(row.no, 12001 + i as u32);
                    assert_eq!(row.batch, i as u32 / 20 + 1);
                }
                None => assert!(false, "第{}行应存在", i),
            }
            i += 1;
        }
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿、覆盖 ≥12 条、零红项。
        let s = run_unx12798_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
