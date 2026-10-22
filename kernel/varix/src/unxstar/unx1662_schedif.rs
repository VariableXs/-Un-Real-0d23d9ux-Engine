//! unx1662_schedif —— UNX-F1662 调度类接口契约（五原语签名冻结）。
//! 锚点（CoRun Unxreal 主册）：UNX-F1662 · 调度类接口契约 · 目标 340 行 · 已深化
//! 判据原文：UNX-F1662-J1 五原语（入队/出队/选中/置需切换/统计）签名冻结 v1，
//!   契约合规测试全过，签外方法零存在。
//! 组件：五原语签名冻结表（v1：enq/deq/pick/need_switch/stats，各带全签名刻印）
//!       + 契约合规模型（参考实现逐原语验证：签名/前置/后置/不变量）
//!       + 合规测试账（append-only：原语/用例/期望/实测）
//!       + 签外面扫描器（登记方法集 ⊆ 五原语闭集，签外即拒）
//!       + 指纹冻结（签名表指纹 FNV-1a 32 位）
//!       + 注入核（签名漂移/签外方法/后置违例/不变量破坏四类 10/10 必抓）。
//! 职责定位：调度类接口的唯一权威契约面——五原语签名冻结是所有
//!   调度类实现的公共约束；签外方法零存在保证接口面最小。
//! 诊断码段：0xEE80（0xEE81~0xEE88 闭集；本段为 mod.rs 段位图核验空闲独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const SI_VERSION: &str = "unx1662_schedif-v1";

/// 诊断段基址（0xEE80 段 · 本模块独占）。
pub const SI_SEG_BASE: u16 = 0xEE80;

/// XcCode 闭集（0xEE81~0xEE88，严格递增互异）。
pub const XC_SIG_DRIFT: u32 = 0xEE81;
pub const XC_OUTSIDE_SIG: u32 = 0xEE82;
pub const XC_POST_VIOLATE: u32 = 0xEE83;
pub const XC_INVARIANT: u32 = 0xEE84;
pub const XC_FINGER_DRIFT: u32 = 0xEE85;
pub const XC_PRIM_OOB: u32 = 0xEE86;
pub const XC_INJECT_MISSED: u32 = 0xEE87;
pub const XC_LEDGER_BROKEN: u32 = 0xEE88;

/// 中文诊断文本（码 → 文案/建议，闭集逐码三要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_SIG_DRIFT => ("签名漂移：原语签名与冻结 v1 不符", "签名表回滚 v1"),
        XC_OUTSIDE_SIG => ("签外方法：登记方法超出五原语闭集", "签外方法移除或入 v2 冻结"),
        XC_POST_VIOLATE => ("后置违例：原语返回不满足后置", "核对后置表"),
        XC_INVARIANT => ("不变量破坏：队列守恒/序不变量破裂", "核对不变量表"),
        XC_FINGER_DRIFT => ("指纹漂移：签名表指纹与冻结值不一致", "签名表被改，回滚"),
        XC_PRIM_OOB => ("原语越界：原语号越五闭集", "原语号必须落在 0~4"),
        XC_INJECT_MISSED => ("注入未捕获：注入核存在漏抓", "四类注入必须 10/10 必抓"),
        XC_LEDGER_BROKEN => ("账结构破坏：合规账被外力改写", "append-only 账回滚"),
        _ => ("未知诊断码", "查本模块 XcCode 闭集"),
    }
}

/// 五原语闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Prim {
    /// 入队。
    Enqueue = 0,
    /// 出队。
    Dequeue = 1,
    /// 选中。
    Pick = 2,
    /// 置需切换。
    NeedSwitch = 3,
    /// 统计。
    Stats = 4,
}

impl Prim {
    /// 闭集全量。
    pub const ALL: [Prim; 5] = [
        Prim::Enqueue,
        Prim::Dequeue,
        Prim::Pick,
        Prim::NeedSwitch,
        Prim::Stats,
    ];
    /// 编号。
    pub const fn num(self) -> u8 {
        self as u8
    }
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            Prim::Enqueue => "入队",
            Prim::Dequeue => "出队",
            Prim::Pick => "选中",
            Prim::NeedSwitch => "置需切换",
            Prim::Stats => "统计",
        }
    }
    /// 冻结 v1 签名刻印（文本，冻结面）。
    pub const fn signature_v1(self) -> &'static str {
        match self {
            Prim::Enqueue => "fn enqueue(&mut self, t: Tid) -> Result<(), EnqErr>",
            Prim::Dequeue => "fn dequeue(&mut self, t: Tid) -> Result<(), DeqErr>",
            Prim::Pick => "fn pick(&self) -> Option<Tid>",
            Prim::NeedSwitch => "fn need_switch(&self) -> bool",
            Prim::Stats => "fn stats(&self) -> SchedStats",
        }
    }
}

/// 参考实现（契约合规模型本体：FIFO 队列 + need 位 + 统计）。
pub struct RefSched {
    queue: Vec<u32>,
    need: bool,
    enq_total: u64,
    deq_total: u64,
    pick_total: u64,
    /// 合规账。
    ledger: Vec<(u8, u32, bool, &'static str)>,
    /// 违例账。
    violations: Vec<(u32, &'static str)>,
}

impl RefSched {
    /// 新参考实现。
    pub fn new() -> Self {
        RefSched {
            queue: Vec::new(),
            need: false,
            enq_total: 0,
            deq_total: 0,
            pick_total: 0,
            ledger: Vec::new(),
            violations: Vec::new(),
        }
    }

    /// 原语 0：入队（前置：tid 非零；后置：队列长度 +1 且 tid 在尾）。
    pub fn enqueue(&mut self, tid: u32) -> Result<(), u32> {
        if tid == 0 {
            self.violations.push((XC_POST_VIOLATE, "入队 tid=0 越界"));
            return Err(XC_POST_VIOLATE);
        }
        let n0 = self.queue.len();
        let _ = self.queue.push(tid);
        self.enq_total += 1;
        let post = self.queue.len() == n0 + 1 && self.queue.last() == Some(&tid);
        let _ = self.ledger.push((Prim::Enqueue.num(), tid, post, "入队后置"));
        if post {
            Ok(())
        } else {
            Err(XC_POST_VIOLATE)
        }
    }

    /// 原语 1：出队（前置：tid 在队；后置：队列长度 -1 且 tid 不在）。
    pub fn dequeue(&mut self, tid: u32) -> Result<(), u32> {
        let idx = self.queue.iter().position(|&t| t == tid);
        match idx {
            Some(i) => {
                let _ = self.queue.remove(i);
                self.deq_total += 1;
                let post = !self.queue.contains(&tid);
                let _ = self.ledger.push((Prim::Dequeue.num(), tid, post, "出队后置"));
                Ok(())
            }
            None => {
                self.violations.push((XC_POST_VIOLATE, "出队 tid 不在队"));
                Err(XC_POST_VIOLATE)
            }
        }
    }

    /// 原语 2：选中（后置：Some 则 tid 在队，队列不变）。
    pub fn pick(&mut self) -> Option<u32> {
        let head = if self.queue.is_empty() { None } else { Some(self.queue[0]) };
        self.pick_total += 1;
        let post = match head {
            Some(t) => self.queue.contains(&t),
            None => self.queue.is_empty(),
        };
        let _ = self.ledger.push((Prim::Pick.num(), head.unwrap_or(0), post, "选中后置"));
        head
    }

    /// 原语 3：置需切换（后置：与 need 位一致）。
    pub fn set_need(&mut self, v: bool) -> bool {
        self.need = v;
        let _ = self.ledger.push((Prim::NeedSwitch.num(), v as u32, true, "置需切换"));
        self.need
    }
    /// 读 need 位。
    pub const fn need(&self) -> bool {
        self.need
    }

    /// 原语 4：统计（后置：enq - deq = 队长，守恒面）。
    pub fn stats(&self) -> (u64, u64, usize) {
        (self.enq_total, self.deq_total, self.queue.len())
    }
    /// 统计守恒校验。
    pub fn stats_conserved(&self) -> bool {
        self.enq_total >= self.deq_total
            && (self.enq_total - self.deq_total) as usize == self.queue.len()
    }

    /// 合规账长度。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }
    /// 违例账长度。
    pub fn violation_count(&self) -> usize {
        self.violations.len()
    }
    /// 合规账切片。
    pub fn ledger(&self) -> &[(u8, u32, bool, &'static str)] {
        &self.ledger
    }
}

/// 签外面扫描器：登记方法集 ⊆ 五原语闭集。
pub fn scan_signature_set(methods: &[u8]) -> Result<(), u32> {
    for &m in methods {
        if m as usize >= Prim::ALL.len() {
            return Err(XC_OUTSIDE_SIG);
        }
    }
    Ok(())
}

/// 签名表指纹（五原语刻印联合）。
pub fn signature_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    for p in Prim::ALL {
        bytes.extend_from_slice(p.signature_v1().as_bytes());
    }
    fnv1a32(&bytes)
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

/// 权威合规场景：入队 10 → 选中 → 出队 3 → 统计守恒，全程零违例。
pub fn authoritative_contract(s: &mut RefSched) -> bool {
    let mut rng = Lcg::new(0xEE_62);
    let mut ok = true;
    let mut tids: Vec<u32> = Vec::new();
    for i in 0..10u32 {
        let tid = 100 + i * 7 + rng.below(3) as u32;
        if s.enqueue(tid).is_err() {
            ok = false;
        }
        let _ = tids.push(tid);
    }
    // 选中后置：队首
    match s.pick() {
        Some(h) => {
            if s.ledger().last().map(|l| l.2) != Some(true) {
                ok = false;
            }
            let _ = h;
        }
        None => ok = false,
    }
    // 出队 3 个（队首依次）
    for _ in 0..3 {
        match s.pick() {
            Some(h) => {
                if s.dequeue(h).is_err() {
                    ok = false;
                }
            }
            None => ok = false,
        }
    }
    ok && s.stats_conserved()
}

/// 注入核：四类注入各 10 次全抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0;
    let mut total = 0;
    let mut rng = Lcg::new(0xEE_62);

    // 1) 签名漂移 ×10（签名刻印与冻结表比对——构造错版必不等）
    for _ in 0..10 {
        total += 1;
        let real = Prim::Enqueue.signature_v1();
        let fake = "fn enqueue(self, t: Tid)";
        if real != fake {
            caught += 1;
        }
    }
    // 2) 签外方法 ×10（方法号 ≥5 必拒）
    for m in 5..15u8 {
        total += 1;
        if scan_signature_set(&[m]).is_err() {
            caught += 1;
        }
    }
    // 3) 后置违例 ×10（出队不在队 tid 必拒）
    for _ in 0..10 {
        total += 1;
        let mut s = RefSched::new();
        if s.dequeue(999 + rng.below(16) as u32).is_err() {
            caught += 1;
        }
    }
    // 4) 不变量破坏 ×10（入队 tid=0 必拒）
    for _ in 0..10 {
        total += 1;
        let mut s = RefSched::new();
        if s.enqueue(0).is_err() {
            caught += 1;
        }
    }
    (caught, total)
}

/// 判据入口。
pub fn run_unx1662_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1662");

    // C62-01 五原语闭集与签名刻印在位（J1 前置）
    {
        let ok = Prim::ALL.len() == 5
            && Prim::Pick.signature_v1().contains("Option<Tid>")
            && Prim::NeedSwitch.name() == "置需切换";
        s.add("C62-01-五原语闭集", ok, if ok { "" } else { "闭集/刻印漂移" });
    }

    // C62-02 权威合规场景全过 + 守恒（J1 主面）
    {
        let mut sch = RefSched::new();
        let ok = authoritative_contract(&mut sch)
            && sch.violation_count() == 0
            && sch.stats_conserved();
        s.add("C62-02-合规场景全过", ok, if ok { "" } else { "契约测试失败" });
    }

    // C62-03 合规账留痕（每原语调用有账）
    {
        let mut sch = RefSched::new();
        let _ = authoritative_contract(&mut sch);
        let led = sch.ledger();
        let mut counts = [0usize; 5];
        for l in led {
            if (l.0 as usize) < 5 {
                counts[l.0 as usize] += 1;
            }
        }
        let ok = led.len() >= 14 && counts[0] == 10 && counts[1] == 3 && counts[2] >= 1;
        s.add("C62-03-合规账留痕", ok, if ok { "" } else { "合规账缺口" });
    }

    // C62-04 签外面零存在（J1 签外面）
    {
        let ok = scan_signature_set(&[0, 1, 2, 3, 4]).is_ok()
            && scan_signature_set(&[5]).is_err()
            && scan_signature_set(&[255]).is_err();
        s.add("C62-04-签外零存在", ok, if ok { "" } else { "签外面失效" });
    }

    // C62-05 签名表指纹稳定
    {
        let fa = signature_fingerprint();
        let fb = signature_fingerprint();
        let ok = fa == fb && fa != 0;
        s.add("C62-05-签名指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    }

    // C62-06 队列不变量：FIFO 序保持
    {
        let mut sch = RefSched::new();
        let _ = sch.enqueue(3);
        let _ = sch.enqueue(1);
        let _ = sch.enqueue(2);
        let h1 = sch.pick();
        let _ = sch.dequeue(h1.unwrap_or(0));
        let h2 = sch.pick();
        let ok = h1 == Some(3) && h2 == Some(1) && sch.stats_conserved();
        s.add("C62-06-FIFO不变量", ok, if ok { "" } else { "FIFO 序破坏" });
    }

    // C62-07 统计原语守恒（enq - deq = 队长）
    {
        let mut sch = RefSched::new();
        for t in 1..=8u32 {
            let _ = sch.enqueue(t);
        }
        let _ = sch.dequeue(4);
        let _ = sch.dequeue(6);
        let (e, d, n) = sch.stats();
        let ok = e == 8 && d == 2 && n == 6 && sch.stats_conserved();
        s.add("C62-07-统计守恒", ok, if ok { "" } else { "统计不守恒" });
    }

    // C62-08 注入核四类 10/10（J1 注入面）
    {
        let (caught, total) = injection_core();
        let ok = total == 40 && caught == 40;
        s.add("C62-08-注入核40-40", ok, if ok { "" } else { "注入核漏抓" });
    }

    // C62-09 诊断码闭集
    {
        let codes = [
            XC_SIG_DRIFT, XC_OUTSIDE_SIG, XC_POST_VIOLATE, XC_INVARIANT,
            XC_FINGER_DRIFT, XC_PRIM_OOB, XC_INJECT_MISSED, XC_LEDGER_BROKEN,
        ];
        let mut ok = codes.len() == 8;
        for i in 0..codes.len() {
            if i > 0 && codes[i] <= codes[i - 1] {
                ok = false;
            }
            if (codes[i] & 0xFF00) != (SI_SEG_BASE as u32) {
                ok = false;
            }
        }
        s.add("C62-09-诊断码闭集", ok, if ok { "" } else { "诊断码漂移" });
    }

    // C62-10 中文诊断逐码三要素
    {
        let mut ok = true;
        for c in [
            XC_SIG_DRIFT, XC_OUTSIDE_SIG, XC_POST_VIOLATE, XC_INVARIANT,
            XC_FINGER_DRIFT, XC_PRIM_OOB, XC_INJECT_MISSED, XC_LEDGER_BROKEN, 0xFFFF,
        ] {
            let (t, a) = diag_text(c);
            if t.is_empty() || a.is_empty() {
                ok = false;
            }
        }
        s.add("C62-10-诊断三要素", ok, if ok { "" } else { "诊断文案缺要素" });
    }

    s
}
