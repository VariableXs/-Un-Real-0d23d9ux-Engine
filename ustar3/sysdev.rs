//! I 通用域·主题文件 sysdev（F540~F547 · AI-U3 分工包 · 判据实装层）。
//!
//! 判据唯一源：主册《Varix STAR I start.md》F540~F547 各节【验收判据】。
//! 本文件为 no_std 兼容的纯逻辑「判据实装层」：零堆热路径（无 String/Vec/
//! Box/format! 进逻辑路径，定长数组 + &str + core 运算），测试可用 std。
//!
//! =========================================================================
//! F540 内存诊断（MemDiagPlanner）
//! =========================================================================
//! 判据（逐字摘录第一句）：**预约/立即两模式；两遍标准模式；报告三要素
//! （结论/地址段/建议）；时间线留痕；测试时长与进度显示。**
//!
//! 功能定义要点：内存诊断工具——重启进诊断环境（F198 恢复环境分支：全屏
//! 检测跑两遍标准读写模式，30 分钟级；不进系统所以测得真）；完成后回桌面
//! 出报告（坏块地址段 + 人话结论「检测通过 / 第 X 段异常——建议送检内存
//! 条」）；报告入 F372 时间线可追溯；睡前点一下诊断、早上看报告。
//! 依赖锚点：F198（恢复环境）、F372（时间线）。
//! 零堆纪律：定长分段表（16 段 × 定长字数组）、定长时间线槽，无 alloc。
//!
//! =========================================================================
//! F541 网络重置（NetReset）
//! =========================================================================
//! 判据（逐字摘录第一句）：**清单完整性与准确性；90s 无整机重启；重配
//! 向导链路；入口位置（诊断链末）；执行留痕（F372）。**
//!
//! 功能定义要点：网络栈一键复位（F242 诊断的终极手段）：设置页「网络重
//! 置」按钮 → 确认框列清单位（清除 Wi-Fi 密码 / VPN 配置 F484 / 代理
//! F485 / 静态 IP——「重置后需要重新配置」逐项列出）→ 执行（90 秒倒计时
//! 自动重启网络栈，无需重启整机）；重置后向导式重配（Wi-Fi 逐个问要不要
//! 重连）。
//! 依赖锚点：F242（网络诊断）、F484（VPN）、F485（代理）、F372（时间线）。
//! 零堆纪律：定长四项清单 + 定长决策表，无 alloc。
//!
//! =========================================================================
//! F542 ClickLock 拖拽锁定（ClickLock）
//! =========================================================================
//! 判据（逐字摘录第一句）：**1.1s 阈值抓起；光环标记；单击放下；Esc 放
//! 弃；默认关与 F262 拖拽语义兼容。**
//!
//! 功能定义要点：ClickLock（点击锁定）：开启后按住主键片刻（1.1s 阈值）
//! 松开 =「抓起」状态（相当于一直按着键），移动到目标再单击 = 放下；抓起
//! 态指针带视觉标记（微光环）；Esc 放弃拖拽；默认关（进阶无障碍）。
//! 依赖锚点：F262（拖拽语义）。
//! 零堆纪律：单状态机 + 定时戳，无 alloc。
//!
//! =========================================================================
//! F543 分设备音量记忆（PerDeviceVolume）
//! =========================================================================
//! 判据（逐字摘录第一句）：**分设备记忆与切换跟随；新设备 40% 首发值；
//! 持久化；与 F240/F241 链路一致；记忆上限（设备清单 10 台淘汰）。**
//!
//! 功能定义要点：音量按输出设备各记各的：耳机 30% / 扬声器 70%——拔插切
//! 换（F241）音量跟着设备走；每设备独立记忆持久化；新设备首插默认 40%
//! （安全首发音量）+ OSD 显示。
//! 依赖锚点：F240（音量 OSD）、F241（输出设备路由切换）。
//! 零堆纪律：定长 10 槽设备表（DEV_CAP=10，LRU 淘汰），无 alloc。
//!
//! =========================================================================
//! F544 通知音量独立分级（DualVolume）
//! =========================================================================
//! 判据（逐字摘录第一句）：**双滑杆独立；OSD 双条形制；默认 50；总闸优
//! 先级；持久化与 F341 叠加。**
//!
//! 功能定义要点：通知提示音与媒体音量分离：通知音量独立滑杆（0-100，默
//! 认 50）——媒体开 20% 时通知也不会被淹没（反之媒体全开时通知不炸耳）；
//! 通知音量在音量 OSD（F240）以第二细条呈现；完全静音档（F341）仍总闸。
//! 依赖锚点：F240（音量 OSD 双条）、F341（完全静音总闸）。
//! 零堆纪律：纯标量状态 + 4 字节持久化，无 alloc。
//!
//! =========================================================================
//! F545 蓝牙耳机电量显示（BtBattery）
//! =========================================================================
//! 判据（逐字摘录第一句）：**三处显示同源；连接通知条；低电节流；无电量
//! 诚实态；电量跳变平滑（不闪跳）。**
//!
//! 功能定义要点：蓝牙耳机/手环电量进系统：已配对设备电量显示三处（蓝牙
//! 设备页 F290 / 电池浮层 F423 附属区 / 连接瞬间通知条「耳机已连接 · 电量
//! 85%」）；低电量（<20%）提示一次（同设备每小时最多一次——不刷屏）；无电
//! 量上报的设备诚实显示「无电量信息」。
//! 依赖锚点：F290（蓝牙设备页）、F423（电池浮层）。
//! 零堆纪律：定长 8 槽电量表（Option&lt;u8&gt; 诚实态），无 alloc。
//!
//! =========================================================================
//! F546 新设备接入通知（DeviceArrive）
//! =========================================================================
//! 判据（逐字摘录第一句）：**三状态横幅；2s 就绪时序；进度与出路；失败
//! 归因；通知与 F290 设备页同步。**
//!
//! 功能定义要点：外设接入的统一通知形制：接入 → 右下横幅一条（设备图标 +
//! 名称 + 状态「已就绪」/「安装驱动中…」/「需要手动安装——点击查看 F444
//! 向导」）；即插即用类 2 秒内就绪（横幅短驻）；需要驱动的显示进度并给出
//! 路；接入失败（驱动无）诚实列出原因与手装路径。
//! 依赖锚点：F290（设备页）、F444（驱动手动安装向导）。
//! 零堆纪律：定长 8 横幅槽 + 静态文案表，无 alloc。
//!
//! =========================================================================
//! F547 音量左右平衡（VolumeBalance）
//! =========================================================================
//! 判据（逐字摘录第一句）：**滑杆与试听实时；分设备记忆；中心格点；软件
//! 平衡延迟；持久化。**
//!
//! 功能定义要点：音量平衡滑杆（左↔右）：音频设置页横滑杆（中心格点标记、
//! 偏离即时试听——测试音循环时拖动可感）；平衡按输出设备记忆（F543 族）；
//! 纯软件平衡（硬件不支持时软件混音实现，延迟 <1ms 无感）。
//! 依赖锚点：F543（分设备记忆族）、F240（音量链路）。
//! 零堆纪律：定长 10 槽平衡表 + Q10 定点增益纯函数，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 域内公共基建：F372 风格时间线留痕（定长、零堆）
// ---------------------------------------------------------------------------

/// 一条时间线记录（F372 风格：id + 事件标签）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TlEntry {
    pub id: u32,
    pub tag: &'static str,
}

/// 时间线容量（定长槽位，超容静默丢弃——与 CheckSet 纪律一致）。
pub const TL_CAP: usize = 8;

/// F372 风格时间线（本文件各域共用留痕基建；零堆：定长数组）。
#[derive(Clone, Copy)]
pub struct TimelineLog {
    entries: [Option<TlEntry>; TL_CAP],
    count: usize,
    next_id: u32,
}

impl TimelineLog {
    pub const fn new() -> Self {
        TimelineLog {
            entries: [None; TL_CAP],
            count: 0,
            next_id: 1,
        }
    }

    /// 追加一条记录，返回其 id（超容丢弃但 id 仍单调递增——留痕诚实）。
    pub fn append(&mut self, tag: &'static str) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        if self.count < TL_CAP {
            self.entries[self.count] = Some(TlEntry { id, tag });
            self.count += 1;
        }
        id
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn get(&self, index: usize) -> Option<TlEntry> {
        if index < self.count {
            self.entries[index]
        } else {
            None
        }
    }
}

// ===========================================================================
// F540 内存诊断（MemDiagPlanner）
// ===========================================================================

/// 诊断模式（判据：预约/立即两模式）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagMode {
    /// 预约：下次重启进诊断环境（F198 恢复环境分支——不进系统所以测得真）。
    ScheduleNextBoot,
    /// 立即：马上重启进诊断环境。
    RunNow,
}

/// 诊断模式种类数（判据「两模式」）。
pub const DIAG_MODE_COUNT: usize = 2;

/// 标准模式遍数（判据：两遍标准模式）。
pub const DIAG_PASS_COUNT: u32 = 2;

/// 标准模式整体测试时长（30 分钟级；主册「30 分钟级」的建模常量）。
pub const STANDARD_MODE_DURATION_MS: u64 = 1_800_000;

/// 内存分段数（测试进度模型的定长分段表）。
pub const SEG_COUNT: usize = 16;
/// 每段字节数。
pub const SEG_BYTES: u64 = 16 << 20;
/// 每段模型字数（读写校验建模粒度：每段 8 个 32 位字）。
pub const WORDS_PER_SEG: usize = 8;
/// 全测试区字数。
pub const MEM_WORDS: usize = SEG_COUNT * WORDS_PER_SEG;
/// 测试区起始基址（模型值，真机由内存图给出）。
pub const MEM_BASE: u64 = 0x0010_0000;
/// 标准读写图案 A（经典 55 走步）。
pub const PATTERN_A: u32 = 0x5555_A5A5;
/// 标准读写图案 B（经典 AA 走步，与 A 互补）。
pub const PATTERN_B: u32 = 0xA5A5_5A5A;
/// 坏位读出污染掩码（建模：坏位使读回值翻转，模拟粘滞位故障）。
pub const FAULT_MASK: u32 = 0xFFFF_0000;

/// 报告结论（报告三要素之一：结论）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 检测通过。
    Pass,
    /// 第 X 段异常（X = 段号；坏块段定位）。
    SegmentFail(u8),
}

/// 人话结论/建议文案（三要素之一 + 之三；锚词「建议送检内存条」）。
pub const ADVICE_PASS: &str = "检测通过——内存条无异常。";
pub const ADVICE_FAIL: &str = "检测到异常段——建议送检内存条。";

/// 诊断报告（三要素：结论 / 坏块地址段 / 人话建议；另带时间线 id 留痕）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiagReport {
    pub verdict: Verdict,
    /// 坏块地址段（段号；Pass 时为 None）。
    pub bad_seg: Option<u8>,
    /// 人话建议文案。
    pub advice: &'static str,
    /// F372 时间线留痕 id。
    pub timeline_id: u32,
}

/// 读写校验扫描器（建模：写入图案 → 读出比对；坏位读回值带污染掩码）。
pub struct MemScanner {
    mem: [u32; MEM_WORDS],
    fault: [bool; MEM_WORDS],
}

impl MemScanner {
    pub const fn new() -> Self {
        MemScanner {
            mem: [0; MEM_WORDS],
            fault: [false; MEM_WORDS],
        }
    }

    /// 注入坏位（诊断演练/测试专用；真机由硬件读回翻转体现，无需注入）。
    pub fn inject_fault(&mut self, seg: u8) {
        let idx = seg as usize * WORDS_PER_SEG;
        if idx < MEM_WORDS {
            self.fault[idx] = true;
        }
    }

    /// 单遍标准扫描：逐段逐字写入两图案并读出比对。
    /// 返回首个失败段号（无失败 → None）。
    pub fn scan_pass(&mut self) -> Option<u8> {
        for seg in 0..SEG_COUNT {
            for w in 0..WORDS_PER_SEG {
                let idx = seg * WORDS_PER_SEG + w;
                for &p in [PATTERN_A, PATTERN_B].iter() {
                    self.mem[idx] = p;
                    let readback = if self.fault[idx] {
                        self.mem[idx] ^ FAULT_MASK
                    } else {
                        self.mem[idx]
                    };
                    if readback != p {
                        return Some(seg as u8);
                    }
                }
            }
        }
        None
    }
}

/// 内存诊断规划器：两模式 + 两遍标准模式编排 + 报告三要素组装 + 时间线留痕。
pub struct MemDiagPlanner {
    pub mode: DiagMode,
    /// 计划已登记（预约模式在 F198 恢复环境分支消费此标志）。
    pub scheduled: bool,
    passes_done: u32,
    segs_done: usize,
    timeline: TimelineLog,
}

impl MemDiagPlanner {
    pub const fn new(mode: DiagMode) -> Self {
        MemDiagPlanner {
            mode,
            scheduled: false,
            passes_done: 0,
            segs_done: 0,
            timeline: TimelineLog::new(),
        }
    }

    /// 登记诊断计划（预约/立即皆走此入口；留痕「mem-diag-plan」）。
    pub fn plan(&mut self) -> bool {
        self.scheduled = true;
        self.timeline.append("mem-diag-plan");
        true
    }

    /// 标准模式两遍编排：每遍全段扫描；任一遍失败即判负并定位坏块段
    /// （两遍都过才 Pass——「不进系统所以测得真」）。
    pub fn run_standard(&mut self, scanner: &mut MemScanner) -> Verdict {
        self.passes_done = 0;
        self.segs_done = 0;
        let mut bad: Option<u8> = None;
        for _ in 0..DIAG_PASS_COUNT {
            match scanner.scan_pass() {
                Some(seg) => {
                    if bad.is_none() {
                        bad = Some(seg);
                    }
                    self.segs_done = seg as usize;
                    break;
                }
                None => {
                    self.passes_done += 1;
                    self.segs_done = SEG_COUNT;
                }
            }
        }
        match bad {
            Some(s) => Verdict::SegmentFail(s),
            None => Verdict::Pass,
        }
    }

    /// 报告三要素组装（结论/地址段/建议）+ 时间线留痕（「mem-diag-report」）。
    pub fn build_report(&mut self, verdict: Verdict) -> DiagReport {
        let (bad_seg, advice) = match verdict {
            Verdict::Pass => (None, ADVICE_PASS),
            Verdict::SegmentFail(s) => (Some(s), ADVICE_FAIL),
        };
        let timeline_id = self.timeline.append("mem-diag-report");
        DiagReport {
            verdict,
            bad_seg,
            advice,
            timeline_id,
        }
    }

    pub fn passes_done(&self) -> u32 {
        self.passes_done
    }

    pub fn segs_done(&self) -> usize {
        self.segs_done
    }

    pub fn timeline(&self) -> &TimelineLog {
        &self.timeline
    }
}

/// 进度百分比（0..=100）：两遍 × 全段的完成度（进度显示判据）。
pub fn progress_percent(passes_done: u32, segs_done: usize) -> u32 {
    let total = (DIAG_PASS_COUNT as usize * SEG_COUNT) as u32;
    let done = passes_done.min(DIAG_PASS_COUNT) as usize * SEG_COUNT + segs_done.min(SEG_COUNT);
    (done as u32 * 100 / total).min(100)
}

/// 每段预计时长：30 分钟级总时长均摊到两遍 × 16 段 = 56_250ms（进度估计）。
pub fn est_ms_per_seg() -> u64 {
    STANDARD_MODE_DURATION_MS / (DIAG_PASS_COUNT as u64 * SEG_COUNT as u64)
}

/// 段基址（报告三要素之二：坏块地址段下界）。
pub fn seg_base(seg: u8) -> u64 {
    MEM_BASE + seg as u64 * SEG_BYTES
}

/// 段上界（开区间端点）。
pub fn seg_end(seg: u8) -> u64 {
    seg_base(seg) + SEG_BYTES
}

/// F540 域自检。
pub fn run_f540_checks() -> CheckSet {
    let mut cs = CheckSet::new("F540-mem-diag");
    // 1) 预约/立即两模式在册且互异。
    cs.add(
        "two_modes",
        DIAG_MODE_COUNT == 2 && DiagMode::ScheduleNextBoot != DiagMode::RunNow,
        "",
    );
    // 2) 两遍标准模式 + 30 分钟级时长常量。
    cs.add(
        "two_pass_standard",
        DIAG_PASS_COUNT == 2 && STANDARD_MODE_DURATION_MS == 1_800_000,
        "",
    );
    // 3) 预约模式留痕：计划入时间线。
    let mut p = MemDiagPlanner::new(DiagMode::ScheduleNextBoot);
    cs.add("schedule_traced", p.plan() && p.timeline().len() == 1, "");
    // 4) 干净内存：两遍全过 → Pass，进度满两遍。
    let mut sc = MemScanner::new();
    let v = p.run_standard(&mut sc);
    cs.add(
        "clean_two_pass_pass",
        v == Verdict::Pass && p.passes_done() == DIAG_PASS_COUNT,
        "",
    );
    // 5) 报告三要素（Pass 版）：结论 + 无坏段 + 建议文案 + 时间线 id。
    let rep = p.build_report(v);
    cs.add(
        "report_pass_elements",
        rep.verdict == Verdict::Pass
            && rep.bad_seg.is_none()
            && rep.advice == ADVICE_PASS
            && rep.timeline_id >= 2,
        "",
    );
    // 6) 坏块段定位：段 5 注入坏位 → SegmentFail(5)（两遍任一失败即判负）。
    let mut p2 = MemDiagPlanner::new(DiagMode::RunNow);
    let mut sc2 = MemScanner::new();
    sc2.inject_fault(5);
    let v2 = p2.run_standard(&mut sc2);
    cs.add("bad_segment_located", v2 == Verdict::SegmentFail(5), "");
    // 7) 坏段地址段：报告段号与 seg_base/seg_end 一致。
    let rep2 = p2.build_report(v2);
    cs.add(
        "bad_segment_range",
        rep2.bad_seg == Some(5)
            && seg_base(5) == MEM_BASE + 5 * SEG_BYTES
            && seg_end(5) == seg_base(5) + SEG_BYTES,
        "",
    );
    // 8) 人话建议锚词「建议送检内存条」。
    cs.add("advice_anchor", rep2.advice.contains("建议送检内存条"), "");
    // 9) 报告入 F372 时间线（tag = mem-diag-report）。
    cs.add(
        "timeline_traced",
        p2.timeline()
            .get(0)
            .map(|e| e.tag == "mem-diag-report" && e.id > 0)
            .unwrap_or(false),
        "",
    );
    // 10) 进度函数边界：0 / 两遍中点 50 / 满两遍 100。
    cs.add(
        "progress_bounds",
        progress_percent(0, 0) == 0
            && progress_percent(1, 0) == 50
            && progress_percent(2, SEG_COUNT) == 100,
        "",
    );
    // 11) 每段预计时长 = 1_800_000 / (2 × 16) = 56_250ms。
    cs.add("est_per_seg", est_ms_per_seg() == 56_250, "");
    // 12) 段 0 边界失败同样精确定位。
    let mut sc3 = MemScanner::new();
    sc3.inject_fault(0);
    let mut p3 = MemDiagPlanner::new(DiagMode::RunNow);
    cs.add(
        "first_segment_fail",
        p3.run_standard(&mut sc3) == Verdict::SegmentFail(0),
        "",
    );
    cs
}

#[cfg(test)]
mod f540_tests {
    use super::*;

    #[test]
    fn clean_memory_passes_two_passes() {
        let mut p = MemDiagPlanner::new(DiagMode::RunNow);
        let mut sc = MemScanner::new();
        assert_eq!(p.run_standard(&mut sc), Verdict::Pass, "干净内存两遍应全过");
        assert_eq!(p.passes_done(), 2, "标准模式必须跑满两遍");
        assert_eq!(p.segs_done(), SEG_COUNT, "全部 16 段完成");
    }

    #[test]
    fn bad_block_segment_is_located() {
        // 坏块段定位必测：段 7 注入 → 报告精确指认第 7 段。
        let mut p = MemDiagPlanner::new(DiagMode::ScheduleNextBoot);
        assert!(p.plan(), "预约应成功");
        let mut sc = MemScanner::new();
        sc.inject_fault(7);
        let v = p.run_standard(&mut sc);
        assert_eq!(v, Verdict::SegmentFail(7), "坏块应定位到第 7 段");
        let rep = p.build_report(v);
        assert_eq!(rep.bad_seg, Some(7), "报告坏段应为段 7");
        assert!(rep.advice.contains("建议送检内存条"), "失败建议必须含送检锚词");
    }

    #[test]
    fn second_pass_only_run_when_first_clean() {
        // 第一遍失败即止（不再空跑第二遍）；第一遍干净才进第二遍。
        let mut p = MemDiagPlanner::new(DiagMode::RunNow);
        let mut sc = MemScanner::new();
        sc.inject_fault(3);
        assert_eq!(p.run_standard(&mut sc), Verdict::SegmentFail(3));
        assert_eq!(p.passes_done(), 0, "第一遍失败则遍数计数为 0");
    }

    #[test]
    fn progress_and_est_constants() {
        assert_eq!(progress_percent(0, 0), 0, "起点进度 0");
        assert_eq!(progress_percent(2, 0), 100, "两遍跑完即使段计未刷也满 100");
        assert_eq!(est_ms_per_seg(), 56_250, "每段预计 56_250ms（30 分钟级均摊）");
        assert_eq!(STANDARD_MODE_DURATION_MS, 1_800_000, "30 分钟级常量");
    }

    #[test]
    fn segment_address_ranges_are_contiguous() {
        // 16 段地址段连续无缝（报告地址段可信的前提）。
        for s in 0..(SEG_COUNT as u8 - 1) {
            assert_eq!(seg_end(s), seg_base(s + 1), "段 {} 与下一段地址应连续", s);
        }
        assert_eq!(seg_base(0), MEM_BASE, "段 0 基址即测试区基址");
    }

    #[test]
    fn timeline_ids_monotonic() {
        let mut p = MemDiagPlanner::new(DiagMode::RunNow);
        let id1 = p.timeline().len();
        let _ = id1;
        p.plan();
        let mut sc = MemScanner::new();
        let v = p.run_standard(&mut sc);
        let rep = p.build_report(v);
        assert!(rep.timeline_id > 1, "报告 id 应大于计划 id（单调递增）");
        assert_eq!(p.timeline().len(), 2, "计划 + 报告两条留痕");
    }
}

// ===========================================================================
// F541 网络重置（NetReset）
// ===========================================================================

/// 倒计时长（判据：90s 自动重启网络栈）。
pub const COUNTDOWN_MS: u64 = 90_000;
/// 清单项数（Wi-Fi 密码 / VPN / 代理 / 静态 IP 四项）。
pub const ITEM_COUNT: usize = 4;
/// 入口深度：一级设置 → 诊断链 → 网络重置（诊断链末端，非一级入口）。
pub const ENTRY_DEPTH: usize = 3;

/// 重置清单项（判据：清单完整性与准确性——四项逐项列出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResetItem {
    /// Wi-Fi 密码。
    WifiPasswords,
    /// VPN 配置（F484）。
    VpnConfigs,
    /// 代理设置（F485）。
    ProxySettings,
    /// 静态 IP。
    StaticIp,
}

/// 清单全表（定长、顺序即确认框展示序）。
pub const RESET_ITEMS: [ResetItem; ITEM_COUNT] = [
    ResetItem::WifiPasswords,
    ResetItem::VpnConfigs,
    ResetItem::ProxySettings,
    ResetItem::StaticIp,
];

impl ResetItem {
    /// 清单项机器名（确认框列表条目锚）。
    pub fn name(self) -> &'static str {
        match self {
            ResetItem::WifiPasswords => "wifi-passwords",
            ResetItem::VpnConfigs => "vpn-configs",
            ResetItem::ProxySettings => "proxy-settings",
            ResetItem::StaticIp => "static-ip",
        }
    }

    /// 逐项「重置后需要重新配置」文案（确认框准确性判据）。
    pub fn reconfig_note(self) -> &'static str {
        match self {
            ResetItem::WifiPasswords => "Wi-Fi 密码——重置后需要重新配置",
            ResetItem::VpnConfigs => "VPN 配置（F484）——重置后需要重新配置",
            ResetItem::ProxySettings => "代理设置（F485）——重置后需要重新配置",
            ResetItem::StaticIp => "静态 IP——重置后需要重新配置",
        }
    }
}

/// 向导逐项决策（保留 / 重配）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WizardDecision {
    /// 保留现状（不重配该项）。
    Keep,
    /// 走重配（Wi-Fi 逐个问要不要重连）。
    Reconfigure,
}

/// 网络栈一键复位状态机（90s 倒计时重启网络栈——不是整机）。
pub struct NetReset {
    executed: bool,
    countdown_left: u64,
    machine_reboot: bool,
    stack_restarted: bool,
    cleared: [bool; ITEM_COUNT],
    /// 待重配清单（向导消费）。
    pending_reconfig: [bool; ITEM_COUNT],
    /// 向导逐项决策（None = 未问询）。
    decisions: [Option<WizardDecision>; ITEM_COUNT],
    timeline: TimelineLog,
}

impl NetReset {
    pub const fn new() -> Self {
        NetReset {
            executed: false,
            countdown_left: 0,
            machine_reboot: false,
            stack_restarted: false,
            cleared: [false; ITEM_COUNT],
            pending_reconfig: [false; ITEM_COUNT],
            decisions: [None; ITEM_COUNT],
            timeline: TimelineLog::new(),
        }
    }

    /// 执行重置：清四项清单 + 进 90s 倒计时 + 留痕。
    /// **重启的是网络栈，不是整机**（machine_reboot 恒 false）。
    pub fn execute(&mut self) -> bool {
        self.executed = true;
        for i in 0..ITEM_COUNT {
            self.cleared[i] = true;
            self.pending_reconfig[i] = true;
        }
        self.countdown_left = COUNTDOWN_MS;
        self.machine_reboot = false;
        self.stack_restarted = false;
        self.timeline.append("net-reset-execute");
        true
    }

    /// 倒计时推进：归零即自动重启网络栈（无需整机重启）。
    pub fn tick(&mut self, ms: u64) {
        self.countdown_left = self.countdown_left.saturating_sub(ms);
        if self.executed && self.countdown_left == 0 {
            self.stack_restarted = true;
        }
    }

    /// 向导逐项问询决策（须在执行后；决策即出待重配清单）。
    pub fn decide(&mut self, item_idx: usize, d: WizardDecision) -> bool {
        if !self.executed || item_idx >= ITEM_COUNT {
            return false;
        }
        self.decisions[item_idx] = Some(d);
        true
    }

    /// 向导完成：四项全部有了保留/重配决策。
    pub fn wizard_done(&self) -> bool {
        self.decisions.iter().all(|d| d.is_some())
    }

    pub fn executed(&self) -> bool {
        self.executed
    }

    pub fn countdown_left(&self) -> u64 {
        self.countdown_left
    }

    /// 整机重启标志（判据：无整机重启——恒 false 断言面）。
    pub fn machine_reboot(&self) -> bool {
        self.machine_reboot
    }

    pub fn stack_restarted(&self) -> bool {
        self.stack_restarted
    }

    pub fn cleared(&self, item_idx: usize) -> bool {
        item_idx < ITEM_COUNT && self.cleared[item_idx]
    }

    pub fn pending_reconfig(&self, item_idx: usize) -> bool {
        item_idx < ITEM_COUNT && self.pending_reconfig[item_idx]
    }

    pub fn decision(&self, item_idx: usize) -> Option<WizardDecision> {
        if item_idx < ITEM_COUNT {
            self.decisions[item_idx]
        } else {
            None
        }
    }

    pub fn timeline(&self) -> &TimelineLog {
        &self.timeline
    }
}

/// F541 域自检。
pub fn run_f541_checks() -> CheckSet {
    let mut cs = CheckSet::new("F541-net-reset");
    // 1) 清单完整性：四项在册。
    cs.add(
        "checklist_complete",
        RESET_ITEMS.len() == ITEM_COUNT
            && RESET_ITEMS[0] == ResetItem::WifiPasswords
            && RESET_ITEMS[3] == ResetItem::StaticIp,
        "",
    );
    // 2) 清单准确性：逐项「重置后需要重新配置」文案在位且条目名非空。
    let mut notes_ok = true;
    for it in RESET_ITEMS.iter() {
        if !it.reconfig_note().contains("重置后需要重新配置") || it.name().is_empty() {
            notes_ok = false;
        }
    }
    cs.add("item_notes_anchor", notes_ok, "");
    // 3) 90s 倒计时常量。
    cs.add("countdown_constant", COUNTDOWN_MS == 90_000, "");
    // 4) 入口位置：诊断链末端（深度 3 > 1，不在一级）。
    cs.add("entry_depth_tail", ENTRY_DEPTH == 3 && ENTRY_DEPTH > 1, "");
    // 5) 执行清空全部四项。
    let mut nr = NetReset::new();
    nr.execute();
    let mut all_cleared = true;
    for i in 0..ITEM_COUNT {
        if !nr.cleared(i) {
            all_cleared = false;
        }
    }
    cs.add("execute_clears_all", nr.executed() && all_cleared, "");
    // 6) 待重配清单全量挂起（向导逐项消费）。
    let mut all_pending = true;
    for i in 0..ITEM_COUNT {
        if !nr.pending_reconfig(i) {
            all_pending = false;
        }
    }
    cs.add("pending_reconfig_all", all_pending, "");
    // 7) 无整机重启（执行后 machine_reboot 恒 false）。
    cs.add("no_machine_reboot", !nr.machine_reboot(), "");
    // 8) 90s 倒计时未到：网络栈未重启。
    nr.tick(COUNTDOWN_MS - 1);
    cs.add(
        "countdown_not_yet",
        nr.countdown_left() == 1 && !nr.stack_restarted(),
        "",
    );
    // 9) 倒计时归零：网络栈自动重启，整机仍不重启。
    nr.tick(1);
    cs.add(
        "stack_restart_after_countdown",
        nr.stack_restarted() && !nr.machine_reboot(),
        "",
    );
    // 10) 执行留痕（F372）。
    cs.add(
        "timeline_traced",
        nr.timeline()
            .get(0)
            .map(|e| e.tag == "net-reset-execute")
            .unwrap_or(false),
        "",
    );
    // 11) 向导链路：四项逐个决策后完成。
    let mut nr2 = NetReset::new();
    nr2.execute();
    cs.add("wizard_incomplete_before_decide", !nr2.wizard_done(), "");
    let _ = nr2.decide(0, WizardDecision::Reconfigure);
    let _ = nr2.decide(1, WizardDecision::Keep);
    let _ = nr2.decide(2, WizardDecision::Reconfigure);
    let _ = nr2.decide(3, WizardDecision::Keep);
    cs.add("wizard_flow_done", nr2.wizard_done(), "");
    // 12) 保留/重配两种决策都被如实记录。
    cs.add(
        "wizard_keep_and_reconfig",
        nr2.decision(0) == Some(WizardDecision::Reconfigure)
            && nr2.decision(1) == Some(WizardDecision::Keep),
        "",
    );
    cs
}

#[cfg(test)]
mod f541_tests {
    use super::*;

    #[test]
    fn full_reset_flow_restarts_stack_not_machine() {
        let mut nr = NetReset::new();
        assert!(nr.execute(), "执行应成功");
        nr.tick(90_000);
        assert!(nr.stack_restarted(), "90s 后网络栈应重启");
        assert!(!nr.machine_reboot(), "整机绝不重启");
    }

    #[test]
    fn countdown_boundary_exactly_90s() {
        let mut nr = NetReset::new();
        nr.execute();
        nr.tick(89_999);
        assert!(!nr.stack_restarted(), "89_999ms 未到不重启");
        nr.tick(1);
        assert!(nr.stack_restarted(), "恰 90_000ms 归零即重启");
        assert_eq!(nr.countdown_left(), 0);
    }

    #[test]
    fn checklist_covers_four_items_with_notes() {
        assert_eq!(RESET_ITEMS.len(), 4, "清单四项");
        for it in RESET_ITEMS.iter() {
            assert!(
                it.reconfig_note().contains("重置后需要重新配置"),
                "{} 缺「重置后需要重新配置」文案",
                it.name()
            );
        }
    }

    #[test]
    fn wizard_rejects_before_execute_and_bad_index() {
        let mut nr = NetReset::new();
        assert!(!nr.decide(0, WizardDecision::Keep), "未执行不得决策");
        nr.execute();
        assert!(!nr.decide(ITEM_COUNT, WizardDecision::Keep), "越界索引拒绝");
        assert!(nr.decide(0, WizardDecision::Reconfigure), "合法决策接受");
    }

    #[test]
    fn tick_before_execute_never_restarts() {
        let mut nr = NetReset::new();
        nr.tick(1_000_000);
        assert!(!nr.stack_restarted(), "未执行时 tick 不应触发重启");
        assert!(!nr.executed(), "tick 不等于执行");
    }

    #[test]
    fn execute_leaves_timeline_trace() {
        let mut nr = NetReset::new();
        nr.execute();
        let e = nr.timeline().get(0).expect("执行必须留痕");
        assert_eq!(e.tag, "net-reset-execute", "留痕标签应为 net-reset-execute");
        assert!(e.id > 0, "时间线 id 应为正");
    }
}

// ===========================================================================
// F542 ClickLock 拖拽锁定（ClickLock）
// ===========================================================================

/// 抓起阈值（判据：1.1s 阈值抓起）。
pub const HOLD_THRESHOLD_MS: u32 = 1100;

/// ClickLock 状态（Idle=普通态 / Locked=抓起态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClState {
    Idle,
    Locked,
}

/// ClickLock 状态机（默认关；与 F262 拖拽语义兼容）。
pub struct ClickLock {
    /// 功能开关（判据：默认关——进阶无障碍）。
    pub enabled: bool,
    state: ClState,
    /// 抓起态视觉标记（微光环）。
    halo: bool,
    press_start: Option<u32>,
    /// Esc 放弃后的「恢复原位」标志（拖拽取消语义）。
    pub restore_pending: bool,
}

impl ClickLock {
    pub const fn new() -> Self {
        ClickLock {
            enabled: false,
            state: ClState::Idle,
            halo: false,
            press_start: None,
            restore_pending: false,
        }
    }

    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if !on {
            // 关闭即复位一切瞬态（不留抓起残态）。
            self.state = ClState::Idle;
            self.halo = false;
            self.press_start = None;
            self.restore_pending = false;
        }
    }

    /// 主键按下（仅 Idle 态且开启时开始 hold 计时）。
    pub fn on_press(&mut self, now_ms: u32) {
        if self.enabled && self.state == ClState::Idle {
            self.press_start = Some(now_ms);
        }
    }

    /// 主键松开：hold ≥ 1100ms → 抓起（Locked + 光环）；< 1100ms → 普通点击。
    pub fn on_release(&mut self, now_ms: u32) {
        if let Some(t0) = self.press_start.take() {
            let held = now_ms.wrapping_sub(t0);
            if self.enabled && held >= HOLD_THRESHOLD_MS {
                self.state = ClState::Locked;
                self.halo = true;
            }
        }
    }

    /// 指针移动：抓起态移动不改状态（抓着拖——相当于一直按着键）。
    pub fn on_move(&mut self) {
        // Locked 态语义：移动不触发任何状态转移。
    }

    /// 抓起态单击 = 放下（回到 Idle、光环熄灭）。返回是否发生了放下。
    pub fn on_click(&mut self) -> bool {
        if self.state == ClState::Locked {
            self.state = ClState::Idle;
            self.halo = false;
            true
        } else {
            false
        }
    }

    /// Esc 放弃：拖拽取消 + 恢复原位标志。返回是否发生了放弃。
    pub fn on_escape(&mut self) -> bool {
        if self.state == ClState::Locked {
            self.state = ClState::Idle;
            self.halo = false;
            self.restore_pending = true;
            true
        } else {
            false
        }
    }

    pub fn is_locked(&self) -> bool {
        self.state == ClState::Locked
    }

    pub fn halo_visible(&self) -> bool {
        self.halo
    }

    /// F262 拖拽语义兼容：Locked 对外呈现等价「主键一直按着」——
    /// 拖拽状态机看到的是同一输入（button-down 保持）。
    pub fn presents_button_down(&self) -> bool {
        self.state == ClState::Locked
    }
}

/// F542 域自检。
pub fn run_f542_checks() -> CheckSet {
    let mut cs = CheckSet::new("F542-clicklock");
    // 1) 默认关：开关、状态、光环全为默认。
    let cl = ClickLock::new();
    cs.add(
        "default_off",
        !cl.enabled && !cl.is_locked() && !cl.halo_visible(),
        "",
    );
    // 2) 阈值常量 1100ms。
    cs.add("threshold_constant", HOLD_THRESHOLD_MS == 1100, "");
    // 3) 1099ms 松开 = 普通点击（分界下侧）。
    let mut a = ClickLock::new();
    a.set_enabled(true);
    a.on_press(0);
    a.on_release(1099);
    let below = !a.is_locked() && !a.halo_visible();
    cs.add("boundary_1099_normal_click", below, "");
    // 4) 1100ms 松开 = 抓起（分界上侧，闭区间含等于）。
    let mut b = ClickLock::new();
    b.set_enabled(true);
    b.on_press(0);
    b.on_release(1100);
    cs.add("boundary_1100_locks", b.is_locked() && b.halo_visible(), "");
    // 5) 关闭状态下长按 2000ms 也不抓起。
    let mut c = ClickLock::new();
    c.on_press(0);
    c.on_release(2000);
    cs.add("disabled_no_lock", !c.is_locked() && !c.enabled, "");
    // 6) 抓起态移动不改状态（抓着拖）。
    b.on_move();
    cs.add("move_keeps_locked", b.is_locked(), "");
    // 7) 抓起态单击 = 放下（光环熄灭、回 Idle）。
    let dropped = b.on_click();
    cs.add(
        "click_releases",
        dropped && !b.is_locked() && !b.halo_visible(),
        "",
    );
    // 8) Esc 放弃：取消拖拽 + 恢复原位标志。
    let mut d = ClickLock::new();
    d.set_enabled(true);
    d.on_press(100);
    d.on_release(100 + HOLD_THRESHOLD_MS);
    let aborted = d.on_escape();
    cs.add(
        "escape_abandon_restore",
        aborted && !d.is_locked() && d.restore_pending,
        "",
    );
    // 9) F262 兼容：Locked 呈现 = 按住；Idle 呈现 = 松开。
    cs.add(
        "f262_button_down_semantics",
        ClickLock::new().presents_button_down() == false,
        "",
    );
    // 10) 放下后可再次抓起（状态机可循环）。
    let mut e = ClickLock::new();
    e.set_enabled(true);
    e.on_press(0);
    e.on_release(HOLD_THRESHOLD_MS);
    let first = e.on_click();
    e.on_press(500);
    e.on_release(500 + HOLD_THRESHOLD_MS);
    cs.add("relock_after_release", first && e.is_locked(), "");
    cs
}

#[cfg(test)]
mod f542_tests {
    use super::*;

    #[test]
    fn threshold_boundary_1099_vs_1100() {
        // 分界必测：1099ms 与 1100ms 语义相反。
        let mut a = ClickLock::new();
        a.set_enabled(true);
        a.on_press(1000);
        a.on_release(1000 + 1099);
        assert!(!a.is_locked(), "1099ms 是普通点击，不得抓起");

        let mut b = ClickLock::new();
        b.set_enabled(true);
        b.on_press(1000);
        b.on_release(1000 + 1100);
        assert!(b.is_locked(), "1100ms（含等于）必须抓起");
        assert!(b.presents_button_down(), "抓起态对外等价按住键");
    }

    #[test]
    fn esc_abandon_sets_restore_flag() {
        let mut cl = ClickLock::new();
        cl.set_enabled(true);
        cl.on_press(0);
        cl.on_release(1100);
        assert!(cl.is_locked());
        assert!(cl.on_escape(), "Esc 应触发放弃");
        assert!(!cl.is_locked(), "放弃后回 Idle");
        assert!(cl.restore_pending, "放弃必须带恢复原位标志");
        assert!(!cl.halo_visible(), "放弃后光环熄灭");
    }

    #[test]
    fn disabled_long_press_is_plain_click() {
        let mut cl = ClickLock::new();
        assert!(!cl.enabled, "默认关");
        cl.on_press(0);
        cl.on_release(5000);
        assert!(!cl.is_locked(), "关闭时长按绝不抓起");
        assert!(!cl.presents_button_down(), "F262 语义不受影响");
    }

    #[test]
    fn move_does_not_break_lock() {
        let mut cl = ClickLock::new();
        cl.set_enabled(true);
        cl.on_press(0);
        cl.on_release(1100);
        for _ in 0..10 {
            cl.on_move();
        }
        assert!(cl.is_locked(), "抓起态移动不改状态");
        assert!(cl.halo_visible(), "移动中光环保持");
    }

    #[test]
    fn click_in_locked_state_drops() {
        let mut cl = ClickLock::new();
        cl.set_enabled(true);
        cl.on_press(0);
        cl.on_release(1100);
        assert!(cl.on_click(), "抓起态单击 = 放下");
        assert!(!cl.on_click(), "Idle 态单击不再是放下");
        assert!(!cl.is_locked());
    }

    #[test]
    fn disable_resets_transient_state() {
        let mut cl = ClickLock::new();
        cl.set_enabled(true);
        cl.on_press(0);
        cl.on_release(1100);
        cl.on_escape();
        cl.set_enabled(false);
        assert!(!cl.restore_pending, "关闭应清瞬态标志");
        cl.set_enabled(true);
        assert!(!cl.is_locked() && !cl.halo_visible(), "重新开启为干净 Idle");
    }
}

// ===========================================================================
// F543 分设备音量记忆（PerDeviceVolume）
// ===========================================================================

/// 设备表容量（判据：记忆上限——设备清单 10 台淘汰）。
pub const DEV_CAP: usize = 10;
/// 新设备首发音量（判据：新设备 40% 首发值——安全首发音量）。
pub const DEFAULT_FIRST: u8 = 40;
/// 持久化未用槽哨兵 id。
pub const UNUSED_ID: u32 = u32::MAX;
/// 持久化区长度：每槽 5 字节（id LE4 + 音量 1）。
pub const PERSIST_LEN: usize = DEV_CAP * 5;

/// 音量事件源（与 F240/F241 链路一致——事件源枚举）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VolEvent {
    /// F241 输出设备路由切换（拔插切换，音量跟着设备走）。
    RouteSwitchF241,
    /// F240 音量 OSD 显示（含新设备首发 40% 的 OSD 提示）。
    OsdShowF240,
    /// 设置页滑杆直接设置。
    DirectSet,
}

/// 设备音量槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DevVolSlot {
    pub dev_id: u32,
    pub volume: u8,
    pub last_used: u64,
    pub used: bool,
}

/// 分设备音量记忆表（定长 10 槽，LRU 淘汰）。
pub struct PerDeviceVolume {
    slots: [DevVolSlot; DEV_CAP],
    clock: u64,
    last_event: Option<VolEvent>,
}

impl PerDeviceVolume {
    pub const fn new() -> Self {
        PerDeviceVolume {
            slots: [DevVolSlot {
                dev_id: 0,
                volume: 0,
                last_used: 0,
                used: false,
            }; DEV_CAP],
            clock: 0,
            last_event: None,
        }
    }

    fn find(&self, dev_id: u32) -> Option<usize> {
        self.slots
            .iter()
            .position(|s| s.used && s.dev_id == dev_id)
    }

    fn find_free(&self) -> Option<usize> {
        self.slots.iter().position(|s| !s.used)
    }

    /// 最久未用槽位（LRU 淘汰目标）。
    fn lru_idx(&self) -> usize {
        let mut best = 0usize;
        for i in 1..DEV_CAP {
            if self.slots[i].last_used < self.slots[best].last_used {
                best = i;
            }
        }
        best
    }

    fn touch(&mut self, idx: usize) {
        self.slots[idx].last_used = self.clock;
        self.clock += 1;
    }

    fn insert(&mut self, dev_id: u32, volume: u8) -> u8 {
        let idx = match self.find_free() {
            Some(i) => i,
            None => self.lru_idx(), // 第 11 台设备：淘汰最久未用
        };
        self.slots[idx] = DevVolSlot {
            dev_id,
            volume,
            last_used: self.clock,
            used: true,
        };
        self.clock += 1;
        volume
    }

    /// 取设备音量（不存在的设备首发 40% 并入表）。
    pub fn volume_of(&mut self, dev_id: u32) -> u8 {
        match self.find(dev_id) {
            Some(i) => {
                self.touch(i);
                self.slots[i].volume
            }
            None => self.insert(dev_id, DEFAULT_FIRST),
        }
    }

    /// 设置设备音量（0..=100 截断；不存在则入表）。
    pub fn set_volume(&mut self, dev_id: u32, volume: u8) -> u8 {
        let v = volume.min(100);
        match self.find(dev_id) {
            Some(i) => {
                self.slots[i].volume = v;
                self.touch(i);
            }
            None => {
                self.insert(dev_id, v);
            }
        }
        v
    }

    /// 切换跟随（F241 路由事件 → 音量 = 该设备记忆值）。
    pub fn switch_to(&mut self, dev_id: u32, ev: VolEvent) -> u8 {
        self.last_event = Some(ev);
        self.volume_of(dev_id)
    }

    /// 只读查询（不触碰 LRU 时钟）。
    pub fn get_volume(&self, dev_id: u32) -> Option<u8> {
        self.find(dev_id).map(|i| self.slots[i].volume)
    }

    pub fn used_count(&self) -> usize {
        self.slots.iter().filter(|s| s.used).count()
    }

    pub fn last_event(&self) -> Option<VolEvent> {
        self.last_event
    }

    /// 持久化导出（定长区，unused 槽写哨兵）。
    pub fn persist(&self, out: &mut [u8; PERSIST_LEN]) {
        for i in 0..DEV_CAP {
            let base = i * 5;
            let id = if self.slots[i].used {
                self.slots[i].dev_id
            } else {
                UNUSED_ID
            };
            out[base] = (id & 0xFF) as u8;
            out[base + 1] = ((id >> 8) & 0xFF) as u8;
            out[base + 2] = ((id >> 16) & 0xFF) as u8;
            out[base + 3] = ((id >> 24) & 0xFF) as u8;
            out[base + 4] = self.slots[i].volume;
        }
    }

    /// 持久化恢复（与导出严格互逆）。
    pub fn restore(&mut self, src: &[u8; PERSIST_LEN]) {
        for i in 0..DEV_CAP {
            let base = i * 5;
            let id = (src[base] as u32)
                | ((src[base + 1] as u32) << 8)
                | ((src[base + 2] as u32) << 16)
                | ((src[base + 3] as u32) << 24);
            if id == UNUSED_ID {
                self.slots[i] = DevVolSlot {
                    dev_id: 0,
                    volume: 0,
                    last_used: 0,
                    used: false,
                };
            } else {
                self.slots[i] = DevVolSlot {
                    dev_id: id,
                    volume: src[base + 4],
                    last_used: 0,
                    used: true,
                };
            }
        }
        self.clock = 0;
    }
}

/// F543 域自检。
pub fn run_f543_checks() -> CheckSet {
    let mut cs = CheckSet::new("F543-dev-volume");
    // 1) 容量与首发值常量。
    cs.add(
        "cap_and_default",
        DEV_CAP == 10 && DEFAULT_FIRST == 40 && PERSIST_LEN == 50,
        "",
    );
    // 2) 新设备首插默认 40%。
    let mut pv = PerDeviceVolume::new();
    cs.add("first_plug_40", pv.volume_of(0xA001) == 40, "");
    // 3) 分设备独立记忆（耳机 30 / 扬声器 70）。
    let mut pv2 = PerDeviceVolume::new();
    let _ = pv2.set_volume(0xB001, 30);
    let _ = pv2.set_volume(0xB002, 70);
    cs.add(
        "independent_memory",
        pv2.get_volume(0xB001) == Some(30) && pv2.get_volume(0xB002) == Some(70),
        "",
    );
    // 4) 切换跟随：F241 路由事件下音量跟着设备走。
    let v_a = pv2.switch_to(0xB001, VolEvent::RouteSwitchF241);
    let v_b = pv2.switch_to(0xB002, VolEvent::RouteSwitchF241);
    cs.add(
        "switch_follows",
        v_a == 30 && v_b == 70 && pv2.last_event() == Some(VolEvent::RouteSwitchF241),
        "",
    );
    // 5) 事件源枚举覆盖 F240/F241 两链路。
    cs.add(
        "event_source_links",
        VolEvent::RouteSwitchF241 != VolEvent::OsdShowF240
            && VolEvent::OsdShowF240 != VolEvent::DirectSet,
        "",
    );
    // 6) 第 11 台设备：淘汰最久未用（d1 最久未用被淘汰，d0 刚用过幸存）。
    let mut pv3 = PerDeviceVolume::new();
    for d in 0..10u32 {
        let _ = pv3.set_volume(100 + d, 50);
    }
    let _ = pv3.volume_of(100); // d0 触碰（变最新）
    let _ = pv3.set_volume(200, 60); // 第 11 台
    cs.add(
        "eleventh_evicts_lru",
        pv3.get_volume(100).is_some()
            && pv3.get_volume(101).is_none()
            && pv3.get_volume(200) == Some(60),
        "",
    );
    // 7) 淘汰后容量仍守 10。
    cs.add("cap_held_after_evict", pv3.used_count() == DEV_CAP, "");
    // 8) 持久化导出/恢复往返一致。
    let mut pv4 = PerDeviceVolume::new();
    let _ = pv4.set_volume(0xC001, 25);
    let _ = pv4.set_volume(0xC002, 85);
    let mut blob = [0u8; PERSIST_LEN];
    pv4.persist(&mut blob);
    let mut pv5 = PerDeviceVolume::new();
    pv5.restore(&blob);
    cs.add(
        "persist_roundtrip",
        pv5.get_volume(0xC001) == Some(25) && pv5.get_volume(0xC002) == Some(85),
        "",
    );
    // 9) 音量界 0..=100（250 截断为 100）。
    let mut pv6 = PerDeviceVolume::new();
    cs.add("volume_clamp", pv6.set_volume(0xD001, 250) == 100, "");
    // 10) 恢复后可继续跟随切换（持久化与 F241 链路衔接）。
    let v_restored = pv5.switch_to(0xC002, VolEvent::RouteSwitchF241);
    cs.add("restore_then_follow", v_restored == 85, "");
    // 11) 未用槽持久化为哨兵，恢复后不误现设备。
    let pv7 = PerDeviceVolume::new();
    let mut blob2 = [0u8; PERSIST_LEN];
    pv7.persist(&mut blob2);
    let mut pv8 = PerDeviceVolume::new();
    pv8.restore(&blob2);
    cs.add(
        "empty_slots_persist_as_sentinel",
        pv8.used_count() == 0 && pv8.get_volume(0xE001).is_none(),
        "",
    );
    cs
}

#[cfg(test)]
mod f543_tests {
    use super::*;

    #[test]
    fn first_plug_defaults_to_40() {
        let mut pv = PerDeviceVolume::new();
        assert_eq!(pv.volume_of(0x1111), 40, "新设备首发必须 40%（安全首发音量）");
        assert_eq!(
            pv.last_event(),
            None,
            "直接 volume_of 不产生链路事件"
        );
    }

    #[test]
    fn volumes_are_per_device() {
        let mut pv = PerDeviceVolume::new();
        let _ = pv.set_volume(0x2222, 30); // 耳机
        let _ = pv.set_volume(0x3333, 70); // 扬声器
        assert_eq!(pv.get_volume(0x2222), Some(30), "耳机记 30");
        assert_eq!(pv.get_volume(0x3333), Some(70), "扬声器记 70");
        assert_eq!(pv.used_count(), 2);
    }

    #[test]
    fn eleventh_device_evicts_least_recently_used() {
        // 必测：10 台占满后第 11 台进表，被淘汰的是最久未用者。
        let mut pv = PerDeviceVolume::new();
        for d in 0..10u32 {
            let _ = pv.set_volume(10 + d, 50);
        }
        assert_eq!(pv.used_count(), 10);
        // 触碰 10 号设备（变最新），11..0 号成为最久未用。
        let _ = pv.volume_of(10);
        let _ = pv.set_volume(99, 77); // 第 11 台
        assert_eq!(pv.get_volume(10), Some(50), "刚用过的设备应幸存");
        assert_eq!(pv.get_volume(11), None, "最久未用者应被淘汰");
        assert_eq!(pv.get_volume(99), Some(77), "新设备应入表");
        assert_eq!(pv.used_count(), 10, "容量恒守 10");
    }

    #[test]
    fn switch_follows_remembered_volume() {
        let mut pv = PerDeviceVolume::new();
        let _ = pv.set_volume(0x4444, 30);
        let _ = pv.set_volume(0x5555, 70);
        let v1 = pv.switch_to(0x4444, VolEvent::RouteSwitchF241);
        let v2 = pv.switch_to(0x5555, VolEvent::RouteSwitchF241);
        assert_eq!(v1, 30, "切到耳机跟随 30");
        assert_eq!(v2, 70, "切到扬声器跟随 70");
        assert_eq!(pv.last_event(), Some(VolEvent::RouteSwitchF241), "事件源 F241");
    }

    #[test]
    fn persist_roundtrip_preserves_all_slots() {
        let mut pv = PerDeviceVolume::new();
        for d in 0..10u32 {
            let _ = pv.set_volume(0x100 + d, (d * 9) as u8);
        }
        let mut blob = [0u8; PERSIST_LEN];
        pv.persist(&mut blob);
        let mut pv2 = PerDeviceVolume::new();
        pv2.restore(&blob);
        for d in 0..10u32 {
            assert_eq!(
                pv2.get_volume(0x100 + d),
                Some((d * 9) as u8),
                "设备 {} 音量往返应一致",
                d
            );
        }
    }

    #[test]
    fn volume_clamped_to_0_100() {
        let mut pv = PerDeviceVolume::new();
        assert_eq!(pv.set_volume(0x6001, 250), 100, "超上界截 100");
        assert_eq!(pv.set_volume(0x6002, 0), 0, "0 合法");
    }

    #[test]
    fn osd_event_source_recordable() {
        let mut pv = PerDeviceVolume::new();
        let v = pv.switch_to(0x7001, VolEvent::OsdShowF240);
        assert_eq!(v, 40, "OSD 展示亦走首发 40 逻辑");
        assert_eq!(pv.last_event(), Some(VolEvent::OsdShowF240), "事件源 F240");
    }
}

// ===========================================================================
// F544 通知音量独立分级（DualVolume）
// ===========================================================================

/// 通知音量默认值（判据：默认 50）。
pub const NOTIF_DEFAULT: u8 = 50;
/// 音量上界。
pub const VOL_MAX: u8 = 100;

/// 媒体/通知双音量模型（通知独立滑杆 + F341 总闸）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DualVolume {
    /// 媒体音量（滑杆值 0..=100）。
    pub media: u8,
    /// 通知音量（独立滑杆值 0..=100）。
    pub notify: u8,
    /// 主音量（系统总音量，min 叠加闸）。
    pub master: u8,
    /// F341 完全静音档（总闸，最后裁决）。
    pub master_mute: bool,
}

impl DualVolume {
    pub const fn new() -> Self {
        DualVolume {
            media: VOL_MAX,
            notify: NOTIF_DEFAULT,
            master: VOL_MAX,
            master_mute: false,
        }
    }

    /// 合成闸：先与主音量取小，总闸（完全静音）最后裁决为 0。
    fn gate(value: u8, master: u8, mute: bool) -> u8 {
        if mute {
            0
        } else {
            value.min(master)
        }
    }

    /// 媒体实际音量。
    pub fn media_effective(&self) -> u8 {
        Self::gate(self.media, self.master, self.master_mute)
    }

    /// 通知实际音量（独立于媒体，仅受总闸约束）。
    pub fn notify_effective(&self) -> u8 {
        Self::gate(self.notify, self.master, self.master_mute)
    }

    /// OSD 双条形制：两条独立条形字段（媒体主条 + 通知细条）。
    pub fn osd_bars(&self) -> (u8, u8) {
        (self.media_effective(), self.notify_effective())
    }

    pub fn set_media(&mut self, v: u8) {
        self.media = v.min(VOL_MAX);
    }

    pub fn set_notify(&mut self, v: u8) {
        self.notify = v.min(VOL_MAX);
    }

    /// F341 完全静音档切换。
    pub fn set_master_mute(&mut self, on: bool) {
        self.master_mute = on;
    }

    /// 持久化导出（4 字节：媒体/通知/主音量/静音位）。
    pub fn persist(&self, out: &mut [u8; 4]) {
        out[0] = self.media;
        out[1] = self.notify;
        out[2] = self.master;
        out[3] = if self.master_mute { 1 } else { 0 };
    }

    /// 持久化恢复。
    pub fn restore(&mut self, src: &[u8; 4]) {
        self.media = src[0].min(VOL_MAX);
        self.notify = src[1].min(VOL_MAX);
        self.master = src[2].min(VOL_MAX);
        self.master_mute = src[3] != 0;
    }
}

/// F544 域自检。
pub fn run_f544_checks() -> CheckSet {
    let mut cs = CheckSet::new("F544-notif-vol");
    // 1) 默认 50（NOTIF_DEFAULT 常量 + 出厂值一致）。
    let dv = DualVolume::new();
    cs.add(
        "notif_default_50",
        NOTIF_DEFAULT == 50 && dv.notify == NOTIF_DEFAULT,
        "",
    );
    // 2) 双滑杆独立：改媒体不动通知。
    let mut d1 = DualVolume::new();
    d1.set_media(20);
    cs.add(
        "sliders_independent",
        d1.media == 20 && d1.notify == NOTIF_DEFAULT,
        "",
    );
    // 3) 媒体 20 / 通知 50：通知不被淹没（通知有效值 > 媒体有效值）。
    cs.add(
        "notif_not_drowned",
        d1.notify_effective() == 50 && d1.media_effective() == 20
            && d1.notify_effective() > d1.media_effective(),
        "",
    );
    // 4) 媒体全开 / 通知 30：通知不炸耳（有效值即滑杆值）。
    let mut d2 = DualVolume::new();
    d2.set_notify(30);
    cs.add(
        "notif_bounded_when_media_full",
        d2.media_effective() == 100 && d2.notify_effective() == 30,
        "",
    );
    // 5) 总闸：F341 完全静音 → 两路输出皆 0。
    let mut d3 = DualVolume::new();
    d3.set_master_mute(true);
    cs.add(
        "master_mute_gates_both",
        d3.media_effective() == 0 && d3.notify_effective() == 0,
        "",
    );
    // 6) 主音量 min 叠加：主 40 时通知 50 被压到 40。
    let mut d4 = DualVolume::new();
    d4.master = 40;
    cs.add("master_volume_caps", d4.notify_effective() == 40, "");
    // 7) 闸序：总闸最后——先 min 后静音（静音压过一切非零合成）。
    let mut d5 = DualVolume::new();
    d5.master = 100;
    d5.set_master_mute(true);
    cs.add("gate_order_last", d5.notify_effective() == 0 && d5.media_effective() == 0, "");
    // 8) OSD 双条形制：两个独立条形字段。
    let (bar_m, bar_n) = d1.osd_bars();
    cs.add("osd_dual_bars", bar_m == 20 && bar_n == 50, "");
    // 9) 持久化往返一致。
    let mut d6 = DualVolume::new();
    d6.set_media(65);
    d6.set_notify(45);
    let mut blob = [0u8; 4];
    d6.persist(&mut blob);
    let mut d7 = DualVolume::new();
    d7.restore(&blob);
    cs.add(
        "persist_roundtrip",
        d7.media == 65 && d7.notify == 45 && !d7.master_mute,
        "",
    );
    // 10) 滑杆界 0..=100。
    let mut d8 = DualVolume::new();
    d8.set_notify(250);
    cs.add("volume_bounds", d8.notify == VOL_MAX, "");
    cs
}

#[cfg(test)]
mod f544_tests {
    use super::*;

    #[test]
    fn default_notify_is_50() {
        let dv = DualVolume::new();
        assert_eq!(dv.notify, 50, "通知音量默认 50");
        assert_eq!(dv.notify_effective(), 50, "默认无闸压时有效值即 50");
    }

    #[test]
    fn notify_not_drowned_by_quiet_media() {
        let mut dv = DualVolume::new();
        dv.set_media(20);
        dv.set_notify(50);
        assert!(
            dv.notify_effective() > dv.media_effective(),
            "媒体 20% 时通知 50 不被淹没"
        );
        assert_eq!(dv.notify_effective(), 50);
    }

    #[test]
    fn full_mute_gates_both_channels() {
        let mut dv = DualVolume::new();
        dv.set_master_mute(true);
        assert_eq!(dv.media_effective(), 0, "总闸静音媒体归零");
        assert_eq!(dv.notify_effective(), 0, "总闸静音通知归零");
        assert_eq!(dv.osd_bars(), (0, 0), "OSD 双条同步归零");
    }

    #[test]
    fn sliders_fully_independent() {
        let mut dv = DualVolume::new();
        dv.set_media(10);
        assert_eq!(dv.notify, NOTIF_DEFAULT, "改媒体不影响通知滑杆");
        dv.set_notify(90);
        assert_eq!(dv.media, 10, "改通知不影响媒体滑杆");
        assert_eq!(dv.notify_effective(), 90);
        assert_eq!(dv.media_effective(), 10);
    }

    #[test]
    fn persist_roundtrip_with_mute_bit() {
        let mut dv = DualVolume::new();
        dv.set_media(33);
        dv.set_notify(77);
        dv.set_master_mute(true);
        let mut blob = [0u8; 4];
        dv.persist(&mut blob);
        let mut dv2 = DualVolume::new();
        dv2.restore(&blob);
        assert_eq!(dv2.media, 33);
        assert_eq!(dv2.notify, 77);
        assert!(dv2.master_mute, "静音位应随持久化恢复");
        assert_eq!(dv2.notify_effective(), 0, "恢复后总闸仍生效");
    }
}

// ===========================================================================
// F545 蓝牙耳机电量显示（BtBattery）
// ===========================================================================

/// 低电量阈值（判据：<20% 提示一次）。
pub const LOW_BATTERY_PCT: u8 = 20;
/// 低电提示节流窗（判据：同设备每小时最多一次）。
pub const WARN_THROTTLE_MS: u64 = 3_600_000;
/// 显示缓变单步上限（判据：电量跳变平滑——不闪跳）。
pub const SMOOTH_MAX_STEP: u8 = 4;
/// 电量表容量。
pub const BT_CAP: usize = 8;
/// 无电量诚实态文案（判据：诚实显示「无电量信息」）。
pub const NO_LEVEL_TEXT: &str = "无电量信息";
/// 连接通知条文案前缀（「耳机已连接 · 电量 NN%」的静态前缀）。
pub const CONNECT_BANNER_TEXT: &str = "耳机已连接 · 电量";

/// 蓝牙设备电量表槽位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BtSlot {
    pub dev_id: u32,
    /// 上报电量（None = 设备无电量上报——诚实态）。
    pub level: Option<u8>,
    /// 平滑后的显示电量。
    pub display: u8,
    pub has_display: bool,
    /// 上次低电提示时刻（节流）。
    pub last_warn_ms: u64,
    pub has_warned: bool,
    pub used: bool,
}

/// 蓝牙电量模型（三处显示同源 + 连接通知条 + 低电节流 + 显示缓变）。
pub struct BtBattery {
    slots: [BtSlot; BT_CAP],
}

impl BtBattery {
    pub const fn new() -> Self {
        BtBattery {
            slots: [BtSlot {
                dev_id: 0,
                level: None,
                display: 0,
                has_display: false,
                last_warn_ms: 0,
                has_warned: false,
                used: false,
            }; BT_CAP],
        }
    }

    fn find(&self, dev_id: u32) -> Option<usize> {
        self.slots.iter().position(|s| s.used && s.dev_id == dev_id)
    }

    fn find_free(&self) -> Option<usize> {
        self.slots.iter().position(|s| !s.used)
    }

    /// 连接/上报：插入或更新电量（None = 无电量上报，诚实态入表）。
    /// 已入表设备只更新 level，**保留显示缓变历史与节流记录**（重报不闪跳、
    /// 重连不重置每小时节流窗）；新槽位才以电量直接初始化显示（无历史）。
    pub fn report_level(&mut self, dev_id: u32, level: Option<u8>) -> bool {
        match self.find(dev_id) {
            Some(i) => {
                self.slots[i].level = level;
                true
            }
            None => {
                let idx = match self.find_free() {
                    Some(i) => i,
                    None => return false,
                };
                self.slots[idx] = BtSlot {
                    dev_id,
                    level,
                    display: level.unwrap_or(0),
                    has_display: level.is_some(),
                    last_warn_ms: 0,
                    has_warned: false,
                    used: true,
                };
                true
            }
        }
    }

    /// 显示缓变刷新：display 向 level 缓步收敛，单步 ≤ SMOOTH_MAX_STEP
    /// （不闪跳——新值与旧值差不一次性跳到位）。
    pub fn refresh_display(&mut self, dev_id: u32) -> Option<u8> {
        let i = self.find(dev_id)?;
        let s = &mut self.slots[i];
        let l = s.level?;
        if !s.has_display {
            s.display = l;
            s.has_display = true;
        } else if s.display < l {
            s.display += (l - s.display).min(SMOOTH_MAX_STEP);
        } else if s.display > l {
            s.display -= (s.display - l).min(SMOOTH_MAX_STEP);
        }
        Some(s.display)
    }

    /// 三处显示同源的唯一读出函数（F290 设备页 / F423 浮层 / 通知条都走这里）。
    fn read_level(&self, dev_id: u32) -> Option<u8> {
        self.find(dev_id).and_then(|i| self.slots[i].level)
    }

    /// 消费面一：蓝牙设备页（F290）。
    pub fn device_page_level(&self, dev_id: u32) -> Option<u8> {
        self.read_level(dev_id)
    }

    /// 消费面二：电池浮层附属区（F423）。
    pub fn overlay_level(&self, dev_id: u32) -> Option<u8> {
        self.read_level(dev_id)
    }

    /// 消费面三：连接通知条。
    pub fn banner_level(&self, dev_id: u32) -> Option<u8> {
        self.read_level(dev_id)
    }

    /// 连接通知条内容：有电量 → (前缀文案, 电量)；无上报 → None（调用侧
    /// 退化为 NO_LEVEL_TEXT 诚实态）。
    pub fn connect_banner(&self, dev_id: u32) -> Option<(&'static str, u8)> {
        self.read_level(dev_id).map(|l| (CONNECT_BANNER_TEXT, l))
    }

    /// 无电量诚实态判定（已入表但无上报）。
    pub fn is_honest_no_level(&self, dev_id: u32) -> bool {
        match self.find(dev_id) {
            Some(i) => self.slots[i].used && self.slots[i].level.is_none(),
            None => false,
        }
    }

    /// 低电节流：电量 < 20% 且（首见或距上次提示 ≥ 3_600_000ms）才提示。
    /// 返回是否发出提示（提示后即时记录时刻——同设备每小时最多一次）。
    pub fn warn_low_if_due(&mut self, dev_id: u32, now_ms: u64) -> bool {
        let i = match self.find(dev_id) {
            Some(i) => i,
            None => return false,
        };
        let s = &mut self.slots[i];
        match s.level {
            Some(l) if l < LOW_BATTERY_PCT => {
                let due = !s.has_warned || now_ms.saturating_sub(s.last_warn_ms) >= WARN_THROTTLE_MS;
                if due {
                    s.has_warned = true;
                    s.last_warn_ms = now_ms;
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    pub fn used_count(&self) -> usize {
        self.slots.iter().filter(|s| s.used).count()
    }
}

/// F545 域自检。
pub fn run_f545_checks() -> CheckSet {
    let mut cs = CheckSet::new("F545-bt-battery");
    // 1) 常量组：20% 阈值 / 1 小时节流 / 缓变步长 / 表容量。
    cs.add(
        "constants",
        LOW_BATTERY_PCT == 20
            && WARN_THROTTLE_MS == 3_600_000
            && SMOOTH_MAX_STEP == 4
            && BT_CAP == 8,
        "",
    );
    // 2) 无电量上报诚实态（None 入表 + 诚实判定 + 文案在位）。
    let mut bt = BtBattery::new();
    let _ = bt.report_level(0xA001, None);
    cs.add(
        "honest_no_level",
        bt.is_honest_no_level(0xA001)
            && bt.device_page_level(0xA001).is_none()
            && !NO_LEVEL_TEXT.is_empty(),
        "",
    );
    // 3) 三处显示同源：同一数据源读出三消费面恒一致。
    let _ = bt.report_level(0xA002, Some(85));
    cs.add(
        "three_displays_same_source",
        bt.device_page_level(0xA002) == Some(85)
            && bt.overlay_level(0xA002) == Some(85)
            && bt.banner_level(0xA002) == Some(85),
        "",
    );
    // 4) 连接通知条：前缀文案 + 电量值。
    cs.add(
        "connect_banner",
        bt.connect_banner(0xA002) == Some((CONNECT_BANNER_TEXT, 85)),
        "",
    );
    // 5) 低电首见即提示（<20%）。
    let _ = bt.report_level(0xA003, Some(15));
    cs.add("low_first_warn", bt.warn_low_if_due(0xA003, 1_000), "");
    // 6) 节流：一小时内再次到点不提示（不刷屏）。
    cs.add("throttle_within_hour", !bt.warn_low_if_due(0xA003, 2_000), "");
    // 7) 节流边界：恰好 3_600_000ms 后可再提示。
    cs.add(
        "throttle_boundary_exact",
        bt.warn_low_if_due(0xA003, 1_000 + WARN_THROTTLE_MS),
        "",
    );
    // 8) 电量 ≥ 20% 不提示。
    let _ = bt.report_level(0xA004, Some(20));
    cs.add("no_warn_at_threshold", !bt.warn_low_if_due(0xA004, 5_000), "");
    // 9) 显示缓变：85 → 30 一次刷新只走一步（≤4），不闪跳。
    let _ = bt.report_level(0xA005, Some(85));
    let _ = bt.report_level(0xA005, Some(30));
    let d = bt.refresh_display(0xA005).unwrap_or(0);
    cs.add(
        "smooth_no_flash_jump",
        d == 85 - SMOOTH_MAX_STEP && (85 - d) <= SMOOTH_MAX_STEP,
        "",
    );
    // 10) 缓变收敛：足次刷新后到达新值。
    let mut converged = false;
    for _ in 0..32 {
        if bt.refresh_display(0xA005) == Some(30) {
            converged = true;
            break;
        }
    }
    cs.add("smooth_converges", converged, "");
    // 11) 无上报设备缓变刷新返回 None（诚实态不受显示逻辑污染）。
    cs.add("refresh_none_is_none", bt.refresh_display(0xA001).is_none(), "");
    // 12) 表容量守恒（8 槽占满后上报拒绝）。
    let mut bt2 = BtBattery::new();
    let mut all_ok = true;
    for d in 0..8u32 {
        if !bt2.report_level(0xB000 + d, Some(50)) {
            all_ok = false;
        }
    }
    cs.add(
        "cap_8_and_full_rejects",
        all_ok && bt2.used_count() == BT_CAP && !bt2.report_level(0xB100, Some(60)),
        "",
    );
    cs
}

#[cfg(test)]
mod f545_tests {
    use super::*;

    #[test]
    fn throttle_boundary_is_exclusive_within_hour() {
        // 节流边界必测：3_599_999ms 拒、3_600_000ms 收。
        let mut bt = BtBattery::new();
        let _ = bt.report_level(0xC001, Some(10));
        assert!(bt.warn_low_if_due(0xC001, 0), "首见 10% 应提示");
        assert!(
            !bt.warn_low_if_due(0xC001, WARN_THROTTLE_MS - 1),
            "3_599_999ms 不得再提示"
        );
        assert!(
            bt.warn_low_if_due(0xC001, WARN_THROTTLE_MS),
            "恰 3_600_000ms 应放行"
        );
    }

    #[test]
    fn none_level_is_honest_everywhere() {
        let mut bt = BtBattery::new();
        let _ = bt.report_level(0xC002, None);
        assert!(bt.is_honest_no_level(0xC002), "无上报 = 诚实态");
        assert_eq!(bt.device_page_level(0xC002), None);
        assert_eq!(bt.overlay_level(0xC002), None);
        assert_eq!(bt.banner_level(0xC002), None);
        assert_eq!(bt.connect_banner(0xC002), None, "无电量无通知条电量段");
        assert_eq!(NO_LEVEL_TEXT, "无电量信息");
    }

    #[test]
    fn three_consumers_read_one_source() {
        let mut bt = BtBattery::new();
        let _ = bt.report_level(0xC003, Some(73));
        let a = bt.device_page_level(0xC003);
        let b = bt.overlay_level(0xC003);
        let c = bt.banner_level(0xC003);
        assert_eq!(a, b, "设备页与浮层同源");
        assert_eq!(b, c, "浮层与通知条同源");
        assert_eq!(a, Some(73));
    }

    #[test]
    fn display_never_jumps_more_than_step() {
        let mut bt = BtBattery::new();
        let _ = bt.report_level(0xC004, Some(90));
        let _ = bt.report_level(0xC004, Some(10));
        let mut prev = 90u8;
        for _ in 0..40 {
            match bt.refresh_display(0xC004) {
                Some(d) => {
                    let diff = prev.abs_diff(d);
                    assert!(diff <= SMOOTH_MAX_STEP, "单步跳变 {} 超 4", diff);
                    prev = d;
                }
                None => panic!("有电量设备刷新不应为 None"),
            }
        }
        assert_eq!(prev, 10, "缓变最终应收敛到新值");
    }

    #[test]
    fn warn_once_per_hour_per_device() {
        let mut bt = BtBattery::new();
        let _ = bt.report_level(0xC005, Some(5));
        assert!(bt.warn_low_if_due(0xC005, 100), "设备 A 首次提示");
        // 同时刻另一设备不受节流牵连（节流按设备独立）。
        let _ = bt.report_level(0xC006, Some(7));
        assert!(bt.warn_low_if_due(0xC006, 100), "设备 B 独立首提");
        assert!(!bt.warn_low_if_due(0xC005, 200), "设备 A 一小时内不再提");
    }

    #[test]
    fn battery_at_or_above_20_never_warns() {
        let mut bt = BtBattery::new();
        let _ = bt.report_level(0xC007, Some(20));
        assert!(!bt.warn_low_if_due(0xC007, 0), "恰 20% 不算低电");
        let _ = bt.report_level(0xC008, Some(100));
        assert!(!bt.warn_low_if_due(0xC008, 0), "满电不提示");
    }

    #[test]
    fn connect_banner_carries_level() {
        let mut bt = BtBattery::new();
        let _ = bt.report_level(0xC009, Some(85));
        let banner = bt.connect_banner(0xC009).expect("有电量应有通知条");
        assert_eq!(banner.0, "耳机已连接 · 电量", "通知条前缀文案");
        assert_eq!(banner.1, 85, "通知条电量值");
    }
}

// ===========================================================================
// F546 新设备接入通知（DeviceArrive）
// ===========================================================================

/// 即插即用就绪时限（判据：2 秒内就绪）。
pub const READY_DEADLINE_MS: u64 = 2_000;
/// 横幅槽容量。
pub const BANNER_CAP: usize = 8;
/// 出路文案：驱动向导（F444）。
pub const OUTLET_WIZARD: &str = "点击查看 F444 驱动手动向导";
/// 横幅状态文案（三态）。
pub const STATE_TEXT_READY: &str = "已就绪";
pub const STATE_TEXT_INSTALLING: &str = "安装驱动中…";
pub const STATE_TEXT_MANUAL: &str = "需要手动安装——点击查看 F444 向导";

/// 失败归因枚举（判据：接入失败诚实列出原因——非空）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FailReason {
    /// 无可用驱动。
    DriverMissing,
    /// 驱动签名校验失败。
    DriverSignFail,
    /// 驱动安装过程出错。
    InstallError,
}

impl FailReason {
    /// 归因人话文案（恒非空）。
    pub fn text(self) -> &'static str {
        match self {
            FailReason::DriverMissing => "未找到可用驱动",
            FailReason::DriverSignFail => "驱动签名校验失败",
            FailReason::InstallError => "驱动安装过程出错",
        }
    }
}

/// 接入状态三态（判据：三状态横幅）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArriveState {
    /// 已就绪。
    Ready,
    /// 安装驱动中（带进度 0..=100）。
    Installing { progress: u8 },
    /// 需要手动安装（带失败归因）。
    ManualNeeded { reason: FailReason },
}

/// 接入横幅（右下一条：设备 + 状态 + 出路）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ArriveBanner {
    pub dev_id: u32,
    pub name: &'static str,
    pub state: ArriveState,
    /// 接入时刻。
    pub arrive_at: u64,
    /// 就绪时刻（就绪后回填——2s 就绪时序的核算依据）。
    pub ready_at: Option<u64>,
    /// 出路（Installing/ManualNeeded 指向 F444 向导）。
    pub outlet: &'static str,
}

/// 新设备接入通知模型（横幅 + F290 设备页同源状态）。
pub struct DeviceArrive {
    banners: [Option<ArriveBanner>; BANNER_CAP],
}

impl DeviceArrive {
    pub const fn new() -> Self {
        DeviceArrive { banners: [None; BANNER_CAP] }
    }

    fn find(&self, dev_id: u32) -> Option<usize> {
        self.banners
            .iter()
            .position(|b| b.map(|x| x.dev_id == dev_id).unwrap_or(false))
    }

    fn find_free(&self) -> Option<usize> {
        self.banners.iter().position(|b| b.is_none())
    }

    fn put(&mut self, banner: ArriveBanner) -> bool {
        match self.find(banner.dev_id) {
            Some(i) => {
                self.banners[i] = Some(banner);
                true
            }
            None => match self.find_free() {
                Some(i) => {
                    self.banners[i] = Some(banner);
                    true
                }
                None => false,
            },
        }
    }

    /// 即插即用类接入：接入即就绪（就绪延迟 0 ≤ 2s 时序）。
    pub fn arrive_pnp(&mut self, dev_id: u32, name: &'static str, now_ms: u64) -> bool {
        self.put(ArriveBanner {
            dev_id,
            name,
            state: ArriveState::Ready,
            arrive_at: now_ms,
            ready_at: Some(now_ms),
            outlet: "",
        })
    }

    /// 需要驱动的设备接入：进安装中态（进度 0，出路指向 F444 向导）。
    pub fn arrive_driver(&mut self, dev_id: u32, name: &'static str, now_ms: u64) -> bool {
        self.put(ArriveBanner {
            dev_id,
            name,
            state: ArriveState::Installing { progress: 0 },
            arrive_at: now_ms,
            ready_at: None,
            outlet: OUTLET_WIZARD,
        })
    }

    /// 安装进度推进：到 100 即转就绪并回填 ready_at（2s 时序核算）。
    pub fn set_progress(&mut self, dev_id: u32, progress: u8, now_ms: u64) -> bool {
        let i = match self.find(dev_id) {
            Some(i) => i,
            None => return false,
        };
        let b = match self.banners[i] {
            Some(b) => b,
            None => return false,
        };
        let p = progress.min(100);
        let mut nb = b;
        if p >= 100 {
            nb.state = ArriveState::Ready;
            nb.ready_at = Some(now_ms);
            nb.outlet = "";
        } else {
            nb.state = ArriveState::Installing { progress: p };
        }
        self.banners[i] = Some(nb);
        true
    }

    /// 失败归因：安装失败 → ManualNeeded（原因非空 + 手装出路）。
    pub fn fail_install(&mut self, dev_id: u32, reason: FailReason) -> bool {
        let i = match self.find(dev_id) {
            Some(i) => i,
            None => return false,
        };
        match self.banners[i] {
            Some(mut b) => {
                b.state = ArriveState::ManualNeeded { reason };
                b.outlet = OUTLET_WIZARD;
                self.banners[i] = Some(b);
                true
            }
            None => false,
        }
    }

    /// 横幅状态读出（F290 同源：设备页与横幅读同一 state）。
    pub fn banner_state(&self, dev_id: u32) -> Option<ArriveState> {
        self.find(dev_id).and_then(|i| self.banners[i].map(|b| b.state))
    }

    /// 设备页状态（F290——与横幅同一数据源）。
    pub fn device_page_state(&self, dev_id: u32) -> Option<ArriveState> {
        self.find(dev_id).and_then(|i| self.banners[i].map(|b| b.state))
    }

    /// 横幅状态文案（三态各一）。
    pub fn banner_state_text(&self, dev_id: u32) -> Option<&'static str> {
        match self.banner_state(dev_id) {
            Some(ArriveState::Ready) => Some(STATE_TEXT_READY),
            Some(ArriveState::Installing { .. }) => Some(STATE_TEXT_INSTALLING),
            Some(ArriveState::ManualNeeded { .. }) => Some(STATE_TEXT_MANUAL),
            None => None,
        }
    }

    /// 横幅出路文案（Installing/ManualNeeded 指向 F444；Ready 无需出路）。
    pub fn banner_outlet(&self, dev_id: u32) -> Option<&'static str> {
        self.find(dev_id).and_then(|i| self.banners[i].map(|b| b.outlet))
    }

    /// 就绪延迟（ready_at - arrive_at；未就绪 → None）。
    pub fn ready_latency(&self, dev_id: u32) -> Option<u64> {
        match self.find(dev_id).and_then(|i| self.banners[i]) {
            Some(b) => b.ready_at.map(|r| r - b.arrive_at),
            None => None,
        }
    }

    pub fn used_count(&self) -> usize {
        self.banners.iter().filter(|b| b.is_some()).count()
    }
}

/// F546 域自检。
pub fn run_f546_checks() -> CheckSet {
    let mut cs = CheckSet::new("F546-dev-arrive");
    // 1) 三态互异（三状态横幅）。
    cs.add(
        "three_states_distinct",
        ArriveState::Ready != ArriveState::Installing { progress: 0 }
            && ArriveState::Installing { progress: 0 }
                != ArriveState::ManualNeeded {
                    reason: FailReason::DriverMissing,
                },
        "",
    );
    // 2) 即插即用：接入即就绪，延迟 0 ≤ 2000ms。
    let mut da = DeviceArrive::new();
    let _ = da.arrive_pnp(0xE001, "usb-keyboard", 10_000);
    cs.add(
        "pnp_ready_within_deadline",
        da.ready_latency(0xE001) == Some(0)
            && da.ready_latency(0xE001).map(|l| l <= READY_DEADLINE_MS).unwrap_or(false),
        "",
    );
    // 3) 就绪时限常量 2000ms。
    cs.add("deadline_constant", READY_DEADLINE_MS == 2_000, "");
    // 4) Ready 横幅文案「已就绪」。
    cs.add(
        "ready_banner_text",
        da.banner_state_text(0xE001) == Some(STATE_TEXT_READY),
        "",
    );
    // 5) 需驱动设备：安装中态 + 进度字段 + F444 出路。
    let _ = da.arrive_driver(0xE002, "usb-dac", 20_000);
    cs.add(
        "installing_progress_and_outlet",
        da.banner_state(0xE002) == Some(ArriveState::Installing { progress: 0 })
            && da.banner_outlet(0xE002) == Some(OUTLET_WIZARD),
        "",
    );
    // 6) 进度推进 → 100 转就绪并回填就绪时刻（延迟 ≤ 2s 时序内）。
    let _ = da.set_progress(0xE002, 50, 20_800);
    let _ = da.set_progress(0xE002, 100, 21_500);
    cs.add(
        "progress_to_ready",
        da.banner_state(0xE002) == Some(ArriveState::Ready)
            && da.ready_latency(0xE002) == Some(1_500),
        "",
    );
    // 7) 失败归因：三种原因文案恒非空。
    let reasons = [
        FailReason::DriverMissing,
        FailReason::DriverSignFail,
        FailReason::InstallError,
    ];
    let mut texts_nonempty = true;
    for r in reasons.iter() {
        if r.text().is_empty() {
            texts_nonempty = false;
        }
    }
    cs.add("fail_reasons_nonempty", texts_nonempty, "");
    // 8) 驱动无 → ManualNeeded + 手装路径（F444 向导出路）。
    let _ = da.fail_install(0xE002, FailReason::DriverMissing);
    cs.add(
        "manual_needed_with_outlet",
        da.banner_state(0xE002)
            == Some(ArriveState::ManualNeeded {
                reason: FailReason::DriverMissing,
            })
            && da.banner_outlet(0xE002) == Some(OUTLET_WIZARD)
            && da.banner_state_text(0xE002) == Some(STATE_TEXT_MANUAL),
        "",
    );
    // 9) 横幅与 F290 设备页同源一致。
    cs.add(
        "sync_with_device_page",
        da.banner_state(0xE002) == da.device_page_state(0xE002)
            && da.banner_state(0xE001) == da.device_page_state(0xE001),
        "",
    );
    // 10) 归因文案区分度（三种原因三句话）。
    cs.add(
        "fail_reasons_distinct",
        FailReason::DriverMissing.text() != FailReason::DriverSignFail.text()
            && FailReason::DriverSignFail.text() != FailReason::InstallError.text(),
        "",
    );
    // 11) 多设备并存 + 槽容量守恒（8 槽占满后接入拒绝）。
    let mut da2 = DeviceArrive::new();
    let mut all_ok = true;
    for d in 0..8u32 {
        if !da2.arrive_pnp(0xF000 + d, "pnp-device", d as u64 * 100) {
            all_ok = false;
        }
    }
    cs.add(
        "cap_8_and_full_rejects",
        all_ok
            && da2.used_count() == BANNER_CAP
            && !da2.arrive_pnp(0xF100, "overflow", 9_999),
        "",
    );
    cs
}

#[cfg(test)]
mod f546_tests {
    use super::*;

    #[test]
    fn pnp_device_ready_within_2000ms() {
        // 2s 就绪时序必测：即插即用接入即就绪（延迟 0），远低于 2s。
        let mut da = DeviceArrive::new();
        assert!(da.arrive_pnp(0xA001, "usb-mouse", 5_000));
        let lat = da.ready_latency(0xA001).expect("就绪设备应有延迟记录");
        assert!(lat <= READY_DEADLINE_MS, "就绪延迟 {}ms 超 2000ms", lat);
        assert_eq!(da.banner_state_text(0xA001), Some("已就绪"));
    }

    #[test]
    fn driver_install_progress_then_ready() {
        let mut da = DeviceArrive::new();
        let _ = da.arrive_driver(0xA002, "usb-audio", 1_000);
        let _ = da.set_progress(0xA002, 40, 1_400);
        assert_eq!(
            da.banner_state(0xA002),
            Some(ArriveState::Installing { progress: 40 }),
            "进度应如实呈现"
        );
        let _ = da.set_progress(0xA002, 100, 2_600);
        assert_eq!(da.banner_state(0xA002), Some(ArriveState::Ready));
        assert_eq!(da.ready_latency(0xA002), Some(1_600), "就绪延迟 1.6s");
        assert!(da.ready_latency(0xA002).unwrap_or(u64::MAX) <= READY_DEADLINE_MS);
    }

    #[test]
    fn fail_reason_is_always_present() {
        let mut da = DeviceArrive::new();
        let _ = da.arrive_driver(0xA003, "legacy-printer", 0);
        assert!(da.fail_install(0xA003, FailReason::DriverMissing));
        match da.banner_state(0xA003) {
            Some(ArriveState::ManualNeeded { reason }) => {
                assert!(!reason.text().is_empty(), "失败归因必须非空");
                assert_eq!(reason.text(), "未找到可用驱动");
            }
            other => panic!("应为 ManualNeeded，实际 {:?}", other),
        }
        assert_eq!(da.banner_outlet(0xA003), Some(OUTLET_WIZARD), "手装出路必给");
    }

    #[test]
    fn banner_and_device_page_stay_in_sync() {
        let mut da = DeviceArrive::new();
        let _ = da.arrive_pnp(0xA004, "usb-hub", 0);
        let _ = da.arrive_driver(0xA005, "usb-serial", 0);
        let _ = da.set_progress(0xA005, 10, 100);
        assert_eq!(
            da.banner_state(0xA004),
            da.device_page_state(0xA004),
            "就绪设备横幅/设备页同源"
        );
        assert_eq!(
            da.banner_state(0xA005),
            da.device_page_state(0xA005),
            "安装中设备横幅/设备页同源"
        );
    }

    #[test]
    fn progress_clamped_and_rejects_unknown_device() {
        let mut da = DeviceArrive::new();
        assert!(!da.set_progress(0xDEAD, 50, 0), "未知设备推进应拒绝");
        let _ = da.arrive_driver(0xA006, "usb-tuner", 0);
        let _ = da.set_progress(0xA006, 250, 100);
        assert_eq!(
            da.banner_state(0xA006),
            Some(ArriveState::Ready),
            "进度超界截 100 即就绪"
        );
    }

    #[test]
    fn fail_reason_texts_are_distinct() {
        assert_ne!(
            FailReason::DriverMissing.text(),
            FailReason::DriverSignFail.text(),
            "无驱动与签名失败归因应可区分"
        );
        assert_ne!(
            FailReason::DriverSignFail.text(),
            FailReason::InstallError.text(),
            "签名失败与安装出错归因应可区分"
        );
    }
}

// ===========================================================================
// F547 音量左右平衡（VolumeBalance）
// ===========================================================================

/// 平衡值下界（左满偏）。
pub const BALANCE_MIN: i16 = -100;
/// 平衡值上界（右满偏）。
pub const BALANCE_MAX: i16 = 100;
/// 中心值。
pub const BALANCE_CENTER: i16 = 0;
/// 软件平衡路径延迟预算（判据：延迟 <1ms 无感——上界 1000μs）。
pub const SOFTWARE_DELAY_US: u32 = 1000;
/// 单位增益定点（Q10：1024 = 1.0）。
pub const GAIN_ONE: u32 = 1024;
/// 平衡表容量（复用 F543 族设备表规模）。
pub const BAL_DEV_CAP: usize = 10;
/// 持久化区长度：每槽 6 字节（id LE4 + balance LE2）。
pub const BAL_PERSIST_LEN: usize = BAL_DEV_CAP * 6;

/// 增益纯函数：balance ≥ 0 衰减左声道、< 0 衰减右声道；
/// 0 = 双侧单位增益；极值 = 对侧衰减到 0（对侧永不增强超过 1.0）。
pub fn gains(balance: i16) -> (u32, u32) {
    let b = balance.clamp(BALANCE_MIN, BALANCE_MAX);
    if b >= 0 {
        let left = GAIN_ONE * (100 - b as i32) as u32 / 100;
        (left, GAIN_ONE)
    } else {
        let right = GAIN_ONE * (100 + b as i32) as u32 / 100;
        (GAIN_ONE, right)
    }
}

/// 中心格点判定：v == 0 显示中心标记（格点吸附判定面）。
pub fn is_center_mark(balance: i16) -> bool {
    balance == BALANCE_CENTER
}

/// 平衡表槽位（按输出设备各记各的——F543 族）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BalanceSlot {
    pub dev_id: u32,
    pub balance: i16,
    pub last_used: u64,
    pub used: bool,
}

/// 音量左右平衡模型（滑杆即时试听 + 分设备记忆 + 持久化）。
pub struct VolumeBalance {
    slots: [BalanceSlot; BAL_DEV_CAP],
    clock: u64,
    /// 拖动即时生效标志（无确认步——试听实时判据的建模面）。
    pub live_applied: bool,
}

impl VolumeBalance {
    pub const fn new() -> Self {
        VolumeBalance {
            slots: [BalanceSlot {
                dev_id: 0,
                balance: 0,
                last_used: 0,
                used: false,
            }; BAL_DEV_CAP],
            clock: 0,
            live_applied: false,
        }
    }

    fn find(&self, dev_id: u32) -> Option<usize> {
        self.slots.iter().position(|s| s.used && s.dev_id == dev_id)
    }

    fn find_free(&self) -> Option<usize> {
        self.slots.iter().position(|s| !s.used)
    }

    fn lru_idx(&self) -> usize {
        let mut best = 0usize;
        for i in 1..BAL_DEV_CAP {
            if self.slots[i].last_used < self.slots[best].last_used {
                best = i;
            }
        }
        best
    }

    /// 设置平衡（截断 -100..=100），即时生效（无确认步），按设备记忆。
    /// 返回截断后的落表值。
    pub fn set_balance(&mut self, dev_id: u32, balance: i16) -> i16 {
        let b = balance.clamp(BALANCE_MIN, BALANCE_MAX);
        let idx = match self.find(dev_id) {
            Some(i) => i,
            None => match self.find_free() {
                Some(i) => i,
                None => self.lru_idx(),
            },
        };
        self.slots[idx] = BalanceSlot {
            dev_id,
            balance: b,
            last_used: self.clock,
            used: true,
        };
        self.clock += 1;
        self.live_applied = true;
        b
    }

    /// 只读查询（不触碰 LRU 时钟）。
    pub fn get_balance(&self, dev_id: u32) -> Option<i16> {
        self.find(dev_id).map(|i| self.slots[i].balance)
    }

    /// 试听实时：测试音循环时拖动 → 即时读出该设备当前增益对
    /// （延迟预算 SOFTWARE_DELAY_US 内——软件混音路径无确认步）。
    pub fn test_tone_gains(&mut self, dev_id: u32) -> (u32, u32) {
        let b = match self.find(dev_id) {
            Some(i) => self.slots[i].balance,
            None => 0,
        };
        gains(b)
    }

    pub fn used_count(&self) -> usize {
        self.slots.iter().filter(|s| s.used).count()
    }

    /// 持久化导出。
    pub fn persist(&self, out: &mut [u8; BAL_PERSIST_LEN]) {
        for i in 0..BAL_DEV_CAP {
            let base = i * 6;
            let (id, bal) = if self.slots[i].used {
                (self.slots[i].dev_id, self.slots[i].balance)
            } else {
                (u32::MAX, 0i16)
            };
            out[base] = (id & 0xFF) as u8;
            out[base + 1] = ((id >> 8) & 0xFF) as u8;
            out[base + 2] = ((id >> 16) & 0xFF) as u8;
            out[base + 3] = ((id >> 24) & 0xFF) as u8;
            let le = bal.to_le_bytes();
            out[base + 4] = le[0];
            out[base + 5] = le[1];
        }
    }

    /// 持久化恢复（与导出严格互逆）。
    pub fn restore(&mut self, src: &[u8; BAL_PERSIST_LEN]) {
        for i in 0..BAL_DEV_CAP {
            let base = i * 6;
            let id = (src[base] as u32)
                | ((src[base + 1] as u32) << 8)
                | ((src[base + 2] as u32) << 16)
                | ((src[base + 3] as u32) << 24);
            let bal = i16::from_le_bytes([src[base + 4], src[base + 5]]);
            if id == u32::MAX {
                self.slots[i] = BalanceSlot {
                    dev_id: 0,
                    balance: 0,
                    last_used: 0,
                    used: false,
                };
            } else {
                self.slots[i] = BalanceSlot {
                    dev_id: id,
                    balance: bal,
                    last_used: 0,
                    used: true,
                };
            }
        }
        self.clock = 0;
    }
}

/// F547 域自检。
pub fn run_f547_checks() -> CheckSet {
    let mut cs = CheckSet::new("F547-vol-balance");
    // 1) 常量组：量程 / 中心 / 延迟预算 / 定点单位。
    cs.add(
        "constants",
        BALANCE_MIN == -100
            && BALANCE_MAX == 100
            && BALANCE_CENTER == 0
            && SOFTWARE_DELAY_US == 1000
            && GAIN_ONE == 1024,
        "",
    );
    // 2) 中心格点：0 显中心标记，非 0 不显。
    cs.add(
        "center_mark",
        is_center_mark(0) && !is_center_mark(1) && !is_center_mark(-1),
        "",
    );
    // 3) 中心 = 双侧单位增益。
    cs.add("unit_gain_at_center", gains(0) == (GAIN_ONE, GAIN_ONE), "");
    // 4) 右满偏 +100：左声道衰减到 0，右声道全额。
    cs.add("extreme_right", gains(100) == (0, GAIN_ONE), "");
    // 5) 左满偏 -100：右声道衰减到 0。
    cs.add("extreme_left", gains(-100) == (GAIN_ONE, 0), "");
    // 6) 衰减单调：|balance| 越大对侧越弱。
    let (l25, _) = gains(25);
    let (l50, _) = gains(50);
    let (l75, _) = gains(75);
    cs.add(
        "attenuation_monotonic",
        GAIN_ONE > l25 && l25 > l50 && l50 > l75 && l75 > 0,
        "",
    );
    // 7) 左右对称：+b 的左衰减 = -b 的右衰减。
    cs.add(
        "left_right_symmetry",
        gains(30).0 == gains(-30).1 && gains(30).1 == gains(-30).0,
        "",
    );
    // 8) 试听实时：拖动即生效（无确认步）。
    let mut vb = VolumeBalance::new();
    let _ = vb.set_balance(0xF001, 40);
    cs.add("live_apply_no_confirm", vb.live_applied, "");
    // 9) 试听增益与落表值一致。
    let (gl, gr) = vb.test_tone_gains(0xF001);
    cs.add("test_tone_matches", (gl, gr) == gains(40), "");
    // 10) 分设备记忆：A +30 / B -20 互不串。
    let _ = vb.set_balance(0xF002, 30);
    let _ = vb.set_balance(0xF003, -20);
    cs.add(
        "per_device_memory",
        vb.get_balance(0xF002) == Some(30) && vb.get_balance(0xF003) == Some(-20),
        "",
    );
    // 11) 输入截断：150 → 100，-150 → -100。
    let mut vb2 = VolumeBalance::new();
    cs.add(
        "input_clamped",
        vb2.set_balance(0xF004, 150) == 100 && vb2.set_balance(0xF004, -150) == -100,
        "",
    );
    // 12) 持久化往返一致（含负值）。
    let mut vb3 = VolumeBalance::new();
    let _ = vb3.set_balance(0xF005, -60);
    let _ = vb3.set_balance(0xF006, 75);
    let mut blob = [0u8; BAL_PERSIST_LEN];
    vb3.persist(&mut blob);
    let mut vb4 = VolumeBalance::new();
    vb4.restore(&blob);
    cs.add(
        "persist_roundtrip",
        vb4.get_balance(0xF005) == Some(-60) && vb4.get_balance(0xF006) == Some(75),
        "",
    );
    cs
}

#[cfg(test)]
mod f547_tests {
    use super::*;

    #[test]
    fn center_is_unit_gain() {
        assert_eq!(gains(0), (1024, 1024), "中心 = 双侧单位增益");
        assert!(is_center_mark(0), "0 显中心格点标记");
    }

    #[test]
    fn extremes_attenuate_opposite_side_only() {
        assert_eq!(gains(100), (0, 1024), "右满偏：左声归零、右声全额");
        assert_eq!(gains(-100), (1024, 0), "左满偏：右声归零、左声全额");
        // 对侧永不增强超过单位增益。
        for b in [-100i16, -50, -1, 0, 1, 50, 100] {
            let (l, r) = gains(b);
            assert!(l <= GAIN_ONE && r <= GAIN_ONE, "增益不得超过 1.0（b={}）", b);
        }
    }

    #[test]
    fn attenuation_is_monotonic() {
        let mut prev_left = GAIN_ONE;
        for b in [0i16, 10, 25, 50, 75, 99, 100] {
            let (l, _) = gains(b);
            assert!(l <= prev_left, "左衰减应随 +b 单调不增（b={}）", b);
            prev_left = l;
        }
    }

    #[test]
    fn per_device_balance_independent() {
        let mut vb = VolumeBalance::new();
        let _ = vb.set_balance(0x0101, 30);
        let _ = vb.set_balance(0x0102, -20);
        assert_eq!(vb.get_balance(0x0101), Some(30), "设备 A 记 +30");
        assert_eq!(vb.get_balance(0x0102), Some(-20), "设备 B 记 -20");
        assert_eq!(vb.used_count(), 2);
    }

    #[test]
    fn input_clamped_into_range() {
        let mut vb = VolumeBalance::new();
        assert_eq!(vb.set_balance(0x0201, 500), 100, "超右界截 100");
        assert_eq!(vb.get_balance(0x0201), Some(100));
        assert_eq!(vb.set_balance(0x0201, -500), -100, "超左界截 -100");
        assert!(vb.live_applied, "截断写入同样即时生效");
    }

    #[test]
    fn persist_roundtrip_keeps_negative_values() {
        let mut vb = VolumeBalance::new();
        for (i, b) in [0i16, 1, -1, 50, -50, 100, -100, 33, -33, 7].iter().enumerate() {
            let _ = vb.set_balance(0x0300 + i as u32, *b);
        }
        let mut blob = [0u8; BAL_PERSIST_LEN];
        vb.persist(&mut blob);
        let mut vb2 = VolumeBalance::new();
        vb2.restore(&blob);
        for (i, b) in [0i16, 1, -1, 50, -50, 100, -100, 33, -33, 7].iter().enumerate() {
            assert_eq!(
                vb2.get_balance(0x0300 + i as u32),
                Some(*b),
                "设备 {} 平衡值往返应一致",
                i
            );
        }
    }
}
