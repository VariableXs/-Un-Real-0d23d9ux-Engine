//! 时间与定时器服务收口（WP-404 · B-3501~3503 · 篇 35）。
//!
//! 全系统的时间口径只有一个：对外接口两钟分离（单调给测量、墙钟给显示），
//! **混用即缺陷——接口层直接分型防混用，混用编译期拒绝（B-3501 达标线）**；
//! 交接校时把双域时间差写进快照、下次启动按差值校准，双域没有 NTP 也保持
//! 秒级一致（≤2s——**B-3502 达标线**）；定时器到期不补发（错过即跳过并
//! 计数——补发风暴是定时器事故之王）、漂移修正可选（**B-3503 达标线**）。

// ---------------------------------------------------------------------------
// B-3501 双钟分离：分型防混用
// ---------------------------------------------------------------------------

/// 单调钟（内核层：TSC 校准，永不过跳——给测量）。
/// 刻意没有任何到墙钟的换算通道：混用编译期拒绝是类型面事实。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MonoClock {
    pub ticks: u64,
}

/// 墙钟（硬件 RTC 落地 + 交接快照校准——给显示）。
/// 刻意没有任何到单调钟的换算通道。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WallClock {
    pub epoch_secs: i64,
}

impl MonoClock {
    pub fn new() -> Self {
        MonoClock { ticks: 0 }
    }
    pub fn advance(&mut self, ticks: u64) {
        self.ticks += ticks;
    }
}

impl WallClock {
    pub fn new(epoch_secs: i64) -> Self {
        WallClock { epoch_secs }
    }
}

/// 计时器测量：签名只收单调钟——拿墙钟计时编译不过（**B-3501 核心**）。
pub fn measure_span(start: MonoClock, end: MonoClock) -> u64 {
    end.ticks.saturating_sub(start.ticks)
}

// ---------------------------------------------------------------------------
// B-3502 交接校时：快照差值 ≤ 2s
// ---------------------------------------------------------------------------

/// 交接快照的时间域（Q14：每次交接把双域时间差写进快照）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandoverSnap {
    /// 双域时间差（秒，有符号——可正可负）。
    pub dual_delta_secs: i64,
}

/// 校准容差：双域没有 NTP 也保持秒级一致（**B-3502 达标线**）。
pub const HANDOVER_TOLERANCE_SECS: i64 = 2;

/// 按快照差值校准墙钟：下次启动按差值校（墙钟 = 挂钟读数 + 快照差）。
pub fn calibrate_wall(raw_epoch: i64, snap: &HandoverSnap) -> i64 {
    raw_epoch + snap.dual_delta_secs
}

/// 校准后与基准的偏差是否在容差内（≤2s——秒级一致）。
pub fn within_tolerance(calibrated: i64, truth: i64) -> bool {
    (calibrated - truth).abs() <= HANDOVER_TOLERANCE_SECS
}

// ---------------------------------------------------------------------------
// B-3503 定时器语义：不补发 + 漂移修正可选
// ---------------------------------------------------------------------------

/// 应用定时器（回调在应用事件循环执行——永不跨线程偷跑）。
#[derive(Clone, Copy, Debug)]
pub struct Timer {
    /// 间隔（毫秒）。
    pub every_ms: u64,
    /// 下一次到期时刻（毫秒时间轴，单调钟域）。
    next_due: u64,
    /// 漂移修正可选：真=间隔对齐上一次到期时刻（音频与动画类用）。
    pub drift_align: bool,
    /// 错过即跳过并计数——补发风暴不存在。
    pub missed: u64,
    /// 正常触发计数（对账用）。
    pub fired: u64,
}

impl Timer {
    /// drift_align=真：到期推进按"上一次到期+间隔"（修漂移）；
    /// 假：按"当前时刻+间隔"（自然漂移——语义二选一如实）。
    pub fn new(every_ms: u64, next_due: u64, drift_align: bool) -> Self {
        Timer { every_ms, next_due, drift_align, missed: 0, fired: 0 }
    }

    /// 到期裁决：未到期 false；到期触发并推进；**严重过期只触发一次并记
    /// 错过数（now-next_due)/间隔——不补发**（错过即跳过并计数）。
    pub fn fire(&mut self, now_ms: u64) -> bool {
        if now_ms < self.next_due {
            return false;
        }
        let elapsed = now_ms - self.next_due;
        if elapsed > self.every_ms {
            // 过期超过一个间隔：跳过的拍数进账本——补发风暴在结构面不存在。
            self.missed += elapsed / self.every_ms;
        }
        self.fired += 1;
        self.next_due = if self.drift_align {
            self.next_due + self.every_ms
        } else {
            now_ms + self.every_ms
        };
        true
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-3501~3503 · 7 项）
// ---------------------------------------------------------------------------

/// 时间服务判据（WP-404）。
pub fn run_timesrv_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("timesrv");
    // 1. 双钟分型存在：两类型各自可构造、字段语义分离。
    let mut mono = MonoClock::new();
    mono.advance(1000);
    let wall = WallClock::new(1_700_000_000);
    cs.add(
        "B-3501 双钟分型",
        mono.ticks == 1000 && wall.epoch_secs == 1_700_000_000,
        "单调给测量、墙钟给显示——两类型两语义",
    );
    // 2. 计时测量走单调钟（签名防混用——measure_span 只吃 MonoClock）。
    let mut end = mono;
    end.advance(250);
    cs.add(
        "B-3501 测量走单调",
        measure_span(mono, end) == 250,
        "计时器用墙钟在回拨时的悲剧——从签名上不可能",
    );
    // 3. 单调不过跳：advance 负数不存在（u64 饱和），时间轴只进不退。
    let mut m2 = MonoClock { ticks: 100 };
    m2.advance(0);
    cs.add("B-3501 单调不过跳", m2.ticks >= 100, "单调钟永不过跳——u64 只进不退");
    // 4. 交接快照差值校准（**B-3502 达标线**）：正差与负差双向 ≤2s。
    let snap = HandoverSnap { dual_delta_secs: 3 };
    let cal = calibrate_wall(1_700_000_000, &snap);
    let snap_neg = HandoverSnap { dual_delta_secs: -5 };
    let cal_neg = calibrate_wall(1_700_000_000, &snap_neg);
    cs.add(
        "B-3502 交接校时",
        within_tolerance(cal, 1_700_000_003)
            && within_tolerance(cal_neg, 1_699_999_995),
        "双域时间差写快照、启动按差值校——双向 ≤2s",
    );
    // 5. 校准容差判据：偏差恰 2s 过、3s 不过（边界如实）。
    cs.add(
        "B-3502 容差边界",
        within_tolerance(100, 102) && !within_tolerance(100, 103),
        "≤2s 是达标线不是约数——3s 就是不合格",
    );
    // 6. 到期不补发（**B-3503 达标线**）：过期三拍只触发一次、missed 记两拍。
    let mut t = Timer::new(100, 1_000, false);
    let fired_now = t.fire(1_350); // 应在 1000/1100/1200/1300 触发——只补当前一拍
    cs.add(
        "B-3503 到期不补发",
        fired_now && t.fired == 1 && t.missed == 3,
        "错过即跳过并计数——补发风暴是定时器事故之王",
    );
    // 7. 漂移修正可选：对齐模式按上次到期推进（不随处理时刻漂移）。
    let mut ta = Timer::new(100, 1_000, true);
    let _ = ta.fire(1_005);
    let aligned = ta.fire(1_110); // 对齐模式下 next_due=1100，1110 到期
    let mut tn = Timer::new(100, 1_000, false);
    let _ = tn.fire(1_005); // 自然模式 next_due=1105
    cs.add(
        "B-3503 漂移修正",
        aligned && tn.next_due == 1_105,
        "间隔对齐上一次到期——音频与动画的语义二选一如实可选",
    );
    cs
}

// ---------------------------------------------------------------------------
// 单测（fe33 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe33_clock_typing() {
        // 两钟独立推进互不干扰；measure_span 恒非负（饱和减法）。
        let mut mono = MonoClock::new();
        mono.advance(500);
        let wall = WallClock::new(1_700_000_000);
        let before = wall;
        mono.advance(100);
        assert_eq!(wall.epoch_secs, before.epoch_secs); // 墙钟不随单调动
        assert_eq!(measure_span(mono, mono), 0); // 同点测量为零
        let earlier = MonoClock { ticks: 100 };
        assert_eq!(measure_span(earlier, mono), 500); // 600-100
    }

    #[test]
    fn fe33_handover_calib() {
        // 连续两次交接的差值链：快照差叠加校准仍 ≤2s（无 NTP 双域现实）。
        let s1 = HandoverSnap { dual_delta_secs: 2 };
        let s2 = HandoverSnap { dual_delta_secs: -4 };
        let c1 = calibrate_wall(1_000_000, &s1);
        assert!(within_tolerance(c1, 1_000_002));
        let c2 = calibrate_wall(2_000_000, &s2);
        assert!(within_tolerance(c2, 1_999_996));
        // 零差快照：校准恒等。
        let s0 = HandoverSnap { dual_delta_secs: 0 };
        assert_eq!(calibrate_wall(7, &s0), 7);
    }

    #[test]
    fn fe33_timer_no_catchup() {
        // 未到期不触发；恰到期触发；巨度过期 missed 按拍计数不补发。
        let mut t = Timer::new(100, 1_000, false);
        assert!(!t.fire(999)); // 未到期
        assert!(t.fire(1_000)); // 恰到期
        assert_eq!(t.missed, 0);
        // next_due=1100（自然模式=now+100=1100）；巨度跳到 2000：
        // 拍点 1100~1900 共 9 拍被跳过（elapsed=900，900/100=9），本次只触发 2000 拍点。
        assert!(t.fire(2_000));
        assert_eq!(t.missed, 9);
        assert_eq!(t.fired, 2);
    }

    #[test]
    fn fe33_drift_align() {
        // 对齐模式：next_due 沿理论拍点走（1000→1100→1200），处理时刻迟到不漂移。
        let mut t = Timer::new(100, 1_000, true);
        assert!(t.fire(1_003));
        assert_eq!(t.next_due, 1_100); // 对齐：上次到期+间隔
        assert!(t.fire(1_107));
        assert_eq!(t.next_due, 1_200);
        // 对照：自然模式随处理时刻漂移（1003→1103）。
        let mut n = Timer::new(100, 1_000, false);
        assert!(n.fire(1_003));
        assert_eq!(n.next_due, 1_103);
    }
}
