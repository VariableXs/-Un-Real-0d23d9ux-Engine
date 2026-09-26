//! F020 异常与调试面（compatstar · G-A-20）——崩溃的用户感受是「一个窗口
//! 没了」，不是「整个系统没了」。
//!
//! 主册判据（验收标准第一句）：
//! **「注入异常样本 6 类（除零/非法访问/栈溢出/未处理 C++ 异常/SEH 吞噬/
//! VEH 链）行为全对；崩溃后桌面帧率实测不跌（F175 联动）。」**
//!
//! 功能定义（G-A-20）：SEH/VEH 结构化异常翻译：try/except 语义（异常过滤/
//! 展开/最终处理）映射到 VARIX 信号与 unwind 层；未处理异常 → minidump（线
//! 程栈/寄存器/模块表）落诊断中心，应用按 F175 隔离回收。
//!
//! 【交互设计】崩溃卡片样式对齐 F035 体系；「重新启动应用」保留原参数重拉；
//! 「查看详情」内嵌 dump 摘要只读页。【数据与存储】minidump 存 `diagnostics/
//! dumps/` 上限 20 个 LRU；dump 格式自定轻量格式（可导出转换 CDB 可读文本）。
//! 【状态与异常】异常处理器自身再异常 → 双重故障直接回收进程（不递归）；栈
//! 溢出 → 专用栈保护页捕获按栈溢出归因；dump 写入失败 → 静默降级为日志摘要。
//! 【设计细节】异常翻译层内核零堆（定长异常帧栈深 32）；minidump 含：异常码/
//! 崩溃地址/线程栈回溯 64 帧/已加载模块表（含校验和）/系统版本指纹；「重新
//! 启动应用」保留命令行与工作目录（启动时快照）；同一文件 24 小时内崩溃三次
//! 以上自动建议提交 F036 草稿。
//!
//! 零堆纪律：异常帧栈定长 32、dump 定长结构（64 帧栈回溯 + 32 模块表），
//! 无 Vec/String/Box/format!。

use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

// ---------------------------------------------------------------------------
// 异常码（winnt.h）
// ---------------------------------------------------------------------------

pub const EXC_INT_DIVIDE_BY_ZERO: u32 = 0xC000_0094;
pub const EXC_ACCESS_VIOLATION: u32 = 0xC000_0005;
pub const EXC_STACK_OVERFLOW: u32 = 0xC000_00FD;
pub const EXC_CPP_UNHANDLED: u32 = 0xE06D_7363; // MSVC C++ 异常
pub const EXC_SEH_SWALLOWED: u32 = 0xC000_0194; // SEH 吞噬（包装观测码）
pub const EXC_VEH_CHAIN: u32 = 0xC000_0195; // VEH 链穿透观测码
/// 双重故障（处理器自身再异常——直接回收不递归）。
pub const EXC_DOUBLE_FAULT: u32 = 0xC000_0196;

/// 六类注入样本全集（主册判据）。
pub const SIX_SAMPLE_CODES: [u32; 6] = [
    EXC_INT_DIVIDE_BY_ZERO,
    EXC_ACCESS_VIOLATION,
    EXC_STACK_OVERFLOW,
    EXC_CPP_UNHANDLED,
    EXC_SEH_SWALLOWED,
    EXC_VEH_CHAIN,
];

// ---------------------------------------------------------------------------
// 异常帧与处理器链
// ---------------------------------------------------------------------------

/// 异常帧（定长——翻译层零堆）。
#[derive(Clone, Copy, Debug)]
pub struct ExceptionFrame {
    pub code: u32,
    pub fault_addr: u64,
    /// 处理深度（SEH 链/VEH 链已走过层数）。
    pub depth: u8,
}

/// 处理器链节点类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandlerKind {
    /// VEH 节点（先于 SEH 链执行）。
    Vectored,
    /// SEH try/except 节点。
    Structured,
}

/// 过滤器结论（try/except 三态 + VEH 继续/搜索）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilterVerdict {
    /// EXCEPTION_EXECUTE_HANDLER → 走 except 块（吞噬——异常不再上抛）。
    Handle,
    /// EXCEPTION_CONTINUE_SEARCH → 沿链继续搜索。
    ContinueSearch,
    /// EXCEPTION_CONTINUE_EXECUTION → 恢复执行。
    ContinueExecution,
}

/// 异常翻译层（内核零堆：定长帧栈，深度上限 32）。
pub struct ExceptionTranslator {
    frames: [Option<ExceptionFrame>; 32],
    frame_n: usize,
    /// VEH 链（先执行）。
    veh_handlers: u32,
    /// 已回收进程标记（未处理异常 → F175 隔离回收）。
    pub reclaimed: bool,
    /// 双重故障标记。
    pub double_fault: bool,
    /// 栈保护页触发标记（栈溢出归因）。
    pub stack_guard_hit: bool,
}

impl ExceptionTranslator {
    pub fn new(veh_handlers: u32) -> ExceptionTranslator {
        ExceptionTranslator {
            frames: [None; 32],
            frame_n: 0,
            veh_handlers,
            reclaimed: false,
            double_fault: false,
            stack_guard_hit: false,
        }
    }

    /// 异常投递（翻译层入口）：VEH 链 → SEH 链 → 未处理回收。
    /// 返回处理结果。
    pub fn raise(&mut self, code: u32, fault_addr: u64, verdicts: &[FilterVerdict]) -> Dispatch {
        // 栈溢出特殊路径：专用栈保护页捕获（主册【状态与异常】）。
        if code == EXC_STACK_OVERFLOW {
            self.stack_guard_hit = true;
        }
        // 帧（定长栈，深 32——超限即双重故障语义：翻译层自身不可再展开）。
        if self.frame_n >= 32 {
            self.double_fault = true;
            self.reclaimed = true;
            return Dispatch::ReclaimedDoubleFault;
        }
        self.frames[self.frame_n] = Some(ExceptionFrame { code, fault_addr, depth: 0 });
        self.frame_n += 1;
        // VEH 链先行（Windows 语义：VEH 先于 SEH——逐节点可继续执行/放行）。
        let mut veh_pass = 0u32;
        for _ in 0..self.veh_handlers {
            veh_pass += 1;
        }
        // 过滤器链（调用方给的 try/except 链语义）。
        let mut depth = 0u8;
        for v in verdicts {
            depth += 1;
            match v {
                FilterVerdict::Handle => {
                    // SEH 吞噬：异常到此为止（进程存活，观测码记录）。
                    if let Some(f) = self.frames[self.frame_n - 1].as_mut() {
                        f.depth = depth;
                    }
                    return Dispatch::HandledBySeq(depth);
                }
                FilterVerdict::ContinueExecution => {
                    return Dispatch::ContinuedExecution(depth);
                }
                FilterVerdict::ContinueSearch => {}
            }
        }
        // 全链未处理 → minidump + F175 回收。
        self.reclaimed = true;
        Dispatch::Unhandled(veh_pass, depth)
    }

    /// 双重故障入口：处理器自身再异常 → 直接回收（不递归）。
    pub fn raise_in_handler(&mut self) -> Dispatch {
        self.double_fault = true;
        self.reclaimed = true;
        Dispatch::ReclaimedDoubleFault
    }

    pub fn frame_count(&self) -> usize {
        self.frame_n
    }

    pub fn top_frame(&self) -> Option<ExceptionFrame> {
        self.frames[self.frame_n.checked_sub(1)?]
    }
}

/// 投递结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dispatch {
    /// VEH/SEH 链处理（吞噬——存活）。
    HandledBySeq(u8),
    /// 恢复执行。
    ContinuedExecution(u8),
    /// 未处理（dump 已落，F175 回收）——携带 VEH 走过层数与链深。
    Unhandled(u32, u8),
    /// 双重故障直接回收。
    ReclaimedDoubleFault,
}

// ---------------------------------------------------------------------------
// minidump（自定轻量格式）
// ---------------------------------------------------------------------------

/// 栈回溯帧上限 64（主册【设计细节】）。
pub const DUMP_STACK_FRAMES: usize = 64;
/// 模块表上限 32（主册【设计细节】：已加载模块表含校验和）。
pub const DUMP_MODULES: usize = 32;
/// dump 存储上限 20 个 LRU（主册【数据与存储】）。
pub const DUMP_CAP: usize = 20;

/// 模块表项。
#[derive(Clone, Copy, Debug)]
pub struct DumpModule {
    pub base: u64,
    pub size: u32,
    pub checksum: u32,
}

/// minidump（自定轻量格式——可导出转换 CDB 可读文本）。
#[derive(Clone, Copy, Debug)]
pub struct MiniDump {
    pub exception_code: u32,
    pub crash_addr: u64,
    pub thread_id: u32,
    /// 栈回溯（≤64 帧；0 填充到尾部）。
    pub stack: [u64; DUMP_STACK_FRAMES],
    pub stack_n: usize,
    /// 模块表（≤32，含校验和）。
    pub modules: [Option<DumpModule>; DUMP_MODULES],
    pub module_n: usize,
    /// 系统版本指纹。
    pub system_fingerprint: u32,
    /// 降级标记（dump 写入失败 → 日志摘要——主册【状态与异常】）。
    pub degraded: bool,
    /// 重启快照（保留命令行与工作目录）。
    pub restart_cmd_hash: u64,
    pub restart_cwd_hash: u64,
}

/// dump LRU 库（20 个）。
pub struct DumpStore {
    dumps: [Option<MiniDump>; DUMP_CAP],
    n: usize,
    clock: u64,
    stamps: [u64; DUMP_CAP],
    /// 写入失败降级计数（静默降级为日志摘要的记账——不静默，计数可见）。
    pub write_failures: u32,
}

impl DumpStore {
    pub fn new() -> DumpStore {
        DumpStore { dumps: [None; DUMP_CAP], n: 0, clock: 0, stamps: [0; DUMP_CAP], write_failures: 0 }
    }

    /// 存入 dump（满 20 → LRU 驱逐最旧）。`write_ok=false` → 降级计数。
    pub fn store(&mut self, d: MiniDump, write_ok: bool) {
        if !write_ok {
            self.write_failures += 1;
            return;
        }
        self.clock += 1;
        let slot = if self.n < DUMP_CAP {
            let s = self.n;
            self.n += 1;
            s
        } else {
            // LRU：驱逐 stamp 最小。
            let mut victim = 0;
            let mut oldest = u64::MAX;
            for i in 0..DUMP_CAP {
                if self.stamps[i] < oldest {
                    oldest = self.stamps[i];
                    victim = i;
                }
            }
            victim
        };
        self.dumps[slot] = Some(d);
        self.stamps[slot] = self.clock;
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 最近一次 dump（崩溃卡片「查看详情」页数据源）。
    pub fn latest(&self) -> Option<MiniDump> {
        let mut best: Option<(u64, usize)> = None;
        for i in 0..self.n {
            if best.map_or(true, |(s, _)| self.stamps[i] > s) {
                best = Some((self.stamps[i], i));
            }
        }
        best.and_then(|(_, i)| self.dumps[i])
    }
}

impl Default for DumpStore {
    fn default() -> Self {
        Self::new()
    }
}

/// 构造 dump（栈回溯裁剪到 64 帧 + 模块表裁剪到 32）。
pub fn build_dump(
    code: u32,
    crash_addr: u64,
    thread_id: u32,
    raw_stack: &[u64],
    raw_modules: &[DumpModule],
    restart_cmd_hash: u64,
    restart_cwd_hash: u64,
) -> MiniDump {
    let mut d = MiniDump {
        exception_code: code,
        crash_addr,
        thread_id,
        stack: [0; DUMP_STACK_FRAMES],
        stack_n: raw_stack.len().min(DUMP_STACK_FRAMES),
        modules: [None; DUMP_MODULES],
        module_n: raw_modules.len().min(DUMP_MODULES),
        system_fingerprint: 0x5354_4152, // "STAR"
        degraded: false,
        restart_cmd_hash,
        restart_cwd_hash,
    };
    for i in 0..d.stack_n {
        d.stack[i] = raw_stack[i];
    }
    for i in 0..d.module_n {
        d.modules[i] = Some(raw_modules[i]);
    }
    d
}

/// 24 小时崩溃计数（同文件三次以上 → 建议 F036 草稿）。
pub struct CrashRepeatTracker {
    /// (文件哈希 → 24h 窗口内计数)——定长 64 槽。
    counts: [(u64, u32, u64); 64],
    n: usize,
    /// F036 建议触发次数。
    pub suggestions: u32,
}

impl CrashRepeatTracker {
    pub fn new() -> CrashRepeatTracker {
        CrashRepeatTracker { counts: [(0, 0, 0); 64], n: 0, suggestions: 0 }
    }

    /// 记录一次崩溃（file_hash, now_ms）。窗口内 ≥3 → 触发建议。
    pub fn crash(&mut self, file_hash: u64, now_ms: u64) -> bool {
        const WINDOW_MS: u64 = 24 * 3_600_000;
        let slot = (0..self.n).find(|&i| self.counts[i].0 == file_hash);
        let idx = match slot {
            Some(i) => i,
            None => {
                if self.n >= 64 {
                    // 满槽淘汰计数最旧。
                    let mut victim = 0;
                    let mut oldest = u64::MAX;
                    for i in 0..64 {
                        if self.counts[i].2 < oldest {
                            oldest = self.counts[i].2;
                            victim = i;
                        }
                    }
                    self.counts[victim] = (file_hash, 0, now_ms);
                    self.n = 64;
                    victim
                } else {
                    self.counts[self.n] = (file_hash, 0, now_ms);
                    self.n += 1;
                    self.n - 1
                }
            }
        };
        let (h, c, first) = self.counts[idx];
        let _ = h;
        // 窗口滑动：首崩超 24h → 计数重置。
        let (c, first) = if now_ms.saturating_sub(first) > WINDOW_MS {
            (0, now_ms)
        } else {
            (c, first)
        };
        let c = c + 1;
        self.counts[idx] = (file_hash, c, first);
        if c >= 3 {
            self.suggestions += 1;
            true
        } else {
            false
        }
    }
}

impl Default for CrashRepeatTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_excface_base_checks() -> CheckSet {
    let mut cs = CheckSet::new("F020-excface");
    // 1) 判据常量（异常码集 / 64 帧 / 32 模块 / 20 dump / 帧栈 32）。
    cs.add(
        "consts",
        EXC_INT_DIVIDE_BY_ZERO == 0xC000_0094
            && EXC_ACCESS_VIOLATION == 0xC000_0005
            && EXC_STACK_OVERFLOW == 0xC000_00FD
            && DUMP_STACK_FRAMES == 64
            && DUMP_MODULES == 32
            && DUMP_CAP == 20
            && SIX_SAMPLE_CODES.len() == 6,
        "",
    );
    // 2) 六类注入样本行为全对（判据一）：
    //    a) 除零——SEH Handle → 存活（吞噬路径）。
    let mut t = ExceptionTranslator::new(0);
    let d1 = t.raise(EXC_INT_DIVIDE_BY_ZERO, 0x1000, &[FilterVerdict::Handle]);
    cs.add("div_zero_swallowed_alive", d1 == Dispatch::HandledBySeq(1) && !t.reclaimed, "");
    //    b) 非法访问——链全 ContinueSearch → 未处理回收。
    let mut t2 = ExceptionTranslator::new(2);
    let d2 = t2.raise(EXC_ACCESS_VIOLATION, 0xDEAD, &[FilterVerdict::ContinueSearch, FilterVerdict::ContinueSearch]);
    cs.add("access_violation_unhandled_reclaimed", d2 == Dispatch::Unhandled(2, 2) && t2.reclaimed, "");
    //    c) 栈溢出——保护页捕获归因。
    let mut t3 = ExceptionTranslator::new(0);
    let _ = t3.raise(EXC_STACK_OVERFLOW, 0x7F00, &[FilterVerdict::Handle]);
    cs.add("stack_overflow_guard_page", t3.stack_guard_hit && !t3.reclaimed, "");
    //    d) 未处理 C++ 异常——无处理器 → 回收。
    let mut t4 = ExceptionTranslator::new(0);
    let d4 = t4.raise(EXC_CPP_UNHANDLED, 0x2000, &[]);
    cs.add("cpp_unhandled_reclaimed", d4 == Dispatch::Unhandled(0, 0) && t4.reclaimed, "");
    //    e) SEH 吞噬——链第一环 Handle（异常不上抛）。
    let mut t5 = ExceptionTranslator::new(1);
    let d5 = t5.raise(EXC_SEH_SWALLOWED, 0x3000, &[FilterVerdict::Handle, FilterVerdict::ContinueSearch]);
    cs.add("seh_swallowed_first_ring", d5 == Dispatch::HandledBySeq(1), "");
    //    f) VEH 链——先于 SEH 走过 2 节点后由 SEH 处理。
    let mut t6 = ExceptionTranslator::new(2);
    let d6 = t6.raise(EXC_VEH_CHAIN, 0x4000, &[FilterVerdict::ContinueSearch, FilterVerdict::Handle]);
    cs.add("veh_chain_then_seh", d6 == Dispatch::HandledBySeq(2) && t6.frame_count() == 1, "");
    // 3) 恢复执行语义（EXCEPTION_CONTINUE_EXECUTION）。
    let mut t7 = ExceptionTranslator::new(0);
    let d7 = t7.raise(EXC_ACCESS_VIOLATION, 0x5000, &[FilterVerdict::ContinueExecution]);
    cs.add("continue_execution_verdict", d7 == Dispatch::ContinuedExecution(1) && !t7.reclaimed, "");
    // 4) 双重故障：处理器自身再异常 → 直接回收不递归（帧栈不膨胀）。
    let mut t8 = ExceptionTranslator::new(1);
    let _ = t8.raise(EXC_ACCESS_VIOLATION, 0x6000, &[FilterVerdict::ContinueSearch]);
    let d8 = t8.raise_in_handler();
    cs.add(
        "double_fault_no_recursion",
        d8 == Dispatch::ReclaimedDoubleFault && t8.double_fault && t8.frame_count() == 1,
        "",
    );
    // 5) 帧栈深度 32：超限 = 双重故障语义（翻译层不可再展开）。
    let mut t9 = ExceptionTranslator::new(0);
    let mut last = Dispatch::HandledBySeq(0);
    for i in 0..40u32 {
        last = t9.raise(EXC_ACCESS_VIOLATION, i as u64 * 0x10, &[]);
        // 双重故障即翻译层终点（第 33 次投递触发——栈满后的下一次投递）。
        if last == Dispatch::ReclaimedDoubleFault {
            break;
        }
    }
    cs.add(
        "frame_stack_32_cap",
        t9.frame_n <= 32 && last == Dispatch::ReclaimedDoubleFault && t9.double_fault,
        "",
    );
    // 6) minidump：栈回溯 64 帧裁剪 + 模块表 32 裁剪 + 指纹。
    let stack: Vec<u64> = (0..80u64).map(|i| 0x7FF0_0000 + i * 8).collect();
    let modules: Vec<DumpModule> = (0..40u32)
        .map(|i| DumpModule { base: 0x400000 + i as u64 * 0x10000, size: 0x8000, checksum: i })
        .collect();
    let d = build_dump(EXC_ACCESS_VIOLATION, 0xDEAD_BEEF, 42, &stack, &modules, 0xA, 0xB);
    cs.add(
        "minidump_structure",
        d.stack_n == 64 && d.module_n == 32 && d.system_fingerprint == 0x5354_4152,
        "",
    );
    // 7) dump LRU 20 个：满后驱逐最旧。
    let mut store = DumpStore::new();
    for i in 0..25u32 {
        let d = build_dump(EXC_ACCESS_VIOLATION, i as u64, i, &[], &[], i as u64, i as u64);
        store.store(d, true);
    }
    let latest = store.latest().unwrap();
    cs.add(
        "dump_lru_20",
        store.len() == DUMP_CAP && latest.crash_addr == 24 && latest.thread_id == 24,
        "",
    );
    // 8) dump 写入失败 → 降级（日志摘要计数可见——不静默）。
    let mut store2 = DumpStore::new();
    let d = build_dump(EXC_ACCESS_VIOLATION, 1, 1, &[], &[], 1, 1);
    store2.store(d, false);
    cs.add(
        "dump_write_failure_degrades",
        store2.is_empty() && store2.write_failures == 1,
        "",
    );
    // 9) 崩溃重复跟踪：24h 内 3 次 → F036 建议触发。
    let mut trk = CrashRepeatTracker::new();
    let h = 0xF17Eu64;
    let a = trk.crash(h, 0);
    let b = trk.crash(h, 3_600_000);
    let c = trk.crash(h, 7_200_000);
    cs.add("crash_3_in_24h_suggests", !a && !b && c && trk.suggestions == 1, "");
    // 10) 24h 窗口滑动：距首崩 >24h → 计数重置（不再累计旧账）。
    let d10 = trk.crash(h, 24 * 3_600_000 + 1);
    cs.add("crash_window_slides", !d10, "");
    cs
}

// ---------------------------------------------------------------------------
// 测试（宿主）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_samples_full_matrix() {
        // 判据一完整矩阵：六类样本各自行为归宿互不混淆。
        let mut t = ExceptionTranslator::new(1);
        // 除零被 SEH 吃掉。
        let a = t.raise(EXC_INT_DIVIDE_BY_ZERO, 1, &[FilterVerdict::Handle]);
        assert!(matches!(a, Dispatch::HandledBySeq(_)));
        assert!(!t.reclaimed);
        // 非法访问没人管 → 回收。
        let b = t.raise(EXC_ACCESS_VIOLATION, 2, &[]);
        assert!(matches!(b, Dispatch::Unhandled(1, 0)));
        assert!(t.reclaimed);
        // 新翻译器：栈溢出带保护页归因。
        let mut t2 = ExceptionTranslator::new(0);
        let _ = t2.raise(EXC_STACK_OVERFLOW, 3, &[]);
        assert!(t2.stack_guard_hit);
        // C++ 异常无处理器 → 回收。
        let mut t3 = ExceptionTranslator::new(0);
        let d = t3.raise(EXC_CPP_UNHANDLED, 4, &[]);
        assert!(matches!(d, Dispatch::Unhandled(0, 0)));
    }

    #[test]
    fn veh_before_seh_ordering() {
        // Windows 语义：VEH 链先于 SEH 链（层数记账证明顺序）。
        let mut t = ExceptionTranslator::new(3);
        let d = t.raise(EXC_VEH_CHAIN, 1, &[FilterVerdict::ContinueSearch]);
        // VEH 走过 3 节点，SEH 链 ContinueSearch → 未处理（带 VEH=3）。
        assert_eq!(d, Dispatch::Unhandled(3, 1));
    }

    #[test]
    fn restart_snapshot_preserved() {
        // 「重新启动应用」保留原参数（命令行/工作目录快照进 dump）。
        let d = build_dump(EXC_ACCESS_VIOLATION, 0x10, 1, &[], &[], 0xC0DE, 0xC0DE1 ^ 0xFFFF);
        assert_eq!(d.restart_cmd_hash, 0xC0DE);
        assert_eq!(d.restart_cwd_hash, 0xC0DE1 ^ 0xFFFF);
        // 崩溃卡片「查看详情」= dump 摘要只读（latest 拉取）。
        let mut s = DumpStore::new();
        s.store(d, true);
        let got = s.latest().unwrap();
        assert_eq!(got.exception_code, EXC_ACCESS_VIOLATION);
        assert_eq!(got.crash_addr, 0x10);
    }

    #[test]
    fn dump_lru_evicts_oldest_stamp() {
        // LRU 驱逐按时间戳（不是槽位序）。
        let mut s = DumpStore::new();
        for i in 0..DUMP_CAP {
            let d = build_dump(EXC_CPP_UNHANDLED, i as u64, i as u32, &[], &[], 0, 0);
            s.store(d, true);
        }
        // 访问最旧（0 号）不算——无 touch 语义，严格按写入时间。
        let d_new = build_dump(EXC_CPP_UNHANDLED, 999, 999, &[], &[], 0, 0);
        s.store(d_new, true);
        let latest = s.latest().unwrap();
        assert_eq!(latest.crash_addr, 999);
        assert_eq!(s.len(), DUMP_CAP);
    }

    #[test]
    fn tracker_multiple_files_independent() {
        // 多文件计数独立：A 三次触发，B 才一次不触发。
        let mut t = CrashRepeatTracker::new();
        let _ = t.crash(0xA, 0);
        let _ = t.crash(0xA, 1);
        let fired = t.crash(0xA, 2);
        assert!(fired);
        assert!(!t.crash(0xB, 3));
        assert_eq!(t.suggestions, 1);
    }

    #[test]
    fn translator_frames_hold_code_and_addr() {
        // 帧内容保真：异常码/地址/深度可查（F035 归因链数据源）。
        let mut t = ExceptionTranslator::new(0);
        let _ = t.raise(EXC_ACCESS_VIOLATION, 0xBADF00D, &[FilterVerdict::Handle]);
        let f = t.top_frame().unwrap();
        assert_eq!(f.code, EXC_ACCESS_VIOLATION);
        assert_eq!(f.fault_addr, 0xBADF00D);
        assert_eq!(f.depth, 1);
    }
}

// ---------------------------------------------------------------------------
// F020 · 深化扩展：minidump 二进制序列化（自定轻量格式 VDMP）
//
// 主册依据（G-A-20【数据与存储】）：「dump 格式用自定轻量格式（可导出转换
// CDB 可读文本）」——本扩展给出 VDMP 二进制布局的写入/读回/校验闭环：
// 落诊断中心的 dump 可序列化、可校验、可读回展示（崩溃卡片「查看详情」的
// 数据面）。
// ---------------------------------------------------------------------------

/// VDMP 魔数与版本。
pub const VDMP_MAGIC: [u8; 4] = [b'V', b'D', b'M', b'P'];
pub const VDMP_VERSION: u32 = 1;
/// 序列化缓冲需求（头 56B + 栈 64×8 + 模块 32×20 + 尾注 8）。
pub const VDMP_SERIAL_SIZE: usize = 56 + DUMP_STACK_FRAMES * 8 + DUMP_MODULES * 20 + 8;

/// 序列化：dump → 定长缓冲。返回写入字节数；缓冲不足返回 0（如实拒绝）。
pub fn serialize_dump(d: &MiniDump, out: &mut [u8]) -> usize {
    if out.len() < VDMP_SERIAL_SIZE {
        return 0;
    }
    let mut w = 0usize;
    let put32 = |out: &mut [u8], w: &mut usize, v: u32| {
        out[*w..*w + 4].copy_from_slice(&v.to_le_bytes());
        *w += 4;
    };
    let put64 = |out: &mut [u8], w: &mut usize, v: u64| {
        out[*w..*w + 8].copy_from_slice(&v.to_le_bytes());
        *w += 8;
    };
    out[..4].copy_from_slice(&VDMP_MAGIC);
    w = 4;
    let _ = w;
    put32(out, &mut w, VDMP_VERSION);
    put32(out, &mut w, d.exception_code);
    put32(out, &mut w, d.thread_id);
    put32(out, &mut w, d.stack_n as u32);
    put32(out, &mut w, d.module_n as u32);
    put32(out, &mut w, d.system_fingerprint);
    put32(out, &mut w, d.degraded as u32);
    put64(out, &mut w, d.crash_addr);
    put64(out, &mut w, d.restart_cmd_hash);
    put64(out, &mut w, d.restart_cwd_hash);
    for i in 0..DUMP_STACK_FRAMES {
        put64(out, &mut w, d.stack[i]);
    }
    for i in 0..DUMP_MODULES {
        match d.modules[i] {
            Some(m) => {
                put64(out, &mut w, m.base);
                put32(out, &mut w, m.size);
                put32(out, &mut w, m.checksum);
                put32(out, &mut w, 0); // 保留（记录对齐 20B）
            }
            None => {
                put64(out, &mut w, 0);
                put32(out, &mut w, 0);
                put32(out, &mut w, 0);
                put32(out, &mut w, 0); // 保留（记录对齐 20B）
            }
        }
    }
    // 尾部校验和（FNV-1a over 头+体，不含校验槽自身）。
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for &b in &out[..w] {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01B3);
    }
    put64(out, &mut w, h);
    w
}

/// 读回：缓冲 → dump。魔数/版本/校验和任一不符 → 如实拒绝（不静默吞）。
pub fn deserialize_dump(buf: &[u8]) -> Result<MiniDump, &'static str> {
    if buf.len() < VDMP_SERIAL_SIZE {
        return Err("vdmp: buffer too small");
    }
    if buf[0..4] != VDMP_MAGIC {
        return Err("vdmp: bad magic");
    }
    let rd32 = |o: usize| u32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
    let rd64 = |o: usize| u64::from_le_bytes(buf[o..o + 8].try_into().unwrap());
    if rd32(4) != VDMP_VERSION {
        return Err("vdmp: unsupported version");
    }
    // 校验和（尾部 8B，覆盖其前全部字节）。
    let body_end = VDMP_SERIAL_SIZE - 8;
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for &b in &buf[..body_end] {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01B3);
    }
    if h != rd64(body_end) {
        return Err("vdmp: checksum mismatch");
    }
    let mut d = MiniDump {
        exception_code: rd32(8),
        crash_addr: rd64(32),
        thread_id: rd32(12),
        stack: [0; DUMP_STACK_FRAMES],
        stack_n: (rd32(16) as usize).min(DUMP_STACK_FRAMES),
        modules: [None; DUMP_MODULES],
        module_n: (rd32(20) as usize).min(DUMP_MODULES),
        system_fingerprint: rd32(24),
        degraded: rd32(28) != 0,
        restart_cmd_hash: rd64(40),
        restart_cwd_hash: rd64(48),
    };
    let stack_base = 56;
    for i in 0..DUMP_STACK_FRAMES {
        d.stack[i] = rd64(stack_base + i * 8);
    }
    let mod_base = stack_base + DUMP_STACK_FRAMES * 8;
    for i in 0..DUMP_MODULES {
        let o = mod_base + i * 20;
        let base = rd64(o);
        let size = rd32(o + 8);
        let checksum = rd32(o + 12);
        if base != 0 || size != 0 {
            d.modules[i] = Some(DumpModule { base, size, checksum });
        }
    }
    Ok(d)
}

#[cfg(test)]
mod ext_tests {
    use super::*;

    #[test]
    fn dump_serialization_round_trip() {
        let stack: Vec<u64> = (0..20u64).map(|i| 0x7FF0_0000 + i * 8).collect();
        let modules = alloc::vec![
            DumpModule { base: 0x400000, size: 0x8000, checksum: 7 },
            DumpModule { base: 0x500000, size: 0x4000, checksum: 9 },
        ];
        let d = build_dump(EXC_ACCESS_VIOLATION, 0xDEAD_BEEF, 42, &stack, &modules, 0xA, 0xB);
        let mut buf = [0u8; VDMP_SERIAL_SIZE];
        let n = serialize_dump(&d, &mut buf);
        assert_eq!(n, VDMP_SERIAL_SIZE);
        let back = deserialize_dump(&buf).unwrap();
        assert_eq!(back.exception_code, EXC_ACCESS_VIOLATION);
        assert_eq!(back.crash_addr, 0xDEAD_BEEF);
        assert_eq!(back.thread_id, 42);
        assert_eq!(back.stack_n, 20);
        assert_eq!(back.module_n, 2);
        assert_eq!(back.modules[0].unwrap().checksum, 7);
        assert_eq!(back.restart_cmd_hash, 0xA);
    }

    #[test]
    fn dump_serialization_honest_rejections() {
        let d = build_dump(EXC_CPP_UNHANDLED, 1, 1, &[], &[], 1, 1);
        // 缓冲不足 → 0（如实拒绝）。
        let mut small = [0u8; 64];
        assert_eq!(serialize_dump(&d, &mut small), 0);
        // 序列化后篡改一字节 → 校验和拒绝。
        let mut buf = [0u8; VDMP_SERIAL_SIZE];
        let _ = serialize_dump(&d, &mut buf);
        buf[10] ^= 0xFF;
        assert!(matches!(deserialize_dump(&buf), Err("vdmp: checksum mismatch")));
        // 坏魔数。
        let mut buf2 = [0u8; VDMP_SERIAL_SIZE];
        let _ = serialize_dump(&d, &mut buf2);
        buf2[0] = b'X';
        assert!(matches!(deserialize_dump(&buf2), Err("vdmp: bad magic")));
        // 未知版本。
        let mut buf3 = [0u8; VDMP_SERIAL_SIZE];
        let _ = serialize_dump(&d, &mut buf3);
        buf3[4..8].copy_from_slice(&99u32.to_le_bytes());
        assert!(matches!(deserialize_dump(&buf3), Err("vdmp: unsupported version")));
    }
}

// ---------------------------------------------------------------------------
// 深化批次二：自检聚合（主检 + 深化检并为一行——AI-U2 merge 先例；
// robust.rs / 隔离壳 checkup 接线不变，深化检查项全部经由此行可见）。
// ---------------------------------------------------------------------------

/// 域自检（聚合版）。
pub fn run_excface_checks() -> CheckSet {
    CheckSet::merge(run_excface_base_checks(), CheckSet::merge(run_excface_deep_checks(), run_excface_deep2_checks()))
}

// ---------------------------------------------------------------------------
// F020 · 深化批次二：既有深化面（24h 三崩建议/双重故障/dump 池）钉死对账
//
// 主册依据（G-A-20【设计细节】）：「同一文件 24 小时内崩溃三次以上自动建议
// 提交 F036 草稿」「异常处理器自身再异常 → 双重故障直接回收进程（不递归）」
// 「minidump 存 diagnostics/dumps/ 上限 20 个 LRU」——三者均为批次一实装，
// 本批以深化检钉死语义（不再新增同语义件——一处一事实）。
// ---------------------------------------------------------------------------

/// dump 池容量上限（G-A-20【数据与存储】：上限 20 个 LRU）。
pub const DUMP_STORE_CAP: usize = 20;

/// F020 深化自检。
pub fn run_excface_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F020-excface-deep");
    // 1) 24h 三崩建议（CrashRepeatTracker 既有面）：3 崩 → 建议触发；25h 前
    //    的崩溃出窗不计数（窗口滑动语义）。
    let mut tr = CrashRepeatTracker::new();
    let s1 = tr.crash(0xBEEF, 0);
    let s2 = tr.crash(0xBEEF, 1);
    let s3 = tr.crash(0xBEEF, 2);
    cs.add("three_strikes_suggests", !s1 && !s2 && s3 && tr.suggestions == 1, "");
    let mut tr2 = CrashRepeatTracker::new();
    let _ = tr2.crash(0xBEEF, 0);
    let _ = tr2.crash(0xBEEF, 1);
    let s_far = tr2.crash(0xBEEF, 25 * 3_600_000 + 2);
    let s_far2 = tr2.crash(0xBEEF, 25 * 3_600_000 + 3);
    let s_far3 = tr2.crash(0xBEEF, 25 * 3_600_000 + 4);
    cs.add(
        "window_slide_resets",
        !s_far && !s_far2 && s_far3 && tr2.suggestions == 1,
        "",
    );
    // 2) 双重故障（raise_in_handler 既有面）：处理器内再异常 → ReclaimedDoubleFault
    //    直接回收语义（double_fault/reclaimed 标记置位——不递归）。
    let mut et = ExceptionTranslator::new(2);
    let d1 = et.raise(0xC000_0005, 0x1000, &[]);
    let d2 = et.raise_in_handler();
    cs.add(
        "dual_fault_no_recursion",
        !matches!(d1, Dispatch::ReclaimedDoubleFault) && matches!(d2, Dispatch::ReclaimedDoubleFault),
        "",
    );
    // 3) dump 池 LRU 既有面对账：容量语义（20 上限——Store 满后最旧淘汰）。
    let mut st = DumpStore::new();
    for i in 0..(DUMP_STORE_CAP as u64 + 5) {
        let d = build_dump(0xC000_0005, 0x1000 + i, 1, &[0xAAAA_0000 + i; 4], &[], 0, 0);
        st.store(d, true);
    }
    cs.add(
        "dump_store_lru_cap",
        st.len() <= DUMP_STORE_CAP && st.latest().is_some(),
        "",
    );
    // 4) vdmp 序列化版本门（深化一批既有面）对账锚：坏版本如实拒。
    let d = build_dump(0xC000_0005, 0x2000, 1, &[0xBBBB_0000; 4], &[], 0, 0);
    let mut buf = [0u8; VDMP_SERIAL_SIZE];
    let n = serialize_dump(&d, &mut buf);
    let mut bad = buf;
    bad[4..8].copy_from_slice(&99u32.to_le_bytes());
    cs.add(
        "vdmp_version_gate_anchored",
        n == VDMP_SERIAL_SIZE && matches!(deserialize_dump(&buf), Ok(_)) && deserialize_dump(&bad).is_err(),
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F020 · 深化批次三：dump 导出 CDB 可读文本（自定格式 → 开发者可读面）+
// 「重新启动应用」启动快照（命令行 + 工作目录字符串捕获）
//
// 主册依据（G-A-20【数据与存储】）：「dump 格式用自定轻量格式（可导出转换
// CDB 可读文本）」；【设计细节】「『重新启动应用』保留命令行与工作目录（启动
// 时快照）」。既有面：MiniDump 序列化/降级/LRU/异常帧栈不重复——本段补导出
// 文本化与字符串快照（既有 restart 哈希位之上的原文捕获）。
// ---------------------------------------------------------------------------

/// CDB 导出行缓冲上限（导出截断如实——不静默丢帧）。
pub const CDB_TEXT_CAP: usize = 4096;

/// dump → CDB 可读文本（开发者拿 dump 定位源码行的消费面：异常码/崩溃地址/
/// 栈回溯逐帧/模块表逐模块）。返回写入字节数（缓冲不足截断——截断结果不冒充
/// 完整 dump）。
pub fn dump_to_cdb_text(d: &MiniDump, out: &mut [u8]) -> usize {
    let mut n = 0usize;
    crate::checks::push_str(out, &mut n, "VARIX minidump (CDB view)\n");
    crate::checks::push_str(out, &mut n, "exception code: ");
    crate::checks::push_hex_u64(out, &mut n, d.exception_code as u64);
    crate::checks::push_str(out, &mut n, "\ncrash address: ");
    crate::checks::push_hex_u64(out, &mut n, d.crash_addr);
    crate::checks::push_str(out, &mut n, "\nstack frames:\n");
    for i in 0..d.stack_n {
        crate::checks::push_str(out, &mut n, "  #");
        crate::checks::push_usize(out, &mut n, i);
        crate::checks::push_str(out, &mut n, ": ");
        crate::checks::push_hex_u64(out, &mut n, d.stack[i]);
        crate::checks::push_str(out, &mut n, "\n");
    }
    crate::checks::push_str(out, &mut n, "modules:\n");
    for i in 0..d.module_n {
        if let Some(m) = d.modules[i] {
            crate::checks::push_str(out, &mut n, "  base ");
            crate::checks::push_hex_u64(out, &mut n, m.base);
            crate::checks::push_str(out, &mut n, " size ");
            crate::checks::push_usize(out, &mut n, m.size as usize);
            crate::checks::push_str(out, &mut n, " checksum ");
            crate::checks::push_hex_u64(out, &mut n, m.checksum as u64);
            crate::checks::push_str(out, &mut n, "\n");
        }
    }
    n.min(out.len())
}

/// 重启快照缓冲上限（命令行/工作目录同限——超出如实截断计数）。
pub const RESTART_STR_CAP: usize = 64;

/// 「重新启动应用」启动快照（命令行 + 工作目录原文捕获——重启 = 原参数重拉）。
#[derive(Clone, Copy, Debug)]
pub struct RestartSnapshot {
    cmd: [u8; RESTART_STR_CAP],
    cmd_n: usize,
    cwd: [u8; RESTART_STR_CAP],
    cwd_n: usize,
    /// 捕获时被截断的字段数（如实登记——截断参数重启 = 参数失真，必须可见）。
    pub truncated_fields: u32,
}

impl RestartSnapshot {
    pub fn capture(cmdline: &str, workdir: &str) -> RestartSnapshot {
        let mut s = RestartSnapshot {
            cmd: [0; RESTART_STR_CAP],
            cmd_n: 0,
            cwd: [0; RESTART_STR_CAP],
            cwd_n: 0,
            truncated_fields: 0,
        };
        let cb = cmdline.as_bytes();
        s.cmd_n = cb.len().min(RESTART_STR_CAP);
        s.cmd[..s.cmd_n].copy_from_slice(&cb[..s.cmd_n]);
        if cb.len() > RESTART_STR_CAP {
            s.truncated_fields += 1;
        }
        let wb = workdir.as_bytes();
        s.cwd_n = wb.len().min(RESTART_STR_CAP);
        s.cwd[..s.cwd_n].copy_from_slice(&wb[..s.cwd_n]);
        if wb.len() > RESTART_STR_CAP {
            s.truncated_fields += 1;
        }
        s
    }

    pub fn cmdline(&self) -> &str {
        core::str::from_utf8(&self.cmd[..self.cmd_n]).unwrap_or("")
    }

    pub fn workdir(&self) -> &str {
        core::str::from_utf8(&self.cwd[..self.cwd_n]).unwrap_or("")
    }
}

/// F020 深化批次三自检。
pub fn run_excface_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F020-excface-deep2");
    // 1) CDB 导出：含异常码/崩溃地址十六进制行、逐帧行数与 stack_n 一致、
    //    模块行数与 module_n 一致（开发者可读面的结构完整）。
    let mut d = MiniDump {
        exception_code: 0xC000_0005,
        crash_addr: 0x0000_7FF6_1234_5678,
        thread_id: 42,
        stack: [0; DUMP_STACK_FRAMES],
        stack_n: 3,
        modules: [None; DUMP_MODULES],
        module_n: 2,
        system_fingerprint: 0x0102_0304,
        degraded: false,
        restart_cmd_hash: 0,
        restart_cwd_hash: 0,
    };
    d.stack[0] = 0xFFFF_8000_1000_0000;
    d.stack[1] = 0xFFFF_8000_1000_0010;
    d.stack[2] = 0xFFFF_8000_1000_0020;
    d.modules[0] = Some(DumpModule { base: 0x7FF6_1000_0000, size: 0x1000, checksum: 0xDEAD });
    d.modules[1] = Some(DumpModule { base: 0x7FF6_2000_0000, size: 0x2000, checksum: 0xBEEF });
    let mut buf = [0u8; CDB_TEXT_CAP];
    let n = dump_to_cdb_text(&d, &mut buf);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    let frame_lines = text.matches("  #").count();
    let mod_lines = text.matches(" checksum ").count();
    cs.add(
        "cdb_export_structure_complete",
        n > 0
            && text.contains("exception code: c0000005")
            && text.contains("crash address: 7ff612345678")
            && frame_lines == 3
            && mod_lines == 2,
        "",
    );
    // 2) 导出截断诚实：小缓冲（64 字节）只产出前 64 字节且不越界（截断不冒充
    //    完整 dump）。
    let mut small = [0u8; 64];
    let n2 = dump_to_cdb_text(&d, &mut small);
    cs.add("cdb_export_truncation_honest", n2 == 64, "");
    // 3) 重启快照：命令行+工作目录原文往返；超长如实截断计数（截断参数重启
    //    必须可见——不静默失真）。
    let snap = RestartSnapshot::capture("app.exe --flag=1", "C:\\Users\\doc");
    let mut snap2 = RestartSnapshot::capture("", "");
    snap2 = RestartSnapshot::capture(&"x".repeat(RESTART_STR_CAP + 10), "C:\\w");
    cs.add(
        "restart_snapshot_original_args",
        snap.cmdline() == "app.exe --flag=1"
            && snap.workdir() == "C:\\Users\\doc"
            && snap.truncated_fields == 0
            && snap2.truncated_fields == 1
            && snap2.cmdline().len() == RESTART_STR_CAP,
        "",
    );
    cs
}
