//! VE-F0067 · 呈现表面尺寸协商（VE-A 域 · A03b 同步与呈现组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0067`
//!
//! **职责定位（锚点原文）**：surface 能力三态（当前、最小、最大 extent）查询
//! 与 clamp 策略：请求尺寸越界时钳制到合法域并告知实际值；窗口尺寸变化
//! 联动交换链重建判定（变化超阈值才重建防抖动）。
//!
//! **数据结构（锚点原文）**：能力三态结构；clamp 记录；重建判定阈值。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 能力查询失败 → 按窗口客户区保守估计并标注（估计值必须带「估计」标注）；
//! - clamp 后仍非法 → 拒绝三要素（发生了什么/为什么/下一步）；
//! - 重建风暴 → 去抖窗口合并（窗口内的多次变化合并为一次重建判定）。
//!
//! **性能逐项分解（锚点原文）**：查询 O(1)；clamp O(1)；重建判定 O(1)
//! 阈值比较。
//!
//! **跨批对接点（锚点原文）**：上游窗口系统冻结接口；下游 F0068 格式协商、
//! F0071 失败恢复。
//!
//! **无障碍与隐私（锚点原文）**：无直接无障碍面；实际呈现尺寸与请求不一致
//! 时告知。
//!
//! **判据（锚点原文）**：三态查询、clamp 策略、去抖重建、实际值告知、判据。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数；确定性。
//!
//! 诊断段：0xE89D 基址（全域核查无占用）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const EXTENT_VERSION: &str = "vea67_extent-v1";

/// 诊断段基址。
pub const SEG_BASE: u32 = 0xE89D;

// ---- 诊断码 ----
pub const XC_QUERY_FAIL: u32 = SEG_BASE + 1; // 能力查询失败（保守估计路径）
pub const XC_CLAMP_DEAD: u32 = SEG_BASE + 2; // clamp 后仍非法（三要素拒绝）
pub const XC_PARAM: u32 = SEG_BASE + 3; // 请求参数非法
pub const XC_THROTTLE: u32 = SEG_BASE + 4; // 去抖窗口内吞掉的变化

/// 尺寸。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extent {
    pub width: u32,
    pub height: u32,
}

impl Extent {
    pub fn valid(&self) -> bool {
        self.width > 0 && self.height > 0 && self.width <= 16384 && self.height <= 16384
    }
    /// 面积（防溢出：u64 中转）。
    pub fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }
    /// 与另一尺寸的每边差值绝对值之和（变化量）。
    pub fn delta(&self, o: &Extent) -> u64 {
        (self.width as i64 - o.width as i64).unsigned_abs()
            + (self.height as i64 - o.height as i64).unsigned_abs()
    }
}

/// 能力三态（当前/最小/最大 extent）。
#[derive(Clone, Copy, Debug)]
pub struct SurfaceCaps {
    pub current: Extent,
    pub min: Extent,
    pub max: Extent,
    /// true = 查询自真实 surface；false = 查询失败后按客户区保守估计。
    pub queried: bool,
}

impl SurfaceCaps {
    /// 三态自洽：min ≤ current ≤ max（否则能力本身非法）。
    pub fn consistent(&self) -> bool {
        self.min.width <= self.current.width
            && self.current.width <= self.max.width
            && self.min.height <= self.current.height
            && self.current.height <= self.max.height
    }
    /// 保守估计构造（查询失败降级）：以客户区为中心 ±25% 夹逼。
    /// 标注 queried=false（估计值必须带「估计」标注——锚点原文）。
    pub fn fallback(client: Extent) -> SurfaceCaps {
        let lo = |v: u32| (v / 4).max(1);
        let hi = |v: u32| v.saturating_mul(5) / 4 + 1;
        SurfaceCaps {
            current: client,
            min: Extent { width: lo(client.width), height: lo(client.height) },
            max: Extent { width: hi(client.width), height: hi(client.height) },
            queried: false,
        }
    }
}

/// clamp 结果——实际值告知的载体。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClampVerdict {
    /// 实际可用的尺寸（可能 ≠ 请求）。
    pub actual: Extent,
    /// 是否发生钳制。
    pub clamped: bool,
    /// 请求是否连 clamp 后都不可用（三要素拒绝）。
    pub rejected: bool,
    /// 依据的能力来源（真实查询 / 保守估计）。
    pub from_estimate: bool,
}

/// 尺寸协商器。
pub struct ExtentNegotiator {
    caps: SurfaceCaps,
    last_code: u32,
    /// clamp 记录（环形留痕，最近 16 条）。
    clamp_log: Vec<(Extent, Extent)>,
    /// 重建判定阈值（每边变化超过该值才重建——防抖核心）。
    rebuild_threshold: u64,
    /// 去抖窗口（同一窗口内的变化合并；单位：协商请求次数）。
    debounce_window: usize,
    /// 窗口内累计的变化量（合并用）。
    pending_delta: u64,
    window_used: usize,
    /// 去抖合并掉的重建判定次数。
    merged_rebuilds: u32,
}

impl ExtentNegotiator {
    pub fn new(caps: SurfaceCaps, rebuild_threshold: u64, debounce_window: usize) -> ExtentNegotiator {
        ExtentNegotiator {
            caps,
            last_code: 0,
            clamp_log: Vec::new(),
            rebuild_threshold,
            debounce_window,
            pending_delta: 0,
            window_used: 0,
            merged_rebuilds: 0,
        }
    }

    /// 三态查询。
    pub fn caps(&self) -> &SurfaceCaps {
        &self.caps
    }

    /// 请求尺寸协商（clamp 策略核心）：
    /// 越界请求钳到合法域；估计能力来源如实标注；零/超界死区拒绝。
    pub fn request(&mut self, want: Extent) -> Result<ClampVerdict, u32> {
        if want.width == 0 || want.height == 0 {
            self.last_code = XC_PARAM;
            return Err(self.last_code);
        }
        let from_estimate = !self.caps.queried;
        let mut clamped = false;
        let mut w = want.width;
        let mut h = want.height;
        if w < self.caps.min.width {
            w = self.caps.min.width;
            clamped = true;
        }
        if h < self.caps.min.height {
            h = self.caps.min.height;
            clamped = true;
        }
        if w > self.caps.max.width {
            w = self.caps.max.width;
            clamped = true;
        }
        if h > self.caps.max.height {
            h = self.caps.max.height;
            clamped = true;
        }
        let actual = Extent { width: w, height: h };
        if !actual.valid() {
            // clamp 后仍非法：三要素拒绝（由 failure_notice 输出）。
            self.last_code = XC_CLAMP_DEAD;
            return Err(self.last_code);
        }
        if clamped {
            // clamp 记录环形留痕（16 条）。
            if self.clamp_log.len() >= 16 {
                self.clamp_log.remove(0);
            }
            self.clamp_log.push((want, actual));
        }
        Ok(ClampVerdict { actual, clamped, rejected: false, from_estimate })
    }

    /// 窗口尺寸变化联动重建判定（去抖核心）：变化量入窗口累计；
    /// 窗口满或累计超阈值才判重建；窗口内的小变化合并计数。
    pub fn on_resize(&mut self, old: Extent, new: Extent) -> Result<bool, u32> {
        if !old.valid() || !new.valid() {
            self.last_code = XC_PARAM;
            return Err(self.last_code);
        }
        let d = old.delta(&new);
        self.pending_delta += d;
        self.window_used += 1;
        if self.pending_delta >= self.rebuild_threshold {
            // 达阈值：立即判重建并清窗。
            self.pending_delta = 0;
            self.window_used = 0;
            return Ok(true);
        }
        if self.window_used >= self.debounce_window {
            // 窗口满仍未达阈值：合并掉（去抖），不重建。
            self.merged_rebuilds += 1;
            self.pending_delta = 0;
            self.window_used = 0;
            self.last_code = XC_THROTTLE;
            return Ok(false);
        }
        Ok(false)
    }

    /// clamp 后仍非法的三要素拒绝文案。
    pub fn failure_notice(&self) -> String {
        match self.last_code {
            XC_CLAMP_DEAD => alloc::format!(
                "尺寸协商失败（发生了什么：请求尺寸钳制后仍超出能力域；为什么：窗口能力三态不含任何合法落点；下一步：改用系统能力内的尺寸或先调整窗口客户区）"
            ),
            XC_PARAM => alloc::format!(
                "尺寸请求被拒（发生了什么：请求含零边；为什么：零尺寸不是合法表面；下一步：给出正的宽高）"
            ),
            _ => alloc::format!("协商正常"),
        }
    }

    pub fn clamp_log_len(&self) -> usize {
        self.clamp_log.len()
    }
    pub fn merged_rebuilds(&self) -> u32 {
        self.merged_rebuilds
    }
    pub fn last_code(&self) -> u32 {
        self.last_code
    }

    /// 读屏状态行。
    pub fn status_line(&self) -> String {
        let q = if self.caps.queried { "实测" } else { "估计" };
        alloc::format!(
            "EXTENT 当前{}x{}({}) 阈值{} 钳制记录{} 去抖合并{}",
            self.caps.current.width,
            self.caps.current.height,
            q,
            self.rebuild_threshold,
            self.clamp_log.len(),
            self.merged_rebuilds
        )
    }
}

// =====================================================================
// 自检（判据锚点：三态查询 / clamp 策略 / 去抖重建 / 实际值告知）
// =====================================================================

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, detail);
}

/// VE-F0067 域自检。
pub fn run_vea67_checks() -> CheckSet {
    let mut set = CheckSet::new("vea67");

    fn caps_real(cur: Extent) -> SurfaceCaps {
        SurfaceCaps {
            current: cur,
            min: Extent { width: 320, height: 240 },
            max: Extent { width: 3840, height: 2160 },
            queried: true,
        }
    }

    // C67-01 三态查询：三态自洽
    {
        let c = caps_real(Extent { width: 1920, height: 1080 });
        let ok = c.consistent() && c.queried;
        chk(&mut set, "C67-01 三态自洽", ok, "能力三态不自洽");
    }

    // C67-02 clamp 策略：超上限钳到 max 并告知实际值
    {
        let mut n = ExtentNegotiator::new(caps_real(Extent { width: 1920, height: 1080 }), 64, 4);
        let v = n.request(Extent { width: 7680, height: 4320 });
        let ok = matches!(v, Ok(cv) if cv.actual.width == 3840 && cv.actual.height == 2160
            && cv.clamped && !cv.from_estimate);
        chk(&mut set, "C67-02 上限钳制", ok, "超上限未钳制");
    }

    // C67-03 clamp 策略：低于下限钳到 min
    {
        let mut n = ExtentNegotiator::new(caps_real(Extent { width: 1920, height: 1080 }), 64, 4);
        let v = n.request(Extent { width: 10, height: 10 });
        let ok = matches!(v, Ok(cv) if cv.actual.width == 320 && cv.actual.height == 240 && cv.clamped);
        chk(&mut set, "C67-03 下限钳制", ok, "低于下限未钳制");
    }

    // C67-04 合法请求直通不钳
    {
        let mut n = ExtentNegotiator::new(caps_real(Extent { width: 1920, height: 1080 }), 64, 4);
        let v = n.request(Extent { width: 1280, height: 720 });
        let ok = matches!(v, Ok(cv) if !cv.clamped && cv.actual.width == 1280);
        chk(&mut set, "C67-04 直通", ok, "合法请求被误钳");
    }

    // C67-05 实际值告知：钳制记录留痕
    {
        let mut n = ExtentNegotiator::new(caps_real(Extent { width: 1920, height: 1080 }), 64, 4);
        let _ = n.request(Extent { width: 7680, height: 4320 });
        let ok = n.clamp_log_len() == 1;
        chk(&mut set, "C67-05 钳制留痕", ok, "钳制未留痕");
    }

    // C67-06 保守估计降级：查询失败时估计值带标注
    {
        let fb = SurfaceCaps::fallback(Extent { width: 1000, height: 500 });
        let ok = !fb.queried && fb.min.width == 250 && fb.max.width == 1251;
        chk(&mut set, "C67-06 估计标注", ok, "估计未标注/夹逼错");
    }

    // C67-07 估计来源的协商结果标 from_estimate
    {
        let mut n = ExtentNegotiator::new(SurfaceCaps::fallback(Extent { width: 1000, height: 500 }), 64, 4);
        let v = n.request(Extent { width: 2000, height: 500 });
        let ok = matches!(v, Ok(cv) if cv.from_estimate && cv.clamped);
        chk(&mut set, "C67-07 估计来源", ok, "估计来源未标注");
    }

    // C67-08 零尺寸请求拒绝
    {
        let mut n = ExtentNegotiator::new(caps_real(Extent { width: 1920, height: 1080 }), 64, 4);
        let ok = n.request(Extent { width: 0, height: 100 }) == Err(XC_PARAM);
        chk(&mut set, "C67-08 零尺寸拒", ok, "零尺寸被放行");
    }

    // C67-09 去抖重建：阈值内小变化不触发重建
    {
        let mut n = ExtentNegotiator::new(caps_real(Extent { width: 1920, height: 1080 }), 64, 8);
        let r1 = n.on_resize(
            Extent { width: 1000, height: 500 },
            Extent { width: 1010, height: 505 },
        );
        let ok = matches!(r1, Ok(false));
        chk(&mut set, "C67-09 小变化不重建", ok, "抖动触发重建");
    }

    // C67-10 阈值达成立即重建
    {
        let mut n = ExtentNegotiator::new(caps_real(Extent { width: 1920, height: 1080 }), 64, 8);
        let r = n.on_resize(
            Extent { width: 1000, height: 500 },
            Extent { width: 1600, height: 900 },
        );
        let ok = matches!(r, Ok(true));
        chk(&mut set, "C67-10 达阈重建", ok, "大变化未重建");
    }

    // C67-11 去抖合并：窗口内多次小变化合并为一次不重建
    {
        let mut n = ExtentNegotiator::new(caps_real(Extent { width: 1920, height: 1080 }), 100, 3);
        let a = n.on_resize(Extent { width: 1000, height: 500 }, Extent { width: 1010, height: 500 });
        let b = n.on_resize(Extent { width: 1010, height: 500 }, Extent { width: 1020, height: 500 });
        let c = n.on_resize(Extent { width: 1020, height: 500 }, Extent { width: 1030, height: 500 });
        let ok = matches!(a, Ok(false)) && matches!(b, Ok(false)) && matches!(c, Ok(false))
            && n.merged_rebuilds() == 1;
        chk(&mut set, "C67-11 去抖合并", ok, "窗口未合并");
    }

    // C67-12 clamp 死区三要素拒绝（能力三态本身非法时无可落点）
    {
        // 构造非法能力：min > max（clamp 后必仍非法）。
        let bad = SurfaceCaps {
            current: Extent { width: 100, height: 100 },
            min: Extent { width: 500, height: 500 },
            max: Extent { width: 400, height: 400 },
            queried: true,
        };
        let mut n = ExtentNegotiator::new(bad, 64, 4);
        let r = n.request(Extent { width: 100, height: 100 });
        let ok = r == Err(XC_CLAMP_DEAD) && n.failure_notice().contains("下一步");
        chk(&mut set, "C67-12 死区三要素", ok, "死区未三要素拒绝");
    }

    // C67-13 非法三态能力被 consistency 检出
    {
        let bad = SurfaceCaps {
            current: Extent { width: 100, height: 100 },
            min: Extent { width: 500, height: 500 },
            max: Extent { width: 400, height: 400 },
            queried: true,
        };
        let ok = !bad.consistent();
        chk(&mut set, "C67-13 非法能力", ok, "非法能力未检出");
    }

    // C67-14 确定性
    {
        let seq = || -> (u32, u32) {
            let mut n = ExtentNegotiator::new(caps_real(Extent { width: 1920, height: 1080 }), 64, 4);
            let v = n.request(Extent { width: 7680, height: 4320 });
            match v {
                Ok(cv) => (cv.actual.width, cv.actual.height),
                Err(_) => (0, 0),
            }
        };
        chk(&mut set, "C67-14 确定性", seq() == seq(), "两次不一致");
    }

    // C67-15 读屏行要素
    {
        let n = ExtentNegotiator::new(caps_real(Extent { width: 1920, height: 1080 }), 64, 4);
        let line = n.status_line();
        let ok = line.contains("当前") && line.contains("阈值") && line.contains("钳制记录")
            && line.contains("去抖合并");
        chk(&mut set, "C67-15 读屏要素", ok, "读屏行缺要素");
    }

    set
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea67_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}
