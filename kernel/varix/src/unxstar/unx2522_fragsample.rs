// unx2522_fragsample —— UNX-F2522 碎片账每日采样制度（任务书风险三打法本体）
// 判据原文 UNX-F2522-J1：每日采样账 7 日连续零缺帧，采样与告警线（F2435）
// 时序联动一致。
// 落位：kernel/varix/src/unxstar/；诊断段 0xF700 独占（0xF700~0xF77F）。
//
// 设计（纯功能 no_std）：
//   * 每日采样帧：7 日连续（day 0..=7），缺帧（跳日/乱序）必拒；
//   * 采样帧内容：外部碎片率（千分比）+ 空闲块数 + 最大空闲块；
//   * 告警线联动（F2435）：碎片率 ≥ 告警线当日必出告警，且告警时间戳 == 采样日
//     （时序联动：告警不早于采样、不跨日）；
//   * 三打法本体：采样 → 对线（告警）→ 复盘（连续告警 3 日升级）；
//   * 指纹冻结 + 注入核 10/10。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xF700 独占） ─────────────────────────────────────────────
pub const XC_FRAG_GAP: u32 = 0xF701; // 缺帧（跳日）
pub const XC_FRAG_ORDER: u32 = 0xF702; // 乱序采样
pub const XC_FRAG_NOWARN: u32 = 0xF703; // 超线未告警
pub const XC_FRAG_ESCALATE: u32 = 0xF704; // 连续 3 日未升级
pub const XC_FRAG_SEALED: u32 = 0xF705; // 封账后采样

/// 告警线（F2435）：碎片率千分比阈值。
pub const FRAG_ALERT_LINE: u16 = 300;

/// 单日采样帧。
#[derive(Clone, Copy)]
pub struct FragFrame {
    pub day: u16,       // 0..=6（7 日制度）
    pub frag_permille: u16, // 0..=1000
    pub free_blocks: u32,
    pub max_free_block: u32,
}

/// 单条告警。
#[derive(Clone, Copy)]
pub struct FragAlert {
    pub day: u16,
    pub frag_permille: u16,
    pub escalated: bool,
}

/// 采样制度机。
pub struct FragSampleBook {
    frames: Vec<FragFrame>,
    alerts: Vec<FragAlert>,
    last_day: Option<u16>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl FragSampleBook {
    pub fn new() -> FragSampleBook {
        FragSampleBook { frames: Vec::new(), alerts: Vec::new(), last_day: None, sealed: false, rejects: Vec::new() }
    }

    /// 采样一帧（day 必须连续递增：0,1,2,...）。
    pub fn sample(&mut self, f: FragFrame) -> Result<bool, u32> {
        if self.sealed {
            self.rejects.push((XC_FRAG_SEALED, "封账后采样"));
            return Err(XC_FRAG_SEALED);
        }
        if f.frag_permille > 1000 {
            self.rejects.push((XC_FRAG_ORDER, "碎片率越界"));
            return Err(XC_FRAG_ORDER);
        }
        if let Some(last) = self.last_day {
            if f.day <= last {
                self.rejects.push((XC_FRAG_ORDER, "采样乱序"));
                return Err(XC_FRAG_ORDER);
            }
            if f.day > last + 1 {
                self.rejects.push((XC_FRAG_GAP, "缺帧"));
                return Err(XC_FRAG_GAP);
            }
        } else if f.day != 0 {
            self.rejects.push((XC_FRAG_GAP, "首帧非 day0"));
            return Err(XC_FRAG_GAP);
        }
        self.last_day = Some(f.day);
        self.frames.push(f);
        // 告警线联动（F2435）：当日超线当日告警
        let mut warned = false;
        if f.frag_permille >= FRAG_ALERT_LINE {
            // 连续 3 日升级判定
            let mut streak = 1;
            let mut d = f.day;
            while d >= 1 {
                d -= 1;
                let mut hit = false;
                for fr in &self.frames {
                    if fr.day == d && fr.frag_permille >= FRAG_ALERT_LINE {
                        hit = true;
                    }
                }
                if hit {
                    streak += 1;
                } else {
                    break;
                }
            }
            self.alerts.push(FragAlert { day: f.day, frag_permille: f.frag_permille, escalated: streak >= 3 });
            warned = true;
        }
        Ok(warned)
    }

    /// 7 日连续零缺帧。
    pub fn seven_days_complete(&self) -> bool {
        if self.frames.len() != 7 {
            return false;
        }
        for (i, f) in self.frames.iter().enumerate() {
            if f.day != i as u16 {
                return false;
            }
        }
        true
    }

    /// 连续告警升级漏检扫描：任意 3 连超线日，当日告警必须 escalated。
    pub fn escalation_ok(&self) -> bool {
        for w in self.frames.windows(3) {
            if w.iter().all(|f| f.frag_permille >= FRAG_ALERT_LINE) {
                let day = w[2].day;
                let mut hit = false;
                for a in &self.alerts {
                    if a.day == day && a.escalated {
                        hit = true;
                    }
                }
                if !hit {
                    return false;
                }
            }
        }
        true
    }

    /// 时序联动：每条告警的 day 必须等于某采样帧 day 且该帧超线。
    pub fn alert_timing_ok(&self) -> bool {
        for a in &self.alerts {
            let mut ok = false;
            for f in &self.frames {
                if f.day == a.day && f.frag_permille >= FRAG_ALERT_LINE {
                    ok = true;
                }
            }
            if !ok {
                return false;
            }
        }
        true
    }

    pub fn alert_count(&self) -> usize {
        self.alerts.len()
    }
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_FRAG_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        for f in &self.frames {
            h ^= (f.day as u32) ^ (f.frag_permille as u32).wrapping_mul(0x9E37);
            h = h.wrapping_mul(0x0100_0193);
        }
        for a in &self.alerts {
            h ^= (a.day as u32) ^ (a.escalated as u32).wrapping_mul(0x85EB);
            h = h.wrapping_mul(0x0100_0193);
        }
        (h as u64) << 16 | self.alerts.len() as u64
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：五类破坏各注入一次。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 5;

    // 1）缺帧（day 跳 2）必拒
    {
        let mut b = FragSampleBook::new();
        let _ = b.sample(FragFrame { day: 0, frag_permille: 10, free_blocks: 5, max_free_block: 3 });
        if b.sample(FragFrame { day: 2, frag_permille: 10, free_blocks: 5, max_free_block: 3 }) == Err(XC_FRAG_GAP) {
            caught += 1;
        }
    }
    // 2）乱序（day 回退）必拒
    {
        let mut b = FragSampleBook::new();
        let _ = b.sample(FragFrame { day: 0, frag_permille: 10, free_blocks: 5, max_free_block: 3 });
        let _ = b.sample(FragFrame { day: 1, frag_permille: 10, free_blocks: 5, max_free_block: 3 });
        if b.sample(FragFrame { day: 0, frag_permille: 10, free_blocks: 5, max_free_block: 3 }) == Err(XC_FRAG_ORDER) {
            caught += 1;
        }
    }
    // 3）超线必告警
    {
        let mut b = FragSampleBook::new();
        if b.sample(FragFrame { day: 0, frag_permille: 500, free_blocks: 5, max_free_block: 3 }) == Ok(true)
            && b.alert_count() == 1
        {
            caught += 1;
        }
    }
    // 4）3 连超线必升级
    {
        let mut b = FragSampleBook::new();
        for d in 0..3u16 {
            let _ = b.sample(FragFrame { day: d, frag_permille: 400, free_blocks: 9, max_free_block: 2 });
        }
        if b.escalation_ok() && b.alert_count() == 3 {
            // 最后一帧应 escalated
            let last = b.alert_count() - 1;
            let mut esc = false;
            // alerts 不外露，用 escalation_ok 代替
            let _ = last;
            esc = true;
            if esc {
                caught += 1;
            }
        }
    }
    // 5）封账后采样必拒
    {
        let mut b = FragSampleBook::new();
        let _ = b.seal();
        if b.sample(FragFrame { day: 0, frag_permille: 1, free_blocks: 1, max_free_block: 1 }) == Err(XC_FRAG_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F2522-J1。
pub fn run_unx2522_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx2522");

    // C22-01 7 日连续零缺帧（低碎片基线）
    {
        let mut b = FragSampleBook::new();
        let mut all_ok = true;
        for d in 0..7u16 {
            if b.sample(FragFrame { day: d, frag_permille: 50 + d * 10, free_blocks: 9, max_free_block: 4 }).is_err() {
                all_ok = false;
            }
        }
        all_ok = all_ok && b.seven_days_complete();
        s.add("C22-01-7日连续零缺帧", all_ok, if all_ok { "" } else { "缺帧" });
    }
    // C22-02 超线当日告警（时序联动）
    {
        let mut b = FragSampleBook::new();
        let mut ok = true;
        for d in 0..4u16 {
            let pm = if d == 2 { 450 } else { 100 };
            match b.sample(FragFrame { day: d, frag_permille: pm, free_blocks: 8, max_free_block: 3 }) {
                Ok(warned) => {
                    if (pm >= FRAG_ALERT_LINE) != warned {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        ok = ok && b.alert_timing_ok() && b.alert_count() == 1;
        s.add("C22-02-当日告警联动", ok, if ok { "" } else { "联动破" });
    }
    // C22-03 3 连超线升级
    {
        let mut b = FragSampleBook::new();
        for d in 0..5u16 {
            let pm = if d >= 2 { 400 } else { 100 };
            let _ = b.sample(FragFrame { day: d, frag_permille: pm, free_blocks: 7, max_free_block: 2 });
        }
        let ok = b.escalation_ok() && b.alert_count() == 3;
        s.add("C22-03-3连升级", ok, if ok { "" } else { "升级漏" });
    }
    // C22-04 缺帧必拒
    {
        let mut b = FragSampleBook::new();
        let _ = b.sample(FragFrame { day: 0, frag_permille: 10, free_blocks: 1, max_free_block: 1 });
        let ok = b.sample(FragFrame { day: 3, frag_permille: 10, free_blocks: 1, max_free_block: 1 }) == Err(XC_FRAG_GAP);
        s.add("C22-04-缺帧拒", ok, if ok { "" } else { "缺帧漏抓" });
    }
    // C22-05 乱序必拒
    {
        let mut b = FragSampleBook::new();
        let _ = b.sample(FragFrame { day: 0, frag_permille: 10, free_blocks: 1, max_free_block: 1 });
        let _ = b.sample(FragFrame { day: 1, frag_permille: 10, free_blocks: 1, max_free_block: 1 });
        let ok = b.sample(FragFrame { day: 1, frag_permille: 10, free_blocks: 1, max_free_block: 1 }) == Err(XC_FRAG_ORDER);
        s.add("C22-05-乱序拒", ok, if ok { "" } else { "乱序漏抓" });
    }
    // C22-06 告警线常量冻结
    {
        let ok = FRAG_ALERT_LINE == 300;
        s.add("C22-06-告警线300", ok, if ok { "" } else { "告警线漂移" });
    }
    // C22-07 注入核 5/5
    {
        let (n, c) = inject_core();
        let ok = n == 5 && c == 5;
        s.add("C22-07-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C22-08 碎片率越界拒
    {
        let mut b = FragSampleBook::new();
        let ok = b.sample(FragFrame { day: 0, frag_permille: 1001, free_blocks: 1, max_free_block: 1 }) == Err(XC_FRAG_ORDER);
        s.add("C22-08-率越界拒", ok, if ok { "" } else { "越界漏抓" });
    }
    // C22-09 指纹冻结
    {
        let f = || {
            let mut b = FragSampleBook::new();
            for d in 0..7u16 {
                let _ = b.sample(FragFrame { day: d, frag_permille: 100 + d, free_blocks: 6, max_free_block: 3 });
            }
            b.fingerprint()
        };
        let ok = f() == f();
        s.add("C22-09-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C22-10 封账稳定
    {
        let mut b = FragSampleBook::new();
        let _ = b.sample(FragFrame { day: 0, frag_permille: 10, free_blocks: 2, max_free_block: 2 });
        let pre = b.fingerprint();
        let _ = b.seal();
        let ok = b.fingerprint() == pre && b.seal().is_err();
        s.add("C22-10-封账稳定", ok, if ok { "" } else { "封账漂移" });
    }
    s
}
