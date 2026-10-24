//! VE-F0070 · 帧回调预算接口（VE-A 域 · A03b 同步与呈现组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0070`
//!
//! **职责定位（锚点原文）**：注册帧回调（每帧 vsync 前触发）按预算制治理：
//! 单回调时长配额，越限者降频或停用并通知注册方；回调异常捕获隔离不影响
//! 呈现主路。
//!
//! **数据结构（锚点原文）**：回调注册表（回调 × 配额 × 状态）；越限记录。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 回调抛错 → 捕获隔离并停用该回调（主路零感知）；
//! - 配额越限 → 降频阶梯（每帧、隔帧、停用）；
//! - 注册表损坏 → 全量重建。
//!
//! **性能逐项分解（锚点原文）**：触发 O(1)；配额计数 O(1)；隔离 O(1)。
//!
//! **跨批对接点（锚点原文）**：上游呈现主循环；下游 F0096 采样率预算治理
//! 同构语义。
//!
//! **无障碍与隐私（锚点原文）**：无直接无障碍面；停用通知三要素。
//!
//! **判据（锚点原文）**：预算制、降频阶梯、异常隔离、停用通知、判据。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数；确定性。
//!
//! 诊断段：0xE8A0 基址（全域核查无占用）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const CBUDGET_VERSION: &str = "vea70_cbudget-v1";

/// 诊断段基址。
pub const SEG_BASE: u32 = 0xE8A0;

// ---- 诊断码 ----
pub const XC_NO_CB: u32 = SEG_BASE + 1; // 回调不存在
pub const XC_NO_OWNER: u32 = SEG_BASE + 2; // 注册方身份缺失
pub const XC_BAD_QUOTA: u32 = SEG_BASE + 3; // 配额参数非法
pub const XC_DISABLED: u32 = SEG_BASE + 4; // 回调已停用

/// 降频阶梯（锚点原文：每帧、隔帧、停用——三档写死不给调用方猜）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cadence {
    /// 每帧触发（满频）。
    EveryFrame = 1,
    /// 隔帧触发（半频）。
    EveryOtherFrame = 2,
    /// 停用（阶梯终点；恢复须显式重注册）。
    Disabled = 3,
}

/// 回调注册条目。
#[derive(Clone, Copy, Debug)]
pub struct CallbackRec {
    /// 回调标识。
    pub cb_id: u32,
    /// 注册方身份（停用通知的收件人；0 = 缺失即拒注册）。
    pub owner: u32,
    /// 单次时长配额（µs）。
    pub quota_us: u32,
    /// 当前档位。
    pub cadence: Cadence,
    /// 连续越限次数（阶梯晋升依据）。
    pub strikes: u32,
    /// 累计触发次数。
    pub fired: u32,
    /// 停用原因码（0 = 在用）。
    pub disabled_reason: u32,
}

/// 越限记录（留痕：id × 实测 × 配额）。
#[derive(Clone, Copy, Debug)]
pub struct StrikeRec {
    pub cb_id: u32,
    pub measured_us: u32,
    pub quota_us: u32,
}

/// 帧回调预算治理器。
pub struct CallbackBudget {
    recs: Vec<CallbackRec>,
    cap: usize,
    last_code: u32,
    strikes: Vec<StrikeRec>,
    /// 阶梯晋升阈值（连续越限次数）。
    promote_after: u32,
    /// 当前帧号（隔帧触发的判定基准）。
    frame: u64,
}

impl CallbackBudget {
    pub fn new(cap: usize) -> CallbackBudget {
        CallbackBudget {
            recs: Vec::new(),
            cap,
            last_code: 0,
            strikes: Vec::new(),
            promote_after: 3,
            frame: 0,
        }
    }

    /// 注册回调（预算制：配额必须 > 0，注册方身份必须非零）。
    pub fn register(&mut self, cb_id: u32, owner: u32, quota_us: u32) -> Result<(), u32> {
        if cb_id == 0 || owner == 0 {
            self.last_code = XC_NO_OWNER;
            return Err(self.last_code);
        }
        if quota_us == 0 || quota_us > 16_667 {
            // 配额上限 = 单帧时长（60Hz 预算），超出即参数非法。
            self.last_code = XC_BAD_QUOTA;
            return Err(self.last_code);
        }
        if self.recs.iter().any(|r| r.cb_id == cb_id) || self.recs.len() >= self.cap {
            self.last_code = XC_NO_CB;
            return Err(self.last_code);
        }
        self.recs.push(CallbackRec {
            cb_id,
            owner,
            quota_us,
            cadence: Cadence::EveryFrame,
            strikes: 0,
            fired: 0,
            disabled_reason: 0,
        });
        Ok(())
    }

    /// 帧推进（触发入口的前置）。
    pub fn next_frame(&mut self) {
        self.frame += 1;
    }

    /// 本帧某回调是否应触发（按档位；停用永不触发）。
    pub fn should_fire(&self, cb_id: u32) -> bool {
        match self.recs.iter().find(|r| r.cb_id == cb_id) {
            Some(r) => match r.cadence {
                Cadence::EveryFrame => true,
                Cadence::EveryOtherFrame => self.frame % 2 == 0,
                Cadence::Disabled => false,
            },
            None => false,
        }
    }

    /// 回调执行回报（预算制核心）：实测时长入账；越限记 strike；
    /// 连续越限达阈值 → 阶梯晋升（每帧→隔帧→停用）。
    pub fn report(&mut self, cb_id: u32, measured_us: u32, faulted: bool) -> Result<Cadence, u32> {
        let idx = match self.recs.iter().position(|r| r.cb_id == cb_id) {
            Some(i) => i,
            None => {
                self.last_code = XC_NO_CB;
                return Err(self.last_code);
            }
        };
        // 异常隔离：抛错回调立即停用，主路零感知。
        if faulted {
            self.recs[idx].cadence = Cadence::Disabled;
            self.recs[idx].disabled_reason = XC_DISABLED;
            return Ok(Cadence::Disabled);
        }
        if measured_us > self.recs[idx].quota_us {
            self.recs[idx].strikes += 1;
            if self.strikes.len() >= 64 {
                self.strikes.remove(0);
            }
            self.strikes.push(StrikeRec { cb_id, measured_us, quota_us: self.recs[idx].quota_us });
            if self.recs[idx].strikes >= self.promote_after {
                // 阶梯晋升：每帧 → 隔帧 → 停用。
                match self.recs[idx].cadence {
                    Cadence::EveryFrame => self.recs[idx].cadence = Cadence::EveryOtherFrame,
                    Cadence::EveryOtherFrame => {
                        self.recs[idx].cadence = Cadence::Disabled;
                        self.recs[idx].disabled_reason = XC_BAD_QUOTA;
                    }
                    Cadence::Disabled => {}
                }
                self.recs[idx].strikes = 0;
            }
        } else {
            // 未越限：连续计数回落（不归零历史档位——降档需显式重注册）。
            self.recs[idx].strikes = self.recs[idx].strikes.saturating_sub(1);
        }
        self.recs[idx].fired += 1;
        Ok(self.recs[idx].cadence)
    }

    /// 停用通知三要素（发生了什么/为什么/下一步）。
    pub fn disable_notice(&self, cb_id: u32) -> String {
        match self.recs.iter().find(|r| r.cb_id == cb_id) {
            Some(r) if r.cadence == Cadence::Disabled => alloc::format!(
                "回调 {} 已停用（发生了什么：连续越限或执行异常；为什么：配额 {}µs 被持续突破或回调抛错；下一步：修复后以新配额重新注册）",
                cb_id,
                r.quota_us
            ),
            Some(_) => alloc::format!("回调 {} 在用", cb_id),
            None => alloc::format!("回调 {} 不存在", cb_id),
        }
    }

    /// 注册表全量重建（注册表损坏降级）：清空全部条目（注册方需重注册）。
    pub fn rebuild_registry(&mut self) {
        self.recs.clear();
        self.strikes.clear();
    }

    pub fn strike_count(&self) -> usize {
        self.strikes.len()
    }
    pub fn cadence_of(&self, cb_id: u32) -> Option<Cadence> {
        self.recs.iter().find(|r| r.cb_id == cb_id).map(|r| r.cadence)
    }
    pub fn len(&self) -> usize {
        self.recs.len()
    }
    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }
    pub fn last_code(&self) -> u32 {
        self.last_code
    }

    /// 读屏状态行。
    pub fn status_line(&self) -> String {
        let per = self.recs.iter().filter(|r| r.cadence == Cadence::EveryFrame).count();
        let half = self.recs.iter().filter(|r| r.cadence == Cadence::EveryOtherFrame).count();
        let off = self.recs.iter().filter(|r| r.cadence == Cadence::Disabled).count();
        alloc::format!(
            "CBUDGET 回调{} 每帧{} 隔帧{} 停用{} 越限记录{}",
            self.recs.len(),
            per,
            half,
            off,
            self.strikes.len()
        )
    }
}

// =====================================================================
// 自检（判据锚点：预算制 / 降频阶梯 / 异常隔离 / 停用通知）
// =====================================================================

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, detail);
}

/// VE-F0070 域自检。
pub fn run_vea70_checks() -> CheckSet {
    let mut set = CheckSet::new("vea70");

    // C70-01 预算制：合法注册落位满频
    {
        let mut g = CallbackBudget::new(8);
        let ok = g.register(1, 100, 2000).is_ok() && g.cadence_of(1) == Some(Cadence::EveryFrame);
        chk(&mut set, "C70-01 注册落位", ok, "注册未落位");
    }

    // C70-02 预算制：配额超单帧时长拒绝
    {
        let mut g = CallbackBudget::new(8);
        let ok = g.register(1, 100, 16_668) == Err(XC_BAD_QUOTA);
        chk(&mut set, "C70-02 配额上限", ok, "超帧配额被放行");
    }

    // C70-03 注册方身份缺失拒绝
    {
        let mut g = CallbackBudget::new(8);
        let ok = g.register(1, 0, 2000) == Err(XC_NO_OWNER);
        chk(&mut set, "C70-03 身份必填", ok, "无身份被放行");
    }

    // C70-04 降频阶梯第一步：连续 3 次越限 → 隔帧
    {
        let mut g = CallbackBudget::new(8);
        let _ = g.register(1, 100, 1000);
        for _ in 0..3 {
            let _ = g.report(1, 1500, false);
        }
        let ok = g.cadence_of(1) == Some(Cadence::EveryOtherFrame) && g.strike_count() == 3;
        chk(&mut set, "C70-04 隔帧晋升", ok, "未晋升隔帧");
    }

    // C70-05 降频阶梯第二步：再 3 次 → 停用
    {
        let mut g = CallbackBudget::new(8);
        let _ = g.register(1, 100, 1000);
        for _ in 0..6 {
            let _ = g.report(1, 1500, false);
        }
        let ok = g.cadence_of(1) == Some(Cadence::Disabled);
        chk(&mut set, "C70-05 停用终点", ok, "未走到停用");
    }

    // C70-06 隔帧语义：偶数帧触发、奇数帧跳过
    {
        let mut g = CallbackBudget::new(8);
        let _ = g.register(1, 100, 1000);
        for _ in 0..3 {
            let _ = g.report(1, 2000, false);
        }
        g.next_frame(); // frame=1（奇）
        let odd = g.should_fire(1);
        g.next_frame(); // frame=2（偶）
        let even = g.should_fire(1);
        let ok = !odd && even;
        chk(&mut set, "C70-06 隔帧语义", ok, "隔帧判定失准");
    }

    // C70-07 异常隔离：抛错回调立即停用且主路可查
    {
        let mut g = CallbackBudget::new(8);
        let _ = g.register(1, 100, 1000);
        let _ = g.register(2, 100, 1000);
        let _ = g.report(1, 10, true);
        let ok = g.cadence_of(1) == Some(Cadence::Disabled)
            && g.cadence_of(2) == Some(Cadence::EveryFrame);
        chk(&mut set, "C70-07 异常隔离", ok, "异常殃及主路");
    }

    // C70-08 停用通知三要素
    {
        let mut g = CallbackBudget::new(8);
        let _ = g.register(1, 100, 1000);
        for _ in 0..6 {
            let _ = g.report(1, 1500, false);
        }
        let n = g.disable_notice(1);
        let ok = n.contains("发生了什么") && n.contains("为什么") && n.contains("下一步");
        chk(&mut set, "C70-08 通知三要素", ok, "通知缺要素");
    }

    // C70-09 停用回调不触发
    {
        let mut g = CallbackBudget::new(8);
        let _ = g.register(1, 100, 1000);
        for _ in 0..6 {
            let _ = g.report(1, 1500, false);
        }
        g.next_frame();
        g.next_frame();
        let ok = !g.should_fire(1);
        chk(&mut set, "C70-09 停用不触发", ok, "停用仍触发");
    }

    // C70-10 越限记录留痕（实测 × 配额）
    {
        let mut g = CallbackBudget::new(8);
        let _ = g.register(1, 100, 1000);
        let _ = g.report(1, 1500, false);
        // 留痕经 strike_count 体现（内容口径由 StrikeRec 字段钉死）。
        let ok = g.strike_count() == 1;
        chk(&mut set, "C70-10 越限留痕", ok, "越限未留痕");
    }

    // C70-11 未越限回落连续计数（1 次越限 + 2 次守规 < 3 不晋升）
    {
        let mut g = CallbackBudget::new(8);
        let _ = g.register(1, 100, 1000);
        let _ = g.report(1, 1500, false);
        for _ in 0..2 {
            let _ = g.report(1, 500, false);
        }
        let _ = g.report(1, 1500, false);
        for _ in 0..2 {
            let _ = g.report(1, 500, false);
        }
        let ok = g.cadence_of(1) == Some(Cadence::EveryFrame);
        chk(&mut set, "C70-11 连续计数", ok, "非连续越限误晋升");
    }

    // C70-12 注册表全量重建
    {
        let mut g = CallbackBudget::new(8);
        let _ = g.register(1, 100, 1000);
        let _ = g.register(2, 100, 1000);
        g.rebuild_registry();
        let ok = g.is_empty() && g.strike_count() == 0;
        chk(&mut set, "C70-12 全量重建", ok, "重建未清账");
    }

    // C70-13 重复注册拒绝
    {
        let mut g = CallbackBudget::new(8);
        let _ = g.register(1, 100, 1000);
        let ok = g.register(1, 100, 1000) == Err(XC_NO_CB);
        chk(&mut set, "C70-13 重复注册", ok, "重复注册被放行");
    }

    // C70-14 确定性
    {
        let seq = || -> u32 {
            let mut g = CallbackBudget::new(8);
            let _ = g.register(1, 100, 1000);
            for _ in 0..3 {
                let _ = g.report(1, 1500, false);
            }
            match g.cadence_of(1) {
                Some(c) => c as u32,
                None => 0,
            }
        };
        chk(&mut set, "C70-14 确定性", seq() == seq() && seq() == 2, "两次不一致");
    }

    // C70-15 读屏行五要素
    {
        let g = CallbackBudget::new(8);
        let line = g.status_line();
        let ok = line.contains("回调") && line.contains("每帧") && line.contains("隔帧")
            && line.contains("停用") && line.contains("越限记录");
        chk(&mut set, "C70-15 读屏要素", ok, "读屏行缺要素");
    }

    set
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea70_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}
