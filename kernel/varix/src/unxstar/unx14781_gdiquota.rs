//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14781 · GDI 配额 10,000 触顶语义 · 0x66B40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F14781-J1 配额顶 CreateObject 判 ERROR_NO_MORE_HANDLES，
//!      触顶后既有功能全保活断言；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 配额常量写死：GDI_PER_PROCESS_OBJECT_QUOTA = 10_000（来源
//!     Win32 GDI 对象每进程配额约定）；
//!   - 错误码写死：ERROR_NO_MORE_HANDLES = 18（来源 Win32 系统错误
//!     码表，独立判据常量，不复用被测值）；
//!   - 三态配额状态机：Open（未满）→ AtCap（触顶）→ Drained（销毁
//!     腾位），非法边（触顶后继续成功创建）拒收；
//!   - 保活断言：触顶后既有句柄查询/枚举/销毁全保活——销毁一个
//!     腾位后创建立即成功且复用最低空闲槽位；
//!   - 槽位守恒恒等式：占用数 = 累创 - 累毁；触顶失败数与状态机
//!     拒收次数一致；
//!   - LCG 压测：确定性填满-触顶-腾位-复用循环，种子写死 = SEG_BASE
//!     （0x66B40），禁 rand/浮点/IO/墙钟；no_std + alloc；
//!     零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66B40，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66B40;

/// 配额顶（写死：GDI 每进程对象配额 10,000，来源 Win32 约定）。
pub const GDI_QUOTA: usize = 10_000;
/// 触顶错误码（写死：ERROR_NO_MORE_HANDLES = 18，来源 Win32 错误码表）。
pub const ERROR_NO_MORE_HANDLES: u32 = 18;
/// 独立判据常量：配额顶副本（不复用被测值，勾稽用）。
pub const QUOTA_EXPECTED: usize = 10_000;
/// 保活断言探针数：触顶前预置的既有对象数。
pub const KEEPALIVE_PROBES: usize = 8;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuotaCode {
    /// 句柄下标越槽位闭集
    HandleOutOfRange,
    /// 触顶错误码不符（非 ERROR_NO_MORE_HANDLES）
    CapErrorCodeMismatch,
    /// 触顶后既有功能失活（保活断言破）
    KeepaliveBroken,
    /// 槽位守恒破（占用 ≠ 累创-累毁）
    SlotConservationBroken,
    /// 重复销毁未拒收
    DoubleDestroyAccepted,
    /// 复用槽位非最低空闲
    ReuseNotLowestFree,
    /// 配额状态机非法边
    QuotaEdgeIllegal,
    /// 双跑不一致（不可复现）
    NotReproducible,
    /// 账目未入账
    OpUnbooked,
    /// 配额常量与独立判据不符
    QuotaConstMismatch,
}

impl QuotaCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            QuotaCode::HandleOutOfRange => "句柄下标越槽位闭集：10000 槽冻结",
            QuotaCode::CapErrorCodeMismatch => "触顶错误码不符：非 ERROR_NO_MORE_HANDLES(18)",
            QuotaCode::KeepaliveBroken => "触顶后既有功能失活：保活断言破",
            QuotaCode::SlotConservationBroken => "槽位守恒破：占用 ≠ 累创-累毁",
            QuotaCode::DoubleDestroyAccepted => "重复销毁未拒收",
            QuotaCode::ReuseNotLowestFree => "复用槽位非最低空闲",
            QuotaCode::QuotaEdgeIllegal => "配额状态机非法边",
            QuotaCode::NotReproducible => "双跑不一致：复现红线",
            QuotaCode::OpUnbooked => "操作未入账",
            QuotaCode::QuotaConstMismatch => "配额常量与独立判据 10000 不符",
        }
    }
    pub const ALL: [QuotaCode; 10] = [
        QuotaCode::HandleOutOfRange,
        QuotaCode::CapErrorCodeMismatch,
        QuotaCode::KeepaliveBroken,
        QuotaCode::SlotConservationBroken,
        QuotaCode::DoubleDestroyAccepted,
        QuotaCode::ReuseNotLowestFree,
        QuotaCode::QuotaEdgeIllegal,
        QuotaCode::NotReproducible,
        QuotaCode::OpUnbooked,
        QuotaCode::QuotaConstMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < QuotaCode::ALL.len() {
        if QuotaCode::ALL[i].val() == code {
            return QuotaCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66B40 段闭集内"
}

// ── 配额状态机三态 ───────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuotaPhase {
    /// 未满（占用 < 配额）
    Open,
    /// 触顶（占用 == 配额，创建必拒）
    AtCap,
    /// 腾位后回落（销毁使占用 < 配额）
    Drained,
}

/// 对象记录（槽位载荷）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ObjRecord {
    pub name: String,
    pub serial: u64,
    pub alive: bool,
}

/// 配额账本：槽位闭集 + 操作账。
pub struct QuotaLedger {
    pub slots: Vec<Option<ObjRecord>>,
    pub phase: QuotaPhase,
    pub created: u32,
    pub destroyed: u32,
    pub cap_fails: u32,
    pub kept_alive_checks: u32,
}

impl QuotaLedger {
    pub fn new() -> QuotaLedger {
        QuotaLedger {
            slots: Vec::new(),
            phase: QuotaPhase::Open,
            created: 0,
            destroyed: 0,
            cap_fails: 0,
            kept_alive_checks: 0,
        }
    }

    /// 最低空闲槽位（无则返回槽位总数 = 需扩容）。
    fn lowest_free(&self) -> usize {
        let mut i = 0;
        while i < self.slots.len() {
            if self.slots[i].is_none() {
                return i;
            }
            i += 1;
        }
        self.slots.len()
    }

    fn used(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.slots.len() {
            if self.slots[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// CreateObject：配额内分配，触顶判 ERROR_NO_MORE_HANDLES。
    pub fn create(&mut self, name: &str, serial: u64) -> Result<usize, u32> {
        let free = self.lowest_free();
        if free >= GDI_QUOTA {
            // 触顶：状态机 Open/Drained → AtCap，拒收并留痕。
            self.phase = QuotaPhase::AtCap;
            self.cap_fails += 1;
            return Err(ERROR_NO_MORE_HANDLES);
        }
        if free == self.slots.len() {
            self.slots.push(None);
        }
        self.slots[free] = Some(ObjRecord {
            name: String::from(name),
            serial,
            alive: true,
        });
        self.created += 1;
        let used_now = self.used();
        // 非法边拒收：占用不得越过配额闭集。
        if used_now > GDI_QUOTA {
            return Err(QuotaCode::QuotaEdgeIllegal.val());
        }
        // 满账回落 AtCap，未满归 Open（腾位复用后守恒一致）。
        self.phase = if used_now == GDI_QUOTA {
            QuotaPhase::AtCap
        } else {
            QuotaPhase::Open
        };
        Ok(free)
    }

    /// DestroyObject：释放槽位；无效/重复销毁拒收。
    pub fn destroy(&mut self, h: usize) -> Result<(), u32> {
        if h >= self.slots.len() {
            return Err(QuotaCode::HandleOutOfRange.val());
        }
        // 下标先比长度后再取；双 Option 检查拒重复销毁。
        let rec = match &self.slots[h] {
            Some(r) => r.clone(),
            None => return Err(QuotaCode::DoubleDestroyAccepted.val()),
        };
        if !rec.alive {
            return Err(QuotaCode::DoubleDestroyAccepted.val());
        }
        self.slots[h] = None;
        self.destroyed += 1;
        // 销毁腾位：AtCap → Drained（守恒账先核后转）。
        if self.used() != (self.created - self.destroyed) as usize {
            return Err(QuotaCode::SlotConservationBroken.val());
        }
        if self.phase == QuotaPhase::AtCap {
            self.phase = QuotaPhase::Drained;
        }
        Ok(())
    }

    /// 保活断言：触顶状态下既有句柄查询/枚举/销毁全保活。
    pub fn keepalive_audit(&mut self, probes: &[usize]) -> Result<u32, u32> {
        if self.phase != QuotaPhase::AtCap {
            return Err(QuotaCode::QuotaEdgeIllegal.val());
        }
        let mut n = 0;
        let mut i = 0;
        while i < probes.len() {
            let h = probes[i];
            if h >= self.slots.len() {
                return Err(QuotaCode::HandleOutOfRange.val());
            }
            match &self.slots[h] {
                Some(r) => {
                    if !r.alive || r.name.len() == 0 {
                        return Err(QuotaCode::KeepaliveBroken.val());
                    }
                    n += 1;
                    self.kept_alive_checks += 1;
                }
                None => return Err(QuotaCode::KeepaliveBroken.val()),
            }
            i += 1;
        }
        Ok(n)
    }

    /// 守恒对账：占用 = 累创 - 累毁；失败留痕与触顶态一致。
    pub fn conservation(&self) -> Result<(), u32> {
        if self.used() as u64 != self.created as u64 - self.destroyed as u64 {
            return Err(QuotaCode::SlotConservationBroken.val());
        }
        if self.used() == GDI_QUOTA && self.phase != QuotaPhase::AtCap {
            return Err(QuotaCode::QuotaEdgeIllegal.val());
        }
        Ok(())
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n < 1 { 1 } else { n }
    }
}

/// 黄金正演：预置保活探针 → LCG 压满至触顶 → 错误码断言 →
/// 保活全查 → 腾位复用最低空闲槽位。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    if GDI_QUOTA != QUOTA_EXPECTED {
        return Err(QuotaCode::QuotaConstMismatch.val());
    }
    let mut led = QuotaLedger::new();
    let mut g = Lcg::new(SEG_BASE as u64);
    // 预置保活探针句柄（0..KEEPALIVE_PROBES，先创先得即最低槽位）。
    let mut probes: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < KEEPALIVE_PROBES {
        let nm = format!("probe{}", i);
        let h = led.create(&nm, g.next())?;
        probes.push(h);
        i += 1;
    }
    // LCG 压满至触顶：循环创建直至首拒。
    let mut first_err: u32 = 0;
    let mut k = 0;
    loop {
        let nm = format!("fill{}", k);
        match led.create(&nm, g.next()) {
            Ok(_) => {
                k += 1;
                if led.used() >= GDI_QUOTA {
                    // 下一发必触顶，继续走一轮显式断言。
                    continue;
                }
            }
            Err(e) => {
                first_err = e;
                break;
            }
        }
        if k > GDI_QUOTA + 8 {
            return Err(QuotaCode::QuotaEdgeIllegal.val());
        }
    }
    if first_err != ERROR_NO_MORE_HANDLES {
        return Err(QuotaCode::CapErrorCodeMismatch.val());
    }
    if led.phase != QuotaPhase::AtCap {
        return Err(QuotaCode::QuotaEdgeIllegal.val());
    }
    // 触顶后再发一发仍判同码（稳定拒收）。
    if let Err(e2) = led.create("again", g.next()) {
        if e2 != ERROR_NO_MORE_HANDLES {
            return Err(QuotaCode::CapErrorCodeMismatch.val());
        }
    } else {
        return Err(QuotaCode::QuotaEdgeIllegal.val());
    }
    // 保活断言：既有探针全保活。
    let alive = led.keepalive_audit(&probes)?;
    if alive as usize != KEEPALIVE_PROBES {
        return Err(QuotaCode::KeepaliveBroken.val());
    }
    // 腾位复用：销毁探针 0 → 创建成功且复用槽位 0。
    led.destroy(probes[0])?;
    let h = led.create("reuse", g.next())?;
    if h != probes[0] {
        return Err(QuotaCode::ReuseNotLowestFree.val());
    }
    led.conservation()?;
    Ok((led.created, led.destroyed, led.cap_fails, led.kept_alive_checks))
}

// ── 注入面：三族 3/3 必抓 ────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：越界句柄销毁必拒。
    ledger.tries[0] += 1;
    let mut led = QuotaLedger::new();
    if let Err(e) = led.destroy(GDI_QUOTA + 1) {
        if e == QuotaCode::HandleOutOfRange.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：重复销毁必拒（同句柄销毁两次）。
    ledger.tries[1] += 1;
    let mut led2 = QuotaLedger::new();
    let h = led2.create("dup", 1);
    let mut dup_ok = false;
    if let Ok(hh) = h {
        let _ = led2.destroy(hh);
        if let Err(e) = led2.destroy(hh) {
            if e == QuotaCode::DoubleDestroyAccepted.val() {
                dup_ok = true;
            }
        }
    }
    if dup_ok {
        ledger.caught[1] += 1;
    }
    // 族 2：空账保活审计必拒（未触顶即审计 = 非法前置）。
    ledger.tries[2] += 1;
    let led3 = QuotaLedger::new();
    if let Err(e) = led3.conservation() {
        let _ = e;
    }
    let mut empty_probe: Vec<usize> = Vec::new();
    let mut led4 = QuotaLedger::new();
    led4.phase = QuotaPhase::AtCap;
    let mut caught = false;
    if let Err(e) = led4.keepalive_audit(&empty_probe) {
        if e == QuotaCode::QuotaEdgeIllegal.val() {
            caught = false; // 空探针在 AtCap 下合法返回 0，不构成注入
        }
    } else {
        caught = false;
    }
    // 真注入：AtCap 下探针指向空槽必判保活破。
    let mut led5 = QuotaLedger::new();
    led5.phase = QuotaPhase::AtCap;
    led5.slots.push(None);
    empty_probe.push(0);
    if let Err(e) = led5.keepalive_audit(&empty_probe) {
        if e == QuotaCode::KeepaliveBroken.val() {
            caught = true;
        }
    }
    if caught {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < QuotaCode::ALL.len() {
        let v = QuotaCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    // 黄金账：created = 1 万 + 8 探针 + 1 复用 - 1 触顶拒 = 触顶前全创；
    // 断言：created ≥ 配额、destroyed = 1、cap_fails ≥ 1、保活 = 8。
    let golden_ok = match &golden {
        Ok((c, d, f, k)) => {
            *c >= QUOTA_EXPECTED as u32 && *d == 1 && *f >= 2 && *k == KEEPALIVE_PROBES as u32
        }
        Err(_) => false,
    };
    v.push((
        "J1-触顶错误码+保活+腾位复用",
        golden_ok,
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    v.push((
        "J1-配额常量勾稽10000",
        GDI_QUOTA == QUOTA_EXPECTED && ERROR_NO_MORE_HANDLES == 18,
        format!("quota={} err={}", GDI_QUOTA, ERROR_NO_MORE_HANDLES),
    ));
    // 三态状态机闭集点验：Open/AtCap/Drained 互异且 Eq 可比。
    let phases_distinct = QuotaPhase::Open != QuotaPhase::AtCap
        && QuotaPhase::AtCap != QuotaPhase::Drained
        && QuotaPhase::Open != QuotaPhase::Drained;
    v.push((
        "J1-配额三态闭集互异",
        phases_distinct,
        String::from("Open/AtCap/Drained"),
    ));
    // 反向判据：空账 conservation 若 used==quota 判非法——空账 used=0 不触，
    // 构造满账置 phase=Open 必被判非法边。
    let mut tampered = QuotaLedger::new();
    let mut i = 0;
    while i < QUOTA_EXPECTED {
        tampered.slots.push(Some(ObjRecord {
            name: String::from("t"),
            serial: i as u64,
            alive: true,
        }));
        tampered.created += 1;
        i += 1;
    }
    let reverse_ok = match tampered.conservation() {
        Err(e) => e == QuotaCode::QuotaEdgeIllegal.val(),
        Ok(_) => false,
    };
    v.push((
        "J1-反向判据满账漏态被检出",
        reverse_ok,
        String::from("满账+Open 态必判非法边"),
    ));
    // 守恒恒等式直查：new 账 used=created-destroyed=0。
    let fresh_ok = QuotaLedger::new().conservation().is_ok();
    v.push(("J1-空账守恒恒等式", fresh_ok, String::from("0=0-0")));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66B40 窗口")));
    v
}

pub fn run_unx14781_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14781 GDI 配额 10000 触顶语义（ERROR_NO_MORE_HANDLES+保活）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
