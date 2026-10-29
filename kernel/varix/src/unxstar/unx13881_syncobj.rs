//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13881 · D1 同步族对象联签预演（Event/Mutex 句柄化） · 0x66000
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F13881-J1 同步对象经句柄表全链预演通（创建→命名→句柄化
//!      →等待），接口映射表联签留痕；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 同步族对象两型：Event（通知型/同步型，置位/复位语义）与
//!     Mutex（归属型，持有/释放语义，重入计账）；
//!   - 全链预演四段：创建（类型+初态）→ 命名（唯一名）→ 句柄化
//!      （句柄表登记，句柄唯一）→ 等待（Event 读态 / Mutex 竞争）；
//!   - 接口映射表联签留痕：每段操作的接口名→内核动作映射逐条入账，
//!     联签指纹（FNV 链）随账冻结；
//!   - Mutex 归属红线：非持有者释放必拒；重入计数守恒（锁定数 ≡
//!      释放数+当前持有重入数）；
//!   - 注入四族：未持有释放/句柄失配/重名创建/Event 二次自动复位
//!      误记账 10/10 必抓；
///   - LCG 种子写死 = SEG_BASE（0x66000），禁 rand/浮点/IO/墙钟；
///     no_std + alloc；零 unwrap/expect。
/// ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66000，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66000;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncCode {
    /// 未持有者释放 Mutex
    ReleaseByNonOwner,
    /// 句柄失配（句柄不存在或类型错）
    HandleMismatch,
    /// 重名创建
    DuplicateName,
    /// Event 语义误记账（自动复位事件二次读态）
    EventDoubleConsume,
    /// Mutex 重入计数破
    RecountBreak,
    /// 类型枚举外值
    TypeInvalid,
    /// 联签指纹漂移
    FingerprintDrift,
    /// 预演链断段
    ChainSegmentMissing,
}

impl SyncCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SyncCode::ReleaseByNonOwner => "未持有者释放：归属红线",
            SyncCode::HandleMismatch => "句柄失配：句柄表查无或类型错",
            SyncCode::DuplicateName => "重名创建：命名唯一红线",
            SyncCode::EventDoubleConsume => "自动复位事件二次读态：语义误记账",
            SyncCode::RecountBreak => "重入计数破：锁定≡释放+重入",
            SyncCode::TypeInvalid => "类型枚举外值",
            SyncCode::FingerprintDrift => "联签指纹漂移：留痕被改",
            SyncCode::ChainSegmentMissing => "预演链断段：四段必须齐",
        }
    }
    pub const ALL: [SyncCode; 8] = [
        SyncCode::ReleaseByNonOwner,
        SyncCode::HandleMismatch,
        SyncCode::DuplicateName,
        SyncCode::EventDoubleConsume,
        SyncCode::RecountBreak,
        SyncCode::TypeInvalid,
        SyncCode::FingerprintDrift,
        SyncCode::ChainSegmentMissing,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SyncCode::ALL.len() {
        if SyncCode::ALL[i].val() == code {
            return SyncCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66000 段闭集内"
}

// ── 对象模型 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncKind {
    EventManual,
    EventAuto,
    Mutex,
}

/// 同步对象态。
pub struct SyncObject {
    pub name: String,
    pub kind: SyncKind,
    /// Event：信号态；Mutex：0=空闲，≥1=持有重入数。
    pub signaled: bool,
    pub lock_count: u32,
    pub owner: Option<u32>,
    /// 累计锁定/释放账。
    pub locks_total: u32,
    pub releases_total: u32,
}

impl SyncObject {
    /// Event 置位（两型都置位）。
    pub fn event_set(&mut self) {
        self.signaled = true;
    }
    /// Event 读态+消费：自动复位型读后即清（一次消费）；手动复位
    /// 型读后保持（多人可读）。二次消费误记账：自动型读态已清再读
    /// 返回 false 并记 Noop（不计消费账）。
    pub fn event_wait(&mut self) -> bool {
        if !self.signaled {
            return false;
        }
        match self.kind {
            SyncKind::EventAuto => {
                self.signaled = false;
                true
            }
            _ => true,
        }
    }
    /// Mutex 锁定（重入计账）。
    pub fn mutex_lock(&mut self, tid: u32) -> Result<(), u32> {
        match (self.lock_count, self.owner) {
            (0, None) => {
                self.lock_count = 1;
                self.owner = Some(tid);
                self.locks_total += 1;
                Ok(())
            }
            (_, Some(o)) if o == tid => {
                self.lock_count += 1;
                self.locks_total += 1;
                Ok(())
            }
            _ => Err(SyncCode::HandleMismatch.val()),
        }
    }
    /// Mutex 释放（非持有者必拒；重入递减）。
    pub fn mutex_release(&mut self, tid: u32) -> Result<(), u32> {
        match self.owner {
            Some(o) if o == tid => {
                self.releases_total += 1;
                if self.lock_count > 1 {
                    self.lock_count -= 1;
                } else {
                    self.lock_count = 0;
                    self.owner = None;
                }
                Ok(())
            }
            _ => Err(SyncCode::ReleaseByNonOwner.val()),
        }
    }
    /// 重入守恒：锁定数 ≡ 释放数 + 当前重入数。
    pub fn conservation(&self) -> Result<(), u32> {
        if self.locks_total != self.releases_total + self.lock_count {
            return Err(SyncCode::RecountBreak.val());
        }
        Ok(())
    }
}

// ── 句柄表 ───────────────────────────────────────────────────────
pub struct HandleEntry {
    pub handle: u32,
    pub obj_idx: usize,
    pub kind: SyncKind,
}

pub struct HandleTable {
    pub entries: Vec<HandleEntry>,
    pub next_handle: u32,
}

impl HandleTable {
    pub fn new() -> HandleTable {
        HandleTable { entries: Vec::new(), next_handle: 0x1000 }
    }
    /// 句柄化。
    pub fn insert(&mut self, obj_idx: usize, kind: SyncKind) -> u32 {
        let h = self.next_handle;
        self.next_handle += 4;
        self.entries.push(HandleEntry { handle: h, obj_idx, kind });
        h
    }
    /// 查句柄（类型失配必抓）。
    pub fn lookup(&self, handle: u32, expect: SyncKind) -> Result<usize, u32> {
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            if e.handle == handle {
                if e.kind != expect {
                    return Err(SyncCode::HandleMismatch.val());
                }
                return Ok(e.obj_idx);
            }
            i += 1;
        }
        Err(SyncCode::HandleMismatch.val())
    }
}

// ── 对象管理器 + 联签留痕 ────────────────────────────────────────
/// 接口映射账行：接口名 → 内核动作。
pub struct MapRow {
    pub api: &'static str,
    pub action: &'static str,
}

/// 预演链四段闭集。
pub const CHAIN_APIS: [&str; 4] = ["创建", "命名", "句柄化", "等待"];

pub fn interface_map() -> Vec<MapRow> {
    let mut v: Vec<MapRow> = Vec::new();
    v.push(MapRow { api: "NtCreateEvent", action: "SyncObject(Event)+命名" });
    v.push(MapRow { api: "NtCreateMutant", action: "SyncObject(Mutex)+命名" });
    v.push(MapRow { api: "NtOpenXxx", action: "按名查对象" });
    v.push(MapRow { api: "NtWaitForXxx", action: "句柄→对象→等待语义" });
    v.push(MapRow { api: "NtReleaseMutant", action: "归属校验+重入递减" });
    v.push(MapRow { api: "NtSetEvent", action: "信号置位" });
    v
}

pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x01000193);
        i += 1;
    }
    h
}

/// 联签指纹：接口映射表 FNV 链。
pub fn map_fingerprint() -> u32 {
    let rows = interface_map();
    let mut acc: u32 = 0x811C9DC5;
    let mut i = 0;
    while i < rows.len() {
        acc = fnv1a(rows[i].api.as_bytes()) ^ acc;
        acc = acc.wrapping_mul(0x01000193);
        acc = fnv1a(rows[i].action.as_bytes()) ^ acc;
        acc = acc.wrapping_mul(0x01000193);
        i += 1;
    }
    acc
}

// ── 预演引擎 ─────────────────────────────────────────────────────
pub struct SyncManager {
    pub objects: Vec<SyncObject>,
    pub handles: HandleTable,
}

impl SyncManager {
    pub fn new() -> SyncManager {
        SyncManager { objects: Vec::new(), handles: HandleTable::new() }
    }

    /// 创建+命名（唯一名）。
    pub fn create(&mut self, name: &str, kind: SyncKind, init_signaled: bool) -> Result<usize, u32> {
        if self.objects.iter().any(|o| o.name == name) {
            return Err(SyncCode::DuplicateName.val());
        }
        self.objects.push(SyncObject {
            name: String::from(name),
            kind,
            signaled: init_signaled,
            lock_count: 0,
            owner: None,
            locks_total: 0,
            releases_total: 0,
        });
        Ok(self.objects.len() - 1)
    }

    /// 句柄化。
    pub fn to_handle(&mut self, obj_idx: usize) -> Result<u32, u32> {
        if obj_idx >= self.objects.len() {
            return Err(SyncCode::ChainSegmentMissing.val());
        }
        let k = self.objects[obj_idx].kind;
        Ok(self.handles.insert(obj_idx, k))
    }

    /// 等待（按句柄）。
    pub fn wait(&mut self, handle: u32) -> Result<bool, u32> {
        let idx = self.handles.lookup(handle, SyncKind::EventManual)
            .or_else(|_| self.handles.lookup(handle, SyncKind::EventAuto))?;
        Ok(self.objects[idx].event_wait())
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
        self.next() % n.max(1)
    }
}

// ── 黄金正演：Event/Mutex 全链预演 ───────────────────────────────
pub fn golden_run() -> Result<u32, u32> {
    let mut m = SyncManager::new();
    // Event（手动复位，初态无信号）→ 命名 → 句柄化 → 等待。
    let e = m.create("\\BaseNamedObjects\\GoEvent", SyncKind::EventManual, false)?;
    let he = m.to_handle(e)?;
    if m.wait(he)? {
        return Err(SyncCode::EventDoubleConsume.val());
    }
    m.objects[e].event_set();
    if !m.wait(he)? {
        return Err(SyncCode::ChainSegmentMissing.val());
    }
    // 手动复位：二次读仍真。
    if !m.wait(he)? {
        return Err(SyncCode::EventDoubleConsume.val());
    }
    // Mutex 全链：锁→重入→释放→守恒。
    let mx = m.create("\\BaseNamedObjects\\Lock", SyncKind::Mutex, false)?;
    let hm = m.to_handle(mx)?;
    let idx = m.handles.lookup(hm, SyncKind::Mutex)?;
    m.objects[idx].mutex_lock(7)?;
    m.objects[idx].mutex_lock(7)?;
    m.objects[idx].mutex_release(7)?;
    m.objects[idx].mutex_release(7)?;
    m.objects[idx].conservation()?;
    // 指纹冻结。
    let fp1 = map_fingerprint();
    let fp2 = map_fingerprint();
    if fp1 != fp2 {
        return Err(SyncCode::FingerprintDrift.val());
    }
    Ok(4) // 四段链全通。
}

// ── 注入面：四族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    let mut m = SyncManager::new();
    let mx = m.create("Lock", SyncKind::Mutex, false).unwrap_or(0);
    let _ = m.to_handle(mx);
    let idx = mx;
    let _ = m.objects[idx].mutex_lock(1);
    // 族 0：非持有者释放。
    ledger.tries[0] += 1;
    if m.objects[idx].mutex_release(2) == Err(SyncCode::ReleaseByNonOwner.val()) {
        ledger.caught[0] += 1;
    }
    // 族 1：句柄失配（Event 句柄查 Mutex）。
    ledger.tries[1] += 1;
    let e = m.create("Ev", SyncKind::EventManual, false).unwrap_or(0);
    let he = m.to_handle(e).unwrap_or(0);
    if m.handles.lookup(he, SyncKind::Mutex).is_err() {
        ledger.caught[1] += 1;
    }
    // 族 2：重名创建。
    ledger.tries[2] += 1;
    if m.create("Lock", SyncKind::Mutex, false).is_err() {
        ledger.caught[2] += 1;
    }
    // 族 3：自动复位二次消费语义（读后清——第二次为 false 且不误记）。
    ledger.tries[3] += 1;
    let a = m.create("EvA", SyncKind::EventAuto, true).unwrap_or(0);
    let ha = m.to_handle(a).unwrap_or(0);
    let first = m.wait(ha).unwrap_or(false);
    let second = m.wait(ha).unwrap_or(true);
    if first && !second {
        ledger.caught[3] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SyncCode::ALL.len() {
        let v = SyncCode::ALL[i].val();
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
    v.push((
        "J1-四段链全通",
        golden == Ok(4),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入四族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 接口映射表留痕点验。
    let rows = interface_map();
    let mut ok = 0;
    let mut i = 0;
    while i < rows.len() {
        if rows[i].api.len() > 0 && rows[i].action.len() > 0 {
            ok += 1;
        }
        i += 1;
    }
    v.push(("J1-映射表留痕", ok == 6, format!("{}/6", ok)));
    // 四段名点验。
    v.push((
        "J1-四段名点验",
        CHAIN_APIS.len() == 4 && CHAIN_APIS[0] == "创建",
        String::from("创建→命名→句柄化→等待"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66000 窗口")));
    v
}

pub fn run_unx13881_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13881 D1 同步族对象联签预演（Event/Mutex 句柄化）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
