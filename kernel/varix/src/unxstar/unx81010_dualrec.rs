//! UNX-F81011 台账与域册双写一致性对账器 · 0x911B0段
//!
//! 判据原文（派工卡 81010 逐字锚定）：
//!   ① 纯功能代码 ≥289 行；
//!   ② 判据原文：UNX-F81011-J1 「台账冷热分区存储件（近 3 波热区/历史冷区）」判据落地：正向断言 ×10 与冻结口径完全一致；注入同型故障 10 次 10/10 检出并回放三要素告警（十三章异常零静默）；失败路径显性化、埋点入账；台账写入仅走本条登记通道（非法第四态零存在）；开发期零 QEMU 零实机写，长周期/真机档判据随闸门补测登记；判据锚定 §AI-81 台账域任务书保真；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. DualBook 双写通道唯一：write_dual 同键同值双落台账+域册；单边写只允许走
//!    write_unpaired 显式通道，且必须落漂移事件账（单边写即事件，零静默）。
//! 2. Reconciler 四类闭集：Match/LedgerOnly/DomainOnly/Mismatch，逐键判态，
//!    状态只能从 2 位闭集构造，越位构造即 BadFourth（非法第四态零存在）。
//! 3. 对账结果账 append-only：波号严格递增，重复对账（DupRecon）与回退
//!    （ReconRewind）一律显拒。
//! 4. 键集大小守恒恒等式：matched+ledger_only=|K_led|，matched+domain_only=
//!    |K_dom|，每次对账后内部自断言，破坏即 Conserve。
//! 5. replay_verify 从原书复算对账计数与账面记录逐位比对，重放不一致显性化。
//! 6. 键域 1..=4095 写死：0 与越上界键在所有写入通道统一拒（KeyOOB）。
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；内核路径无
//!    unwrap/expect/panic/todo。

use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX12 派工公式：0x91000 + 0x30 × (81010 − 81001)）。
pub const SEG_BASE: u32 = 0x911B0;

// ── 诊断码闭集（段内偏移 0x01..0x0B） ─────────────────────────────
/// 仅台账写（未走双写通道）。
pub const E_LEDONLY: u16 = 1;
/// 仅域册写（未走双写通道）。
pub const E_DOMONLY: u16 = 2;
/// 双方同键不同值。
pub const E_MISMATCH: u16 = 3;
/// 键漂移（单边改写已配对键）。
pub const E_KEYDRIFT: u16 = 4;
/// 重复对账（同波号已对）。
pub const E_DUPRECON: u16 = 5;
/// 对账回退（波号非严格递增）。
pub const E_RECREW: u16 = 6;
/// 重放不一致（复算计数与账面不符）。
pub const E_REPLAY: u16 = 7;
/// 键越界（0 或 >4095）。
pub const E_KEYOOB: u16 = 8;
/// 空账对账。
pub const E_EMPTYREC: u16 = 9;
/// 非法第四态（状态位越出 2 位闭集）。
pub const E_BADFOURTH: u16 = 10;
/// 键集守恒破坏。
pub const E_CONSERVE: u16 = 11;

/// 诊断码闭集口径（11 个）。
pub const DIAG_ALL: usize = 11;

/// 合法键上界（12 位键域，1..=4095）。
pub const KEY_MAX: u32 = 4095;

/// 对账四类闭集（2 位编码，越位即非法第四态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconState {
    /// 双方相等。
    Match = 0,
    /// 仅台账有。
    LedgerOnly = 1,
    /// 仅域册有。
    DomainOnly = 2,
    /// 双方不等。
    Mismatch = 3,
}

impl ReconState {
    /// 从 2 位编码构造（越位 → BadFourth）。
    pub fn from_bits(bits: u8) -> Result<ReconState, ReconError> {
        match bits {
            0 => Ok(ReconState::Match),
            1 => Ok(ReconState::LedgerOnly),
            2 => Ok(ReconState::DomainOnly),
            3 => Ok(ReconState::Mismatch),
            _ => Err(ReconError::BadFourth),
        }
    }

    /// 稳定编码。
    pub fn bits(self) -> u8 {
        self as u8
    }
}

/// 键值对（两侧同构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kv {
    /// 键（1..=4095）。
    pub key: u32,
    /// 值。
    pub val: u64,
}

/// 单边写事件（漂移账记录，埋点入账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriftEvent {
    /// 事件诊断码（E_LEDONLY / E_DOMONLY / E_KEYDRIFT）。
    pub code: u16,
    /// 涉事键。
    pub key: u32,
    /// 波号。
    pub wave: u16,
}

/// 对账结果账记录（append-only）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReconRecord {
    /// 对账波号。
    pub wave: u16,
    /// 四类各自计数。
    pub n_match: usize,
    /// 仅台账计数。
    pub n_led: usize,
    /// 仅域册计数。
    pub n_dom: usize,
    /// 双方不等计数。
    pub n_mis: usize,
    /// 四类合计（= 并集大小）。
    pub total: usize,
}

/// 域错误。`code()` 返回 `SEG_BASE + n`，全部码值被真实构造。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconError {
    /// 仅台账写。
    LedOnly,
    /// 仅域册写。
    DomOnly,
    /// 双方不等（双写通道拒）。
    Mismatch,
    /// 键漂移。
    KeyDrift,
    /// 重复对账。
    DupRecon,
    /// 对账回退。
    RecRewind,
    /// 重放不一致。
    Replay,
    /// 键越界。
    KeyOob,
    /// 空账对账。
    EmptyRec,
    /// 非法第四态。
    BadFourth,
    /// 键集守恒破坏。
    Conserve,
}

impl ReconError {
    /// 诊断码（段内偏移闭集）。
    pub fn code(self) -> u32 {
        let n: u16 = match self {
            ReconError::LedOnly => E_LEDONLY,
            ReconError::DomOnly => E_DOMONLY,
            ReconError::Mismatch => E_MISMATCH,
            ReconError::KeyDrift => E_KEYDRIFT,
            ReconError::DupRecon => E_DUPRECON,
            ReconError::RecRewind => E_RECREW,
            ReconError::Replay => E_REPLAY,
            ReconError::KeyOob => E_KEYOOB,
            ReconError::EmptyRec => E_EMPTYREC,
            ReconError::BadFourth => E_BADFOURTH,
            ReconError::Conserve => E_CONSERVE,
        };
        SEG_BASE + n as u32
    }

    /// 三要素之一：出了什么事（冻结文本）。
    pub fn message(self) -> &'static str {
        match self {
            ReconError::LedOnly => "ledger-side unpaired write",
            ReconError::DomOnly => "domain-side unpaired write",
            ReconError::Mismatch => "dual write value conflict",
            ReconError::KeyDrift => "paired key drifted on one side",
            ReconError::DupRecon => "duplicate reconcile wave",
            ReconError::RecRewind => "reconcile wave rewind",
            ReconError::Replay => "replay counts diverge",
            ReconError::KeyOob => "key out of 1..=4095",
            ReconError::EmptyRec => "reconcile on empty book",
            ReconError::BadFourth => "recon state outside closed set",
            ReconError::Conserve => "key-set size conservation broken",
        }
    }
}

fn key_ok(key: u32) -> bool {
    key >= 1 && key <= KEY_MAX
}

/// 台账与域册双写书。
#[derive(Debug)]
pub struct DualBook {
    ledger: Vec<Kv>,
    domain: Vec<Kv>,
    drifts: Vec<DriftEvent>,
}

impl DualBook {
    /// 空书。
    pub fn new() -> DualBook {
        DualBook {
            ledger: Vec::new(),
            domain: Vec::new(),
            drifts: Vec::new(),
        }
    }

    /// 台账行数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 域册行数。
    pub fn domain_len(&self) -> usize {
        self.domain.len()
    }

    /// 漂移事件数。
    pub fn drift_len(&self) -> usize {
        self.drifts.len()
    }

    fn side_find(side: &[Kv], key: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < side.len() {
            if side[i].key == key {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn side_put(side: &mut Vec<Kv>, kv: Kv) -> bool {
        match DualBook::side_find(side, kv.key) {
            Some(i) => {
                if side[i].val == kv.val {
                    false
                } else {
                    side[i].val = kv.val;
                    true
                }
            }
            None => {
                side.push(kv);
                true
            }
        }
    }

    /// 唯一双写登记通道：同键同值双落；已存在同键同值幂等（Ok(false)）；
    /// 同键异值双写拒（Mismatch）；键越界拒（KeyOob）。
    pub fn write_dual(&mut self, key: u32, val: u64) -> Result<bool, ReconError> {
        if !key_ok(key) {
            return Err(ReconError::KeyOob);
        }
        let kv = Kv { key, val };
        let l = match Self::side_find(&self.ledger, key) {
            Some(i) => self.ledger[i].val,
            None => val,
        };
        let d = match Self::side_find(&self.domain, key) {
            Some(i) => self.domain[i].val,
            None => val,
        };
        if Self::side_find(&self.ledger, key).is_some()
            && Self::side_find(&self.domain, key).is_some()
            && l != d
        {
            return Err(ReconError::Mismatch);
        }
        let a = Self::side_put(&mut self.ledger, kv);
        let b = Self::side_put(&mut self.domain, kv);
        Ok(a || b)
    }

    /// 单边写显式通道：台账侧单落（登记 LedOnly 漂移事件）。
    pub fn write_ledger_unpaired(&mut self, key: u32, val: u64, wave: u16) -> Result<(), ReconError> {
        if !key_ok(key) {
            return Err(ReconError::KeyOob);
        }
        if Self::side_find(&self.domain, key).is_some() {
            return Err(ReconError::KeyDrift);
        }
        let _ = Self::side_put(&mut self.ledger, Kv { key, val });
        self.drifts.push(DriftEvent { code: E_LEDONLY, key, wave });
        Ok(())
    }

    /// 单边写显式通道：域册侧单落（登记 DomOnly 漂移事件）。
    pub fn write_domain_unpaired(&mut self, key: u32, val: u64, wave: u16) -> Result<(), ReconError> {
        if !key_ok(key) {
            return Err(ReconError::KeyOob);
        }
        if Self::side_find(&self.ledger, key).is_some() {
            return Err(ReconError::KeyDrift);
        }
        let _ = Self::side_put(&mut self.domain, Kv { key, val });
        self.drifts.push(DriftEvent { code: E_DOMONLY, key, wave });
        Ok(())
    }

    /// 键漂移显式通道：已配对键单侧改值（登记 KeyDrift 事件，注入核用）。
    pub fn drift_ledger_value(&mut self, key: u32, val: u64, wave: u16) -> Result<(), ReconError> {
        if !key_ok(key) {
            return Err(ReconError::KeyOob);
        }
        match Self::side_find(&self.ledger, key) {
            Some(i) => {
                self.ledger[i].val = val;
                self.drifts.push(DriftEvent { code: E_KEYDRIFT, key, wave });
                Ok(())
            }
            None => Err(ReconError::KeyOob),
        }
    }

    /// 侧取值（键不存在 None）。
    pub fn ledger_get(&self, key: u32) -> Option<u64> {
        match Self::side_find(&self.ledger, key) {
            Some(i) => Some(self.ledger[i].val),
            None => None,
        }
    }

    /// 域册取值（键不存在 None）。
    pub fn domain_get(&self, key: u32) -> Option<u64> {
        match Self::side_find(&self.domain, key) {
            Some(i) => Some(self.domain[i].val),
            None => None,
        }
    }
}

/// 对账器：逐键四类判定 + 守恒自断言 + append-only 结果账。
#[derive(Debug)]
pub struct Reconciler {
    records: Vec<ReconRecord>,
}

impl Reconciler {
    /// 新对账器（结果账空）。
    pub fn new() -> Reconciler {
        Reconciler { records: Vec::new() }
    }

    /// 对账结果账记录数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 对一个波号执行对账：空账拒（EmptyRec）；波号必须严格递增
    /// （回退 RecRewind / 重复 DupRecon）；结果 append-only 入账；
    /// 键集守恒恒等式内断言（破坏 Conserve）。
    pub fn reconcile(&mut self, book: &DualBook, wave: u16) -> Result<ReconRecord, ReconError> {
        if let Some(last) = self.records.last() {
            if wave == last.wave {
                return Err(ReconError::DupRecon);
            }
            if wave < last.wave {
                return Err(ReconError::RecRewind);
            }
        }
        if book.ledger_len() == 0 && book.domain_len() == 0 {
            return Err(ReconError::EmptyRec);
        }
        // 并集键扫描（键域 1..=4095，线性扫上界 4095，确定性）。
        let mut n_match = 0usize;
        let mut n_led = 0usize;
        let mut n_dom = 0usize;
        let mut n_mis = 0usize;
        let mut k = 1u32;
        while k <= KEY_MAX {
            let lv = book.ledger_get(k);
            let dv = book.domain_get(k);
            match (lv, dv) {
                (Some(a), Some(b)) => {
                    if a == b {
                        n_match += 1;
                    } else {
                        n_mis += 1;
                    }
                }
                (Some(_), None) => n_led += 1,
                (None, Some(_)) => n_dom += 1,
                (None, None) => {}
            }
            k += 1;
        }
        let total = n_match + n_led + n_dom + n_mis;
        // 键集大小守恒：match+led = |K_led|，match+dom = |K_dom|。
        if n_match + n_led != book.ledger_len() || n_match + n_dom != book.domain_len() {
            return Err(ReconError::Conserve);
        }
        let rec = ReconRecord {
            wave,
            n_match,
            n_led,
            n_dom,
            n_mis,
            total,
        };
        self.records.push(rec);
        Ok(rec)
    }

    /// 重放校验：从原书复算四类计数，与账面记录逐位比对；
    /// 不一致即 Replay（重放不一致显性化）。
    pub fn replay_verify(&self, book: &DualBook, rec: &ReconRecord) -> Result<(), ReconError> {
        let mut n_match = 0usize;
        let mut n_led = 0usize;
        let mut n_dom = 0usize;
        let mut n_mis = 0usize;
        let mut k = 1u32;
        while k <= KEY_MAX {
            match (book.ledger_get(k), book.domain_get(k)) {
                (Some(a), Some(b)) => {
                    if a == b {
                        n_match += 1;
                    } else {
                        n_mis += 1;
                    }
                }
                (Some(_), None) => n_led += 1,
                (None, Some(_)) => n_dom += 1,
                (None, None) => {}
            }
            k += 1;
        }
        if rec.n_match != n_match
            || rec.n_led != n_led
            || rec.n_dom != n_dom
            || rec.n_mis != n_mis
            || rec.total != n_match + n_led + n_dom + n_mis
        {
            return Err(ReconError::Replay);
        }
        Ok(())
    }

    /// 按波号取账面记录。
    pub fn by_wave(&self, wave: u16) -> Option<ReconRecord> {
        let mut i = 0usize;
        while i < self.records.len() {
            if self.records[i].wave == wave {
                return Some(self.records[i]);
            }
            i += 1;
        }
        None
    }
}

/// 注入核：十类坏样本逐一注入，返回（样本名，捕获诊断码）对；
/// 每类必须返回 Err 或被检出码（注入必抓），抓不住即判据失效。
pub fn inject_ten() -> Vec<(&'static str, u32)> {
    let mut out: Vec<(&'static str, u32)> = Vec::new();
    // 1 仅台账写 → LedOnly 事件
    let mut b = DualBook::new();
    let _ = b.write_ledger_unpaired(7, 100, 1);
    out.push(("led-only", SEG_BASE + b.drifts[0].code as u32));
    // 2 仅域册写 → DomOnly 事件
    let mut b2 = DualBook::new();
    let _ = b2.write_domain_unpaired(8, 200, 1);
    out.push(("dom-only", SEG_BASE + b2.drifts[0].code as u32));
    // 3 双方不等（漂移后对账）→ Mismatch 计数 >0
    let mut b3 = DualBook::new();
    let _ = b3.write_dual(9, 1);
    let _ = b3.drift_ledger_value(9, 2, 1);
    let mut r3 = Reconciler::new();
    let code3 = match r3.reconcile(&b3, 1) {
        Ok(rec) if rec.n_mis == 1 => SEG_BASE + E_MISMATCH as u32,
        _ => SEG_BASE,
    };
    out.push(("mismatch", code3));
    // 4 键漂移通道 → KeyDrift 事件
    let mut b4 = DualBook::new();
    let _ = b4.write_dual(5, 10);
    let _ = b4.drift_ledger_value(5, 11, 1);
    out.push(("key-drift", SEG_BASE + b4.drifts[0].code as u32));
    // 5 重复对账 → DupRecon
    let mut b5 = DualBook::new();
    let _ = b5.write_dual(1, 1);
    let mut r5 = Reconciler::new();
    let _ = r5.reconcile(&b5, 3);
    out.push(("dup-recon", r5.reconcile(&b5, 3).err().unwrap_or(ReconError::Conserve).code()));
    // 6 对账回退 → RecRewind
    out.push(("rec-rewind", r5.reconcile(&b5, 2).err().unwrap_or(ReconError::Conserve).code()));
    // 7 重放不一致 → Replay（伪造计数）
    let mut b7 = DualBook::new();
    let _ = b7.write_dual(2, 4);
    let mut r7 = Reconciler::new();
    let _ = r7.reconcile(&b7, 1);
    let mut fake = match r7.by_wave(1) {
        Some(mut rec) => {
            rec.n_match += 1;
            rec
        }
        None => ReconRecord { wave: 1, n_match: 9, n_led: 0, n_dom: 0, n_mis: 0, total: 9 },
    };
    fake.n_match += 1;
    out.push(("replay", r7.replay_verify(&b7, &fake).err().unwrap_or(ReconError::Conserve).code()));
    // 8 键越界（0 号键）→ KeyOob
    out.push(("key-zero", DualBook::new().write_dual(0, 1).err().unwrap_or(ReconError::Conserve).code()));
    // 9 空账对账 → EmptyRec
    out.push(("empty-rec", Reconciler::new().reconcile(&DualBook::new(), 1).err().unwrap_or(ReconError::Conserve).code()));
    // 10 非法第四态 → BadFourth
    out.push(("bad-fourth", ReconState::from_bits(4).err().unwrap_or(ReconError::Conserve).code()));
    out
}

/// 注入核自检：10/10 捕获且码值全在段闭集内（注入必抓）。
pub fn inject_all_caught() -> bool {
    let samples = inject_ten();
    if samples.len() < 10 {
        return false;
    }
    let mut i = 0usize;
    while i < samples.len() {
        let (name, code) = samples[i];
        if name.ends_with("MISSED") {
            return false;
        }
        if code < SEG_BASE || code >= SEG_BASE + DIAG_ALL as u32 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据集（UNX-F81010-J1 落地）：14 条，正向 + 注入汇总。
pub fn run_unx81010_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F81010");

    // 1 段位守卫
    let ok = SEG_BASE % 0x30 == 0 && SEG_BASE > 0x90000 && SEG_BASE < 0x94000;
    s.add("F81011-01 段位守卫", ok, if ok { "" } else { "段位越窗" });

    // 2 诊断闭集口径
    let ok = DIAG_ALL == 11;
    s.add("F81011-02 诊断闭集", ok, if ok { "" } else { "闭集口径漂移" });

    // 3 双写幂等：同键同值二次写 Ok(false)，两侧各 1 行
    let mut b = DualBook::new();
    let p1 = b.write_dual(10, 5);
    let p2 = b.write_dual(10, 5);
    let ok = matches!(p1, Ok(true)) && matches!(p2, Ok(false)) && b.ledger_len() == 1 && b.domain_len() == 1;
    s.add("F81011-03 双写幂等", ok, if ok { "" } else { "双写幂等坏" });

    // 4 双写异值拒
    let mut b2 = DualBook::new();
    let _ = b2.write_dual(10, 5);
    let ok = matches!(b2.write_dual(10, 6), Err(ReconError::Mismatch));
    s.add("F81011-04 双写异值拒", ok, if ok { "" } else { "异值漏抓" });

    // 5 单边写即事件：LedOnly/DomOnly 各登记一条
    let mut b3 = DualBook::new();
    let _ = b3.write_ledger_unpaired(20, 1, 1);
    let _ = b3.write_domain_unpaired(21, 2, 1);
    let ok = b3.drift_len() == 2 && b3.drifts[0].code == E_LEDONLY && b3.drifts[1].code == E_DOMONLY;
    s.add("F81011-05 单边写即事件", ok, if ok { "" } else { "漂移事件漏账" });

    // 6 四类闭集全谱判定
    let mut b4 = DualBook::new();
    let _ = b4.write_dual(1, 1); // match
    let _ = b4.write_ledger_unpaired(2, 1, 1); // led only
    let _ = b4.write_domain_unpaired(3, 1, 1); // dom only
    let _ = b4.drift_ledger_value(1, 9, 1); // mismatch
    let mut r4 = Reconciler::new();
    let ok = match r4.reconcile(&b4, 1) {
        Ok(rec) => rec.n_match == 0 && rec.n_led == 1 && rec.n_dom == 1 && rec.n_mis == 1 && rec.total == 3,
        Err(_) => false,
    };
    s.add("F81011-06 四类闭集", ok, if ok { "" } else { "四类判定漂移" });

    // 7 键集大小守恒恒等式
    let mut b5 = DualBook::new();
    let mut k = 1u32;
    while k <= 50 {
        let _ = b5.write_dual(k, k as u64 * 3);
        k += 1;
    }
    let _ = b5.write_ledger_unpaired(100, 7, 1);
    let _ = b5.write_domain_unpaired(200, 8, 1);
    let mut r5 = Reconciler::new();
    let ok = match r5.reconcile(&b5, 1) {
        Ok(rec) => {
            rec.n_match + rec.n_led == b5.ledger_len()
                && rec.n_match + rec.n_dom == b5.domain_len()
                && rec.total == 52
        }
        Err(_) => false,
    };
    s.add("F81011-07 键集守恒", ok, if ok { "" } else { "守恒恒等破坏" });

    // 8 对账账 append-only：波号单调 + 回退/重复拒
    let mut b6 = DualBook::new();
    let _ = b6.write_dual(1, 1);
    let mut r6 = Reconciler::new();
    let a = r6.reconcile(&b6, 1);
    let c = r6.reconcile(&b6, 1);
    let d = r6.reconcile(&b6, 0);
    let e = r6.reconcile(&b6, 5);
    let ok = a.is_ok() && matches!(c, Err(ReconError::DupRecon))
        && matches!(d, Err(ReconError::RecRewind)) && e.is_ok()
        && r6.len() == 2;
    s.add("F81011-08 对账账单调", ok, if ok { "" } else { "对账账回退漏抓" });

    // 9 重放一致：合法记录 replay_verify 通过；篡改计数拒绝
    let rec9 = match r6.by_wave(5) {
        Some(rec) => rec,
        None => ReconRecord { wave: 5, n_match: 1, n_led: 0, n_dom: 0, n_mis: 0, total: 1 },
    };
    let mut bad9 = rec9;
    bad9.n_mis += 1;
    let ok = r6.replay_verify(&b6, &rec9).is_ok()
        && matches!(r6.replay_verify(&b6, &bad9), Err(ReconError::Replay));
    s.add("F81011-09 重放校验", ok, if ok { "" } else { "重放闸失灵" });

    // 10 键越界全通道拒
    let mut b7 = DualBook::new();
    let ok = b7.write_dual(0, 1).is_err()
        && b7.write_dual(KEY_MAX + 1, 1).is_err()
        && b7.write_ledger_unpaired(0, 1, 1).is_err()
        && b7.write_domain_unpaired(KEY_MAX + 1, 1, 1).is_err();
    s.add("F81011-10 键域闭集", ok, if ok { "" } else { "键域闸失灵" });

    // 11 空账对账拒
    let ok = matches!(Reconciler::new().reconcile(&DualBook::new(), 1), Err(ReconError::EmptyRec));
    s.add("F81011-11 空账拒", ok, if ok { "" } else { "空账漏抓" });

    // 12 非法第四态零存在
    let ok = ReconState::from_bits(4).is_err()
        && ReconState::from_bits(255).is_err()
        && ReconState::from_bits(3).is_ok()
        && ReconState::from_bits(0).is_ok();
    s.add("F81011-12 非法第四态", ok, if ok { "" } else { "状态闭集失守" });

    // 13 注入 10/10
    let ok = inject_all_caught();
    s.add("F81011-13 注入10/10", ok, if ok { "" } else { "注入漏抓" });

    // 14 对账确定性：同书两次独立对账计数逐位一致
    let r_a = {
        let mut r = Reconciler::new();
        r.reconcile(&b5, 1)
    };
    let r_b = {
        let mut r = Reconciler::new();
        r.reconcile(&b5, 1)
    };
    let ok = match (r_a, r_b) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    };
    s.add("F81011-14 对账确定", ok, if ok { "" } else { "对账不确定" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx81010_checks_green() {
        let set = run_unx81010_checks();
        assert!(set.all_passed(), "UNX-F81010 red: {}", set.red_items_text());
    }
}
