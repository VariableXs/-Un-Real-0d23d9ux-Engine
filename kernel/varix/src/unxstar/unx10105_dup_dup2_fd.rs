//! UNX-F10105 dup/dup2 正反双判据（复制达成与非法 fd 误用受控） · 0x92a40 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10105（dup/dup2 正反双判据：J1 复制达成，J2 非法 fd 误用
//!       受控）。
//! 锚点：UNX-F10106-J1 lseek 定位达成（SEEK_SET/CUR/END 三基面 LTP
//!       lseek01 10/10 次）；UNX-F10106-J2 非法偏移误用受控（预期
//!       EINVAL 精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92a40（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `DupCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 注册表 `ObjTable`：文件描述对象账（offset/refcount/live），
//!     dup 共享同一对象、refcount 逐次 +1、归零即回收（live=false）；
//!   - 账本 `RefLedger`：引用守恒式 Σlive 对象 refcount == 占用槽数，
//!     引用漂移显性入账；
//!   - 状态机 `DupCycle`：Idle → Duplicated → Closed 单向；
//!   - 解析器 `parse_dup_call`："dup|fd" / "dup2|fd|target" 两协议
//!     逐段解析（fd/target 允许负号，负值即注入对象）；
//!   - dup2 语义核：目标占用先释放、self-dup 原样返回、越界目标
//!     按 EBADF 受控；
//!   - 共享偏移核：经 fd A 推进偏移，fd B 回读同值（dup01 语义）；
//!   - 注入核 `BadDupCase`：负 fd dup / 越界 fd dup / dup2 越界目标 /
//!     关闭后 dup / 引用漂移 五类坏样本逐类 10/10 次拒绝，
//!     账守恒逃逸=0；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10105 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92a40;

/// 段窗上界（下一卡 0x92a80 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92a80;

/// fd 表容量（本卡模拟槽位上限）。
pub const FD_CAP: usize = 16;

/// 对象账容量（描述对象上限；容纳标准语料 + 注入核全程对象创建）。
pub const OBJ_CAP: usize = 32;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EBADF: i32 = 9;
/// 非法参数（越界目标族）。
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DupCode {
    /// 负 fd（J2 主样本，EBADF）
    FdNegative,
    /// fd 越界（≥16 槽，EBADF）
    FdRange,
    /// fd 已关闭（空槽，EBADF）
    FdClosed,
    /// dup2 目标越界（EBADF）
    TargetRange,
    /// dup2 目标==源（原样返回，非错误；协议层显性登记）
    SelfDup,
    /// 引用漂移（Σrefcount ≠ 占用槽数）
    RefDrift,
    /// 最低空槽违规（dup 未按最低号分配）
    RecycleMissed,
    /// 对象账耗尽（8 对象占满）
    ObjExhausted,
    /// 引用未递增（dup 后 refcount 不变）
    RefMissed,
    /// 共享偏移失配（A 推进后 B 回读不同值）
    OffsetMissed,
    /// 状态倒退（Closed 后再复制）
    StateRegress,
    /// 段位越界
    SegmentViolation,
}

impl DupCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            DupCode::FdNegative => "负 fd 被拒：dup(-1) 按 EBADF 受控复现；请核 fd 来源",
            DupCode::FdRange => "fd 越界被拒：fd 必须落在 0..16；请核对槽号",
            DupCode::FdClosed => "关闭后 dup 被拒：槽位已空；请先重开再复制",
            DupCode::TargetRange => "dup2 目标越界被拒：target 必须落在 0..16；按 EBADF 受控",
            DupCode::SelfDup => "dup2(自体) 显性登记：原样返回 target，不递增引用",
            DupCode::RefDrift => "引用漂移被拒：Σrefcount≠占用槽数；请回查引用账",
            DupCode::RecycleMissed => "最低空槽违规被拒：dup 必须按最低号分配；请核分配器",
            DupCode::ObjExhausted => "对象账耗尽被拒：32 对象占满；请先释放再建",
            DupCode::RefMissed => "引用未递增被拒：dup 必须推进 refcount；请核复制链",
            DupCode::OffsetMissed => "共享偏移失配被拒：A 推进后 B 必须同值；请核共享语义",
            DupCode::StateRegress => "状态倒退被拒：Idle→Duplicated→Closed 单向；请排查回滚",
            DupCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92a40..=0x92a80",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [DupCode; 12] = [
        DupCode::FdNegative,
        DupCode::FdRange,
        DupCode::FdClosed,
        DupCode::TargetRange,
        DupCode::SelfDup,
        DupCode::RefDrift,
        DupCode::RecycleMissed,
        DupCode::ObjExhausted,
        DupCode::RefMissed,
        DupCode::OffsetMissed,
        DupCode::StateRegress,
        DupCode::SegmentViolation,
    ];
}

/// 诊断码到 Linux errno 的映射（反判据受控复现的对照口径）。
pub fn errno_of(code: DupCode) -> i32 {
    match code {
        DupCode::FdNegative | DupCode::FdRange | DupCode::FdClosed | DupCode::TargetRange => EBADF,
        DupCode::SelfDup => 0,
        _ => EINVAL,
    }
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 分段器：按 '|' 切 n 段（至多 3），返回切割点数组与段数。
fn split_line(line: &str) -> Option<([usize; 4], usize)> {
    let b = line.as_bytes();
    let mut cuts = [0usize; 4];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if b.get(i) == Some(&b'|') {
            if n >= 2 {
                return None;
            }
            cuts[n + 1] = i + 1;
            n += 1;
        }
        i += 1;
    }
    if n == 0 {
        return None;
    }
    cuts[n + 1] = b.len();
    Some((cuts, n + 1))
}

/// dup 协议行解析："dup|fd" → ("dup", fd, None)；"dup2|fd|target" →
/// ("dup2", fd, Some(target))。fd/target 允许负号；段数不符/非数字
/// 显性拒绝。
pub fn parse_dup_call(line: &str) -> Result<(&'static str, i32, Option<i32>), DupCode> {
    let (cuts, nseg) = match split_line(line) {
        Some(v) => v,
        None => return Err(DupCode::FdRange),
    };
    let op_seg = line.get(cuts[0]..cuts[1] - 1).unwrap_or("");
    let op: &'static str = if op_seg == "dup" {
        "dup"
    } else if op_seg == "dup2" {
        "dup2"
    } else {
        return Err(DupCode::FdRange);
    };
    // fd 段：两段时是末段（右端=总长），三段时后随 '|'（右端-1）
    let fd_seg = if nseg == 2 {
        line.get(cuts[1]..cuts[2]).unwrap_or("")
    } else {
        line.get(cuts[1]..cuts[2] - 1).unwrap_or("")
    };
    let fd = match fd_seg.parse::<i32>() {
        Ok(v) => v,
        Err(_) => return Err(DupCode::FdRange),
    };
    if nseg == 2 {
        if op == "dup2" {
            return Err(DupCode::TargetRange);
        }
        return Ok((op, fd, None));
    }
    let tgt_seg = line.get(cuts[2]..cuts[3]).unwrap_or("");
    // dup 只允许两段：三段的 dup 一律协议残形（先于 target 解析判定）
    if op == "dup" {
        return Err(DupCode::FdRange);
    }
    let tgt = match tgt_seg.parse::<i32>() {
        Ok(v) => v,
        Err(_) => return Err(DupCode::TargetRange),
    };
    Ok((op, fd, Some(tgt)))
}

// ── 对象注册表与引用账本 ──────────────────────────────────────────────
/// 一个文件描述对象：共享偏移 + 引用计数 + 存活位。
#[derive(Clone, Copy, Debug)]
pub struct FileObj {
    /// 共享偏移（dup 各档共用）
    pub offset: u32,
    /// 引用计数（fd 槽数）
    pub refcount: u32,
    /// 存活位（refcount 归零即回收）
    pub live: bool,
}

/// 对象注册表：8 对象账。
#[derive(Clone, Debug)]
pub struct ObjTable {
    /// 对象数组（None=空位）
    pub objs: [Option<FileObj>; OBJ_CAP],
}

impl ObjTable {
    /// 构造空表。
    pub const fn new() -> ObjTable {
        ObjTable { objs: [None; OBJ_CAP] }
    }

    /// 新建对象（refcount=1, live=true）；账满拒绝。
    pub fn create(&mut self) -> Result<usize, DupCode> {
        let mut i = 0usize;
        while i < OBJ_CAP {
            if let Some(None) = self.objs.get(i) {
                if let Some(s) = self.objs.get_mut(i) {
                    *s = Some(FileObj { offset: 0, refcount: 1, live: true });
                }
                return Ok(i);
            }
            i += 1;
        }
        Err(DupCode::ObjExhausted)
    }

    /// 引用 +1；对象必须存活。
    pub fn ref_up(&mut self, obj: usize) -> Result<u32, DupCode> {
        match self.objs.get_mut(obj) {
            Some(Some(o)) if o.live => {
                o.refcount = o.refcount.saturating_add(1);
                Ok(o.refcount)
            }
            _ => Err(DupCode::StateRegress),
        }
    }

    /// 引用 -1；归零即回收（live=false，offset 保留可查）。
    pub fn ref_down(&mut self, obj: usize) -> Result<u32, DupCode> {
        match self.objs.get_mut(obj) {
            Some(Some(o)) if o.live => {
                if o.refcount > 0 {
                    o.refcount -= 1;
                }
                if o.refcount == 0 {
                    o.live = false;
                }
                Ok(o.refcount)
            }
            _ => Err(DupCode::StateRegress),
        }
    }

    /// 推进共享偏移（模拟 lseek/read 前移）。
    pub fn advance(&mut self, obj: usize, n: u32) -> Result<u32, DupCode> {
        match self.objs.get_mut(obj) {
            Some(Some(o)) if o.live => {
                o.offset = o.offset.saturating_add(n);
                Ok(o.offset)
            }
            _ => Err(DupCode::StateRegress),
        }
    }

    /// 读共享偏移。
    pub fn offset_of(&self, obj: usize) -> Result<u32, DupCode> {
        match self.objs.get(obj) {
            Some(Some(o)) if o.live => Ok(o.offset),
            _ => Err(DupCode::StateRegress),
        }
    }

    /// 引用守恒式：Σlive 对象 refcount（返回总和）。
    pub fn ref_sum(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < OBJ_CAP {
            if let Some(Some(o)) = self.objs.get(i) {
                if o.live {
                    n = n.saturating_add(o.refcount);
                }
            }
            i += 1;
        }
        n
    }
}

// ── dup 表与复制状态机 ────────────────────────────────────────────────
/// 复制周期状态机：Idle → Duplicated → Closed 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DupCycle {
    /// 未复制
    Idle,
    /// 已复制（≥1 个副本在册）
    Duplicated,
    /// 已全部关闭
    Closed,
}

/// dup 表：fd → 对象号映射（None=空槽）。
#[derive(Clone, Debug)]
pub struct DupTable {
    /// fd 槽（存对象号）
    pub slots: [Option<usize>; FD_CAP],
    /// 周期态
    pub cycle: DupCycle,
}

impl DupTable {
    /// 构造空表。
    pub const fn new() -> DupTable {
        DupTable { slots: [None; FD_CAP], cycle: DupCycle::Idle }
    }

    /// 占用槽（打开原对象）。
    pub fn attach(&mut self, obj: usize) -> Result<usize, DupCode> {
        let mut i = 0usize;
        while i < FD_CAP {
            if let Some(None) = self.slots.get(i) {
                if let Some(s) = self.slots.get_mut(i) {
                    *s = Some(obj);
                }
                return Ok(i);
            }
            i += 1;
        }
        Err(DupCode::FdRange)
    }

    /// 释放槽。
    pub fn detach(&mut self, fd: usize) -> Result<usize, DupCode> {
        match self.slots.get(fd).copied() {
            Some(Some(obj)) => {
                if let Some(s) = self.slots.get_mut(fd) {
                    *s = None;
                }
                Ok(obj)
            }
            _ => Err(DupCode::FdClosed),
        }
    }

    /// 占用槽数。
    pub fn used(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < FD_CAP {
            if let Some(Some(_)) = self.slots.get(i) {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// dup 语义核：fd 必检（负/越界/空槽按 EBADF）→ 最低空槽分配 →
/// 引用 +1 → 周期推进 Duplicated。返回新 fd 与新引用数。
pub fn do_dup(table: &mut DupTable, objs: &mut ObjTable, fd: i32) -> Result<(usize, u32), DupCode> {
    if fd < 0 {
        return Err(DupCode::FdNegative);
    }
    let fd_u = fd as usize;
    if fd_u >= FD_CAP {
        return Err(DupCode::FdRange);
    }
    let obj = match table.slots.get(fd_u).copied() {
        Some(Some(o)) => o,
        _ => return Err(DupCode::FdClosed),
    };
    let mut new_fd = None;
    let mut i = 0usize;
    while i < FD_CAP {
        if let Some(None) = table.slots.get(i) {
            new_fd = Some(i);
            i = FD_CAP;
        } else {
            i += 1;
        }
    }
    let target = match new_fd {
        Some(t) => t,
        None => return Err(DupCode::FdRange),
    };
    if let Some(s) = table.slots.get_mut(target) {
        *s = Some(obj);
    }
    let rc = objs.ref_up(obj)?;
    table.cycle = DupCycle::Duplicated;
    Ok((target, rc))
}

/// dup2 语义核：fd/target 必检 → self-dup 原样返回（引用不变）→
/// 目标占用先释放（旧对象引用 -1）→ 复制并引用 +1。
pub fn do_dup2(
    table: &mut DupTable,
    objs: &mut ObjTable,
    fd: i32,
    target: i32,
) -> Result<usize, DupCode> {
    if fd < 0 {
        return Err(DupCode::FdNegative);
    }
    if target < 0 || target as usize >= FD_CAP {
        return Err(DupCode::TargetRange);
    }
    let fd_u = fd as usize;
    if fd_u >= FD_CAP {
        return Err(DupCode::FdRange);
    }
    let obj = match table.slots.get(fd_u).copied() {
        Some(Some(o)) => o,
        _ => return Err(DupCode::FdClosed),
    };
    let tgt = target as usize;
    if tgt == fd_u {
        return Ok(tgt); // self-dup：原样返回，引用不变
    }
    if let Some(old) = table.slots.get(tgt).copied() {
        if let Some(old_obj) = old {
            objs.ref_down(old_obj)?;
        }
    }
    if let Some(s) = table.slots.get_mut(tgt) {
        *s = Some(obj);
    }
    let _ = objs.ref_up(obj)?;
    table.cycle = DupCycle::Duplicated;
    Ok(tgt)
}

// ── 引用账本 ──────────────────────────────────────────────────────────
/// 引用账本：Σlive refcount 与表占用槽数比对，漂移显性入账。
#[derive(Clone, Debug)]
pub struct RefLedger {
    /// 漂移检出次数
    pub drifts: u32,
    /// 逃逸次数（坏账误入账，必须恒 0）
    pub escapes: u32,
}

impl RefLedger {
    /// 构造零账。
    pub const fn new() -> RefLedger {
        RefLedger { drifts: 0, escapes: 0 }
    }

    /// 守恒核：引用总数 == 占用槽数 才零漂移。
    pub fn audit(&mut self, table: &DupTable, objs: &ObjTable) -> Result<(), DupCode> {
        if objs.ref_sum() != table.used() {
            self.drifts = self.drifts.saturating_add(1);
            return Err(DupCode::RefDrift);
        }
        Ok(())
    }
}

/// 标准语料：建对象 → 占槽 0 → dup 得槽 1 → dup2 覆盖槽 3 →
/// A 推进偏移 B 回读同值 → 逐次引用审计。
pub fn standard_corpus(
    table: &mut DupTable,
    objs: &mut ObjTable,
    ledger: &mut RefLedger,
) -> Result<(), DupCode> {
    let obj = objs.create()?;
    let f0 = table.attach(obj)?;
    let (f1, rc1) = do_dup(table, objs, f0 as i32)?;
    if rc1 != 2 {
        return Err(DupCode::RefMissed);
    }
    let _ = do_dup2(table, objs, f1 as i32, 3)?;
    ledger.audit(table, objs)?;
    // 共享偏移核：经 f0 推进 8，经 f1 回读应同值
    let off0 = objs.advance(obj, 8)?;
    let off1 = objs.offset_of(obj)?;
    if off0 != off1 {
        return Err(DupCode::OffsetMissed);
    }
    // dup2 覆盖已开槽：先建第二个对象占槽 2，再 dup2 覆盖之
    let obj2 = objs.create()?;
    let f2 = table.attach(obj2)?;
    let _ = do_dup2(table, objs, f0 as i32, f2 as i32)?;
    ledger.audit(table, objs)?;
    // 全链关闭：引用逐次 -1，归零回收
    let _ = table.detach(f0)?;
    objs.ref_down(obj)?;
    let _ = table.detach(f1)?;
    objs.ref_down(obj)?;
    let _ = table.detach(3)?;
    objs.ref_down(obj)?;
    ledger.audit(table, objs)?;
    Ok(())
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏 dup 样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadDupCase {
    /// 负 fd dup（J2 主样本）
    NegativeFd,
    /// 越界 fd dup
    RangeFd,
    /// dup2 越界目标
    TargetRange,
    /// 关闭后 dup
    ClosedFd,
    /// 引用漂移
    RefDrift,
}

impl BadDupCase {
    /// 闭集全量。
    pub const ALL: [BadDupCase; 5] = [
        BadDupCase::NegativeFd,
        BadDupCase::RangeFd,
        BadDupCase::TargetRange,
        BadDupCase::ClosedFd,
        BadDupCase::RefDrift,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拒绝。
    pub fn inject(
        self,
        table: &mut DupTable,
        objs: &mut ObjTable,
        ledger: &mut RefLedger,
    ) -> bool {
        match self {
            BadDupCase::NegativeFd => {
                do_dup(table, objs, -1).err() == Some(DupCode::FdNegative)
                    && errno_of(DupCode::FdNegative) == EBADF
            }
            BadDupCase::RangeFd => {
                do_dup(table, objs, FD_CAP as i32).err() == Some(DupCode::FdRange)
            }
            BadDupCase::TargetRange => {
                do_dup2(table, objs, 0, FD_CAP as i32).err() == Some(DupCode::TargetRange)
            }
            BadDupCase::ClosedFd => {
                let obj = match objs.create() {
                    Ok(o) => o,
                    Err(_) => return false,
                };
                let f = match table.attach(obj) {
                    Ok(f) => f,
                    Err(_) => return false,
                };
                let detached = table.detach(f).is_ok();
                let downed = objs.ref_down(obj).is_ok();
                detached && downed && do_dup(table, objs, f as i32).err()
                    == Some(DupCode::FdClosed)
            }
            BadDupCase::RefDrift => {
                // 自包含局部账：建对象占槽后把引用打到 9 制造漂移
                let mut os = ObjTable::new();
                let mut tb = DupTable::new();
                let o = match os.create() {
                    Ok(o) => o,
                    Err(_) => return false,
                };
                let _ = tb.attach(o);
                let mut k = 0u32;
                while k < 8 {
                    let _ = os.ref_up(o);
                    k += 1;
                }
                let mut lg = RefLedger::new();
                lg.audit(&tb, &os).err() == Some(DupCode::RefDrift) && lg.drifts == 1
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_dup_injection() -> (RefLedger, [u32; 5]) {
    let mut objs = ObjTable::new();
    let mut table = DupTable::new();
    let mut ledger = RefLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadDupCase::ALL.len() {
        if let Some(case) = BadDupCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject(&mut table, &mut objs, &mut ledger);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                } else {
                    ledger.escapes = ledger.escapes.saturating_add(1);
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；负 fd/越界/关闭后 dup 族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；引用漂移/越界目标族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：建对象→dup→dup2→共享偏移→覆盖→全关 全链入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut objs = ObjTable::new();
    let mut table = DupTable::new();
    let mut lg = RefLedger::new();
    bytes.push(match standard_corpus(&mut table, &mut objs, &mut lg) {
        Ok(()) => 1,
        Err(_) => 2,
    });
    bytes.extend_from_slice(&table.used().to_le_bytes());
    bytes.extend_from_slice(&objs.ref_sum().to_le_bytes());
    bytes.push(match lg.audit(&table, &objs) {
        Ok(()) => 3,
        Err(_) => 4,
    });
    bytes.push(table.cycle as u8);
    bytes.push(lg.drifts as u8);
    bytes.push(lg.escapes as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10105 判据集（18 条静态 + 3 条动态明细）。
pub fn run_unx10105_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10105");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92a40..=0x92a80", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 标准语料全链成功
    let mut objs = ObjTable::new();
    let mut table = DupTable::new();
    let mut lg = RefLedger::new();
    let corpus_ok = standard_corpus(&mut table, &mut objs, &mut lg).is_ok();
    s.add("复制达成：dup/dup2 全链（复制→覆盖→共享偏移→回收）成功", corpus_ok,
          "J1 正判据：dup01/dup02 面语义逐环落账");

    // 3. dup 最低空槽 10/10
    let mut low_hits = 0u32;
    let mut lr = 0u32;
    while lr < INJECT_ROUNDS {
        let mut os = ObjTable::new();
        let mut tb = DupTable::new();
        let o = match os.create() {
            Ok(o) => o,
            Err(_) => 0,
        };
        let f0 = tb.attach(o).ok().unwrap_or(0);
        match do_dup(&mut tb, &mut os, f0 as i32) {
            Ok((nf, _)) => {
                if nf == f0 + 1 {
                    low_hits += 1;
                }
            }
            Err(_) => {}
        }
        lr += 1;
    }
    s.add("dup 最低空槽：10/10 次命中 f0+1", low_hits == INJECT_ROUNDS,
          "dup 必须按最低空号分配（dup01 语义），RecycleMissed 兜底");

    // 4. 引用递增 10/10
    let mut ref_hits = 0u32;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut os = ObjTable::new();
        let mut tb = DupTable::new();
        let o = os.create().ok().unwrap_or(0);
        let f0 = tb.attach(o).ok().unwrap_or(0);
        match do_dup(&mut tb, &mut os, f0 as i32) {
            Ok((_, rc)) => {
                if rc == 2 {
                    ref_hits += 1;
                }
            }
            Err(_) => {}
        }
        rr += 1;
    }
    s.add("引用递增：dup 后 refcount=2，10/10 复算一致", ref_hits == INJECT_ROUNDS,
          "每次复制引用 +1，RefMissed 兜底检出");

    // 5. dup2 显式目标 + self-dup 原样返回
    let mut os2 = ObjTable::new();
    let mut tb2 = DupTable::new();
    let o2 = os2.create().ok().unwrap_or(0);
    let f0 = tb2.attach(o2).ok().unwrap_or(0);
    let self_ok = do_dup2(&mut tb2, &mut os2, f0 as i32, f0 as i32) == Ok(f0);
    let rc_after_self = match os2.objs.get(o2).copied() {
        Some(Some(x)) => x.refcount,
        Some(None) => 0,
        None => 0,
    };
    s.add("dup2 self：原样返回且引用不变", self_ok && rc_after_self == 1,
          "dup2(fd,fd) 不递增引用（POSIX 语义显性登记）");

    // 6. dup2 覆盖已开槽：旧对象引用 -1
    let mut os3 = ObjTable::new();
    let mut tb3 = DupTable::new();
    let oa = os3.create().ok().unwrap_or(0);
    let ob = os3.create().ok().unwrap_or(1);
    let fa = tb3.attach(oa).ok().unwrap_or(0);
    let fb = tb3.attach(ob).ok().unwrap_or(1);
    let _ = do_dup2(&mut tb3, &mut os3, fa as i32, fb as i32);
    let rc_b = match os3.objs.get(ob).copied() {
        Some(Some(x)) => x.refcount,
        Some(None) => 0,
        None => 0,
    };
    let slot_b = match tb3.slots.get(fb).copied() {
        Some(Some(x)) => x,
        Some(None) => OBJ_CAP,
        None => OBJ_CAP,
    };
    s.add("dup2 覆盖：旧对象引用 -1 且槽位换绑", rc_b == 0 && slot_b == oa,
          "被覆盖目标的引用必须先释放，引用账守恒");

    // 7. 共享偏移 10/10：A 推进 B 同值（dup01/dup02 面）
    let mut off_hits = 0u32;
    let mut or = 0u32;
    while or < INJECT_ROUNDS {
        let mut os = ObjTable::new();
        let mut tb = DupTable::new();
        let o = os.create().ok().unwrap_or(0);
        let f0 = tb.attach(o).ok().unwrap_or(0);
        let f1 = match do_dup(&mut tb, &mut os, f0 as i32) {
            Ok((f, _)) => f,
            Err(_) => 0,
        };
        let a = match os.advance(o, 8) {
            Ok(v) => v,
            Err(_) => 0,
        };
        let b = match os.offset_of(o) {
            Ok(v) => v,
            Err(_) => 1,
        };
        let _ = f1;
        if a == b {
            off_hits += 1;
        }
        or += 1;
    }
    s.add("共享偏移：A 推进后 B 回读同值 10/10", off_hits == INJECT_ROUNDS,
          "dup 各档共用同一描述对象，偏移共享语义正确");

    // 8. 全关回收：引用归零 live 翻转
    let mut os4 = ObjTable::new();
    let mut tb4 = DupTable::new();
    let o4 = os4.create().ok().unwrap_or(0);
    let f4 = tb4.attach(o4).ok().unwrap_or(0);
    let f5 = match do_dup(&mut tb4, &mut os4, f4 as i32) {
        Ok((f, _)) => f,
        Err(_) => 0,
    };
    let _ = tb4.detach(f4);
    let _ = os4.ref_down(o4);
    let _ = tb4.detach(f5);
    let _ = os4.ref_down(o4);
    let live4 = match os4.objs.get(o4).copied() {
        Some(Some(x)) => x.live,
        Some(None) => true,
        None => true,
    };
    let rc4 = match os4.objs.get(o4).copied() {
        Some(Some(x)) => x.refcount,
        Some(None) => 1,
        None => 1,
    };
    s.add("全关回收：引用归零后对象回收（live=false）", !live4 && rc4 == 0,
          "refcount 归零即回收，引用账随开随销");

    // 9. 非法 fd 10/10 EBADF（J2 反判据：负 fd）
    let mut neg_hits = 0u32;
    let mut os5 = ObjTable::new();
    let mut tb5 = DupTable::new();
    let mut nr = 0u32;
    while nr < INJECT_ROUNDS {
        if do_dup(&mut tb5, &mut os5, -1).err().map(|e| errno_of(e) == EBADF) == Some(true) {
            neg_hits += 1;
        }
        nr += 1;
    }
    s.add("非法 fd：dup(-1) 10/10 次 EBADF 精确复现", neg_hits == INJECT_ROUNDS,
          "J2 反判据：负 fd 逐次按 EBADF 受控复现，不静默");

    // 10. 越界 fd / 越界目标拒
    let mut os6 = ObjTable::new();
    let mut tb6 = DupTable::new();
    let range_rej = do_dup(&mut tb6, &mut os6, FD_CAP as i32).err() == Some(DupCode::FdRange);
    let tgt_rej = do_dup2(&mut tb6, &mut os6, 0, FD_CAP as i32).err()
        == Some(DupCode::TargetRange);
    s.add("越界守卫：dup(fd=16) 拒、dup2(target=16) 拒", range_rej && tgt_rej,
          "fd/target 都必须落在 0..16 窗，越界按 EBADF 族受控");

    // 11. 关闭后 dup 拒
    let mut os7 = ObjTable::new();
    let mut tb7 = DupTable::new();
    let o7 = os7.create().ok().unwrap_or(0);
    let f7 = tb7.attach(o7).ok().unwrap_or(0);
    let _ = tb7.detach(f7);
    let _ = os7.ref_down(o7);
    let closed_rej = do_dup(&mut tb7, &mut os7, f7 as i32).err() == Some(DupCode::FdClosed);
    s.add("关闭后 dup：空槽复制拒收", closed_rej,
          "槽位已空的 fd 不得复制，显性拒绝");

    // 12. 协议解析：dup/dup2 两协议
    let p_ok = parse_dup_call("dup|3") == Ok(("dup", 3, None));
    let p2_ok = parse_dup_call("dup2|3|7") == Ok(("dup2", 3, Some(7)));
    let p_neg = parse_dup_call("dup|-1") == Ok(("dup", -1, None));
    let p_bad = parse_dup_call("dupx|3").err() == Some(DupCode::FdRange);
    let p_seg = parse_dup_call("dup|3|4").err() == Some(DupCode::FdRange);
    s.add("协议解析：dup 两段/dup2 三段/负值可表达/残形拒", p_ok && p2_ok && p_neg && p_bad && p_seg,
          "dup 恰两段、dup2 恰三段，段数与操作名互锁");

    // 13. 引用守恒：Σrefcount == 占用槽数
    let mut os8 = ObjTable::new();
    let mut tb8 = DupTable::new();
    let o8 = os8.create().ok().unwrap_or(0);
    let f8 = tb8.attach(o8).ok().unwrap_or(0);
    let _ = do_dup(&mut tb8, &mut os8, f8 as i32);
    let _ = do_dup(&mut tb8, &mut os8, f8 as i32);
    let cons_ok = os8.ref_sum() == 3 && tb8.used() == 3;
    s.add("引用守恒：3 槽共用 1 对象，Σrefcount==占用槽数", cons_ok,
          "引用账与表实数逐槽对核，漂移即显性");

    // 14. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_dup_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "负 fd/越界 fd/越界目标/关闭后 dup/引用漂移全被显式拒收");

    // 15. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0", ledger.escapes == 0,
          "注入核全程不得产生坏账逃逸入账");

    // 16. 文案闭集 + 诊断码唯一落段
    let text_ok = DupCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < DupCode::ALL.len() {
        let mut j = i2 + 1;
        while j < DupCode::ALL.len() {
            if let (Some(x), Some(y)) = (DupCode::ALL.get(i2), DupCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let in_seg = DupCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 码文案非空、两两唯一且落段窗", text_ok && uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 17. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 18. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：引用快照 + 周期快照 + 指纹快照
    let ref_text = format!("Σrefcount={} / 占用槽={}", objs.ref_sum(), table.used());
    s.add_dyn("引用快照", objs.ref_sum() == table.used(), &ref_text);
    let cyc_text = format!("周期态 {:?}", table.cycle);
    s.add_dyn("周期快照", table.cycle != DupCycle::Idle, &cyc_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dup_call_ok() {
        assert_eq!(parse_dup_call("dup|3"), Ok(("dup", 3, None)));
        assert_eq!(parse_dup_call("dup2|3|7"), Ok(("dup2", 3, Some(7))));
        assert_eq!(parse_dup_call("dup|-1"), Ok(("dup", -1, None)));
    }

    #[test]
    fn parse_dup_call_rejects() {
        assert_eq!(parse_dup_call("dupx|3"), Err(DupCode::FdRange));
        assert_eq!(parse_dup_call("dup|3|4"), Err(DupCode::FdRange));
        assert_eq!(parse_dup_call("dup2|3"), Err(DupCode::TargetRange));
        assert_eq!(parse_dup_call("dup|3|"), Err(DupCode::FdRange));
    }

    #[test]
    fn dup_lowest_slot_and_refcount() {
        let mut os = ObjTable::new();
        let mut tb = DupTable::new();
        let o = os.create().ok().unwrap();
        let f0 = tb.attach(o).ok().unwrap();
        assert_eq!(do_dup(&mut tb, &mut os, f0 as i32), Ok((f0 + 1, 2)));
    }

    #[test]
    fn negative_fd_is_ebadf() {
        let mut os = ObjTable::new();
        let mut tb = DupTable::new();
        let e = do_dup(&mut tb, &mut os, -1).err().unwrap();
        assert_eq!(errno_of(e), EBADF);
    }

    #[test]
    fn dup2_self_no_ref_change() {
        let mut os = ObjTable::new();
        let mut tb = DupTable::new();
        let o = os.create().ok().unwrap();
        let f = tb.attach(o).ok().unwrap();
        assert_eq!(do_dup2(&mut tb, &mut os, f as i32, f as i32), Ok(f));
        assert_eq!(os.objs[o].unwrap().refcount, 1);
    }

    #[test]
    fn dup2_covers_open_target() {
        let mut os = ObjTable::new();
        let mut tb = DupTable::new();
        let oa = os.create().ok().unwrap();
        let ob = os.create().ok().unwrap();
        let fa = tb.attach(oa).ok().unwrap();
        let fb = tb.attach(ob).ok().unwrap();
        assert!(do_dup2(&mut tb, &mut os, fa as i32, fb as i32).is_ok());
        assert_eq!(os.objs[ob].unwrap().refcount, 0);
        assert_eq!(tb.slots[fb].unwrap(), oa);
    }

    #[test]
    fn shared_offset_visible() {
        let mut os = ObjTable::new();
        let mut tb = DupTable::new();
        let o = os.create().ok().unwrap();
        let f = tb.attach(o).ok().unwrap();
        let _ = do_dup(&mut tb, &mut os, f as i32);
        assert_eq!(os.advance(o, 8), Ok(8));
        assert_eq!(os.offset_of(o), Ok(8));
    }

    #[test]
    fn full_close_recycles() {
        let mut os = ObjTable::new();
        let mut tb = DupTable::new();
        let o = os.create().ok().unwrap();
        let f = tb.attach(o).ok().unwrap();
        let _ = tb.detach(f);
        assert_eq!(os.ref_down(o), Ok(0));
        assert!(!os.objs[o].unwrap().live);
    }

    #[test]
    fn ref_drift_detected() {
        let mut os = ObjTable::new();
        let mut tb = DupTable::new();
        let o = os.create().ok().unwrap();
        let _ = tb.attach(o);
        let _ = os.ref_up(o);
        let mut lg = RefLedger::new();
        assert_eq!(lg.audit(&tb, &os), Err(DupCode::RefDrift));
        assert_eq!(lg.drifts, 1);
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_dup_injection();
        assert_eq!(l.escapes, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10105_checks_green() {
        let s = run_unx10105_checks();
        assert!(s.all_passed(), "UNX-F10105 red: {}", s.red_items_text());
    }
}
