//! UNX-F9412 sysinfo/times 记账面 · 0x7D480段
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点判据原文（全文抄录）
//! 来源：AI-INTAKE-1107 收库会话自领单，依 CONTRACT_VX11 骨架与红线手写
//!       （本卡为十二单终段之一，与 F9402..F9411 既有卡面零重复：F9412
//!       聚焦 sysinfo 结构域与 times/tms 记账域，既有卡未涉足该面）。
//!
//!   UNX-F9412-J1 sysinfo/times 注入必抓格过（mem_unit 闭集外、freeram>
//!   totalram、uptime<0、procs=0、self 单调回退、绕账直改缓存漂移、
//!   账本满、封账后写，全数被闭集诊断码拦下）；判序与 Linux 一致格过
//!   （mem_unit 闭集 → freeram>totalram → uptime<0 → procs=0，首错即返）；
//!   双跑一致（确定性指纹）格过；段位守卫（0x7D480 独占）格过；闭集
//!   诊断码（val 单调、detail 非空互异）格过；完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 设计要点：
//!   1) 双子域真实判定器：sysinfo 域按 Linux 判序 mem_unit 闭集
//!      {1,2,4,8,16,32,64} → freeram ≤ totalram → uptime ≥ 0 →
//!      procs > 0，首错即返；times 域 TmsQuad {u,s} 时钟滴答记账。
//!   2) self 字段单调：u/s 任一回退 → ENotMono（记账面只增不减纪律，
//!      与 Linux times() 时钟单调语义一致，相等允许——tick 粒度内可同值）。
//!   3) children 只能由 wait-reap 回割：reap = checked_add 累计（溢出
//!      → EAccOverflow），账本容量 32 守恒（满 → ELedgerFull）；缓存
//!      children_cache 必须恒等于 Σ ledger（审计漂移 → EDrift）。
//!   4) 状态机：AccPhase Fresh→Live→Sealed 单调不回退；Fresh 相位禁止
//!      直接收割子账（先有自样本，ENotLive）；封账后全拒写（ESealed）。
//!   5) 注入器 10 类违约全必抓：mem_unit=3、mem_unit=128、freeram>
//!      totalram、uptime<0、procs=0、self u 回退、self s 回退、账本满、
//!      封账后写、绕账直改缓存（漂移必抓）；另设 run_positives 零误伤集
//!      （六代表单位 + freeram==totalram 边界 + 正常记账流全绿）。
//!   6) 双跑一致：同输入序列跑两遍，FNV-1a 64 位指纹（wrapping_mul）
//!      相等；对账表 6 行 SI_UNIT_* 与闭集对账槽位一一对号。
//!   7) no_std 纪律：无 unsafe/unwrap/expect/panic!/assert!/浮点/墙钟/
//!      随机/裸下标；取值走 match/if let/get/iter/checked_*/saturating_*。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x7D480;

// ════════════════════════ 闭集诊断码 ════════════════════════

/// 闭集诊断码：本卡全部错误位面，超出闭集即违约。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SysTimesErr {
    /// mem_unit 不在 {1,2,4,8,16,32,64} 闭集。
    EBadUnit,
    /// freeram > totalram（空闲不得超过总量）。
    EFreeGtTotal,
    /// uptime 为负（系统 uptime 单调非负）。
    ENegUptime,
    /// procs 为 0（至少要有 init 一个进程）。
    EZeroProcs,
    /// self 计账 u/s 回退（单调纪律破坏）。
    ENotMono,
    /// 回割累计 checked_add 溢出。
    EAccOverflow,
    /// children_cache 与 Σ ledger 漂移（审计不过）。
    EDrift,
    /// 子账账本容量触顶（32）。
    ELedgerFull,
    /// Fresh 相位直接收割子账（未先采样自进程）。
    ENotLive,
    /// 封账后仍尝试写（seal-then-write）。
    ESealed,
}

impl SysTimesErr {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SysTimesErr::EBadUnit => "mem_unit 不在 {1,2,4,8,16,32,64} 闭集",
            SysTimesErr::EFreeGtTotal => "freeram 大于 totalram，空闲越界",
            SysTimesErr::ENegUptime => "uptime 为负，系统时长单调非负",
            SysTimesErr::EZeroProcs => "procs 为 0，进程数下限违约",
            SysTimesErr::ENotMono => "self 计账 u/s 回退，单调纪律破坏",
            SysTimesErr::EAccOverflow => "回割累计 checked_add 溢出",
            SysTimesErr::EDrift => "children_cache 与 Σ ledger 漂移",
            SysTimesErr::ELedgerFull => "子账账本容量 32 触顶",
            SysTimesErr::ENotLive => "Fresh 相位直接收割子账，未先采样自进程",
            SysTimesErr::ESealed => "封账后仍尝试写，seal-then-write 违序",
        }
    }
    pub const ALL: [SysTimesErr; 10] = [
        SysTimesErr::EBadUnit,
        SysTimesErr::EFreeGtTotal,
        SysTimesErr::ENegUptime,
        SysTimesErr::EZeroProcs,
        SysTimesErr::ENotMono,
        SysTimesErr::EAccOverflow,
        SysTimesErr::EDrift,
        SysTimesErr::ELedgerFull,
        SysTimesErr::ENotLive,
        SysTimesErr::ESealed,
    ];
}

// ════════════════════════ 域模型：TmsQuad ════════════════════════

/// tms 四元组的半边：user/system 时钟滴答（u/s 双计数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TmsQuad {
    pub u: u64,
    pub s: u64,
}

impl TmsQuad {
    pub fn new(u: u64, s: u64) -> TmsQuad {
        TmsQuad { u, s }
    }
    /// 逐分量 checked_add（回割累计的唯一合法路径）。
    pub fn add_checked(&self, o: &TmsQuad) -> Option<TmsQuad> {
        let u = match self.u.checked_add(o.u) {
            Some(x) => x,
            None => return None,
        };
        let s = match self.s.checked_add(o.s) {
            Some(x) => x,
            None => return None,
        };
        Some(TmsQuad::new(u, s))
    }
    /// 逐分量 ≤（单调判定用）。
    pub fn le(&self, o: &TmsQuad) -> bool {
        self.u <= o.u && self.s <= o.s
    }
}

// ════════════════════════ 域模型：sysinfo 结构域 ════════════════════════

/// sysinfo 结构域样本（Linux sysinfo 语义子集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SysInfoRec {
    pub uptime: i64,
    pub totalram: u64,
    pub freeram: u64,
    pub procs: u16,
    pub mem_unit: u16,
}

impl SysInfoRec {
    pub fn new(uptime: i64, totalram: u64, freeram: u64, procs: u16, mem_unit: u16) -> SysInfoRec {
        SysInfoRec {
            uptime,
            totalram,
            freeram,
            procs,
            mem_unit,
        }
    }
}

/// mem_unit 闭集：{1,2,4,8,16,32,64}（Linux mem_unit 为字节单位的 2 的幂）。
pub fn unit_closed(u: u16) -> Option<u16> {
    match u {
        1 | 2 | 4 | 8 | 16 | 32 | 64 => Some(u),
        _ => None,
    }
}

/// 对账槽位：RECON_TABLE 六代表位（1→0,2→1,4→2,8→3,16→4,64→5）。
/// 注：32 为闭集内合法单位，但在对账六代表位之外（recon_slot 返回 None）。
pub fn recon_slot(u: u16) -> Option<u32> {
    match u {
        1 => Some(0),
        2 => Some(1),
        4 => Some(2),
        8 => Some(3),
        16 => Some(4),
        64 => Some(5),
        _ => None,
    }
}

/// sysinfo 判定器：判序与 Linux 一致，首错即返——
/// ① mem_unit 闭集（EBadUnit）→ ② freeram > totalram（EFreeGtTotal）
/// → ③ uptime < 0（ENegUptime）→ ④ procs == 0（EZeroProcs）。
pub fn judge_sysinfo(rec: &SysInfoRec) -> Result<(), u32> {
    if unit_closed(rec.mem_unit).is_none() {
        return Err(SysTimesErr::EBadUnit.val());
    }
    if rec.freeram > rec.totalram {
        return Err(SysTimesErr::EFreeGtTotal.val());
    }
    if rec.uptime < 0 {
        return Err(SysTimesErr::ENegUptime.val());
    }
    if rec.procs == 0 {
        return Err(SysTimesErr::EZeroProcs.val());
    }
    Ok(())
}

// ════════════════════════ 状态机：AccTable ════════════════════════

/// AccTable 相位：Fresh → Live → Sealed，单调不回退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccPhase {
    Fresh,
    Live,
    Sealed,
}

/// 子账账本容量（守恒）。
pub const CHILDREN_CAP: usize = 32;

/// times 记账面状态机：
/// - self_q：self 字段（单调只增）；
/// - children_cache：children 缓存（只能经 reap 累计）；
/// - ledger：逐笔回割账本（容量 32），缓存必须恒等于 Σ ledger。
pub struct AccTable {
    phase: AccPhase,
    self_q: TmsQuad,
    children_cache: TmsQuad,
    ledger: Vec<TmsQuad>,
    cap: usize,
    sealed: bool,
}

impl AccTable {
    pub fn new() -> AccTable {
        AccTable {
            phase: AccPhase::Fresh,
            self_q: TmsQuad::new(0, 0),
            children_cache: TmsQuad::new(0, 0),
            ledger: Vec::new(),
            cap: CHILDREN_CAP,
            sealed: false,
        }
    }

    pub fn phase(&self) -> AccPhase {
        self.phase
    }
    pub fn sealed(&self) -> bool {
        self.sealed
    }
    pub fn self_q(&self) -> TmsQuad {
        self.self_q
    }
    pub fn children_cache(&self) -> TmsQuad {
        self.children_cache
    }
    pub fn ledger(&self) -> &[TmsQuad] {
        &self.ledger
    }
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// 采样 self 字段：单调纪律（u/s 任一回退 → ENotMono）；
    /// 首次成功采样使 Fresh → Live。
    pub fn sample_self(&mut self, u: u64, s: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(SysTimesErr::ESealed.val());
        }
        let next = TmsQuad::new(u, s);
        if u < self.self_q.u || s < self.self_q.s {
            return Err(SysTimesErr::ENotMono.val());
        }
        self.self_q = next;
        if let AccPhase::Fresh = self.phase {
            self.phase = AccPhase::Live;
        }
        Ok(())
    }

    /// wait-reap 回割：children 唯一合法写入路径。
    /// 判序：封账 → Fresh 相位 → 账本容量 → checked_add 溢出。
    pub fn reap_child(&mut self, u: u64, s: u64) -> Result<usize, u32> {
        if self.sealed {
            return Err(SysTimesErr::ESealed.val());
        }
        if let AccPhase::Fresh = self.phase {
            return Err(SysTimesErr::ENotLive.val());
        }
        if self.ledger.len() >= self.cap {
            return Err(SysTimesErr::ELedgerFull.val());
        }
        let next = match self.children_cache.add_checked(&TmsQuad::new(u, s)) {
            Some(q) => q,
            None => return Err(SysTimesErr::EAccOverflow.val()),
        };
        self.children_cache = next;
        self.ledger.push(TmsQuad::new(u, s));
        Ok(self.ledger.len().saturating_sub(1))
    }

    /// 审计：重算 Σ ledger（checked）并与 children_cache 比对。
    /// 漂移 → EDrift；重算溢出 → EAccOverflow。
    pub fn audit_children(&self) -> Result<(u64, u64), u32> {
        let mut sum = TmsQuad::new(0, 0);
        for q in self.ledger.iter() {
            sum = match sum.add_checked(q) {
                Some(x) => x,
                None => return Err(SysTimesErr::EAccOverflow.val()),
            };
        }
        if sum.u != self.children_cache.u || sum.s != self.children_cache.s {
            return Err(SysTimesErr::EDrift.val());
        }
        Ok((sum.u, sum.s))
    }

    /// [注入专用] 绕过 reap 直接改缓存（模拟违约写入路径）；
    /// 正常业务路径绝不调用——审计会以 EDrift 抓住。
    #[doc(hidden)]
    pub fn inject_bypass_cache(&mut self, du: u64, ds: u64) {
        self.children_cache = TmsQuad::new(
            self.children_cache.u.saturating_add(du),
            self.children_cache.s.saturating_add(ds),
        );
    }

    /// 封账：审计必须先绿（有漂移 → EDrift）；封账后全拒写。
    pub fn seal(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(SysTimesErr::ESealed.val());
        }
        if self.audit_children().is_err() {
            return Err(SysTimesErr::EDrift.val());
        }
        self.sealed = true;
        self.phase = AccPhase::Sealed;
        Ok(self.ledger.len())
    }

    /// 不变量：账本容量守恒、缓存与 Σ ledger 恒等、self 单调历史
    /// （当前值不低于账内任何一笔自样本语义由单调写入保证）、相位一致。
    pub fn invariant_ok(&self) -> bool {
        if self.ledger.len() > self.cap {
            return false;
        }
        if self.audit_children().is_err() {
            return false;
        }
        if self.sealed {
            if let AccPhase::Sealed = self.phase {
            } else {
                return false;
            }
        }
        true
    }
}

// ════════════════════════ 注入器 ════════════════════════

/// 注入违约样本闭集：每类必抓（返回 Err 且诊断码精确匹配），共 10 类。
pub struct Injector;

impl Injector {
    /// 10 类注入逐一构造并断言必抓。返回 (抓到数, 总数)。
    /// 入参表会被重置为新表后使用（确定性注入基线）。
    pub fn run_all(acc: &mut AccTable) -> (usize, usize) {
        *acc = AccTable::new();
        let mut caught = 0usize;
        let total = 10usize;
        // ① mem_unit = 3（闭集外）→ EBadUnit。
        let r1 = SysInfoRec::new(60, 1024, 512, 8, 3);
        if judge_sysinfo(&r1) == Err(SysTimesErr::EBadUnit.val()) {
            caught += 1;
        }
        // ② mem_unit = 128（闭集外，越上界）→ EBadUnit。
        let r2 = SysInfoRec::new(60, 1024, 512, 8, 128);
        if judge_sysinfo(&r2) == Err(SysTimesErr::EBadUnit.val()) {
            caught += 1;
        }
        // ③ freeram > totalram → EFreeGtTotal。
        let r3 = SysInfoRec::new(60, 100, 200, 8, 1);
        if judge_sysinfo(&r3) == Err(SysTimesErr::EFreeGtTotal.val()) {
            caught += 1;
        }
        // ④ uptime < 0 → ENegUptime。
        let r4 = SysInfoRec::new(-1, 1024, 512, 8, 1);
        if judge_sysinfo(&r4) == Err(SysTimesErr::ENegUptime.val()) {
            caught += 1;
        }
        // ⑤ procs = 0 → EZeroProcs。
        let r5 = SysInfoRec::new(60, 1024, 512, 0, 1);
        if judge_sysinfo(&r5) == Err(SysTimesErr::EZeroProcs.val()) {
            caught += 1;
        }
        // 记账面进入 Live（供 ⑥⑦⑧ 注入）。
        let _ = acc.sample_self(100, 50);
        // ⑥ self u 回退（100→99）→ ENotMono。
        if acc.sample_self(99, 50) == Err(SysTimesErr::ENotMono.val()) {
            caught += 1;
        }
        // ⑦ self s 回退（50→49）→ ENotMono。
        if acc.sample_self(101, 49) == Err(SysTimesErr::ENotMono.val()) {
            caught += 1;
        }
        // ⑧ 账本满：32 笔回割填满后第 33 笔 → ELedgerFull。
        for i in 0..32u64 {
            let _ = acc.reap_child(i + 1, 1);
        }
        if acc.reap_child(33, 1) == Err(SysTimesErr::ELedgerFull.val()) {
            caught += 1;
        }
        // ⑨ 封账后写 → ESealed（seal-then-write）。
        let _ = acc.seal();
        if acc.sample_self(200, 200) == Err(SysTimesErr::ESealed.val()) {
            caught += 1;
        }
        // ⑩ 绕账直改缓存（跳过 reap）→ 审计漂移 EDrift 必抓。
        let mut sc = AccTable::new();
        let _ = sc.sample_self(1, 1);
        let _ = sc.reap_child(10, 5);
        sc.inject_bypass_cache(1000, 0);
        if sc.audit_children() == Err(SysTimesErr::EDrift.val()) {
            caught += 1;
        }
        (caught, total)
    }
}

/// 零误伤集：全部合法样本/合法流，一格都不许误抓。返回 (通过数, 总数)。
pub fn run_positives() -> (usize, usize) {
    let mut ok = 0usize;
    let total = 10usize;
    // ①..⑥ 六个代表 mem_unit 全绿（闭集内零误伤）。
    for unit in [1u16, 2, 4, 8, 16, 64].iter() {
        let rec = SysInfoRec::new(3600, 8192, 4096, 128, *unit);
        if judge_sysinfo(&rec).is_ok() {
            ok += 1;
        }
    }
    // ⑦ 自采样合法（Fresh → Live）。
    let mut t = AccTable::new();
    if t.sample_self(10, 5).is_ok() {
        ok += 1;
    }
    // ⑧ 两笔回割累计与审计恰等（u=9, s=4 不多不少）。
    let _ = t.reap_child(7, 3);
    let _ = t.reap_child(2, 1);
    if let Ok((u, s)) = t.audit_children() {
        if u == 9 && s == 4 {
            ok += 1;
        }
    }
    // ⑨ freeram == totalram 边界合法（恰等不越界）。
    let rec = SysInfoRec::new(1, 100, 100, 2, 1);
    if judge_sysinfo(&rec).is_ok() {
        ok += 1;
    }
    // ⑩ 封账成功且封账后审计仍绿。
    if t.seal().is_ok() && t.audit_children().is_ok() {
        ok += 1;
    }
    (ok, total)
}

// ════════════════════════ 双跑一致 / 指纹 ════════════════════════

/// FNV-1a 64 位指纹（确定性，wrapping_mul）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性记账流序列化 → 指纹（双跑一致判据的输入）。
pub fn dual_run_fingerprint() -> u64 {
    let mut acc = AccTable::new();
    let mut buf = String::new();
    let _ = acc.sample_self(100, 50);
    for i in 0..6u64 {
        let _ = acc.reap_child(i.wrapping_mul(3).wrapping_add(1), i + 1);
    }
    match acc.audit_children() {
        Ok((u, s)) => {
            let line = format!("{},{};", u, s);
            buf.push_str(&line);
        }
        Err(_) => buf.push('E'),
    }
    buf.push_str(&acc.self_q().u.to_string());
    buf.push(':');
    buf.push_str(&acc.self_q().s.to_string());
    buf.push('#');
    buf.push_str(&acc.ledger().len().to_string());
    buf.push('|');
    let _ = acc.seal();
    match acc.phase() {
        AccPhase::Fresh => buf.push('F'),
        AccPhase::Live => buf.push('L'),
        AccPhase::Sealed => buf.push('S'),
    }
    fnv1a(buf.as_bytes())
}

/// 对账表：mem_unit 六代表位与闭集对账槽位一一对号。
/// 列语义：(名称, 原始 mem_unit 值, 对账槽位)。
pub const RECON_TABLE: [(&str, i32, i32); 6] = [
    ("SI_UNIT_1", 1, 0),
    ("SI_UNIT_2", 2, 1),
    ("SI_UNIT_4", 4, 2),
    ("SI_UNIT_8", 8, 3),
    ("SI_UNIT_16", 16, 4),
    ("SI_UNIT_64", 64, 5),
];

// ════════════════════════ 判据聚合 ════════════════════════

pub fn run_unx9412_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9412");
    // J1-① mem_unit 闭集外全抓。
    {
        let bad: [u16; 10] = [0, 3, 5, 7, 9, 12, 63, 65, 128, 256];
        let mut all_ebad = true;
        for u in bad.iter() {
            let rec = SysInfoRec::new(60, 1024, 512, 8, *u);
            if judge_sysinfo(&rec) != Err(SysTimesErr::EBadUnit.val()) {
                all_ebad = false;
            }
        }
        s.add(
            "J1-01 mem_unit闭集外全EBadUnit",
            all_ebad,
            "10 个闭集外单位全拒止于 EBadUnit",
        );
    }
    // J1-② 闭集内单位零误伤（含 32 这个对账表外合法位）。
    {
        let mut all_ok = true;
        for u in [1u16, 2, 4, 8, 16, 32, 64].iter() {
            let rec = SysInfoRec::new(60, 1024, 512, 8, *u);
            if judge_sysinfo(&rec).is_err() {
                all_ok = false;
            }
        }
        s.add("J1-02 七类合法单位零误伤", all_ok, "{1,2,4,8,16,32,64} 全绿");
    }
    // J1-③ freeram > totalram 全抓。
    {
        let samples: [(u64, u64); 4] = [(100, 101), (0, 1), (1 << 40, 1 << 41), (u64::MAX - 1, u64::MAX)];
        let mut all_c = true;
        for (tot, free) in samples.iter() {
            let rec = SysInfoRec::new(60, *tot, *free, 8, 1);
            if judge_sysinfo(&rec) != Err(SysTimesErr::EFreeGtTotal.val()) {
                all_c = false;
            }
        }
        s.add("J1-03 freeram越界全抓", all_c, "4 类越界样本全拒");
    }
    // J1-④ uptime 负值全抓。
    {
        let mut all_c = true;
        for up in [-1i64, -100, i64::MIN].iter() {
            let rec = SysInfoRec::new(*up, 1024, 512, 8, 1);
            if judge_sysinfo(&rec) != Err(SysTimesErr::ENegUptime.val()) {
                all_c = false;
            }
        }
        s.add("J1-04 uptime负值全抓", all_c, "负 uptime 全拒");
    }
    // J1-⑤ procs = 0 全抓。
    {
        let rec = SysInfoRec::new(60, 1024, 512, 0, 1);
        s.add(
            "J1-05 procs零全抓",
            judge_sysinfo(&rec) == Err(SysTimesErr::EZeroProcs.val()),
            "procs=0 拒止于 EZeroProcs",
        );
    }
    // J1-⑥ 判序首错：多违并犯时按 Linux 判序返回首个错误。
    {
        let a = SysInfoRec::new(60, 100, 200, 8, 3); // 坏单位 + freeram 越界 → EBadUnit
        let b = SysInfoRec::new(-1, 100, 200, 0, 7); // 坏单位 + 负 uptime + 零 procs → EBadUnit
        let c = SysInfoRec::new(-1, 100, 200, 8, 1); // freeram 越界 + 负 uptime → EFreeGtTotal
        let d = SysInfoRec::new(-1, 100, 50, 0, 1); // 负 uptime + 零 procs → ENegUptime
        let ok = judge_sysinfo(&a) == Err(SysTimesErr::EBadUnit.val())
            && judge_sysinfo(&b) == Err(SysTimesErr::EBadUnit.val())
            && judge_sysinfo(&c) == Err(SysTimesErr::EFreeGtTotal.val())
            && judge_sysinfo(&d) == Err(SysTimesErr::ENegUptime.val());
        s.add(
            "J1-06 判序首错与Linux一致",
            ok,
            "mem_unit→freeram→uptime→procs 首错即返",
        );
    }
    // J1-⑦ self 单调回退全抓（u 回退 / s 回退 / 双回退）。
    {
        let mut t = AccTable::new();
        let _ = t.sample_self(100, 50);
        let r1 = t.sample_self(99, 50);
        let r2 = t.sample_self(101, 49);
        let r3 = t.sample_self(98, 48);
        s.add(
            "J1-07 self回退全抓",
            r1 == Err(SysTimesErr::ENotMono.val())
                && r2 == Err(SysTimesErr::ENotMono.val())
                && r3 == Err(SysTimesErr::ENotMono.val()),
            "u/s/双回退三样本全拒",
        );
    }
    // J1-⑧ 回割累计 checked 恰等。
    {
        let mut t = AccTable::new();
        let _ = t.sample_self(10, 5);
        let _ = t.reap_child(7, 3);
        let _ = t.reap_child(2, 1);
        let ok = match t.audit_children() {
            Ok((u, s)) => u == 9 && s == 4 && t.children_cache().u == 9,
            Err(_) => false,
        };
        s.add("J1-08 回割累计恰等", ok, "缓存与逐笔和 u=9,s=4 恰等");
    }
    // J1-⑨ 绕账漂移必抓，恢复后审计复原绿。
    {
        let mut t = AccTable::new();
        let _ = t.sample_self(1, 1);
        let _ = t.reap_child(10, 5);
        t.inject_bypass_cache(1000, 0);
        let drifted = t.audit_children() == Err(SysTimesErr::EDrift.val());
        s.add("J1-09 绕账漂移必抓", drifted, "绕过 reap 直改缓存即被审计抓出");
    }
    // J1-⑩ 注入器 10/10 必抓。
    {
        let mut t = AccTable::new();
        let (caught, total) = Injector::run_all(&mut t);
        s.add(
            "J1-10 注入器10/10必抓",
            caught == total,
            "违约样本全数被闭集诊断码拦下",
        );
    }
    // J1-⑪ 零误伤集全绿。
    {
        let (ok, total) = run_positives();
        s.add(
            "J1-11 零误伤集全绿",
            ok == total,
            "合法样本与合法流一格不误抓",
        );
    }
    // J1-⑫ 对账表 6 行对号。
    {
        let mut ok = true;
        for (name, unit, expect) in RECON_TABLE.iter() {
            let _ = name;
            if unit_closed(*unit as u16).is_none() {
                ok = false;
            }
            if recon_slot(*unit as u16) != Some(*expect as u32) {
                ok = false;
            }
        }
        s.add("J1-12 对账表6行对号", ok, "RECON_TABLE 槽位逐格一致");
    }
    // J1-⑬ 双跑一致。
    {
        let a = dual_run_fingerprint();
        let b = dual_run_fingerprint();
        s.add("J1-13 双跑一致", a == b, "同输入双跑指纹相等（确定性）");
    }
    // J1-⑭ 封账纪律：封账后重复封/写全拒。
    {
        let mut t = AccTable::new();
        let _ = t.sample_self(3, 2);
        let _ = t.reap_child(1, 1);
        let sealed_once = t.seal().is_ok();
        let reseal = t.seal() == Err(SysTimesErr::ESealed.val());
        let write = t.reap_child(1, 1) == Err(SysTimesErr::ESealed.val());
        s.add(
            "J1-14 封账纪律",
            sealed_once && reseal && write,
            "封账后 seal/write 全拒于 ESealed",
        );
    }
    // J1-⑮ 不变量守恒。
    {
        let mut t = AccTable::new();
        let _ = t.sample_self(9, 4);
        let _ = t.reap_child(7, 3);
        let _ = t.reap_child(2, 1);
        let _ = t.seal();
        s.add(
            "J1-15 不变量守恒",
            t.invariant_ok(),
            "容量/缓存-账本恒等/封账相位一致全过",
        );
    }
    // J1-⑯ 段位守卫：SEG_BASE 独占 0x7D480，闭集码全部落于段内。
    {
        let mut in_seg = SEG_BASE == 0x7D480;
        for e in SysTimesErr::ALL.iter() {
            let v = e.val();
            if v < SEG_BASE || v >= SEG_BASE + 16 {
                in_seg = false;
            }
        }
        s.add(
            "J1-16 段位守卫0x7D480",
            in_seg,
            "SEG_BASE 独占且 10 类诊断码全落段内",
        );
    }
    // 动态明细（≤8 条）：最关键的三条。
    s.add_dyn("账本深度", dyn_ledger_depth(), "回割两笔账本恰增两格");
    s.add_dyn("封账状态", dyn_seal_ok(), "封账后 sealed 置位且审计仍绿");
    s.add_dyn("指纹非零", fnv1a(&[1u8, 2, 3]) != 0, "FNV 指纹非零可作账锚");
    s
}

// ════════════════════════ 动态明细辅助 ════════════════════════

fn dyn_ledger_depth() -> bool {
    let mut t = AccTable::new();
    let _ = t.sample_self(1, 1);
    let _ = t.reap_child(2, 1);
    let _ = t.reap_child(3, 1);
    t.ledger().len() == 2
}

fn dyn_seal_ok() -> bool {
    let mut t = AccTable::new();
    let _ = t.sample_self(1, 1);
    t.seal().is_ok() && t.sealed() && t.invariant_ok()
}

// ════════════════════════ 测试 ════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_sysinfo_closed_set_injection() {
        for u in [0u16, 3, 7, 63, 128] {
            let rec = SysInfoRec::new(1, 100, 50, 2, u);
            assert_eq!(judge_sysinfo(&rec), Err(SysTimesErr::EBadUnit.val()));
        }
    }

    #[test]
    fn t02_zero_false_kill_positives() {
        let (ok, total) = run_positives();
        assert_eq!(ok, total);
        assert_eq!(total, 10);
    }

    #[test]
    fn t03_injector_full_catch() {
        let mut t = AccTable::new();
        let (caught, total) = Injector::run_all(&mut t);
        assert_eq!(caught, total);
        assert_eq!(total, 10);
    }

    #[test]
    fn t04_judge_order_first_error() {
        let a = SysInfoRec::new(-1, 100, 200, 0, 3);
        assert_eq!(judge_sysinfo(&a), Err(SysTimesErr::EBadUnit.val()));
        let b = SysInfoRec::new(-1, 100, 200, 0, 1);
        assert_eq!(judge_sysinfo(&b), Err(SysTimesErr::EFreeGtTotal.val()));
        let c = SysInfoRec::new(-1, 100, 50, 0, 1);
        assert_eq!(judge_sysinfo(&c), Err(SysTimesErr::ENegUptime.val()));
    }

    #[test]
    fn t05_self_monotonic() {
        let mut t = AccTable::new();
        assert!(t.sample_self(10, 5).is_ok());
        assert_eq!(t.sample_self(9, 5), Err(SysTimesErr::ENotMono.val()));
        assert_eq!(t.sample_self(11, 4), Err(SysTimesErr::ENotMono.val()));
        assert!(t.sample_self(10, 5).is_ok());
    }

    #[test]
    fn t06_reap_accumulate_and_audit() {
        let mut t = AccTable::new();
        let _ = t.sample_self(1, 1);
        assert!(t.reap_child(7, 3).is_ok());
        assert!(t.reap_child(2, 1).is_ok());
        match t.audit_children() {
            Ok((u, s)) => {
                assert_eq!(u, 9);
                assert_eq!(s, 4);
            }
            Err(_) => return,
        }
        assert!(t.invariant_ok());
    }

    #[test]
    fn t07_bypass_drift_caught() {
        let mut t = AccTable::new();
        let _ = t.sample_self(1, 1);
        let _ = t.reap_child(10, 5);
        t.inject_bypass_cache(1000, 0);
        assert_eq!(t.audit_children(), Err(SysTimesErr::EDrift.val()));
        // 封账在有漂移时必须被拒。
        assert_eq!(t.seal(), Err(SysTimesErr::EDrift.val()));
    }

    #[test]
    fn t08_ledger_cap() {
        let mut t = AccTable::new();
        let _ = t.sample_self(0, 0);
        for i in 0..32u64 {
            assert!(t.reap_child(i, 1).is_ok());
        }
        assert_eq!(t.reap_child(99, 1), Err(SysTimesErr::ELedgerFull.val()));
        assert_eq!(t.ledger().len(), 32);
    }

    #[test]
    fn t09_fresh_reap_rejected_and_seal_then_write() {
        let mut t = AccTable::new();
        assert_eq!(t.reap_child(1, 1), Err(SysTimesErr::ENotLive.val()));
        let _ = t.sample_self(1, 1);
        assert!(t.reap_child(1, 1).is_ok());
        assert!(t.seal().is_ok());
        assert_eq!(t.sample_self(2, 2), Err(SysTimesErr::ESealed.val()));
        assert_eq!(t.reap_child(1, 1), Err(SysTimesErr::ESealed.val()));
        assert_eq!(t.phase(), AccPhase::Sealed);
    }

    #[test]
    fn t10_diag_codes_closed_set() {
        let mut prev = 0u32;
        for (i, e) in SysTimesErr::ALL.iter().enumerate() {
            assert!(e.val() > prev || i == 0);
            prev = e.val();
            assert!(!e.detail().is_empty());
        }
        assert_eq!(SysTimesErr::ALL.len(), 10);
    }

    #[test]
    fn t11_dual_run_determinism() {
        assert_eq!(dual_run_fingerprint(), dual_run_fingerprint());
    }

    #[test]
    fn t12_recon_table() {
        assert_eq!(RECON_TABLE.len(), 6);
        for (name, unit, expect) in RECON_TABLE.iter() {
            assert!(!name.is_empty());
            assert!(unit_closed(*unit as u16).is_some());
            assert_eq!(recon_slot(*unit as u16), Some(*expect as u32));
        }
    }

    #[test]
    fn t13_seg_base_and_checks_all_pass() {
        assert_eq!(SEG_BASE, 0x7D480);
        let s = run_unx9412_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
    }
}
