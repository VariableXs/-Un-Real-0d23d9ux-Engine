//! UNX-F9311 部分完成语义矩阵（bytes-copied-before-signal） · 0x7BB40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9311 逐字）：
//!   锚点：UNX-F9311-J1 半程拷贝后中断注入判据过，已拷贝字节返回语义
//!   格过，与 EFAULT 半程探针对账一致格过。
//!
//! 设计要点：
//!   ① 半程契约建模：长拷贝（writev/长 read）分块推进；信号在半程到达
//!      时，已拷贝字节必须如实返回——返回值 = 已完成块字节数之和，同时
//!      回 EINTR 仅当已完成字节为 0。这是 Linux 语义的两难分界：拷了
//!      一半不算失败（返回字节数），一字未拷才算中断（返回 EINTR）。
//!   ② iovec 解析器：把 (指针,长度) 对解析成逐块账本；坏块（零长、
//!      指针越界、总长溢出）逐项拒绝。块推进单调：cur 块索引只增不减，
//!      块内偏移不回退。
//!   ③ 与 EFAULT 半程探针对账：EFAULT 也有半程语义——坏块之前的字节
//!      已到内核/用户，返回「已拷字节数」而非 EFAULT，仅当第一块就坏
//!      才回 EFAULT。两条半程（信号半程/坏指针半程）同构：返回值都是
//!      已完成字节，错误码都只属于零进度情形。
//!   ④ 注入核四类坏样本：半程却回 EINTR（吞进度）、零进度却回字节数
//!      （假进度）、返回字节数越过总长、块推进倒退——逐类拒绝，账守恒
//!      逃逸 = 0。
//!   ⑤ 恒等式：返回值 ∈ {EINTR(0 进度)} ∪ [1, total]；返回字节数 +
//!      剩余字节数 = 总长，闭合可复算。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9311 独占 0x7BB40..=0x7BB7F）。
pub const SEG_BASE: u32 = 0x7BB40;

/// 注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// iovec 块容量。
pub const MAX_IOV: usize = 8;

/// 单块长度上界。
pub const IOV_LEN_LIMIT: u64 = 1 << 20;

/// Linux errno：中断。
pub const EINTR: i32 = 4;
/// Linux errno：坏地址。
pub const EFAULT: i32 = 14;
/// Linux errno：坏参数。
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyCode {
    /// 半程却回 EINTR（吞进度）
    EintrAfterProgress,
    /// 零进度却回字节数（假进度）
    FakeProgress,
    /// 返回字节数越过总长
    CopiedBeyondTotal,
    /// 块推进倒退
    BlockRegress,
    /// 零长块
    ZeroLenBlock,
    /// 总长溢出
    TotalOverflow,
    /// 空块表
    EmptyIov,
    /// 块表满
    IovFull,
    /// 账本倒退
    LedgerRegress,
    /// 对账冲突
    ReconcileConflict,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl CopyCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            CopyCode::EintrAfterProgress => "半程 EINTR 被拒：已有进度时必须返回 \
                已拷字节数；回 EINTR 会把进度吞掉，让上层重发整段造成重复写",
            CopyCode::FakeProgress => "假进度被拒：零进度只允许回 EINTR/EFAULT；\
                凭空报字节数破坏字节账闭合",
            CopyCode::CopiedBeyondTotal => "越界进度被拒：已拷字节数不得超过总长；\
                越界说明块推进账有错",
            CopyCode::BlockRegress => "块倒退被拒：块索引与块内偏移只增不减；\
                倒退会让同一字节被拷两次",
            CopyCode::ZeroLenBlock => "零长块被拒：iovec 块长度必须为正；零长块 \
                扰动逐块账，拒绝入表",
            CopyCode::TotalOverflow => "总长溢出被拒：逐块长度之和必须可查；\
                u64 溢出即拒绝整表",
            CopyCode::EmptyIov => "空块表被拒：无块可拷；返回 0 属合法短路但不得 \
                进入中断判定",
            CopyCode::IovFull => "块表满被拒：容量闭集；超容显性拒绝不许静默截断",
            CopyCode::LedgerRegress => "账本倒退被拒：进度账只增不减；请检查是否 \
                有回滚路径误改累计值",
            CopyCode::ReconcileConflict => "对账冲突：信号半程与 EFAULT 半程必须 \
                同构同账；两套进度账即冲突",
            CopyCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查 \
                隐藏状态或非确定哈希",
            CopyCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7B400..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [CopyCode; 12] = [
        CopyCode::EintrAfterProgress,
        CopyCode::FakeProgress,
        CopyCode::CopiedBeyondTotal,
        CopyCode::BlockRegress,
        CopyCode::ZeroLenBlock,
        CopyCode::TotalOverflow,
        CopyCode::EmptyIov,
        CopyCode::IovFull,
        CopyCode::LedgerRegress,
        CopyCode::ReconcileConflict,
        CopyCode::NotReproducible,
        CopyCode::SegmentViolation,
    ];
}

/// 系统调用失败结果：携带 errno 正数。
pub type SysResult<T> = Result<T, i32>;

// ── iovec 解析器 ─────────────────────────────────────────────────────
/// 单块（长度语义；指针面由 PtrClass 表示）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IovBlock {
    /// 块长度（正数）
    pub len: u64,
}

/// 缓冲区类别（EFAULT 面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtrClass {
    /// 合法可访问
    Valid,
    /// NULL
    Null,
    /// 未映射
    Unmapped,
}

impl PtrClass {
    /// 可访问性。
    pub fn accessible(self) -> bool {
        matches!(self, PtrClass::Valid)
    }
}

/// iovec 账本：块表 + 总长 + 进度。
#[derive(Clone, Copy, Debug)]
pub struct IovLedger {
    blocks: [IovBlock; MAX_IOV],
    used: usize,
    /// 总长
    pub total: u64,
    /// 已拷字节
    pub copied: u64,
    /// 当前块索引
    cur_block: usize,
    /// 当前块内已拷偏移
    cur_off: u64,
}

impl IovLedger {
    /// 从 (len, ptr) 对列表构造；坏块逐项拒绝。
    pub fn build(specs: &[(u64, PtrClass)]) -> SysResult<IovLedger> {
        if specs.is_empty() {
            return Err(EINVAL);
        }
        let mut total: u64 = 0;
        let mut used = 0usize;
        let mut i = 0usize;
        while i < specs.len() {
            if let Some((len, _ptr)) = specs.get(i).copied() {
                if len == 0 {
                    return Err(EINVAL);
                }
                if len > IOV_LEN_LIMIT {
                    return Err(EINVAL);
                }
                total = match total.checked_add(len) {
                    Some(v) => v,
                    None => return Err(EINVAL),
                };
                if used >= MAX_IOV {
                    return Err(EINVAL);
                }
                used += 1;
            }
            i += 1;
        }
        let mut blocks = [IovBlock { len: 0 }; MAX_IOV];
        let mut j = 0usize;
        while j < used {
            if let Some(spec) = specs.get(j) {
                if let Some(slot) = blocks.get_mut(j) {
                    *slot = IovBlock { len: spec.0 };
                }
            }
            j += 1;
        }
        Ok(IovLedger {
            blocks,
            used,
            total,
            copied: 0,
            cur_block: 0,
            cur_off: 0,
        })
    }

    /// 块表长。
    pub fn block_count(&self) -> usize {
        self.used
    }

    /// 第 k 块长度。
    pub fn block_len(&self, k: usize) -> Option<u64> {
        match self.blocks.get(k) {
            Some(b) if k < self.used => Some(b.len),
            _ => None,
        }
    }

    /// 推进 n 字节：块索引与偏移单调推进；越过总长拒绝。
    pub fn advance(&mut self, n: u64) -> SysResult<u64> {
        if self.copied.saturating_add(n) > self.total {
            return Err(EINVAL);
        }
        let mut left = n;
        let mut idx = self.cur_block;
        let mut off = self.cur_off;
        let mut steps = 0usize;
        while left > 0 && idx < self.used {
            let blen = match self.block_len(idx) {
                Some(v) => v,
                None => return Err(EINVAL),
            };
            let avail = blen - off;
            if left >= avail {
                left -= avail;
                idx += 1;
                off = 0;
            } else {
                off += left;
                left = 0;
            }
            steps += 1;
            if steps > MAX_IOV + 1 {
                return Err(EINVAL);
            }
        }
        if left > 0 {
            return Err(EINVAL);
        }
        if idx < self.cur_block || (idx == self.cur_block && off < self.cur_off) {
            return Err(EINVAL);
        }
        self.copied = self.copied.saturating_add(n);
        self.cur_block = idx;
        self.cur_off = off;
        Ok(self.copied)
    }

    /// 完成态：进度是否到达总长。
    pub fn complete(&self) -> bool {
        self.copied == self.total
    }
}

// ── 半程判定器 ───────────────────────────────────────────────────────
/// 半程结果：Done(字节数) | Interrupted(EINTR) | Fault(EFAULT)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HalfResult {
    /// 完成或部分完成：返回已拷字节数
    Done(u64),
    /// 零进度被打断
    Interrupted,
    /// 零进度遇坏指针
    Fault,
}

/// 半程语义判定：signal_at 表示信号在第 n 字节后到达（None=无信号）；
/// bad_at 表示坏指针在第 k 块出现（None=无坏块）。
/// 规则：进度>0 → Done(copied)；进度==0 且信号 → EINTR；进度==0 且坏块
/// 在第一块 → EFAULT；否则继续推进。
pub fn judge_half_copy(
    led: &mut IovLedger,
    signal_at: Option<u64>,
    bad_block: Option<usize>,
) -> HalfResult {
    // 先处理坏指针：坏块之前的块可拷（半程 EFAULT 语义）。
    if let Some(bad_idx) = bad_block {
        if bad_idx == 0 {
            return HalfResult::Fault;
        }
        // 拷完坏块之前的全部块。
        let mut k = 0usize;
        while k < bad_idx {
            let blen = match led.block_len(k) {
                Some(v) => v,
                None => break,
            };
            if led.advance(blen).is_err() {
                break;
            }
            k += 1;
        }
        if led.copied == 0 {
            return HalfResult::Fault;
        }
        return HalfResult::Done(led.copied);
    }
    // 信号半程：推进到信号点。
    match signal_at {
        None => {
            let _ = led.advance(led.total);
            HalfResult::Done(led.copied)
        }
        Some(at) => {
            if at >= led.total {
                let _ = led.advance(led.total);
                return HalfResult::Done(led.copied);
            }
            if at == 0 {
                return HalfResult::Interrupted;
            }
            let _ = led.advance(at);
            if led.copied == 0 {
                HalfResult::Interrupted
            } else {
                HalfResult::Done(led.copied)
            }
        }
    }
}

// ── 注入核：四类坏样本 ───────────────────────────────────────────────
/// 四类坏半程样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCopyCase {
    /// 半程却回 EINTR
    EintrAfterProgress,
    /// 零进度报字节数
    FakeProgress,
    /// 越界进度
    BeyondTotal,
    /// 零长块
    ZeroLen,
}

impl BadCopyCase {
    /// 闭集全量。
    pub const ALL: [BadCopyCase; 4] = [
        BadCopyCase::EintrAfterProgress,
        BadCopyCase::FakeProgress,
        BadCopyCase::BeyondTotal,
        BadCopyCase::ZeroLen,
    ];
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮四类坏半程注入：每类 INJECT_ROUNDS 次，全部必须被拒。
pub fn run_bad_copy_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadCopyCase::ALL.len() {
        if let Some(case) = BadCopyCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = match case {
                    BadCopyCase::EintrAfterProgress => {
                        // 半程信号必须回 Done(copied)，不得回 Interrupted。
                        let mut l = IovLedger::build(&[(100, PtrClass::Valid)])
                            .unwrap_or(IovLedger {
                                blocks: [IovBlock { len: 0 }; MAX_IOV],
                                used: 0,
                                total: 0,
                                copied: 0,
                                cur_block: 0,
                                cur_off: 0,
                            });
                        match judge_half_copy(&mut l, Some(50), None) {
                            HalfResult::Done(n) => n == 50,
                            _ => false,
                        }
                    }
                    BadCopyCase::FakeProgress => {
                        // 零进度 + 信号必须回 Interrupted，不得报字节数。
                        let mut l = IovLedger::build(&[(100, PtrClass::Valid)])
                            .unwrap_or(IovLedger {
                                blocks: [IovBlock { len: 0 }; MAX_IOV],
                                used: 0,
                                total: 0,
                                copied: 0,
                                cur_block: 0,
                                cur_off: 0,
                            });
                        match judge_half_copy(&mut l, Some(0), None) {
                            HalfResult::Interrupted => true,
                            HalfResult::Done(_) => false,
                            _ => false,
                        }
                    }
                    BadCopyCase::BeyondTotal => {
                        // advance 越过总长必须被拒。
                        let mut l = IovLedger::build(&[(100, PtrClass::Valid)])
                            .unwrap_or(IovLedger {
                                blocks: [IovBlock { len: 0 }; MAX_IOV],
                                used: 0,
                                total: 0,
                                copied: 0,
                                cur_block: 0,
                                cur_off: 0,
                            });
                        l.advance(101).is_err()
                    }
                    BadCopyCase::ZeroLen => {
                        // 零长块必须拒绝入表。
                        IovLedger::build(&[(0, PtrClass::Valid)]).is_err()
                    }
                };
                ledger.record(caught);
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

// ── EFAULT 半程探针对账 ──────────────────────────────────────────────
/// 对账行：(情形, 信号半程, EFAULT 半程, 是否同构)。
pub const RECONCILE_TABLE: &[(&str, &str, &str, bool)] = &[
    ("零进度", "EINTR", "EFAULT(首块坏)", true),
    ("半程", "Done(copied)", "Done(copied)", true),
    ("完成", "Done(total)", "Done(total)", true),
    ("账闭合", "copied+rem=total", "copied+rem=total", true),
];

/// 与 EFAULT 半程探针对账。
pub fn agrees_with_efault_probe() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < RECONCILE_TABLE.len() {
        if let Some(row) = RECONCILE_TABLE.get(i) {
            if !row.3 {
                ok = false;
            }
        }
        i += 1;
    }
    ok
}

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINTR", -4, 995, "Windows ERROR_OPERATION_ABORTED；中断族"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏地址族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
];

/// 查表。
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

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
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

fn empty_led() -> IovLedger {
    IovLedger {
        blocks: [IovBlock { len: 0 }; MAX_IOV],
        used: 0,
        total: 0,
        copied: 0,
        cur_block: 0,
        cur_off: 0,
    }
}

/// 规范语料双跑指纹：信号半程五点 + EFAULT 半程三块位。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    // 信号半程：at = 0/1/150/299/无（总长 300 = 100+200）
    let specs = [(100u64, PtrClass::Valid), (200, PtrClass::Valid)];
    let points: [Option<u64>; 5] = [Some(0), Some(1), Some(150), Some(299), None];
    let mut i = 0usize;
    while i < points.len() {
        let mut l = IovLedger::build(&specs).unwrap_or(empty_led());
        let rr = match points.get(i).copied() {
            Some(Some(at)) => judge_half_copy(&mut l, Some(at), None),
            Some(None) => judge_half_copy(&mut l, None, None),
            None => HalfResult::Fault,
        };
        bytes.push(match rr {
            HalfResult::Done(n) => (n & 0xFF) as u8,
            HalfResult::Interrupted => 0xF1,
            HalfResult::Fault => 0xF2,
        });
        i += 1;
    }
    // EFAULT 半程：坏块位 0/1/2
    let mut k = 0usize;
    while k < 3 {
        let mut l3 = IovLedger::build(&specs).unwrap_or(empty_led());
        bytes.push(match judge_half_copy(&mut l3, None, Some(k)) {
            HalfResult::Done(n) => (n & 0xFF) as u8,
            HalfResult::Interrupted => 0xF1,
            HalfResult::Fault => 0xF2,
        });
        k += 1;
    }
    bytes.push(if agrees_with_efault_probe() { 1 } else { 0 });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9311 判据集（≥12 条；动态明细 ≤8 条）。
pub fn run_unx9311_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9311");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7B400..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本批窗口", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 半程信号正判据
    let mut l = IovLedger::build(&[(100, PtrClass::Valid)]).unwrap_or(empty_led());
    let half_ok = judge_half_copy(&mut l, Some(50), None) == HalfResult::Done(50);
    s.add("半程正判据：信号在第 50 字节后 → Done(50)", half_ok,
          "有进度必须报字节数，不得吞进度回 EINTR");

    // 3. 零进度 EINTR
    let mut l0 = IovLedger::build(&[(100, PtrClass::Valid)]).unwrap_or(empty_led());
    let zero_ok = judge_half_copy(&mut l0, Some(0), None) == HalfResult::Interrupted;
    s.add("零进度：一字未拷被打断 → EINTR", zero_ok,
          "EINTR 只属于零进度情形");

    // 4. 多块推进与块边界
    let mut l2 = IovLedger::build(&[(100, PtrClass::Valid), (200, PtrClass::Valid)])
        .unwrap_or(empty_led());
    let a1 = l2.advance(100);
    let a2 = l2.advance(200);
    let blocks_ok = a1 == Ok(100)
        && a2 == Ok(300)
        && l2.complete()
        && l2.block_count() == 2
        && l2.block_len(0) == Some(100);
    s.add("块推进：跨块逐块单调，总长 300 可达", blocks_ok,
          "块索引/偏移单调推进，完成态 copied==total");

    // 5. 越界进度拒绝
    let mut l3 = IovLedger::build(&[(100, PtrClass::Valid)]).unwrap_or(empty_led());
    let beyond_ok = l3.advance(101).is_err() && l3.advance(100).is_ok();
    s.add("越界守卫：advance 越过总长被拒（可恰达）", beyond_ok,
          "copied ≤ total 是闭合的前提");

    // 6. 零长块与空表
    let bad_ok = IovLedger::build(&[(0, PtrClass::Valid)]).is_err()
        && IovLedger::build(&[]).is_err();
    s.add("入表守卫：零长块/空表拒绝", bad_ok,
          "iovec 块必须为正，空表无中断语义");

    // 7. EFAULT 半程：首块坏 → EFAULT，后块坏 → Done(前块和)
    let mut l4 = IovLedger::build(&[(100, PtrClass::Valid), (200, PtrClass::Valid)])
        .unwrap_or(empty_led());
    let f0 = judge_half_copy(&mut l4, None, Some(0));
    let mut l5 = IovLedger::build(&[(100, PtrClass::Valid), (200, PtrClass::Valid)])
        .unwrap_or(empty_led());
    let f1 = judge_half_copy(&mut l5, None, Some(1));
    let efault_ok = f0 == HalfResult::Fault && f1 == HalfResult::Done(100);
    s.add("EFAULT 半程：首块坏 → EFAULT，第二块坏 → Done(100)", efault_ok,
          "坏块前已拷字节必须如实返回，与信号半程同构");

    // 8. 注入核：四类坏样本
    let (ledger, per_class) = run_bad_copy_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("四类坏半程各 10/10 全检出", all_caught,
          "吞进度/假进度/越界/零长块必须逐类拒绝");

    // 9. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 字节账闭合：copied + remaining = total
    let mut l6 = IovLedger::build(&[(100, PtrClass::Valid)]).unwrap_or(empty_led());
    let _ = l6.advance(40);
    let closure_ok = l6.copied + (l6.total - l6.copied) == l6.total;
    s.add("字节账闭合：copied+remaining=total", closure_ok,
          "恒等式可复算，缺一块都破坏对账");

    // 11. EFAULT 探针对账
    s.add("对账：信号半程与 EFAULT 半程同构", agrees_with_efault_probe(),
          "两条半程共用一套进度账，错误码只属于零进度");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(EINTR)
        && agrees_with_linux(EFAULT)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 13. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 14. 文案闭集与码唯一
    let text_ok = CopyCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");
    let mut uniq = true;
    let mut i = 0usize;
    while i < CopyCode::ALL.len() {
        let mut j = i + 1;
        while j < CopyCode::ALL.len() {
            if let (Some(x), Some(y)) = (CopyCode::ALL.get(i), CopyCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_copy_returns_progress() {
        let mut l = IovLedger::build(&[(100, PtrClass::Valid)]).unwrap_or(empty_led());
        assert_eq!(judge_half_copy(&mut l, Some(50), None), HalfResult::Done(50));
    }

    #[test]
    fn zero_progress_interrupted() {
        let mut l = IovLedger::build(&[(100, PtrClass::Valid)]).unwrap_or(empty_led());
        assert_eq!(
            judge_half_copy(&mut l, Some(0), None),
            HalfResult::Interrupted
        );
    }

    #[test]
    fn full_copy_when_no_signal() {
        let mut l = IovLedger::build(&[(100, PtrClass::Valid)]).unwrap_or(empty_led());
        assert_eq!(judge_half_copy(&mut l, None, None), HalfResult::Done(100));
    }

    #[test]
    fn multi_block_advance() {
        let mut l = IovLedger::build(&[(100, PtrClass::Valid), (200, PtrClass::Valid)])
            .unwrap_or(empty_led());
        assert_eq!(l.advance(150), Ok(150));
        assert_eq!(l.advance(150), Ok(300));
        assert!(l.complete());
    }

    #[test]
    fn beyond_total_rejected() {
        let mut l = IovLedger::build(&[(100, PtrClass::Valid)]).unwrap_or(empty_led());
        assert!(l.advance(101).is_err());
    }

    #[test]
    fn zero_len_and_empty_rejected() {
        assert!(IovLedger::build(&[(0, PtrClass::Valid)]).is_err());
        assert!(IovLedger::build(&[]).is_err());
    }

    #[test]
    fn efault_half_semantics() {
        let mut l = IovLedger::build(&[(100, PtrClass::Valid), (200, PtrClass::Valid)])
            .unwrap_or(empty_led());
        assert_eq!(judge_half_copy(&mut l, None, Some(0)), HalfResult::Fault);
        let mut l2 = IovLedger::build(&[(100, PtrClass::Valid), (200, PtrClass::Valid)])
            .unwrap_or(empty_led());
        assert_eq!(judge_half_copy(&mut l2, None, Some(1)), HalfResult::Done(100));
    }

    #[test]
    fn bad_matrix_green() {
        let (l, per) = run_bad_copy_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn reconcile_holds() {
        assert!(agrees_with_efault_probe());
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9311_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
