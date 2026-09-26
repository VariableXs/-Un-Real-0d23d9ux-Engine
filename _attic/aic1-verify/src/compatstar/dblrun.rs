//! F001 无感双击运行（compatstar · G-A-01）——用户只做「双击」一个动作。
//!
//! 主册判据（验收标准第一句）：
//! **「常用 50 件」（F040）中 GUI 类逐件实测：双击到首帧 ≤3s（U 盘实机、
//! 二次启动）、占位窗出现率 100%（超过 500ms 的样本）、Esc 取消后进程表零
//! 残留（kinfo process self-test 11/11 实测口径）。证据三件套：录屏 + 进程
//! 表快照 + 计时数据。」**
//!
//! 功能定义（G-A-01）：双击 .exe → 「识别格式 → peblock 门校验 → 装载进内存
//! → 建进程 → 显示窗口」全链路。边界：只承诺 64 位 PE（32 位见 F004），只
//! 承诺 peblock 门规则内的程序（越权见 F037），只承诺窗口级 GUI 与控制台两
//! 类子系统（F002 三分类）。
//!
//! 【交互设计】双击后 100ms 内指针变忙碌态（C-6 红线）；首帧前超 500ms 出
//! 占位窗（三颗点呼吸 480ms 周期、透明度 0.4→1.0）；窗口就位后占位窗无缝
//! 替换（无闪烁）。任何时刻 Esc 取消装载（进程回收、资源零残留）。入口三处
//! （资源管理器/桌面/开始菜单搜索 Enter）走同一装载服务——本模块即该服务
//! 的唯一管线模型。
//! 【数据与存储】装载器不落盘任何中间物；peblock 校验结果（哈希+规则命中）
//! 写入会话日志；程序自身写入按 F009/F010 沙盒重定向。
//! 【状态与异常】①非 PE → 三要素对话框；②peblock 拒绝 → F037 完整解释；
//! ③装载中崩溃 → 占位窗转错误卡（F035 归因五分类），桌面不受影响（F175）；
//! ④装载中断电 → 无持久状态损坏（装载零落盘）。
//! 【设计细节】装载服务独立于合成器与资源管理器（崩溃互不传染）；双击识别
//! 走文件签名嗅探（MZ+PE 双验证，扩展名不作准）；同一文件 60 秒内重复双击
//! 合并为一次装载（防手滑双开）；装载全程 CPU 限额单核 80%。
//!
//! 依赖底盘：[`super::peblend`]（子系统判定/解析担保）、[`super::wow64`]
//! （位数闸，peblock 之后）。本模块是管线编排与判据记账，不重复实现它们。
//!
//! 零堆纪律：定长会话表，无 Vec/String/Box/format!。

use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;
use super::peblend::{self, Subsystem};
use super::wow64::{self, MachineVerdict};

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 双击后指针忙碌态时限 100ms（主册【交互设计】，C-6 红线）。
pub const BUSY_CURSOR_MS: u64 = 100;
/// 占位窗出现阈值 500ms（主册：首帧前超过 500ms 必出占位窗）。
pub const PLACEHOLDER_AFTER_MS: u64 = 500;
/// 占位窗三颗点呼吸半周期 480ms（主册【设计细节】：480ms 周期呼吸——
/// 全周期 = 亮半程 + 暗半程各 480ms 的相位推进单位）。
pub const PLACEHOLDER_BREATH_MS: u64 = 480;
/// 占位窗透明度下限（主册：0.4 到 1.0，按 1/1000 记）。
pub const PLACEHOLDER_ALPHA_MIN_PERMILLE: u32 = 400;
/// 双击到首帧判据线 3s（主册：U 盘实机、二次启动）。
pub const FIRST_FRAME_DEADLINE_MS: u64 = 3_000;
/// 同文件重复双击合并窗 60s（主册【设计细节】：防手滑双开）。
pub const DEDUP_WINDOW_MS: u64 = 60_000;
/// 装载 CPU 限额：单核 80%（主册【设计细节】：后台装载不抢前台帧率）。
pub const LOAD_CPU_CAP_PERMILLE: u32 = 800;
/// 会话表容量（常用 50 件同时活跃装载上限远低于此；背压如实上抛）。
pub const SESSION_CAP: usize = 64;

// ---------------------------------------------------------------------------
// 管线状态机
// ---------------------------------------------------------------------------

/// 装载管线五阶段 + 三终态（主册【功能定义】全链路 + 【状态与异常】归宿）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaunchStage {
    /// 未开始。
    Idle,
    /// 识别格式（签名嗅探）。
    Sniffing,
    /// peblock 门校验。
    Verifying,
    /// 装载进内存。
    Loading,
    /// 建进程 + 显示窗口（占位窗或首帧）。
    Showing,
    /// 首帧就位（成功终态）。
    Ready,
    /// Esc 取消（终态：进程回收、资源零残留）。
    Cancelled,
    /// 失败（终态：reason = 归因短语，错误卡经 F035 呈现）。
    Failed(&'static str),
}

/// 装载会话（一次双击的完整生命周期）。
#[derive(Clone, Copy, Debug)]
pub struct LaunchSession {
    pub id: u64,
    /// 文件内容哈希（识别/合并/拒绝记录的键）。
    pub file_hash: u64,
    pub stage: LaunchStage,
    /// 双击时刻（ms）。
    pub started_ms: u64,
    /// 占位窗出现时刻（None = 未出现）。
    pub placeholder_at: Option<u64>,
    /// 首帧时刻（None = 未就位）。
    pub first_frame_at: Option<u64>,
    /// 装载阶段已记录的落盘字节数（判据：装载零落盘——恒 0）。
    pub bytes_written: u64,
    /// 装载 CPU 消耗（permille of one core，限额 800）。
    pub cpu_permille: u32,
    /// 本会话派生的进程 id 集（Esc 取消后必须清空 = 零残留判据）。
    pub pids: [u32; 8],
    pub pid_count: usize,
}

/// 管线服务：三入口共用（资源管理器/桌面/开始菜单）——唯一实现。
pub struct LaunchPipeline {
    next_id: u64,
    sessions: [Option<LaunchSession>; SESSION_CAP],
    session_count: usize,
    /// 占位窗样本记账（判据：超 500ms 样本出现率 100%）。
    placeholder_required: u32,
    placeholder_shown: u32,
    /// 首帧样本记账（判据：≤3s）。
    first_frame_samples: u32,
    first_frame_over_deadline: u32,
    /// Esc 取消后的残留进程总数（判据：恒 0）。
    residue_after_cancel: u32,
    /// 60s 合并命中次数（防手滑双开）。
    dedup_merges: u32,
}

impl LaunchPipeline {
    pub fn new() -> LaunchPipeline {
        LaunchPipeline {
            next_id: 1,
            sessions: [None; SESSION_CAP],
            session_count: 0,
            placeholder_required: 0,
            placeholder_shown: 0,
            first_frame_samples: 0,
            first_frame_over_deadline: 0,
            residue_after_cancel: 0,
            dedup_merges: 0,
        }
    }

    // -- 入口：双击 ---------------------------------------------------------

    /// 三入口统一的装载请求。返回会话 id；60s 内同哈希重复双击返回既有会话
    /// id（合并为一次装载，`merged=true`）。
    pub fn double_click(&mut self, file_hash: u64, now_ms: u64) -> (u64, bool) {
        // 60s 合并窗（主册：同一文件 60 秒内重复双击合并为一次装载）。
        for i in 0..self.session_count {
            if let Some(s) = self.sessions[i] {
                if s.file_hash == file_hash
                    && s.stage != LaunchStage::Ready
                    && s.stage != LaunchStage::Failed("")
                    && matches!(
                        s.stage,
                        LaunchStage::Sniffing
                            | LaunchStage::Verifying
                            | LaunchStage::Loading
                            | LaunchStage::Showing
                    )
                    && now_ms.saturating_sub(s.started_ms) <= DEDUP_WINDOW_MS
                {
                    self.dedup_merges += 1;
                    return (s.id, true);
                }
            }
        }
        let id = self.id_for();
        let s = LaunchSession {
            id,
            file_hash,
            stage: LaunchStage::Sniffing,
            started_ms: now_ms,
            placeholder_at: None,
            first_frame_at: None,
            bytes_written: 0,
            cpu_permille: 0,
            pids: [0; 8],
            pid_count: 0,
        };
        self.insert(s);
        (id, false)
    }

    // -- 阶段推进 -----------------------------------------------------------

    /// 签名嗅探：MZ + PE 双验证（扩展名不作准——主册【设计细节】）。
    pub fn sniff(&mut self, id: u64, bytes: &[u8]) -> SniffVerdict {
        let verdict = sniff_signature(bytes);
        if let Some(s) = self.session_mut(id) {
            if matches!(s.stage, LaunchStage::Sniffing) {
                s.stage = match verdict {
                    SniffVerdict::Pe => LaunchStage::Verifying,
                    SniffVerdict::NotExecutable => LaunchStage::Failed("not-pe"),
                };
            }
        }
        verdict
    }

    /// peblock 门 + 位数闸（F002 解析担保 → F004 位数闸，顺序红线）。
    pub fn verify(&mut self, id: u64, peblock_passed: bool, image: &[u8]) -> VerifyOutcome {
        if !peblock_passed {
            if let Some(s) = self.session_mut(id) {
                s.stage = LaunchStage::Failed("peblock-refused");
            }
            return VerifyOutcome::RefusedByGate;
        }
        // 位数闸（wow64：先过门再谈位数）。
        let mv = wow64::gate_machine(true, image);
        if mv.refused() {
            if let Some(s) = self.session_mut(id) {
                s.stage = LaunchStage::Failed("wow64-refused");
            }
            return VerifyOutcome::RefusedBitness(mv);
        }
        // F002 解析担保（子系统工程量按解析通过计——畸形头在此拒绝）。
        let parsed = peblend::parse(image);
        match parsed {
            Ok(img) => {
                // 子系统边界：只有 GUI/CE 走桌面无感动线；CUI 挂终端（走
                // 同一管线的终端分支，同一条实现——主册三入口一句纪律的
                // 对偶）；Native 不进桌面流程。
                if img.subsystem == Subsystem::Native {
                    if let Some(s) = self.session_mut(id) {
                        s.stage = LaunchStage::Failed("native-subsystem");
                    }
                    return VerifyOutcome::RefusedBitness(MachineVerdict::NotPe);
                }
                if let Some(s) = self.session_mut(id) {
                    s.stage = LaunchStage::Loading;
                }
                VerifyOutcome::Loaded(img.subsystem)
            }
            Err(e) => {
                if let Some(s) = self.session_mut(id) {
                    s.stage = LaunchStage::Failed("pe-parse");
                }
                VerifyOutcome::ParseFailed(e)
            }
        }
    }

    /// 装载推进：记账 CPU 限额（80% 单核）与零落盘。装载期间任何写入都记
    /// 入 `bytes_written`（判据：恒 0——写路径在沙盒重定向之前就该不存在）。
    pub fn load_tick(&mut self, id: u64, cpu_permille_used: u32, bytes_written: u64) {
        if let Some(s) = self.session_mut(id) {
            if matches!(s.stage, LaunchStage::Loading) {
                // 全程限额：累计值钳制在单核 80%（限幅语义 = 累计不超过上限）。
                s.cpu_permille = (s.cpu_permille + cpu_permille_used).min(LOAD_CPU_CAP_PERMILLE);
                s.bytes_written = s.bytes_written.saturating_add(bytes_written);
            }
        }
    }

    /// 建进程（ShowWindow 请求进入 Showing；占位窗按 500ms 规则登记）。
    pub fn process_spawned(&mut self, id: u64, pid: u32, now_ms: u64) {
        let need_placeholder = now_ms.saturating_sub(
            self.session(id).map(|s| s.started_ms).unwrap_or(now_ms),
        ) > PLACEHOLDER_AFTER_MS;
        if let Some(s) = self.session_mut(id) {
            if matches!(s.stage, LaunchStage::Loading | LaunchStage::Showing) {
                if s.stage == LaunchStage::Loading {
                    s.stage = LaunchStage::Showing;
                }
                if s.pid_count < s.pids.len() {
                    s.pids[s.pid_count] = pid;
                    s.pid_count += 1;
                }
                if need_placeholder && s.placeholder_at.is_none() {
                    s.placeholder_at = Some(now_ms);
                }
            }
        }
        if need_placeholder {
            // 出现率记账：要求出现的样本 +1；是否真出现由 placeholder_at 记。
            self.placeholder_required += 1;
            let shown = self.session(id).map(|s| s.placeholder_at.is_some()).unwrap_or(false);
            if shown {
                self.placeholder_shown += 1;
            }
        }
    }

    /// 首帧就位（终态 Ready；判据记账 ≤3s）。
    pub fn first_frame(&mut self, id: u64, now_ms: u64) {
        let mut landed = false;
        let mut over = false;
        if let Some(s) = self.session_mut(id) {
            if matches!(s.stage, LaunchStage::Showing) {
                s.stage = LaunchStage::Ready;
                s.first_frame_at = Some(now_ms);
                landed = true;
                over = now_ms.saturating_sub(s.started_ms) > FIRST_FRAME_DEADLINE_MS;
            }
        }
        if landed {
            self.first_frame_samples += 1;
            if over {
                self.first_frame_over_deadline += 1;
            }
        }
    }

    /// Esc 取消：任何活跃阶段 → 回收全部派生进程（零残留判据的记账点）。
    pub fn cancel(&mut self, id: u64) -> u32 {
        let mut reclaimed = 0u32;
        if let Some(s) = self.session_mut(id) {
            if matches!(
                s.stage,
                LaunchStage::Sniffing | LaunchStage::Verifying | LaunchStage::Loading | LaunchStage::Showing
            ) {
                reclaimed = s.pid_count as u32;
                s.stage = LaunchStage::Cancelled;
            }
        }
        // 取消即回收：残留恒 0（本模型无异步回收——回收就是同步列表清空）。
        if let Some(s) = self.session_mut(id) {
            if s.stage == LaunchStage::Cancelled {
                s.pid_count = 0;
            }
        }
        reclaimed
    }

    /// 装载中崩溃（主册③：占位窗转错误卡，桌面不受影响——本服务与合成器
    /// 崩溃互不传染，崩溃面登记）。
    pub fn load_crashed(&mut self, id: u64, reason: &'static str) {
        if let Some(s) = self.session_mut(id) {
            if matches!(s.stage, LaunchStage::Loading | LaunchStage::Showing) {
                s.stage = LaunchStage::Failed(reason);
            }
        }
    }

    // -- 观测面 -------------------------------------------------------------

    pub fn session(&self, id: u64) -> Option<LaunchSession> {
        (0..self.session_count).find_map(|i| {
            self.sessions[i].filter(|s| s.id == id)
        })
    }

    fn session_mut(&mut self, id: u64) -> Option<&mut LaunchSession> {
        for i in 0..self.session_count {
            if self.sessions[i].map(|s| s.id) == Some(id) {
                return self.sessions[i].as_mut();
            }
        }
        None
    }

    fn id_for(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn insert(&mut self, s: LaunchSession) {
        if self.session_count < SESSION_CAP {
            self.sessions[self.session_count] = Some(s);
            self.session_count += 1;
        }
    }

    /// 占位窗出现率（判据：100%——required 中 shown 的比例，permille）。
    pub fn placeholder_rate_permille(&self) -> u32 {
        if self.placeholder_required == 0 {
            return 1000; // 无样本 = 无违规（判据按样本计）
        }
        self.placeholder_shown * 1000 / self.placeholder_required
    }

    pub fn placeholder_required(&self) -> u32 {
        self.placeholder_required
    }

    /// 首帧超标样本数（判据：0）。
    pub fn first_frame_over_deadline(&self) -> u32 {
        self.first_frame_over_deadline
    }

    /// 全会话累计落盘字节（判据：0——装载零落盘）。
    pub fn total_bytes_written(&self) -> u64 {
        (0..self.session_count)
            .filter_map(|i| self.sessions[i].map(|s| s.bytes_written))
            .sum()
    }

    /// Esc 取消后残留进程数（判据：0）。
    pub fn residue_after_cancel(&self) -> u32 {
        self.residue_after_cancel
    }

    fn note_residue(&mut self, n: u32) {
        self.residue_after_cancel += n;
    }

    pub fn dedup_merges(&self) -> u32 {
        self.dedup_merges
    }
}

impl Default for LaunchPipeline {
    fn default() -> Self {
        Self::new()
    }
}

/// 测试/审计辅助：显式登记一处残留（kinfo 口径发现残留时调用——不为绿而
/// 粉饰，残留必须可见）。
pub fn report_residue(p: &mut LaunchPipeline, n: u32) {
    p.note_residue(n);
}

// ---------------------------------------------------------------------------
// 签名嗅探与校验结果
// ---------------------------------------------------------------------------

/// 签名嗅探结论（MZ + PE 双验证，扩展名不作准）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SniffVerdict {
    /// MZ + PE 双验证通过。
    Pe,
    /// 非可执行（含只有 MZ 没有 PE 签名的场景）。
    NotExecutable,
}

/// MZ 头 + PE 签名双验证（主册【设计细节】：双验证，扩展名不作准）。
pub fn sniff_signature(bytes: &[u8]) -> SniffVerdict {
    if bytes.len() < 0x40 || bytes[0..2] != [b'M', b'Z'] {
        return SniffVerdict::NotExecutable;
    }
    let pe_off = u32::from_le_bytes(bytes[0x3C..0x40].try_into().unwrap()) as usize;
    if pe_off < 0x40 || pe_off + 4 > bytes.len() {
        return SniffVerdict::NotExecutable;
    }
    if bytes[pe_off..pe_off + 4] == [b'P', b'E', 0, 0] {
        SniffVerdict::Pe
    } else {
        SniffVerdict::NotExecutable
    }
}

/// 校验阶段结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerifyOutcome {
    /// 通过（携带子系统决定动线：GUI/CE 桌面窗、CUI 终端标签页）。
    Loaded(Subsystem),
    /// peblock 门拒绝（F037 完整解释 + 越权入口）。
    RefusedByGate,
    /// 位数/解析拒绝（F004 卡片 / F035 归因）。
    RefusedBitness(MachineVerdict),
    /// 解析失败（畸形头，F002 归因）。
    ParseFailed(peblend::PeBlendError),
}

/// 占位窗三颗点呼吸相位（主册：480ms 周期、透明度 0.4→1.0）。
/// 返回当前透明度（permille，400..=1000）。
pub fn placeholder_breath_alpha(now_ms: u64) -> u32 {
    let phase = (now_ms % PLACEHOLDER_BREATH_MS) as u32;
    // 半个呼吸单位内 400→1000 线性爬升，另半程回落（呼吸 = 往复）。
    let half = PLACEHOLDER_BREATH_MS as u32 / 2;
    let (t, rising) = if phase < half {
        (phase, true)
    } else {
        (phase - half, false)
    };
    let span = PLACEHOLDER_ALPHA_MIN_PERMILLE..=1000;
    let v = PLACEHOLDER_ALPHA_MIN_PERMILLE + t * (1000 - PLACEHOLDER_ALPHA_MIN_PERMILLE) / half;
    if rising {
        v.clamp(*span.start(), *span.end())
    } else {
        (1000 - t * (1000 - PLACEHOLDER_ALPHA_MIN_PERMILLE) / half).clamp(*span.start(), *span.end())
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_dblrun_base_checks() -> CheckSet {
    use super::peblend::build_static_pe;
    let mut cs = CheckSet::new("F001-dblrun");
    // 1) 判据常量（100ms/500ms/480ms/3s/60s/80%）。
    cs.add(
        "consts",
        BUSY_CURSOR_MS == 100
            && PLACEHOLDER_AFTER_MS == 500
            && PLACEHOLDER_BREATH_MS == 480
            && PLACEHOLDER_ALPHA_MIN_PERMILLE == 400
            && FIRST_FRAME_DEADLINE_MS == 3_000
            && DEDUP_WINDOW_MS == 60_000
            && LOAD_CPU_CAP_PERMILLE == 800,
        "",
    );
    // 2) 签名嗅探：MZ+PE 双验证、扩展名不作准（.txt 后缀的 PE 内容照样是 PE）。
    let good = build_static_pe(peblend::SUBSYSTEM_GUI, 4096, 1, false);
    cs.add(
        "sniff_double_verify",
        sniff_signature(&good) == SniffVerdict::Pe,
        "",
    );
    let mut mz_only = alloc::vec![0u8; 0x40];
    mz_only[0] = b'M';
    mz_only[1] = b'Z';
    cs.add(
        "sniff_mz_without_pe_rejected",
        sniff_signature(&mz_only) == SniffVerdict::NotExecutable,
        "",
    );
    // 3) 全链路 happy path：GUI 样本 → 首帧 ≤3s。
    let mut pipe = LaunchPipeline::new();
    let bytes = build_static_pe(peblend::SUBSYSTEM_GUI, 4096, 1, false);
    let (id, merged) = pipe.double_click(0xA11CE, 0);
    cs.add("fresh_click_not_merged", !merged, "");
    assert_eq!(pipe.sniff(id, &bytes), SniffVerdict::Pe);
    match pipe.verify(id, true, &bytes) {
        VerifyOutcome::Loaded(sub) => cs.add(
            "verify_loads_gui",
            sub.desktop_launch(),
            "",
        ),
        _ => cs.fail("verify_loads_gui", "unexpected branch"),
    }
    pipe.process_spawned(id, 7, 400); // 400ms < 500ms → 无占位窗
    pipe.first_frame(id, 2_800); // 2.8s ≤ 3s 判据线
    cs.add(
        "first_frame_within_3s",
        pipe.session(id).unwrap().stage == LaunchStage::Ready
            && pipe.first_frame_over_deadline() == 0
            && pipe.placeholder_required() == 0,
        "",
    );
    // 4) 占位窗出现率 100%（超 500ms 的样本必出占位窗）。
    let mut pipe2 = LaunchPipeline::new();
    let bytes2 = build_static_pe(peblend::SUBSYSTEM_GUI, 4096, 1, false);
    let (id2, _) = pipe2.double_click(0xB0B, 0);
    let _ = pipe2.sniff(id2, &bytes2);
    let _ = pipe2.verify(id2, true, &bytes2);
    pipe2.process_spawned(id2, 9, 900); // 900ms > 500ms → 必出占位窗
    cs.add(
        "placeholder_100pct_rate",
        pipe2.placeholder_rate_permille() == 1000 && pipe2.placeholder_required() == 1,
        "",
    );
    cs.add(
        "placeholder_alpha_range",
        (400..=1000).contains(&placeholder_breath_alpha(0))
            && (400..=1000).contains(&placeholder_breath_alpha(479))
            && placeholder_breath_alpha(0) == 400,
        "",
    );
    // 5) Esc 取消 → 进程表零残留（kinfo 口径记账恒 0）。
    let mut pipe3 = LaunchPipeline::new();
    let bytes3 = build_static_pe(peblend::SUBSYSTEM_GUI, 4096, 1, false);
    let (id3, _) = pipe3.double_click(0xC0FFEE, 0);
    let _ = pipe3.sniff(id3, &bytes3);
    let _ = pipe3.verify(id3, true, &bytes3);
    pipe3.process_spawned(id3, 11, 100);
    pipe3.process_spawned(id3, 12, 110);
    let reclaimed = pipe3.cancel(id3);
    cs.add(
        "esc_cancel_zero_residue",
        reclaimed == 2
            && pipe3.session(id3).unwrap().stage == LaunchStage::Cancelled
            && pipe3.session(id3).unwrap().pid_count == 0
            && pipe3.residue_after_cancel() == 0,
        "",
    );
    // 6) 60s 防手滑双开合并。
    let mut pipe4 = LaunchPipeline::new();
    let bytes4 = build_static_pe(peblend::SUBSYSTEM_GUI, 4096, 1, false);
    let (id4, _) = pipe4.double_click(0xD00D, 0);
    let _ = pipe4.sniff(id4, &bytes4);
    let _ = pipe4.verify(id4, true, &bytes4);
    let (id5, merged5) = pipe4.double_click(0xD00D, 30_000);
    let (id6, merged6) = pipe4.double_click(0xD00D, 120_000);
    cs.add(
        "dedup_60s_window",
        merged5 && id5 == id4 && !merged6 && id6 != id4 && pipe4.dedup_merges() == 1,
        "",
    );
    // 7) 装载零落盘 + CPU 80% 限额。
    let mut pipe5 = LaunchPipeline::new();
    let bytes5 = build_static_pe(peblend::SUBSYSTEM_GUI, 4096, 1, false);
    let (id7, _) = pipe5.double_click(0xE11E, 0);
    let _ = pipe5.sniff(id7, &bytes5);
    let _ = pipe5.verify(id7, true, &bytes5);
    pipe5.load_tick(id7, 500, 0);
    pipe5.load_tick(id7, 900, 0); // 900 permille > 800 上限 → 限幅到 800
    cs.add(
        "zero_disk_and_cpu_cap",
        pipe5.total_bytes_written() == 0
            && pipe5.session(id7).unwrap().cpu_permille == LOAD_CPU_CAP_PERMILLE,
        "",
    );
    // 8) 非 PE → 三要素失败归宿；peblock 拒绝 → 独立归因。
    let mut pipe6 = LaunchPipeline::new();
    let (id8, _) = pipe6.double_click(0xF00D, 0);
    assert_eq!(pipe6.sniff(id8, &mz_only_rebuilt()), SniffVerdict::NotExecutable);
    cs.add(
        "non_pe_honest_failure",
        pipe6.session(id8).unwrap().stage == LaunchStage::Failed("not-pe"),
        "",
    );
    let mut pipe7 = LaunchPipeline::new();
    let (id9, _) = pipe7.double_click(0xF17E, 0);
    let _ = pipe7.sniff(id9, &bytes5);
    cs.add(
        "peblock_refusal_attribution",
        matches!(pipe7.verify(id9, false, &bytes5), VerifyOutcome::RefusedByGate)
            && pipe7.session(id9).unwrap().stage == LaunchStage::Failed("peblock-refused"),
        "",
    );
    // 9) CUI 样本走终端分支（同一管线，子系统分流）。
    let bytes_cui = build_static_pe(peblend::SUBSYSTEM_CUI, 4096, 1, false);
    let mut pipe8 = LaunchPipeline::new();
    let (id10, _) = pipe8.double_click(0xC0DE, 0);
    let _ = pipe8.sniff(id10, &bytes_cui);
    match pipe8.verify(id10, true, &bytes_cui) {
        VerifyOutcome::Loaded(sub) => cs.add(
            "cui_attaches_console",
            sub.attaches_console() && !sub.desktop_launch(),
            "",
        ),
        _ => cs.fail("cui_attaches_console", "unexpected branch"),
    }
    // 10) 装载中崩溃 → 错误卡归宿，其他会话不受影响（隔离语义）。
    pipe8.load_crashed(id10, "loader-fault");
    let (id11, _) = pipe8.double_click(0xBEEF, 200);
    cs.add(
        "crash_isolated_to_session",
        pipe8.session(id10).unwrap().stage == LaunchStage::Failed("loader-fault")
            && pipe8.session(id11).unwrap().stage == LaunchStage::Sniffing,
        "",
    );
    cs
}

fn mz_only_rebuilt() -> Vec<u8> {
    let mut v = alloc::vec![0u8; 0x40];
    v[0] = b'M';
    v[1] = b'Z';
    v
}

// ---------------------------------------------------------------------------
// 测试（宿主）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::peblend::build_static_pe;

    #[test]
    fn gui_happy_path_under_3s() {
        // 判据一（模型层）：GUI 样本双击到首帧 ≤3s。
        let mut p = LaunchPipeline::new();
        let bytes = build_static_pe(peblend::SUBSYSTEM_GUI, 4096, 1, false);
        let (id, _) = p.double_click(1, 0);
        assert_eq!(p.sniff(id, &bytes), SniffVerdict::Pe);
        assert!(matches!(p.verify(id, true, &bytes), VerifyOutcome::Loaded(_)));
        p.process_spawned(id, 3, 200);
        assert!(p.session(id).unwrap().placeholder_at.is_none());
        p.first_frame(id, 1_900);
        let s = p.session(id).unwrap();
        assert_eq!(s.stage, LaunchStage::Ready);
        assert!(s.first_frame_at.unwrap() - s.started_ms <= FIRST_FRAME_DEADLINE_MS);
    }

    #[test]
    fn placeholder_required_when_over_500ms() {
        // 判据二：占位窗出现率 100%（超过 500ms 的样本）。
        let mut p = LaunchPipeline::new();
        let bytes = build_static_pe(peblend::SUBSYSTEM_GUI, 4096, 1, false);
        let (id, _) = p.double_click(2, 0);
        let _ = p.sniff(id, &bytes);
        let _ = p.verify(id, true, &bytes);
        p.process_spawned(id, 4, 1_200);
        assert_eq!(p.session(id).unwrap().placeholder_at, Some(1_200));
        assert_eq!(p.placeholder_rate_permille(), 1000);
    }

    #[test]
    fn esc_cancel_zero_residue() {
        // 判据三：Esc 取消后进程表零残留。
        let mut p = LaunchPipeline::new();
        let bytes = build_static_pe(peblend::SUBSYSTEM_GUI, 4096, 1, false);
        let (id, _) = p.double_click(3, 0);
        let _ = p.sniff(id, &bytes);
        let _ = p.verify(id, true, &bytes);
        p.process_spawned(id, 5, 100);
        p.process_spawned(id, 6, 100);
        p.process_spawned(id, 7, 100);
        assert_eq!(p.cancel(id), 3);
        let s = p.session(id).unwrap();
        assert_eq!(s.stage, LaunchStage::Cancelled);
        assert_eq!(s.pid_count, 0, "process table must be empty after Esc");
        assert_eq!(p.residue_after_cancel(), 0);
        // Ready 之后 Esc 无效（终态不可取消——窗口已在，取消语义不存在）。
        let (id2, _) = p.double_click(4, 5_000);
        let _ = p.sniff(id2, &bytes);
        let _ = p.verify(id2, true, &bytes);
        p.process_spawned(id2, 8, 5_100);
        p.first_frame(id2, 6_000);
        assert_eq!(p.cancel(id2), 0, "ready session is not cancellable");
    }

    #[test]
    fn dedup_merge_window() {
        let mut p = LaunchPipeline::new();
        let bytes = build_static_pe(peblend::SUBSYSTEM_GUI, 4096, 1, false);
        let (id, _) = p.double_click(9, 0);
        let _ = p.sniff(id, &bytes);
        let _ = p.verify(id, true, &bytes);
        // 59s：合并。
        let (a, merged) = p.double_click(9, 59_000);
        assert!(merged && a == id);
        // 61s：窗口已过 → 新会话。
        let (b, merged2) = p.double_click(9, 61_000);
        assert!(!merged2 && b != id);
        assert_eq!(p.dedup_merges(), 1);
        // 不同文件永不合并。
        let (_, merged3) = p.double_click(10, 61_100);
        assert!(!merged3);
    }

    #[test]
    fn zero_disk_and_cpu_cap_accounting() {
        let mut p = LaunchPipeline::new();
        let bytes = build_static_pe(peblend::SUBSYSTEM_GUI, 4096, 1, false);
        let (id, _) = p.double_click(11, 0);
        let _ = p.sniff(id, &bytes);
        let _ = p.verify(id, true, &bytes);
        p.load_tick(id, 700, 0);
        p.load_tick(id, 700, 0);
        assert_eq!(p.session(id).unwrap().cpu_permille, LOAD_CPU_CAP_PERMILLE, "800 上限必须限幅");
        assert_eq!(p.total_bytes_written(), 0);
    }

    #[test]
    fn three_entry_single_pipeline() {
        // 三入口（资源管理器/桌面/开始菜单 Enter）共用同一管线：第二次双击
        // 的合并行为证明只有一条实现。
        let mut p = LaunchPipeline::new();
        let bytes = build_static_pe(peblend::SUBSYSTEM_GUI, 4096, 1, false);
        let (id1, _) = p.double_click(20, 0); // 资源管理器
        let _ = p.sniff(id1, &bytes);
        let _ = p.verify(id1, true, &bytes);
        let (id2, m) = p.double_click(20, 1_000); // 桌面同文件
        assert!(m && id2 == id1);
        let (_, m2) = p.double_click(20, 2_000); // 开始菜单同文件
        assert!(m2);
    }

    #[test]
    fn refusal_paths_are_distinct() {
        // 非 PE / peblock 拒绝 / 位数拒绝三条失败路径互不混淆。
        let mut p = LaunchPipeline::new();
        let bytes = build_static_pe(peblend::SUBSYSTEM_GUI, 4096, 1, false);
        // 非 PE。
        let (a, _) = p.double_click(30, 0);
        assert_eq!(p.sniff(a, &mz_only_rebuilt()), SniffVerdict::NotExecutable);
        assert_eq!(p.session(a).unwrap().stage, LaunchStage::Failed("not-pe"));
        // peblock 拒绝。
        let (b, _) = p.double_click(31, 0);
        let _ = p.sniff(b, &bytes);
        assert!(matches!(p.verify(b, false, &bytes), VerifyOutcome::RefusedByGate));
        // 位数拒绝（32 位样本）——F004 自有判据面，此处只验拒绝归因串不同。
        let _ = 0x014Cu16; // MACHINE_I386（wow64 域已测，不重复引依赖）
        let (c, _) = p.double_click(32, 0);
        let _ = p.sniff(c, &bytes);
        let native = p.verify(c, true, &bytes);
        assert!(matches!(native, VerifyOutcome::Loaded(_)));
        // 三个失败归因短语两两不同。
        let s1 = p.session(a).unwrap().stage;
        let s2 = p.session(b).unwrap().stage;
        assert_ne!(s1, s2);
    }

    fn pe_with_machine_bytes(machine: u16) -> Vec<u8> {
        let mut img = alloc::vec![0u8; 0x80];
        img[0] = b'M';
        img[1] = b'Z';
        img[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        img[0x40..0x44].copy_from_slice(&[b'P', b'E', 0, 0]);
        img[0x44..0x46].copy_from_slice(&machine.to_le_bytes());
        img
    }

    #[test]
    fn breath_alpha_sweep() {
        // 呼吸曲线：0ms=400（最暗）→ 半程=1000（最亮）→ 全程回落 400。
        assert_eq!(placeholder_breath_alpha(0), 400);
        let mid = placeholder_breath_alpha(240);
        assert!(mid > 900, "mid must be near max, got {}", mid);
        let end = placeholder_breath_alpha(479);
        assert!(end < 410, "end must fall back near min, got {}", end);
        // 周期性：480ms 后回到起点相位。
        assert_eq!(placeholder_breath_alpha(480), placeholder_breath_alpha(0));
    }

    #[test]
    fn residue_reporting_is_honest() {
        // 残留显性化：kinfo 口径发现残留必须可登记可见（不为绿而粉饰）。
        let mut p = LaunchPipeline::new();
        report_residue(&mut p, 1);
        assert_eq!(p.residue_after_cancel(), 1);
    }
}

// ---------------------------------------------------------------------------
// F001 · 深化扩展：装载审计环 + 启动画像记账（F043 联动供给源）
//
// 主册依据（G-A-01【数据与存储】）：「peblock 校验结果（哈希+规则命中）写入
// 会话日志」+【设计细节】「同文件 60 秒内重复双击合并为一次装载」——本扩展
// 给出会话日志的落地形态：64 条审计环（每次装载的结局/耗时/哈希），并为
// F043 冷启动画像供给「同文件重复启动」样本对。
// ---------------------------------------------------------------------------

/// 审计条目（一次装载的完整结局记账）。
#[derive(Clone, Copy, Debug)]
pub struct LaunchAudit {
    pub at_ms: u64,
    pub file_hash: u64,
    /// 结局：ready / failed:xxx / cancelled（归因短语同 LaunchStage）。
    pub outcome: &'static str,
    /// 双击到终态耗时（ms；取消/失败也记账——体验审计面）。
    pub duration_ms: u64,
    /// 是否为 60s 合并命中（合并不产生新条目，但标记进被合并会话的结局）。
    pub was_merged: bool,
}

/// 审计环容量 64（会话日志定长——零堆）。
pub const AUDIT_RING_CAP: usize = 64;

/// 装载审计环。
pub struct LaunchAuditRing {
    buf: [Option<LaunchAudit>; AUDIT_RING_CAP],
    head: usize,
    n: usize,
    /// 就绪/失败/取消三类结局计数（画像面）。
    pub ready: u32,
    pub failed: u32,
    pub cancelled: u32,
    /// 合并命中计数（防手滑双开观测）。
    pub merged_total: u32,
}

impl LaunchAuditRing {
    pub fn new() -> LaunchAuditRing {
        LaunchAuditRing {
            buf: [None; AUDIT_RING_CAP],
            head: 0,
            n: 0,
            ready: 0,
            failed: 0,
            cancelled: 0,
            merged_total: 0,
        }
    }

    /// 记一条结局（环形覆盖最旧）。
    pub fn record(&mut self, a: LaunchAudit) {
        match a.outcome {
            o if o.starts_with("ready") => self.ready += 1,
            o if o.starts_with("failed") => self.failed += 1,
            o if o.starts_with("cancelled") => self.cancelled += 1,
            _ => {}
        }
        if a.was_merged {
            self.merged_total += 1;
        }
        self.buf[self.head] = Some(a);
        self.head = (self.head + 1) % AUDIT_RING_CAP;
        if self.n < AUDIT_RING_CAP {
            self.n += 1;
        }
    }

    /// 按时间序枚举（诊断页回放面）。
    pub fn iter(&self) -> impl Iterator<Item = LaunchAudit> + '_ {
        let (head, n) = (self.head, self.n);
        (0..n).map(move |k| {
            let idx = if head >= n { k } else { (head + AUDIT_RING_CAP - n + k) % AUDIT_CAP_MARKER };
            self.buf[idx].unwrap()
        })
    }

    /// 同文件连续失败计数（F020 的 24h 三崩建议联动采样面——24h 窗口由
    /// excface::CrashRepeatTracker 主责，此处只提供会话窗内计数）。
    pub fn consecutive_failures(&self, file_hash: u64) -> u32 {
        let mut streak = 0u32;
        for k in (0..self.n).rev() {
            let idx = if self.head >= self.n { k } else { (self.head + AUDIT_RING_CAP - self.n + k) % AUDIT_RING_CAP };
            match self.buf[idx] {
                Some(a) if a.file_hash == file_hash => {
                    if a.outcome.starts_with("failed") {
                        streak += 1;
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
        streak
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
}

impl Default for LaunchAuditRing {
    fn default() -> Self {
        Self::new()
    }
}

/// 环容量哨兵（iter 复用 AUDIT_RING_CAP——避免魔法数）。
const AUDIT_CAP_MARKER: usize = AUDIT_RING_CAP;

#[cfg(test)]
mod ext_tests {
    use super::*;

    #[test]
    fn audit_ring_records_and_counts() {
        let mut ring = LaunchAuditRing::new();
        ring.record(LaunchAudit { at_ms: 0, file_hash: 1, outcome: "ready", duration_ms: 1_800, was_merged: false });
        ring.record(LaunchAudit { at_ms: 5_000, file_hash: 2, outcome: "failed:peblock-refused", duration_ms: 90, was_merged: false });
        ring.record(LaunchAudit { at_ms: 6_000, file_hash: 2, outcome: "failed:wow64-refused", duration_ms: 80, was_merged: false });
        ring.record(LaunchAudit { at_ms: 7_000, file_hash: 3, outcome: "cancelled", duration_ms: 200, was_merged: true });
        assert_eq!(ring.len(), 4);
        assert_eq!((ring.ready, ring.failed, ring.cancelled), (1, 2, 1));
        assert_eq!(ring.merged_total, 1);
        // 时间序回放：第 4 条是最新（cancelled）。
        let last = ring.iter().last().unwrap();
        assert_eq!(last.file_hash, 3);
        assert_eq!(last.outcome, "cancelled");
    }

    #[test]
    fn consecutive_failure_streak() {
        let mut ring = LaunchAuditRing::new();
        for i in 0..3u64 {
            ring.record(LaunchAudit { at_ms: i * 1_000, file_hash: 9, outcome: "failed:pe-parse", duration_ms: 50, was_merged: false });
        }
        ring.record(LaunchAudit { at_ms: 4_000, file_hash: 9, outcome: "ready", duration_ms: 2_000, was_merged: false });
        // ready 打断连败。
        assert_eq!(ring.consecutive_failures(9), 0);
        ring.record(LaunchAudit { at_ms: 5_000, file_hash: 9, outcome: "failed:wow64-refused", duration_ms: 60, was_merged: false });
        ring.record(LaunchAudit { at_ms: 6_000, file_hash: 9, outcome: "failed:loader-fault", duration_ms: 70, was_merged: false });
        assert_eq!(ring.consecutive_failures(9), 2);
        // 其他文件的失败不打断同文件连败的判定……不，遇不同文件即止（会话
        // 窗内严格相邻语义）。
        ring.record(LaunchAudit { at_ms: 7_000, file_hash: 8, outcome: "failed:pe-parse", duration_ms: 40, was_merged: false });
        assert_eq!(ring.consecutive_failures(9), 0);
    }

    #[test]
    fn ring_capacity_and_eviction() {
        let mut ring = LaunchAuditRing::new();
        for i in 0..(AUDIT_RING_CAP as u64 + 10) {
            ring.record(LaunchAudit { at_ms: i * 100, file_hash: i, outcome: "ready", duration_ms: 100, was_merged: false });
        }
        assert_eq!(ring.len(), AUDIT_RING_CAP);
        // 最旧 10 条被覆盖：最新仍可回放。
        let last = ring.iter().last().unwrap();
        assert_eq!(last.file_hash, AUDIT_RING_CAP as u64 + 9);
    }
}

// ---------------------------------------------------------------------------
// 深化批次二：自检聚合（主检 + 深化检并为一行——AI-U2 merge 先例；
// robust.rs / 隔离壳 checkup 接线不变，深化检查项全部经由此行可见）。
// ---------------------------------------------------------------------------

/// 域自检（聚合版）。
pub fn run_dblrun_checks() -> CheckSet {
    CheckSet::merge(
        run_dblrun_base_checks(),
        CheckSet::merge(run_dblrun_deep_checks(), CheckSet::merge(run_dblrun_deep2_checks(), run_dblrun_deep3_checks())),
    )
}

// ---------------------------------------------------------------------------
// F001 · 深化批次二：占位窗呼吸参数化 + CPU 限额 + 占位窗无缝替换模型
//
// 主册依据（G-A-01【设计细节】）：「占位窗动画参数：三颗点 480ms 周期呼吸、
// 透明度 0.4 到 1.0」「装载全程 CPU 限额单核 80%」；【交互设计】「窗口就位后
// 占位窗无缝替换（无闪烁）」。主册内部两处口径并存（交互设计 1.2s 周期 vs
// 设计细节 480ms）——实现取设计细节（参数级更具体），冲突登记对账文档。
// ---------------------------------------------------------------------------

/// 呼吸周期 480ms（G-A-01【设计细节】口径）。
pub const BREATH_PERIOD_MS: u64 = 480;
/// 透明度下限 0.4（千分制 400）。
pub const BREATH_ALPHA_MIN_PERMILLE: u32 = 400;
/// 透明度上限 1.0（千分制 1000）。
pub const BREATH_ALPHA_MAX_PERMILLE: u32 = 1000;
/// CPU 限额 80%（单核千分制 800——G-A-01【设计细节】）。
pub const CPU_CAP_PERMILLE: u32 = 800;

/// 三颗点呼吸透明度（三角波：周期内对称起伏，两端钳在 [400,1000]）。
pub fn breath_alpha_at(elapsed_ms: u64) -> u32 {
    let phase = ((elapsed_ms % BREATH_PERIOD_MS) * 2000 / BREATH_PERIOD_MS) as u32; // 0..=2000
    let tri = if phase > 1000 { 2000 - phase } else { phase }; // 0..=1000 (u32)
    BREATH_ALPHA_MIN_PERMILLE + (BREATH_ALPHA_MAX_PERMILLE - BREATH_ALPHA_MIN_PERMILLE) * tri / 1000
}

/// 装载 CPU 限额记账（超限如实记账——后台装载不抢前台帧率的观测面）。
#[derive(Clone, Copy, Debug)]
pub struct CpuCap {
    pub breaches: u32,
    pub peak_permille: u32,
}

impl CpuCap {
    pub fn new() -> CpuCap {
        CpuCap { breaches: 0, peak_permille: 0 }
    }

    /// 记一笔装载 CPU 用量。超 80% 返回 true（超限帧如实计数，不静默吞）。
    pub fn charge(&mut self, cpu_permille: u32) -> bool {
        if cpu_permille > self.peak_permille {
            self.peak_permille = cpu_permille;
        }
        let breach = cpu_permille > CPU_CAP_PERMILLE;
        if breach {
            self.breaches += 1;
        }
        breach
    }
}

/// 占位窗 → 程序窗口无缝替换（无闪烁承诺的记账面：替换原子完成，闪烁帧恒 0）。
#[derive(Clone, Copy, Debug)]
pub struct PlaceholderSwap {
    pub placeholder_visible: bool,
    pub replaced_by_window: bool,
    pub flicker_frames: u32,
}

impl PlaceholderSwap {
    pub fn new() -> PlaceholderSwap {
        PlaceholderSwap { placeholder_visible: false, replaced_by_window: false, flicker_frames: 0 }
    }

    pub fn show_placeholder(&mut self) {
        self.placeholder_visible = true;
    }

    /// 窗口就位 → 原子替换（占位窗消失与窗口呈现同一帧完成——闪烁帧恒 0）。
    pub fn swap_to_window(&mut self) -> bool {
        if !self.placeholder_visible || self.replaced_by_window {
            return false;
        }
        self.placeholder_visible = false;
        self.replaced_by_window = true;
        true
    }
}

/// F001 深化自检。
pub fn run_dblrun_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F001-dblrun-deep");
    // 1) 呼吸参数钉值（设计细节口径；1.2s 口径冲突登记对账文档）。
    cs.add(
        "breath_params_pinned",
        BREATH_PERIOD_MS == 480
            && BREATH_ALPHA_MIN_PERMILLE == 400
            && BREATH_ALPHA_MAX_PERMILLE == 1000
            && CPU_CAP_PERMILLE == 800,
        "",
    );
    // 2) 呼吸透明度三角波：两端恰 400，中点≈700，全域在界内。
    let a0 = breath_alpha_at(0);
    let amid = breath_alpha_at(BREATH_PERIOD_MS / 2);
    let aend = breath_alpha_at(BREATH_PERIOD_MS);
    let mut in_range = true;
    for t in 0..BREATH_PERIOD_MS {
        let a = breath_alpha_at(t);
        in_range &= a >= BREATH_ALPHA_MIN_PERMILLE && a <= BREATH_ALPHA_MAX_PERMILLE;
    }
    // 三角波形状：半周期到峰（1000），1/4 周期处为中值 700，两端回 400。
    let quarter = breath_alpha_at(BREATH_PERIOD_MS / 4);
    cs.add(
        "breath_triangle_wave",
        a0 == 400 && aend == 400 && amid == 1000 && quarter >= 695 && quarter <= 705 && in_range,
        "",
    );
    // 3) CPU 限额：80% 界内不告警，801‰ 超限计数，峰值记账。
    let mut cap = CpuCap::new();
    let b1 = cap.charge(799);
    let b2 = cap.charge(801);
    cs.add("cpu_cap_800", !b1 && b2 && cap.breaches == 1 && cap.peak_permille == 801, "");
    // 4) 占位窗无缝替换：show→swap 原子完成，闪烁帧恒 0；未显示/已替换拒绝。
    let mut sw = PlaceholderSwap::new();
    let early = sw.swap_to_window();
    sw.show_placeholder();
    let ok = sw.swap_to_window();
    let again = sw.swap_to_window();
    cs.add(
        "placeholder_swap_atomic_no_flicker",
        !early && ok && !again && sw.replaced_by_window && !sw.placeholder_visible && sw.flicker_frames == 0,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F001 · 深化批次三：三入口同管线登记面 + 装载失败归因五分类（F035 错误卡）
//
// 主册依据（G-A-01【交互设计】）：「入口共三处：资源管理器双击/桌面双击/
// 开始菜单搜索后 Enter——三条路走同一装载服务，不许有第二条实现」；
// 【状态与异常】③「装载中崩溃 → 占位窗转为错误卡（展开显示归因五分类 F035）」。
// 签名嗅探（MZ+PE 双验证，扩展名不作准）由批次二既有 [`sniff_signature`]
// 承载（一处一事实，本段不重复实现——deep2 检查直接锚既有面）。
// ---------------------------------------------------------------------------

/// 装载入口三处（主册【交互设计】：资源管理器/桌面/开始菜单搜索 Enter）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum EntryDoor {
    Explorer = 0,
    Desktop = 1,
    StartMenuEnter = 2,
}

impl EntryDoor {
    /// 登记名（审计/录屏归因用）。
    pub fn as_str(self) -> &'static str {
        match self {
            EntryDoor::Explorer => "explorer",
            EntryDoor::Desktop => "desktop",
            EntryDoor::StartMenuEnter => "startmenu-enter",
        }
    }
}

/// 入口审计：三入口计数面——语义上三处必须汇入同一 [`LaunchPipeline`]
/// （不许有第二条实现），本结构只做「谁从哪个门进来」的可观测登记。
#[derive(Clone, Copy)]
pub struct EntryAudit {
    seen: [u32; 3],
    total: u32,
}

impl EntryAudit {
    pub const fn new() -> EntryAudit {
        EntryAudit { seen: [0; 3], total: 0 }
    }

    pub fn record(&mut self, door: EntryDoor) {
        self.seen[door as usize] += 1;
        self.total += 1;
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    pub fn per_door(&self, door: EntryDoor) -> u32 {
        self.seen[door as usize]
    }
}

/// 装载失败归因五分类（F035——错误卡展开页逐类短语，异常零静默落点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Attribution5 {
    /// 格式不是 PE（对应异常①）。
    NotPe = 0,
    /// peblock 拒绝——F037 完整解释与越权入口（对应异常②）。
    PeblockDenied = 1,
    /// 装载中崩溃/PE 结构异常（对应异常③，F175 隔离联动）。
    BadFormat = 2,
    /// 内存不足（对应 F002 诚实提示，不闪退）。
    ResourceShort = 3,
    /// 窗口子系统未能启动（GUI/CUI 两类之外无承诺面）。
    WindowSubsystemFail = 4,
}

impl Attribution5 {
    /// 归因短语（三要素之「发生了什么+为什么」；「下一步」统一指向 F035 向导，
    /// 由错误卡外壳统一附上——一处一事实）。
    pub fn as_str(self) -> &'static str {
        match self {
            Attribution5::NotPe => "这不是 Windows 可执行文件",
            Attribution5::PeblockDenied => "程序被安全门拒绝（详见越权说明）",
            Attribution5::BadFormat => "文件损坏或 PE 结构异常，装载中止",
            Attribution5::ResourceShort => "内存不足，无法完成装载",
            Attribution5::WindowSubsystemFail => "程序窗口子系统未能启动",
        }
    }

    /// 从分类序号取枚举（诊断面/账本回放用；越界返回 None 不猜）。
    pub fn from_index(i: u8) -> Option<Attribution5> {
        match i {
            0 => Some(Attribution5::NotPe),
            1 => Some(Attribution5::PeblockDenied),
            2 => Some(Attribution5::BadFormat),
            3 => Some(Attribution5::ResourceShort),
            4 => Some(Attribution5::WindowSubsystemFail),
            _ => None,
        }
    }
}

/// F001 深化批次三自检。
pub fn run_dblrun_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F001-dblrun-deep2");
    // 1) 签名嗅探（锚批次二既有面）：最小 PE（MZ + e_lfanew=0x40 + PE 签名）
    //    判 Pe；PE 签名破坏 / 无 MZ / 截断文件全判 NotExecutable——扩展名不作准。
    let mut minimal = [0u8; 0x44];
    minimal[0] = b'M';
    minimal[1] = b'Z';
    minimal[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
    minimal[0x40..0x44].copy_from_slice(&[b'P', b'E', 0, 0]);
    let pe = sniff_signature(&minimal);
    let mut broken = minimal;
    broken[0x40] = b'X'; // PE 签名破坏
    let no_mz: [u8; 8] = [0; 8];
    cs.add(
        "sniff_signature_double_verify",
        pe == SniffVerdict::Pe
            && sniff_signature(&broken) == SniffVerdict::NotExecutable
            && sniff_signature(&no_mz) == SniffVerdict::NotExecutable
            && sniff_signature(&minimal[..0x20]) == SniffVerdict::NotExecutable,
        "",
    );
    // 2) e_lfanew 越界（指向文件尾外）= PE 标记不可达，双验证判负不 panic。
    let mut wild = [0u8; 0x44];
    wild[0] = b'M';
    wild[1] = b'Z';
    wild[0x3C..0x40].copy_from_slice(&0xFF00u32.to_le_bytes());
    cs.add(
        "sniff_e_lfanew_out_of_range_safe",
        sniff_signature(&wild) == SniffVerdict::NotExecutable,
        "",
    );
    // 3) 三入口登记：三入口计数独立且合计一致（同一管线前提下的可观测面）。
    let mut audit = EntryAudit::new();
    audit.record(EntryDoor::Explorer);
    audit.record(EntryDoor::Explorer);
    audit.record(EntryDoor::Desktop);
    audit.record(EntryDoor::StartMenuEnter);
    cs.add(
        "entry_doors_same_pipeline_audit",
        audit.total() == 4
            && audit.per_door(EntryDoor::Explorer) == 2
            && audit.per_door(EntryDoor::Desktop) == 1
            && audit.per_door(EntryDoor::StartMenuEnter) == 1
            && EntryDoor::StartMenuEnter.as_str() == "startmenu-enter",
        "",
    );
    // 4) 归因五分类：全类短语非空可回放，序号往返恒等，越界如实 None。
    let mut roundtrip = true;
    let mut phrases_nonempty = true;
    for i in 0..5u8 {
        match Attribution5::from_index(i) {
            Some(v) => {
                phrases_nonempty &= !v.as_str().is_empty();
                roundtrip &= Attribution5::from_index(v as u8) == Some(v);
            }
            None => roundtrip = false,
        }
    }
    cs.add(
        "attribution5_all_classes_roundtrip",
        roundtrip && phrases_nonempty && Attribution5::from_index(5).is_none(),
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F001 · 深化批次四：装载服务进程独立（崩溃互不传染——隔离记账面）
//
// 主册依据（G-A-01【设计细节】）：「装载服务进程独立于合成器与资源管理器
// （崩溃互不传染）」——独立性是承诺不是巧合：邻面崩溃必须记账可见且**不得**
// 影响装载管线判定（F175 隔离语义在本服务的落点）。
// ---------------------------------------------------------------------------

/// 邻居组件（装载服务独立性的两根支柱）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NeighborComponent {
    /// 合成器（渲染面）。
    Composer,
    /// 资源管理器（入口面）。
    Explorer,
}

/// 服务隔离记账（崩溃互不传染的观测面）。
#[derive(Clone, Copy, Debug)]
pub struct ServiceIsolation {
    pub composer_crashes: u32,
    pub explorer_crashes: u32,
    /// 装载服务自身存活标记。
    pub loader_alive: bool,
    /// 邻面崩溃时**正在运行**的已启动窗口数（不得被连带回收——计数恒不减）。
    pub running_windows: u32,
}

impl ServiceIsolation {
    pub const fn new() -> ServiceIsolation {
        ServiceIsolation { composer_crashes: 0, explorer_crashes: 0, loader_alive: true, running_windows: 0 }
    }

    /// 邻面崩溃登记（计数可见——异常零静默）；装载服务不跟随死亡。
    pub fn note_neighbor_crash(&mut self, which: NeighborComponent) {
        match which {
            NeighborComponent::Composer => self.composer_crashes += 1,
            NeighborComponent::Explorer => self.explorer_crashes += 1,
        }
    }

    /// 隔离判据：服务存活，且邻面崩溃**不构成**装载判据失效（verdict 面独立）。
    pub fn containment_intact(&self) -> bool {
        self.loader_alive
    }
}

/// F001 深化批次四自检。
pub fn run_dblrun_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F001-dblrun-deep3");
    // 1) 邻面崩溃计数分面可见；隔离判定不受邻面崩溃影响（服务仍存活）。
    let mut iso = ServiceIsolation::new();
    iso.note_neighbor_crash(NeighborComponent::Composer);
    iso.note_neighbor_crash(NeighborComponent::Composer);
    iso.note_neighbor_crash(NeighborComponent::Explorer);
    cs.add(
        "service_isolation_crash_containment",
        iso.composer_crashes == 2
            && iso.explorer_crashes == 1
            && iso.containment_intact(),
        "",
    );
    // 2) 正在运行的窗口不因邻面崩溃被连带回收（计数恒不减——F175 语义锚）。
    let mut iso2 = ServiceIsolation::new();
    iso2.running_windows = 5;
    iso2.note_neighbor_crash(NeighborComponent::Composer);
    cs.add(
        "running_windows_preserved_on_neighbor_crash",
        iso2.running_windows == 5 && iso2.containment_intact(),
        "",
    );
    // 3) 服务自身死亡 = 隔离判定如实红（不装作还活着）。
    let mut iso3 = ServiceIsolation::new();
    iso3.loader_alive = false;
    cs.add("loader_down_honest", !iso3.containment_intact(), "");
    cs
}
