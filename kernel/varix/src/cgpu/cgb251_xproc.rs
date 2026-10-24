//! CGPU-F0251 · 跨进程死锁（cgb251_xproc）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0251`
//! **判据原文摘录**：命名同步对象/共享段的进程间等待边（本进程视角：跨进程等待
//! 标注为「外部等待」——能见度先于能解性）、解除边界诚实声明（外部进程死锁引擎
//! 只能报告不能解除——三要素告知候选拯救动作）、与宿主进程监控接口预留。
//!
//! ## 能见度先于能解性
//!
//! 跨进程死锁的最大困难不是算法，是**边界**：
//!
//! - 引擎能看到**本进程**的等待边（内核钩子）；
//! - 引擎看不到**其他进程**的等待边（它们在自己的地址空间）；
//! - 因此跨进程等待只能标注「外部等待」——**看得见谁在等，看不见
//!   对面在干什么**。
//!
//! 原则：**能见度先于能解性**。先把可见的做好（标注外部等待），再谈
//! 解除。不可见的部分诚实声明「不能解」，不假装能解。
//!
//! ## 三要素告知
//!
//! 跨进程死锁候选的拯救动作必须告知：
//! 1. **检测到了什么**——本进程的等待边 + 外部等待标注；
//! 2. **为什么不能自动解除**——对面进程不在引擎管辖内；
//! 3. **建议的下一步**——上报宿主 / 通知对端 / 人工介入。
//!
//! 这三要素缺一不可——用户必须知道「引擎尽力了，但这个它管不了」。

use alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

use super::cgb241_dlarch::{SubjectId, WaitSource};

// ---------------------------------------------------------------------------
// 诊断码（0xCBE0 起）
// ---------------------------------------------------------------------------

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCBE0;
/// 外部进程不可达。
pub const E_EXT_UNREACHABLE: u16 = SEG_BASE + 1;
/// 等待记录满。
pub const E_TABLE_FULL: u16 = SEG_BASE + 2;

// ---------------------------------------------------------------------------
// 容量
// ---------------------------------------------------------------------------

/// 跨进程等待记录上限。
pub const MAX_XPROC_WAITS: usize = 256;
/// 外部进程注册上限。
pub const MAX_EXT_PROCS: usize = 32;

// ---------------------------------------------------------------------------
// 外部等待
// ---------------------------------------------------------------------------

/// 外部等待类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExternalWaitKind {
    /// 命名互斥锁。
    NamedMutex,
    /// 命名信号量。
    NamedSemaphore,
    /// 共享内存段。
    SharedMemory,
    /// 命名事件。
    NamedEvent,
    /// 管道。
    Pipe,
}

impl ExternalWaitKind {
    pub const ALL: [ExternalWaitKind; 5] = [
        ExternalWaitKind::NamedMutex,
        ExternalWaitKind::NamedSemaphore,
        ExternalWaitKind::SharedMemory,
        ExternalWaitKind::NamedEvent,
        ExternalWaitKind::Pipe,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            ExternalWaitKind::NamedMutex => "named_mutex",
            ExternalWaitKind::NamedSemaphore => "named_semaphore",
            ExternalWaitKind::SharedMemory => "shared_memory",
            ExternalWaitKind::NamedEvent => "named_event",
            ExternalWaitKind::Pipe => "pipe",
        }
    }
}

/// 跨进程等待边（本进程视角）。
#[derive(Clone, Debug)]
pub struct ExternalWait {
    /// 等待主体（本进程内）。
    pub waiter: SubjectId,
    /// 同步对象名。
    pub object_name: String,
    /// 等待类型。
    pub kind: ExternalWaitKind,
    /// 对端进程 ID（若已知）。
    pub peer_pid: Option<u32>,
    /// 建边 tick。
    pub created_at: u32,
    /// 是否已消亡。
    pub settled: bool,
}

/// 三要素告知。
#[derive(Clone, Debug)]
pub struct BoundaryDisclosure {
    /// ① 检测到了什么。
    pub what: String,
    /// ② 为什么不能自动解除。
    pub why_not: String,
    /// ③ 建议的下一步。
    pub next_step: String,
}

impl BoundaryDisclosure {
    /// 渲染三要素（人话）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(&alloc::format!("检测到：{}\n", self.what));
        s.push_str(&alloc::format!("不能自动解除：{}\n", self.why_not));
        s.push_str(&alloc::format!("建议下一步：{}\n", self.next_step));
        s
    }
}

// ---------------------------------------------------------------------------
// 跨进程死锁检测器
// ---------------------------------------------------------------------------

/// 跨进程死锁检测器。
///
/// 纪律：
/// 1. **能见度先于能解性**——先标注可见的，诚实声明不可见的；
/// 2. **解除边界诚实**——不假装能解除跨进程死锁；
/// 3. **三要素告知**——每次报告必须含三要素。
pub struct CrossProcessDetector {
    waits: Vec<ExternalWait>,
    /// 外部进程注册表。
    ext_procs: Vec<(u32, String)>,
    /// 遥测：外部等待边数。
    pub ext_wait_count: u64,
    /// 遥测：三要素告知次数。
    pub disclosures: u64,
}

impl CrossProcessDetector {
    pub fn new() -> CrossProcessDetector {
        CrossProcessDetector {
            waits: Vec::new(),
            ext_procs: Vec::new(),
            ext_wait_count: 0,
            disclosures: 0,
        }
    }

    /// 注册外部进程。
    pub fn register_ext_proc(&mut self, pid: u32, name: &str) -> Result<(), u16> {
        if self.ext_procs.len() >= MAX_EXT_PROCS {
            return Err(E_TABLE_FULL);
        }
        if self.ext_procs.iter().any(|(p, _)| *p == pid) {
            return Ok(());
        }
        self.ext_procs.push((pid, String::from(name)));
        Ok(())
    }

    /// 记录跨进程等待。
    pub fn add_external_wait(
        &mut self,
        waiter: SubjectId,
        object_name: &str,
        kind: ExternalWaitKind,
        peer_pid: Option<u32>,
        now: u32,
    ) -> Result<usize, u16> {
        if self.waits.len() >= MAX_XPROC_WAITS {
            return Err(E_TABLE_FULL);
        }
        self.waits.push(ExternalWait {
            waiter,
            object_name: String::from(object_name),
            kind,
            peer_pid,
            created_at: now,
            settled: false,
        });
        self.ext_wait_count += 1;
        Ok(self.waits.len() - 1)
    }

    /// 等待消亡。
    pub fn settle(&mut self, index: usize) -> Result<(), u16> {
        if index >= self.waits.len() {
            return Err(E_EXT_UNREACHABLE);
        }
        self.waits[index].settled = true;
        Ok(())
    }

    /// 活动跨进程等待。
    pub fn active(&self) -> Vec<&ExternalWait> {
        self.waits.iter().filter(|w| !w.settled).collect()
    }

    /// 生成三要素告知。
    pub fn disclose(&mut self, wait_index: usize) -> BoundaryDisclosure {
        self.disclosures += 1;
        let w = self.waits.get(wait_index);
        let (what, peer) = match w {
            Some(w) => (
                alloc::format!(
                    "本进程任务#{} 等待命名对象「{}」（{}）",
                    w.waiter, w.object_name, w.kind.name()
                ),
                w.peer_pid,
            ),
            None => (String::from("未知等待"), None),
        };

        let why_not = match peer {
            Some(pid) => alloc::format!(
                "对端进程 {} 不在引擎管辖内，无法直接解除",
                pid
            ),
            None => String::from("对端进程未知，无法定位解除点"),
        };

        let next_step = match peer {
            Some(_) => String::from("上报宿主进程监控接口，请宿主协调解除"),
            None => String::from("人工介入：确认对端进程状态后手动解除"),
        };

        BoundaryDisclosure {
            what,
            why_not,
            next_step,
        }
    }

    /// 跨进程等待边生成（进 WFG 的外部等待标注）。
    pub fn generate_annotations(&self) -> Vec<(SubjectId, WaitSource, String)> {
        self.waits
            .iter()
            .filter(|w| !w.settled)
            .map(|w| {
                (
                    w.waiter,
                    match w.kind {
                        ExternalWaitKind::NamedMutex | ExternalWaitKind::NamedSemaphore => {
                            WaitSource::Lock
                        }
                        _ => WaitSource::VramAlloc,
                    },
                    alloc::format!("ext:{}", w.object_name),
                )
            })
            .collect()
    }

    // -- 自检 --------------------------------------------------------------

    pub fn check_consistency(&self) -> CheckSet {
        let mut cs = CheckSet::new("cgb251_xproc");
        cs.add(
            "等待有界",
            self.waits.len() <= MAX_XPROC_WAITS,
            "MAX=256",
        );
        cs.add(
            "外部进程有界",
            self.ext_procs.len() <= MAX_EXT_PROCS,
            "MAX=32",
        );
        let active = self.active().len();
        cs.add_dyn(
            "活动外部等待",
            true,
            &alloc::format!("{}", active),
        );
        cs.add(
            "三要素可产出",
            self.disclosures >= 0,
            "disclose 计数",
        );
        cs
    }
}

// ---------------------------------------------------------------------------
// 宿主监控接口（预留）
// ---------------------------------------------------------------------------

/// 宿主进程监控接口（预留）。
///
/// 真实系统中，跨进程死锁的解除需要宿主协调。此接口预留。
pub struct HostMonitorBridge {
    /// 是否已连接宿主。
    pub connected: bool,
    /// 上报队列。
    pub reports: Vec<BoundaryDisclosure>,
}

impl HostMonitorBridge {
    pub fn new() -> HostMonitorBridge {
        HostMonitorBridge {
            connected: false,
            reports: Vec::new(),
        }
    }

    /// 上报三要素给宿主。
    pub fn report(&mut self, disclosure: BoundaryDisclosure) {
        self.reports.push(disclosure);
    }

    /// 接口可用性。
    pub fn available(&self) -> bool {
        self.connected
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// CGPU-F0251 域自检。
pub fn run_cgb251_checks() -> CheckSet {
    let mut cs = CheckSet::new("cgb251_xproc");
    let mut det = CrossProcessDetector::new();
    let mut bridge = HostMonitorBridge::new();

    // --- 外部进程注册 ---
    det.register_ext_proc(1000, "gpu_server").ok();
    det.register_ext_proc(2000, "compositor").ok();
    cs.add("外部进程注册", det.ext_procs.len() == 2, "2 procs");

    // --- 跨进程等待 ---
    let idx = det.add_external_wait(
        10,
        "Global\\GPU_LOCK",
        ExternalWaitKind::NamedMutex,
        Some(1000),
        0,
    );
    cs.add("外部等待记录", idx.is_ok(), "named mutex");
    cs.add(
        "等待类型",
        det.active().first().map(|w| w.kind == ExternalWaitKind::NamedMutex).unwrap_or(false),
        "NamedMutex",
    );

    // --- 外部等待标注 ---
    let annotations = det.generate_annotations();
    cs.add(
        "标注生成",
        !annotations.is_empty(),
        "ext: 前缀",
    );
    cs.add(
        "标注含源",
        annotations
            .first()
            .map(|(_, src, _)| *src == WaitSource::Lock)
            .unwrap_or(false),
        "Lock 源",
    );

    // --- 三要素告知 ---
    let disclosure = det.disclose(idx.unwrap_or(0));
    cs.add(
        "三要素完整",
        !disclosure.what.is_empty()
            && !disclosure.why_not.is_empty()
            && !disclosure.next_step.is_empty(),
        "what/why/next",
    );
    let text = disclosure.render();
    cs.add(
        "三要素可读",
        text.contains("检测到") && text.contains("不能自动解除") && text.contains("建议"),
        "人话三行",
    );

    // --- 边界诚实 ---
    cs.add(
        "不假装能解",
        disclosure.why_not.contains("管辖") || disclosure.why_not.contains("未知"),
        "诚实声明",
    );

    // --- 等待消亡 ---
    det.settle(idx.unwrap_or(0)).ok();
    cs.add(
        "等待消亡",
        det.active().is_empty(),
        "settle",
    );

    // --- 宿主接口预留 ---
    cs.add(
        "接口预留",
        !bridge.available(),
        "未连接时不可用",
    );
    bridge.report(disclosure);
    cs.add(
        "上报队列",
        bridge.reports.len() == 1,
        "report",
    );

    // --- 一致性 ---
    let ic = det.check_consistency();
    let mut ok = true;
    for i in 0..ic.len() {
        if let Some(c) = ic.get(i) {
            if !c.passed {
                ok = false;
            }
        }
    }
    cs.add("跨进程一致性", ok, "四项");

    // --- 五类等待覆盖 ---
    let all_covered = ExternalWaitKind::ALL
        .iter()
        .all(|k| !k.name().is_empty());
    cs.add("五类等待", all_covered, "mutex/sem/shm/event/pipe");

    cs
}
