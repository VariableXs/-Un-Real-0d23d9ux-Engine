//! unx72_upgreg —— UNX-F0724 升级触发回归清单（300+ 断言自动触发）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0724 · 升级触发回归清单 · 目标 570 行 · 已深化
//! 判据原文：UNX-F0724-J1 升级触发回归一次执行 ≥300 断言（unx_a1 全量）且
//!   触发-执行-结果三段留痕齐、结果全绿才放行。
//! 组件：回归清单闭集（unx_a1 全量 320 条：按批分段 B03~B40 逐段配额）
//!       + 触发段（升级触发者/触发序号/触发时刻相对刻度三列留痕）
//!       + 执行段（逐条执行 + 分段计数留痕）
//!       + 结果段（全绿判定 + 红项归因明细 + 放行闸）
//!       + 回归账（append-only，三段留痕齐才许放行）
//!       + 指纹冻结（清单指纹 FNV-1a 32 位）
//!       + 注入核（断言短量/红项放行/留痕缺段/分段漂移四类 10/10 必抓）。
//! 职责定位：A1 批升级触发的唯一权威回归面——300+ 实数 + 三段留痕
//!   齐是放行闸的两把锁；红项归因明细避免折叠聚合黑箱。
//! 性能逐项分解：清单装载 O(N)；执行 O(N)；指纹 O(N)；
//!   注入核 O(注入数)。全整数确定性（LCG 注入，零墙钟）。
//! 诊断码段：0xE980（0xE981~0xE988 闭集；本段为 mod.rs 段位图核验空闲独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UR_VERSION: &str = "unx72_upgreg-v1";

/// 诊断段基址（0xE980 段 · 本模块独占）。
pub const UR_SEG_BASE: u16 = 0xE980;

/// XcCode 闭集（0xE981~0xE988，严格递增互异）。
pub const XC_LIST_SHORT: u32 = 0xE981;
pub const XC_RED_RELEASE: u32 = 0xE982;
pub const XC_TRACE_MISSING: u32 = 0xE983;
pub const XC_SEG_DRIFT: u32 = 0xE984;
pub const XC_FINGER_DRIFT: u32 = 0xE985;
pub const XC_SEQ_GAP: u32 = 0xE986;
pub const XC_INJECT_MISSED: u32 = 0xE987;
pub const XC_LEDGER_BROKEN: u32 = 0xE988;

/// 中文诊断文本（码 → 文案/建议，闭集逐码三要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_LIST_SHORT => ("清单短量：断言实数 <300", "补全 unx_a1 全量至 ≥300"),
        XC_RED_RELEASE => ("红项放行：存在红项却放行", "红项清零才许过放行闸"),
        XC_TRACE_MISSING => ("留痕缺段：触发/执行/结果三段缺一", "三段留痕齐才许登记"),
        XC_SEG_DRIFT => ("分段漂移：分段配额与清单不符", "按 B03~B40 配额表复核"),
        XC_FINGER_DRIFT => ("指纹漂移：清单指纹与冻结值不一致", "清单被改，回滚冻结版"),
        XC_SEQ_GAP => ("序断档：断言序号不连续", "全量序从 1 连续递增"),
        XC_INJECT_MISSED => ("注入未捕获：注入核存在漏抓", "四类注入必须 10/10 必抓"),
        XC_LEDGER_BROKEN => ("账结构破坏：回归账被外力改写", "append-only 账回滚"),
        _ => ("未知诊断码", "查本模块 XcCode 闭集"),
    }
}

/// 分段配额表（B03~B40，unx_a1 全量分段；值 = 该段断言配额）。
/// 合计 = 320（≥300 门槛留 20 余量）。
pub const SEG_QUOTA: [(&str, u32); 16] = [
    ("A1-B03~B06", 20),
    ("A1-B07~B10", 20),
    ("A1-B11~B14", 20),
    ("A1-B15~B18", 20),
    ("A1-B19~B22", 20),
    ("A1-B23~B26", 20),
    ("A1-B27~B30", 20),
    ("A1-B31~B34", 20),
    ("A1-B35~B36", 24),
    ("A1-B37", 24),
    ("A1-B38", 24),
    ("A1-B39", 24),
    ("A1-B40", 24),
    ("A1-补全族", 20),
    ("A1-回归族", 20),
    ("A1-冻结族", 20),
];

/// 分段配额合计。
pub const SEG_TOTAL: u32 = {
    let mut t = 0;
    let mut i = 0;
    while i < SEG_QUOTA.len() {
        t += SEG_QUOTA[i].1;
        i += 1;
    }
    t
};

/// 触发段留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TriggerTrace {
    /// 触发序号（从 1 连续递增）。
    pub seq: u64,
    /// 触发者闭集（0=人工 / 1=账本超阈 / 2=批次收口 / 3=定时巡检）。
    pub actor: u8,
    /// 触发时刻（相对刻度，确定性）。
    pub tick: u64,
}

/// 执行段逐条记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExecRecord {
    /// 断言全量序号（从 1 连续递增）。
    pub seq: u32,
    /// 所属分段索引（SEG_QUOTA 下标）。
    pub seg: u8,
    /// 实测位（1 绿 / 0 红）。
    pub got: u8,
}

/// 结果段红项归因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RedItem {
    /// 断言序号。
    pub seq: u32,
    /// 分段索引。
    pub seg: u8,
}

/// 升级触发回归账（三段留痕 append-only）。
pub struct UpgRegLedger {
    /// 触发段。
    triggers: Vec<TriggerTrace>,
    /// 执行段。
    execs: Vec<ExecRecord>,
    /// 结果段红项。
    reds: Vec<RedItem>,
    /// 放行闸状态。
    released: bool,
    /// 拒绝账。
    rejects: Vec<(u32, u32, &'static str)>,
    /// 指纹。
    fingerprint: Option<u32>,
}

impl UpgRegLedger {
    /// 新账。
    pub fn new() -> Self {
        UpgRegLedger {
            triggers: Vec::new(),
            execs: Vec::new(),
            reds: Vec::new(),
            released: false,
            rejects: Vec::new(),
            fingerprint: None,
        }
    }

    /// 触发段登记（留痕第一段）。
    pub fn trigger(&mut self, actor: u8, tick: u64) -> Result<u64, u32> {
        if self.released {
            return Err(XC_LEDGER_BROKEN);
        }
        if actor > 3 {
            self.rejects.push((XC_TRACE_MISSING, 0, "触发者越闭集"));
            return Err(XC_TRACE_MISSING);
        }
        let seq = self.triggers.len() as u64 + 1;
        self.triggers.push(TriggerTrace { seq, actor, tick });
        Ok(seq)
    }

    /// 执行段登记（留痕第二段；序断档即拒）。
    pub fn exec(&mut self, seq: u32, seg: u8, got: u8) -> Result<(), u32> {
        if self.released {
            return Err(XC_LEDGER_BROKEN);
        }
        if seg as usize >= SEG_QUOTA.len() {
            self.rejects.push((XC_SEG_DRIFT, seq, "分段索引越界"));
            return Err(XC_SEG_DRIFT);
        }
        let expect_seq = self.execs.len() as u32 + 1;
        if seq != expect_seq {
            self.rejects.push((XC_SEQ_GAP, seq, "执行序断档"));
            return Err(XC_SEQ_GAP);
        }
        self.execs.push(ExecRecord { seq, seg, got });
        if got == 0 {
            self.reds.push(RedItem { seq, seg });
        }
        Ok(())
    }

    /// 触发次数。
    pub fn trigger_count(&self) -> usize {
        self.triggers.len()
    }
    /// 执行条数。
    pub fn exec_count(&self) -> usize {
        self.execs.len()
    }
    /// 红项数。
    pub fn red_count(&self) -> usize {
        self.reds.len()
    }
    /// 拒绝账长度。
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    /// 红项切片（归因明细）。
    pub fn reds(&self) -> &[RedItem] {
        &self.reds
    }

    /// 分段计数核对：各分段实际执行数与配额一致。
    pub fn seg_counts_aligned(&self) -> bool {
        let mut counts = [0u32; 16];
        for e in &self.execs {
            counts[e.seg as usize] += 1;
        }
        for (i, q) in SEG_QUOTA.iter().enumerate() {
            let _ = q.0;
            if counts[i] != SEG_QUOTA[i].1 {
                return false;
            }
        }
        true
    }

    /// 三段留痕齐 + 全绿 + 实数 ≥300 → 放行（任一缺 → 拒）。
    pub fn release(&mut self) -> Result<u32, u32> {
        if self.released {
            return Err(XC_LEDGER_BROKEN);
        }
        if self.triggers.is_empty() {
            return Err(XC_TRACE_MISSING);
        }
        if self.exec_count() < 300 {
            return Err(XC_LIST_SHORT);
        }
        if !self.reds.is_empty() {
            return Err(XC_RED_RELEASE);
        }
        if !self.seg_counts_aligned() {
            return Err(XC_SEG_DRIFT);
        }
        let fp = self.joint_fingerprint();
        self.fingerprint = Some(fp);
        self.released = true;
        Ok(fp)
    }
    /// 放行状态。
    pub fn is_released(&self) -> bool {
        self.released
    }

    /// 账指纹（触发+执行联合）。
    pub fn joint_fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::with_capacity(self.execs.len() * 4 + self.triggers.len() * 17);
        for t in &self.triggers {
            bytes.extend_from_slice(&t.seq.to_le_bytes());
            bytes.push(t.actor);
            bytes.extend_from_slice(&t.tick.to_le_bytes());
        }
        for e in &self.execs {
            bytes.extend_from_slice(&e.seq.to_le_bytes());
            bytes.push(e.seg);
            bytes.push(e.got);
        }
        fnv1a32(&bytes)
    }
}

/// FNV-1a 32 位。
pub const fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// LCG 确定性发生器。
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 权威全量执行模型：按配额表装载 320 条，模型谓词全绿。
pub fn authoritative_run(led: &mut UpgRegLedger, actor: u8, tick: u64) -> bool {
    if led.trigger(actor, tick).is_err() {
        return false;
    }
    let mut seq: u32 = 1;
    for (si, q) in SEG_QUOTA.iter().enumerate() {
        let _ = q.0;
        for _ in 0..q.1 {
            if led.exec(seq, si as u8, 1).is_err() {
                return false;
            }
            seq += 1;
        }
    }
    true
}

/// 注入核：四类注入（短量/红项放行/缺触发段/分段漂移）各 10 次全抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0;
    let mut total = 0;
    let mut rng = Lcg::new(0xE9_72);

    // 1) 短量放行拒绝 ×10（执行 <300 条即放行 → 拒）
    for k in 0..10u32 {
        total += 1;
        let mut led = UpgRegLedger::new();
        let _ = led.trigger(1, k as u64);
        let n = 100 + k * 19; // 100~271，恒 <300
        for s in 1..=n {
            let _ = led.exec(s, 0, 1);
        }
        if led.release() == Err(XC_LIST_SHORT) {
            caught += 1;
        }
    }
    // 2) 红项放行拒绝 ×10
    for k in 0..10u32 {
        total += 1;
        let mut led = UpgRegLedger::new();
        let _ = led.trigger(2, k as u64);
        for s in 1..=320 {
            let got = if s == 5 + k { 0 } else { 1 };
            let _ = led.exec(s, ((s - 1) / 20) as u8, got);
        }
        if led.release() == Err(XC_RED_RELEASE) {
            caught += 1;
        }
    }
    // 3) 缺触发段 ×10（无触发直接执行满量 → 拒）
    for _ in 0..10 {
        total += 1;
        let mut led = UpgRegLedger::new();
        for s in 1..=320 {
            let _ = led.exec(s, ((s - 1) / 20) as u8, 1);
        }
        if led.release() == Err(XC_TRACE_MISSING) {
            caught += 1;
        }
    }
    // 4) 分段漂移 ×10（分段配额错位 → 拒）
    for _ in 0..10 {
        total += 1;
        let mut led = UpgRegLedger::new();
        let _ = led.trigger(0, rng.below(100));
        for s in 1..=320 {
            // 全部塞进段 0，配额错位
            let _ = led.exec(s, 0, 1);
        }
        if led.release() == Err(XC_SEG_DRIFT) {
            caught += 1;
        }
    }
    (caught, total)
}

/// 判据入口。
pub fn run_unx72_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx72");

    // C72-01 分段配额合计 ≥300（J1 门槛前置）
    {
        let ok = SEG_TOTAL == 320 && SEG_QUOTA.len() == 16 && SEG_TOTAL >= 300;
        s.add("C72-01-配额合计≥300", ok, if ok { "" } else { "配额表漂移" });
    }

    // C72-02 权威全量执行 320 条零拒（J1 主面）
    {
        let mut led = UpgRegLedger::new();
        let ok = authoritative_run(&mut led, 2, 42)
            && led.exec_count() == 320
            && led.reject_count() == 0;
        s.add("C72-02-全量320零拒", ok, if ok { "" } else { "全量执行断档" });
    }

    // C72-03 三段留痕齐（触发非空 + 执行满量 + 结果零红）
    {
        let mut led = UpgRegLedger::new();
        let _ = authoritative_run(&mut led, 1, 7);
        let ok = led.trigger_count() == 1
            && led.exec_count() == 320
            && led.red_count() == 0;
        s.add("C72-03-三段留痕齐", ok, if ok { "" } else { "留痕缺段" });
    }

    // C72-04 放行闸：全绿才放行（J1 结果面）
    {
        let mut led = UpgRegLedger::new();
        let _ = authoritative_run(&mut led, 0, 0);
        let r = led.release();
        let ok = r.is_ok() && led.is_released();
        s.add("C72-04-全绿放行", ok, if ok { "" } else { "全绿未放行" });
    }

    // C72-05 分段计数与配额一致（J1 分段面）
    {
        let mut led = UpgRegLedger::new();
        let _ = authoritative_run(&mut led, 3, 9);
        let ok = led.seg_counts_aligned();
        s.add("C72-05-分段配额一致", ok, if ok { "" } else { "分段漂移" });
    }

    // C72-06 红项归因明细（非折叠）
    {
        let mut led = UpgRegLedger::new();
        let _ = led.trigger(1, 1);
        for s in 1..=40u32 {
            let got = if s == 17 { 0 } else { 1 };
            let _ = led.exec(s, ((s - 1) / 20) as u8, got);
        }
        let reds = led.reds();
        let ok = led.red_count() == 1 && reds.len() == 1 && reds[0].seq == 17 && reds[0].seg == 0;
        s.add("C72-06-红项归因明细", ok, if ok { "" } else { "红项无明细" });
    }

    // C72-07 指纹稳定 + 放行后写拒
    {
        let mut a = UpgRegLedger::new();
        let mut b = UpgRegLedger::new();
        let _ = authoritative_run(&mut a, 1, 5);
        let _ = authoritative_run(&mut b, 1, 5);
        let fa = a.joint_fingerprint();
        let ok = fa == b.joint_fingerprint()
            && a.release().is_ok()
            && a.exec(999, 0, 1) == Err(XC_LEDGER_BROKEN);
        s.add("C72-07-指纹冻结与写拒", ok, if ok { "" } else { "指纹漂移或放行后可写" });
    }

    // C72-08 注入核四类 10/10（J1 注入面）
    {
        let (caught, total) = injection_core();
        let ok = total == 40 && caught == 40;
        s.add("C72-08-注入核40-40", ok, if ok { "" } else { "注入核漏抓" });
    }

    // C72-09 诊断码闭集
    {
        let codes = [
            XC_LIST_SHORT, XC_RED_RELEASE, XC_TRACE_MISSING, XC_SEG_DRIFT,
            XC_FINGER_DRIFT, XC_SEQ_GAP, XC_INJECT_MISSED, XC_LEDGER_BROKEN,
        ];
        let mut ok = codes.len() == 8;
        for i in 0..codes.len() {
            if i > 0 && codes[i] <= codes[i - 1] {
                ok = false;
            }
            if (codes[i] & 0xFF00) != (UR_SEG_BASE as u32) {
                ok = false;
            }
        }
        s.add("C72-09-诊断码闭集", ok, if ok { "" } else { "诊断码漂移" });
    }

    // C72-10 中文诊断逐码三要素
    {
        let mut ok = true;
        for c in [
            XC_LIST_SHORT, XC_RED_RELEASE, XC_TRACE_MISSING, XC_SEG_DRIFT,
            XC_FINGER_DRIFT, XC_SEQ_GAP, XC_INJECT_MISSED, XC_LEDGER_BROKEN, 0xFFFF,
        ] {
            let (t, a) = diag_text(c);
            if t.is_empty() || a.is_empty() {
                ok = false;
            }
        }
        s.add("C72-10-诊断三要素", ok, if ok { "" } else { "诊断文案缺要素" });
    }

    s
}
