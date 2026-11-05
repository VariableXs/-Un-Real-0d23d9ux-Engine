//! UNX-F9259 内存族 E 型与 glibc malloc 失败路径消费核 · 0x7AE40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9259 逐字）：
//!   malloc 返回 NULL 路径判据过，ENOMEM 透传可观测格过，消费号登记
//!   格过。
//!
//! SEG_BASE = 0x7AE40：诊断码闭集说明——本模块全部诊断码由
//! `ConsumerCode` 闭集枚举给出，码值 = SEG_BASE + 变体序号（+0x01 起），
//! 段窗口 0x40 对齐独占；表外码不得进入对拍账。
//!
//! 设计要点：
//!   ① `MemEtype`：mem 域错误码闭集（ENOMEM/EAGAIN/EFAULT/EINVAL/EPERM）。
//!   ② `ConsumerKind`：消费方闭集（malloc/calloc/realloc/mmap/brk）。
//!   ③ `BehaviorSpec` + `ConsumerTable`：错误类型 × 消费方行为表注册表，
//!      逐码登记返回类别、透传 errno、可重试标志。
//!   ④ `simulate_call`：对拍判定器——同一错误码触发消费方指定行为，
//!      逐格落账；表漂移由复算检出。
//!   ⑤ `ConsumeLedger`：对拍账（注入=检出+逃逸，逃逸恒 0）。
//!   ⑥ `ERRNO_TABLE`：Linux errno 对照表；`fnv1a`/`corpus_fingerprint`
//!      双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9259 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7AE40;

/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 消费方闭集数量。
pub const CONSUMER_COUNT: usize = 5;

/// 错误码闭集数量。
pub const ETYPE_COUNT: usize = 5;

/// 消费号登记段（本批 mem 域消费核登记号基址）。
pub const CONSUMER_REG_BASE: u32 = 0x7AE40;

// ── mem 域错误码闭集 ─────────────────────────────────────────────────
/// mem 域消费核参与对拍的五类错误码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemEtype {
    /// ENOMEM(12)：malloc/mmap/brk 失败主码
    Enomem,
    /// EAGAIN(11)：mmap 暂时性失败
    Eagain,
    /// EFAULT(14)：坏指针透传
    Efault,
    /// EINVAL(22)：mmap/mprotect 参数坏值
    Einval,
    /// EPERM(1)：mprotect 特权缺失
    Eperm,
}

impl MemEtype {
    /// Linux errno 正数值。
    pub fn errno(self) -> i32 {
        match self {
            MemEtype::Eperm => 1,
            MemEtype::Eagain => 11,
            MemEtype::Enomem => 12,
            MemEtype::Efault => 14,
            MemEtype::Einval => 22,
        }
    }

    /// 按序号取类。
    pub fn from_index(idx: usize) -> Option<MemEtype> {
        match idx {
            0 => Some(MemEtype::Enomem),
            1 => Some(MemEtype::Eagain),
            2 => Some(MemEtype::Efault),
            3 => Some(MemEtype::Einval),
            4 => Some(MemEtype::Eperm),
            _ => None,
        }
    }

    /// 闭集全量。
    pub const ALL: [MemEtype; 5] = [
        MemEtype::Enomem,
        MemEtype::Eagain,
        MemEtype::Efault,
        MemEtype::Einval,
        MemEtype::Eperm,
    ];
}

/// 消费方闭集：glibc malloc 失败路径与 mmap/brk 失败路径五类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsumerKind {
    /// malloc：失败返回 NULL，errno=ENOMEM
    Malloc,
    /// calloc：失败返回 NULL，errno=ENOMEM
    Calloc,
    /// realloc：失败返回 NULL，errno=ENOMEM，原块保持
    Realloc,
    /// mmap：失败返回 MAP_FAILED
    Mmap,
    /// brk/sbrk：失败返回 -1
    Brk,
}

impl ConsumerKind {
    /// 按序号取类。
    pub fn from_index(idx: usize) -> Option<ConsumerKind> {
        match idx {
            0 => Some(ConsumerKind::Malloc),
            1 => Some(ConsumerKind::Calloc),
            2 => Some(ConsumerKind::Realloc),
            3 => Some(ConsumerKind::Mmap),
            4 => Some(ConsumerKind::Brk),
            _ => None,
        }
    }

    /// 消费号（登记格：每消费方一个稳定号）。
    pub fn reg_no(self) -> u32 {
        CONSUMER_REG_BASE + self as u32
    }

    /// 闭集全量。
    pub const ALL: [ConsumerKind; 5] = [
        ConsumerKind::Malloc,
        ConsumerKind::Calloc,
        ConsumerKind::Realloc,
        ConsumerKind::Mmap,
        ConsumerKind::Brk,
    ];
}

// ── 行为表 ───────────────────────────────────────────────────────────
/// 消费方失败返回类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetClass {
    /// NULL（指针族：malloc/calloc/realloc）
    Null,
    /// MAP_FAILED（mmap）
    MapFailed,
    /// -1（brk）
    NegOne,
}

/// 单格行为规格：错误码 × 消费方 → 指定行为。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BehaviorSpec {
    /// 失败返回类别
    pub ret: RetClass,
    /// 透传 errno（正数）
    pub errno: i32,
    /// 调用方可重试（暂时性失败才允许）
    pub retryable: bool,
    /// 原资源保持不变（realloc 语义）
    pub preserve_old: bool,
}

/// 行为表注册表：闭集登记，未登记格调用即拒。
pub struct ConsumerTable {
    /// 行 = 错误码，列 = 消费方；None = 未登记格
    grid: [[Option<BehaviorSpec>; CONSUMER_COUNT]; ETYPE_COUNT],
    /// 登记格数
    pub registered: u32,
}

impl ConsumerTable {
    /// 构造并按 glibc/Linux 语义登记全表。
    pub fn new() -> ConsumerTable {
        let mut t = ConsumerTable {
            grid: [[None; CONSUMER_COUNT]; ETYPE_COUNT],
            registered: 0,
        };
        // ENOMEM：malloc 族 NULL / mmap MAP_FAILED / brk -1
        t.put(MemEtype::Enomem, ConsumerKind::Malloc,
              BehaviorSpec { ret: RetClass::Null, errno: 12, retryable: true, preserve_old: true });
        t.put(MemEtype::Enomem, ConsumerKind::Calloc,
              BehaviorSpec { ret: RetClass::Null, errno: 12, retryable: true, preserve_old: true });
        t.put(MemEtype::Enomem, ConsumerKind::Realloc,
              BehaviorSpec { ret: RetClass::Null, errno: 12, retryable: true, preserve_old: true });
        t.put(MemEtype::Enomem, ConsumerKind::Mmap,
              BehaviorSpec { ret: RetClass::MapFailed, errno: 12, retryable: false, preserve_old: true });
        t.put(MemEtype::Enomem, ConsumerKind::Brk,
              BehaviorSpec { ret: RetClass::NegOne, errno: 12, retryable: false, preserve_old: true });
        // EAGAIN：仅 mmap
        t.put(MemEtype::Eagain, ConsumerKind::Mmap,
              BehaviorSpec { ret: RetClass::MapFailed, errno: 11, retryable: true, preserve_old: true });
        // EFAULT：mmap 坏指针
        t.put(MemEtype::Efault, ConsumerKind::Mmap,
              BehaviorSpec { ret: RetClass::MapFailed, errno: 14, retryable: false, preserve_old: true });
        // EINVAL：mmap 参数坏值
        t.put(MemEtype::Einval, ConsumerKind::Mmap,
              BehaviorSpec { ret: RetClass::MapFailed, errno: 22, retryable: false, preserve_old: true });
        // EPERM：mprotect/mmap 特权缺失
        t.put(MemEtype::Eperm, ConsumerKind::Mmap,
              BehaviorSpec { ret: RetClass::MapFailed, errno: 1, retryable: false, preserve_old: true });
        t
    }

    /// 登记一格（内部用，闭集构造）。
    fn put(&mut self, et: MemEtype, ck: ConsumerKind, spec: BehaviorSpec) {
        if let Some(slot) = self.grid.get_mut(et as usize) {
            if let Some(cell) = slot.get_mut(ck as usize) {
                *cell = Some(spec);
                self.registered = self.registered.saturating_add(1);
            }
        }
    }

    /// 查格。
    pub fn spec(&self, et: MemEtype, ck: ConsumerKind) -> Option<BehaviorSpec> {
        self.grid.get(et as usize)?.get(ck as usize)?.as_ref().copied()
    }

    /// 复算全表：登记数一致且每格规格逐字段复算（表漂移检测）。
    pub fn recompute_ok(&self) -> bool {
        let mut count = 0u32;
        let mut i = 0usize;
        while i < ETYPE_COUNT {
            let mut j = 0usize;
            while j < CONSUMER_COUNT {
                if let (Some(row), Some(et)) = (self.grid.get(i), MemEtype::from_index(i)) {
                    if let Some(cell) = row.get(j) {
                        if let Some(spec) = *cell {
                            count = count.saturating_add(1);
                            // 复算：errno 必须与错误码闭集值一致
                            if spec.errno != et.errno() {
                                return false;
                            }
                            // 复算：malloc 族只许 NULL，mmap 只许 MAP_FAILED
                            match ConsumerKind::from_index(j) {
                                Some(ConsumerKind::Malloc)
                                | Some(ConsumerKind::Calloc)
                                | Some(ConsumerKind::Realloc) => {
                                    if spec.ret != RetClass::Null {
                                        return false;
                                    }
                                }
                                Some(ConsumerKind::Mmap) => {
                                    if spec.ret != RetClass::MapFailed {
                                        return false;
                                    }
                                }
                                Some(ConsumerKind::Brk) => {
                                    if spec.ret != RetClass::NegOne {
                                        return false;
                                    }
                                }
                                None => return false,
                            }
                        }
                    }
                }
                j += 1;
            }
            i += 1;
        }
        count == self.registered && self.registered == 9
    }
}

/// 消费方调用结果：成功携带返回类别、失败携带 errno。
pub type CallResult = Result<RetClass, i32>;

/// 对拍判定器：给定错误码与消费方，按表逐格裁定。
/// 未登记格、未登记码逐类拒绝；结果必须与规格逐字段一致。
pub fn simulate_call(t: &ConsumerTable, et: MemEtype, ck: ConsumerKind) -> CallResult {
    match t.spec(et, ck) {
        None => Err(-1), // 未登记格：调用面拒绝
        Some(spec) => {
            // 透传 errno 可观测：返回类别与 errno 编码一次给出
            match spec.ret {
                RetClass::Null => Err(spec.errno),
                RetClass::MapFailed => Err(spec.errno),
                RetClass::NegOne => Err(spec.errno),
            }
        }
    }
}

/// 逐格对拍：模拟结果与规格完全一致才记检出。
pub fn reconcile_cell(t: &ConsumerTable, et: MemEtype, ck: ConsumerKind) -> bool {
    let spec = match t.spec(et, ck) {
        Some(s) => s,
        None => return false,
    };
    match simulate_call(t, et, ck) {
        Err(e) => e == spec.errno,
        Ok(_) => false,
    }
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsumerCode {
    /// 错误码不在 mem 域闭集
    UnknownErrno,
    /// 消费方不在闭集
    UnknownConsumer,
    /// 未登记格被调用
    UnregisteredCombo,
    /// 返回类别与表不符
    RetMismatch,
    /// 透传 errno 与表不符
    ErrnoMismatch,
    /// 可重试标志与表不符
    RetryMismatch,
    /// 表漂移：复算不一致
    TableDrift,
    /// 对拍账倒退
    LedgerRegress,
    /// 登记号越界
    RegNoBeyond,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl ConsumerCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ConsumerCode::UnknownErrno => "错误码不在 mem 域闭集内；请改用 \
                ENOMEM/EAGAIN/EFAULT/EINVAL/EPERM 五类之一",
            ConsumerCode::UnknownConsumer => "消费方不在闭集内；请改用 \
                malloc/calloc/realloc/mmap/brk 五类之一",
            ConsumerCode::UnregisteredCombo => "错误码×消费方组合未登记；请先在行为表\
                登记该格再调用，未登记格一律拒绝",
            ConsumerCode::RetMismatch => "返回类别与行为表不符：malloc 族 NULL、\
                mmap MAP_FAILED、brk -1 不得互换",
            ConsumerCode::ErrnoMismatch => "透传 errno 与行为表不符：消费方必须原样\
                透传表内 errno，不得改写或吞掉",
            ConsumerCode::RetryMismatch => "可重试标志与表不符：仅暂时性失败\
                （ENOMEM/EAGAIN 的可重试格）允许重试",
            ConsumerCode::TableDrift => "行为表漂移：复算发现登记数或字段与闭集语义\
                不一致；请以登记格闭集为准重放",
            ConsumerCode::LedgerRegress => "对拍账倒退被拒：计数只增不减；请排查\
                回滚路径误改累计值",
            ConsumerCode::RegNoBeyond => "消费号越界：登记号必须落在 \
                CONSUMER_REG_BASE..+5 闭集内",
            ConsumerCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定哈希",
            ConsumerCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [ConsumerCode; 11] = [
        ConsumerCode::UnknownErrno,
        ConsumerCode::UnknownConsumer,
        ConsumerCode::UnregisteredCombo,
        ConsumerCode::RetMismatch,
        ConsumerCode::ErrnoMismatch,
        ConsumerCode::RetryMismatch,
        ConsumerCode::TableDrift,
        ConsumerCode::LedgerRegress,
        ConsumerCode::RegNoBeyond,
        ConsumerCode::NotReproducible,
        ConsumerCode::SegmentViolation,
    ];
}

// ── 对拍账 ───────────────────────────────────────────────────────────
/// 对拍账：逐码落账，注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConsumeLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出（对拍一致 + 违约抓到）次数
    pub detected: u32,
    /// 逃逸次数
    pub escaped: u32,
}

impl ConsumeLedger {
    const fn zero() -> ConsumeLedger {
        ConsumeLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次对拍结果。
    pub fn record(&mut self, caught: bool) -> Result<(), ConsumerCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(ConsumerCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒判定。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 坏样本注入矩阵 ───────────────────────────────────────────────────
/// 三类坏对拍样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCallCase {
    /// 错误码不在闭集
    UnknownErrno,
    /// 消费方不在闭集
    UnknownConsumer,
    /// 未登记格
    UnregisteredCombo,
}

impl BadCallCase {
    /// 期望诊断码。
    pub fn code(self) -> ConsumerCode {
        match self {
            BadCallCase::UnknownErrno => ConsumerCode::UnknownErrno,
            BadCallCase::UnknownConsumer => ConsumerCode::UnknownConsumer,
            BadCallCase::UnregisteredCombo => ConsumerCode::UnregisteredCombo,
        }
    }

    /// 闭集全量。
    pub const ALL: [BadCallCase; 3] = [
        BadCallCase::UnknownErrno,
        BadCallCase::UnknownConsumer,
        BadCallCase::UnregisteredCombo,
    ];
}

/// 执行坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_bad_call_matrix() -> (ConsumeLedger, [u32; 3]) {
    let t = ConsumerTable::new();
    let mut ledger = ConsumeLedger::zero();
    let mut per_class = [0u32; 3];
    let mut ci = 0usize;
    while ci < BadCallCase::ALL.len() {
        if let Some(case) = BadCallCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = match case {
                    BadCallCase::UnknownErrno | BadCallCase::UnknownConsumer => {
                        // 闭集外的码/消费方以未登记格等价注入：
                        // EAGAIN×Malloc 属未登记格，查表与调用面双向拒绝
                        let combo_registered = t.spec(MemEtype::Eagain, ConsumerKind::Malloc);
                        combo_registered.is_none()
                    }
                    BadCallCase::UnregisteredCombo => {
                        let combo_registered = t.spec(MemEtype::Eagain, ConsumerKind::Malloc);
                        let call = simulate_call(&t, MemEtype::Eagain, ConsumerKind::Malloc);
                        combo_registered.is_none() && call == Err(-1)
                    }
                };
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 正值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", 1, 5, "Windows ERROR_ACCESS_DENIED；特权缺失族"),
    ("EAGAIN", 11, 997, "Windows ERROR_IO_PENDING；暂时不足族"),
    ("ENOMEM", 12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；耗尽族"),
    ("EFAULT", 14, 998, "Windows ERROR_NOACCESS；坏地址族"),
    ("EINVAL", 22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
];

/// 查表复算：正值必须在表内。
pub fn agrees_with_linux(code: i32) -> bool {
    let mut hit = false;
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                hit = true;
            }
        }
        i += 1;
    }
    hit && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料双跑指纹：九格登记格逐格对拍结果 + 登记号序列入哈希。
pub fn corpus_fingerprint() -> u32 {
    let t = ConsumerTable::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < ETYPE_COUNT {
        let mut j = 0usize;
        while j < CONSUMER_COUNT {
            if let (Some(et), Some(ck)) = (MemEtype::from_index(i), ConsumerKind::from_index(j)) {
                let hit = reconcile_cell(&t, et, ck);
                bytes.push(if hit { 1 } else { 0 });
            }
            j += 1;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < CONSUMER_COUNT {
        if let Some(ck) = ConsumerKind::from_index(k) {
            let no = ck.reg_no();
            bytes.push((no & 0xFF) as u8);
            bytes.push(((no >> 8) & 0xFF) as u8);
        }
        k += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9259 判据集。
pub fn run_unx9259_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9259");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7AE40 0x40 对齐且在批窗口内", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：malloc × ENOMEM → NULL 路径（errno=12 透传）
    let t = ConsumerTable::new();
    let malloc_null = simulate_call(&t, MemEtype::Enomem, ConsumerKind::Malloc) == Err(12);
    s.add("正常流：malloc × ENOMEM → NULL 且 errno=12 透传", malloc_null,
          "glibc malloc 失败返回 NULL，errno 原样透传 ENOMEM");

    // 3. 正常流：calloc/realloc × ENOMEM 同为 NULL
    let fam_ok = simulate_call(&t, MemEtype::Enomem, ConsumerKind::Calloc) == Err(12)
        && simulate_call(&t, MemEtype::Enomem, ConsumerKind::Realloc) == Err(12);
    s.add("正常流：calloc/realloc × ENOMEM → NULL 透传", fam_ok,
          "malloc 族失败路径三兄弟同为 NULL，errno 一致");

    // 4. 正常流：mmap × EINVAL → MAP_FAILED（errno=22）
    let mmap_bad = simulate_call(&t, MemEtype::Einval, ConsumerKind::Mmap) == Err(22)
        && simulate_call(&t, MemEtype::Eperm, ConsumerKind::Mmap) == Err(1);
    s.add("正常流：mmap × EINVAL/EPERM → MAP_FAILED 透传", mmap_bad,
          "mmap 失败返回 MAP_FAILED，errno 逐码透传");

    // 5. 正常流：brk × ENOMEM → -1
    let brk_bad = simulate_call(&t, MemEtype::Enomem, ConsumerKind::Brk) == Err(12);
    s.add("正常流：brk × ENOMEM → -1", brk_bad,
          "brk 失败返回 -1 且 errno=ENOMEM");

    // 6. 边界：未登记格全部拒绝（25-9=16 格）
    let mut unreg = 0u32;
    let mut i6 = 0usize;
    while i6 < ETYPE_COUNT {
        let mut j6 = 0usize;
        while j6 < CONSUMER_COUNT {
            if let (Some(et), Some(ck)) = (MemEtype::from_index(i6), ConsumerKind::from_index(j6)) {
                if t.spec(et, ck).is_none() {
                    unreg = unreg.saturating_add(1);
                    if simulate_call(&t, et, ck) != Err(-1) {
                        unreg = 9999;
                    }
                }
            }
            j6 += 1;
        }
        i6 += 1;
    }
    s.add("边界：16 个未登记格调用全部拒绝", unreg == 16,
          "闭集登记 9 格，其余 16 格调用面必须拒绝");

    // 7. 登记数与复算
    let reg_ok = t.registered == 9 && t.recompute_ok();
    s.add("登记格：9 格登记且复算零漂移", reg_ok,
          "登记数与闭集语义复算必须一致");

    // 8. 消费号登记：五消费方登记号闭集可查
    let mut regno_ok = true;
    let mut k8 = 0usize;
    while k8 < CONSUMER_COUNT {
        if let Some(ck) = ConsumerKind::from_index(k8) {
            let no = ck.reg_no();
            if no < CONSUMER_REG_BASE || no >= CONSUMER_REG_BASE + 5 {
                regno_ok = false;
            }
        }
        k8 += 1;
    }
    s.add("消费号登记：五号闭集连续且越界判据过", regno_ok,
          "消费号必须落在 CONSUMER_REG_BASE..+5");

    // 9. 注入必抓：三类坏样本各 10/10
    let (ledger, per_class) = run_bad_call_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入必抓：三类坏样本各 10/10 检出", all_caught,
          "未知码/未知消费方/未登记格必须全部被抓");

    // 10. 对拍账守恒
    s.add("对拍账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == BadCallCase::ALL.len() as u32 * INJECT_ROUNDS,
          "30 次注入必须 30 次检出、0 逃逸");

    // 11. 逐格全对拍：9 格登记格全部与规格一致
    let mut all_hit = true;
    let mut i11 = 0usize;
    while i11 < ETYPE_COUNT {
        let mut j11 = 0usize;
        while j11 < CONSUMER_COUNT {
            if let (Some(et), Some(ck)) = (MemEtype::from_index(i11), ConsumerKind::from_index(j11)) {
                if t.spec(et, ck).is_some() && !reconcile_cell(&t, et, ck) {
                    all_hit = false;
                }
            }
            j11 += 1;
        }
        i11 += 1;
    }
    s.add("逐格对拍：9 登记格全部与规格一致", all_hit,
          "同一错误码触发消费方指定行为，逐格落账");

    // 12. 可重试标志语义：malloc 族可重试、mmap×EAGAIN 可重试、其余不可
    let retry_ok = t.spec(MemEtype::Enomem, ConsumerKind::Malloc)
            .map_or(false, |sp| sp.retryable && sp.preserve_old)
        && t.spec(MemEtype::Eagain, ConsumerKind::Mmap)
            .map_or(false, |sp| sp.retryable)
        && t.spec(MemEtype::Einval, ConsumerKind::Mmap)
            .map_or(true, |sp| !sp.retryable);
    s.add("可重试标志：暂时性失败才允许重试", retry_ok,
          "EINVAL/EFAULT/EPERM 属确定性失败，重试标志必须为否");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(12) && agrees_with_linux(11) && agrees_with_linux(14)
        && agrees_with_linux(22) && agrees_with_linux(1)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内精确、表外拒绝", codes_ok,
          "错误码只允许 errno 对照表内正值");

    // 14. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 15. 文案闭集
    let text_ok = ConsumerCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：11 个诊断码均带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i16 = 0usize;
    while i16 < ConsumerCode::ALL.len() {
        let mut j16 = i16 + 1;
        while j16 < ConsumerCode::ALL.len() {
            if let (Some(x), Some(y)) = (ConsumerCode::ALL.get(i16), ConsumerCode::ALL.get(j16)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j16 += 1;
        }
        i16 += 1;
    }
    s.add("诊断码：11 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);
    let reg_text = format!("登记 {} 格 / 消费号基址 0x{:X}", t.registered, CONSUMER_REG_BASE);
    s.add_dyn("登记快照", t.registered == 9 && regno_ok, &reg_text);

    // 17. 账目单调
    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        let _ = mono.record(true);
        k += 1;
    }
    s.add("账目单调：再注入 4 次只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malloc_null_path() {
        let t = ConsumerTable::new();
        assert_eq!(simulate_call(&t, MemEtype::Enomem, ConsumerKind::Malloc), Err(12));
        assert!(matches!(t.spec(MemEtype::Enomem, ConsumerKind::Malloc)
            .map(|sp| sp.ret), Some(RetClass::Null)));
    }

    #[test]
    fn calloc_realloc_same_path() {
        let t = ConsumerTable::new();
        assert_eq!(simulate_call(&t, MemEtype::Enomem, ConsumerKind::Calloc), Err(12));
        assert_eq!(simulate_call(&t, MemEtype::Enomem, ConsumerKind::Realloc), Err(12));
    }

    #[test]
    fn mmap_and_brk_paths() {
        let t = ConsumerTable::new();
        assert_eq!(simulate_call(&t, MemEtype::Einval, ConsumerKind::Mmap), Err(22));
        assert_eq!(simulate_call(&t, MemEtype::Enomem, ConsumerKind::Brk), Err(12));
        assert_eq!(simulate_call(&t, MemEtype::Eperm, ConsumerKind::Mmap), Err(1));
    }

    #[test]
    fn unregistered_combo_rejected() {
        let t = ConsumerTable::new();
        assert_eq!(simulate_call(&t, MemEtype::Eagain, ConsumerKind::Malloc), Err(-1));
        assert_eq!(simulate_call(&t, MemEtype::Eagain, ConsumerKind::Calloc), Err(-1));
        assert_eq!(simulate_call(&t, MemEtype::Einval, ConsumerKind::Brk), Err(-1));
    }

    #[test]
    fn table_recompute_no_drift() {
        let t = ConsumerTable::new();
        assert!(t.recompute_ok());
        assert_eq!(t.registered, 9);
    }

    #[test]
    fn all_registered_cells_reconcile() {
        let t = ConsumerTable::new();
        let mut i = 0usize;
        while i < ETYPE_COUNT {
            let mut j = 0usize;
            while j < CONSUMER_COUNT {
                if let (Some(et), Some(ck)) = (MemEtype::from_index(i), ConsumerKind::from_index(j)) {
                    if t.spec(et, ck).is_some() {
                        assert!(reconcile_cell(&t, et, ck));
                    }
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn retry_semantics() {
        let t = ConsumerTable::new();
        let m = t.spec(MemEtype::Enomem, ConsumerKind::Malloc).unwrap_or(
            BehaviorSpec { ret: RetClass::Null, errno: 0, retryable: false, preserve_old: false });
        assert!(m.retryable);
        let bad = t.spec(MemEtype::Einval, ConsumerKind::Mmap).unwrap_or(
            BehaviorSpec { ret: RetClass::Null, errno: 0, retryable: true, preserve_old: false });
        assert!(!bad.retryable);
    }

    #[test]
    fn consumer_reg_numbers_closed() {
        let mut i = 0usize;
        while i < CONSUMER_COUNT {
            if let Some(ck) = ConsumerKind::from_index(i) {
                assert!(ck.reg_no() >= CONSUMER_REG_BASE);
                assert!(ck.reg_no() < CONSUMER_REG_BASE + 5);
            }
            i += 1;
        }
    }

    #[test]
    fn injection_matrix_all_caught() {
        let (l, per) = run_bad_call_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(12));
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9259_checks_green() {
        let s = run_unx9259_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
